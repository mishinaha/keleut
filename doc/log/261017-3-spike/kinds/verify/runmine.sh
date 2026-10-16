#!/bin/bash
S=<一時領域>; K=$S/clusters/kinds; V=$K/verify
D="V124/ex V124/verify/ex V22/ex V22/ex2 V22/verify/ex V29/ex V29/verify/ex V30/ex V30/ex2 V30/verify/ex"
$K/run.sh $V/diktor-final $S/items $D > $V/r.mine.txt 2>&1
diff -q $K/r.final.txt $V/r.mine.txt && echo SAME260 $(grep -c '^===' $V/r.mine.txt)
for i in V27 V31 V56 V57 V117; do
  DD=$(cd $S/items; ls -d $i/ex $i/ex2 $i/verify/ex 2>/dev/null)
  $K/run.sh $V/diktor-final $S/items $DD > $V/i.$i.mine.txt 2>&1
  diff -q $K/i.$i.final.txt $V/i.$i.mine.txt >/dev/null && echo "$i same $(grep -c '^===' $V/i.$i.mine.txt)"
done
