# V101 の下書きの反証

## 1. 裏取り(行番号、件数)

- `lib/elab.ml` の 1914 行(`Unify.unify (elab_exp env level eff arg) args_row`)、1964 行(`Unify.unify (elab_exp env level eff e) op_ret`)、1736 行(`elab_check env level eff arg pvar`)、2159 行(`and elab_check`)は、写しのとおりである。
- 節の見出しは §11.11 が 1550 行、§11.12 が 1658 行、§11.15 が 1870 行、§11.16 が 1925 行、§11.19 が 2122 行、§11.20 が 2192 行、§11.26 が 2809 行で、下書きの § 番号と合う。
- LangSpec の 1136、1137、1170 行の文面は下書きの引用どおりである。
  §13.3 は 1181〜1210 行で、下書きの 1181〜1209 行とほぼ合う。
- 出典 `261010-1-plan.md:3217-3220`、`:5891`、`261010-2-m53.md:692` は、それぞれ V101 の段落、台帳の行、延期のままの行である。
- `test/annot_rows.t` の noheadperform は 778〜785 行、noheadresume は 786〜798 行である。
  下書きの「noheadresume(778〜800 行)」は 2 ブロックを合わせた範囲で、noheadresume だけなら 786〜798 行である(軽微)。
- sample.kel 480 行は「関数型を要求する位置（引数、返り値や値束縛の注釈）」と書き、`perform` の引数を除外していない。
  下書きの「sample.kel は変えない」は妥当で、むしろ試作は sample.kel の「引数」の読みに近づく。

## 2. 実測の再現

- 担当の例 17 本(`ex/`)を HEAD と試作の両方で走らせ直し、`results.head.txt`、`results.new.txt`、`results.diag.txt` と同じ出力を得た(`verify/run_draft.txt`)。
- p7 は試作で実行まで通り、`after` を出力して終了状態 0 である。
- e5 は「型の不一致」ではなく `ラベル _item がありません(行は閉じています)` で、位置が 2:25 から 2:37 に移る。
  下書き 2 節の表で e1、e2 と同じ「引数の型の不一致」にまとめているのは不正確である(文面が同じで位置だけ移る点は正しい)。
- 7 節の「`ex/` の 9 本(p1〜p8、r1、r2)」は列挙が 10 本で、p3 を除くと 9 本になる。
  数え方を書き分けるとよい(軽微)。

## 3. 試作の差分

- `V101.patch` は `verify/diktor` にそのまま当たった。
- `dune build` は通り、`dune runtest` の出力は担当の `runtest.log` と同一である(`annot_rows.t` の opsig2 の 1 行が 4:48 から 4:63 に変わるだけ。`verify/runtest.out`)。
- 一時的な git の写し(`verify/gitcheck`)で `tools/litcheck.py` を走らせた。
  elab.ml のコードの字句の不一致は試作の 1 か所目(1914 行の置き換え)から始まるものだけで、marker は 0 件、ほかのファイルはすべて一致である。
  `tools/litwrap.py lib/elab.ml` は試作の elab.ml を変えなかった。
  下書きの「写しでは litcheck のコードの一致を確かめていない」は、この方法で確かめられる。

## 4. 退行の探索

HEAD が受理し正しく実行する形で、試作が拒否するか出力を変える形は見つからなかった。
試した形(`verify/ex/`、出力は `run1.txt`〜`run10.txt`)は次のとおりである。

- 操作の引数の型がエイリアス(x01alias、y1aliaslam、y5aliasnn、y6aliasres)、行のエイリアス(y7rowalias)。
- 無名関数の本体で型クラスのメソッドを呼ぶ(x02show)、型パラメータと制約を持つ関数の中の perform(x11tparam)、多相の関数を渡す(x10poly)、既定化される数値リテラル(x20lit)。
- 入れ子のハンドラ(x04nested)、ガードで引数の関数を呼ぶ(x05guard)、注釈つきの引数を持つ無名関数(x06annot)、module の中の perform(x07module)、resume に無名関数(x08resumefn、x16resumeann)、無名関数の本体の match(x09opt)、Never の操作を起こす無名関数(x12never)、操作節の中の perform(x19resumein)。
- レコード式(x22rec)、import した effect(imp/main.kel)、`--prelude` で差し替えたプレリュードの effect(x21prelude)、REPL(repl.in)、`pub let`(x17pubann)。
- 網羅性の警告と `--strict-exhaustive`(x15strict、z1warnorder、z2warnorder)は、文面も件数も終了状態も同じである。

