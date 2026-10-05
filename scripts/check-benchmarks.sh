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

RAW="$ROOT/bench/results/stage8-compiled-full.txt"
[ -f "$RAW" ] || fail "bench/results/stage8-compiled-full.txt is missing"
FULL=$(tr '\n' ' ' < "$BENCH" | tr ',' ' ' | tr -s ' ')
T8FLAT=$(tr '\n' ' ' < "$RAW" | tr ',' ' ' | tr -s ' ')

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
    _doc_num=$(printf '%s' "$_doc_ms" | grep -o '[0-9][0-9]* ms')

    case "$_doc_ms" in
        "") fail "BENCHMARKS.md's '$_doc' row quotes no wall clock number" ;;
    esac

    # What the transcript recorded. The stage 8 transcript prints every
    # individual run as a bare millisecond figure, so the quoted number has to
    # be one of those rather than a single "N ms" per row.
    # Every run of the row, so the quoted number may be any of them. The table
    # quotes the fastest and the transcript prints all five.
    _raw_runs=$(printf '%s' "$RAWFLAT" | \
        grep -o "\b$_raw  *[0-9][0-9]*\([ 0-9]*\)*" | head -n 1)
    _raw_ms=""
    for _r in $(printf '%s' "$_raw_runs" | sed "s/^$_raw *//" | tr ' ' '\n'); do
        _m=$(printf '%s' "$_r" | grep -o '^[0-9][0-9]*$') || continue
        case "$_raw_ms" in
            "") _raw_ms="$_m ms" ;;
            *)
                if [ "$_m" -lt "${_raw_ms% ms}" ]; then
                    _raw_ms="$_m ms"
                fi
                ;;
        esac
    done

    case "$_raw_ms" in
        "") fail "the committed transcript has no '$_raw' row with a time" ;;
    esac

    case "$_doc_num" in
        *"$_raw_ms"*) ;;
        *)
            fail "BENCHMARKS.md quotes '$_doc_ms' but the committed transcript \
records '$_raw_ms'. The table and the transcript disagree; write the table \
from the transcript rather than from memory."
            ;;
    esac
}

check_row_numbers "C" "C"
check_row_numbers "Rust" "Rust"
check_row_numbers "Vortex C" "Vortex, compiled to C"
check_row_numbers "Vortex tree" "Vortex, tree interpreter"
check_row_numbers "Vortex VM" "Vortex, bytecode VM"

# Stage 8 removed the sieve-only label, because the compiled path now
# measures the whole workload. The check that required the label was
# replaced by one that requires the full checksum in BENCHMARKS.md, and
# rejects the label if it reappears on the compiled row.
case "$FULL" in
    *"1179908154 3314.003906"*) ;;
    *) fail "BENCHMARKS.md does not record the full workload checksum 1179908154 3314.003906" ;;
esac
case "$T8FLAT" in
    *"1179908154 3314.003906"*) ;;
    *) fail "the stage 8 transcript does not record the full workload checksum" ;;
esac
# The compiled row itself, in its own cell, must carry the full checksum and
# must not be labelled sieve only.
# $FULL has commas flattened to spaces, so the label reads "Vortex  compiled
# to C" here.
CROW=$(printf '%s' "$FULL" | grep -o "| Vortex compiled to C |[^|]*|[^|]*|[^|]*|[^|]*|" | head -n 1)
case "$CROW" in
    "") fail "BENCHMARKS.md has no compiled Vortex row" ;;
esac
case "$CROW" in
    *"1179908154 3314.003906"*) ;;
    *) fail "BENCHMARKS.md's compiled row does not carry the full checksum: $CROW" ;;
esac
case "$CROW" in
    *"sieve only"*) fail "BENCHMARKS.md's compiled row is still labelled sieve only: $CROW" ;;
esac
# Every run of the compiled path is printed, so a single flattering number
# cannot stand in for the spread.
# The compiled path's row, with every individual run it printed.
CRAW=$(grep '^Vortex C' "$RAW" | head -n 1)
case "$CRAW" in
    "") fail "bench/results/stage8-compiled-full.txt has no Vortex C row" ;;
esac
CRUNS=$(printf '%s' "$CRAW" | grep -o '[0-9][0-9]*' | grep -c .)
[ "$CRUNS" -ge 5 ] ||
    fail "bench/results/stage8-compiled-full.txt prints $CRUNS runs for the compiled path, fewer than five"

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


# --- STAGE7.md --------------------------------------------------------------
#
# Stage 7 records five paths on the benchmark workload. The rule is the same as
# for the other documents: a number quoted here has to be the number
# bench/results/stage7-sieve.txt records.
ST7="$ROOT/STAGE7.md"
T7="$ROOT/bench/results/stage7-sieve.txt"


