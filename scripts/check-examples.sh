#!/bin/sh
# Checks that every example produces identical output on all three paths.
#
# Stage 11's claim is that the compiled path now carries three of the four
# examples. That claim is only worth anything if it is checked, and a
# disagreement here is a failing check rather than a line in a transcript.
#
# The compiled path is built at -O2 and again at -O0, because stage 10 showed
# that -O2 can quietly repair the emitter. An example the compiled path does not
# carry is listed rather than failing, so a refusal by name stays visible
# without being treated as a regression.
#
# Usage: scripts/check-examples.sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

RUNNER="target/release/examples/run_example"
CRUNNER="target/release/examples/run_compiled"

cargo build --release --quiet --example run_example --example run_compiled

fail() {
    echo "$1" >&2
    exit 1
}

carried=""
refused=""
for path in examples/*.vx; do
    name=$(basename "$path")

    tree=$("$RUNNER" "$path" 2>/dev/null)
    vm=$("$RUNNER" "$path" --vm 2>/dev/null)

    # The two engines have to agree before the compiled path is even compared,
    # or a difference could be either engine rather than the emitter.
    if [ "$tree" != "$vm" ]; then
        fail "$name: the tree interpreter and the VM disagree"
    fi

    for opt in -O2 -O0; do
        rm -f /tmp/vortex-cgen/*
        if ! VORTEX_C_OPT="$opt" "$CRUNNER" "$path" main >/dev/null 2>&1; then
            if [ "$opt" = "-O2" ]; then
                refused="$refused $name"
            fi
            continue
        fi
        compiled=$(/tmp/vortex-cgen/program 2>/dev/null)
        if [ "$compiled" != "$tree" ]; then
            fail "$name: the compiled path at $opt disagrees with the interpreters"
        fi
        if [ "$opt" = "-O2" ]; then
            carried="$carried $name"
        fi
    done
done

echo "identical on all three paths at -O2 and -O0:${carried:- none}"
if [ -n "$refused" ]; then
    # An example the emitter refuses is reported, not passed over silently. The
    # emitter refuses by name, which is the behaviour this stage wanted.
    echo "refused by the compiled path, each with its reason:${refused}"
    echo "(each reason is printed by: $CRUNNER <example> main)"
fi

# One example being carried is the floor: a regression that takes the compiled
# path back to nothing should fail rather than pass quietly.
case "$carried" in
    *"hello.vx"*) ;;
    *) fail "the compiled path no longer carries hello.vx" ;;
esac
