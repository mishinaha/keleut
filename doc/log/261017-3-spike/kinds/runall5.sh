#!/bin/bash
S=<一時領域>; K=<一時領域>/clusters/kinds
D="V124/ex V124/verify/ex V22/ex V22/ex2 V22/verify/ex V29/ex V29/verify/ex V30/ex V30/ex2 V30/verify/ex"
$K/run.sh $S/bin/diktor-head $S/items $D > $K/r.head.txt 2>&1
$K/run.sh $S/items/V22/merged2/diktor/_build/default/bin/main.exe $S/items $D > $K/r.v22v124.txt 2>&1
$K/run.sh $S/items/V30/diktor/_build/default/bin/main.exe $S/items $D > $K/r.v30.txt 2>&1
$K/run.sh $S/items/V29/ext2/diktor/_build/default/bin/main.exe $S/items $D > $K/r.v29ext.txt 2>&1
$K/run.sh ${BIN:-$K/diktor-core} $S/items $D > $K/r.${TAG:-core}.txt 2>&1
