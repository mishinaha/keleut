import itertools, subprocess
exec(open('gen.py').read().split('sets={')[0])
pairs=[
 (['effect Log = { log: (String) => Unit }','newtype H = MkH(K[{}])','type K[E] = (() => Unit @ E, () => Unit @ {Log})'],
  ['effect Log = { log: (String) => Unit }','newtype H = MkH((() => Unit @ {}, () => Unit @ {Log}))','type K[E] = (() => Unit @ E, () => Unit @ {Log})']),
 (['effect Log = { log: (String) => Unit }','newtype H = MkH(K[{}])','type K[E] = B[E]','newtype B[E] = MkB((() => Unit @ E, () => Unit @ {Log}))'],
  ['effect Log = { log: (String) => Unit }','newtype H = MkH(B[{}])','type K[E] = B[E]','newtype B[E] = MkB((() => Unit @ E, () => Unit @ {Log}))']),
 (['effect Log = { log: (String) => Unit }','effect Ev = { on: (K[{}]) => Unit }','type K[E] = (() => Unit @ E, () => Unit @ {Log})'],
  ['effect Log = { log: (String) => Unit }','effect Ev = { on: ((() => Unit @ {}, () => Unit @ {Log})) => Unit }','type K[E] = (() => Unit @ E, () => Unit @ {Log})']),
]
for i,(a,d) in enumerate(pairs):
  for perm in itertools.permutations(range(len(a))):
    r={}
    for tag,ds in (('alias',a),('direct',d)):
      open('t3.kel','w').write('\n'.join(ds[j] for j in perm)+'\n')
      for b,p in bins.items():
        r[(tag,b)]=subprocess.run([p,'--type-check','t3.kel'],capture_output=True,text=True).returncode
    for b in 'HPX':
      if (r[('alias',b)]==0)!=(r[('direct',b)]==0):
        print('pair',i,'perm',perm,b,'alias',r[('alias',b)],'direct',r[('direct',b)])
print('done')
