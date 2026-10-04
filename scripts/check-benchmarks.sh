#!/bin/sh
# Guards the performance claim in BENCHMARKS.md.
#
# Two things are checked, and both matter:
#
# 1. While no Vortex result is recorded, BENCHMARKS.md must still say so. A
#    result may only be added by removing that marker deliberately, together
#    with the machine and the raw output, so the two cannot drift apart.
#
# 2. While the Vortex row is empty, bench/run.sh must explain why. A row that
#    says only "n/a" hides whether the number is missing on purpose or was
#    forgotten, so the harness has to carry a reason.
#
# This script is not weakened to make a run pass. If it fails, the claim in
# BENCHMARKS.md and the behaviour of the harness disagree, and one of them is
# wrong.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BENCH="$ROOT/BENCHMARKS.md"
RUN="$ROOT/bench/run.sh"

fail() {
    echo "$1" >&2
    exit 1
}

[ -f "$BENCH" ] || fail "BENCHMARKS.md is missing"
[ -f "$RUN" ] || fail "bench/run.sh is missing"

# A recorded result would be a Vortex row carrying a time. Detect it by looking
# for a wall clock next to the Vortex row in this file's own terms, which is
# the marker below.
if grep -q '^## NO VORTEX RESULTS YET' "$BENCH"; then
    HAS_RESULT=no
else
    HAS_RESULT=yes
fi

# The harness must say why the Vortex row has no time, and the reason must
# survive as real text rather than a bare placeholder.
if ! grep -q 'VORTEX_STATUS="no Vortex workload:' "$RUN"; then
    if [ "$HAS_RESULT" = "no" ]; then
        fail "bench/run.sh no longer says why the Vortex row is empty, while \
BENCHMARKS.md still says no results were recorded"
    fi
fi

if [ "$HAS_RESULT" = "no" ]; then
    # The two files must agree that there is no result, and the reasoning for
    # it must be present rather than asserted. The file is wrapped for reading,
    # so the text is flattened before it is searched.
    FLAT=$(tr '\n' ' ' < "$BENCH" | tr -s ' ')

    case "$FLAT" in
        *"no index assignment"*) ;;
        *) fail "BENCHMARKS.md claims no Vortex result without recording why" ;;
    esac

    case "$FLAT" in
        *"computed length"*) ;;
        *) fail "BENCHMARKS.md should record the second language gap too" ;;
    esac
    echo "BENCHMARKS.md records no Vortex result, and bench/run.sh explains why."
    echo "The check is satisfied."
    exit 0
fi

# A result is recorded, so the evidence has to be here with it.
echo "BENCHMARKS.md records a Vortex result. Checking the evidence is present."

grep -q '^## Machine specification' "$BENCH" \
    || fail "a result is recorded but the machine specification is missing"
grep -q 'raw output' "$BENCH" \
    || fail "a result is recorded but the file does not point at raw output"

if [ ! -d "$ROOT/bench/results" ]; then
    fail "a result is recorded but bench/results does not exist, so the raw \
harness output is not committed"
fi

echo "The check is satisfied."
