import sys,subprocess,os,re
S,D=sys.argv[1],sys.argv[2]
txt=open(S+'/results.final.txt').read()
ents=re.split(r'^== ',txt,flags=re.M)[1:]
diff=0
for e in ents:
    head,rest=e.split('\n',1)
    if '-- C\n' not in rest: continue
    exp=rest.split('-- C\n',1)[1].rstrip('\n')
    name=head.split()[0]
    repl='--repl' in head
    cands=[S+'/'+name, S+'/verify/'+name, S+'/verify/ex/'+os.path.basename(name), S+'/ex2/'+name]
    p=next((c for c in cands if os.path.exists(c)),None)
    if not p: print('MISSING',name); continue
    d,b=os.path.split(p)
    if repl:
        r=subprocess.run([D,'--repl'],cwd=d,stdin=open(p),capture_output=True,text=True)
    else:
        r=subprocess.run([D,b],cwd=d,capture_output=True,text=True,timeout=10)
    got=(r.stdout+r.stderr).rstrip('\n')+('\nexit %d'%r.returncode)
    if got.strip()!=exp.strip():
        diff+=1; print('DIFF',name); print(' exp:',exp.replace('\n',' | ')[:400]); print(' got:',got.replace('\n',' | ')[:400])
print('entries',len(ents),'diffs',diff)
