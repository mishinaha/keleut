#!/bin/bash
# usage: run.sh dir files...
S=<一時領域>
V=$S/items/V29/verify
H=$S/bin/diktor-head
P=$V/diktor/_build/default/bin/main.exe
X=$V/ext/diktor/_build/default/bin/main.exe
for f in "$@"; do
  opts=()
  if grep -q '^type Unit = {}' "$f"; then opts+=(--no-prelude); fi
  if ! grep -q 'with_stdout\|^println\|^[a-z_]*(' "$f"; then opts+=(--type-check); fi
  echo "### $(basename $f) ${opts[*]}"
  for b in H P X; do
    bin=${!b}
    out=$(cd $(dirname $f) && timeout 10 $bin "${opts[@]}" $(basename $f) 2>&1); rc=$?
    echo "$b: $(echo "$out" | tr '\n' '|') [rc=$rc]"
  done
done
