import re
W='<一時領域>/clusters/effects/work/'
def split(f):
    s=open(W+f).read().split('\n')
    secs={}; cur=None
    for l in s[1:]:
        m=re.match(r'^#### \((\d)\)',l)
        if m: cur=m.group(1); secs[cur]=[]; continue
        if cur: secs[cur].append(l)
    return {k:'\n'.join(v).strip('\n') for k,v in secs.items()}
a=split('V101.md'); b=split('X-perform.md')
heads={'1':'判定、規模、依存、出典','2':'現状(根拠つき)','3':'判断と確定','4':'仕様側の改訂案','5':'diktor 側の変更','6':'リスク','7':'検証の観点','8':'検証の反映'}
extra=open(W+'merge_extra.txt').read()
ex={}
for blk in extra.split('@@@')[1:]:
    k,_,body=blk.partition('\n'); ex[k.strip()]=body.strip('\n')
out=['### N.4 V101：perform と resume の引数を検査モードに通す(X-perform を吸収)','']
out.append(ex['intro']); out.append('')
for k in '12345678':
    out.append(f'#### ({k}) {heads[k]}'); out.append('')
    if 'pre'+k in ex: out.append(ex['pre'+k]); out.append('')
    out.append('**V101 の部分。**'); out.append('')
    out.append(a[k]); out.append('')
    out.append('**X-perform の部分。**'); out.append('')
    out.append(b[k]); out.append('')
    if 'post'+k in ex: out.append(ex['post'+k]); out.append('')
open(W+'V101m.md','w').write('\n'.join(out).rstrip('\n')+'\n')
