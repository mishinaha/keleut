# 全分野の合成の確認

計画 261017-3 の 15.2 節と同じ内容である。段ごとの手直しの全文は order.txt、段ごとの promote 前の runtest の差分は runtest.log、生の記録は logs/ にある。

筆者は、10 分野の合成差分（`261017-3-spike/<分野>/<分野>.patch`）を、上の順で diktor 39d4091 の写しに `patch -p1` で当て、段ごとに `dune build` と `dune runtest` を走らせた（担当 1 人。記録は `261017-3-spike/all/`）。
最後の段まで当てた木で `dune build` は通り（警告は HEAD と同じ `lib/parser.mly` の Warning 60 だけ）、段ごとに期待値を更新した後の `dune runtest` の差分は 0 行である。
`menhir --explain --strict --table` の衝突は 0 件、`tools/litcheck.py --old` の marker は変えた 15 ファイル（`lib/dune` を含む）すべてで 0 件、`test/` での sample の型検査は終了コード 0 で 68 行（HEAD と同一）だった。
順序を変える必要は無いと判断した。

段ごとの当たり方は次のとおりである。

| 段 | 分野 | ファイル | 当たらない hunk | fuzz で当たった hunk | 手直し | その段で promote が要った既存のブロック |
|---|---|---|---|---|---|---|
| 1 | spec（M67） | 2 | 0 | 0 | 0 | 0 |
| 2 | syntax（M68） | 25 | 0 | 0 | 0 | 0 |
| 3 | numeric（M69） | 14 | 1 | 0 | 1 | 0 |
| 4 | eval（M70） | 8 | 0 | 1 | 0 | 0 |
| 5 | effects（M71） | 12 | 0 | 0 | 0 | 7 ファイル 9 ブロック |
| 6 | never（M72） | 4 | 0 | 0 | 0 | 0 |
| 7 | classes（M73） | 15 | 0 | 3 | 0 | 1 ファイル 1 ブロック |
| 8 | region（M74） | 13 | 0 | 1 | 0 | 1 ファイル 1 ブロック |
| 9 | diagtext（M75） | 14 | 3 | 3 | 3 | 4 ファイル 5 ブロック |
| 9b | numeric の要望（`default_to` に経由元を渡す） | 1 | 0 | 0 | 1 | 1 ファイル 2 ブロック |
| 10 | kinds（M76） | 7 | 3 | 0 | 4 | 1 ファイル 3 ブロック |
| 11 | extra（M77。検証で加えた 6 件） | 12 | 0 | 0 | 0 | 1 ファイル 1 ブロック |

「その段で promote が要った既存のブロック」は、その段の差分を当てた後の `dune runtest` で期待値と違ったブロックで、前の段までの変更によるもの（段 7 以降の大半）と、その段の差分が期待値を含んでいなかったもの（段 5 の effects の 7 ブロック、段 11 の ml5c）を含む。各段で `dune promote` して次へ進んだ。
段 11 は、検証で加えた 6 件（本計画 13 節）の差分を段 10 までの木の上で順に当てたもので、13.7 節の担当と反証者が測った（失敗した hunk は無く、`test/typecheck.t` の ml5c の 1 行は V49 の予告どおり）。
offset だけで当たった hunk は多いが、fuzz で当たった hunk は挿入位置と内容を目で確かめ、すべて正しい位置に入っていた。

手直しは 9 か所で、どれも文脈のずれによるものである（全文は `261017-3-spike/all/order.txt`）。

