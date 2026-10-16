## X-E13 の反証

対象は <一時領域>/items/X-E13/draft.md と X-E13.patch である。
試作の差分は写し(verify/diktor)に当たり、`dune build` と `dune runtest` は差分なしで通った(verify/runtest.log は空、rt=0)。
例は verify/ex/ にある。
HEAD は scratchpad/bin/diktor-head、試作は verify/diktor/_build/default/bin/main.exe である。

### 1. 裏取り(正しいもの)

- LangSpec §15.5 は 1603 行、限度の文は 1610 行、表は 1617〜1622 行、「処理系が定める」は 1624〜1625 行、対話的な実行の継続は 1639 行である。
- driver.ml の `main` は 1129 行、`Out_of_memory` の節は 1175〜1177 行、`Stack_overflow` の節は 1196〜1198 行である。
- §16.8 の表の 1 と 3 の行は 1049、1051 行、入力の表の 2 行は 1078〜1079 行、不変条件は 1091〜1093 行である。
- `with_undo` の `exception ex` の節は 855 行、評価の段の `runtime_message` を出す節は 903 行である。
- elab.ml の `type_check` は 5529 行、`catch_input_errors` は 5558 行、`type_check_units` は 5870 行である。
  ただし `type_check_units` は §11.44 ではなく §11.45(5611 行〜)の中にある。
- `at_node`(elab.ml:163〜166)は `Type_error` と `NotImplemented` だけを受ける。
- elab.ml、unify.ml、exhaust.ml、decls.ml に `with _ ->` と `exception _` の節は無い。
- 出典の 260829-4-plan.md:118 と 4123〜、260912-1-status.md:518〜519 と 640 は、下書きの言うとおりの内容である。
  260829-4 の修正設計も終了コード 1 を選んでいる。
- 新しい cram は 4 ブロック、追加 36 行である。

### 2. 実測の再現

担当の awk で作った deep.kel は担当のものとバイト単位で一致した。
l=100k での実行、`--type-check`、`--repl` の出力と終了状態は、HEAD と試作の両方で results.head.txt と results.spike.txt のとおりだった。
l=8M の対照は両方で `before`、`20001`、0 である。
既定の上限での 100 万段(6 MB)も HEAD で `before`、`1000001`、0 だった(約 21 秒)。
`:type` に 2 万段の式を渡す形(ex/typerepl.txt)も、HEAD は 3 で終わり、試作は診断を出して続く。
これは `type_of_expr` が `catch_input_errors` を通るためで、下書きの cram には無い。

### 3. 反証: 構文解析の段でもスタックは溢れる

下書きは「構文解析は入れ子の深さで溢れない」とし、P[X-E13-3] で読み込みの段に節を置かないとする。
試作は driver.ml §16.8 の不変条件に「構文解析は入れ子の深さに比例するスタックを使わないので、読み込みの段には節を置かない」と書く。
実測ではこれが成り立たない。

parser.mly:304〜317 の `block_of_items` は、ブロックの文の列を末尾でない再帰で畳む(309 行と 316 行の再帰呼び出しの後に構築がある)。
ブロックの文の数に比例してスタックを使う。

ex/seq5000.kel(`let x = {` の中に `let aN = 1` を 5000 行並べ、`1 }` で閉じる)は、試作でも次のとおりである。

```
$ OCAMLRUNPARAM=l=16k main.exe seq5000.kel; echo "exit=$?"
実行時エラー: スタックオーバーフロー(再帰が深すぎます)
exit=3
$ OCAMLRUNPARAM=l=16k main.exe --type-check seq5000.kel; echo "exit=$?"
実行時エラー: スタックオーバーフロー(再帰が深すぎます)
exit=3
```

標準出力は空で、初期化は始まらない。
バックトレース付きの写し(verify/diktor-instr。`main` の節で `Printexc.get_backtrace` を印字するだけの改変)では、溢れた場所は `Diktor__Parser.Make.block_of_items`(parser.mly 316 行)である。
式文を並べた形(ex/seqexp5000.kel、309 行)とトップレベルの `with` の後ろの文(ex/topwith.kel)でも同じ関数で溢れる。
l=100k でも 5 万文(ex/seq50000.kel)で 3 になる。

対話的な実行でも、同じ形の入力(ex/seqrepl.txt、l=16k)でセッションが 3 で終わる。
`try_parse`(driver.ml:757〜773)は `Stack_overflow` を受けない。

```
$ OCAMLRUNPARAM=l=16k main.exe --repl < seqrepl.txt; echo "exit=$?"
y : Int32 = 1
実行時エラー: スタックオーバーフロー(再帰が深すぎます)
exit=3
```

