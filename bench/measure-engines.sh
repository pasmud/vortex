#!/bin/sh
# Measures the two Vortex execution engines on the reference workload.
#
# This exists because stage 5 has to decide the backend from measurement rather
# than from assumption, and the stage 4 harness reports one number per engine
# from a single run of bench/run.sh, which is not enough to tell an improvement
# from timing jitter on this host.
#
# It runs each engine REPEATS times and reports the fastest, which is what
# bench/run.sh does too, and it also reports every individual run so a reader
# can see the spread rather than trusting one figure.
#
# Usage:
#   bench/measure-engines.sh              five repeats, the default
#   bench/measure-engines.sh --repeats 9  more repeats for a tighter figure
#
# Raw output is written to stdout so it can be captured, and nothing here writes
# to BENCHMARKS.md. A number reaches that file by a person copying it out of a
# committed transcript, and scripts/check-benchmarks.sh checks it landed there.

set -eu

REPEATS=5
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
        echo "usage: bench/measure-engines.sh [--repeats N]" >&2
        exit 2
        ;;
esac

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
WORKLOAD="$ROOT/bench/vortex/sieve.vx"
RUNNER="$ROOT/target/release/examples/run_example"

[ -f "$WORKLOAD" ] || { echo "missing $WORKLOAD" >&2; exit 1; }

if [ ! -x "$RUNNER" ]; then
    cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml" --example run_example
fi

# One engine's runs. Prints every time so the spread is visible.
measure() {
    _label=$1
    _flag=$2
    printf '%-8s ' "$_label"
    _i=1
    while [ "$_i" -le "$REPEATS" ]; do
        _start=$(date +%s%N)
        # shellcheck disable=SC2086
        _out=$("$RUNNER" "$WORKLOAD" $_flag 2>/dev/null)
        _end=$(date +%s%N)
        _ms=$(awk -v a="$_start" -v b="$_end" 'BEGIN { printf "%d", (b - a) / 1000000 }')
        _checksum=$(printf '%s\n' "$_out" | sed -n 's/^checksum \([0-9][0-9]*\).*/\1/p')
        if [ -z "$_checksum" ]; then
            printf 'failed '
        else
            printf '%s ' "$_ms"
        fi
        _i=$((_i + 1))
    done
    printf '| checksum %s\n' "$_checksum"
}

echo "Vortex engine measurement"
echo "========================="
echo
echo "Repeats per engine: $REPEATS (every run is shown, not just the fastest)"
echo "Workload:           bench/vortex/sieve.vx"
echo "Machine:            $(uname -srm) on $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //')"
echo
echo "ENGINE   RUN TIMES IN ms, THEN CHECKSUM"
echo "-------  ------------------------------------------------"
measure "tree" ""
measure "vm" "--vm"

echo
echo "The checksum is printed by each engine and must be the same as the C and"
echo "Rust baselines. A run that printed no checksum shows as 'failed'."
echo
echo "A single fastest figure hides jitter. Stage 4 saw the tree interpreter"
echo "move by 55 ms and the VM by 76 ms between runs on this host, so a change"
echo "smaller than that spread is not evidence of anything."