- numeric の `lib/driver.ml`（§16.8 の終了コードの表）：syntax の V121 が同じ表の 2 の行を変えていたので、V114 が変える 1 の行と 4 の行だけを入れた。
- diagtext の `lib/elab.ml`（V102）：`row_failure` の直後に effects の関数（`args_arity`、`check_call_arity`、`op_row_note`）が入っていたので、`has_sub` を `row_failure` の前に出し、`closed_row_arg` と `has_closed_arrow` を `row_failure` の直後に置いた。
- diagtext の `lib/elab.ml`（V24）：classes の V106 が `check_tparams` を間に足していたので、V24 の `at_node` と重複の書き換えを V106 の行を残して入れた。
- diagtext の `lib/unify.ml`（X-premise の `reset`）：numeric の V91 が `reset` を変えていたので、`Hashtbl.reset premise_origins;` を足した。
- numeric の要望（段 9b）：X-premise を後に入れる側が `default_to` で経由元を渡す手直し（`261017-3-spike/numeric/clean/default_to-via.patch`）を当てた。
- kinds の `lib/elab.ml`（V117）：`pub_effect_error` を、V24 が足した `at_node f.T.fd_ty` の中に入れた。
- kinds の `lib/elab.ml`（V30）：`add_effect` を `set_effect_ops` に替える 1 行だけを入れた。
- kinds の `lib/elab.ml`（V30、V22）：`DEffect` の枝と `speculate_until_stable` の塊を、classes の V106 の `class_checks` とパス 1b の見出しを残して入れた。
- kinds の `test/pub.t`（V117 の pubaliastp）：classes の V126 の後はインスタンスの本体の `pub let` が構文エラー（終了状態 2）になるので、インスタンスの `let` から `pub` を外し、前書きの「インスタンスの型パラメータも同じである」を消した。
  新しく落ちたブロックはこの 1 つだけである。

promote が要った既存のブロックのうち、分野の節の「合成の確認」が予告していなかったものは次の 5 ブロックと、10.4 節の要望（V97 の後に `test/region.t` を走らせ直す）の範囲内だが変わる行を特定していなかった regionshow の 1 ブロックで、どれも別の分野の変更で説明がつく。
実施では、後に入れる分野の cram のコミットでこれらを合わせる（D[org-7]）。

- `test/smoke.t` の cr（2:9 から 2:50）と `test/typecheck.t` の mix（3:11 から 3:52）：numeric の V114 が入力を `cancel(reason)` の形に差し替え、effects の V58 がその位置を節に寄せる。
- `test/region.t` の regionshow の `w` の行：classes の V97 が組み込みの署名の行を閉じたので ` extends _R1` が消える（10.4 節の要望の範囲）。
- `test/never_ops.t` の clsfailpure：classes の V132 のブロックの文面が、diagtext の V33 の「注釈に @ {} 以外の行を書いています」になる。プログラムは実装の側に `@ Fail` を書いており、V33 の意図どおりである。
- `test/pub.t` の pubaliasnt と pubaliasarg4（2:1 から 2:23）：diagtext の V24 がフィールドの型の位置を指す。

このほかに `test/ctor_field_row.t` の ctorarity（effects の V127 の文面）と `test/rec_tparam_note.t` の region.kel（region の X-D74 の文面）も変わるが、これらは本計画 11.7 節と 10.4 節が予告していた。

分野の「合成の確認」の要望のうち、この順で満たされないものが 6 つあるが、どれも壊れなかった。

- classes は `test/premise.t` を変える分野を classes の後に置くことを求めたが、syntax（M68）の V115 と V118 が同じファイルの別のブロック（pr15）を変える。classes の pr7 とは別のブロックで、段 7 は手直しなしで当たった。
- classes は V101、V108、V109 を classes の後に当てることを求めたが、effects と eval が先である。`test/repl.t` は promote で済み、`test/builtin_rows.t` と `test/instance_param_types.t` は通った。
- classes は V136 を V135 より先に入れることを求めたが、never が先である。V135 の例 26 本と never_ops.t の neverclass 系のブロックは、どの例も仮引数に `Never` を注釈しているので変わらなかった。
- diagtext は V122 を diagtext の後に当てることを求めたが、region が先である。diagtext の `lib/unify.ml` の hunk は fuzz で正しく当たり、V111 の注記は X-D74 の包んだ文面の後に付く（X-D74 の f1.kel で実測）。
- region は X-perform を region の後に入れることを求めたが、effects が先である。region の段で変わったのは 1 行だけだった。
- numeric は V109 の文脈の行を直すことを求めたが、eval の差分は fuzz 2 で当たった。

