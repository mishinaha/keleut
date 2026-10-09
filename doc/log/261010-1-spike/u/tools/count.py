import sys, re, difflib, os, collections
base, new = sys.argv[1], sys.argv[2]
EXT = re.compile(r' extends R\d+')
tot = collections.Counter(); perfile = collections.defaultdict(collections.Counter)
blocks = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
def block_name(lines, idx):
    for i in range(idx, -1, -1):
        l = lines[i]
        m = re.match(r'^  \$ (?:cat > |printf .*> )([\w./-]+\.kel)', l) or re.match(r'^  \$ diktor .*?([\w./-]+\.kel)', l)
        if m: return os.path.basename(m.group(1))[:-4]
    return '?'
for f in sorted(os.listdir(base)):
    if not f.endswith('.t'): continue
    a = open(os.path.join(base,f)).read().split('\n'); b = open(os.path.join(new,f)).read().split('\n')
    if a == b: continue
    sm = difflib.SequenceMatcher(None, a, b, autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag == 'equal': continue
        old = a[i1:i2]; nw = b[j1:j2]
        bn = block_name(a, i1-1)
        if len(old) != len(nw) or any(not (EXT.sub('',o)==n or re.match(r'^(.*?) @ \{[^{}]*\}( = <fn>)?$', n)) for o,n in zip(old,nw)):
            k='semantic/err'
            perfile[f][k]+=1; tot[k]+=1; blocks[f][bn][k]+=1; continue
        for o,n in zip(old,nw):
            o2 = EXT.sub('', o)
            if o2 == n: k='ext'
            else:
                m = re.match(r'^(.*?) @ (\{[^{}]*\})( = <fn>)?$', n)
                base_ = m.group(1)+(m.group(3) or '')
                if base_ == o: k='closed'+m.group(2)
                elif base_ == o2: k='closed'+m.group(2)+'?ext'
                else: k='??'
            perfile[f][k]+=1; tot[k]+=1; blocks[f][bn][k]+=1
print(tot)
for f in perfile: print(f, dict(perfile[f]), 'blocks', len(blocks[f]))
for f in blocks:
    print('==', f, ' '.join(f"{bn}:{'/'.join(f'{k}{v}' for k,v in c.items())}" for bn,c in blocks[f].items()))
