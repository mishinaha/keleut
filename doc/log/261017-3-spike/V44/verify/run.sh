#!/bin/sh
# usage: run.sh BINARY OUT
B=$1; OUT=$2; cd <一時領域>/items/V44/verify/exr
: > "$OUT"
for f in t19 t19b t19e t19f t19v a1 a1np a4 a7 a8 a8np a9 a9np a10 a10np b1 b1np b2 b3 b3np b4 b4np b5 b5np b6 b6np b7 b10 b11 b12 b13 b14 b14np c1 c1np c2 c3 n1 d1 d2 r1 r5; do
  { echo "==== $f.kel"; cat $f.kel; echo "---- $ diktor --type-check $f.kel"; "$B" --type-check $f.kel 2>&1; echo "[exit $?]"; } >> "$OUT"
done
