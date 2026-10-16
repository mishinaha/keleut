import subprocess,time,sys
B=sys.argv[1]
for v in sys.argv[2].split(","):
  for f in sys.argv[3:]:
    ts=[]
    for i in range(3):
      t=time.time(); r=subprocess.run([B,"--prelude","p/%s.kel"%v,f],capture_output=True); ts.append(time.time()-t)
    print(v,f,"%.3f"%min(ts),r.stdout.strip()[:40],r.stderr[:80])
