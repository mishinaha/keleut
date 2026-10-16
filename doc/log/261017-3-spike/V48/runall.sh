#!/bin/bash
S=<一時領域>
cd $S/items/V48/ex
run() { # $1 binary, $2 out
  : > $2
  for f in params lambdas matches binds rets absurd elim rec keep guard; do
    { echo "\$ diktor --type-check $f.kel"; "$1" --type-check $f.kel 2>&1; echo "[exit $?]"; } >> $2
  done
  { echo "\$ diktor run.kel"; "$1" run.kel 2>&1; echo "[exit $?]"; echo "\$ diktor --strict-exhaustive run.kel"; "$1" --strict-exhaustive run.kel 2>&1; echo "[exit $?]"; } >> $2
}
run $S/bin/diktor-head $S/items/V48/results.head.txt
run $S/all/diktor/_build/default/bin/main.exe $S/items/V48/results.all.txt
run $S/items/V48/diktor/_build/default/bin/main.exe $S/items/V48/results.proto.txt
