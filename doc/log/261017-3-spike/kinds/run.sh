#!/bin/bash
# usage: run.sh BIN ITEMSDIR DIR...
B=$1; I=$2; shift 2
for d in "$@"; do
  for f in "$I/$d"/*.kel; do
    [ -e "$f" ] || continue
    n=$(basename "$f")
    case "$n" in r8_prelude.kel) continue;; esac
    cd "$I/$d"
    if [ "$n" = r8_main.kel ]; then out=$("$B" --prelude r8_prelude.kel "$n" 2>&1); rc=$?
    elif grep -q '^main()\|^let main\|^  *main()' "$n" && ! grep -q '^type Unit = {}' "$n"; then out=$("$B" "$n" 2>&1); rc=$?
    elif grep -q '^type Unit = {}' "$n"; then out=$("$B" --type-check --no-prelude "$n" 2>&1); rc=$?
    else out=$("$B" --type-check "$n" 2>&1); rc=$?; fi
    echo "=== $d/$n"; echo "$out" | head -4; echo "[exit $rc]"
  done
done
