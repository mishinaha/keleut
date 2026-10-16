#!/bin/bash
# run.sh LIMIT FILE [args...]: HEAD と試作で走らせ、出力の先頭と終了状態を出す
S=<一時領域>
L=$1; shift
for b in $S/bin/diktor-head $S/items/X-E13/diktor/_build/default/bin/main.exe; do
  out=$(OCAMLRUNPARAM=l=$L $b "$@" 2>&1); rc=$?
  echo "[$(basename $b)] $(echo "$out" | head -3 | cut -c1-120 | tr '\n' '|') exit=$rc"
done
