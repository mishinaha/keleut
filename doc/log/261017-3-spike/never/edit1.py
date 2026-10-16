import sys
p=sys.argv[1]; s=open(p,encoding='utf-8').read()
def rep(old,new):
    global s
    assert s.count(old)==1,(old,s.count(old))
    s=s.replace(old,new)
rep("見出しは `diktor/` から始まるので、diktor の中で `patch -p1` で当てる)。\n合成の手順と手直しは同じ場所の `order.txt`、`runtest` の分類は `runtest.log` にある。",
    "見出しは `diktor.orig/` と `diktor/` から始まるので、diktor の中で `patch -p1` で当てる)。\n合成の手順、手直し、`runtest` の分類は N.z に書く。")
rep("`lib/exhaust.ml` に 31 行の追加、","`lib/exhaust.ml` に 34 行の追加と 2 行の削除、")
rep("推奨の根拠: V134 の後は、`perform` の式を照合する形は補助関数や値束縛を経由したものも含めて警告が出ないので、案内が出る場面は V[V134-1] の最上位の式文に限られる。",
    "推奨の根拠: V134 の後は、`perform` の式を照合する形は、補助関数や値束縛を経由したものも含めて、照合対象の型変数が束縛の型に現れない限り警告が出ない。案内が出る場面は、V[V134-1] の最上位の式文と、照合対象が束縛の型に現れる形に限られる。後者の例は `let f() = { let r = perform throw(\"x\"); let q = r match {}; r }` で、`r` の型が `f` の返り値の型に現れるので警告が残る(反証者が合成後の処理系で実測。`--strict-exhaustive` で終了状態 1)。")
rep("の警告 7 行が新しい文面になる(4 ブロック、6 プログラム)。","の警告 7 行が新しい文面になる(4 ブロック、5 プログラム、コマンド 7)。")
rep("合成では、例を「注釈のない引数を照合する `fn(x) => x match {}` がその例である。照合対象の型変数が束縛の型に現れるので、上の規則で `Never` に決めない。」の 2 文に替えた。\n",
    "合成では、例を「注釈のない引数を照合する `fn(x) => x match {}` がその例である。照合対象の型変数が束縛の型に現れるので、上の規則で `Never` に決めない。」の 2 文に替えた。\n"
    "この 2 文目の「上の規則」は、V134 が §10.13 に足す段落(N.1 (5) のソースコメントの、決める条件と送りの段落)を指す。\n"
    "合成差分の V134 のコメントは関数に付けた短い仮のもので、その段落はまだ無い(合成差分を読んで確かめた)。\n"
    "実施では V134 の §10.13 の段落を先に書き、本項の記事はその後ろに足すことを前提とする。\n"
    "V134 の段落の書き方が変わったら、本項の 2 文目の参照もそれに合わせる。\n")
rep("合成差分は `doc/log/261017-3-spike/never/never.patch`(437 行。`lib/elab.ml` 12 行の変更、`lib/exhaust.ml` 68 行の追加、`test/empty_match.t` 173 行の新設、`test/never_ops.t` 79 行の追加と変更)である。",
    "合成差分は `doc/log/261017-3-spike/never/never.patch`(437 行。`lib/elab.ml` 6 行の書き換え、`lib/exhaust.ml` 65 行の追加と 3 行の削除、`test/empty_match.t` 173 行の新設、`test/never_ops.t` 76 行の追加と 3 行の削除)である。")
rep("衝突と手直し(`order.txt` に記録した):","衝突と手直し:")
rep("合成後の runtest(`runtest.log`):","合成後の runtest(実測):")
rep("V136 の後、V129 の `verify/ex/p03_instnever.kel` の警告も消える見込みである(未測定)。",
    "合成の上に V136 の差分を当てると、V129 の `verify/ex/p03_instnever.kel` も警告なしで `i` を出力した(反証者の実測)。")
open(p,'w',encoding='utf-8').write(s)
