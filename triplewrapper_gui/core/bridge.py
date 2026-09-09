"""Bridge between GUI and Rust core (real JSON protocol, no silent mocks)."""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
from pathlib import Path

import gi

gi.require_version("GLib", "2.0")
gi.require_version("Gio", "2.0")

from gi.repository import GLib, Gio, GObject

from .models import AnalysisReport, ProgressTick

ALLOWED_SUFFIXES = (
    ".7z", ".zip", ".tar", ".gz", ".xz", ".zst", ".bz2",
    ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.zst", ".tar.bz2", ".tbz2",
)


def core_binary() -> str | None:
    """Resolve triplewrapper-core binary path."""
    env = os.environ.get("TRIPLEWRAPPER_CORE_BIN")
    if env and Path(env).exists():
        return env
    found = shutil.which("triplewrapper-core")
    if found:
        return found
    # Dev fallback: repo release binary relative to this file.
    here = Path(__file__).resolve()
    for _ in range(6):
        here = here.parent
        cand = here / "src" / "core" / "target" / "release" / "triplewrapper-core"
        if cand.exists():
            return str(cand)
    return None


def validate_archive_path(path: str) -> tuple[bool, str]:
    """Pure validation: exists, readable file, allowed suffix."""
    if not path:
        return False, "No hay archivo seleccionado"
    p = Path(path)
    if not p.exists():
        return False, f"El archivo no existe: {path}"
    if not p.is_file():
        return False, f"No es un archivo: {path}"
    try:
        with p.open("rb"):
            pass
    except OSError:
        return False, f"Sin permiso de lectura: {path}"
    lower = path.lower()
    if not any(lower.endswith(s) for s in ALLOWED_SUFFIXES):
        return False, f"Formato no soportado: {Path(path).suffix or 'desconocido'}"
    return True, ""


def queue_items_from_raw(raw_items: list[dict]) -> list[dict]:
    """Normalize raw `queue list --json` items for the QueuePanel.

    Returns plain dicts with keys: id, operation, archive, priority,
    status, progress (0..100), current_file, error_message.
    Pure function (testable without GTK).
    """
    out: list[dict] = []
    for raw in raw_items:
        req = raw.get("request", {}) if isinstance(raw, dict) else {}
        prog = raw.get("progress") or {}
        total = prog.get("bytes_total", 0) or 0
        done = prog.get("bytes_processed", 0) or 0
        pct = (done / total * 100) if total else 0.0
        out.append({
            "id": int(raw.get("id", 0)),
            "operation": str(req.get("op_type", "extract")),
            "archive": str(req.get("archive_path", "")),
            "priority": str(raw.get("priority", "normal")),
            "status": str(raw.get("status", "pending")),
            "progress": round(min(max(pct, 0.0), 100.0), 1),
            "current_file": str(prog.get("current_file", "") or ""),
            "error_message": raw.get("error_message"),
        })
    # Pending/running first, then priority order preserved from backend.
    order = {"running": 0, "pending": 1, "paused": 2}
    out.sort(key=lambda i: order.get(i["status"], 3))
    return out


def parse_event_line(line: str) -> tuple[str, dict] | None:
    """Parse one JSON event line. Returns (kind, payload) or None.

    Accepted shapes:
      {"kind": "analysis", "data": {...}}
      {"kind": "tick", "data": {...}}
      {"kind": "error", "message": "..."}
    """
    line = line.strip()
    if not line.startswith("{"):
        return None
    try:
        payload = json.loads(line)
    except json.JSONDecodeError:
        return None
    kind = payload.get("kind")
    if kind in ("analysis", "tick", "error"):
        return kind, payload
    return None


