# 子が終わった後に、親が持つ同じ書き込み口の O_NONBLOCK が残っているかを見る。
# 使い方: python3 nbflag.py CMD ARGS...
import fcntl, os, subprocess, sys
r, w = os.pipe()
fcntl.fcntl(w, fcntl.F_SETFL, fcntl.fcntl(w, fcntl.F_GETFL) | os.O_NONBLOCK)
p = subprocess.Popen(sys.argv[1:], stdout=w)
code = p.wait()
print("exit", code, "parent O_NONBLOCK after child:", bool(fcntl.fcntl(w, fcntl.F_GETFL) & os.O_NONBLOCK))
