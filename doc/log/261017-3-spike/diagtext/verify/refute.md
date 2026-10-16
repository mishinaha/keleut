# 分野 diagtext の節と合成差分の反証

作業領域: clusters/diagtext/verify(diktor = 合成差分を当てた写し、np = promote 前の合成差分を当てた写し)。

## 1. 合成差分の適用、build、runtest(実測)

- diagtext.patch を snap/diktor の写しに当てると 14 ファイルとも当たる。dune build は通り、警告は lib/parser.mly:88 の 1 件だけ。dune runtest の差分は 0。
- diagtext.nopromote.patch を当てた写しの dune runtest は、7 ファイル(pub.t、premise.t、classes.t、dictionary_passing.t、typecheck_m7.t、verify_fixes.t、annot_rows.t)で 14 コマンドの期待値が変わる(np.raw.log)。項目への割り当て(V24 6 コマンド、X-premise 6、V102 1、V111 1)と変化の中身(位置、末尾の 1 文、言い換えの消滅、事実の追加)は担当の表と一致する。新しい 4 ファイルと classes.t、premise.t への追加は落ちない。
- ブロックの数は割り当てが誤っている(must_fix 2)。premise.t の pr17 と pr18 は空行を挟まない 1 つのブロックで、前書きも共有する(HEAD の premise.t 293〜312 行)。verify_fixes.t の opres と opres2 は 647 行の空行で分かれた 2 つのブロックである。cram のブロックで数えると V24 は 6 ブロック、X-premise は 5 ブロックで、合計 13 になる。前書きで数えると V24 5、X-premise 5 で合計 12 になる。節の「V24 5 ブロック、X-premise 6 ブロック、合計 13」は、2 つの数え方が混ざっている。

## 2. 最終稿との突き合わせ(全 6 項目、diff で照合)

各小節を items/<id>/final.md と diff で照合した。違いは、見出しの形、パスを doc/log/261017-3-spike/<id>/ の形にした書き換え、合成の結果の追記、下書きの番号を言葉に置き換えた箇所だけである。判定、判断事項、差分の内容、cram の数、LangSpec の行は最終稿のとおりに写されている。
数え直した値:
- ctor_field_row.t: 130 行、diktor のコマンド 11 個、プログラム 11 本、前書きの段 5 つ(export のブロックを除く)。
- rec_tparam_note.t: 91 行、プログラム 7 本、export を含めて 8 ブロック。
- rigid_empty_row.t: 86 行、$ の行 15 個、プログラム 7 本、export を含めて 5 ブロック。
- decl_pos.t: 110 行、$ の行 19 個、プログラム 9 本、前書きで数えて export を含めて 8 ブロック(cram のブロックでは effpos2 と effpos3 が空行で分かれるので 9)。
- X-premise の premise.t への追加: 前書き 5 つ、プログラム 7 本、$ の行 14 個。
- diktor のコミット 15、親のコミット 6: 各小節のコミット一覧と一致する。
- HEAD の行番号: elab.ml 276〜282、2056、2111〜2118、335〜337、4551、4558、4559、4562〜4564、unify.ml 576〜577、pub.t:133、verify_fixes.t:645/650/691、typecheck_m7.t:181、dictionary_passing.t:292、annot_rows.t:956/1051、LangSpec 634〜635、1158〜1159、1169〜1171、1782 はどれも主張どおりである。
- V24 の hunk のずれ 117 行は、合成後の elab.ml で 3745 行が 3862 行に移ったことで確かめた。
- V34 の記事の 4 文は、合成後の unify.ml 622〜626 行で 4 文である(order.txt の「5 文」は誤りだが、節は 4 文と書いている)。

## 3. ほかの分野についての記述(実測)