region と kinds は、X-effsuffix と V28 がコードを変える前提で順序を求めていたが、どちらも解消済みで差分を持たない（本計画 3.3 節と 3.4 節）。この要望は取り下げた（10.4 節と 12.9 節に反映した）。

ほかの分野（先の分野を含む）の変更で、項目の例の出力が、その分野だけの合成と全分野の合成で違う組は次のとおりである。各項目の (7) の「合成の処理系で一致」は、断りが無ければその分野だけを HEAD に当てた合成を指す。syntax の V115 の文面の置き換え（「パースエラー」から「構文エラー」）は、構文エラーを含むどの項目の例にも及ぶ。

- V110（classes）の例 c1、c3、u5、r7、r5、r13、r17、q4 は、diagtext の V33 と V34、syntax の V115 で文面が変わる。
- V111（diagtext）の例 fp1〜fp3 は、region の X-D74 で文面が変わる（M74 が M75 より先）。
- V136（classes）の例 j.kel は、effects の V127 で個数の文面になる。
- X-F5b（syntax）の例は numeric の V114 で、V114 の例は effects の V58 で位置や文面が変わる。
- V134（never）の例 r3 と r9 は、never の合成では V129 の文面になるが、classes の V136 の後は従来の文面「例えば _ が漏れています」に戻り、r12 と v136 は警告を出さない。
- X-D74（region）の例 u5 と u6 は、diagtext の V102 の後に「コンストラクタ Cons のフィールドの行です」の言い換えが消える。

M71 の V101 と X-perform の新しい cram のブロック 13 個（`test/annot_rows.t` に 12、`test/arity.t` に 1）は、試作に cram のファイルが無いので全分野の合成差分に入っていない。合成の `dune runtest` の差分 0 行はこれらを含まない。M71 で、7.4 節の表と合成の写しで走らせた例から期待値を起こす。

そのほかに確かめた点は次のとおりである。
V108 の `test/repl.t` の `t` の行は classes の後に `@ {}` になった。
V127 の s2.kel の終了状態は 1 で、報告は 2:18 の個数の型エラーである（本計画 7.3 節に反映した）。
V32 の recrowclosed と recrowclosedmutrev の 2 ブロックは diagtext の V34 の後に「空の行と一致させられません」に変わった。
V37 のプレリュードの 2 つの hunk は V83 の後に offset 3 で当たった。
インスタンスの頭が `Never` のときの、仮引数を注釈しない `let describe(n) = n match {}` は、V136 の前は網羅性の警告が出て、後は出ない。

実施で要ることは次のとおりである。

- numeric の段で、V114 の §16.8 の表の hunk を V121 の後の形で作り直す。
- diagtext の段で、上の 3 つの hunk を作り直し、`default_to` の経由元を X-premise の実装に含め、clsfailpure の期待値の更新を V33 の cram のコミットに含める。
- kinds の段で、上の 3 つの hunk を作り直し、pubaliastp のインスタンスから `pub` を外す。
- `tools/litwrap.py --check` は、表の行の幅の報告が `lib/driver.ml` の V114 の 1 の行（幅 146）で 1 件増え、`lib/elab.ml` の 1a の行は HEAD でも報告されていた幅が 123 から 156 に伸びる。`lib/prelude.kel` の報告は V83 で消える。実施では表の行を折り返すか短くする。
- 未確認事項：V106 の `check_tparams` の型エラーの位置を、V24 の方針どおりメソッドの型の位置に寄せるかは測っていない（いまは `at_node` で包んでいない）。

全分野の合成差分 `261017-3-spike/all/all.patch` は、段 10 までで 65 ファイル（`lib/` 15、`test/` 50。うち新しいファイル 18）、追加 6445 行、削除 870 行である。
HEAD の写しに `patch -p1` で当て直すと、合成の写しと一致した。
段 11 の差分は `261017-3-spike/extra/extra.patch`（段 10 までの木に対するもの）にある。
