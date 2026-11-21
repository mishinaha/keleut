#!/usr/bin/env python3
"""RustKeleut のバックエンドと Diktor の実行時間を測る(計画 261024-1 の M93)。

使い方は同じディレクトリの README.md にある。
programs/ の .kel を一時ディレクトリで KIR にし、設定ごとに何回か実行して、
経過時間の中央値と最小値、最大 RSS を表にする。
どの設定の標準出力も、最初の設定の標準出力と同じでなければならない(違えば終了コード 1)。
"""

import argparse
import datetime
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PROGRAMS = os.path.join(HERE, "programs")

# 設定の名前と、コマンド行の作り方。K は keleut のパス、D は diktor のパス、kir と kel は入力ファイルのパスである。
CONFIGS = {
    "ki": lambda K, D, kir, kel: [K, "run", "--backend", "interp", kir],
    "kj": lambda K, D, kir, kel: [K, "run", "--backend", "jit", "--tier", "jit-only", kir],
    "kd": lambda K, D, kir, kel: [K, "run", kir],
    "kw": lambda K, D, kir, kel: [K, "run", "--backend", "wasm", kir],
    "dk": lambda K, D, kir, kel: [D, kel],
    "dkir": lambda K, D, kir, kel: [D, "--run-ir", kir],
    "dktc": lambda K, D, kir, kel: [D, "--type-check", kel],
}
for _n in (1, 10, 100, 1000, 10000, 100000):
    CONFIGS[f"kt{_n}"] = (
        lambda n: lambda K, D, kir, kel: [K, "run", "--backend", "jit", "--tier", f"threshold={n}", kir]
    )(_n)
for _s in (1, 2):
    CONFIGS[f"ka{_s}"] = (
        lambda s: lambda K, D, kir, kel: [K, "run", "--backend", "jit", "--tier", f"alternating={s}", kir]
    )(_s)
    CONFIGS[f"km{_s}"] = (
        lambda s: lambda K, D, kir, kel: [K, "run", "--backend", "jit", "--tier", f"mixed={s}", kir]
    )(_s)

# 型検査だけの設定は出力が違うので、出力の比較から外す。
NO_COMPARE = {"dktc"}
SLOW = {"dk", "dkir", "dktc"}


def run_once(cmd, env):
    """1 回走らせ、(経過秒, 終了状態, 標準出力の sha256, 最大常駐 KiB) を返す。"""
    out = tempfile.TemporaryFile()
    t0 = time.perf_counter()
    p = subprocess.Popen(cmd, stdout=out, stderr=subprocess.DEVNULL, env=env)
    try:
        _, status, ru = os.wait4(p.pid, 0)
    except BaseException:
        p.kill()
        raise
    t1 = time.perf_counter()
    p.returncode = os.waitstatus_to_exitcode(status)
    out.seek(0)
    h = hashlib.sha256(out.read()).hexdigest()[:12]
    out.close()
    return t1 - t0, p.returncode, h, ru.ru_maxrss


