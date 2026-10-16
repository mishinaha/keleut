#!/bin/bash
# usage: run.sh BIN dir files...
B=$1; shift; D=$1; shift
cd $D
for f in "$@"; do for o in "" --strict-exhaustive --type-check; do echo "=== $o $f"; $B $o $f 2>&1; echo "[exit $?]"; done; done
