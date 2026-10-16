# V127 の反証

## 裏取り

行番号と件数はおおむね正しい。
unify.ml 453〜455(rewrite_row)、500(§8.7 の記事)、561(TArrow の分岐)、elab.ml 276(row_failure)、1716〜1737(T.Apply)、1914(perform)、2159(elab_check)、parser.mly 184〜189、syntax.ml 1034、l_body の出現 56 か所、test/*.t が 40 ファイル、eval.t 792 の with2、trailing_block.t 88〜90 と 104 の tuple2、LangSpec 110 と 186、台帳 261010-2-m53.md:717、計画 906 と 4491 を確かめた。
§6.5 の with _ の記述は 672〜676 行で、下書きの 673〜676 は 1 行ずれる。
test/arity.t はプログラム 8 本、コマンド 16 個(cat 8 個と diktor 8 個)で、下書きの「コマンド 8 個」は数え方の規則に合わない。

## 実測の再現

担当の ex/ の 19 本を HEAD と試作で走らせ直し、results と同じ出力を得た(verify/re.head.txt、re.patched.txt)。
試作を写しに当てて dune build と dune runtest を走らせ、期待値が変わるのは with2 と tuple2 の 2 ブロックだけだった(verify/runtest.log)。

## 下書きの主張への反例

終了状態は変わる。
s2.kel(f(x: Int32) に f(1i8, 2) を渡す)は、HEAD では 2:20 の「未実装: 数値接尾辞 1i8」で終了状態 4、試作では 2:18 の「引数の個数が一致しません」で終了状態 1 になる。
check_call_arity を実引数の精緻化より前に置くので、実引数の中の誤りより先に個数を報告する。

順序の変化は要素の型の不一致に限らない。
r4.kel(f(nosuch, 2))は、HEAD では 2:20 の「未束縛の変数: nosuch」、試作では 2:18 の個数の文面になる。
b1、b2 でも位置が実引数から呼び出しへ移る(2:20 から 2:18、3:13 から 3:9)。

試作は個数の不一致のすべての経路を覆わない。
r7.kel(type class Sh[A] { val sh: (A) => Int32 } に let sh() = 1 のインスタンス)は、試作でも「インスタンスメソッド sh がクラス宣言の型を満たしません(ラベル _item がありません(行は閉じています))」のままである。
elab.ml §11.38 の 4546 行が矢印どうしでなく引数の行どうしを Unify.unify ia sa で単一化するので、check_arity を通らない。

V102 の経路と重なる。
r1.kel(newtype Fn1 = Fn1((Int32) => Int32) に Fn1(fn() => 1))は、HEAD では row_failure が「ラベル 」に掛かり、newtype のフィールドの行の案内が付く。
試作ではその案内が消え、個数の文面になる。
elab.ml 2115 行の Construct の単一化が check_arity を通るからである。

pub の言い換えの文面も変わる。
r2.kel(pub let h: (Int32) => Int32 = fn() => 1)は、HEAD では「pub な宣言はエフェクトを起こせません(... 元の報告: ラベル _item がありません ...)」、試作では「注釈された型を満たしません(引数の個数が一致しません ...)」になる。
改善ではあるが、下書きの変わる形の一覧に無く、cram の観測点も無い。

試作の elab.ml は、row_failure の記事(271〜275 行)と let row_failure の間に args_arity と check_call_arity を挟む。
記事が別の関数の前に来るので、新しい関数は記事の後ではなく row_failure の後に置くべきである。

## 退行の探索

HEAD が受理するプログラム 12 本(エイリアスの関数型、module の中の pub、import、型クラスのメソッド、perform と resume、入れ子のハンドラ、ガード、newtype の関数フィールド、0 引数と 1 引数の with、タプルを 1 引数で渡す形、REPL)を両方のビルドで走らせ、出力と終了状態はすべて一致した(verify/e.head.txt、e.patched.txt、repl.head.txt、repl.patched.txt)。
受理するプログラムの退行は見つからなかった。

## 判断事項

P[V127-1] の推奨案に異論は無い。
