#!/bin/bash
S=<一時領域>
V=$S/items/V108/verify
for f in "$@"; do
  echo "### $f"; echo "--- HEAD"; $S/bin/diktor-head --repl < $f 2>&1; echo "[rc=$?]"
  echo "--- PROTO"; $V/diktor/_build/default/bin/main.exe --repl < $f 2>&1; echo "[rc=$?]"
done