- X-D7: X-D7.patch が触るのは test/std_names.t だけで、X-D7 の最終稿の (5) も「test/std_names.t の末尾に 1 ブロック」と書いている。節の 522 行(N.3 の (6))、861 行(N.5 の (6))、1130 行(N.z)にある「X-D7 は test/classes.t を触る」は誤りである(must_fix 1)。
- V122: V122.patch の unify.ml の hunk は 544 行(§8.7 の match)、601 行(§8.8 の記事)、618 行(§8.8 の unify_row)にあり、§8.7 の記事(481〜536 行)には無い。N.z の「§8.7 の記事にも触れる」と、N.4 の (6) の「X-D74 と V122 も unify.ml §8.7 の記事に触れる」の V122 の部分は誤りである(must_fix 3)。X-D74 の 527 行の hunk は §8.7 の記事にあるので、X-D74 の記述は正しい。
- 合成の上で --dry-run: V122、X-regionshow、V58、V110、V136、X-D74、X-D7 は当たる。V127 と X-perform は elab.ml の hunk 1 が fuzz 2 で当たる。V127 は unify.ml の hunk 2 も fuzz 2、hunk 4 も fuzz 1 で当たる。N.z の V127 の要望は unify.ml の fuzz に触れていない(order.txt には書いてある)。
- V127 を合成の上に当てて build と runtest を走らせると、test/ctor_field_row.t の ctorarity が「引数の個数が一致しません: () => Int32 と (Int32) => Int32 @ {}(引数は 0 個と 1 個です)」に変わる。V102 の (6) と N.z の予測は正しい。
- V58 が触るのは verify_fixes.t の 432 行と 445 行(retguard、cancelguard)で、V24 のブロックとは重ならない。

## 4. 例の実行(実測)

6 項目の ex/、ex2/、verify/ex/ にあるすべての .kel を、項目単独の試作と合成後の処理系で走らせた。モードは実行、--type-check、--no-prelude --type-check の 3 つで、合わせて 690 回である。違ったのは V33 の h2、h2b、h2c、h2z(実行と --type-check)だけで、元の報告の文言が V34 の文言に変わる。名指し、言い分け、位置、終了状態は同じである。担当の cmp.txt と一致する。
最終稿の出力と見比べた例: V102 の nnlist(言い換えなし)と rowarg(言い換えあり)、V111 の gen と genbad、V34 の b1、X-premise の p1、V24 の nt1、ef1、r10_classdup、V33 の v33a。どれも最終稿の主張と一致した。
item の写しの lib と test は、各 <id>.patch を snap に当てたものと一致する。

## 5. litwrap と litcheck(実測)

合成後の elab.ml、unify.ml、show.ml に litwrap を走らせても変化は無い。警告は既存の表の行だけである。snap を基点にした git の写しで litcheck を走らせると、全ファイルで marker は 0 件だった。code の不一致は elab.ml、unify.ml、show.ml だけである。

## 6. 文章の規範

- 一時領域のパス: 無い。
- 英語の文: 無い。
- 番号の形: D[...]、P[...]、V[...]、M[diagtext] と既存の番号(D241、D120、P35、M60、D8、D7 など)だけである。D7 と D74 は X-D7 と X-D74 という項目名の一部である。
- である調: 崩れは無い(「です」「ます」は診断の文面の引用の中だけにある)。
- 765 行の「第11章」は半角スペースが抜けている(「第 11 章」とする)。
- 短い相対パスが残っている。V34 の小節の `ex/a1.kel`、`ex/c9.kel`、`verify/ex/q1.kel`。V34 の (6) の `ex2/h2z.kel` は V33 の例で、V34 のディレクトリには無い。X-premise の小節の `results.extra.txt`、`results.repl.txt`、`final-work/p.after.txt`、`p.after.txt`。V24 の小節の `ex/ef6.kel`、`ex/pl1.kel` など。
- ブロックの数え方が小節ごとに違う。ctor_field_row.t の「ブロック 5 つ」は export のブロックを含めず、rec_tparam_note.t の「8 ブロック」、rigid_empty_row.t の「5 ブロック」、decl_pos.t の「8 ブロック」は含める。
