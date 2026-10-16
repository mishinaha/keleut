# 分野 extra の節と合成差分の反証

作業領域: clusters/extra/verify(diktor = 合成の木 all/diktor の写しに extra.patch を当てたもの、seq = 同じ写しに 6 項目の差分を V47→V50→V44→V49→V53→V48 の順に当てたもの、basenew = 合成の木のコードに新しい cram だけを置いたもの)。

## 1. 合成差分の適用、build、runtest(実測)

- extra.patch は all/diktor の写しに patch -p1 で失敗 0 で当たる。dune build は parser.mly:88 の Warning 60 だけ。dune runtest は差分 0。
- 6 項目の差分を順に当てた seq では、offset は担当の主張どおり(V50 は elab.ml +6/+25、visibility.t +40、V44 は elab.ml #4〜#8 +26、V49 と V53 は elab.ml +68、ほかは 0)。失敗 0、fuzz 0。
- seq の runtest の差分は test/typecheck.t の ml5c の 1 行だけ。seq と extra.patch の木の違いはこの 1 行だけ(diff -ruN)。
- extra.patch の行数: elab.ml +148/-73、exhaust.ml +109/-19、unify.ml +23/-2、12 ファイル。主張どおり。
- 既存の期待値の変化: visibility.t visrow26、eval.t 3 ブロック 4 コマンド(solelit と soleopt、solevar2、opcover)、verify_fixes.t guardonly、typecheck.t ml5c、typecheck_m7.t v10h、instance_param_types.t inswild。6 ファイル 8 ブロックで主張どおり。
- 新しい cram を合成の木のコードで走らせると(basenew)、V47 は 6 コマンド、V50 は 4 コマンド、V44 は 9 コマンド、V49 は 4 コマンド(escrun は通る)、V53 は 2 コマンド、V48 は params、single、elim、valued、deep の 5 ブロックが落ちる。各小節の主張と一致。
- litcheck --old(3 ファイル): marker 0 件。litwrap: 3 ファイルとも変更なし。

## 2. 新しいブロックの数(誤り)

- pub_effect_note.t は export PATH のブロックを含めて 8 ブロック(プログラムのブロックは 7)。
- uninhabited_pattern.t も export PATH のブロックを含めて 8 ブロック(プログラムのブロックは 7)。
- V44 の小節は export を含めて 8 と数え、V48 の小節は export を除いて 7 と数える(V48 の「コマンド 15」は export を含む)。
- N.z の「新しいブロックは 6 ファイルに 25 ブロック」は 2 つの数え方を混ぜている。export を含めれば 26、除けば 24。
- ファイルは visibility.t、pub_effect_note.t、scope_escape_tparam.t、eval.t、uninhabited_pattern.t の 5 本で、「6 ファイル」も誤り(visibility.t を 2 行に分けて数えている)。

## 3. 最終稿との突き合わせ(全 6 項目)

最終稿と節の差を全行で比べた。差は、材料の参照の形の置き換え、M[extra] の順と合成の実測の追記、範囲に加えた経緯の 1 文、担当が挙げた 3 件の訂正(V53 の出典、P[V49-3] の代案 2、V50 の scope)、「必ず」を「例外なく」に、「常に偽」を「どの形にも偽」に変えた語の差し替えだけである。判定、判断事項、推奨案、差分の内容は変わっていない。
数え直し: V47 +85/-19、cram 2 ブロック 16 コマンド 8 プログラム。V50 +54/-29(elab +25/-25、visibility +29/-4)、3 ブロック 10 コマンド 5 プログラム。V44 284 行、pub_effect_note.t 138 行、27 コマンド、cat 12 本と sed 1 本。V49 unify.ml コード 12 行と記事 8 行、elab.ml 1 行、5 ブロック 10 コマンド 5 プログラム。V53 elab.ml +7/-3、114 行、既存 4 ブロック 5 コマンド、新しい 1 ブロック 6 コマンド 3 プログラム。V48 379 行と 352 行、ty_key コード 22 行とコメント 3 行、inhabited コード 22 行とコメント 8 行、コマンド 15。どれも一致。
LangSpec の行(913、916、986、1106〜1116、1121、1147〜1150、1211、1229、1237、1239、1243、1447、1479、1628〜1637、1782〜1783)、sample.kel の 420、469、470、515、diktor HEAD の行(elab.ml 276、604、941、1165、1180、1188、1239、1729、2558、2584、2611、2615、2794、2805、3109、3127、3173、3400、unify.ml 165、173、syntax.ml 283、exhaust.ml 735、888、912)を写しで確かめ、合っていた。
HEAD への dry-run: V47 は 1 hunk、V50 は 3 hunk、V44 は 4 hunk、V48 は exhaust.ml の 2 hunk が失敗し、V53 は -385 と -66 の offset で当たる。主張どおり。

## 4. 文章の規範

- 一時領域のパスは無い。ただし N.z の「基点と当て方」に `base/diktor` という作業領域の名前が残る(差分の見出しの形の説明)。保存しない領域を指すので、「項目ごとに見出しの形が違う」とだけ書くのがよい。
- 英語の文、です・ます調、禁止語は見つからなかった。
- 番号は D[...]、V[...]、P[...]、M[...] の形と既存の番号(M36、M38、P47、V40、V41)だけ。V[A]、V[Int32] はコードの型。
- V47 の (1) の「見出しを除く。見出しの行を除いて」は重複している。

## 5. 順序と要望

- 順序は deps と矛盾しない(V114、V57、V111、V117、V127、V136、V129 はどれも先に入る分野にある。各分野の section.md の小節で確かめた)。
- 要望は妥当。補足: M[kinds] の V57 の小節(clusters/kinds/section.md の 759 行)は `doc/sample.kel:467` を略記の規則として引いているが、HEAD では 469 行である(V50 の (8) が同じ誤りを直している)。M[kinds] への要望に足すとよい。

## 6. 例の実行(合成後のビルド)

各項目の ex、exb、ex2、verify/ex の .kel 327 本を、合成の木、各項目の試作、合成後の 3 つで --type-check にかけた。試作と合成後の差は V44/ex/a4.kel の 1 本だけ(V49 の注記)。base と試作の差の件数は V47 12+2、V50 10、V44 8、V49 6、V53 7 で主張どおり。
個別: V47 a1 は「未知のエフェクト: W(M.W と修飾してください)」、a11 は案内なし。V50 a03 は f の行と 3:29 の行変数の診断。V44 a9 は言い換えなしの脱出の文面、q7 と q7e は元の報告だけ。V49 v49_top は注記つき、v49_run は注記なし。V53 zero は case ask()、two は case pair(_, _)、two_fixed と qual_fixed は受理。V48 params は型の行 10 行、警告 0(合成の木では 4)、run.kel は --strict-exhaustive で 42 を出し終了状態 0、deep と blow_*_200 は 5 ms 以下。
