#!/bin/bash
# usage: cmp.sh [extra args] -- files
S=<一時領域>; V=$S/items/X-perform/verify
H=$S/bin/diktor-head; B=$V/v101/_build/default/bin/main.exe; N=$V/diktor/_build/default/bin/main.exe
args=(); while [ "$1" != "--" ]; do args+=("$1"); shift; done; shift
for f in "$@"; do b=$(basename $f); cd $(dirname $f)
 h=$($H "${args[@]}" $b 2>&1; echo "[exit $?]"); v=$($B "${args[@]}" $b 2>&1; echo "[exit $?]"); n=$($N "${args[@]}" $b 2>&1; echo "[exit $?]")
 echo "== $b ${args[*]}"; echo "H: $h"; [ "$v" = "$h" ] && echo "V: =H" || echo "V: $v"; [ "$n" = "$v" ] && echo "N: =V" || echo "N: $n"
done
