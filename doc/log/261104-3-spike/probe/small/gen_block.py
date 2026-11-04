#!/usr/bin/env python3
"""N 文のブロック { let y_0 = 0; ...; y_{N-1} } を 1 つ書く(M98 の 2 つ目の大きい入力と同じ形)。"""
import sys
n = int(sys.argv[1])
print("let r = {")
for i in range(n):
    print(f"  let y_{i} = {i};")
print(f"  y_{n-1}")
print("}")
print("echoln(show(r))")
