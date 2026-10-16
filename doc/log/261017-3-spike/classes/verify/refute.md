# 分野 classes の節と合成差分の反証

対象は clusters/classes/section.md(1529 行)と classes.patch(1358 行)である。
作業は clusters/classes/verify/ の写しで行った(diktor 39d4091 の写しに classes.patch を -p1 で当てた。--dry-run で 15 ファイルとも当たることを確かめてから当てた)。

## 1. ビルドと runtest(実測)

- dune build: lib/parser.mly 88 行の Warning 60 だけで成功した。
- dune runtest: 差分 0(終了状態 0)。
- HEAD の lib(decls.ml、elab.ml、parser.mly)に戻し、合成後の test/ を置いた runtest で落ちるファイルは builtin_rows.t、instance_param_types.t、instance_init.t、premise.t、classes.t、dictionary_passing.t、import.t の 7 本だった。
- 落ちる段落(前書きで区切った cram の塊)を数え直した。
  builtin_rows.t 7(bshowweak、bshowann、barrlen、brefget、bopweak、bopdisp、stdinst)、instance_init.t 8、instance_param_types.t 5、dictionary_passing.t 1(mtpabsent)、import.t 2(pi1、pi2)で計 23 である。
  premise.t の pr7 と classes.t の ml9e、ml9e2 は期待値が変わる既存のブロックで、計 2 ファイル 3 ブロックである。
  never_ops.t、std_names.t、spec_gaps.t、verify_fixes.t、annot_rows.t は HEAD でも通る。
- 新しいブロックの数(準備の export の段落を除く)は builtin_rows.t 9、instance_init.t 10、instance_param_types.t 8、dictionary_passing.t 3、import.t 3、never_ops.t 3、std_names.t 1 で、計 37 である。
- 中間の状態(HEAD の lib に V110.patch だけを当て、test/ は HEAD に spec_gaps.t、verify_fixes.t、instance_init.t を足したもの)の runtest も差分 0 だった。
- 担当の分類と数はすべて実測と一致した。

## 2. 数の数え直し

- instance_init.t: プログラム 12 本、コマンド 26。節の「10 ブロック、12 プログラムと対話的な実行の入力 1 つ」と合う。
- instance_param_types.t: プログラム 8 本、コマンド 20。
- builtin_rows.t: プログラム 9 本、コマンド 19。
- dictionary_passing.t(V106): 追加 45 行、コマンド 10、プログラム 5。
- import.t(V126): コマンド 14、プログラム 7。
- never_ops.t(V132): 追加 58 行(618〜675 行)、コマンド 6。
- std_names.t(X-D7): 追加 16 行、コマンド 7、printf 4 本。
- V136.patch: lib/elab.ml の追加 41 行、削除 8 行(計 49 行)。節(と最終稿)の「48 行で、追加 40 行と削除 8 行」は 1 行ずれる。
- 合成の比較の回数の和は 110+98+112+94+80+56+38+46 = 634 で、主張と合う。

## 3. 合成の処理系で例を走らせた結果(実測)

各項目から複数の例を走らせ、最終稿と節の主張と照らした。

- V97: b1(`1`、`2`)、b3_array、b4_ref、ordervr(`15`)が受理。ordervr_id と ordervr_call は 10:11 で拒否、vrgen_id は `13`。disp2 の表示は節のとおり。
- V103: a1(`2`)、b1、d1 が受理。c1 はプリミティブなので 4:39 で拒否のまま。ov_id は拒否、ov_inc と ov_plus は `15`、ov_neg は `7`。i5、i6 は V110 の文面で拒否。i7、i8 は `init`、`n` で受理。e3 の表示は節のとおり。
- V110: u4(5:7)、u3(4:7)、q7(5:7)、q3(imp/lib.kel:5:7)は V110 の文面で拒否。e8 は `init`、`foo`、`foo`。c7 は `1`、r6 は `7`。b3、c3 の元の報告は「ラベル Console がありません(行は閉じています)」。q10 と V97 の p4_cmeth は ς1 の拒否のまま。
- V136: a.kel は --strict-exhaustive で終了状態 0。w は `20`、x は警告と `lo`。s は 2:67、t は 2:32、z は 2:91、u は 2:73。r15_gen は ς1 の文面のまま。
- V106: cls1、cls2 は 1:1 の新しい文面。alias1 は 6:33 の「(_A, _A) は Show のインスタンスではありません」。r10_bogus は「未知のクラス: Bogus」。
- V126: pub3、pub1 は 4:1 の構文エラー(終了状態 2)。withpub は従来のブロックの文面。
- V132: closed は `caught bad: 2`、`-1`、`true`。show の元の報告は「ラベル Fail がありません(行は閉じています)」。specexample の型は節のとおり。
- X-D7: show_diff、imp/main.kel は名前で拒否。

