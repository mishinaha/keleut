#!/bin/sh
# bytes.sh MODE NV [HI] : node --stack-size=102400 で RangeError にならない最大の N を二分探索し、
# 1 段(gen.py の N の 1 つ)あたりのバイト数 102400 KiB / N を出す。HI は探索の上限(既定 400000)。
P=$(dirname "$0"); K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=${TMPDIR:-/tmp}/gapsb-$$; mkdir -p "$D"
ok() {
  python3 "$P/gen.py" "$1" "$2" "$3" > "$D/p.kel"
  "$K" build --target wasm --max-depth 100000000 -o "$D/o" "$D/p.kel" >/dev/null 2>&1 || { echo build-fail >&2; exit 2; }
  out=$( (ulimit -s 300000; KELEUT_WASM_DIRECT=1 node --stack-size=102400 "$D/o/main.mjs") 2>&1); rc=$?
  [ $rc -eq 0 ] && return 0
  echo "  N=$3 rc=$rc $(printf '%s' "$out" | tail -c 200 | tr '\n' '|')" >&2
  return 1
}
lo=1; hi=${3:-400000}
if ok "$1" "$2" "$hi"; then echo "$1 nv=$2: N=$hi に届いた(<= $((102400*1024/hi)) バイト/N)"; rm -rf "$D"; exit 0; fi
while [ $((hi - lo)) -gt $((hi / 200 + 1)) ]; do
  mid=$(((lo + hi) / 2))
  if ok "$1" "$2" "$mid" 2>/dev/null; then lo=$mid; else hi=$mid; fi
done
echo "$1 nv=$2: 最大の N 約 $lo、$((102400*1024/lo)) バイト/N"
rm -rf "$D"
