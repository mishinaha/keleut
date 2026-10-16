#!/bin/bash
# usage: run.sh <diktor> <exdir>
B=$1; E=$2
for f in $(ls $E/*.kel | sort -V); do
  echo "== $(basename $f)"; sed 's/^/  | /' $f
  out=$($B --type-check $f 2>&1); c=$?
  echo "$out" | sed "s|$E/||"; echo "  [exit $c]"
done
