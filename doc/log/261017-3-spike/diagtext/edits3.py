f='final.md'
t=open(f).read()
E=[]
def r(a,b): E.append((a,b))
r("どちらも fuzz 2 で当たる(挿入だけで、ビルドは試していない)。",
  "どちらも fuzz 2 で当たる(挿入だけである)。V127 を合成の上に当てた写しでは、反証者が `dune build` と `dune runtest` を走らせた(N.z)。")
r("- ctorarity の期待値は、V127 または X-perform が `_item` の個数の不一致を `row_failure` より先に報告するようにすると変わりうる。その項目の側で `test/ctor_field_row.t` の 5 つ目のブロックを見直す。",
  "- ctorarity の期待値は、`_item` の個数の不一致を `row_failure` より先に報告する項目で変わる。V127 を合成の上に当てると、ctorarity は「引数の個数が一致しません: () => Int32 と (Int32) => Int32 @ {}(引数は 0 個と 1 個です)」に変わった(反証者の実測)。V127 の側で `test/ctor_field_row.t` の最後のブロック(残りの 3 形)の期待値を更新する。X-perform も同じ箇所を触るので、同じく見直す。")
r("合成の上に V127 を当てたビルドと runtest は反証者が走らせ、ctorarity の期待値が変わることを確かめた。`_item` の個数の不一致の報告を変えると、V102 の `test/ctor_field_row.t` の ctorarity の期待値が変わりうるので、その項目の側で期待値を見直してほしい。",
  "合成の上に V127 を当てた写しでは、反証者が `dune build` と `dune runtest` を走らせ、V102 の `test/ctor_field_row.t` の ctorarity が「引数の個数が一致しません: () => Int32 と (Int32) => Int32 @ {}(引数は 0 個と 1 個です)」に変わることを確かめた。V127 の側でこの期待値を更新し、X-perform の側でも同じブロックを見直してほしい。")
for a,b in E:
    n=t.count(a)
    if n!=1: print('COUNT',n,a[:60]); continue
    t=t.replace(a,b)
t=t.rstrip('\n')+'''

#### 検証の反映

- 直すべき指摘 1(X-D7 が `test/classes.t` を触るという記述): 反映した。X-D7 の差分が触るのは `test/std_names.t` の末尾の 1 ブロックだけであることを差分の見出しで確かめ、N.3 の (6) と (8)、N.5 の (6)、上の classes への要望を「重ならない」と書き直した。
- 直すべき指摘 2(ブロックの数の内訳): 反映した。数え方を cram のブロック(空行で区切られた段。`export` の段を含む)に決めて分野の冒頭に書き、V24 を 6 ブロック 6 コマンド、X-premise を 5 ブロック 6 コマンドに直した(合計 13 ブロック 14 コマンドは変わらない)。N.5 の (5) から (8)、N.6 の (1)、(5)、(7)、(8)、上の表と本文、`runtest.log` の内訳を直した。
- 直すべき指摘 3(V122 が §8.7 の記事に触れるという記述): 反映した。V122 の hunk は §8.7 の `match`(544 行)、§8.8 の記事(601 行)、§8.8 の `unify_row`(618 行)にあることを差分で確かめ、N.4 の (6) と上の region への要望を直した。X-D74 が §8.7 の記事(527 行)に触れるという記述は正しいので残した。
- 文章の規範(「第11章」): 反映した。「第 11 章」に直した。
- 文章の規範(短い相対パス): 反映した。V102、V111、V34、X-premise、V24 の小節と上の例の比較で、`ex/`、`verify/ex/`、`results.*.txt`、`final-work/p.after.txt` などを `doc/log/261017-3-spike/<id>/` で始まる形にした。N.4 の (6) の `h2z.kel` は V33 の例なので `doc/log/261017-3-spike/V33/ex2/h2z.kel` とした。
- 文章の規範(ブロックの数え方が小節ごとに違う): 反映した。新しい 4 ファイルをどれも `export` の段を含めて数え、`test/ctor_field_row.t` は 6 ブロック、`test/rec_tparam_note.t` は 8 ブロック、`test/rigid_empty_row.t` は 5 ブロック、`test/decl_pos.t` は 9 ブロック(effpos2 と effpos3 は空行で分かれる)とした。`test/decl_pos.t` で HEAD に落ちるのは 7 ブロック 8 プログラムになる。合成した写しのファイルを数え直して確かめた。`$` の行と `diktor` のコマンドも言い分けた。
- 文章の規範(V127 と X-perform の要望が `elab.ml` の fuzz だけを書く): 反映した。合成の上で V127 を `--dry-run` で当て直し、`lib/unify.ml` の hunk 2 が fuzz 2、hunk 4 が fuzz 1 で当たり、V34 と V111 が変えた記事の末尾と剛定数の分岐の隣であることを上の要望に書いた。V127 で ctorarity の期待値が変わるという反証者の実測も N.1 の (6) と要望に書いた。
- 文章の規範(order.txt の「V34 の 5 文」): 反映した。節の本文はもとから 4 文と書いており、作業領域の order.txt も 4 文に直した。
- 合成差分: 手直しは無い。指摘はどれも本文の記述と数え方で、`doc/log/261017-3-spike/diagtext/diagtext.patch` の内容は変えていない。
'''
open(f,'w').write(t)
