import re, sys
S = sys.argv[1]; out = sys.argv[2]
items = [("V47","module の外の非修飾の `@ W[E]` が、案内の無い「未知の型: W」になる"),
         ("V50","型パラメータと同名の EffectRow エイリアスで、`@ W` と `@ {W}` が別のものを指す"),
         ("V44","pub の言い換えを、本体のエフェクトが原因の失敗に限る"),
         ("V49","剛定数の脱出の報告が、原因(外側の単相の型変数との共有)を名指ししない"),
         ("V53","総和性の診断の「v0 では後送りできません」が §13.4 と言い回しで合わない"),
         ("V48","値の無い型を受けるパターンの警告がパターンの形で変わる")]
mat = re.compile(r'`((?:results[^`]*\.txt|runtest[^`]*\.log|V\d+(?:\.draft|-head|\.ab)?\.patch|x\.proto2\.txt|(?:ex|exb|ex2|verify)/[^`]*))`')
res = []
for k,(i,t) in enumerate(items,1):
    txt = open(f"{S}/items/{i}/final.md").read().split("\n")
    assert txt[0].startswith("## ")
    body = []
    for line in txt[1:]:
        if line.startswith("### ("):
            line = "#### " + line[4:]
        line = mat.sub(lambda m: f"`doc/log/261017-3-spike/{i}/{m.group(1)}`", line)
        body.append(line)
    res.append(f"### N.{k} {i}：{t}\n" + "\n".join(body).rstrip() + "\n")
open(out,"w").write("\n".join(res))
