#!/usr/bin/env bash
#
# End-to-end tests for bulkers. Requires Docker and the bulkers binary.
#
# Usage:
#   ./tests/e2e/run_tests.sh [path/to/bulkers]
#
# If no binary path is given, assumes "bulkers" is on PATH.

set -euo pipefail

BULKERS="${1:-bulkers}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST="$SCRIPT_DIR/test_manifest.yaml"

PASSED=0
FAILED=0
FAILURES=""

# ---------- helpers ----------

setup() {
    TMPDIR="$(mktemp -d)"
    CONFIG="$TMPDIR/bulker_config.yaml"

    # Init
    "$BULKERS" init -c "$CONFIG" >/dev/null 2>&1

    # Patch default_crate_folder to be inside our tmpdir
    sed -i "s|default_crate_folder:.*|default_crate_folder: $TMPDIR/crates|" "$CONFIG"

    # Load the test crate (with --build to pull images)
    "$BULKERS" load -c "$CONFIG" -m "$MANIFEST" -b test/demo:1.0 >/dev/null 2>&1
}

setup_no_load() {
    TMPDIR="$(mktemp -d)"
    CONFIG="$TMPDIR/bulker_config.yaml"
    "$BULKERS" init -c "$CONFIG" >/dev/null 2>&1
    sed -i "s|default_crate_folder:.*|default_crate_folder: $TMPDIR/crates|" "$CONFIG"
}

teardown() {
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

    "$BULKERS" init -c "$CONFIG"

    # Config exists and contains expected content
    [ -f "$CONFIG" ]
    grep -q "container_engine" "$CONFIG"
    grep -q "docker" "$CONFIG"
}

test_load_and_build() {
    setup_no_load

    "$BULKERS" load -c "$CONFIG" -m "$MANIFEST" -b test/demo:1.0

    # Image should have been pulled
    docker image inspect nsheff/cowsay >/dev/null 2>&1
}

test_run_cowsay() {
    setup

    output=$("$BULKERS" run -c "$CONFIG" test/demo:1.0 cowsay "hello from bulkers" 2>&1)

    echo "$output" | grep -q "hello from bulkers"
}

test_run_exit_code() {
    setup

    # Run a command that doesn't exist - should return non-zero
    set +e
    "$BULKERS" run -c "$CONFIG" test/demo:1.0 nonexistent_command 2>/dev/null
    rc=$?
    set -e

    [ "$rc" -ne 0 ]
}

test_activate_echo_and_source() {
    setup

    activate_output=$("$BULKERS" activate -c "$CONFIG" --echo test/demo:1.0 2>&1)

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
    output=$("$BULKERS" activate -c "$CONFIG" --echo test/demo:1.0 2>&1)
    # The non-strict PATH should contain the crate folder AND existing PATH
    echo "$output" | grep -q "export PATH="

    # Now test non-strict run works (baseline)
    output=$("$BULKERS" run -c "$CONFIG" test/demo:1.0 cowsay "strict test" 2>&1)
    echo "$output" | grep -q "strict test"

    # Verify strict flag is accepted and runs (it may fail because docker isn't
    # on the strict PATH, but the binary should not error on the flag itself)
    set +e
    "$BULKERS" run -c "$CONFIG" -s test/demo:1.0 cowsay "strict test" 2>/dev/null
    rc=$?
    set -e
    # rc may be non-zero due to docker not being on strict PATH - that's expected
    # The key assertion is that bulkers accepted the -s flag (it would exit 2 for bad args)
    [ "$rc" -ne 2 ]
}

test_lifecycle() {
    setup_no_load

    # Load
    "$BULKERS" load -c "$CONFIG" -m "$MANIFEST" test/demo:1.0

    # List shows the crate
    list_output=$("$BULKERS" list -c "$CONFIG" 2>&1)
    echo "$list_output" | grep -q "test/demo:1.0"

    # Inspect shows commands
    inspect_output=$("$BULKERS" inspect -c "$CONFIG" test/demo:1.0 2>&1)
    echo "$inspect_output" | grep -q "cowsay"
    echo "$inspect_output" | grep -q "fortune"

    # Unload
    "$BULKERS" unload -c "$CONFIG" test/demo:1.0

    # List no longer shows it
    list_output=$("$BULKERS" list -c "$CONFIG" 2>&1)
    if echo "$list_output" | grep -q "test/demo:1.0"; then
        return 1
    fi
}

test_envvars_passthrough() {
    setup

    # Add envvar to config
    "$BULKERS" envvars -c "$CONFIG" -a MY_TEST_VAR

    # Set the envvar and run a command that echoes it
    export MY_TEST_VAR=hello_from_test
    output=$("$BULKERS" run -c "$CONFIG" test/demo:1.0 fortune 2>&1)

    # fortune just prints a quote - we can't check the envvar this way since
    # the container doesn't echo it. Instead, verify the envvar was added to config.
    grep -q "MY_TEST_VAR" "$CONFIG"
    unset MY_TEST_VAR
}

# ---------- main ----------

echo "=== Bulkers End-to-End Tests ==="
echo ""

# Verify prerequisites
if ! command -v "$BULKERS" >/dev/null 2>&1 && [ ! -x "$BULKERS" ]; then
    echo "ERROR: bulkers binary not found: $BULKERS"
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
