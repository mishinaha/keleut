#!/bin/bash
f=$1; A=$2; B=$3
d=$(dirname "$f"); b=$(basename "$f")
ho=$(cd "$d" && timeout 5 $A --type-check "$b" </dev/null 2>&1; echo "rc $?")
po=$(cd "$d" && timeout 5 $B --type-check "$b" </dev/null 2>&1; echo "rc $?")
[ "$ho" == "$po" ] || echo "DIFF $f"
