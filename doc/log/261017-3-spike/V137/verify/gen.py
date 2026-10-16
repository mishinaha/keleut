import random, sys, os
out = sys.argv[1]; n = int(sys.argv[2]); seed0 = int(sys.argv[3])
L = 4
def op_perform(rnd, lvl, ctx):
    # perform an op of some level <= ? any level 0..L-1 (handled by enclosing chain)
    j = rnd.randrange(L)
    if rnd.random() < 0.5:
        return f'perform a{j}({rnd.randrange(10)})', 'int'
    return f'perform t{j}("{ctx}")', 'never'
def stmt(rnd, ctx):
    s, k = op_perform(rnd, 0, ctx)
    if k == 'int':
        return f'{{ let q = {s}; echoln("{ctx} got " + show(q)) }}'
    return s
def body(rnd, ctx, depth):
    parts = []
    for i in range(rnd.randrange(1, 4)):
        if depth > 0 and rnd.random() < 0.3:
            parts.append(f'{{ let w = {handler(rnd, ctx+"i"+str(i), depth-1)}; echoln("{ctx} w " + show(w)) }}')
        else:
            parts.append(stmt(rnd, ctx + str(i)))
    parts.append(str(rnd.randrange(100)))
    return '{ ' + '; '.join(parts) + ' }'
hid = [0]
def handler(rnd, ctx, depth):
    hid[0] += 1; h = hid[0]
    lvl = rnd.randrange(L)
    b = body(rnd, ctx, depth)
    cl = []
    r = rnd.random()
    if r < 0.33:
        cl.append(f'case a{lvl}(x) => resume(x + {h})')
    elif r < 0.66:
        cl.append(f'case a{lvl}(x) => {{ let y = resume(x + {h}); echoln("H{h} a got " + show(y)); y + 1 }}')
    else:
        cl.append(f'case a{lvl}(x) => {{ echoln("H{h} a noresume"); x + 1000 }}')
    r = rnd.random()
    if r < 0.4:
        cl.append(f'case t{lvl}(e) => {{ echoln("H{h} t " + e); {h * 10} }}')
    elif r < 0.7:
        s, k = op_perform(rnd, lvl, f"H{h}clause")
        if k == 'int':
            cl.append(f'case t{lvl}(e) => {{ echoln("H{h} t " + e); let z = {s}; z + 1 }}')
        else:
            cl.append(f'case t{lvl}(e) => {{ echoln("H{h} t " + e); {s} }}')
    else:
        cl.append(f'case t{lvl}(e) => {{ echoln("H{h} t " + e); 1 / 0 }}')
    if rnd.random() < 0.6:
        s, k = op_perform(rnd, lvl, f"H{h}cancel")
        if k == 'int':
            cl.append(f'case cancel => {{ echoln("H{h} cancel"); let c = {s}; echoln("H{h} cancel got " + show(c)) }}')
        else:
            cl.append(f'case cancel => {{ echoln("H{h} cancel"); {s} }}')
    if rnd.random() < 0.5:
        cl.append(f'case return(v) => {{ echoln("H{h} ret " + show(v)); v + {h} }}')
    return f'({b}) handle {{ \n' + '\n'.join(cl) + '\n}'
for i in range(n):
    rnd = random.Random(seed0 + i); hid[0] = 0
    eff = '\n'.join(f'effect E{j} = {{ a{j}: (Int32) => Int32, t{j}: (String) => Never }}' for j in range(L))
    inner = handler(rnd, "m", 2)
    prog = eff + '\nlet r = ' + '(' * L + '(' + inner + ')'
    for j in range(L):
        prog += f' handle {{\ncase a{j}(x) => {{ let y = resume(x); echoln("top a{j} got " + show(y)); y + {j+1} }}\ncase t{j}(e) => {{ echoln("top t{j} " + e); -1 }}\n}})'
    prog += '\necholn("r=" + show(r))\n'
    open(os.path.join(out, f'g{i:04d}.kel'), 'w').write(prog)
