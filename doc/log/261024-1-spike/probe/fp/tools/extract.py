import sys, re, os, collections
tdir, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
stats = collections.Counter(); n=0
for fn in sorted(os.listdir(tdir)):
    if not fn.endswith('.t'): continue
    lines = open(os.path.join(tdir, fn), encoding='utf-8').read().split('\n')
    i = 0
    while i < len(lines):
        m = re.match(r"^  \$ cat > (\S+\.kel) <<'?(\w+)'?\s*$", lines[i])
        if m:
            name, term = m.group(1), m.group(2)
            body = []; i += 1
            while i < len(lines) and lines[i] != '  > ' + term:
                body.append(lines[i][4:] if lines[i].startswith('  > ') else lines[i][3:]); i += 1
            d = os.path.join(out, fn[:-2]); os.makedirs(d, exist_ok=True)
            n += 1
            open(os.path.join(d, '%04d_%s' % (n, name.replace('/', '_'))), 'w', encoding='utf-8').write('\n'.join(body) + '\n')
            stats[fn] += 1
        elif re.match(r"^  \$ cat > \S+\.kel", lines[i]):
            stats['UNMATCHED'] += 1
        i += 1
print(n, stats['UNMATCHED'])
