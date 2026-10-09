import sys, subprocess, time, resource
def run(exe, f):
    t=time.time()
    p=subprocess.run([exe,f],capture_output=True,text=True)
    dt=time.time()-t
    ru=resource.getrusage(resource.RUSAGE_CHILDREN)
    return dt, p.stdout.strip().replace("\n"," | "), p.returncode
for f in sys.argv[3:]:
    for name,exe in (("base",sys.argv[1]),("dict",sys.argv[2])):
        best=None
        for _ in range(3):
            dt,out,rc=run(exe,f)
            best=dt if best is None else min(best,dt)
        print(f"{f}\t{name}\t{best:.2f}s\trc={rc}\t{out[:60]}")
