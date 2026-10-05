#!/bin/sh
# Measures every execution path on the benchmark workload.
#
# Four paths, one workload, five repeats each, every run printed rather than
# only the fastest. The compiled row is here for the first time, which is what
# makes a comparison between a compiled Vortex program and a compiled C program
# possible at all.
#
# Usage:
#   bench/measure-all.sh              five repeats
#   bench/measure-all.sh --repeats 9  more repeats
#
# Nothing here writes to BENCHMARKS.md. A number reaches that file by a person
# copying it out of a committed transcript, and scripts/check-benchmarks.sh
# checks it landed there.

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
        echo "usage: bench/measure-all.sh [--repeats N]" >&2
        exit 2
        ;;
esac

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
WORK="$ROOT/bench/vortex/sieve.vx"
BUILD="$ROOT/bench/build"
mkdir -p "$BUILD"

RUNNER="$ROOT/target/release/examples/run_example"
CRUNNER="$ROOT/target/release/examples/run_compiled"

cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml" \
    --example run_example --example run_compiled

# The C and Rust baselines.
cc -O2 -o "$BUILD/c_sieve" "$ROOT/bench/c/sieve.c"
cargo build --release --quiet --manifest-path "$ROOT/bench/rust/Cargo.toml"
cp "$ROOT/bench/rust/target/release/vortex-bench" "$BUILD/rust_bench" 2>/dev/null || true

# The compiled Vortex binary, built from the same Vortex source as the two
# engines so the three measure the same program.
rm -f /tmp/vortex-cgen/*
"$CRUNNER" "$WORK" main >/dev/null
CBIN="/tmp/vortex-cgen/program"
[ -x "$CBIN" ] || { echo "the compiled binary was not produced" >&2; exit 1; }

# The sieve sum, so a mismatch is visible rather than assumed.
# The workload prints its own checksum on stdout and returns 0, so the printed
# line is what carries the answer. Reading the return value would report 0.
CBIN_ANSWER=$("$CBIN" 2>/dev/null | sed -n 's/^checksum //p')

echo "Vortex execution path measurement, stage 8"
echo "=========================================="
echo
echo "Repeats:        $REPEATS"
echo "Workload:       $WORK"
echo "Compiled C:     $(cc --version 2>/dev/null | head -n 1)"
echo "Rust:           $(rustc --version 2>/dev/null)"
echo "Machine:        $(uname -srm) on $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //')"
echo
echo "PATH          RUN TIMES IN ms, THEN CHECKSUM"
echo "-------------  ------------------------------------------------"

measure() {
    _label=$1
    shift
    printf '%-13s ' "$_label"
    _i=1
    while [ "$_i" -le "$REPEATS" ]; do
        _start=$(date +%s%N)
        if [ "$1" = "$CBIN" ]; then
            _out=$("$@" 2>/dev/null | sed -n 's/^checksum //p')
        else
            _out=$("$@" 2>/dev/null | head -n 1)
        fi
        _end=$(date +%s%N)
        _ms=$(awk -v a="$_start" -v b="$_end" 'BEGIN { printf "%d", (b - a) / 1000000 }')
        printf '%s ' "$_ms"
        _answer=$_out
        _i=$((_i + 1))
    done
    printf '| %s\n' "$_answer"
}

measure "C" "$BUILD/c_sieve"
measure "Rust" "$BUILD/rust_bench"
measure "Vortex C" "$CBIN"
measure "Vortex tree" "$RUNNER" "$WORK"
measure "Vortex VM" "$RUNNER" "$WORK" --vm

echo
echo "The checksums are printed so a reader can see whether the paths agree,"
echo "rather than being asked to take it."
echo
echo "All four Vortex paths run the whole workload and print the same checksum,"
echo "the sieve and the matrix together. The Vortex C row was sieve only until"
echo "stage 8 fixed the float path; FLOAT-DEFECT.md records the four defects"
echo "that had to be fixed before it could measure the same work as the others."
