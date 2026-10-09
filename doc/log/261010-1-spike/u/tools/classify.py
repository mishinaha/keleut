import sys, re, difflib, os, collections
base, new = sys.argv[1], sys.argv[2]
EXT = re.compile(r' extends R\d+')
def block_name(lines, idx):
    # nearest preceding command that names a .kel file
    for i in range(idx, -1, -1):
        l = lines[i]
        m = re.match(r'^  \$ (?:cat > |printf .*> |diktor .*?)([\w./-]+\.kel)', l)
        if m: return os.path.basename(m.group(1))[:-4], i+1
        m = re.match(r'^  \$ diktor .*?([\w./-]+\.kel)', l)
        if m: return os.path.basename(m.group(1))[:-4], i+1
    return '?', 0
def cmd_line(lines, idx):
    for i in range(idx, -1, -1):
        if lines[i].startswith('  $ '): return i+1
    return 0
summary = collections.Counter()
rows = []
for f in sorted(os.listdir(base)):
    if not f.endswith('.t'): continue
    a = open(os.path.join(base,f)).read().split('\n')
    b = open(os.path.join(new,f)).read().split('\n')
    if a == b: continue
    sm = difflib.SequenceMatcher(None, a, b, autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag == 'equal': continue
        old = a[i1:i2]; nw = b[j1:j2]
        bn, _ = block_name(a, i1-1 if i1>0 else 0)
        cl = cmd_line(a, i1-1)
        kind = None
        if len(old) == len(nw):
            kinds = set()
            for o, n in zip(old, nw):
                o2 = EXT.sub('', o)
                if o2 == n and o != n: kinds.add('ext'); continue
                # closed row explicit: n == o + ' @ {...}' at end, or before ' = <fn>'
                m = re.match(r'^(.*?) @ (\{[^{}]*\})( = <fn>)?$', n)
                if m and (m.group(1) + (m.group(3) or '')) == o: kinds.add('closed:' + m.group(2)); continue
                if m and (m.group(1) + (m.group(3) or '')) == o2: kinds.add('closed+ext:'+m.group(2)); continue
                kinds.add('other')
            kind = ','.join(sorted(kinds))
        else:
            kind = 'shape'
        rows.append((f, bn, i1+1, cl, tag, kind, old, nw))
for r in rows:
    f,bn,ln,cl,tag,kind,old,nw = r
    print(f"{f}\t{bn}\tL{ln}\tcmdL{cl}\t{tag}\t{kind}\t-{len(old)}+{len(nw)}")
    if 'other' in kind or kind=='shape':
        for o in old: print('    - '+o)
        for n in nw: print('    + '+n)
