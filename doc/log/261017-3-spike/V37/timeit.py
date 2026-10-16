import subprocess, sys, time
exe = sys.argv[1]
for f in sys.argv[2:]:
    ts = []
    for _ in range(3):
        t = time.perf_counter()
        p = subprocess.run([exe, f], capture_output=True)
        ts.append(time.perf_counter() - t)
    print(f"{f.rsplit('/',1)[-1]}: rc={p.returncode} bytes={len(p.stdout)} min={min(ts):.3f}s runs={' '.join(f'{x:.3f}' for x in ts)} err={p.stderr[:100]!r}")
