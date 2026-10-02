#!/usr/bin/env python3
# usage: blocks.py <diktor dir>  -- compares test/*.t with _build/default/test/*.t.corrected
import sys, glob, os
d = sys.argv[1]
def blocks(lines):
    out = []; cur = None
    for i, l in enumerate(lines, 1):
        if l.startswith('  $ '):
            cur = [i, l.rstrip('\n'), []]; out.append(cur)
        elif l.startswith('  > ') and cur is not None and not cur[2]:
            pass
        elif l.startswith('  ') and cur is not None:
            cur[2].append(l.rstrip('\n'))
        else:
            cur = None
    return out
tot = 0
import subprocess
base = sys.argv[2] if len(sys.argv) > 2 else 'HEAD'
names = subprocess.run(['git','-C',d,'diff','--name-only',base,'--','test/'],capture_output=True,text=True).stdout.split()
for nm in names:
    if not nm.endswith('.t'): continue
    name = os.path.basename(nm)
    old = subprocess.run(['git','-C',d,'show',base+':'+nm],capture_output=True,text=True).stdout
    a = blocks(old.splitlines(True))
    b = blocks(open(os.path.join(d,nm)).readlines()) if os.path.exists(os.path.join(d,nm)) else []
    n = 0
    for x, y in zip(a, b):
        if x[2] != y[2]:
            n += 1
            new = y[2][0].strip() if y[2] else '(出力なし)'
            print(f"{name}:{x[0]}: {x[1].strip()[:70]}\n    -> {new[:110]}")
    print(f"== {name}: {n} blocks (of {len(a)})")
    tot += n
print("TOTAL", tot)