if [ -f "$ST7" ] && [ -f "$T7" ]; then
    S7FLAT=$(tr '\n' ' ' < "$ST7" | tr ',' ' ' | tr -s ' ')
    T7FLAT=$(tr '\n' ' ' < "$T7" | tr -s ' ')

    for row in "18 23 19 17 18" \
                "23 19 20 22 23" \
                "52 44 48 43 39" \
                "3276 3232 3284 3262 3233" \
                "5027 4997 5026 4993 5029"; do
        case "$S7FLAT" in
            *"$row"*) ;;
            *) fail "STAGE7.md does not quote the run '$row'" ;;
        esac
        case "$T7FLAT" in
            *"$row"*) ;;
            *) fail "STAGE7.md quotes the run '$row' and bench/results/stage7-sieve.txt does not record it" ;;
        esac
    done

    # Every path reports the sieve sum, and the document has to say so.
    case "$S7FLAT" in
        *"1179908154"*) ;;
        *) fail "STAGE7.md does not record the sieve checksum" ;;
    esac
    case "$T7FLAT" in
        *"1179908154"*) ;;
        *) fail "the committed transcript does not record the sieve checksum" ;;
    esac

    echo "STAGE7.md's figures appear in bench/results/stage7-sieve.txt, the stage"
    echo "8 transcript records the full checksum on every path, and BENCHMARKS.md"
    echo "carries the compiled row at the full workload without the sieve-only label."
fi
# --- STAGE9.md --------------------------------------------------------------
#
# Stage 9 is a before-and-after document, so it is checked against two
# transcripts rather than one. Every run it quotes has to be recorded by the
# transcript for that half of the comparison. The shape is otherwise the same as
# the STAGE7 check.
ST9="$ROOT/STAGE9.md"
T9A="$ROOT/bench/results/stage9-before.txt"
T9B="$ROOT/bench/results/stage9-after.txt"

[ -f "$ST9" ] || fail "STAGE9.md is missing"
[ -f "$T9A" ] || fail "bench/results/stage9-before.txt is missing"
[ -f "$T9B" ] || fail "bench/results/stage9-after.txt is missing"

S9FLAT=$(tr '\n' ' ' < "$ST9" | tr ',' ' ' | tr -s ' ')
T9AFLAT=$(tr '\n' ' ' < "$T9A" | tr ',' ' ' | tr -s ' ')
T9BFLAT=$(tr '\n' ' ' < "$T9B" | tr ',' ' ' | tr -s ' ')

# The runs quoted in the before-and-after table, before first.
for row in "55 46 44 43 52" "3335 3262 3261 3268 3256" "4957 5015 4996 4978 4983"; do
    case "$S9FLAT" in
        *"$row"*) ;;
        *) fail "STAGE9.md does not quote the before run '$row'" ;;
    esac
    case "$T9AFLAT" in
        *"$row"*) ;;
        *) fail "STAGE9.md quotes the before run '$row' and stage9-before.txt does not record it" ;;
    esac
done

# And after.
for row in "48 50 55 51 48" "3254 3268 3293 3259 3285" "4986 5019 4987 4995 5020"; do
    case "$S9FLAT" in
        *"$row"*) ;;
        *) fail "STAGE9.md does not quote the after run '$row'" ;;
    esac
    case "$T9BFLAT" in
        *"$row"*) ;;
        *) fail "STAGE9.md quotes the after run '$row' and stage9-after.txt does not record it" ;;
    esac
done

# The comparison is against the spread committed at stage 8, so that spread has
# to be quoted too.
case "$S9FLAT" in
    *"42 to 53 ms"*) ;;
    *) fail "STAGE9.md does not compare against the stage 8 spread of 42 to 53 ms" ;;
esac
case "$RAWFLAT" in
    *"42 53 50 48 49"*) ;;
    *) fail "the stage 8 transcript does not record the run 42 53 50 48 49 the comparison depends on" ;;
esac

# Each transcript has to print five runs for the compiled path, so a single
# number cannot stand in for a spread.
for pair in "before:$T9A" "after:$T9B"; do
    label=${pair%%:*}
    file=${pair#*:}
    craw=$(grep '^Vortex C' "$file" | head -n 1)
    case "$craw" in
        "") fail "the stage 9 $label transcript has no Vortex C row" ;;
    esac
    runs=$(printf '%s' "$craw" | grep -o '[0-9][0-9]*' | grep -c .)
    [ "$runs" -ge 5 ] ||
        fail "the stage 9 $label transcript prints $runs runs for the compiled path, fewer than five"
done

echo "STAGE9.md's figures appear in bench/results/stage9-before.txt and"
echo "bench/results/stage9-after.txt, and the stage 8 spread it compares against"
echo "is recorded in bench/results/stage8-compiled-full.txt."

echo "The check is satisfied."
