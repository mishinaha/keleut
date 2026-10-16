#!/bin/bash
# usage: run.sh BIN DIR OUT
BIN=$1; DIR=$2; OUT=$3
: > $OUT
for f in $(ls $DIR/*.kel 2>/dev/null); do
  b=$(basename $f)
  for m in "--type-check" "--type-check --no-prelude" ""; do
    echo "## $b [$m]" >> $OUT
    (cd $DIR && timeout 5 $BIN $m $b < /dev/null >> $OUT 2>&1; echo "[exit $?]" >> $OUT)
  done
done
