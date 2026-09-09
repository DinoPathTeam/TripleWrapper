"""Protocol tests for GUI<->core JSON events (no display needed)."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from triplewrapper_gui.core.bridge import (
    parse_event_line,
    queue_items_from_raw,
    validate_archive_path,
)


def test_parse_analysis_event():
    line = json.dumps({"kind": "analysis", "data": {"archive_path": "/tmp/a.zip"}})
    kind, payload = parse_event_line(line)
    assert kind == "analysis"
    assert payload["data"]["archive_path"] == "/tmp/a.zip"


def test_parse_tick_event():
    line = json.dumps({"kind": "tick", "data": {"progress": 0.5}})
    kind, payload = parse_event_line(line)
    assert kind == "tick"
    assert payload["data"]["progress"] == 0.5


def test_parse_error_event():
    line = json.dumps({"kind": "error", "message": "boom"})
    kind, payload = parse_event_line(line)
    assert kind == "error"


def test_parse_invalid_lines():
    assert parse_event_line("not json") is None
    assert parse_event_line('{"kind": "unknown"}') is None
    assert parse_event_line("") is None


def test_validate_archive_path(tmp_path):
    ok, _ = validate_archive_path("")
    assert not ok
    ok, _ = validate_archive_path("/no/existe.zip")
    assert not ok
    f = tmp_path / "a.txt"
    f.write_text("x")
    ok, _ = validate_archive_path(str(f))
    assert not ok  # .txt no soportado
    z = tmp_path / "a.zip"
    z.write_bytes(b"PK")
    ok, err = validate_archive_path(str(z))
    assert ok, err


def test_queue_items_from_raw_ordering():
    raw = [
        {"id": 1, "request": {"op_type": "extract", "archive_path": "/a.zip"},
         "priority": "low", "status": "pending", "progress": None, "error_message": None},
        {"id": 2, "request": {"op_type": "test", "archive_path": "/b.zip"},
         "priority": "high", "status": "running",
         "progress": {"bytes_processed": 50, "bytes_total": 100, "current_file": "f"},
         "error_message": None},
    ]
    items = queue_items_from_raw(raw)
    assert items[0]["id"] == 2  # running primero
    assert items[0]["progress"] == 50.0
    assert items[1]["id"] == 1
