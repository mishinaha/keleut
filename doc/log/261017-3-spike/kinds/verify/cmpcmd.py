import sys
def parse(p):
    cmds=[];cur=None
    for l in open(p):
        if l.startswith('  $ '):
            cur=[l,[]];cmds.append(cur)
        elif l.startswith('  > ') and cur and not cur[1]:
            cur[0]+=l
        elif l.startswith('  ') and cur is not None:
            cur[1].append(l)
        else:
            cur=None
    return cmds
a=parse(sys.argv[1]);b=parse(sys.argv[2])
assert len(a)==len(b),(len(a),len(b))
n=0
for x,y in zip(a,b):
    if x[1]!=y[1]:
        n+=1; print('  ',x[0].splitlines()[0][:90])
print(sys.argv[1].split('/')[-1],n)
