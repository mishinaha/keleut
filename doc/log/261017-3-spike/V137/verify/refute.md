# V137 の下書きの反証

作業領域は <一時領域>/items/V137/verify である。
試作は verify/diktor(V137.patch を当てた写し)、1 本目だけの版は verify/s1/diktor(V137-step1.patch を当てた写し)である。

## 1. 裏取り(ソースと文書)

- `lib/interp.ml` の `eval_handle` は 954 行、`run_cancel` は 969〜979 行、`exnc` は 995〜1004 行、再開しない操作の経路は 1047〜1063 行、§14.4 の `Resume` の判定は 412 行で、下書きのとおりである。
- `lib/value.ml` の `VPending` は 88 行(型)と 481 行(`show` の `<pending>`)にあり、`Unwind` の宣言は 248 行である。
- LangSpec §5.7 の除外は 480〜482 行、残す文は 477〜479 行、「次のものは保証しない」は 500〜512 行、§13.6 の打ち切りは 1323〜1329 行で、下書きのとおりである。
- 261017-2-m63.md の 218 行(再検証の表)、221 行(`VPending` が届かないことの検証)、268 行(台帳 V137)は下書きのとおりである。
- 261017-1-plan.md の「286〜300 行(3 つの経路と cancel 節からの操作)」は範囲がずれている。
  3 つの経路の表は 268〜276 行、`cancel` 節から再開しない操作を起こす形は 283〜284 行、`effc` の末尾で評価する形(D340)は 286〜292 行で、293〜300 行は「判定の位置」(`is_never`)である。
- cram の挿入位置を下書きは「retry と retryresumepath の間」と書くが、差分は retryresume のブロック(`test/tail_calls.t` 182〜193 行)の後、retryresumepath の前書き(196 行)の前に入れている。
- 差分 `V137.patch` は 330 行である。
- never_ops.t の order、clauseerror、rethrow、otherescape、cancelnormal、cancelsame、cancelresume、cancelresumevia、cancelresumetwo は実在する。

## 2. 実測の再現

`OCAMLRUNPARAM=l=100k`、10 万回で、HEAD と試作を走らせ直した。

| 例 | HEAD | 試作 |
|---|---|---|
| retrycancel | `0`、終了状態 0、正常な打ち切りの記録 33771 行と `Stack overflow` 66229 行 | `0`、終了状態 0、正常な記録 100000 行 |
| retrycancelvia | 同上 | 同上 |
| grow_tick | 同上 | 同上 |
| retrycancelsame | `0`、正常な記録 100000 行 | 同じ |

下書きの表と一致した。
試作は 100 万回(l=100k)と 10 万回(l=20k)でも正常な記録だけで終わった。
末尾でない `resume` の入れ子の深さ(nontail_N.kel を l=100k で二分探索)は HEAD 5403 段、試作 7946 段、1 本目だけの版 5403 段で、下書きと一致した。
時間(既定のスタック、5 回の最短)は retrycancel 0.65 倍、nontail5000 0.97 倍、retry 1.00 倍で、下書きの傾向と同じである。

### 下書きに無い形: 打ち切られたハンドラの return 節からの再帰

verify/ex/s1_retclause.kel は、外側のハンドラが打ち切られた後に、そのハンドラの `return` 節から再帰する形である。

```
let rec loop(n: Int32): Int32 =
  if n == 0 then 0 else
  (({ perform use(); perform stop(); 1 }) handle {
    case use() => resume()
    case cancel => perform throw("from cancel")
  } handle {
    case stop() => 5
  }) handle {
    case throw(e) => { echoln("caught " + e); 1 }
    case return(x) => loop(n - 1)
  }
echoln(show(loop(100000)))
```

HEAD(l=100k)は正常な打ち切りの記録 33761 行の後に `実行時エラー: スタックオーバーフロー(再帰が深すぎます)` で終了状態 3 になった。
試作と 1 本目だけの版は `0`、終了状態 0、正常な記録 100000 行である。
この形の `return` 節は「打ち切りの後にそのハンドラの本体の中で評価される節の本体」に当たらず、HEAD の §5.7 の除外(480〜482 行)は覆っていない。
したがって HEAD はこの形で §5.7 の `return` 節の保証(475 行)を満たしていなかった(実測)。
下書きの「台帳の 10 万回がスタックオーバーフローは、実測では終了状態 3 ではなく…抑制され記録される形で現れた」は 3 つの例に限れば正しいが、この形では終了状態 3 で現れる。
「溢れるのは cancel 節の中の perform だけだと推測する」も、この形では成り立たない。

## 3. 試作の差分

`V137.patch` は写しに当たり、`dune build` は通り、`dune runtest` は差分 0 だった(verify/runtest.log は空)。
当てた結果は担当の写しと `diff -r -x _build` で一致した。
`V137-step1.patch` も写しに当たり、`dune runtest` は差分 0 だった(verify/runtest-step1.log は空)。
`tools/litcheck.py --old` は interp.ml と value.ml で code の不一致(意図した変更)だけを報告し、marker は 0 件で、`tools/litwrap.py` を走らせても interp.ml は変わらなかった。
担当の分類(既存のブロックは変わらない、新しい 3 プログラムのうち retrycancel と retrycancelvia は HEAD で落ちる)は正しい。

