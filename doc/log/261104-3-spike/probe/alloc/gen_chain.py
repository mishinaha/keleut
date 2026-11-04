#!/usr/bin/env python3
"""V229 の測定に使う拡張の鎖のプログラムを書く。

使い方: gen_chain.py KIND N > OUT.kel

KIND:
  top   最上位に let r{i} = {a{i} = i extends r{i-1}} を並べる(ラベルがすべて違う)
  fun   関数の中で let s{i} = {a = i, b{i%7} = 1 extends s{i-1}} を並べ、s{n}.a を返す
  same  最上位に let s{i} = {a = i extends s{i-1}} を並べる(同じラベルを重ねる)
  func  fun と同じ鎖を、cancel 節を持つハンドラの中で呼ぶ(メモリの不足で cancel 節が走るかを見る)
  loop  再帰で同じラベル a を n 回重ねる(形は n 個。翻訳の大きさが n に依らない)
"""
import sys

kind, n = sys.argv[1], int(sys.argv[2])
w = sys.stdout.write
if kind == "top":
    w("let r0 = {}\n")
    for i in range(1, n + 1):
        w(f"let r{i} = {{a{i} = {i} extends r{i-1}}}\n")
    w(f"echoln(show(r{n}.a{n}))\n")
elif kind == "same":
    w("let s0 = {a = 0}\n")
    for i in range(1, n + 1):
        w(f"let s{i} = {{a = {i} extends s{i-1}}}\n")
    w(f"echoln(show(s{n}.a))\n")
elif kind == "fun":
    w("let f(): Int32 = {\n  let s0 = {a = 0}\n")
    for i in range(1, n + 1):
        w(f"  let s{i} = {{a = {i}, b{i % 7} = 1 extends s{i-1}}}\n")
    w(f"  s{n}.a\n}}\necholn(show(f()))\n")
elif kind == "func":
    w("effect Res = { use: () => Unit }\n")
    w("let f(): Int32 = {\n  let s0 = {a = 0}\n")
    for i in range(1, n + 1):
        w(f"  let s{i} = {{a = {i}, b{i % 7} = 1 extends s{i-1}}}\n")
    w(f"  s{n}.a\n}}\n")
    w("let main(): Int32 = run h { f() } handle { case use() => resume(()) case cancel => echoln(\"cancel\") }\n")
    w("echoln(\"before\")\necholn(show(main()))\n")
elif kind == "loop":
    # 行多相の再帰は型が無限になるので書けない。代わりに深さ n の再帰で同じ形を作り直す対照。
    w("let rec go(n: Int32, acc: Int32): Int32 = if n == 0 then acc else { let r = {a = n, b = acc}; go(n - 1, r.a + r.b - n + 1) }\n")
    w(f"echoln(show(go({n}, 0)))\n")
else:
    sys.exit("unknown kind")