def machine():
    cpu = ""
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    return {
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "kernel": platform.release(),
        "cpu": cpu,
        "cores": os.cpu_count(),
        "load": os.getloadavg(),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--keleut", default=os.path.join(HERE, "..", "target", "release", "keleut"))
    ap.add_argument("--diktor", default=os.environ.get("KELEUT_DIKTOR") or shutil.which("diktor") or "")
    ap.add_argument("--configs", default="ki,kj,kd,kw,dk")
    ap.add_argument("--bench", default="", help="コンマで区切った名前(省くと programs/ のすべて)")
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--slow-repeat", type=int, default=3, help="dk、dkir、dktc の回数")
    ap.add_argument("--json", default="", help="すべてのサンプルを書く JSON のファイル")
    ap.add_argument("--work", default="", help="KIR を置くディレクトリ(省くと一時ディレクトリ)")
    ap.add_argument("--label", default="", help="表に添える名前(バージョンなど)")
    ap.add_argument("--interleave", action="store_true", help="1 回ごとに設定を順に回す(負荷の変動を設定の間に散らす)")
    ap.add_argument("--warmup", type=int, default=0, help="測る前に各設定を捨てる回数だけ実行する")
    a = ap.parse_args()

    K = os.path.abspath(a.keleut)
    D = os.path.abspath(a.diktor) if a.diktor else ""
    if not D:
        sys.exit("diktor が見つかりません(--diktor か KELEUT_DIKTOR)")
    configs = [c for c in a.configs.split(",") if c]
    for c in configs:
        if c not in CONFIGS:
            sys.exit(f"知らない設定です: {c}(使える設定: {', '.join(CONFIGS)})")
    names = sorted(f[:-4] for f in os.listdir(PROGRAMS) if f.endswith(".kel"))
    if a.bench:
        want = a.bench.split(",")
        for w in want:
            if w not in names:
                sys.exit(f"知らない計測です: {w}")
        names = want

    work = a.work or tempfile.mkdtemp(prefix="keleut-bench-")
    os.makedirs(work, exist_ok=True)
    env = dict(os.environ)
    env["KELEUT_DIKTOR"] = D
    m = machine()
    print(f"# {a.label or 'keleut bench'}  {m['date']}")
    print(f"# {m['cpu']}, {m['cores']} 論理コア, Linux {m['kernel']}, 負荷 {m['load'][0]:.2f}")
    order = "設定を回ごとに順に回す" if a.interleave else "設定ごとに続けて実行する"
    print(
        f"# 回数 {a.repeat}(dk、dkir、dktc は {a.slow_repeat})、捨てる回 {a.warmup}、{order}。"
        "値は経過時間の中央値 [最小値] ミリ秒"
    )

    results = {
        "machine": m,
        "label": a.label,
        "configs": configs,
        "interleave": a.interleave,
        "warmup": a.warmup,
        "bench": {},
    }
    failed = False
    for name in names:
        kel = os.path.join(PROGRAMS, name + ".kel")
        kir = os.path.join(work, name + ".kir")
        r = subprocess.run([D, "--emit-ir", kir, kel], capture_output=True, text=True)
        if r.returncode != 0:
            print(f"{name}: KIR を書けません: {r.stdout}{r.stderr}", file=sys.stderr)
            failed = True
            continue
        cmds = {c: CONFIGS[c](K, D, kir, kel) for c in configs}
        for c in configs:
            for _ in range(a.warmup):
                run_once(cmds[c], env)
        samples_of = {c: [] for c in configs}
        if a.interleave:
            # 1 回ごとにすべての設定を 1 度ずつ実行する。回数の少ない設定は先に終わる。
            for i in range(max(a.slow_repeat if c in SLOW else a.repeat for c in configs)):
                for c in configs:
                    if i < (a.slow_repeat if c in SLOW else a.repeat):
                        samples_of[c].append(run_once(cmds[c], env))
        else:
            for c in configs:
                n = a.slow_repeat if c in SLOW else a.repeat
                for _ in range(n):
                    samples_of[c].append(run_once(cmds[c], env))
        row = {}
        ref = None
        for c in configs:
            samples = [{"t": t, "rc": rc, "out": h, "rss_kib": rss} for t, rc, h, rss in samples_of[c]]
            outs = {s["out"] for s in samples}
            rcs = {s["rc"] for s in samples}
            ok = rcs == {0} and len(outs) == 1
            if ok and c not in NO_COMPARE:
                if ref is None:
                    ref = (c, next(iter(outs)))
                elif next(iter(outs)) != ref[1]:
                    ok = False
            if not ok:
                failed = True
            ts = [s["t"] for s in samples]
            row[c] = {
                "median": statistics.median(ts),
                "min": min(ts),
                "max": max(ts),
                "rss_kib": max(s["rss_kib"] for s in samples),
                "ok": ok,
                "samples": samples,
            }
        results["bench"][name] = row
        cells = []
        for c in configs:
            v = row[c]
            mark = "" if v["ok"] else " 誤"
            cells.append(f"{c} {v['median'] * 1000:.0f} [{v['min'] * 1000:.0f}]{mark}")
        print(f"{name:12s} " + "  ".join(cells), flush=True)

    # Markdown の表(中央値、ミリ秒)。
    print()
    print("| 計測 | " + " | ".join(configs) + " |")
    print("| --- | " + " | ".join("---:" for _ in configs) + " |")
    for name, row in results["bench"].items():
        cells = []
        for c in configs:
            v = row[c]
            cells.append(f"{v['median'] * 1000:.0f}" + ("" if v["ok"] else " 誤"))
        print(f"| {name} | " + " | ".join(cells) + " |")
    if a.json:
        with open(a.json, "w") as f:
            json.dump(results, f, indent=1, ensure_ascii=False)
    if not a.work:
        shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
