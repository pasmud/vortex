#!/bin/sh
# Guards the promise in BENCHMARKS.md that no result has been recorded yet.
#
# While that is true, the file must contain the NO RESULTS YET marker. The check
# exists so that the moment a real measurement is added, the marker has to be
# removed deliberately, together with the machine and the raw output, rather
# than the two drifting apart.
#
# A later stage relaxes this once stage 2 has a Vortex row to record.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FILE="$ROOT/BENCHMARKS.md"

if [ ! -f "$FILE" ]; then
    echo "BENCHMARKS.md is missing" >&2
    exit 1
fi

if grep -q '^## NO RESULTS YET' "$FILE"; then
    echo "BENCHMARKS.md still reports no results, which is correct for this stage."
    exit 0
fi

echo "BENCHMARKS.md no longer says NO RESULTS YET." >&2
echo "When adding a measurement, record the machine, the compiler versions and" >&2
echo "the raw bench/run.sh output in the same change." >&2
exit 1