#!/usr/bin/env python3
"""conformance/corpus/ の小さなプログラムをすべて実行し、設定ごとの合計の経過時間を測る(計画 261024-1 の M93)。

短いプログラムの多い負荷で、ティアの設定(JIT のしきい値)のコストを比較するためのものである。
使い方は同じディレクトリの README.md にある。
各プログラムは一時ディレクトリで KIR にし、標準入力を閉じて実行する。
どの設定でも、標準出力と終了コードは最初の設定と同じでなければならない(違えば終了コード 1)。
"""

import argparse
import hashlib
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True  # run.py の __pycache__ を作らない
from run import CONFIGS  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "..", "conformance", "corpus")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--keleut", default=os.path.join(HERE, "..", "target", "release", "keleut"))
    ap.add_argument("--diktor", default=os.environ.get("KELEUT_DIKTOR") or shutil.which("diktor") or "")
    ap.add_argument("--configs", default="ki,kt1,kt10,kt100,kt1000,kt10000,kj")
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--timeout", type=float, default=60.0)
    a = ap.parse_args()
    K = os.path.abspath(a.keleut)
    D = os.path.abspath(a.diktor)
    work = tempfile.mkdtemp(prefix="keleut-corpus-")
    kirs = []
    for d, _, fs in sorted(os.walk(CORPUS)):
        for f in sorted(fs):
            if not f.endswith(".kel"):
                continue
            kel = os.path.join(d, f)
            kir = os.path.join(work, f"{len(kirs)}.kir")
            r = subprocess.run([D, "--emit-ir", kir, kel], capture_output=True, cwd=d,
                               stdin=subprocess.DEVNULL)
            if r.returncode == 0:
                kirs.append((kel, kir))
    print(f"# KIR にできたプログラム {len(kirs)} 個、回数 {a.repeat}、負荷 {os.getloadavg()[0]:.2f}")
    configs = a.configs.split(",")
    ref = {}
    failed = False
    env = dict(os.environ, KELEUT_DIKTOR=D)
    for c in configs:
        totals = []
        for _ in range(a.repeat):
            total = 0.0
            for kel, kir in kirs:
                cmd = CONFIGS[c](K, D, kir, kel)
                t0 = time.perf_counter()
                try:
                    r = subprocess.run(cmd, capture_output=True, stdin=subprocess.DEVNULL,
                                       cwd=os.path.dirname(kel), env=env, timeout=a.timeout)
                    key = (r.returncode, hashlib.sha256(r.stdout).hexdigest())
                except subprocess.TimeoutExpired:
                    key = ("timeout", "")
                total += time.perf_counter() - t0
                if kir not in ref:
                    ref[kir] = (c, key)
                elif ref[kir][1] != key:
                    print(f"  差: {kel} [{c}] と [{ref[kir][0]}]", file=sys.stderr)
                    failed = True
            totals.append(total)
        print(f"{c:8s} 合計 中央値 {statistics.median(totals):.2f} 秒 [最小 {min(totals):.2f}]", flush=True)
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
