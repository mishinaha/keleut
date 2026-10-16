#!/bin/bash
S=<一時領域>
W=$S/clusters/extra
BASE=$S/all/diktor/_build/default/bin/main.exe
NEW=$W/diktor/_build/default/bin/main.exe
for i in V47 V50 V44 V49 V53 V48; do
  P=$S/items/$i/diktor/_build/default/bin/main.exe
  for d in ex exb ex2 verify/ex; do
    [ -d $S/items/$i/$d ] || continue
    ( cd $S/items/$i/$d
      for f in $(ls *.kel 2>/dev/null | sort -V); do
        for k in base proto new; do
          case $k in base) B=$BASE;; proto) B=$P;; new) B=$NEW;; esac
          o=$(timeout 20 $B --type-check $f 2>&1); c=$?
          printf '%s\n[exit %s]\n' "$o" "$c" > $W/cmp/$i.$(echo $d|tr / _).$f.$k
        done
        b=$W/cmp/$i.$(echo $d|tr / _).$f
        s1=same; cmp -s $b.base $b.proto || s1=diff
        s2=same; cmp -s $b.proto $b.new || s2=DIFF
        echo "$i $d $f base-vs-proto=$s1 proto-vs-new=$s2"
      done )
  done
done
