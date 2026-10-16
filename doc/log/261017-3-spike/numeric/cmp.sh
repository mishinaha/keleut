#!/bin/bash
S=<一時領域>
C=$S/clusters/numeric
NEW=$C/diktor/_build/default/bin/main.exe
for id in V114 V8 V91; do
  OLD=$S/items/$id/diktor/_build/default/bin/main.exe
  for dir in $S/items/$id/ex $S/items/$id/verify/ex; do
    [ -d $dir ] || continue
    cd $dir
    for f in *.kel; do
      for mode in "" "--type-check"; do
        a=$(timeout 10 $OLD $mode $f 2>&1; echo "[exit $?]")
        b=$(timeout 10 $NEW $mode $f 2>&1; echo "[exit $?]")
        if [ "$a" != "$b" ]; then echo "### $id ${dir#$S/items/} $mode $f"; diff <(echo "$a") <(echo "$b"); fi
      done
    done
  done
done
