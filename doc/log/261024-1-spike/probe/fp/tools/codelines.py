import sys
def strip(src):
    out=[];i=0;depth=0;instr=False;n=len(src)
    while i<n:
        c=src[i]
        if depth==0 and not instr and c=='"': instr=True; out.append(c); i+=1; continue
        if instr:
            out.append(c)
            if c=='\\': out.append(src[i+1:i+2]); i+=2; continue
            if c=='"': instr=False
            i+=1; continue
        if src.startswith('(*',i): depth+=1; i+=2; continue
        if depth>0 and src.startswith('*)',i): depth-=1; i+=2; continue
        if depth>0:
            if c=='\n': out.append(c)
            i+=1; continue
        out.append(c); i+=1
    return ''.join(out)
for f in sys.argv[1:]:
    s=open(f,encoding='utf-8').read()
    code=strip(s)
    cl=sum(1 for l in code.split('\n') if l.strip())
    print("%6d %6d %s"%(s.count('\n'),cl,f.split('/')[-1]))
