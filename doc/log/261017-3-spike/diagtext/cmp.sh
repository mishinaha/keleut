#!/bin/bash
S=<一時領域>
C=$S/clusters/diagtext
CB=$C/diktor/_build/default/bin/main.exe
for i in V102 V111 V33 V34 X-premise V24; do
  IB=$S/items/$i/diktor/_build/default/bin/main.exe
  for d in $S/items/$i/ex $S/items/$i/ex2 $S/items/$i/verify/ex; do
    [ -d $d ] || continue
    for f in $(cd $d && ls *.kel 2>/dev/null); do
      a=$(cd $d && timeout 10 $IB $f 2>&1; echo "exit=$?")
      b=$(cd $d && timeout 10 $CB $f 2>&1; echo "exit=$?")
      h=$(cd $d && timeout 10 $S/bin/diktor-head $f 2>&1; echo "exit=$?")
      if [ "$a" != "$b" ]; then echo "### DIFF $i ${d#$S/items/} $f"; echo "head: $h" | grep -E '!|exit'; echo "item: $a" | grep -E '!|exit'; echo "comb: $b" | grep -E '!|exit'; fi
    done
  done
done
