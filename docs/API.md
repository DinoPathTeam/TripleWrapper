# TripleWrapper Stable API (v1.0 target)

> **Status: STABLE as of v0.4.** The CLI + newline-delimited JSON protocol
> below is the supported integration surface (GUI, scripts, plugins).
> The legacy D-Bus service (`triplewrapper-core serve`) is **deprecated**
> and will be removed in v1.0.

All interaction is **local-only**: subprocesses on the user's machine.
No network, no accounts, no telemetry.

## Conventions

- Every command accepts a global `-j/--json` flag for machine output.
- Logs go to **stderr**; **stdout carries only JSON** when `--json` is set.
- Passwords travel via the `TRIPLEWRAPPER_PASSWORD` env var (preferred) or
  `--password` flag (warns: visible in process list). They are never logged
  and never persisted.
- Exit code `0` = success, non-zero = failure (message on stderr or JSON).

## Commands

| Command | Purpose | JSON output |
|---|---|---|
| `analyze -a ARCHIVE [--remove N --add N --ratio R]` | Dry-run space simulation (writes nothing) | `{"kind":"analysis","data": AnalysisReport}` |
| `list -a ARCHIVE` | Archive entries | `ArchiveMetadata` |
| `extract -a ARCHIVE -o DIR [--resume]` | Extract (resume skips present files) | `OperationStats` |
| `test -a ARCHIVE` | Integrity check | `{"ok": bool}` |
| `create -a OUT -f FILE... [--level N]` | Create/append archive | `OperationStats` |
| `delete -a ARCHIVE -f PATH...` | Delete entries (7z/zip) | `OperationStats` |
| `run --archive A --workspace W --operation test\|extract` | Streaming operation | `{"kind":"tick","data": ProgressTick}` lines, then exit code |
| `queue add/list/status/pause/...` | Batch queue (persisted locally) | varies |
| `integrity record/check/last -a ARCHIVE` | Local BLAKE3 log | record / check result |
| `devices`, `mount -d DEV`, `unmount -d DEV`, `watch` | External devices (UDisks2) | device list / mount point / event lines |
| `plugin list` | Installed format plugins | `[{"id","extensions","description"}]` |

## Event protocol (`run`, `watch`)

One compact JSON object per line on stdout:

```json
{"kind": "analysis", "data": {"archive_path": "...", "archive_bytes": 0,
  "used_bytes": 0, "needed_bytes": 0, "free_bytes": 0,
  "external_free_bytes": null, "suggested_workspace": "...",
  "status": "ok|external|critical", "status_label": "...",
  "blake3_expected": "", "encrypted": false}}
{"kind": "tick", "data": {"stage": "...", "progress": 0.0,
  "read_mbps": 0.0, "write_mbps": 0.0, "compress_mbps": 0.0,
  "bytes_processed": 0, "bytes_total": 0}}
{"kind": "error", "message": "..."}
{"kind": "device-added"} / {"kind": "device-removed"}
```

## Plugin convention (v0.4+)

A format plugin is **any executable** named `triplewrapper-<id>` found via
`TRIPLEWRAPPER_PLUGIN_DIR` or `PATH`. No SDK, no registration, no daemon.

Required subcommands (same shape as core):

- `list -a FILE [--password PW] [--json]` → `ArchiveMetadata` JSON
- `extract -a FILE -o DIR [--password PW]` → exit code (+ optional ticks)
- `test -a FILE [--password PW]` → exit code
- `plugin-info --json` (optional) → `{"id","extensions","description"}`;
  without it, the plugin handles the single extension matching its filename
  suffix (e.g. `triplewrapper-rar` handles `.rar`).

Rules for plugins:

1. Logs to stderr, JSON to stdout — same as core.
2. Never persist passwords; accept `TRIPLEWRAPPER_PASSWORD` too.
3. Exit non-zero with a human message on stderr on failure.
4. `add`/`delete` support is optional; core reports a clear error when
   the active plugin lacks them.

Discovery order for an unknown suffix: `TRIPLEWRAPPER_PLUGIN_DIR` first,
then `PATH` entries in order. Core formats always win on conflict.
