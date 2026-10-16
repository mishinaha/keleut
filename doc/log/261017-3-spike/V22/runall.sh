#!/bin/bash
# usage: runall.sh BIN DIR...
B=$1; shift
for d in "$@"; do
  for f in "$d"/*.kel "$d"/*.txt; do
    [ -e "$f" ] || continue
    n=$(basename "$f")
    case "$n" in r8_prelude.kel|results*|*.log) continue;; esac
    cd "$d"
    if [ "$n" = r9_repl.txt ]; then out=$("$B" --repl < "$n" 2>&1); rc=$?
    elif [ "$n" = r8_main.kel ]; then out=$("$B" --prelude r8_prelude.kel "$n" 2>&1); rc=$?
    elif grep -q 'let main' "$n"; then out=$("$B" "$n" 2>&1); rc=$?
    elif grep -q '^type Unit = {}' "$n"; then out=$("$B" --type-check --no-prelude "$n" 2>&1); rc=$?
    else out=$("$B" --type-check "$n" 2>&1); rc=$?; fi
    echo "=== $d/$n"; echo "$out" | head -4; echo "[exit $rc]"
    cd - >/dev/null
  done
done
