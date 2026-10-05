#!/bin/sh
# Measures the compiled path against the two Vortex engines on one function.
#
# The claim this stage makes is that a compiled path exists and agrees with the
# tree interpreter and the VM. The measurement below is not about whether it is
# faster, and at the size that can be compiled today it is not expected to be:
# a C function is called once, so the cost is process start and dynamic linking
# rather than the loop. That is recorded rather than left out, because a
# disappointing number measured honestly is worth more than no number.
#
# Usage:
#   bench/measure-compiled.sh              five repeats
#   bench/measure-compiled.sh --repeats 9  more repeats
#
# The compiled path is measured by timing the built binary directly, so the
# measurement is of the compiled code and not of the emitter or the C compiler.

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
        echo "usage: bench/measure-compiled.sh [--repeats N]" >&2
        exit 2
        ;;
esac

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
WORK="$ROOT/bench/build/compiled"
mkdir -p "$WORK"

# The function under test. It is a loop, so the measurement is not dominated by
# call overhead the way a straight line function would be.
SRC="$WORK/loop.vx"
cat > "$SRC" <<'VORTEX'
fn sum_squares(n: Int) -> Int {
    var total = 0;
    var i = 1;
    while i <= n {
        total = total + i * i;
        i = i + 1;
    }
    return total;
}

fn main() -> Int {
    let r = sum_squares(2000000);
    return r;
}
VORTEX

RUNNER="$ROOT/target/release/examples/run_example"
CRUNNER="$ROOT/target/release/examples/run_compiled"

cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml" \
    --example run_example --example run_compiled

# The compiled binary, built once and timed on its own.
"$CRUNNER" "$SRC" sum_squares >/dev/null
BIN="/tmp/vortex-cgen/program"
[ -x "$BIN" ] || { echo "the compiled binary was not produced" >&2; exit 1; }

echo "Vortex compiled path measurement"
echo "=============================="
echo
echo "Repeats:          $REPEATS"
echo "Function:         sum_squares over 2000000"
echo "Workload:         $SRC"
echo "Compiled binary:  $BIN"
echo "C compiler:       $(${CC:-cc} --version 2>/dev/null | head -n 1)"
echo
echo "PATH       RUN TIMES IN ms, THEN ANSWER"
echo "---------  ------------------------------------------------"

measure() {
    _label=$1
    shift
    printf '%-9s ' "$_label"
    _i=1
    while [ "$_i" -le "$REPEATS" ]; do
        _start=$(date +%s%N)
        _out=$("$@" 2>/dev/null)
        _end=$(date +%s%N)
        _ms=$(awk -v a="$_start" -v b="$_end" 'BEGIN { printf "%d", (b - a) / 1000000 }')
        printf '%s ' "$_ms"
        _answer=$_out
        _i=$((_i + 1))
    done
    printf '| %s\n' "$_answer"
}

measure "compiled" "$BIN"
measure "tree" "$RUNNER" "$SRC" --print-value
measure "vm" "$RUNNER" "$SRC" --vm --print-value


echo
echo "The three answers have to match. They are printed above so a reader can"
echo "see that they do, rather than being asked to take it."
echo
echo "What this does not measure: the whole benchmark workload. The emitter"
echo "handles scalar functions with loops, and refuses anything with a list, an"
echo "if expression or a match, so it cannot carry bench/vortex/sieve.vx yet."
echo "A number for the full workload would require an emitter that can, and"
echo "none is claimed here."