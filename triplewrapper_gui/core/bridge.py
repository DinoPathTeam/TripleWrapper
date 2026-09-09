"""Bridge between GUI and Rust core."""
from __future__ import annotations

import json
import threading
from pathlib import Path

import gi

gi.require_version("GLib", "2.0")
gi.require_version("Gio", "2.0")

from gi.repository import GLib, Gio, GObject

from .models import AnalysisReport, ProgressTick


class CoreBridge(GObject.Object):
    """Async bridge to triplewrapper-core binary."""

    __gsignals__ = {
        "analysis-ready": (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        "analysis-failed": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
        "progress-tick": (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        "operation-failed": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
    }

    def __init__(self) -> None:
        super().__init__()
        self._archive_path: str | None = None
        self._proc: Gio.Subprocess | None = None
        self._reader_thread: threading.Thread | None = None
        self._cancelled = False

    # ----------------------------------------------------------------- API
    def set_archive_path(self, path: str) -> None:
        self._archive_path = path

    def analyze(self) -> None:
        if not self._archive_path:
            self.emit("analysis-failed", "No hay archivo seleccionado")
            return
        threading.Thread(target=self._run_analysis, daemon=True).start()

    def start_operation(self, workspace: str | None) -> None:
        if not self._archive_path or not workspace:
            self.emit("operation-failed", "Faltan parámetros")
            return
        self._cancelled = False
        threading.Thread(
            target=self._run_operation, args=(workspace,), daemon=True
        ).start()

    def cancel(self) -> None:
        self._cancelled = True
        if self._proc is not None:
            self._proc.force_exit()

    def reset(self) -> None:
        self._archive_path = None
        self._proc = None

    # -------------------------------------------------------------- Workers
    def _run_analysis(self) -> None:
        # TODO: replace with real Rust binary call:
        #   self._spawn(["triplewrapper-core", "analyze", self._archive_path])
        try:
            report = self._mock_analysis()
            GLib.idle_add(self.emit, "analysis-ready", report)
        except Exception as exc:  # noqa: BLE001
            GLib.idle_add(self.emit, "analysis-failed", str(exc))

    def _run_operation(self, workspace: str) -> None:
        # TODO: replace with real Rust binary call:
        #   self._spawn(["triplewrapper-core", "run", self._archive_path, workspace])
        try:
            for i in range(100):
                if self._cancelled:
                    return
                tick = ProgressTick(
                    stage="Comprimiendo",
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

    # ----------------------------------------------------------- Mock helpers
    def _mock_analysis(self) -> AnalysisReport:
        return AnalysisReport(
            archive_path=self._archive_path or "",
            archive_bytes=2 * 1024**3,
            needed_bytes=4 * 1024**3,
            free_bytes=3 * 1024**3,
            external_free_bytes=60 * 1024**3,
            suggested_workspace="/mnt/usb-ssd/.triplewrapper-cache",
            status="external",
            status_label="Se requiere unidad externa",
            blake3_expected="0" * 64,
        )

    # --------------------------------------------------------- Subprocess API
    def _spawn(self, argv: list[str]) -> None:
        """Spawn the Rust core and stream JSON events."""
        launcher = Gio.SubprocessLauncher()
        launcher.set_stdout_pipe(True)
        launcher.set_stderr_pipe(True)
        self._proc = launcher.spawnv(argv)
        stdout = self._proc.get_stdout_pipe()

        def reader() -> None:
            try:
                data_stream = Gio.DataInputStream.new(stdout)
                while True:
                    line, _ = data_stream.readline_utf8()
                    if line is None:
                        break
                    self._dispatch(json.loads(line))
            except Exception as exc:  # noqa: BLE001
                GLib.idle_add(self.emit, "operation-failed", str(exc))

        self._reader_thread = threading.Thread(target=reader, daemon=True)
        self._reader_thread.start()

    def _dispatch(self, payload: dict) -> None:
        kind = payload.get("kind")
        if kind == "analysis":
            report = AnalysisReport(**payload["data"])
            GLib.idle_add(self.emit, "analysis-ready", report)
        elif kind == "tick":
            tick = ProgressTick(**payload["data"])
            GLib.idle_add(self.emit, "progress-tick", tick)
        elif kind == "error":
            GLib.idle_add(self.emit, "operation-failed", payload["message"])