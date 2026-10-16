import sys
def paras(p):
    out=[];cur=None
    for l in open(p):
        if l.startswith('  '):
            if cur is None: cur=[]
            cur.append(l)
        else:
            if cur is not None: out.append(''.join(cur)); cur=None
    if cur is not None: out.append(''.join(cur))
    return out
head=set(paras(sys.argv[3])) if len(sys.argv)>3 else set()
a=paras(sys.argv[1]); b=paras(sys.argv[2])
new=[i for i,x in enumerate(a) if x not in head]
ch=[i for i,(x,y) in enumerate(zip(a,b)) if x!=y]
print(sys.argv[1].split('/')[-1],'paras',len(a),'new',len(new),'failing',len(ch),'failing-new',len([i for i in ch if i in new]))
for i in ch: print('   ',a[i].splitlines()[0][:80])
