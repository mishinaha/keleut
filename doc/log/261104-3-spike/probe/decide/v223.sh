#!/bin/sh
# V223 の測定: 標準入力を読めない REPL(閉じた標準入力、ディレクトリの標準入力)。
set -u
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
for who in diktor keleut; do
  if [ $who = diktor ]; then c="$D --repl"; else c="$K repl"; fi
  echo "== $who 閉じた標準入力"; sh -c "exec $c <&-" 2>&1; echo "exit=$?"
  echo "== $who ディレクトリの標準入力"; sh -c "exec $c < /tmp" 2>&1; echo "exit=$?"
  echo "== $who 閉じた標準入力(--no-prelude)"; sh -c "exec $c --no-prelude <&-" 2>&1; echo "exit=$?"
done
echo "== diktor 閉じた標準入力のファイル実行(比較: 標準入力を読まない)"; printf 'echoln("a")\n' > /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/a.kel
sh -c "exec $D /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/a.kel <&-" 2>&1; echo "exit=$?"
