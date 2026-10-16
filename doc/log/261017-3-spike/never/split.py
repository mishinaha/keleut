import sys,re
p=sys.argv[1]; lines=open(p,encoding='utf-8').read().split('\n')
out=[];code=False;n=0
opens='(（「『';closes=')）」』'
for ln in lines:
    if ln.startswith('```'):
        code=not code; out.append(ln); continue
    if code or ln.lstrip().startswith('|') or ln.startswith('#'):
        out.append(ln); continue
    m=re.match(r'^(\s*)(- |\d+\. )?',ln)
    ind=m.group(1); mk=m.group(2) or ''
    cont=ind+(' '*len(mk))
    parts=[];cur='';d=0;bt=False
    for i,ch in enumerate(ln):
        cur+=ch
        if ch=='`': bt=not bt
        elif bt: continue
        elif ch in opens: d+=1
        elif ch in closes: d=max(0,d-1)
        elif ch=='。' and d==0:
            rest=ln[i+1:]
            if rest.strip():
                parts.append(cur); cur=''
    parts.append(cur)
    if len(parts)>1: n+=1
    out.append(parts[0])
    for q in parts[1:]:
        out.append(cont+q.lstrip())
open(p,'w',encoding='utf-8').write('\n'.join(out))
print('split lines',n)