受理が変わった形はすべて HEAD で拒否、試作で受理の向きである(REPL の 4 行目、imp/main.kel、y1、y3、y4、y5、y6、y7)。

## 5. 下書きに無い診断の変化

次の 3 つは試作で文面か位置が変わるが、下書きの 6 節に無い。

- 操作の型の矢印の行に候補が 2 つ以上あるとき、修飾を求める文面が 2 段目の「行に現れる」版に変わる(x18ambig)。
  HEAD: `x18ambig.kel:3:47: 型エラー: 操作 write は複数のエフェクト(Console, File)に属します。Console.write のように修飾してください`
  試作: `x18ambig.kel:3:47: 型エラー: 操作 write は、この位置の行に現れる複数のエフェクト(Console, File)に属します。Console.write、File.write のどれかに修飾してください`
- 純粋な矢印(`() => Unit`)を要求する操作の引数に、perform を起こす無名関数を渡すと、文面と位置が変わる(w1pureperf、w2pubeff)。
  HEAD: `w1pureperf.kel:3:32: 型エラー: ラベル Get がありません(行は閉じています)`
  試作: `w1pureperf.kel:3:52: 型エラー: エフェクト Get をここでは実行できません(ラベル Get がありません(行は閉じています))`
  関数呼び出しの同じ形(w5callpure)は HEAD でも試作の文面になるので、そろう向きの変化である。
- 返り値が Unit の操作の `resume(1i32)` の位置が 2:60 から 2:67 に移る(x14resumeunit)。
  e2 と同じ類である。

## 6. 仕様の文面の案

- §13.3 に足す段落は「実引数に直接書いた無名関数の本体」に限って書いているが、実測では入れ子の形にも働く。
  `perform reg((fn() => perform write("x"), 1i32))`(y3tuplelam)と `perform reg(fn() => fn() => perform write("x"))`(y4nestlam)は試作で受理になり、関数呼び出しの同じ形(y8calltuple の 4 行目)は HEAD で受理である。
  1137 行の「入れ子になっていてもよい」と同じ範囲を書くべきである。
- 一方で、型注釈が関数型である値束縛の初期化式に書いた無名関数には働かない(y8calltuple の 5 行目 `let anno: () => Unit @ File = fn() => perform write("z")` は HEAD と試作の両方で拒否)。
  したがって、段落を「関数型を要求する位置の無名関数」一般に広げて書いてはいけない。
- 仮引数の型が型パラメータのときは押し込まない(y9generic は両方で拒否)。
  段落の「仮引数の型の対応する矢印の行」は、対応する部分が関数型のときに限ることを明示するとよい。
- §18(保存する性質)のうち §18.4(型エイリアスへの置換)とは、y1、y5、y6、y7 の実測で、エイリアスを通しても直接書いた形と同じ受理になるので矛盾しない。
- §15.5 の終了状態は変わらない(型エラーは 1 のまま)。
  付録 A の文法は変えない。

## 7. 判断事項

P[V101-1] の推奨(通す)に異論は無い。
代案 A の評価(操作名の解決を救えない)も実測(p2、r2)と合う。
P[V101-2] は書く推奨に賛成するが、6 節のとおり範囲を入れ子まで広げ、値束縛の注釈を含めない書き方にする必要がある。

## 8. ほかの項目との重なり

- V135: §11.15 の `T.Perform` の同じ分岐(1914 行の近く)を触る。
- V133: §11.16 の `T.Resume` の同じ分岐(1964 行の近く)と resume の診断を触る。
- V129、V134: `perform op(...) match {}` の網羅性の警告で、perform の型付けの経路に関わる。
- V127: `_item` の文面(`ラベル _item がありません`)の言い換えを入れるなら、この項目の後は perform の引数の個数の誤り(e5)も `elab_check` の同じ経路を通るので、同じ言い換えの対象になる。
- V102: 言い換えの場所は §11.18 で、この項目が触る分岐とは別である。ただし、この項目の後は §11.12 1734 行の言い換え(「この位置の行は空…」)が perform と resume の引数の無名関数にも出る(p3、w3pubeff2)ので、言い換えの文面をそろえる作業をするなら両方を見る。
- V103: §13.2 の開かない位置の列挙(1170 行)を両方が参照する。
