"""Bridge between GUI and Rust core (real JSON protocol, no silent mocks)."""
from __future__ import annotations

import json
import subprocess
import threading
from pathlib import Path

import gi

gi.require_version("GLib", "2.0")
gi.require_version("Gio", "2.0")

from gi.repository import Gio, GLib, GObject

from .models import AnalysisReport, ProgressTick
from .protocol import core_binary, parse_event_line, validate_archive_path


class CoreBridge(GObject.Object):
    """Async bridge to triplewrapper-core binary."""

    __gsignals__ = {  # noqa: RUF012 - GObject signal map must be a dict
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
        import contextlib

        self._cancelled = True
        if self._proc is not None:
            with contextlib.suppress(Exception):
                self._proc.force_exit()

    def reset(self) -> None:
        self._archive_path = None
        self._proc = None

    # -------------------------------------------------------------- Workers
    def _resolve_bin(self) -> str | None:
        return self._core_bin or core_binary()

    def _run_analysis(self) -> None:
        import os

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
            archive = self._archive_path
            assert archive is not None  # guarded by analyze()
            proc = subprocess.run(
                [binary, "analyze", "-a", archive, "--json"],
                capture_output=True, text=True, timeout=120, check=False,
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
        import os

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
        archive = self._archive_path
        assert archive is not None  # guarded by start_operation()
        self._spawn([binary, "run", "--archive", archive,
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
            capture_output=True, text=True, timeout=30, check=False,
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
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=30, check=False)
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
