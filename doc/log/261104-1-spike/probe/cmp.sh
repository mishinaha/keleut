#!/bin/bash
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
f=$1
dir=$(dirname "$f"); n=$(basename "$f")
cd "$dir"
de=$(timeout 20 $D "$n" 2>&1 >/dev/null </dev/null; echo "[$?]")
ke=$(timeout 20 $K run --backend interp "$n" 2>&1 >/dev/null </dev/null; echo "[$?]")
if [ "$de" != "$ke" ]; then
  printf '=== %s\nD: %s\nK: %s\n' "$f" "$de" "$ke"
fi
