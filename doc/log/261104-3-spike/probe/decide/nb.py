# bytes.t:35 と同じ。標準出力を非ブロッキングのパイプにして子を走らせ、1 秒後に読み始める。
# 使い方: python3 nb.py CMD ARGS...   出力: 子の終了状態 と 読んだバイト数(子の標準エラーはそのまま)
import fcntl, os, subprocess, sys, time
r, w = os.pipe()
fcntl.fcntl(w, fcntl.F_SETFL, fcntl.fcntl(w, fcntl.F_GETFL) | os.O_NONBLOCK)
stdin = open(sys.argv[2]) if sys.argv[1] == "-i" else None
argv = sys.argv[3:] if stdin else sys.argv[1:]
p = subprocess.Popen(argv, stdout=w, stdin=stdin)
os.close(w)
time.sleep(1)
n = 0
while True:
    b = os.read(r, 65536)
    if not b:
        break
    n += len(b)
print(p.wait(), n)
