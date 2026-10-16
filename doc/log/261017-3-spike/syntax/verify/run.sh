#!/bin/bash
# usage: run.sh exe dir  -> "$ diktor f" / output / "exit: N" for each .kel
B=$1; cd "$2" || exit 1
for f in $(ls *.kel); do echo "\$ diktor $f"; timeout 20 $B $f 2>&1; echo "exit: $?"; done
