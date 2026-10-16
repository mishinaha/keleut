import sys
def load(p):
    r={};k=None
    for l in open(p):
        if l.startswith('=== '): k=l[4:].strip().split('/scratchpad/items/')[1]; r[k]=''
        else: r[k]+=l
    return r
h,a,b,m=[load('cmp.%s.txt'%x) for x in ('head','v22','v124','merged')]
for k in h:
    if not (h[k]==a[k]==b[k]==m[k]):
        def tag(x): return 'H' if x==h[k] else ('A' if x==a[k] else ('B' if x==b[k] else '?'))
        print(k, 'v22=',tag(a[k]),'v124=',tag(b[k]),'merged=',tag(m[k]))
        print('  head:', h[k].strip().replace('\n',' | ')[:220])
        if a[k]!=h[k]: print('  v22 :', a[k].strip().replace('\n',' | ')[:220])
        if b[k]!=h[k] and b[k]!=a[k]: print('  v124:', b[k].strip().replace('\n',' | ')[:220])
        if m[k] not in (h[k],a[k],b[k]): print('  merg:', m[k].strip().replace('\n',' | ')[:220])
