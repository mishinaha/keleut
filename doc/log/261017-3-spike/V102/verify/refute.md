# V102 の下書きの反証

## 裏取り

行番号は写し(diktor 39d4091、親 754eb08)で確かめた。
`lib/elab.ml` の `row_failure` は 276〜282 行、§11.18 の記事は 2025〜2054 行、`elab_construct` は 2056 行、`try Unify.unify ety fty` は 2114 行、言い換えは 2115〜2118 行で、下書きのとおりである。
`フィールドの行です` は `lib/*.ml` では elab.ml:2118 の 1 か所だけである。
`test/annot_rows.t` の nonnamelist は 948〜957 行で、期待値の行は 956 行、前書きは 944〜946 行である。
LangSpec §13.2 は 1123 行から始まり、§13.3 が 1181 行なので 1180 行まで続く(下書きは 1123〜1174 行)。
最外以外の矢印の `@` の省略を `@ {}` と読む規則は 1158〜1159 行で、下書きの 1152〜1153 行は誤りである。
制限の 1 つ目の 1169〜1171 行と、§18 の 1782 行は下書きのとおりである。
出典の 261010-1-plan.md:3221-3223、5892、2614-2615、2654 と 261010-2-m53.md:372 は下書きのとおりである。
P[V102-2] の代案が引く「台帳の備考の『フィールドの型に矢印が書いてあるとき』」という文面は、doc/log のどこにも無い(grep で 0 件)。

件数の食い違い:
(1) は新しい cram を「7 ブロック」とし、(5) は「ブロック 4 つ」とする。
試作の test/ctor_field_row.t はプログラム 7 本、ブロック 4 つ、diktor のコマンド 7 つで、(1) が誤りである。
(7) は「ex/ の 15 本」とするが、ex/ の .kel は 16 本である。
担当の要約の「8 形が的外れ」に当たる記述は下書きに無く、表の的外れは 6 行である。

## 実測の再現

担当の ex/ の 16 本を HEAD と試作で走らせ直した。
終了状態はすべて同じである。
文面が変わるのは mixed、nestedname、nnlist、pairtp、rowparam、tworow の 6 本で、どれも言い換えが消えるだけである。
alias、box、explicit、nestedfield、recfield、tuplefield、mixed3 は両方で言い換えを添え、pa2 と nonrow は両方で添えない。
下書きの表と一致する。

## 試作の差分

V102.patch は写しに当たった。
dune build は通った(parser.mly の Warning 60 は HEAD の写しでも出る)。
dune runtest の差分は nonnamelist の 1 行だけで、ctor_field_row.t は通った。
adv5 と launder は変わらない。
litwrap を走らせても elab.ml は変わらず、litcheck は elab.ml の marker 0 件、code は不一致(関数を足したため)である。

## 退行

`when` の条件を狭めるだけで、`row_failure` が先に評価されるので `has_closed_arrow` はエラーの経路でしか呼ばれない。
受理されるプログラムの出力は変わらない(ソースを読んで導いた性質)。
実測でも、HEAD が受理する次の 6 本(verify/ex/)は両方で同じ出力だった。
ok1(module の pub newtype、エイリアスのフィールド、型パラメータ)、ok2(Cb[R] の行変数)、ok3(行変数のフィールドとガード)、ok4(handle の中の newtype)、ok5(pub newtype と pub let)、ok6(ガードの中のフィールドの関数)、impok(import した newtype)。
REPL(repl.in)は終了状態 0 のまま、nnlist の形の言い換えが消えるだけである。
--no-prelude で自前のリストを使う noprel も、言い換えが消えるだけである。

## 下書きが見落とした形

1. フィールドに書いた閉じた行が、行の型パラメータを取る型の型引数にあるとき(rowarg、rowarg2)。

```
newtype Cb[R] = Cb(() => Int32 @ R)
newtype W = W(Cb[{}])
let b(): Int32 @ Console = { echo("b"); 2 }
let w = W(Cb(b))
```

HEAD: `! rowarg.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)(コンストラクタ W のフィールドの行です。… 行を通すなら行変数を型パラメータに取ってください。§9)`
試作: `! rowarg.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)`
原因はフィールドに書いた `{}` で、案内の後半(行変数を型パラメータに取る)は的確である。
試作はこの案内を消す。
下書きの (6) は「案内が消えて困る形」を挙げず、残りの一覧にも無い。
`has_closed_arrow` の `TCon` の型引数で、閉じた行そのもの(`TRowEmpty`、または `TRowExtend` が `TRowEmpty` で終わるもの)も数える変種を写し(verify/alt)で試した。
rowarg と rowarg2 で案内が戻り、runtest の差分は nonnamelist の 1 行のまま、担当の ex/ 16 本の出力は試作と同じだった。

2. 行の失敗が効果の行でなく、引数の行やレコードの行のとき(arity、recmiss)。

```
newtype Box = Box((Int32) => Int32)
let k(): Int32 = 1
let x = Box(k)
```

HEAD と試作のどちらも `ラベル _item がありません(行は閉じています)(コンストラクタ Box のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み…)` を出す。
原因は引数の個数で、効果の行の案内は的外れである。
`newtype R = R({f: () => Int32, g: Int32})` に `R({f = fn() => 1})` を渡す recmiss も、欠けたフィールド g について同じ案内を出す。
`row_failure` は `ラベル ` を含む文面を広く拾うので、フィールドの型に閉じた矢印があれば、効果と関係の無い行の失敗にも案内が付く。
下書きは残りを ctormixed だけとするが、この 2 形も残る。
逆に、矢印の無いレコードのフィールド(recmissnoarrow)では試作が的外れの案内を消すので、改善の側の形としても表に載せるとよい。

## 仕様との整合

LangSpec を変えない判断は、§18 の 1782 行(診断の文面は評価の結果に含めない)と、§15.5 の終了状態が変わらないこと(実測で全例同じ)に合う。
付録 A の文法には触れない。

## ほかの項目との重なり

V34 の試作は `row_failure` の直後(281 行の後)に `rigid_empty_failure` を足し、V102 も同じ位置に `has_closed_arrow` を足すので、差分の文脈がぶつかる。
V111 は `row_failure` の局所関数 `has` を最上位の `has_sub` に出すので、同じ定義の本体を書き換える。
V127 の試作は 273 行の前後に `args_arity` などを足す。
V127 と X-perform は、`row_failure` が `ラベル _item がありません` を行の失敗とみなす問題を扱い、上の arity の形はその問題の §11.18 側の現れである。
test/annot_rows.t は V101(opsig2)、V111(recrejtpflow)、V97(instmonorow)、V32 も変えるが、ブロックは nonnamelist と重ならない。
