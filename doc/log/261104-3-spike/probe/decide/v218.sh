#!/bin/sh
# V218 の測定: 非ブロッキングの標準出力で、diktor の各道と keleut を走らせる。
# 使い方: sh v218.sh WORKDIR
set -u
W=$1; mkdir -p "$W"; cd "$W"
P=/home/sumito3478/repo/keleut/doc/log/261104-3-spike/probe/decide
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
"$D" --emit-ir big.kir "$P/big.kel"
echo "== diktor (木)"; python3 "$P/nb.py" "$D" "$P/big.kel" 2>&1 | cut -c1-200
echo "== diktor --run-ir"; python3 "$P/nb.py" "$D" --run-ir big.kir 2>&1 | cut -c1-200
echo "== diktor --repl"; python3 "$P/nb.py" -i "$P/bigrepl.txt" "$D" --repl 2>&1 | cut -c1-200
echo "== keleut run"; python3 "$P/nb.py" "$K" run "$P/big.kel" 2>&1 | cut -c1-200
echo "== keleut run --backend wasm"; python3 "$P/nb.py" "$K" run --backend wasm "$P/big.kel" 2>&1 | cut -c1-200
echo "== keleut repl"; python3 "$P/nb.py" -i "$P/bigrepl.txt" "$K" repl 2>&1 | cut -c1-200
echo "== diktor --via-ir"; python3 "$P/nb.py" "$D" --via-ir "$P/big.kel" 2>&1 | cut -c1-200
