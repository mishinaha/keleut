import sys, bisect, collections, subprocess, re
syms=[]
for l in open(sys.argv[1]):
    p=l.rstrip('\n').split(' ',2)
    if len(p)<3: continue
    syms.append((int(p[0],16),p[2]))
syms.sort(); addrs=[a for a,_ in syms]
data=sys.argv[2]; vaddr=int(sys.argv[3],16)
mm=subprocess.run(['perf','script','-i',data,'--show-mmap-events'],capture_output=True,text=True).stdout
bias=None
for l in mm.splitlines():
    m=re.search(r'\[0x([0-9a-f]+)\(0x[0-9a-f]+\) @ 0x([0-9a-f]+) .*r-xp .*/release/keleut$',l)
    if m: bias=int(m.group(1),16)-int(m.group(2),16)+int(sys.argv[5],16)-vaddr; break
out=subprocess.run(['perf','script','-i',data,'-F','ip,dso','-G'],capture_output=True,text=True).stdout
agg=collections.Counter(); n=0
for l in out.splitlines():
    l=l.strip()
    if not l: continue
    parts=l.split(); 
    try: ip=int(parts[0],16)
    except: continue
    dso=parts[-1].strip('()'); n+=1
    if dso.endswith('/keleut'):
        a=ip-bias; i=bisect.bisect_right(addrs,a)-1
        name=syms[i][1] if i>=0 else '?'
        name=re.sub(r'::h[0-9a-f]{16}$','',name)[:100]
    else: name=dso.split('/')[-1]
    agg[name]+=1
print('samples',n)
for k,v in agg.most_common(int(sys.argv[4])): print(f"{100*v/n:6.2f}% {k}")
