#!/bin/sh
S=<一時領域>; V=$S/items/V24; H=$S/bin/diktor-head; P=$V/diktor/_build/default/bin/main.exe
f=$1; shift
a=$($H "$@" $f 2>&1; echo "[exit $?]"); b=$($P "$@" $f 2>&1; echo "[exit $?]"); echo "== $f $*"; echo "$a"; if [ "$a" = "$b" ]; then echo SAME; else echo DIFF; echo "$b"; fi
