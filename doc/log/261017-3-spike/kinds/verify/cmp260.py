import sys,re
K='<一時領域>/clusters/kinds/'
def load(p):
    d={};cur=None
    for l in open(p):
        if l.startswith('=== '): cur=l[4:].strip(); d[cur]=[]
        else: d[cur].append(l)
    return d
H=load(K+'verify/r.headmine.txt');C=load(K+'verify/r.mine.txt')
P={n:load(K+f) for n,f in [('v22v124','r.v22v124.txt'),('v30','r.v30.txt'),('v29ext','r.v29ext.txt')]}
acc=lambda o:o[-1].strip()=='[exit 0]'
for k in H:
    if acc(H[k]) and H[k]!=C[k]: print('HEAD-changed',k,H[k],C[k])
    for n,p in P.items():
        if acc(p[k]) and not acc(C[k]): print('proto-acc-final-rej',n,k)
print('n',len(H))
