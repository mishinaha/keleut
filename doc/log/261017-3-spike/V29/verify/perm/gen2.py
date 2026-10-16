import itertools, subprocess, sys
exec(open('gen.py').read().split('sets={')[0])
sets={
 #'alt6':(['effect Log = { log: (String) => Unit }'],['newtype N0 = MkN0(A1[{}], A1[{Log}])','type A1[E] = (N1[E], Int32)','newtype N1[E] = MkN1(A2[E])','type A2[E] = N2[{Log extends E}]','newtype N2[E] = MkN2((() => Unit @ E, () => Unit @ {Log}))','type A3[E] = A2[E]']),
 #'wr':(['effect Log = { log: (String) => Unit }'],['newtype J = MkJ(() => Unit @ W2[{}])','type W2[E]: EffectRow = W1[{Print extends E}]','type W1[E]: EffectRow = {Log extends E}','newtype K = MkK(B[W1[{}]])','newtype B[E] = MkB(() => Unit @ E)']),
 'rec':([],['newtype L[E] = LNil | LCons(Cb[E], L[E])','type Cb[E] = () => Unit @ E','newtype M = MkM(L[{}])','type Q = L[{Print}]']),
}
for name,(fixed,ds) in sets.items():
  res={}
  for perm in itertools.permutations(range(len(ds))):
    src='\n'.join(fixed+[ds[i] for i in perm])+'\n'
    open('t2.kel','w').write(src)
    r={}
    for b,p in bins.items():
      o=subprocess.run([p,'--type-check','t2.kel'],capture_output=True,text=True)
      r[b]=(o.returncode,o.stdout+o.stderr)
    res[perm]=r
  for b in 'HPX':
    print(name,b,'accept',sum(1 for v in res.values() if v[b][0]==0),'/',len(res))
  for p,v in list(res.items()):
    if v['X'][0]!=0:
      print('  Xfail',p,v['X'][1].strip()[:150]); break
  reg=[p for p,v in res.items() if v['H'][0]==0 and (v['P'][0]!=0 or v['X'][0]!=0 or v['P'][1]!=v['H'][1])]
  print('  regressions',len(reg), reg[:3])
