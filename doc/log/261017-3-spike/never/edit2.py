import sys
p=sys.argv[1]; L=open(p,encoding='utf-8').read().split('\n')
R={
14:"分野の合成差分は `doc/log/261017-3-spike/never/never.patch` である(diktor 39d4091 に対する差分)。\n見出しは `diktor.orig/` と `diktor/` から始まるので、diktor の中で `patch -p1` で当てる。",
24:"規模は S である。\n`lib/exhaust.ml` に 34 行の追加と 2 行の削除、`lib/elab.ml` の `Exhaust.drain` の呼び出し 6 か所の書き換えで、試作の差分 `doc/log/261017-3-spike/V134/V134.patch` は 111 行である。\ncram は 1 ファイルの新設と既存 1 ブロックの期待値の変更である。",
177:"- 警告の順序: 送ったエントリの警告は外側の drain で出るので、1 つの宣言の中で順序が変わる(`r4_order.kel`)。\n  HEAD は「例えば _」「例えば 0」の順、試作は逆の順である(実測)。",
244:"規模は S である。\n`lib/exhaust.ml` のコードが 12 行ほど、記事が 17 行ほどである。\ncram は 2 ブロック、2 プログラム、コマンド 5 行の追加と、V134 の `test/empty_match.t` の 7 行の期待値の変更である。",
313:"後者の例は `let f() = { let r = perform throw(\"x\"); let q = r match {}; r }` で、`r` の型が `f` の返り値の型に現れるので警告が残る(反証者が合成後の処理系で実測し、`--strict-exhaustive` で終了状態 1)。",
475:"HEAD での実測(`ex/` の各プログラム、入出力の全文は `results.head.txt`)は次の表のとおりである。\nPRE は規則 1 の直前の dbcb088~1 を写しで build したものである。",
522:"  - 代案 B は写しで試作した(`V135-alt-default-never.patch`)。\n    `lib/unify.ml` の `check_ambiguity` に 8 行を足すもので、既存の cram は全部通る。",
526:"  - 代案 C は、印が `let fail(m: String) = perform throw(m)` の一般化を越えないので `b_helper` を救えない(ソースを読んで導いた性質)。\n    一般化された型変数は具体化のたびに新しい変数になり、印は写らない。",
569:"cram: `test/never_ops.t` の absurdann のブロックの後ろ(HEAD の 119 行の後ろ)に 2 ブロックを足す(試作 `V135.patch`)。\n単独で `dune runtest` は差分なしで、`runtest.log` は空である。",
636:"合成差分は `doc/log/261017-3-spike/never/never.patch` で、437 行である。\n内訳は、`lib/elab.ml` 6 行の書き換え、`lib/exhaust.ml` 65 行の追加と 3 行の削除、`test/empty_match.t` 173 行の新設、`test/never_ops.t` 76 行の追加と 3 行の削除である。",
}
for k,v in R.items():
    assert L[k-1].strip(), k
    L[k-1]=v
open(p,'w',encoding='utf-8').write('\n'.join(L))
