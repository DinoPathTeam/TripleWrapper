"""Pure protocol helpers for GUI<->core communication (no GTK imports).

This module is intentionally free of ``gi`` dependencies so it can be
unit-tested on headless CI runners. All GTK/GObject code lives in
:mod:`triplewrapper_gui.core.bridge`.
"""
from __future__ import annotations

import json
import os
import shutil
from pathlib import Path

ALLOWED_SUFFIXES = (
    ".7z", ".zip", ".tar", ".gz", ".xz", ".zst", ".bz2",
    ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.zst", ".tar.bz2", ".tbz2",
)


def core_binary(explicit: str | None = None) -> str | None:
    """Resolve triplewrapper-core binary path."""
    if explicit and Path(explicit).exists():
        return explicit
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
        return False, f"Formato no soportado: {p.suffix or 'desconocido'}"
    return True, ""


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


def queue_items_from_raw(raw_items: list[dict]) -> list[dict]:
    """Normalize raw `queue list --json` items for the QueuePanel.

    Returns plain dicts with keys: id, operation, archive, priority,
    status, progress (0..100), current_file, error_message.
    """
    out: list[dict] = []
    for raw in raw_items:
        if not isinstance(raw, dict):
            continue
        req = raw.get("request", {}) or {}
        prog = raw.get("progress") or {}
        total = prog.get("bytes_total", 0) or 0
        done = prog.get("bytes_processed", 0) or 0
        pct = (done / total * 100) if total else 0.0
        try:
            item_id = int(raw.get("id", 0))
        except (TypeError, ValueError):
            continue
        out.append({
            "id": item_id,
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
