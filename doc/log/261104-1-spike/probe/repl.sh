#!/bin/bash
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
f=$1; cd "$(dirname "$f")"; n=$(basename "$f")
a=$(timeout 30 $D --repl < $n 2>/dev/null; echo "[$?]")
b=$(timeout 30 $K repl < $n 2>/dev/null; echo "[$?]")
[ "$a" != "$b" ] && { echo "== $f"; diff <(echo "$a") <(echo "$b"); }
exit 0
