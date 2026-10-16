#!/bin/bash
S=<一時領域>; V=$S/items/V47/verify
H=$S/bin/diktor-head; A=$V/diktor-allbase/_build/default/bin/main.exe; N=$V/diktor-all/_build/default/bin/main.exe
cd $V/ex
for f in "$@"; do
  for b in H A N; do eval bin=\$$b; out=$($bin $f 2>&1); echo "[$b $f exit $?] $out"; done
done
