## V58 の反証

### 裏取り

行番号は HEAD の写しで確かめた。
`at_node` は `lib/elab.ml:163`、`elab_exp` の包みは 1539 行、`elab_handle` は 2406 行、§11.22 の記事は 2370 行から、§11.23 は 2439 行、§11.24 は 2632 行である。
誤りの箇所は 2429-2430(節の頭)、2436-2437(2 つ目の return / cancel)、2438(操作節が 0 個)、2566(所属)、2586-2588(引数の個数とラベル)、2702 と 2713(ガード)で、下書きの表と合う。
`Decls.resolve_unit` は `lib/decls.ml:390-392` の `Hashtbl.find_opt` だけで、例外を投げない。
`T.Handle` を作るのは `lib/parser.mly:897` だけで、`elab_handle` を呼ぶのは `lib/elab.ml:1955` だけである(grep)。
したがって、包みの内側の `Type_error` は、HEAD でも 1539 行の `at_node` を出るときに位置つきになっていた。
`Type_error` だけを捕まえる節(1724、1916、2115、2189、3128、3401、4560 行)は、どれも `elab_exp` か `elab_check` の外側にあり、`elab_handle` との間に挟まらない。
このため、試作が変えるのは位置だけで、受理と拒否、文面、終了コードは変わらないと読める(ソースから導いた性質)。

出典の行(diktor の `doc/log/260926-2-m34.md:345`、`:219-221`、`:364`、`260926-1-plan.md:898`、`260926-1-spike/cases-V39.txt:93-100`)は、下書きの引用どおりである。
既存の cram で、対象の誤りを期待値に持つのは bh、qual4、qualret、qualcancel、retguard、cancelguard の 6 ブロックだけで、引数の個数とラベルのブロックは無い(grep)。
LangSpec の §15.5(1603 行)は終了状態と出力先だけを定め、診断の位置には触れない。
§13.4 の見出しは 1211 行で、下書きの「1228-1249 付近」は節の規則の段落を指している。

### 実測の再現

担当の `ex/` の 17 本を HEAD と試作で走らせ直し、`results.head.txt` と `results.new.txt` と同じ出力を得た。
17 本のうち `label1.kel` はパースエラーなので、型検査まで進むのは 16 本である。
下書きの (3) の「16 本」と (7) の「17 本」は、この違いである。

試作の差分は写しに `patch -p1` で当たり、担当の写しと `lib/` と `test/` が一致した。
`dune build` は通り、差分を当てた `dune runtest` は差分なしで終わった。
`lib/elab.ml` だけを差し替えて HEAD の期待値で `dune runtest` を走らせると、落ちるのは 3 ファイルの 6 ブロック(12 行の差分)で、位置だけが変わる。
担当の分類は正しい。
`git init` した写しで `tools/litwrap.py lib/elab.ml` は何も変えず、`tools/litcheck.py` の marker は 0 件だった。
`elab.ml` の code の不一致はコードを変えたためで、想定どおりである。
`lib/elab.ml` の差分は +28/-12 行で、新しい cram のコマンドは `export` を除いて 26(diktor の実行 13)である。

### 退行の探索

`verify/ex/` に次の形を書き、HEAD と試作で走らせた。
どれも出力と終了コードが同じだった。

- `r1_guard.kel`: 操作節のガードと return 節(51、終了 0)
- `r2_nested.kel`: 修飾した入れ子のハンドラと cancel 節(7、103、終了 0)
- `r3_module.kel`: module の中の pub な関数の handle、修飾した節、ガード(6、10、終了 0。`--type-check` の型の行も同じ)
- `r4_tparam.kel`: 型パラメータを持つ関数の中の handle(42、2、終了 0)
- `r5_import.kel`: import した関数の中の handle(9、終了 0)
- `r6_noprelude.kel`: `--no-prelude` で Console を自分で宣言してハンドルする形(出力なし、終了 0)
- `r8_override.kel`: プレリュードの Print の再宣言(両方とも 1:1 の同じ拒否)

