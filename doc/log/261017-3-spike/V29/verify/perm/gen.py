import itertools, subprocess, sys, os
S='<一時領域>'
bins={'H':S+'/bin/diktor-head','P':S+'/items/V29/verify/diktor/_build/default/bin/main.exe','X':S+'/items/V29/verify/ext/diktor/_build/default/bin/main.exe'}
sets={
 'chain':(['effect Log = { log: (String) => Unit }'],['newtype N0 = MkN0(A1[{}])','type A1[E] = N1[E]','newtype N1[E] = MkN1(A2[E])','type A2[E] = N2[E]','newtype N2[E] = MkN2((() => Unit @ E, () => Unit @ {Log}))']),
 'chainP':(['effect Log = { log: (String) => Unit }'],['newtype N0 = MkN0(A1[{Print}])','type A1[E] = N1[E]','newtype N1[E] = MkN1(A2[E])','type A2[E] = N2[E]','newtype N2[E] = MkN2((() => Unit @ E, () => Unit @ {Log}))']),
 'aa':([],['type C = A1[{}]','type A1[E] = A2[E]','type A2[E] = A3[E]','type A3[E] = N[E]','newtype N[E] = MkN(() => Unit @ E)']),
 'eff':(['effect Log = { log: (String) => Unit }'],['effect Ev = { on: (A1[{}]) => Unit }','type A1[E] = N1[E]','newtype N1[E] = MkN1((() => Unit @ E, () => Unit @ {Log}))']),
}
for name,(fixed,ds) in sets.items():
  res={}
  for perm in itertools.permutations(range(len(ds))):
    src='\n'.join(fixed+[ds[i] for i in perm])+'\n'
    open('t.kel','w').write(src)
    r={}
    for b,p in bins.items():
      o=subprocess.run([p,'--type-check','t.kel'],capture_output=True,text=True)
      r[b]=o.returncode
    res[perm]=r
  for b in 'HPX':
    ok=sum(1 for v in res.values() if v[b]==0)
    print(name,b,'accept',ok,'/',len(res))
  bad=[p for p,v in res.items() if v['P']!=0 or v['X']!=0]
  for p in bad[:4]:
    print('  fail',name,p,res[p])
  reg=[p for p,v in res.items() if v['H']==0 and (v['P']!=0 or v['X']!=0)]
  print('  regressions',len(reg))
