import sys,re
cur=None;cmd=None;res={}
for l in open(sys.argv[1],encoding='utf-8'):
    m=re.match(r'^File "(test/[^"]+)"',l)
    if m: cur=m.group(1); continue
    if l.startswith('@@'): cmd=None; continue
    if l.startswith('+++') or l.startswith('---'): continue
    body=l[1:] if l[:1] in ' +-' else l
    if body.startswith('  $ ') and ('diktor' in body or 'inv ' in body):
        cmd=body.strip()
        continue
    if l[:1] in '+-' and cmd:
        res.setdefault(cur,[])
        if cmd not in res[cur]: res[cur].append(cmd)
for f,cs in res.items():
    print(f,len(cs))
    for c in cs: print('   ',c)
