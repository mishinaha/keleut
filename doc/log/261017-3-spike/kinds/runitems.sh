#!/bin/bash
S=<一時領域>; K=<一時領域>/clusters/kinds
for i in V27 V31 V56 V57 V117; do
  D=$(cd $S/items; ls -d $i/ex $i/ex2 $i/verify/ex 2>/dev/null)
  $K/run.sh $S/bin/diktor-head $S/items $D > $K/i.$i.head.txt 2>&1
  $K/run.sh $S/items/$i/diktor/_build/default/bin/main.exe $S/items $D > $K/i.$i.proto.txt 2>&1
  $K/run.sh $K/diktor-final $S/items $D > $K/i.$i.final.txt 2>&1
done
