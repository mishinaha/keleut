#!/bin/sh
# run1.sh MODE NV N BACKEND [extra keleut run options] : 1 回走らせ、出力の先頭と末尾と終了状態を 1 行で出す
P=$(dirname "$0"); K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
T=${TMPDIR:-/tmp}/gaps-$$.kel
python3 "$P/gen.py" "$1" "$2" "$3" > "$T"
m=$1 nv=$2 n=$3 b=$4; shift 4
s=$(date +%s.%N)
o=$("$K" run --backend "$b" "$@" "$T" 2>&1); rc=$?
e=$(date +%s.%N)
printf '%s nv=%s N=%s %s %s: rc=%s out=[%s] %ss\n' "$m" "$nv" "$n" "$b" "$*" "$rc" "$(printf '%s' "$o" | head -c 300 | tr '\n' '|')" "$(awk "BEGIN{printf \"%.2f\", $e - $s}")"
rm -f "$T"
