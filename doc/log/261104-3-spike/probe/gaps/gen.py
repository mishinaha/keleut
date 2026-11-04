#!/usr/bin/env python3
"""cancel 節の中に直に書いたハンドラ式を重ねる探りのプログラムを書き出す。

使い方: gen.py MODE NV N
  MODE a: cancel 節の中に直に N の節のハンドラ式を書き、その本体が go(n - 1) を呼ぶ。
  MODE c: a に加えて、go のハンドラ式と、N の節のハンドラ式の本体の go の呼び出しを末尾でなくし、
          go、cancel 節、本体のそれぞれで NV 個の値を呼び出しをまたいで生かす(1 段に重なるフレームをすべて重くする)。
  MODE d: lv(n) が NV 個の値を生かして N の節のハンドラ式を末尾でなく書き、その本体が NV 個の値を生かして
          cancel 節つきのハンドラ式を末尾でなく書き、その cancel 節が lv(n - 1) を末尾で呼ぶ
          (論理的な 1 段に lv、$rt_handle、本体、$rt_handle を重ねる)。
  MODE e: 対照。c から cancel 節を除き、go が NV 個の値を生かしてハンドラ式を末尾でなく書き、
          その本体が NV 個の値を生かして go(m - 1) を末尾でなく呼ぶ。
  MODE b: a に加えて、本体が go を呼ばず、本体の中に直に cancel 節つきのハンドラ式を書く(rec 関数は lv)。
  環境変数 GADD=1 なら、G の節のハンドラ式を 1 つ足す(プログラムに G の節があると、すべての関数に取り出しの区切りが付く)。
  NV: cancel 節で再帰の呼び出しをまたいで生かす値の数(0 なら値を生かさない)。
  N: 重ねる段の数。
"""
import sys

mode, nv, n = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])

def lets(var):
    if nv == 0:
        return "", "0"
    ls = "".join(f"let a{i} = {var} * {i + 3}; " for i in range(nv))
    s = " + ".join(f"a{i}" for i in range(nv))
    return ls, f"({s}) - ({s})"

out = []
out.append("effect Stop = { stop: () => Int32 }")
out.append("effect Zz = { zz: () => Unit }")
import os
if os.environ.get("GADD") == "1":
    out.append("effect Ask = { ask: () => Int32 }")
    out.append("echoln(show((perform ask()) handle { case ask() => { let r = resume(1); r + 1 } }))")
ls, chk = lets("n")
if mode == "a":
    out.append(
        "let rec go(n: Int32): Int32 @ {Stop, Console} = if n == 0 then { echoln(\"bottom\"); 0 } else "
        "(perform stop()) handle { case zz() => resume(()) "
        f"case cancel => {{ {ls}let r = go(n - 1) handle {{ case stop() => 0 }}; "
        f"if r + ({chk}) != 0 then echoln(\"bad\") else () }} }}")
elif mode == "b":
    # 本体の中に直に cancel 節つきのハンドラ式を書く。
    out.append(
        "let rec go(n: Int32): Int32 @ {Stop, Console} = if n == 0 then { echoln(\"bottom\"); 0 } else "
        "(perform stop()) handle { case zz() => resume(()) "
        f"case cancel => {{ {ls}let r = (if n == 1 then 0 else ((perform stop()) handle {{ case zz() => resume(()) "
        f"case cancel => {{ let r2 = go(n - 2) handle {{ case stop() => 0 }}; if r2 != 0 then echoln(\"bad\") else () }} }})) handle {{ case stop() => 0 }}; "
        f"if r + ({chk}) != 0 then echoln(\"bad\") else () }} }}")
elif mode == "c":
    lm, cm = lets("m")
    lk, ck = lets("k")
    out.append(
        "let rec go(m: Int32): Int32 @ {Stop, Console} = if m == 0 then { echoln(\"bottom\"); 0 } else "
        f"{{ {lm}let q = (perform stop()) handle {{ case zz() => resume(()) "
        f"case cancel => {{ {ls.replace('n *', 'm *')}let r = {{ let k = m - 1; {lk}let z = go(k); z + ({ck}) }} handle {{ case stop() => 0 }}; "
        f"if r + ({chk}) != 0 then echoln(\"bad\") else () }} }}; q + ({cm}) }}")
elif mode == "d":
    lk, ck = lets("k")
    out.append(
        "let rec lv(n: Int32): Unit @ {Console} = if n == 0 then echoln(\"bottom\") else "
        f"{{ {ls}let r = {{ let k = n + 1; {lk}let q = (perform stop()) handle {{ case zz() => resume(()) "
        "case cancel => lv(n - 1) }; "
        f"q + ({ck}) }} handle {{ case stop() => 0 }}; "
        f"if r + ({chk}) != 0 then echoln(\"bad\") else () }}")
    out.append(f"lv({n})")
    print("\n".join(out)); raise SystemExit(0)
elif mode == "e":
    lm, cm = lets("m")
    lk, ck = lets("k")
    out.append(
        "let rec go(m: Int32): Int32 @ {Stop, Console} = if m == 0 then { echoln(\"bottom\"); 0 } else "
        f"{{ {lm}let q = {{ let k = m - 1; {lk}let z = go(k); z + ({ck}) }} handle {{ case zz() => resume(()) }}; q + ({cm}) }}")
else:
    raise SystemExit("MODE must be a or b")
out.append(f"echoln(show(go({n}) handle {{ case stop() => 7 }}))")
print("\n".join(out))
