import sys, subprocess, time
exes=[a.split("=",1) for a in sys.argv[1:] if "=" in a]
files=[a for a in sys.argv[1:] if "=" not in a]
print("file\t"+"\t".join(n for n,_ in exes))
for f in files:
    row=[f]
    for n,exe in exes:
        best=None
        for _ in range(5):
            t=time.time(); p=subprocess.run([exe,f],capture_output=True,text=True); dt=time.time()-t
            best=dt if best is None else min(best,dt)
        row.append(f"{best:.2f}")
    print("\t".join(row))
