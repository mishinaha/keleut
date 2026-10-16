#!/bin/bash
# run.sh BIN OUT : run each ex/*.kel with BIN (run and --type-check)
W=<一時領域>/items/V50
B=$1; out=$2; : > $out
cd $W/ex
for f in *.kel; do
  echo "== $f" >> $out; sed 's/^/| /' $f >> $out
  echo "-- run" >> $out; timeout 10 $B $f >> $out 2>&1; echo "[exit $?]" >> $out
  echo "-- --type-check" >> $out; timeout 10 $B --type-check $f >> $out 2>&1; echo "[exit $?]" >> $out
done
