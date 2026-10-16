#!/bin/bash
# usage: cmp.sh H P files...
H=$1; P=$2; shift 2
for f in "$@"; do
  d=$(dirname $f); b=$(basename $f)
  ho=$(cd $d && timeout 10 $H $FLAGS $b 2>&1; echo "[exit $?]")
  po=$(cd $d && timeout 10 $P $FLAGS $b 2>&1; echo "[exit $?]")
  if [ "$ho" == "$po" ]; then echo "SAME $b: $(echo "$ho" | tail -2 | tr '\n' ' ' | cut -c1-200)"; else echo "DIFF $b"; echo "  H: $ho"; echo "  P: $po"; fi
done
