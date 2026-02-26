#!/usr/bin/env bash
# Test: does a symlink to a venv python preserve venv detection?
# This replicates exactly what bulkers does with host_commands.
set -euo pipefail

TESTDIR=$(mktemp -d /tmp/bulker_venv_test_XXXXXX)
trap 'rm -rf "$TESTDIR"' EXIT

ORIG_PATH="$PATH"

check_python() {
    local label="$1"
    local python_bin="$2"
    echo "--- $label ---"
    echo "  invoking: $python_bin"
    if [[ -L "$python_bin" ]]; then
        echo "  symlink -> $(readlink "$python_bin")"
    fi
    "$python_bin" -c "
import sys
venv_active = sys.prefix != sys.base_prefix
print(f'  sys.prefix:      {sys.prefix}')
print(f'  sys.base_prefix: {sys.base_prefix}')
print(f'  sys.executable:  {sys.executable}')
print(f'  venv detected:   {venv_active}')
try:
    import pip_install_test
    print(f'  test package:    FOUND')
except ImportError:
    print(f'  test package:    NOT FOUND')
" 2>&1 || echo "  (python failed)"
    echo ""
}

########################################################################
echo "========================================"
echo "TEST 1: Activate venv FIRST, then bulker"
echo "========================================"
echo "(User activates venv, then bulker creates shimlinks)"
echo ""

VENV1="$TESTDIR/venv1"
python3 -m venv "$VENV1"
"$VENV1/bin/pip" install --quiet pip-install-test 2>/dev/null

# Baseline: direct venv python
check_python "1a) venv python directly" "$VENV1/bin/python3"

# Simulate venv active on PATH
export PATH="$VENV1/bin:$ORIG_PATH"
VENV_PYTHON=$(which python3)
echo "  which python3 = $VENV_PYTHON"
echo ""

# Simulate what bulker does: create shimdir, symlink python3 to what which found
SHIMDIR1="$TESTDIR/shimdir1"
mkdir -p "$SHIMDIR1"
ln -sf "$VENV_PYTHON" "$SHIMDIR1/python3"

# Now prepend shimdir to PATH (like bulker activate does)
export PATH="$SHIMDIR1:$VENV1/bin:$ORIG_PATH"
echo "  PATH order: shimdir -> venv/bin -> system"
echo ""

check_python "1b) via bulker-style symlink (shimdir/python3 -> venv/bin/python3)" "$SHIMDIR1/python3"

# What about 'which python3' resolution?
echo "  which python3 = $(which python3)"
check_python "1c) via 'which python3' with shimdir first on PATH" "$(which python3)"

export PATH="$ORIG_PATH"

########################################################################
echo "========================================"
echo "TEST 2: Activate bulker FIRST, then venv"
echo "========================================"
echo "(Bulker shimlinks point to system python, then user activates venv)"
echo ""

VENV2="$TESTDIR/venv2"
python3 -m venv "$VENV2"
"$VENV2/bin/pip" install --quiet pip-install-test 2>/dev/null

# Baseline
check_python "2a) venv python directly" "$VENV2/bin/python3"

# Simulate bulker activated first (before venv): which python3 -> system python
SYS_PYTHON=$(which python3)
SHIMDIR2="$TESTDIR/shimdir2"
mkdir -p "$SHIMDIR2"
ln -sf "$SYS_PYTHON" "$SHIMDIR2/python3"

echo "  system python: $SYS_PYTHON"
echo "  shimlink: $SHIMDIR2/python3 -> $SYS_PYTHON"
echo ""

# Now user activates venv AFTER bulker — venv/bin goes before shimdir
export PATH="$VENV2/bin:$SHIMDIR2:$ORIG_PATH"
echo "  PATH order: venv/bin -> shimdir -> system"
echo "  which python3 = $(which python3)"
echo ""

check_python "2b) venv/bin before shimdir (venv wins)" "$(which python3)"

# Opposite: shimdir before venv (bulker shadows venv)
export PATH="$SHIMDIR2:$VENV2/bin:$ORIG_PATH"
echo "  PATH order: shimdir -> venv/bin -> system"
echo "  which python3 = $(which python3)"
echo ""

check_python "2c) shimdir before venv (bulker shadows, points to system python)" "$(which python3)"

export PATH="$ORIG_PATH"

########################################################################
echo "========================================"
echo "RESULTS SUMMARY"
echo "========================================"
echo ""
echo "Key: if 'venv detected: True' and 'test package: FOUND', the venv works."
echo "     if 'venv detected: False' or 'test package: NOT FOUND', the venv is broken."
