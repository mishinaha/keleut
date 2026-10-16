import re,sys
S='<一時領域>/items'
order=[('V25',1),('V58',2),('V127',3),('V101',4),('X-perform',None),('V133',5),('V89',6),('V128',7)]
heads={'1':'判定、規模、依存、出典','2':'現状(根拠つき)','3':'判断と確定','4':'仕様側の改訂案','5':'diktor 側の変更','6':'リスク','7':'検証の観点','8':'検証の反映'}
filepat=re.compile(r'`((?:\.\./V101/)?(?:ex2?|verify)/[^`]*|[A-Za-z0-9_.-]*\.patch|results[^`]*\.txt|runtest[^`]*\.(?:log|txt)|litcheck\.final\.txt)`')
for id_,k in order:
    t=open(f'{S}/{id_}/final.md').read()
    lines=t.split('\n')
    out=[]
    for i,l in enumerate(lines):
        if i==0 and l.startswith('## '):
            title=re.sub(r'^## '+re.escape(id_)+r'[:：]?\s*','',l)
            title=re.sub(r'^## ','',title)
            out.append(f'### N.{k} {id_}：{title}' if k else f'### (吸収) {id_}：{title}')
            continue
        m=re.match(r'^### \((\d)\)',l)
        if m:
            out.append(f'#### ({m.group(1)}) {heads[m.group(1)]}'); continue
        def rep(mm):
            p=mm.group(1)
            if p.startswith('../V101/'):
                return f'`doc/log/261017-3-spike/V101/{p[8:]}`'
            return f'`doc/log/261017-3-spike/{id_}/{p}`'
        l=filepat.sub(rep,l)
        out.append(l)
    open(f'<一時領域>/clusters/effects/work/{id_}.md','w').write('\n'.join(out))
