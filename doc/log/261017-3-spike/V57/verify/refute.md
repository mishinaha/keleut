# V57 の下書きの反証

## 裏取り

行番号は HEAD の写しで合っている。
`lib/elab.ml:895` が `find_row_alias`、`:912-926` が `find_alias`、`:1168-1170` が `T.EIdent` の型パラメータの分岐、`:1211` が `T.BLabel`、`:1222-1227` が `None` の分岐と行変数の診断である。
§11.6 の記事は `lib/elab.ml:1021` から始まり、`:1072` が行変数の中置の合成の文である。
diktor の `doc/log/260926-2-m34.md` の `:176`、`:344`、`:457` は、それぞれ指摘 4 の台帳化、台帳 V57、訂正 7 である。
`test/visibility.t:342-352` は visrow12 と visrow13 のブロック、`:363-364` は visrow15 の最後の 2 行である。
`LangSpec.md:1105-1116` は §13.1 の記法の表と別名の段落である。
`extends の位置` は、diktor の `test/`、親の `doc/sample.kel`、`doc/LangSpec.md` のどれにも 0 行である。

誤りが 1 つある。
(2) は b6 の原因を「`find_alias` の 2 つ目の場所 `resolve_con_global` がトップレベルのエイリアスに当たるため」と書く。
`Decls.resolve_con`(`lib/decls.ml:439-443`)は、`current_module` が `None` のとき `resolve_con_global` そのものであり、module の中でも同義語が無ければ `resolve_con_global` に落ちる。
したがって b6 では 1 つ目の場所でエイリアスが当たる(ソースから)。
2 つ目の場所が効くのは、module の同義語が行にならない宣言を指すときだけである。

件数の誤りが 2 つある。
cram の追加は、課題の数え方(前書きとコマンドの組)では 2 ブロックである(前書きは 2 つ)。
(1) の「cram 5 コマンド組」と (5) の「5 ブロック」はどちらも数え方が違う。
コマンドは 10、プログラムは printf で作る 5 本である。
差分の「82 行の増減」は、`+++` と `---` の見出し 4 行を含めた数である。
増減の行は 59 行の増加と 19 行の減少で、計 78 行である。

## 実測の再現

担当の 19 本を HEAD と試作で走らせ直した(`mine.head.txt`、`mine.new.txt`)。
差は b1、b2、b3、b6、b8、b14、b15 の 7 本の文面だけで、位置と終了コードは同じだった。
b4、b5、b9 から b13、b16 から b18 は同一だった。
V56 の例 17 本では、差は V56 の `ex/b1.kel` の 1 本だけだった。
sample の型検査(`--type-check sample/stubs.kel sample/sample.kel`)は、どちらも終了コード 0、68 行で同一だった。

## 試作

`V57.patch` は写しにそのまま当たった。
`dune build` は通り、`dune runtest` の出力は空だった(`runtest.log`)。
`tools/litwrap.py lib/elab.ml` は差分を出さなかった。
`tools/litcheck.py` は写しに git が無いので HEAD との比較ができず、確かめられなかった。
担当の `runtest.head-code-new-test.log` では、HEAD のコードで visrow16 から visrow16d の 4 コマンドが落ち、visrow16e は通る。
分類(新しいテストだけが変わり、既存の期待値は変わらない)は正しい。

## 退行の探索

ソースから: 新しい検査が発火するのは `args = []`、`n` が `env.types` にあり、`find_row_alias n` が `None` のときである。
この条件で HEAD の `find_alias` は、effect の宣言表に `n` があれば `None` を返して同じ行変数の診断に当たり、そうでなければ行にならないエイリアスを返すか可視性の誤りを投げる。
どちらも型エラーなので、受理と拒否は変わらず、文面だけが変わる。

実測(`ex/`): HEAD が受理して実行する 8 本を両方のビルドで走らせ、出力と終了コードは同一だった。
r1(module の EffectRow エイリアスと同名の型パラメータを波括弧の中に書く)、r2(トップレベルの Type エイリアスと同名の型パラメータ、ガードつきのハンドラ、`@ E`)、r3(pub の EffectRow コンパニオンと同名の型パラメータ)、r4(import した Type エイリアスと EffectRow エイリアス)、r5(module の Type エイリアスと同名の newtype の型パラメータ、入れ子のハンドラ)、r6(effect と別の名前の Type エイリアスを型パラメータ名にして `run` とリージョン)、r7(トップレベルの EffectRow エイリアスを module の Type エイリアスが覆う形)、r9(`--prelude` で差し替えたプレリュードの Type エイリアス)。
REPL(r8.repl)では、受理された行は同一で、拒否される 4 行目の文面だけが行変数の診断に変わった。
退行は見つからなかった。

## 文面の誤り

試作のコメント(`lib/elab.ml` §11.6 の追記)は「同名の Type エイリアスやコンパニオンがあっても、診断は変わらない」と書く。
cram の前書きは「同名の module のコンパニオンが pub でも pub でなくても、診断は変わらない」と書く。
どちらも、pub でない EffectRow エイリアスのコンパニオンには当たらない。
担当の b18(`module W { type W: EffectRow = {Print} }` と `let f[W](x: Int32): Int32 @ {W} = x`)は試作でも「型 W.W は module W の外からは参照できません(pub を付けてください)」である。
「Type エイリアスのコンパニオン」に限って書く必要がある。

## 判断事項

推奨案(V56 と別の修正にする)に異論は無い。
代案が visrow12 と visrow13 の期待値を変えることは、ソースから正しい(`find_alias` が `None` を返すと `unknown_effect` の文面になる)。
見落とした代案として、波括弧の中でも型パラメータを EffectRow エイリアスより先に引く形がある。
これは V50 の非対称も閉じるが、b11 と visrow16e の受理を拒否に変えるので、V57 の範囲(受理と拒否を変えない)を越える。
V50 は diktor の `doc/log/260926-1-plan.md:4411` で「残す」とされ、`doc/sample.kel:467` の略記の規則と食い違うと記録されている。
visrow16e はこの食い違いの片側を cram に固定するので、前書きか計画にその旨を書いておくのがよい。
