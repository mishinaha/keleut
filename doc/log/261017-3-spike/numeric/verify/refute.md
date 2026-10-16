# 分野 numeric の節と合成差分の反証

対象: clusters/numeric/section.md、clusters/numeric/numeric.patch。
作業: verify/diktor(HEAD の写しに numeric.patch を -p1 で当てた)、verify/headtests(合成のコードに HEAD の cram)、verify/p_V91・p_V114・p_V8(各項目の差分を HEAD の写しに単独で当てた試作)、verify/xp(合成に X-premise を当てた写し)。

## 1. ビルドと cram(実測)

- numeric.patch は -p1 で 14 ファイルにそのまま当たる。dune build は成功し、警告は parser.mly の Warning 60 だけ。
- 昇格後の dune runtest は差分なし(runtest.after.log は空)。
- 合成のコードに HEAD の cram 6 ファイルを戻して走らせると、落ちるのは 10 ブロックだった(rt_headtests.log): smoke の u8(未実装は exit 4)、neg_rem の remf、dictionary_passing の negremf、typecheck の numbad2 と mix、numeric の u8、w9999、lead0/lead0f(1 ブロック)、mask、verify_fixes の hugesuf。担当の内訳(V91 2、V114 7、V8 1)と一致。
- 昇格前の再現(neg_rem.t と dictionary_passing.t だけ HEAD に戻す)では、落ちるのは remf 1:1→1:13 と negremf 2:1→2:13 だけ(rt_prepromote.log)。
- 新しいブロックの数: literal_default.t はブロック 10、コマンド 21、cat のプログラム 9、130 行。V114 の numeric.t の 3 ブロックはコマンド 13、cat 2(patsfx、int8user)、printf の .kel 4。V8 の 3 ブロックはコマンド 15、cat 4、printf 3。どれも節の主張どおり。
- 各項目の差分の追加行はすべて合成の木に含まれる(手直しで内容が変わっていない)。
- litcheck --old: aux、driver、exhaust、lexer、syntax が code 一致、elab と unify が不一致、7 ファイルとも marker 0 件。主張どおり。

## 2. 例の突き合わせ(実測)

- 各項目の差分から自分で作った単独の試作と合成の処理系で、ex/、verify/ex/、ex2/、final/ の全 .kel を実行と --type-check で、repl の入力を --repl で、計 275 回走らせ、差は 0 件(cmp_mine.txt は空)。
- V91 の (2) の表の位置(rem 2:9 … pat 2:58、exprstmt 2:1、repl 2:9 と 3:17)、w1_weak 4:11、o1/o3/o4 の 2:11、2:14、2:15、g1/g2 の 2:5 は合成でも同じ。
- V114 の sfx_i8 など(1:9、終了 1)、ty_int8 の案内、ty_int8_user と r01_alias の受理、mod_int8 の修飾の案内、e01/e03 の案内なし、m3 の未実装は主張どおり。
- V8 の dead、hex、i64、nested、dflt、handler、dup は型エラー、expr は実行時エラー、local は 1。主張どおり。
- 交差の例 x1〜x5 と premlit は節の N.z の記述どおり。V127 の s2.kel は HEAD で 2:20 の未実装 4、合成で 2:20 の型エラー 1。
- X-premise を合成に当てると reset のハンクだけ当たらず、premlit は 5:9 に補足なし。節の提案どおり default_to で via を引くと補足が付き、runtest の差は classes.t の pshowop、premise.t の 5 か所、literal_default.t の premlit と weaklit だけ。主張どおり。
- V121 は合成に対して driver.ml の第 3 ハンクだけ当たらない。主張どおり。

## 3. 最終稿との突き合わせ

3 項目とも最終稿との差分を全行で取った。判定、判断事項、裁定、差分の内容、cram の数、LangSpec の行は写されている。変更は、参照パスの書き換え、合成の情報の追記(D[V114-4] など)、一文ごとの改行、V91 の (6) から V108 の記述の削除。V108 の差分は dictionary_passing.t に触れない(driver、interp、value、repl.t)ので、この削除は正しい。

## 4. 指摘

must_fix:

1. N.z の lexer.ml の行番号が小節と食い違う。N.z は「V8 は 74 行、V114 は 91 行と 129 行」と書くが、これはハンクの見出しの開始行である。変わる行は V8 が 76 行の後、V114 が 94 行と 132 行(V114 の (5) は「94 行と 131〜132 行」、V8 の (5) は「76 行」)。どちらかにそろえる。
2. V109 との関係の記述(V91 の (6) と N.z の要望)は「文脈が合わない」と書くが、合成の木に V109.patch を -p1 --dry-run で当てると、dictionary_passing.t の第 1 ハンクは 419 行で fuzz 2 で当たる(失敗はしない)。「fuzz 2 で当たるが、文脈の 1 行を 2:13 に直しておく」に改める。
3. 統括者が doc/log/261017-3-spike/ に写す差分の見出しに一時領域の絶対パスが残る。numeric.patch に 28 行、V91.patch に 8 行、V8.patch に 8 行。写す前に見出しを a/ b/ などに直す(本文の規則の趣旨に照らして)。

軽微(直すのが望ましい):

- V91 の (2) の generalize の行は「675〜681 行」だが、let rec generalize は 674 行から(最終稿は 676)。
- V8 の (4) の「§11.1 の表の直後(873 行の後の空行の次)」の 873 行は空行そのもので、本文は 874 行から。「873 行の空行の後」が正確(最終稿のまま)。
- V91 の (6) の注釈の例で、文の順(let b、let f)と括弧の順(g1_annot.kel、g2_annot.kel)が逆に対応する(g1 が let f、g2 が let b)。
- N.z の「V91 の差分(289 行、668〜747 行、954 行、1062 行)」は unify.ml の 31 行のハンクを抜かしている(V127 と重ならない結論は変わらない)。

## 5. 文章規範

一時領域のパス、英語の文、仮の番号(D/V/P/M の括弧の形以外)、禁止語は本文に見つからなかった。である調も保たれている。
