import subprocess,time,sys
bins=dict(a.split("=",1) for a in sys.argv[1].split(","))
for f in sys.argv[2:]:
  row=[f]
  outs=[]
  for b,X in bins.items():
    ts=[]
    for i in range(3):
      t=time.time(); r=subprocess.run([X,f],capture_output=True); ts.append(time.time()-t)
    outs.append(r.stdout+r.stderr); row.append("%s=%.3f"%(b,min(ts)))
  row.append("same" if all(o==outs[0] for o in outs) else "DIFF")
  print(" ".join(row))
