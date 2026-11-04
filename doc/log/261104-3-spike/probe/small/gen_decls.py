#!/usr/bin/env python3
"""N 個のトップレベルの宣言 let x_i = i + 1 を書き、続けて TAIL を足す。
使い方: gen_decls.py N [tail]  (tail は none か list)
list は 200 万要素の Int32 のリストを作って長さを数える(実行時のヒープを使う)。"""
import sys
n = int(sys.argv[1])
tail = sys.argv[2] if len(sys.argv) > 2 else "none"
for i in range(n):
    print(f"let x_{i} = {i} + 1")
if tail == "list":
    print("let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))")
    print("let rec len(xs: List[Int32], k: Int32): Int32 = xs match { case Nil => k case Cons(_, t) => len(t, k + 1) }")
    print("echoln(show(len(build(2000000, Nil), 0)))")
print(f"echoln(show(x_{n-1}))")
