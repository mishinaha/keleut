#!/bin/sh
# REPL の表示の書式を両処理系で比べる。
P=/home/sumito3478/repo/keleut/doc/log/261104-3-spike/probe/decide
K=/home/sumito3478/repo/keleut/rustkeleut/target/release/keleut
D=/home/sumito3478/repo/keleut/diktor/_build/install/default/bin/diktor
"$D" --repl < "$P/display.txt" > /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/d.out 2>&1; echo "diktor exit=$?"
"$K" repl < "$P/display.txt" > /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/k.out 2>&1; echo "keleut exit=$?"
echo "== diktor"; cat /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/d.out
echo "== diff diktor keleut"; diff /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/d.out /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/decide/k.out && echo same
