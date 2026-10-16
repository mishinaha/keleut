# 分野 kinds の節と合成差分の反証

作業領域: clusters/kinds/verify(写し diktor、合成のビルド diktor-final、例の出力 r.mine.txt、i.<id>.mine.txt、ex/)。
/tmp が満杯に近く(空き 80M 前後)、写しのビルドは 1 つずつ作っては消した。

## 1. ビルドと runtest(実測)

- kinds.patch は snap/diktor に -p1 で全 hunk が当たる。dune build は成功(Warning 60 のみ)、dune runtest は出力 0 行。担当の写しとの差は .git だけ。
- HEAD の test/*.t を合成の lib で走らせると、変わる既存のブロックは kinds.t の specunit2、speclab と invariants.t の aliaspub の 3 つ。主張どおり。
- 合成の test/*.t を HEAD の lib で走らせ、コマンドごとに比べた(cmpcmd.py)。結果が違うのは effect_order.t 9、kinds.t 31(既存の 2 を除くと 29)、invariants.t 3(既存の aliaspub を除くと 2)、pub.t 3、visibility.t 8。
  - effect_order.t は担当の主張の 8 ではなく 9 である。effrepl の `diktor --repl` も違う。HEAD は 5 つ目の入力で `! <stdin>:5:65: 型エラー: 未知のエフェクト: Later2` を余分に出す。担当自身の rt.finaltests-headlib.log の 81 行目にも出ている。新しいコマンドの合計は 50 ではなく 51 になる。
  - kinds.t の 29 の内訳(V22 5、V124 6、V29 5、V27 9、V31 4)、visibility.t の内訳(V56 4、V57 4)は数え直して一致した。
- 繰り返しを外した変種(`if now > prev then ...` を `ignore (now, prev)` に替えた)で落ちるのは specfix、specalias、fwdalias3、fwdalias4 の 4 本だけである。主張どおり。
- 担当の run.sh を合成のビルドで走らせ直すと、r.final.txt(260 本)と i.<id>.final.txt(274 本 = 96 + 45 + 55 + 31 + 47)に完全に一致した。
  HEAD の受理は変わらず、3 つの試作のどれかが受理する例はすべて受理する。HEAD が拒否して合成が受理する例は 115 本、V22+V124 との違いは 28 本。主張どおり。
  274 本で試作と違うのは V27/ex/c3.kel と V56 の b1(ex/b1.kel と verify/ex/b1.kel の 2 本)である。
- 差分の行数: lib/elab.ml は +451 / -179(git apply --numstat)で、節の +452 / -180 と 1 行ずれる。ほかのファイルは一致する。

## 2. 項目ごとの写し(final.md と節の比較)

9 項目すべてで final.md と節の小節を diff で比べた。
判定、判断事項、差分の内容、cram の数、LangSpec の行は、合成で変えた点(V30 の specres の扱い、V22 の is_effect_name と P[V22-1]〜P[V22-3] の消去、V27 の D[V27-4]、V31 の P[V31-4] と D[V31-2]、V57 の改名)を除いて写されている。
変えた点は (8) と N.z に理由が書いてある。
ほかの不一致:

- N.3 の「`ex2/` の 2 本」: V22/ex2 には effop.kel と effop2.kel を含めて 4 本ある(final.md から引き継いだ誤り)。(7) の「変わる側」には gap2 も入る(HEAD は 3:41 で拒否、合成は受理)。
- runtest.log の [D] のディレクトリごとの本数が実際の .kel の数と合わない(V124/verify/ex は 47 ではなく 45、V22/verify/ex は 17 ではなく 14、V29/verify/ex は 41 ではなく 39、V30/ex は 27 ではなく 26、V30/verify/ex は 34 ではなく 30)。書いてある本数の和は 272 で、260 にならない。
- N.2 の (6) の「合成の runtest で既存の期待値が変わるのは V30 による 2 ブロックだけだった」と P[V124-1] の根拠の同じ文: 分野の合成全体では V117 の aliaspub も変わる。4 項目の範囲に限ると書く必要がある。

## 3. V31 の D[V31-2] が字面で判定する件(実測で退行を確かめた)

N.7 の (6) は「推測。測っていない」として残していた。合成のビルドで測った(verify/ex/id2.kel)。

    newtype Callback[E] = Callback(() => Unit @ E)
    type Id[A] = A
    type Id2[A] = Id[A]
    newtype N[X] = MkN(Callback[Id2[X]])
    let f(n: N[{Print}]): Int32 = 1

HEAD は `f : (N[{Print}]) => Int32` で受理する。合成は `! id2.kel:3:15: 型エラー: Type エイリアス Id2 の本体のカインドが Type ではありません: ς1 :: Row` で拒否する。
`Id` を直接使う形(id1.kel)は両方とも受理する。
したがって次の記述が実測と食い違う。

- P[V31-4] の推奨の根拠「HEAD の受理を変えず」。
- kinds.t の hktrowok の前書き(合成の 1089〜1090 行)「エイリアスの展開結果が型パラメータそのものになる形は、行変数のカインドの推論として Row に決まって通る」。Id2[X] の展開結果は X だが落ちる。
- LangSpec §3.3 の 3 文目の案は「右辺が型パラメータそのもの」に限るので Id2 は対象外で、仕様の上では拒否が正しくなる。だが V27 の前書きとは食い違う。

## 4. 文章の規範

- 一時領域のパス、英語の文、仮の番号の形の崩れ、である調の崩れ、禁止の語は節の本文に見つからなかった。
- 材料として写す kinds.patch の見出しに、一時領域の絶対パスが 14 か所残っている(`diff -ruN -x _build -x .git /tmp/claude-1000/.../snap/diktor/lib/decls.ml diktor/lib/decls.ml` など)。doc/log に写す前に a/ と b/ の見出しで作り直す必要がある。
- 節が引く材料のパスは、写し先の items/ または clusters/ にすべて実在する。

## 5. 順序とほかの分野への要望

- 当てる順(V30、V124+V22、V29 の cram、V56、V57、V27、V31、V117)は各項目の依存と矛盾しない。コミットの順で、V124 のコミット 2(fwdalias、fwdalias2、fwdalias5 を含む)は繰り返しより前に入る。繰り返しを外した変種でこの 3 本は通るので成り立つ。D[V124-1] だけの段は測っていないと節に書いてある。
- region(X-regionshow)への要望の対象の期待値(hktrow、hktrow3、hktrow5〜7、aliasrow3、aliasrowlib)には ς が出ており、要望は妥当である。spec(V28)の visrow27 からの番号、diagtext(V24)、classes(V126)への要望も妥当である。

## 6. 例(合成のビルド)

V30 の fwd.kel(42)、effpos(v、3、w)、P[V30-1] の dd.kel / pp.kel。
V124 の a1(受理)、d5、r16。V29 の a1(受理)、d1(2:20)。V22 の ex/ と ex2/。
V27 の c3(1:1 の D[V27-4] の文面)。V31 の a1(3:10)。V56 の b1。V57 の b7、b8。V117 の a2、a6、a9、a10(2)。
どれも final.md と節の主張に一致した。
