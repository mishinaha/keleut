### N.z 合成の確認

#### 順序

M[effects] の中で、次の順に入れる。

1. V25(handle の対象確定後の検査の順)
2. V58(handle の節の誤りの位置)
3. V127(引数の個数の診断)
4. V101(`perform` と `resume` の引数の検査モード)
5. X-perform(V101 に吸収。行の不一致の §9 の案内と D[V101-3])
6. V133(`resume` を書けない位置の診断)
7. V89(§13.5 のガードの範囲と cram)
8. V128(§13.6、§15.5 の `cancel` 節の規則と cram)

順序の理由は次のとおりである。

- V25 → V58: 同じ所属の検査の行に触る。統括者の要望どおり、qual5 の位置(5:19 から 7:8)は V58 の期待値のコミットで直す。
- V127 → V101: V127 の `check_call_arity` は、perform の引数を `elab_check` に通す行より前に置く。V127 を先に当てると、この並びが自然に得られる。
- V101 → X-perform: X-perform は V101 の `elab_check` を前提にする。コミットは分ける。
- V58、V101 → V133: V133 は環境の `never_clause` を `no_resume` に置き換える。後から当てる V133 の側で、外れた 1 行を `no_resume` の形に当て直す。V101 の差分は `{ env with ... }` を書かないので、V101 の側で `no_resume` を使う箇所は生じない。
- V133 → V89: V89 の cram の innerguard を、V133 の文面で 1 度で書く。
- V128 はコードを変えず、ほかの項目と行が重ならないので最後に置く。

#### 衝突と手直し

当てた順と hunk ごとの結果は `doc/log/261017-3-spike/effects/order.txt` にある。
各回とも `patch -p1 --dry-run` で確かめてから当てた。

1. V58: `lib/elab.ml` の所属の検査の hunk が fuzz 2 で当たる。V25 が移した所属の検査に `at_node c.T.cl_pat` が掛かることを目で確かめた。`test/typecheck_m6.t` は V25 の qual5w の分だけ 17 行ずれて当たる。
2. V101: perform の引数の hunk が外れる(V127 の `check_call_arity` の行が直前にあるため)。`Unify.unify (elab_exp env level eff arg) args_row;` を `elab_check env level eff arg args_row;` に置き換えた。
3. X-perform(`X-perform-over-V101.patch`): `op_row_note` の hunk は fuzz 2 で `check_call_arity` の定義の後に当たる。perform の `elab_check` に `~row_note` を渡す hunk は 2 と同じ理由で外れるので、同じ行に当て直した。
4. V133: return 節の hunk は fuzz 2 で当たる(V58 がガードの行を `at_node` の形に変えたため)。cancel 節の hunk は外れるので、`{ env with resume_ty = None; never_clause = None }` を `{ env with resume_ty = None; no_resume = Some No_cancel_clause }` に当て直した。
5. 合成で見つけた食い違い: X-perform の `fallback` が矢印を分けて単一化するので、V127 の個数の検査を通らない。D[V101-3] として `Unify.check_arity` の呼び出しを 1 行足した(`doc/log/261017-3-spike/effects/arity-fallback.patch`)。詳細は N.4 の (3) と (5) にある。

1〜4 は文脈のずれによるもので、項目の内容は変わらない。
5 は項目の内容に関わるので、裁定 D[V101-3] とした。

#### 合成後の dune build と dune runtest

`dune build` は通る(警告は HEAD からある `lib/parser.mly` の警告 60 だけ)。
`dune runtest` の結果は `doc/log/261017-3-spike/effects/runtest.log` にあり、5 の手直しの前後で同じだった(実測)。

- 新しく落ちるブロック: 0。
- 期待値が変わるブロック: 5 ファイルの 7 ブロック。どれも項目の最終稿の (5) が予告したものである。
  - `test/typecheck_m6.t` の qual5: 5:19 から 7:8(V25 と V58 の合成。V58 のコミット 2 で改める)。
  - `test/typecheck_m6.t` の efferr と `test/annot_rows.t` の nested2: §11.12 と同じ案内が付く(X-perform の P[X-perform-2]。X-perform のコミット 2 で改める)。
  - `test/annot_rows.t` の opsig2: 4:48 から 4:63(V101 のコミット 2)、さらに `op_row_note` の文言が付く(X-perform のコミット 2)。
  - `test/eval.t` の with2 と `test/trailing_block.t` の tuple2: 引数の個数の文面(V127 のコミット 3)。
  - `test/resume_guard.t` の innerguard: V89 の新しいブロックの HEAD の文面が、V133 の操作節のガードの文面に変わる。V89 の新しいファイルを V133 の文面で書く(N.6)。
- 差分に入れた期待値のまま通るブロック: V25 の qual5 の文面と qual5w、V58 の 6 ブロックと `test/handle_clause_loc.t` の 15 ブロック、V127 の `test/arity.t`、V133 の gresume、gresume2 と `test/resume_sites.t`、V89 の `test/resume_guard.t` の innerguard 以外の 4 ブロック、V128 の `test/eval.t` の 3 ブロック。
- そのほかのブロックは変わらない。

V101 と X-perform の新しいブロック(合わせて 12 個)と、D[V101-3] の perfarity のブロックは、試作に cram のファイルが無いので合成に含めていない。
その期待値は、合成の版で例を走らせ直して確かめた(N.4 の (5))。

合成の版では、全項目の例(`ex/`、`ex2/`、`verify/ex/`)を走らせ、HEAD の版と各項目の試作の版の出力と比べた。
項目の試作の版との違いは、次のとおりで、どれも別の項目の変更で説明がつく(実測)。

