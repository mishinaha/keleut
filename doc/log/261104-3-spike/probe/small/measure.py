#!/usr/bin/env python3
"""コマンドを R 回走らせ、壁時計の時間、子の CPU 時間(user+sys)の中央値と、最大の常駐の大きさ(ru_maxrss)の最大を書く。
使い方: measure.py R cmd args...  (標準出力と標準エラーは捨てる)
機械の負荷が高いときは壁時計より CPU 時間を見る。"""
import os, sys, time, statistics
r = int(sys.argv[1]); cmd = sys.argv[2:]
wall = []; cpu = []; peak = 0; code = None
for _ in range(r):
    t0 = time.monotonic()
    pid = os.fork()
    if pid == 0:
        fd = os.open(os.devnull, os.O_WRONLY); os.dup2(fd, 1); os.dup2(fd, 2)
        os.execvp(cmd[0], cmd)
    _, st, ru = os.wait4(pid, 0)
    wall.append(time.monotonic() - t0)
    cpu.append(ru.ru_utime + ru.ru_stime)
    peak = max(peak, ru.ru_maxrss)
    code = os.waitstatus_to_exitcode(st)
print(f"wall {statistics.median(wall):.3f} s, cpu {statistics.median(cpu):.3f} s (median of {r}), max RSS {peak/1024:.0f} MB, exit {code}")
