#!/bin/sh
# usage: runprobes.sh dir binA binB
cd "$1"
for f in *.kel; do
  echo "##### $f"
  for b in "$2" "$3"; do
    echo "--- $(basename $b) tc"; "$b" --type-check "$f" 2>&1; echo "rc=$?"
    echo "--- $(basename $b) run"; "$b" "$f" 2>&1 | tail -3; echo "rc=$?"
  done
done
