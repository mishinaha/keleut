import sys, re
src = open(sys.argv[1]).read().split('\n')
out = []
fence = None
openers = {'(':')','（':'）','「':'」','『':'』'}
closers = set(openers.values())
def split_line(line):
    m = re.match(r'^(\s*)(- |\d+\. )?', line)
    indent = m.group(1); marker = m.group(2) or ''
    cont = indent + ' ' * len(marker)
    body = line[len(indent)+len(marker):]
    parts = []; depth = 0; code = False; cur = ''
    i = 0
    while i < len(body):
        c = body[i]; cur += c
        if c == '`': code = not code
        elif not code:
            if c in openers: depth += 1
            elif c in closers: depth = max(0, depth-1)
            elif c == '。' and depth == 0:
                rest = body[i+1:]
                if rest.strip():
                    parts.append(cur); cur = ''
                    while i+1 < len(body) and body[i+1] == ' ': i += 1
        i += 1
    parts.append(cur)
    if len(parts) == 1: return [line]
    return [indent + marker + parts[0]] + [cont + p for p in parts[1:]]
changed = 0
for line in src:
    s = line.lstrip()
    if s.startswith('```'):
        if fence is None: fence = s[:len(s)-len(s.lstrip('`'))]
        elif s.startswith(fence) and s.strip() == fence: fence = None
        out.append(line); continue
    if fence is not None or s.startswith('|') or s.startswith('#'):
        out.append(line); continue
    r = split_line(line)
    if len(r) > 1: changed += 1
    out.extend(r)
open(sys.argv[2], 'w').write('\n'.join(out))
print('changed', changed, file=sys.stderr)