class CoreBridge(GObject.Object):
    """Async bridge to triplewrapper-core binary."""

    __gsignals__ = {
        "analysis-ready": (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        "analysis-failed": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
        "progress-tick": (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        "operation-failed": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
    }

    def __init__(self, core_bin: str | None = None) -> None:
        super().__init__()
        self._archive_path: str | None = None
        self._proc: Gio.Subprocess | None = None
        self._reader_thread: threading.Thread | None = None
        self._cancelled = False
        self._core_bin = core_bin

    # ----------------------------------------------------------------- API
    def set_archive_path(self, path: str) -> None:
        ok, err = validate_archive_path(path)
        if not ok:
            self.emit("analysis-failed", err)
            return
        self._archive_path = path

    def analyze(self) -> None:
        if not self._archive_path:
            self.emit("analysis-failed", "No hay archivo seleccionado")
            return
        ok, err = validate_archive_path(self._archive_path)
        if not ok:
            self.emit("analysis-failed", err)
            return
        threading.Thread(target=self._run_analysis, daemon=True).start()

    def start_operation(self, workspace: str | None, operation: str = "extract") -> None:
        if not self._archive_path or not workspace:
            self.emit("operation-failed", "Faltan parámetros")
            return
        self._cancelled = False
        threading.Thread(
            target=self._run_operation, args=(workspace, operation), daemon=True
        ).start()

    def cancel(self) -> None:
        self._cancelled = True
        if self._proc is not None:
            try:
                self._proc.force_exit()
            except Exception:
                pass

    def reset(self) -> None:
        self._archive_path = None
        self._proc = None

    # -------------------------------------------------------------- Workers
    def _resolve_bin(self) -> str | None:
        return self._core_bin or core_binary()

    def _run_analysis(self) -> None:
        if os.environ.get("TRIPLEWRAPPER_GUI_MOCK") == "1":
            GLib.idle_add(self.emit, "analysis-ready", self._mock_analysis())
            return
        binary = self._resolve_bin()
        if not binary:
            GLib.idle_add(
                self.emit, "analysis-failed",
                "Binario triplewrapper-core no encontrado (TRIPLEWRAPPER_CORE_BIN)",
            )
            return
        try:
            proc = subprocess.run(
                [binary, "analyze", "-a", self._archive_path, "--json"],
                capture_output=True, text=True, timeout=120,
            )
            out = (proc.stdout or "").strip().splitlines()
            for line in reversed(out):
                parsed = parse_event_line(line)
                if parsed and parsed[0] == "analysis":
                    report = AnalysisReport(**parsed[1]["data"])
                    GLib.idle_add(self.emit, "analysis-ready", report)
                    return
            err = (proc.stderr or "").strip().splitlines()
            msg = err[-1] if err else "Sin salida del analizador"
            GLib.idle_add(self.emit, "analysis-failed", f"Analyze falló: {msg}")
        except Exception as exc:  # noqa: BLE001
            GLib.idle_add(self.emit, "analysis-failed", str(exc))

    def _run_operation(self, workspace: str, operation: str = "extract") -> None:
        if os.environ.get("TRIPLEWRAPPER_GUI_MOCK") == "1":
            self._run_mock_operation()
            return
        binary = self._resolve_bin()
        if not binary:
            GLib.idle_add(
                self.emit, "operation-failed",
                "Binario triplewrapper-core no encontrado (TRIPLEWRAPPER_CORE_BIN)",
            )
            return
        try:
            Path(workspace).mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            GLib.idle_add(self.emit, "operation-failed", f"No se puede crear workspace: {exc}")
            return
        self._spawn([binary, "run", "--archive", self._archive_path,
                     "--workspace", workspace, "--operation", operation,
                     "--output", workspace])

    def _run_mock_operation(self) -> None:
        try:
            for i in range(100):
                if self._cancelled:
                    return
                tick = ProgressTick(
                    stage="Comprimiendo (mock)",
                    progress=(i + 1) / 100,
                    read_mbps=42.0 + (i % 7),
                    write_mbps=31.0 + (i % 5),
                    compress_mbps=18.0 + (i % 3),
                    bytes_processed=(i + 1) * 1024 * 1024,
                    bytes_total=100 * 1024 * 1024,
                )
                GLib.idle_add(self.emit, "progress-tick", tick)
                import time
                time.sleep(0.05)
        except Exception as exc:  # noqa: BLE001
            GLib.idle_add(self.emit, "operation-failed", str(exc))

    # ----------------------------------------------------------- Mock helper
    def _mock_analysis(self) -> AnalysisReport:
        return AnalysisReport(
            archive_path=self._archive_path or "",
            archive_bytes=2 * 1024**3,
            used_bytes=0,
            needed_bytes=4 * 1024**3,
            free_bytes=3 * 1024**3,
            external_free_bytes=60 * 1024**3,
            suggested_workspace="/mnt/usb-ssd/.triplewrapper-cache",
            status="external",
            status_label="Se requiere unidad externa (mock)",
            blake3_expected="0" * 64,
        )

    # --------------------------------------------------------- Subprocess API
    def _spawn(self, argv: list[str]) -> None:
        """Spawn the Rust core and stream JSON events."""
        launcher = Gio.SubprocessLauncher()
        launcher.set_flags(Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE)
        self._proc = launcher.spawnv(argv)
        stdout = self._proc.get_stdout_pipe()

        def reader() -> None:
            try:
                data_stream = Gio.DataInputStream.new(stdout)
                while True:
                    if self._cancelled:
                        return
                    line, _ = data_stream.read_line_utf8()
                    if line is None:
                        break
                    parsed = parse_event_line(line)
                    if not parsed:
                        continue
                    self._dispatch(parsed[0], parsed[1])
            except Exception as exc:  # noqa: BLE001
                GLib.idle_add(self.emit, "operation-failed", str(exc))

        self._reader_thread = threading.Thread(target=reader, daemon=True)
        self._reader_thread.start()

    def _dispatch(self, kind: str, payload: dict) -> None:
        if kind == "analysis":
            report = AnalysisReport(**payload["data"])
            GLib.idle_add(self.emit, "analysis-ready", report)
        elif kind == "tick":
            tick = ProgressTick(**payload["data"])
            GLib.idle_add(self.emit, "progress-tick", tick)
        elif kind == "error":
            GLib.idle_add(self.emit, "operation-failed", payload.get("message", "Error desconocido"))

    # ------------------------------------------------------------- Queue API
    def queue_list(self) -> list[dict]:
        """Run `queue list --json` and return raw items (sync, call in thread)."""
        binary = self._resolve_bin()
        if not binary:
            raise FileNotFoundError("triplewrapper-core no encontrado")
        proc = subprocess.run(
            [binary, "queue", "list", "--status", "all", "--json"],
            capture_output=True, text=True, timeout=30,
        )
        if proc.returncode != 0:
            raise RuntimeError((proc.stderr or "queue list falló").strip())
        data = json.loads(proc.stdout or "[]")
        return data if isinstance(data, list) else []

    def queue_add(self, archive: str, operation: str = "extract", priority: str = "normal",
                  output: str | None = None) -> int:
        """Run `queue add` and return numeric id."""
        ok, err = validate_archive_path(archive)
        if not ok:
            raise ValueError(err)
        binary = self._resolve_bin()
        if not binary:
            raise FileNotFoundError("triplewrapper-core no encontrado")
        cmd = [binary, "queue", "add", "-a", archive, "-o", operation, "--priority", priority]
        if output:
            cmd += ["--output", output]
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if proc.returncode != 0:
            raise RuntimeError((proc.stderr or proc.stdout or "queue add falló").strip())
        try:
            return int(json.loads(proc.stdout).get("id", 0))
        except (json.JSONDecodeError, ValueError, AttributeError):
            # Fallback: parse "Operation queued with ID: N"
            import re
            m = re.search(r"ID:\s*(\d+)", proc.stdout or "")
            if m:
                return int(m.group(1))
            raise RuntimeError("No se pudo obtener el ID encolado")
