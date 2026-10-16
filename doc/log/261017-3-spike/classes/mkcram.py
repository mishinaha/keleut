import sys
def blk(name, src, cmds):
    s=f"  $ cat > {name}.kel <<'KEL'\n"+''.join('  > '+l+'\n' for l in src.rstrip('\n').split('\n'))+"  > KEL\n"
    for c in cmds: s+=f"  $ {c}\n"
    return s
def rd(p): return open(p).read()
