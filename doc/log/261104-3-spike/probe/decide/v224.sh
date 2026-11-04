#!/bin/sh
# V224 の測定: 入れ子 N 段の式を keleut repl / diktor --repl / keleut run / diktor (木) / diktor --emit-ir に与える。
# 使い方: sh v224.sh WORKDIR   (出力の各行は 120 字で切る。exit= は各コマンドの終了状態)
set -u
W=$1; mkdir -p "$W"; cd "$W"
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
run() { "$@" > o.txt 2>&1; s=$?; sed "s|$W/||g" o.txt | cut -c1-120; echo "exit=$s"; }
for n in 99999 100000 150000 199999; do
  awk -v n=$n 'BEGIN { print "let y = 1"; s = "1"; for (i = 0; i < n; i++) s = "1 + (" s ")"; print "let x = " s; print "echoln(show(y))"; print "x + y" }' > "repl$n.txt"
  awk -v n=$n 'BEGIN { s = "1"; for (i = 0; i < n; i++) s = "1 + (" s ")"; print "let x = " s; print "echoln(show(x))" }' > "file$n.kel"
  echo "== n=$n keleut repl"; run sh -c "'$K' repl < repl$n.txt"
  echo "== n=$n diktor --repl"; run sh -c "'$D' --repl < repl$n.txt"
  echo "== n=$n keleut run"; run "$K" run "file$n.kel"
  echo "== n=$n diktor"; run "$D" "file$n.kel"
  echo "== n=$n diktor --emit-ir"; run "$D" --emit-ir "o$n.kir" "file$n.kel"
  rm -f "o$n.kir"
done
