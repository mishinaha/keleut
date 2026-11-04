#!/usr/bin/env python3
"""コマンドを 1 回走らせ、壁時計の秒、最大の常駐の大きさ(MB)、終了状態、標準エラーの末尾を 1 行で書く。

使い方: measure.py [--as KIB] [--timeout SEC] -- CMD ARGS...

--as は ulimit -v と同じく RLIMIT_AS を KiB で絞る。
/usr/bin/time が無い機械で使うために、os.wait4 の rusage を読む。
"""
import os
import resource
import subprocess
import sys
import time

args = sys.argv[1:]
as_kib = None
timeout = 600
while args and args[0] != "--":
    if args[0] == "--as":
        as_kib = int(args[1])
        args = args[2:]
    elif args[0] == "--timeout":
        timeout = float(args[1])
        args = args[2:]
    else:
        sys.exit("bad option " + args[0])
cmd = args[1:]


def pre():
    if as_kib is not None:
        b = as_kib * 1024
        resource.setrlimit(resource.RLIMIT_AS, (b, b))


t0 = time.time()
p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, preexec_fn=pre)
try:
    out, err = p.communicate(timeout=timeout)
except subprocess.TimeoutExpired:
    p.kill()
    out, err = p.communicate()
    print(f"TIMEOUT {timeout}s")
    sys.exit(0)
dt = time.time() - t0
ru = resource.getrusage(resource.RUSAGE_CHILDREN)
rc = p.returncode
code = rc if rc >= 0 else 128 - rc
tail_out = out.decode(errors="replace").strip().splitlines()[-1:] if out else []
tail_err = err.decode(errors="replace").strip().splitlines()[-1:] if err else []
print(f"{dt:.2f}s maxrss={ru.ru_maxrss/1024:.0f}MB exit={code} out={tail_out} err={tail_err}")
