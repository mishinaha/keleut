import itertools, subprocess, random
exec(open('gen.py').read().split('sets={')[0])
ds=['newtype N0 = MkN0(A1[{}])','type A1[E] = N1[E]','newtype N1[E] = MkN1(A2[E])','type A2[E] = N2[E]','newtype N2[E] = MkN2(A3[E])','type A3[E] = N3[E]','newtype N3[E] = MkN3(() => Unit @ E)']
random.seed(1)
perms=list(itertools.permutations(range(len(ds))))
random.shuffle(perms)
cnt={b:0 for b in bins}
fails=[]
for perm in perms[:300]:
  open('t4.kel','w').write('\n'.join(ds[j] for j in perm)+'\n')
  for b,p in bins.items():
    rc=subprocess.run([p,'--type-check','t4.kel'],capture_output=True,text=True).returncode
    if rc==0: cnt[b]+=1
    elif b!='H' and len(fails)<3: fails.append((b,perm))
print(cnt, fails)
