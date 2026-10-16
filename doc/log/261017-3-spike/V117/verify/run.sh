#!/bin/bash
# usage: run.sh bin label dir files...
bin=$1; label=$2; dir=$3; shift 3
cd $dir
for f in "$@"; do
  echo "=== $f"; cat $f
  echo "--- \$ $label $f"
  $bin $f 2>&1; echo "[exit $?]"
done
