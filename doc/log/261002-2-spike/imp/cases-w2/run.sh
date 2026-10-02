#!/bin/sh
# 試作ビルド(../imp-spike.patch)で実行する。可視性は未実装で、単位を帰りがけ順に連結するだけ
D=/tmp/claude-1000/-home-sumito3478-repo-keleut/8fd1dfd3-6874-4981-8f61-9d2478ad4a92/scratchpad/imp/diktor/_build/default/bin/main.exe
for f in main cyc_a nf sp col abs ext deep late semis semis2 nl nl2 empty split wth cyc_self kw only; do
  echo "\$ diktor $f.kel"; $D $f.kel; echo "exit: $?"
done
echo '$ diktor --import-path . --type-check sp.kel'; $D --import-path . --type-check sp.kel; echo "exit: $?"
echo '$ diktor --import-path . --import-path alt sp.kel'; $D --import-path . --import-path alt sp.kel; echo "exit: $?"