退行は見つからなかった。

誤りの位置の確認として、次も走らせた。

- `e1_import.kel`: import 先の誤りは `lib/bad.kel:3:3` から `lib/bad.kel:4:10` に移る。ファイル名は import 先のままである。
- `e2_multi.kel`: 引数の個数、所属、ガードの 3 つの誤りを持つ入力で、最初に出る誤り(所属)は HEAD と同じで、位置だけが 4:3 から 6:10 に移る。
- `e3_threeret.kel`: return 節が 3 つあるとき、2 つ目(6:10)を指す。
- `e4_pubval.kel`: pub な値束縛の本体の handle でも、文面は変わらず位置だけが移る。
- `e5_twocancel.kel`: 2 つ目の cancel 節は 6:10 を指す。
- `e6_noop.kel`: 操作節が 1 つも無い誤りは 2:3(handle 式)のままである。
- `e7_qualcancel.kel`: 修飾した cancel 節は 5:10 を指す。
- `r7_repl.txt`(`--repl`): 引数の個数の誤りは `<stdin>:3:1` から `<stdin>:3:30` に移り、後続の入力の処理は同じである。

### ほかの項目との重なり

V25 は、同じ `elab_handle` の所属の検査(HEAD の 2556-2567 行)を網羅の検査の前へ移し、`test/typecheck_m6.t` の qual5 の期待値を「網羅漏れ」から「flush の所属違反」(5:19)に変える。
下書きは「依存は無い」と書くが、これは誤りである。
V25 の差分を当ててから V58 の差分を当てると、hunk 3 は fuzz 2 で当たり、結果は正しく合成された。
そのうえで `dune runtest` を走らせると、qual5 だけが `qual5.kel:5:19` から `qual5.kel:7:8` に変わって落ちる(`verify/runtest.comb.log`)。
どちらを先に入れても、後に入れる側が qual5 の期待値と、V25 の新しいブロック qual5w の扱いを確かめ直す必要がある。
qual5w は網羅漏れなので、合成でも 5:19 のままで、落ちなかった(実測)。

V24 の差分は 3745 行以降で、`elab_handle` に重ならない。
X-perform の差分は 1583-2184 行と 2843 行で、`elab_handle` の範囲(2406-2730 行)に重ならず、行のずれだけが生じる。
X-D74 と X-regionshow は `typecheck_m6.t` と `verify_fixes.t` に触れるが、別のブロック(typecheck_m6.t の 325 行と 345 行、verify_fixes.t の 389 行)である。

### 下書きの直すべき点

1. (1) の「依存は無い」を、V25 との重なりに直す。
   根拠は上の合成の実測である。
2. 試作の §11.22 の記事の末尾「`test/handle_clause_loc.t` が、それぞれの位置を確かめる」は、言い過ぎである。
   新しい cram に、2 つ目の cancel 節、cancel 節のガード、修飾した return / cancel 節、操作節が 1 つも無い誤りのブロックは無い。
   cancel 節のガードと修飾した return / cancel は既存の `verify_fixes.t` と `typecheck_m6.t` が受け持つが、2 つ目の cancel 節と操作節が 0 個の誤りの位置は、どの cram も確かめない(grep)。
   e5_twocancel と e6_noop を cram に足すか、記事の文を「主な位置を確かめる」のように弱める。

### 判断事項について

P[V58-1] と P[V58-3] の推奨に異論は無い。
P[V58-2] について、見落とした代案が 1 つある。
ラベルの誤りは、節のパターンではなく、ラベルを書いた引数のパターン(`a.T.cap_pat`)を指すこともできる。
ただし `cap_pat` の位置はラベルの後ろのパターン(`x = a` の `a`)で、ラベルそのものではない。
節のパターンを指す推奨案のほうが、引数の個数の誤りとそろうので、推奨案を採ってよい。