したがって、試作の後も「初期化を始めないのに実行時エラーと名乗って 3 で終わる」形と「対話的な実行が 3 で終わる」形が、読み込みの段に残る。
P[X-E13-3] の根拠「到達する入力が作れず、cram で固定できない」も成り立たない(l=16k、5000 文で決まって再現する)。

`block_of_items` を、先に前から検査してから後ろから `List.fold_left` で畳む形に変える試作(verify/block_of_items.iter.patch、diktor-blk)では、`dune runtest` は差分なしで通った。
このとき seq5000.kel は l=16k で `! 処理系の限度: 型検査で…`、1 になり、seqexp5000.kel は l=16k で `before`、`1`、0 になる。
seqrepl.txt では診断のあとに `1` を出し、0 で終わる。
`block_of_items` のほかに構文解析の動作で再帰するものがあるかは、全数は調べていない。

### 4. その他の指摘

- 下書きの「コード 9 行」は、差分で数えると 6 行(`limit_error` の 3 行と節の 3 行)である。
- 下書きの「コマンド 10」は、差分の `$` の行で数えると 9 である。
- `litcheck.py` の幅の報告は、試作で driver.ml:1049 に新しく 1 件増える(幅 168)。
  HEAD の driver.ml にはこの報告が無い。
  下書きの「既存のものと同種」は種類としては正しいが、件数は増える。
- `--prelude` で型検査が限度を超えるプレリュードを渡すと、実行は試作で 1 になるが、`--repl` は 3 で終わる(ex/deepprelude.kel)。
  `repl` の `start` が `Elab.start_session` を受け手の外で呼ぶからである。
  試作のコメント「対話的な実行でも、型検査の段の限度は入力の拒否として報告され、セッションは続く」は入力については正しく、プレリュードは対象外である。
- API の `type_check_string` と `eval_string`(driver.ml:562〜580)は `flatten_modules` を `type_check` の外で呼ぶ。
  そこで溢れれば例外のまま出る。
  CLI の経路では平坦化は `type_check_units` の中なので、節が効く。
- elab.ml:5494 の「例外を型付きの返り値に変えるのは、下の `type_check` の 1 か所だけである」は HEAD でも成り立っていない(`catch_input_errors` と `type_check_units` もそうする)。
  試作は同じ段落を書き換えるので、合わせて直すのがよい。
- interp.ml:1466〜1472 の「driver は `Stack_overflow` と `Out_of_memory` を同じ終了コード 3 で報告する」は、評価の段の話としては試作の後も正しい。
  ただし §16.8 と読み合わせると段の限定が無いので、「評価の段の」を足すかを考える余地がある。

### 5. 退行の探索

HEAD が受理して正しく走るプログラムを 8 本、両方のビルドで走らせ、標準出力、標準エラー、終了状態、`--type-check` の出力を比べた。
どれも一致した。

- r1_alias_module.kel: 型エイリアスと module の中の pub の型エイリアス、タプルのパターン。`2,1`、0。
- r2_import.kel と lib/util.kel: import した型パラメータつきの関数と newtype。`42`、0。
- r3_handlers.kel: 再開しない操作と入れ子のハンドラ、`cancel` 節、ガード。`err big`、0。
- r4_rec_runtime_overflow.kel: ハンドラの中の評価の段の再帰過多(l=100k)。`before`、`cancel`、実行時エラー、3。
- r5_class.kel: 型クラスと制約つきの型パラメータ。`8`、`99`、0。
- r6_repl.txt: `--repl` で評価の段の再帰過多のあと `:type` と評価を続ける。両方 0 で同じ出力。
- `--prelude` に埋め込みと同じプレリュードを渡して r3 を走らせる。同じ。
- deep.kel を l=300k で走らせる(型検査が限度の手前で通る形)。同じ。

試作の節は型検査の受け手の最も外側にあり、型検査の中に例外をすべて受ける節は無い(1 節の grep)。
HEAD が受理する入力では `Stack_overflow` と `Out_of_memory` は型検査から出ないので、節に届かない。
退行が無いことはこの構造からも導ける。

### 6. 仕様の文面

表の 1 の行の条件に「型検査に要する領域が処理系の限度を超えた」を足す案は、1610 行、§6.6(型検査に通らなかった入力は受理しない)、1639 行と矛盾しない。
ただし 3 節のとおり、読み込みの段の限度は案の後も表と 1624〜1625 行の列挙のどちらにも無い。
`block_of_items` を直さないなら、文面を「読み込みと型検査に要する領域」にするか、読み込みの段を 1624 行の「処理系が定める」に足す必要がある。
§18 の冒頭は、評価の結果を標準出力と終わり方の組とし、各節は受理を変えないと述べる。
限度による拒否を型検査での拒否に数えると、限度の境目では、変換が受理を変えうる。
実際に変わる例は作っていない。
評価の段の限度(1610 行)についても同じ性質は HEAD から既にあるので、今回の案で新しく生じる矛盾ではない。
