#!/bin/bash
# run.sh BIN DIR OUT
B=$1; dir=$2; out=$3; : > $out
cd $dir
for f in *.kel; do
  echo "== $f" >> $out; sed 's/^/| /' $f >> $out
  echo "-- run" >> $out; timeout 10 $B $f >> $out 2>&1; echo "[exit $?]" >> $out
  echo "-- --type-check" >> $out; timeout 10 $B --type-check $f >> $out 2>&1; echo "[exit $?]" >> $out
done
