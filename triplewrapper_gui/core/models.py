"""Data models shared between GUI and Rust core."""
from __future__ import annotations

from dataclasses import dataclass


@dataclass
class AnalysisReport:
    """Result of the analyze step."""

    archive_path: str
    archive_bytes: int
    used_bytes: int = 0
    needed_bytes: int = 0        # current_size + estimated_final_size
    free_bytes: int = 0          # free space on source
    external_free_bytes: int | None = None
    suggested_workspace: str = ""
    status: str = "critical"              # "ok" | "external" | "critical"
    status_label: str = ""
    blake3_expected: str = ""
    encrypted: bool = False

    @property
    def human_archive(self) -> str:
        return _human(self.archive_bytes)

    @property
    def human_needed(self) -> str:
        return _human(self.needed_bytes)

    @property
    def human_free_source(self) -> str:
        return _human(self.free_bytes)


@dataclass
class ProgressTick:
    """Single telemetry sample emitted during operation."""

    stage: str
    progress: float          # 0.0 .. 1.0
    read_mbps: float
    write_mbps: float
    compress_mbps: float
    bytes_processed: int
    bytes_total: int


def _human(n: int) -> str:
    units = ["B", "KB", "MB", "GB", "TB"]
    value = float(n)
    for unit in units:
        if value < 1024:
            return f"{value:.1f} {unit}"
        value /= 1024
    return f"{value:.1f} PB"