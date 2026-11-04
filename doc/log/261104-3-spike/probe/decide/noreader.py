# 読み口を閉じたパイプを標準出力にして子を走らせる(読み手のいないパイプ)。5 秒で打ち切る。
# 使い方: python3 noreader.py CMD ARGS...
import os, subprocess, sys
r, w = os.pipe()
os.close(r)
p = subprocess.Popen(sys.argv[1:], stdout=w, stderr=subprocess.PIPE)
os.close(w)
try:
    _, err = p.communicate(timeout=5)
    print("exit", p.returncode, err.decode().strip())
except subprocess.TimeoutExpired:
    p.kill(); p.wait()
    print("timeout (5 s)")
