# 分野 region の節と合成差分の反証

対象: clusters/region/section.md、clusters/region/region.patch。
作業: verify/diktor(snap/diktor に region.patch を -p1 で当てた写し)、verify/headtests(同じ処理系に HEAD の test/*.t を戻した写し)、verify/stage(V122 → X-D74-adjusted → X-regionshow を段ごとに当てた写し)、verify/rerun(各項目の ex/ の再実行)。

## 1. 合成の build と runtest(実測)

- region.patch は 13 ファイルとも --dry-run で当たり、dune build が通る(parser.mly の Warning 60 は HEAD からあるもの)。dune runtest の差分は 0 行。
- verify/diktor の lib と test は担当の clusters/region/diktor と一致した(diff -rq で差分なし)。
- 段ごとの実測: V122 だけで 0 行、X-D74-adjusted を足して 0 行、X-regionshow を足すと region.t の 4 コマンド 4 行(regesc、regheapesc、regoccurs、regescref の型の表示行で A が h になるだけ)が落ちる。kinds.t の hunk は fuzz 1(842 行)。担当の主張どおり。
- HEAD の test/*.t を合成の処理系で走らせた差分: 10 ファイル 19 ブロック。内訳は V122 が kinds.t の refkind3、refkind5、X-D74 が region.t の bump、pardet、pubpure、trailing_block.t の run2、typecheck_m6.t の stbad3、verify_fixes.t の nestrun、X-regionshow が 8 ファイル 11 ブロック 12 コマンド 14 行(visrow10 と visrow10b を 1 ブロックと数える)。担当の分類と一致した。新しく落ちるブロックは無い。
- 期待値の差分のほかに前書きの変更が 3 箇所ある(kinds.t 848〜849、region.t 164、nonfeatures.t 84〜87)。
- 新しいブロックは 9(kinds.t に refkind6〜9、region.t に regswap、regannot、regesc+regheapesc、regoccurs+regescref、regionshow)。X-D74 の分は前書き 4、プログラム 6、コマンド 12、regionshow はプログラム 2、コマンド 4。数え直して一致した。
- X-regionshow の合計 8 ファイル 13 ブロック 16 コマンド 18 行も一致した。
- litcheck(show.ml、syntax.ml、unify.ml を HEAD のファイルと比べる)は code の不一致だけで marker 0 件。litwrap はファイルを変えなかった。

## 2. final.md の写し

3 項目とも、判定、判断事項(P[V122-1]、P[X-D74-1〜3]、P[X-regionshow-1〜3])と推奨案、裁定(D[V122-1〜2]、D[X-D74-1]、D[X-regionshow-1〜2])、差分の内容、cram の数、LangSpec の行(§14.1 の 1353、1359〜1361、1389〜1391、:47-49、:126、:786-789、:1120、:1405-1407、:1423-1427、:1782)を正しく写している。LangSpec とソースの行番号は HEAD の写しで確かめた。
合成で変わった点(定数の共有、X-D74 の記事の hunk、promote の範囲、親のコミットの統合)は節の中で書き足されていて、final.md との違いはそれで説明できる。

食い違い:

- N.1 (8) の「記事の「ここに来る型変数は剛定数で」: … (6) にリスクとして書いた」の指す先が節に無い。final.md の (6) にあった「どちらも型変数の枝でも、片方が未束縛なら bind の脱出検査で失敗しうる(実例は反証者も作れなかった)」を、節は f7、f10 の箇条に差し替えた。差し替えは X-D74 の反証の結果に合っているが、(8) の記述が古いままである。
- nonfeatures.t の前書きの行(N.3 (5) と N.z の「nonfeatures.t:82-84」)。該当の文は 83〜84 行にあり、差分で書き換わるのは 84〜87 行(折り返しの組み直し)である。final.md の写しなので、節が正すか「83〜84 行の文。差分は 84〜87 行」と書く。

## 3. 文章規範

- 本文に一時領域のパス、英語の文、仮の番号は無い。番号は D[...]、V[...]、P[...]、M[region] の形だけである。である調も崩れていない。
- 見出しの形がそろっていない。N.3 は「#### (2) 現状(根拠つき)」、N.1 と N.2 は「#### (2) 現状」。指定は「(2) 現状(根拠つき)」。
- 材料の差分の見出しに一時領域のパスが残っている(region.patch に 26 箇所、V122.patch に 6 箇所、X-regionshow.patch に 18 箇所。diff -ruN の行と --- の行)。doc/log/261017-3-spike/ に写すとこのパスがリポジトリに入る。写す前に見出しを a/ b/ の形へ置き換えるよう統括者に伝える(どの分野にも当てはまる)。

## 4. 順序と要望

- V122 → X-D74 → X-regionshow は deps(X-D74 は V122、X-regionshow は合成で V122 の定数を使う)と矛盾しない。コミット 7 は V122 の ty_ref を使うので、コミット 1 より後でなければならない。節の順で満たされる。
- V111 を本分野の後に入れる要望は妥当。V111.patch は合成の写しに offset 6 で当たり(実測)、promote し直すのは rec_tparam_note.t の region.kel の 1 行と annot_rows.t の recrejtpflow(後者は V111 自身の変化)。
- X-effsuffix、X-premise、X-perform を後に入れる要望も妥当。X-perform の二重に包む件は elab.ml:1913 の Unify.unify の呼び出しで、読みとして正しい。
- 実施のコミットを分けるとき、コミット 4〜6(X-D74)は合成の最終の状態ではなく、X-D74 の記事の hunk を直した中間の差分が要る。担当の X-D74-adjusted.patch を `doc/log/261017-3-spike/region/` に写して参照するとよい。今は節の文で手直しを説明しているだけである。

## 5. 例の再実行(合成のビルド、--type-check)

- V122: a1〜a8、b1〜b3、c1〜c3、d1〜d4 は results.new.txt と比べて、a2、c1 の文言(X-D74)と a3、a4、a7 の型の表示名(X-regionshow)だけが違う。d2 の実行モードは終了状態 1。
- X-D74: e1〜e10、u1、u5〜u9 は results.new.txt と比べて、e6、u6、u7、u9 の型の表示名だけが違う(u8 と r1 は results.new.txt に無い。u8 は脱出の文言のまま、r1 は実行モードで包んだ文言と終了状態 1)。反証者の f1、f2、f3 は包んだ文言、f1r、f2r は通り、f7、f10 は HEAD と同じ文言。
- X-regionshow: r1〜r9 は results.new.txt と、r10〜r13 は results.extra.txt の試作側と一致した。verify/ex の q5、p6、q1 も節の主張どおり。
