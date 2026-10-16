#!/bin/bash
S=<一時領域>; K=$S/clusters/kinds; B=$K/diktor/_build/default/bin/main.exe
D="V124/ex V124/verify/ex V22/ex V22/ex2 V22/verify/ex V29/ex V29/verify/ex V30/ex V30/ex2 V30/verify/ex"
$K/run.sh $B $S/items $D > $K/r.final2.txt 2>&1
cmp -s $K/r.final.txt $K/r.final2.txt && echo "260 same" || diff $K/r.final.txt $K/r.final2.txt
echo "count $(grep -c '^===' $K/r.final2.txt)"
for d in $D; do echo "$d $(grep -c "^=== $d/[^/]*\$" $K/r.final2.txt)"; done
for i in V27 V31 V56 V57 V117; do
  DD=$(cd $S/items; ls -d $i/ex $i/ex2 $i/verify/ex 2>/dev/null)
  $K/run.sh $B $S/items $DD > $K/i.$i.final2.txt 2>&1
  n=$(grep -c '^===' $K/i.$i.final2.txt)
  cmp -s $K/i.$i.final.txt $K/i.$i.final2.txt && echo "$i same $n" || { echo "$i differs $n"; diff $K/i.$i.final.txt $K/i.$i.final2.txt; }
done
