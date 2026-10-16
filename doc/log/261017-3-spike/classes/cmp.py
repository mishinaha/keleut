import sys,subprocess,os,glob
items=sys.argv[1]; comp=sys.argv[2]; ids=sys.argv[3:]
def run(b,d,args):
    try:
        r=subprocess.run([b]+args,cwd=d,capture_output=True,text=True,timeout=5)
        return (r.stdout+r.stderr).strip()+' [%d]'%r.returncode
    except subprocess.TimeoutExpired: return 'TIMEOUT'
for i in ids:
    proto=f'{items}/{i}/diktor/_build/install/default/bin/diktor'
    files=[]
    for sub in ['ex','ex2','verify/ex','fin']:
        files+=sorted(glob.glob(f'{items}/{i}/{sub}/*.kel'))
    n=0;dd=0
    for f in files:
        d,b=os.path.split(f)
        for args in ([b],['--type-check',b]):
            n+=1
            a=run(proto,d,args); c=run(comp,d,args)
            if a!=c:
                dd+=1
                print(f'## {i} {os.path.relpath(f,items+"/"+i)} {" ".join(args[:-1])}')
                print('  proto:',a.replace('\n',' | ')[:300]); print('  comp :',c.replace('\n',' | ')[:300])
    print(f'#### {i}: runs {n}, diffs {dd}')
