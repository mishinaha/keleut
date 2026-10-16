f='final.md'
t=open(f).read()
E=[]
def r(a,b): E.append((a,b))
r("promote 前の `dune runtest` の差分は、13 ブロック 14 コマンド、7 ファイルだった(実測)。\n",
  "promote 前の `dune runtest` の差分は、13 ブロック 14 コマンド、7 ファイルだった(実測)。\nブロックは分野の冒頭の数え方(空行で区切られた cram のブロック)で数え、内訳は V24 が 6 ブロック 6 コマンド、X-premise が 5 ブロック 6 コマンド、V102 と V111 が 1 ブロック 1 コマンドずつである。\n`test/verify_fixes.t` の opres と opres2 は前書きを共有するが、647 行の空行で分かれた 2 つのブロックである。\n`test/premise.t` の pr17 と pr18 は空行を挟まない 1 つのブロックで、前書きも共有する(HEAD の 293〜312 行)。\n")
r("| V24 | `test/verify_fixes.t` opres と opres2(1 ブロック) | `1:1` → `1:22`、`1:1` → `1:23` |\n",
  "| V24 | `test/verify_fixes.t` opres | `1:1` → `1:22` |\n| V24 | `test/verify_fixes.t` opres2 | `1:1` → `1:23` |\n")
r("| X-premise | `test/premise.t` pr2、pr3、pr17、pr18、pr19 | ",
  "| X-premise | `test/premise.t` pr2、pr3、pr17 と pr18(1 ブロック)、pr19 | ")
r("各項目の (5) が書いた期待値の変化(V102 は 1 行、V111 は 1 行、V33 と V34 は 0、X-premise は 6 ブロック、V24 は 5 ブロック 6 コマンド)は、合成後もそのまま成り立つ。\n食い違いは無かった。",
  "各項目の (5) が書いた期待値の変化(V102 は 1 行、V111 は 1 行、V33 と V34 は 0、X-premise は 5 ブロック 6 コマンド、V24 は 6 ブロック 6 コマンド)は、合成後もそのまま成り立つ。\n変わるコマンドと文面は、項目単独の試作と食い違わなかった。")
r("項目の例(各項目の `ex/`、`ex2/`、`verify/ex/`)を、",
  "項目の例(各項目の `doc/log/261017-3-spike/<id>/ex/`、`doc/log/261017-3-spike/<id>/ex2/`、`doc/log/261017-3-spike/<id>/verify/ex/`)を、")
r("- region の V122: `lib/unify.ml` §8.7 の `match` に分岐を足し、§8.7 の記事にも触れる。本分野の V34 と V111 が同じ記事の末尾に段落を足し、剛定数の分岐に `last_rigid_clash` の代入を入れるので、V122 は本分野の後に当てることを望む。",
  "- region の V122: `lib/unify.ml` §8.7 の `match` に分岐を足し(HEAD の 544 行)、§8.8 の記事とコード(601 行の記事と 618 行の `unify_row`)を変える。§8.7 の記事(481〜536 行)には触れないので、本分野の V34 と V111 が足す記事の段落とは重ならない。同じ `match` の剛定数の分岐に V34 が文言の変更を、V111 が `last_rigid_clash` の代入を入れるので、V122 は本分野の後に当てることを望む。")
r("- effects の V127 と X-perform: `lib/elab.ml` の `row_failure` の直後に関数を足す。V111 が `row_failure` の局所関数を `has_sub` に出したので、本分野の後に当てると fuzz 2 で当たる(挿入だけで、手直しは要らない見込み。ビルドは試していない)。",
  "- effects の V127 と X-perform: `lib/elab.ml` の `row_failure` の直後に関数を足す。V111 が `row_failure` の局所関数を `has_sub` に出したので、本分野の後に当てるとこの hunk は fuzz 2 で当たる。V127 は `lib/unify.ml` も触り、hunk 2(§8.7 の記事の末尾の直後に `item_arity` を足す)が fuzz 2、hunk 4(剛定数の分岐の直後に `check_arity` を足す)が fuzz 1 で当たる。どちらも V34 と V111 が変えた記事の末尾と剛定数の分岐(V111 の `last_rigid_clash` の代入)の隣である(`--dry-run` で実測。挿入だけで、手直しは要らない見込み)。合成の上に V127 を当てたビルドと runtest は反証者が走らせ、ctorarity の期待値が変わることを確かめた。")
r("- classes の V110、V136、X-D7: V110 と V136 は `lib/elab.ml` の §11.29 と §11.38 の近くを触り、X-D7 は `test/classes.t` を触る。本分野の合成の上で 3 つの差分はどれも当たる。",
  "- classes の V110、V136、X-D7: V110 と V136 は `lib/elab.ml` の §11.29 と §11.38 の近くを触る。X-D7 が触るのは `test/std_names.t` の末尾の 1 ブロックだけで、本分野のファイルとは重ならない(`test/classes.t` は触らない)。本分野の合成の上で 3 つの差分はどれも当たる。")
for a,b in E:
    n=t.count(a)
    if n!=1: print('COUNT',n,a[:60]); continue
    t=t.replace(a,b)
open(f,'w').write(t)
