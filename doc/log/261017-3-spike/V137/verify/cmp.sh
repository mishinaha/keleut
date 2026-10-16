#!/bin/bash
S=<一時領域>
H=$S/bin/diktor-head; N=$S/items/V137/verify/diktor/_build/default/bin/main.exe
O=$S/items/V137/verify/out; mkdir -p $O
for f in "$@"; do b=$(basename $f .kel)
  (cd $(dirname $f); $H $EXTRA $(basename $f) > $O/$b.head 2>&1; echo "exit $?" >> $O/$b.head; $N $EXTRA $(basename $f) > $O/$b.new 2>&1; echo "exit $?" >> $O/$b.new)
  if cmp -s $O/$b.head $O/$b.new; then echo "== $b SAME"; else echo "== $b DIFF"; diff $O/$b.head $O/$b.new | head -20; fi
done
