#!/bin/sh
# usage: runex.sh <diktor-binary> <dir-of-kel>
B=$1; D=$2
for f in "$D"/*.kel; do
  n=$(basename "$f" .kel)
  out=$("$B" --type-check "$f" 2>&1); rc=$?
  echo "== $n rc=$rc"
  echo "$out" | sed "s#$D/##g" | sed 's/^/   /'
done
