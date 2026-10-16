K='<一時領域>/clusters/kinds/'
def load(p):
    d={};cur=None
    for l in open(p):
        if l.startswith('=== '): cur=l[4:].strip(); d[cur]=[]
        else: d[cur].append(l)
    return d
for i in ['V27','V31','V56','V57','V117']:
    P=load(K+f'i.{i}.proto.txt');C=load(K+f'verify/i.{i}.mine.txt');H=load(K+f'i.{i}.head.txt')
    for k in C:
        if P[k]!=C[k]: print(i,k,'\n P:',''.join(P[k]).strip(),'\n C:',''.join(C[k]).strip(),'\n H:',''.join(H[k]).strip())
