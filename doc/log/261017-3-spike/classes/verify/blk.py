import sys,re
def blocks(p):
    # block = command starting "  $ " plus following "  > " lines and outputs, until next "  $ " or non-indented text
    out=[];cur=None;name=None
    for l in open(p):
        if l.startswith('  $ '):
            if cur is not None: out.append(cur)
            cur=[l]
        elif l.startswith('  ') and cur is not None:
            cur.append(l)
        else:
            if cur is not None: out.append(cur); cur=None
    if cur is not None: out.append(cur)
    return out
a=blocks(sys.argv[1]); b=blocks(sys.argv[2])
print(sys.argv[2].split('/')[-1], 'cmds', len(a), len(b), 'changed', sum(1 for x,y in zip(a,b) if x!=y))
for x,y in zip(a,b):
    if x!=y: print('   ', x[0].strip()[:100])
