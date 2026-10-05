#!/bin/sh
# Guards the performance claim in BENCHMARKS.md.
#
# The claim has to come from a committed transcript, not from a run anyone
# remembers. This checks that in four layers.
#
# 1. While no comparable Vortex result is recorded, BENCHMARKS.md must still say
#    so, and must carry the reasoning for the empty row rather than asserting it.
#
# 2. Every wall clock number in BENCHMARKS.md must appear in the committed raw
#    output for that row, and every number the transcript records must be the one
#    the table quotes.
#
#    This is the check that matters most, and it exists because the numbers had
#    drifted: the table was written by hand from one run while the committed
#    transcript was from another, and all four documented numbers were the faster
#    ones. Comparing a committed document against a committed artifact is not
#    sensitive to timing jitter, because it compares two fixed things, and it
#    catches both a transcription error and a stale transcript.
#
# 3. Every row of the table must be present and the matrix half of the checksum
#    must be recorded, so a row cannot quietly claim less than it measured.
#
# 4. When a result is recorded, the machine specification and the committed raw
#    output must both be present.
#
# This script is not weakened to make a run pass. If it fails, the claim in
# BENCHMARKS.md and the artifact meant to prove it disagree, and one is wrong.

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

# Both documents are wrapped for reading, so their text is flattened before it
# is searched. Searching the wrapped file would miss a phrase split across lines.
FLAT=$(tr '\n' ' ' < "$BENCH" | tr -s ' ')

if printf '%s' "$FLAT" | grep -q '## NO COMPARABLE VORTEX RESULT YET'; then
    # --- no result recorded yet -------------------------------------------
    if ! grep -q 'VORTEX_STATUS="no comparable time:' "$RUN"; then
        fail "bench/run.sh no longer says why the Vortex row is empty, while \
BENCHMARKS.md still says no comparable result was recorded"
    fi

    for phrase in "\`as\` cast" "too slow in a tree interpreter"; do
        case "$FLAT" in
            *"$phrase"*) ;;
            *) fail "BENCHMARKS.md claims no comparable result without recording: $phrase" ;;
        esac
    done

    [ -f "$ROOT/bench/vortex/sieve.vx" ] \
        || fail "BENCHMARKS.md says the Vortex sieve is written, but \
bench/vortex/sieve.vx is missing"

    echo "BENCHMARKS.md records no comparable Vortex result, bench/run.sh"
    echo "explains why, and the measurements behind the decision are present."
    echo "The check is satisfied."
    exit 0
fi

# --- a result is recorded --------------------------------------------------

echo "BENCHMARKS.md records a Vortex result. Checking it against the transcript."

RAW="$ROOT/bench/results/stage4-vm.txt"
[ -f "$RAW" ] || fail "bench/results/stage4-vm.txt is missing"

RAWFLAT=$(tr '\n' ' ' < "$RAW" | tr -s ' ')

# (4) The machine specification has to be there.
grep -q '^## Machine specification' "$BENCH" \
    || fail "a result is recorded but the machine specification is missing"

# (3) Every row must be present. Rows are matched by name rather than by number,
# because a timing that moves by a millisecond between runs is not a falsified
# claim, while a deleted row is a measurement that disappeared.
# The table's cells sit on their own lines once the text is split on the pipe,
# so the row name and its number end up separated by a newline. The cells are
# joined back together so a row is one searchable line again.
ROWS=$(printf '%s' "$FLAT" | tr '|' '\n' | sed 's/^ *//; s/ *$//' | tr '\n' ' ')

for name in "C" "Rust" "Vortex, tree interpreter" "Vortex, bytecode VM"; do
    case "$ROWS" in
        *" $name "*) ;;
        *) fail "BENCHMARKS.md is missing the $name row" ;;
    esac
done

case "$FLAT" in
    *"3314.003906"*) ;;
    *) fail "BENCHMARKS.md does not record the matrix half of the checksum" ;;
esac

case "$RAWFLAT" in
    *"checksum 1179908154 3314.003906"*) ;;
    *) fail "the committed transcript does not carry the baseline checksum" ;;
esac

