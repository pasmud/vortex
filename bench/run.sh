#!/bin/sh
# The Vortex benchmark harness.
#
# Builds the same workload in every language that has an implementation, runs
# each one, and prints a comparison table.
#
# A row is filled in only by a binary that actually executed. This script never
# prints a number for an implementation that did not run, and never estimates
# one.
#
# Usage:
#   bench/run.sh              run every implemented workload once
#   bench/run.sh --repeats 5  run each workload five times and report the best
#
# Results are not recorded by this script. A result is added to BENCHMARKS.md by
# a person, together with the machine it was measured on, the compiler versions
# and the raw output of this script.

set -eu

REPEATS=1
case "${1:-}" in
    "") ;;
    --repeats)
        REPEATS="${2:-}"
        if ! [ "$REPEATS" -ge 1 ] 2>/dev/null; then
            echo "--repeats needs a positive integer, got '$REPEATS'" >&2
            exit 2
        fi
        ;;
    *)
        echo "usage: bench/run.sh [--repeats N]" >&2
        exit 2
        ;;
esac

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BUILD="$ROOT/bench/build"

mkdir -p "$BUILD"

# --- Build ---------------------------------------------------------------

C_STATUS="built"
if command -v cc >/dev/null 2>&1; then
    cc -O2 -o "$BUILD/c_sieve" "$ROOT/bench/c/sieve.c"
    C_VERSION=$(cc --version 2>/dev/null | head -n 1)
else
    C_STATUS="skipped: no C compiler found"
    C_VERSION="not available"
fi

RUST_STATUS="built"
if command -v cargo >/dev/null 2>&1; then
    cargo build --release --quiet --manifest-path "$ROOT/bench/rust/Cargo.toml"
    RUST_BIN=$(cargo build --release --quiet --manifest-path "$ROOT/bench/rust/Cargo.toml" \
        --message-format=json 2>/dev/null \
        | grep -o '"executable":"[^"]*"' | head -n 1 \
        | cut -d'"' -f4)
    if [ -n "$RUST_BIN" ]; then
        cp "$RUST_BIN" "$BUILD/rust_bench"
    fi
    RUST_VERSION=$(rustc --version 2>/dev/null || echo "not available")
else
    RUST_STATUS="skipped: no cargo found"
    RUST_VERSION="not available"
fi

# The Vortex row stays empty until a Vortex program can express the same
# workload. See bench/vortex/README.md for the two language gaps that stop it
# today: there is no index assignment and no list of a computed length.
VORTEX_STATUS="no Vortex workload: the baseline sieve needs index assignment and a list of a computed length, neither of which v0.1 has. See bench/vortex/README.md"

# --- Run -----------------------------------------------------------------

# Runs a binary REPEATS times and reports the fastest wall clock time in
# milliseconds, or the reason it did not run.
#
# Timing uses `date +%s%N` rather than the shell keyword `time`, because that
# keyword is not available in every POSIX shell the script may be run from.
# Elapsed time is measured in whole milliseconds, which is ample for a workload
# of this size.
run_one() {
    _label=$1
    _binary=$2
    _status=$3

    if [ "$_status" != "built" ]; then
        printf '%-10s %-22s %s\n' "$_label" "n/a" "$_status"
        return 0
    fi

    _best=""
    _checksum=""
    _i=0
    while [ "$_i" -lt "$REPEATS" ]; do
        _start=$(date +%s%N)
        _output=$("$_binary") || {
            printf '%-10s %-22s %s\n' "$_label" "error" "the binary exited non-zero"
            return 0
        }
        _end=$(date +%s%N)
        _elapsed=$(awk -v a="$_start" -v b="$_end" 'BEGIN { printf "%d", (b - a) / 1000000 }')

        _checksum=$(printf '%s\n' "$_output" | grep '^checksum ' || true)
        if [ -z "$_checksum" ]; then
            printf '%-10s %-22s %s\n' "$_label" "error" "the binary printed no checksum"
            return 0
        fi

        if [ -z "$_best" ]; then
            _best=$_elapsed
        else
            _best=$(awk -v a="$_best" -v b="$_elapsed" 'BEGIN { print (b < a ? b : a) }')
        fi
        _i=$((_i + 1))
    done

    printf '%-10s %-22s %s\n' "$_label" "$_best ms" "$_checksum"
}

echo
echo "Vortex benchmark harness"
echo "========================"
echo
echo "Repeats per workload: $REPEATS (the fastest run is reported)"
echo "Build directory:      $BUILD"
echo
printf '%-10s %-22s %s\n' "LANGUAGE" "WALL CLOCK" "CHECKSUM"
printf '%-10s %-22s %s\n' "--------" "----------" "--------"

run_one "C" "$BUILD/c_sieve" "$C_STATUS"
run_one "Rust" "$BUILD/rust_bench" "$RUST_STATUS"
printf '%-10s %-22s %s\n' "Vortex" "n/a" "$VORTEX_STATUS"

echo
echo "Toolchain used for this run"
echo "---------------------------"
echo "C:    $C_VERSION"
echo "Rust: $RUST_VERSION"
echo "Host: $(uname -srm)"
echo "CPU:  $( (grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //') || echo 'not available')"

if [ "$REPEATS" -eq 1 ]; then
    echo
    echo "One run per workload is noisy. Use --repeats 5 or more for any result"
    echo "that is going into BENCHMARKS.md."
fi
echo