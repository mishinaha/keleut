#!/bin/bash
# usage: run129.sh BIN ITEMDIR
B=$1; D=$2
cd $D
grep '^=== ' results.spike.txt | sed 's/^=== //' | while read f o; do
  echo "=== $f $o"
  dir=$(dirname $f); base=$(basename $f)
  if [ "$o" = "--repl" ]; then (cd $dir && $B --repl < $base 2>&1; echo "[exit $?]")
  else (cd $dir && $B $o $base 2>&1; echo "[exit $?]"); fi
done
