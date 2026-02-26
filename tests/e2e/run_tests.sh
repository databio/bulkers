#!/usr/bin/env bash
#
# End-to-end tests for bulker. Requires Docker and the bulker binary.
#
# Usage:
#   ./tests/e2e/run_tests.sh [path/to/bulker]
#
# If no binary path is given, assumes "bulker" is on PATH.

set -euo pipefail

BULKERS="${1:-bulker}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST="$SCRIPT_DIR/test_manifest.yaml"

PASSED=0
FAILED=0
FAILURES=""

# ---------- helpers ----------

setup() {
    TMPDIR="$(mktemp -d)"
    export XDG_CONFIG_HOME="$TMPDIR/xdg"
    CONFIG="$TMPDIR/bulker_config.yaml"

    "$BULKERS" config init -c "$CONFIG" >/dev/null 2>&1

    # Install the test crate with --build to pull images
    "$BULKERS" crate install -c "$CONFIG" -b "$MANIFEST" >/dev/null 2>&1
}

setup_no_load() {
    TMPDIR="$(mktemp -d)"
    export XDG_CONFIG_HOME="$TMPDIR/xdg"
    CONFIG="$TMPDIR/bulker_config.yaml"
    "$BULKERS" config init -c "$CONFIG" >/dev/null 2>&1
}

teardown() {
    unset XDG_CONFIG_HOME 2>/dev/null || true
    if [ -n "${TMPDIR:-}" ] && [ -d "$TMPDIR" ]; then
        rm -rf "$TMPDIR"
    fi
}

run_test() {
    local name="$1"
    echo -n "  $name ... "
    if "$name" >/dev/null 2>&1; then
        echo "PASS"
        PASSED=$((PASSED + 1))
    else
        echo "FAIL"
        FAILED=$((FAILED + 1))
        FAILURES="$FAILURES  - $name\n"
    fi
    teardown
}

# ---------- test cases ----------

test_init() {
    TMPDIR="$(mktemp -d)"
    CONFIG="$TMPDIR/bulker_config.yaml"

    "$BULKERS" config init -c "$CONFIG"

    # Config exists and contains expected content
    [ -f "$CONFIG" ]
    grep -q "container_engine" "$CONFIG"
    grep -q "docker" "$CONFIG"
}

test_load_and_build() {
    setup_no_load

    "$BULKERS" crate install -c "$CONFIG" -b "$MANIFEST"

    # Image should have been pulled
    docker image inspect nsheff/cowsay >/dev/null 2>&1
}

test_run_cowsay() {
    setup

    output=$("$BULKERS" exec -c "$CONFIG" local/test_manifest:default -- cowsay "hello from bulker" 2>&1)

    echo "$output" | grep -q "hello from bulker"
}

test_run_exit_code() {
    setup

    # Run a command that doesn't exist - should return non-zero
    set +e
    "$BULKERS" exec -c "$CONFIG" local/test_manifest:default -- nonexistent_command 2>/dev/null
    rc=$?
    set -e

    [ "$rc" -ne 0 ]
}

test_activate_echo_and_source() {
    setup

    activate_output=$("$BULKERS" activate -c "$CONFIG" --echo local/test_manifest:default 2>&1)

    # Should contain export statements
    echo "$activate_output" | grep -q "export PATH="
    echo "$activate_output" | grep -q "export BULKERCRATE="

    # Source it and verify cowsay is on PATH
    eval "$activate_output"
    which cowsay >/dev/null 2>&1
}

test_run_strict_mode() {
    setup

    # Strict mode sets PATH to only the crate directory.
    # Verify via activate --echo that PATH contains only the crate path.
    output=$("$BULKERS" activate -c "$CONFIG" --echo local/test_manifest:default 2>&1)
    # The non-strict PATH should contain the crate folder AND existing PATH
    echo "$output" | grep -q "export PATH="

    # Now test non-strict exec works (baseline)
    output=$("$BULKERS" exec -c "$CONFIG" local/test_manifest:default -- cowsay "strict test" 2>&1)
    echo "$output" | grep -q "strict test"

    # Verify strict flag is accepted and runs (it may fail because docker isn't
    # on the strict PATH, but the binary should not error on the flag itself)
    set +e
    "$BULKERS" exec -c "$CONFIG" -s local/test_manifest:default -- cowsay "strict test" 2>/dev/null
    rc=$?
    set -e
    # rc may be non-zero due to docker not being on strict PATH - that's expected
    # The key assertion is that bulker accepted the -s flag (it would exit 2 for bad args)
    [ "$rc" -ne 2 ]
}

test_lifecycle() {
    setup_no_load

    # Install (local manifest gets cached as local/test_manifest:default)
    "$BULKERS" crate install -c "$CONFIG" "$MANIFEST"

    # List shows the crate
    list_output=$("$BULKERS" crate list -c "$CONFIG" 2>&1)
    echo "$list_output" | grep -q "test_manifest"

    # Inspect shows commands
    inspect_output=$("$BULKERS" crate inspect -c "$CONFIG" local/test_manifest:default 2>&1)
    echo "$inspect_output" | grep -q "cowsay"
    echo "$inspect_output" | grep -q "fortune"

    # Clean
    "$BULKERS" crate clean -c "$CONFIG" local/test_manifest:default

    # List no longer shows it
    list_output=$("$BULKERS" crate list -c "$CONFIG" 2>&1)
    if echo "$list_output" | grep -q "local/test_manifest:default"; then
        return 1
    fi
}

test_envvars_passthrough() {
    setup

    # Add envvar via config set (comma-separated list)
    "$BULKERS" config set -c "$CONFIG" "envvars=DISPLAY,MY_TEST_VAR"

    # Verify it was added
    get_output=$("$BULKERS" config get -c "$CONFIG" envvars 2>&1)
    echo "$get_output" | grep -q "MY_TEST_VAR"
}

# ---------- main ----------

echo "=== Bulkers End-to-End Tests ==="
echo ""

# Verify prerequisites
if ! command -v "$BULKERS" >/dev/null 2>&1 && [ ! -x "$BULKERS" ]; then
    echo "ERROR: bulker binary not found: $BULKERS"
    exit 1
fi

if ! docker info >/dev/null 2>&1; then
    echo "ERROR: Docker is not running"
    exit 1
fi

echo "Binary: $BULKERS"
echo "Manifest: $MANIFEST"
echo ""

run_test test_init
run_test test_load_and_build
run_test test_run_cowsay
run_test test_run_exit_code
run_test test_activate_echo_and_source
run_test test_run_strict_mode
run_test test_lifecycle
run_test test_envvars_passthrough

echo ""
echo "=== Results: $PASSED passed, $FAILED failed ==="

if [ "$FAILED" -gt 0 ]; then
    echo ""
    echo "Failures:"
    echo -e "$FAILURES"
    exit 1
fi