## 4. 退行の探索

担当の例とは別に、次の 13 本を HEAD と試作で走らせ、標準出力、標準エラー、終了状態がすべて一致した(verify/out/)。

- r1_clause_perform: 再開しない操作の節の本体が外側の再開する操作を起こし、外側が末尾でない `resume` で返す。
- r2_stop_then_error: 打ち切りの後に、別のハンドラの再開しない操作の節の本体が再び再開しない操作を起こす。
- r3_stop_twice: 2 段の `cancel` 節がそれぞれ打ち切られる。
- r4_stop_then_same_again: 打ち切られたハンドラの本体が、後で同じハンドラに再び再開しない操作を送る。
- r5_nontail_stop: 同じハンドラの末尾でない `resume` の中で打ち切りが起きる。
- r6_guard_err: ガード付きの節と、節の本体の実行時エラー(終了状態 3)。
- b1_import: `import` した `pub` の関数とエイリアス `type Boom = Never` を返す操作。
- b2_module: `module` の中の型パラメータ付きの関数とガード付きの再開しない操作の節。
- b3_tick_outer: 打ち切られる外側のハンドラの末尾でない `resume`。
- b4_withstdout: プレリュードの `with_stdout` の下の再試行。
- b5_prelude: `--prelude` でプレリュードを差し替えた実行。
- b6_repl.txt: `--repl` での打ち切りと節の本体の実行時エラー。
- s1_retclause: 上の 2 節の形(HEAD は終了状態 3、試作は 0。HEAD の誤りの修正であり退行ではない)。

さらに、深さ 3 までの入れ子のハンドラ、末尾と末尾でない `resume`、`resume` しない節、値・操作・実行時エラーで終わる再開しない操作の節、操作を起こす `cancel` 節、`return` 節を無作為に組んだ 5000 本(verify/gen.py、verify/gen と verify/gen2)を比べ、5000 本すべてが一致した。
そのうち 1200 本が `cancel` 節の打ち切りを記録し、377 本が終了状態 3 で終わる。
退行は見つからなかった。

## 5. 仕様の文面

§5.7 の 480〜482 行を消すと、475 行(`return` 節)と 476〜479 行(再開しない操作の節)の保証が打ち切りの後にも及ぶ。
試作は retrycancel、retrycancelvia、retrycancelsame、s1_retclause でそれを満たす(実測)。
§13.6(1323〜1329 行)の打ち切りの意味は変わらず、§15.5 の終了状態、§18、付録 A の文法とは関わらない。
§15 の診断の記録(1629 行)も変わらない。

## 6. 判断事項

- P[V137-1]: 推奨案に同意する。
  見落とした代案として、`exception Unwind of oid * (unit -> t)` とし、再開する操作の経路では `fun () -> v` を載せる形がある。
  型が 1 つ減るが、resume しない節のたびに閉包を確保するので、推奨案のほうが意図を型で示せる。
- P[V137-2]: 推奨案に同意する。
  ただし根拠に、HEAD が除外の外の `return` 節の形(s1_retclause)で保証を破っていたことを足し、cram にもその形を足すべきである。
- P[V137-3]: 分割は成り立つ(1 本目だけで runtest 0 差分、深さ 5403 段を実測)。
  ただし `V137-step1.patch` は記事を改訂しておらず、1 本目に入れると `effc`(1048〜1059 行)、`exnc`(997〜999 行)、§14.9 の「再開しない操作の節」の記事が印の照合を説明したまま残る。
  1 本目にも記事の改訂(印の照合を外した分)を含めると計画に書くべきである。
  また 1 本目では §14.4 の `Resume` の `VPending` の分岐が到達しない枝になるので、その注釈も 1 本目で直すか、2 本目と合わせて 1 本にするかを決める必要がある。

## 7. ほかの項目との重なり

- V138: 同じ `lib/interp.ml` 412 行(§14.4 の `Resume`)と、§14.9 の記事の V138 の段落(741〜744 行付近)を触る。V137 の 2 本目が V138 を解消するので、V138 を独立に直す下書きとは衝突する。
- V128: §14.9 の「3 つの経路」の表(673 行)と 1084〜1115 行の再開する操作の経路を触りうる。V137 の差分はその表の `Unwind (inst, v)` を `UValue v` に書き換え、1093 行と 1109 行の `discontinue` も書き換えるので、文面とコードが衝突する。
- `test/tail_calls.t` は V137 だけが変える(一覧のほかの項目には tail_calls.t を挙げるものは無い)。
- LangSpec §5.7 の 480〜482 行を変える項目はほかに無い。§13.6 は V128(1312〜1315 行付近の cancel 節の規定)が変えうるが、V137 は §13.6 を変えない。
