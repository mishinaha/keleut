#!/bin/sh
# usage: run.sh <diktor exe> <case dir> [extra env]
EXE=$1; DIR=$2
cd "$DIR" || exit 1
for f in *.kel; do
  echo "=== $f"
  case $f in
    tail_*|nontail_*|resume_*) OCAMLRUNPARAM=l=100k "$EXE" "$f" 2>&1; echo "[rc=$?]";;
    *) "$EXE" "$f" 2>&1; echo "[rc=$?]";;
  esac
done
