#!/bin/bash
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
f=$1; cd "$(dirname "$f")"
e=$(timeout 20 $D "$(basename "$f")" 2>&1 >/dev/null </dev/null)
case "$e" in *"実行時エラー"*) printf '%s\t%s\n' "$f" "$(echo "$e" | grep 実行時エラー | head -1)";; esac
