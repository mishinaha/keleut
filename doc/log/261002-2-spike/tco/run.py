import resource, subprocess, sys, time, os
# usage: run.py exe file.kel N [OCAMLRUNPARAM]
exe, f, n = sys.argv[1], sys.argv[2], sys.argv[3]
src = open(f).read().replace("N", n) if False else open(f).read()
import re
src = re.sub(r'\bN\b', n, src)
tmp = f"/tmp/claude-1000/-home-sumito3478-repo-keleut/8fd1dfd3-6874-4981-8f61-9d2478ad4a92/scratchpad/plan/spike/tco/m/_{os.getpid()}.kel"
open(tmp, "w").write(src)
env = dict(os.environ)
if len(sys.argv) > 4: env["OCAMLRUNPARAM"] = sys.argv[4]
t = time.time()
p = subprocess.run([exe, tmp], capture_output=True, text=True, timeout=600, env=env)
dt = time.time() - t
ru = resource.getrusage(resource.RUSAGE_CHILDREN)
os.remove(tmp)
out = (p.stdout + p.stderr).strip().splitlines()
print(f"{os.path.basename(f)} N={n} l={sys.argv[4] if len(sys.argv)>4 else '-'}: exit={p.returncode} time={dt:.2f}s maxrss={ru.ru_maxrss//1024}MB out={out[-1][:80] if out else ''}")
