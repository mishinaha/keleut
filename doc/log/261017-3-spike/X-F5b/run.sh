#!/bin/bash
# usage: run.sh BIN OUT
B=$1; OUT=$2; cd "$(dirname "$0")/ex"
lits='1x 1.2.3 0x 0x_1 0xg 0b102 0b2 0o8 0o 1i 1e 1e+ 1abc 1_ 0xff_ 1. 1.5.3 1._1 1.foo 1u8x 1i32x 0xffz 1f 1.5e 2abc 1E 1u 0b 1i64 0xff 1e5 1.e5 1__2 0xffi8'
n=0
: > $OUT
for l in $lits; do n=$((n+1)); f=lit$n.kel; printf 'let a = %s\nlet b = 2\n' "$l" > $f
 { echo "=== $f: let a = $l (2 行目 let b = 2)"; printf '$ --dump-tokens: '; $B --dump-tokens $f 2>&1 | awk '{printf "%s ", $2}'; echo; $B --type-check $f 2>&1; echo "[exit $?]"; } >> $OUT; done
