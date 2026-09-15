#!/usr/bin/env bash
# Local release gate: build + verify everything testable WITHOUT network,
# SDK or push, so only green trees reach the repo. Flatpak packaging stays
# in CI (blocking job). Exit 0 = safe to push.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

pass() { echo "  OK: $1"; }
fail() { echo "  FAIL: $1"; exit 1; }

echo "== Rust core =="
# shellcheck disable=SC1091
source "$HOME/.cargo/env" 2>/dev/null || true
(cd src/core && cargo fmt --all -- --check) > /dev/null || fail "cargo fmt"
# NOTE: never grep -q a live pipe under `set -o pipefail` here: the early
# exit closes the pipe, the producer dies with SIGPIPE (141) and the gate
# misfires. Always land output in a file first, then grep the file.
(cd src/core && cargo clippy --all-targets --all-features -- -D warnings > /tmp/tw-clippy.log 2>&1 && ! grep -qE "^(warning|error):" /tmp/tw-clippy.log) || fail "cargo clippy"
(cd src/core && cargo test --all-targets > /tmp/tw-test.log 2>&1 && grep -q "test result: ok" /tmp/tw-test.log) || fail "cargo test"
(cd src/core && cargo build -q --release) || fail "release build"
pass "rust core"

echo "== Python GUI =="
python3 -m ruff check triplewrapper_gui tests > /dev/null || fail "ruff"
python3 -m mypy triplewrapper_gui > /dev/null 2>&1 || fail "mypy"
python3 -m pytest tests/unit -q > /tmp/tw-pytest.log 2>&1 && grep -q "passed" /tmp/tw-pytest.log || fail "pytest"
python3 - <<'EOF' || fail "meson install guard"
import re
import pathlib
listed = set(re.findall(r"'(triplewrapper_gui/[^']+\.py)'", open('meson.build').read()))
actual = {str(p) for p in pathlib.Path('triplewrapper_gui').rglob('*.py')
          if '__pycache__' not in str(p)}
missing, extra = sorted(actual - listed), sorted(listed - actual)
assert not missing and not extra, f"out of sync: missing={missing} extra={extra}"
EOF
python3 -m compileall -q triplewrapper_gui || fail "compileall"
pass "python gui"

echo "== Smoke (native) =="
export TRIPLEWRAPPER_CORE_BIN="$REPO/src/core/target/release/triplewrapper-core"
"$TRIPLEWRAPPER_CORE_BIN" --help > /dev/null || fail "core --help"
rm -rf /tmp/tw-local && mkdir -p /tmp/tw-local/files
echo "local-release probe" > /tmp/tw-local/files/a.txt
python3 -c "import shutil; shutil.make_archive('/tmp/tw-local/t', 'zip', '/tmp/tw-local/files')"
"$TRIPLEWRAPPER_CORE_BIN" analyze -a /tmp/tw-local/t.zip --json \
    | python3 -c "import json,sys; d=json.loads(sys.stdin.read()); assert d['kind']=='analysis', d" \
    || fail "analyze protocol"
pass "smoke"

echo "== GUI imports =="
python3 -c "
import sys
sys.path.insert(0, '.')
import triplewrapper_gui.main
import triplewrapper_gui.main_window
import triplewrapper_gui.views.analysis_view
import triplewrapper_gui.views.progress_view
print('imports OK')" || fail "gui imports"
pass "gui imports"

echo
echo "LOCAL RELEASE: GREEN — safe to push"