- V25 の例の位置(`prelude5.kel` 4:8、`qual5b.kel` と `qual5c.kel` と `qual5.kel` 7:8、`r9_typo.kel` 6:8): V58 による。
- V127 の例の操作節の引数の個数の誤りの位置(`c3.kel` 3:8、`s3.kel` 4:8): V58 による。
- X-perform の `a6.kel` の文面: V127 による。
- V101 の `w1pureperf`、`w2pubeff`、`w5callpure` の案内: X-perform による。`e5.kel` の文面: V127 と D[V101-3] による。
- V89 の拒否する例(`g1`、`g4`、`g5`、`g10`、`g11`、`g12`)の文面: V133 による。

`tools/litcheck.py --old` で HEAD の版と比べると、`lib/elab.ml`、`lib/unify.ml`、`lib/interp.ml`、`lib/parser.mly` のどれも marker は 0 件だった。
`lib/elab.ml` と `lib/unify.ml` の code の不一致はコードの変更によるもので、`lib/interp.ml` と `lib/parser.mly` は code も一致した(実測)。
実施時はリポジトリの中で `tools/litwrap.py` と `tools/litcheck.py` を走らせる。

#### LangSpec の同じ行を触る項目の調整

- §13.5(1273 行): この分野で文面を書き換えるのは V89 だけである。V133 は仕様を変えず、診断の文言で「操作節のガード」「return 節」「cancel 節」の語を使う。V89 の改訂文の 1 文目「操作節のガード（§13.4）と、`return` 節、`cancel` 節の中でも使えない。」と語が揃っている。
- §13.6: V128 は 1290 行の後に段落を足し、1315 行を置き換える。1297〜1298 行の「ガード」は残し、新しい文では「ガード」の語を使わないので、V89 の「操作節のガード」の読みと食い違わない。V89 は §13.6 を変えない。
- §15.5(1611 行の後): V128 が 1 文を足す。分野 syntax の X-E13 が同じ節の 1610 行と 1617〜1625 行を書き換えるので、統括者が両方を同じ版の上で合わせる。
- §13.2(1136、1137、1170 行)と §13.3(1204 行と 1206 行の間): V101 だけがこの分野で触る。分野 classes の V103 が同じ段落(1169〜1172 行)の制限の 2 つ目の周辺を触るので、V103 の文面の位置の列挙は V101 の後の 1170 行に合わせる。

#### ほかの分野への要望

- 分野 eval の V137(V138 を吸収): V128 は `lib/interp.ml` §14.9 の「3 つの経路」の holecancel の文の後(HEAD の 695 行の後)に 3 文を足し、LangSpec §13.6 に規則を足す。V137 はこの記事の「Unwind のプロトコル」、「3 つの経路」の表、「再開しない操作の節」を書き換える。本分野の合成の上に `V137.patch` を当てると、ずれだけで当たり、`dune runtest` の結果は変わらず、V128 の例の出力は HEAD と一致した(実測)。V137 の記事の改訂では、holecancel の文と V128 の 3 文の並びを残し、表の 3 行目の「捨てた継続の中と自分の cancel 節を走らせる」を V128 の規則と同じ意味に保つことを求める。V128 の (2) が引く再開しない操作の経路の行番号(1060〜1063 行)は V137 の後の形で読み替える。
- 分野 never と classes の V132、V135、V129、V134: どれも `test/never_ops.t` を触る(V134 は `lib/elab.ml` と `lib/exhaust.ml` も触る)。本分野は `test/never_ops.t` を変えないが、V133 は aliasresume、resumedirect、resumelambda、resumenested、mixedresume を、V128 は fromclause を、変わらない観測点として使う。これらのブロックの期待値を変えないことを求める。4 項目の差分は、本分野の合成の上にそれぞれずれだけで当たる(`--dry-run` で実測)。V135 は §11.15 の perform の失敗の分岐の周辺を読むが、X-perform がこの分岐に `repr eff = TRowEmpty` の腕を足すので、V135 の文面や記事がこの分岐を引くときは X-perform の後の形に合わせる。
- 分野 diagtext の V102: `lib/elab.ml` §11.18 の `Construct` の単一化と `row_failure` の周辺に触る。V127 を先に入れ、V102 の cram の ctorarity などで個数の誤りを観測するなら、期待値を V127 の文面(「引数の個数が一致しません」)で書く。V24、V34、V111 も `row_failure` の周辺の文脈に触るので、V127 の `args_arity`、`check_call_arity` と X-perform の `op_row_note` の挿入位置(`row_failure` の定義の後)との当てる順を統括者が決める。
- 分野 syntax の V60: `test/trailing_block.t` の tuple2 の期待値を V127 が変える。期待値のコミットを項目ごとに分ける。
- 分野 numeric の V114: V127 の例 s2.kel の終了状態は、V114 の後は HEAD でも V127 の後でも 1 になる。
- 分野 kinds の V117: pub の値束縛の言い換え(`lib/elab.ml` 3128〜3132 行)の文面が V127 の r2.kel で変わる。V117 の差分は V25 の qual5w(`test/typecheck_m6.t` に 17 行)の分だけずれる。
- 分野 classes の V110、V136: インスタンスメソッドの検査(§11.38)の 4546 行の前に V127 が 1 行足す。
- 分野 region の X-D74、X-regionshow: `test/typecheck_m6.t`(325、345 行)、`test/verify_fixes.t`(389 行)、`test/annot_rows.t`(1009 行)の別のブロックに触る。本分野の V25、V58、V133、V101 が同じファイルの別のブロックを変えるので、期待値のコミットを項目ごとに分け、後から入れる側で `dune promote` し直す。
- 分野 spec の X-effsuffix: nested2 の `call_pure : (() => {} @ {}) => {}` の行は X-perform で動かない。X-effsuffix が型の表示を変えるなら、nested2 の期待値は両方の変更を合わせて書く。
