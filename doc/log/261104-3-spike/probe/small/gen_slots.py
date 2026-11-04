#!/usr/bin/env python3
"""N 個の値を生かしたまま末尾でない呼び出しを挟む関数を書く(スロットが 256 を超える関数)。"""
import sys
n = int(sys.argv[1])
print("let id(x: Int32): Int32 = x")
print("let big(k: Int32): Int32 = {")
for i in range(n):
    print(f"  let v{i} = id(k + {i})")
print("  " + " + ".join(f"v{i}" for i in range(n)))
print("}")
print("echoln(show(big(1)))")
print("echoln(show(big(2)))")