# (2) The check that matters. Each documented number must be in the transcript,
# and each transcript number must be the one the table quotes, so the table
# cannot drift from the artifact that is supposed to prove it.
#
# $1 is the transcript label, $2 is the document row name.
check_row_numbers() {
    _raw=$1
    _doc=$2

    # What the document quotes on its row. The row label is turned into a
    # pattern so the name may contain a comma.
    _pat=$(printf '%s' "$_doc" | sed 's/,/\\,/g')
    _doc_ms=$(printf '%s' "$ROWS" | grep -o "\($_pat\) [0-9][0-9]* ms" | head -n 1)

    case "$_doc_ms" in
        "") fail "BENCHMARKS.md's '$_doc' row quotes no wall clock number" ;;
    esac

    # What the transcript recorded.
    _raw_ms=$(printf '%s' "$RAWFLAT" | sed -n "s/.*$_raw[^0-9]*\([0-9][0-9]* ms\).*/\1/p")

    case "$_raw_ms" in
        "") fail "bench/results/stage4-vm.txt has no '$_raw' row with a time" ;;
    esac

    case "$_doc_ms" in
        *"$_raw_ms"*) ;;
        *)
            fail "BENCHMARKS.md quotes '$_doc_ms' but bench/results/stage4-vm.txt \
records '$_raw_ms'. The table and the transcript disagree; write the table \
from the transcript rather than from memory."
            ;;
    esac
}

check_row_numbers "C" "C"
check_row_numbers "Rust" "Rust"
check_row_numbers "Vortex tree" "Vortex, tree interpreter"
check_row_numbers "Vortex VM" "Vortex, bytecode VM"

echo "The machine specification is present, every row is present, the matrix"
echo "half of the checksum is recorded, and every wall clock number the"
echo "document quotes is the number the committed transcript records."
echo "The check is satisfied."
# --- DECISION.md ------------------------------------------------------------
#
# Stage 5 quotes before and after numbers for the two VM changes it tried, and
# a reader has as much right to diff those against a committed transcript as
# they have for the BENCHMARKS.md table. The before figures are the baseline in
# bench/results/stage5-baseline.txt, so every "was" figure has to appear there.
#
# A figure in DECISION.md that is not in a transcript is a measurement that
# cannot be checked, so this fails rather than passing on trust.
DEC="$ROOT/DECISION.md"
[ -f "$DEC" ] || fail "DECISION.md is missing"

BASELINE="$ROOT/bench/results/stage5-baseline.txt"
[ -f "$BASELINE" ] || fail "bench/results/stage5-baseline.txt is missing"
BASEFLAT=$(tr '\n' ' ' < "$BASELINE" | tr -s ' ')

DECFLAT=$(tr '\n' ' ' < "$DEC" | tr -s ' ')

# The baseline figures DECISION.md quotes, which are the fastest runs in the
# committed stage5 baseline transcript.
for ms in 4935 3223; do
    case "$DECFLAT" in
        *"was"*"$ms ms"*) ;;
        *) fail "DECISION.md quotes $ms ms as a before figure but does not word it as one" ;;
    esac
    case "$BASEFLAT" in
        *"$ms"*) ;;
        *) fail "DECISION.md quotes $ms ms as a before figure and \
bench/results/stage5-baseline.txt does not record it" ;;
    esac
done

echo "DECISION.md's before figures appear in bench/results/stage5-baseline.txt."
echo "The check is satisfied."


# --- MEASUREMENTS.md --------------------------------------------------------
#
# Stage 6 records its figures in its own document, and they have the same
# traceability requirement as the other two. The compiled path measurement is in
# bench/results/stage6-compiled.txt and every row quoted in MEASUREMENTS.md has
# to match it.
MEAS="$ROOT/MEASUREMENTS.md"
TRANSCRIPT6="$ROOT/bench/results/stage6-compiled.txt"

if [ -f "$MEAS" ] && [ -f "$TRANSCRIPT6" ]; then
    MFLAT=$(tr '\n' ' ' < "$MEAS" | tr -s ' ')
    T6FLAT=$(tr '\n' ' ' < "$TRANSCRIPT6" | tr -s ' ')

    # Each row's run times and answer, as the transcript records them.
    # The document writes the runs as a comma separated list for readability while
    # the transcript writes them space separated, so commas become spaces before
    # the two are compared.
    MDOTS=$(printf '%s' "$MFLAT" | tr ',' ' ' | tr -s ' ')
    for row in "6 6 7 9 7" "560 561 561 560 563" "949 949 943 954 941"; do
        case "$MDOTS" in
            *"$row"*) ;;
            *) fail "MEASUREMENTS.md does not quote the run '$row'" ;;
        esac
        case "$T6FLAT" in
            *"$row"*) ;;
            *) fail "MEASUREMENTS.md quotes the run '$row' and bench/results/stage6-compiled.txt does not record it" ;;
        esac
    done
    # Every path has to report the same answer, and the document has to say it.
    for answer in 2666668666667000000; do
        case "$MFLAT" in
            *"$answer"*) ;;
            *) fail "MEASUREMENTS.md does not record the answer $answer" ;;
        esac
        case "$T6FLAT" in
            *"$answer"*) ;;
            *) fail "the committed transcript does not record the answer $answer" ;;
        esac
    done

    echo "MEASUREMENTS.md's figures appear in bench/results/stage6-compiled.txt."
fi
echo "The check is satisfied."
