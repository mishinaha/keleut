#!/bin/bash
# V229: 拡張の鎖の長さに対する時間と最大の常駐の大きさを、各後段と Diktor で測る。
# 使い方: run_chain.sh WORKDIR "KIND N" ... (KIND は gen_chain.py の種類)
# 環境変数 BACKENDS(既定 "interp jit wasm")、DK=1 で diktor --run-ir も測る。
set -u
P=$(cd "$(dirname "$0")" && pwd)
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
W=$1; shift
cd "$W"
for spec in "$@"; do
  set -- $spec
  f=$1_$2
  python3 "$P/gen_chain.py" "$1" "$2" > "$f.kel"
  echo "== $f emit-kir: $(python3 "$P/measure.py" -- "$K" emit-kir -o "$f.kir" "$f.kel")"
  for b in ${BACKENDS:-interp jit wasm}; do
    echo "  $b: $(python3 "$P/measure.py" --timeout 300 -- "$K" run --backend "$b" "$f.kir")"
  done
  if [ "${DK:-0}" = 1 ]; then
    echo "  dk-ir: $(python3 "$P/measure.py" --timeout 300 -- "$D" --run-ir "$f.kir")"
  fi
  rm -f "$f.kel" "$f.kir"
done
