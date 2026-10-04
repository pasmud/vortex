#!/bin/sh
# Guards the performance claim in BENCHMARKS.md.
#
# Three things are checked, and each matters:
#
# 1. While no comparable Vortex result is recorded, BENCHMARKS.md must still say
#    so. A result may only be added by removing that marker deliberately,
#    together with the machine and the raw output, so the two cannot drift
#    apart.
#
# 2. While the Vortex row is empty, bench/run.sh must explain why. A row that
#    says only "n/a" hides whether the number is missing on purpose or was
#    forgotten.
#
# 3. While no result is recorded, the file must carry the measurements that led
#    to that decision, so the decision is auditable rather than asserted.
#
# When a result is recorded, the machine specification and the raw harness
# output must be committed with it.
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

# BENCHMARKS.md is wrapped for reading, so the text is flattened before it is
# searched. Searching the wrapped file would miss a phrase split across lines.
FLAT=$(tr '\n' ' ' < "$BENCH" | tr -s ' ')

if printf '%s' "$FLAT" | grep -q '## NO COMPARABLE VORTEX RESULT YET'; then
    NO_RESULT=yes
else
    NO_RESULT=no
fi

if [ "$NO_RESULT" = "yes" ]; then
    # The harness has to carry a real reason, not a bare placeholder.
    if ! grep -q 'VORTEX_STATUS="no comparable time:' "$RUN"; then
        fail "bench/run.sh no longer says why the Vortex row is empty, while \
BENCHMARKS.md still says no comparable result was recorded"
    fi

    # The reasoning for the empty row must be present, not merely asserted.
    for phrase in "\`as\` cast" "too slow in a tree interpreter"; do
        case "$FLAT" in
            *"$phrase"*) ;;
            *) fail "BENCHMARKS.md claims no comparable result without recording: $phrase" ;;
        esac
    done

    # The timings must actually be there. Checking only for the limit passed
    # even when a measurement had been deleted from the table, so each row is
    # matched whole: the limit and its measured milliseconds.
    for row in "10000 | 5736396 | 3676 ms" "20000 | 21171191 | 15862 ms"; do
        case "$FLAT" in
            *"$row"*) ;;
            *) fail "BENCHMARKS.md is missing the measurement: $row" ;;
        esac
    done

    # A Vortex workload must exist for the decision to be about one.
    [ -f "$ROOT/bench/vortex/sieve.vx" ] \
        || fail "BENCHMARKS.md says the Vortex sieve is written, but \
bench/vortex/sieve.vx is missing"

    echo "BENCHMARKS.md records no comparable Vortex result, bench/run.sh"
    echo "explains why, and the measurements behind the decision are present."
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