P[V110-1] の代案 A の主張も試した。
check_init_row の許す行を前提の有無によらず空にすると、instance_init.t で変わるのは instinitcon と REPL のブロックだけだった(実測)。
ただし、そのときの拒否の文面は「初期化式が起こせるのはトップレベルの評価と同じエフェクトです」のままで、代案 A の規則と合わない。
「1 行の変更で済み」は文面の変更を含まない。

## 4. 指摘

### must_fix

1. §6.4 の 654 行の置き換えの 2 文目「インスタンスのメソッドの初期化式は構文上の値でなくてもよいが、エフェクトを起こせない（§12.2）。」は、主語が初期化式の全体に読める。
   同じ節の §12.2 の V110 の段落と instance_init.t の instinitcon(1 回だけ評価する初期化式が `Console` を起こして受理、実測)と食い違う。
   担当が V132 の 3 文目で直したのと同じ型の食い違いである。
   「これらのインスタンスのメソッドの初期化式は」か「メソッドを使うたびに評価する初期化式は」のように、653 行の場合に限る形に直す。
   N.2 の (4) と N.z の該当の記述(654 行は V110 の 2 文)の両方が対象である。
2. §12.2 に足す V110 の段落の 4 文目「…停止しないことがある。評価の時点は §6.4 で定める。」は 2 文を 1 行に書いている(N.2 の (4) と N.z の全文の 2 か所)。
   LangSpec は一文ごとに改行しているので、そのまま貼ると規約に反する。2 行に分ける。
3. N.1 の (2) の `doc/log/261017-3-spike/V97/verify/ex/ordervr_call.kel` は存在しない。
   ファイルは V97 の `ex/ordervr_call.kel` にある(V97 の最終稿の誤りを引き継いだ)。
4. N.3 の (1) の規模「`lib/elab.ml` の 48 行で、追加 40 行と削除 8 行」は、V136.patch を数え直すと追加 41 行、削除 8 行(計 49 行)である。
5. N.z の「当てた順と中間の状態の記録は同じ場所の `order.txt` と `runtest.log` にある」は、統括者が写すのが `classes/classes.patch` だけなら参照先が無くなる。
   runtest.log はさらに runtest.raw.log、runtest.headwithnew.log、runtest.stage-V110.log、runtest.stage-V97.log、cmp.V97V103.txt、cmp.rest.txt を引き、order.txt は保存しない p/V97-103.sel.patch と snap/diktor を引く。
   写す対象に加えるよう統括者に伝えるか、本文の参照を外す。
6. classes.patch の各ファイルの見出し(30 行)に一時領域の絶対パスが入っている。
   doc/log/261017-3-spike/classes/ に写すと一時領域のパスがリポジトリに残る。
   見出しを a/、b/ の形に直してから写す(項目の差分も同じ形なので、統括者の側で一括して扱うのがよい)。

### 文章規範

- 一文ごとの改行が崩れている行が多い。
  D[V110-1] の行(3 文)、D[V132-1] の行(3 文)、D[X-D7-1] の行(2 文)、P[V126-1] と P[V126-2] の推奨の根拠(各 5 文)、P[V132-1] の推奨の根拠(3 文)、N.3 の (6) の箇条(2〜3 文)、N.4 の P[V106-1] の推奨案(3 文)などである。
  最終稿から写した箇条に多いが、計画の本文の規則に合わせて分ける。
- P[V110-1] の推奨の根拠は、V97 の最終稿の理由を「」で引くが、文面は V97 の最終稿(「トップレベルの行の代案は、前提つきのインスタンスで初期化式の echoln が使うたびに起きることになり、§13.2 の表に反する」)の言い換えで、引用になっていない。
  「」を外して地の文にするか、原文どおりに引く。
- 一時領域のパス、英語の文、です・ます調、禁止の語は本文に見つからなかった。
- 番号は D[...]、V[...]、P[...]、M[classes] の形と既存の番号(D190、D240、D283、D284、D300、P38 など)だけで、仮の番号の崩れは無かった。

## 5. 順序とほかの分野への要望

- 合成の順(前段の cram、V110、instance_init.t、V97/V103、V136、V106、V126、V132、X-D7)は、各項目の依存(V97 は V110 の後、V136 は V110 の後)と矛盾しない。
  V110 を V97 より前に置かないと組み込みのインスタンスの `Print` の初期化式が実行時の未処理のエフェクトになることは、V103 の i5 の実測から支持される。
- コミットの表(diktor 15、親 7)は順の記述と合う。
  実装のコミットと期待値のコミットを分けているので、V97 のコミット 5 と V136 のコミット 7 の直後は runtest が落ちる。AGENTS.md の方針どおりで、問題ではない。
- 要望(V108 は N.1 の後、V101 は M[classes] の後、V111 は V136 の method_param_tys を前提に、V109 は V106 のブロックの後、V135 は V136 の後、premise.t は M[classes] の後)は、触る関数とファイルの重なりから見て妥当である。
  V101 と V108 の順の要望は、分野 effects と分野 eval の順と衝突しうるので、統括者が全体の順で確かめる必要がある(推測)。
