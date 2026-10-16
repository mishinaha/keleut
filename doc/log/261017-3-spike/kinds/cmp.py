import sys
def load(f):
    d={};k=None
    for l in open(f,encoding='utf-8'):
        if l.startswith('=== '): k=l[4:].strip(); d[k]=''
        else: d[k]+=l
    return d
tag=sys.argv[1] if len(sys.argv)>1 else 'core'
H=load('r.head.txt');M=load('r.v22v124.txt');V=load('r.v30.txt');X=load('r.v29ext.txt');C=load('r.%s.txt'%tag)
def acc(o): return o.strip().endswith('[exit 0]')
def one(o): return o.strip().replace('\n',' / ')[:160]
print('== %s differs from V22+V124:'%tag)
for k in C:
    if C[k]!=M[k]: print(' ',k,'| H',acc(H[k]),'M',acc(M[k]),'V30',acc(V[k]),'C',acc(C[k])); print('    M:',one(M[k])); print('    C:',one(C[k]))
print('== rejects but some proto accepts:')
for k in C:
    if not acc(C[k]) and (acc(M[k]) or acc(V[k]) or acc(X[k])): print(' ',k, 'M',acc(M[k]),'V30',acc(V[k]),'X',acc(X[k]))
print('== accepts, HEAD rejects:', sum(1 for k in C if acc(C[k]) and not acc(H[k])))
print('== HEAD accepts, rejects:', [k for k in C if acc(H[k]) and not acc(C[k])])
print('== both accept, output differs from HEAD:', [k for k in C if acc(H[k]) and acc(C[k]) and H[k]!=C[k]])
print('== both reject, diag differs from HEAD:')
for k in C:
    if not acc(H[k]) and not acc(C[k]) and H[k]!=C[k]: print('  ',k); print('    H:',one(H[k])); print('    C:',one(C[k]))
