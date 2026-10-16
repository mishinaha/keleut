# V108 の反証

## 裏取り

試作の差分(183 行)は写し verify/diktor にそのまま当たり、dune build と dune runtest は緑だった(runtest の出力は空)。
担当の分類(既存の期待値で変わるものは無く、新しいブロックが 1 つ増える)は正しい。
ex/block.txt と r1〜r5 は HEAD と試作のどちらでも results.head.txt と results.proto.txt に一致した。
--type-check と実行(ex/f1.kel)の出力は HEAD と試作で同じだった。
VDictAbs を作る位置は lib/interp.ml の 588、616、1254、1411、1581 行の 5 か所で、D[V108-1] の「5 か所」は正しい。
value.ml 456、475、480 行、driver.ml 804 行、interp.ml 97 行、LangSpec §6.6 の 689-711 行は正しい。
litcheck は試作で marker 0 件、code の不一致は interp.ml と value.ml と driver.ml だけだった。

行の誤り:
- 「§6.4(650-652 行)により初期化式は構文上の値」の文は LangSpec 654 行である。
- (4) の「評価の時点の違いが現れるのは…(652 行)」は 655 行である。
- DExp の「1578-1582 行」は `| T.DExp e ->` が 1577 行である。
- P[V108-2] の代案 1 で変わるのは repl.t の 273、274、277 行の 3 行で、275-276 行(pr)は <fn> のまま残る(fn_name を外した版を作って実測。alt1)。

## 書かれていない振る舞いの変化

前提なしの束縛子を持つ newtype の Show のインスタンスがあると、制約付きの値の表示が Show の経路に入る(ex/e20.txt)。
HEAD は利用者の show に VDictAbs を渡す。
- `type instance[A] Show[W[_]] { let show(w) = w match { case W(_) => "W!" } }` の後の `let x = W(show)` を、HEAD は「実行時エラー: match のどの節にも一致しません: <fn>」で受理しない。試作は `x : [A: Show] W[(A) => String] = W!` で受理する。
- `type instance[A] Show[U[_]] { let show(u) = "U!" }` の後の `let z = U((show, ???))` を、HEAD は `= U!` と出し、試作は `= <unevaluated>` と出す。
下書きの「入力の受理は変わらない」「差が <fn> の行だけ」はこの形で成り立たない。
(7) の差の記述も、r1 の t と r3 の h の `<prim Show.show>` の行を含めていない。

## da_last の主張

表示の評価の外側は da_fn を直に呼ぶが、初期化式の中の識別子(show や前の入力の p)は with_evidence から apply_dicts(interp.ml 459-470 行)を通り、その値の da_last に DPending の列と結果を残す。
結果は同じ辞書から同じ値を作るので、後の使用は変わらなかった(ex/e6.txt の p3、ex/e9.txt の p match)。
「da_last の覚え書きを通さない」は外側の値に限って正しい、と書き直すべきである。

## 退行の探索

ex/e1〜e20 を両方で走らせた(全出力は ex/all.results.txt)。
エイリアスや注釈付きの値は構文の誤りで試せなかったが、module の中の pub、クラスのメソッドに制約を持つ pp、--no-prelude の利用者のクラス、ブロック式 `{ (show, 1) }`、穴を含む値の後の使用を試した。
一般化は値制限どおりで、エフェクトを起こす初期化式は HEAD でも試作でも曖昧性エラーになり、表示の評価に届かない(e1、e16、e17)。
変化は上の newtype の 2 形と、P[V108-2] による `<prim 名前>` から `<fn>` への変化(e13 の h)だけだった。

## 重なり

V109 は lib/driver.ml の show_value の 815 行(env のレコード)、lib/value.ml、lib/interp.ml の bind_dicts(97 行)を変える。
V137 は lib/value.ml の Value.show の VPending の腕(481 行)と Unwind を変える。
下書きの (1) はこの 2 つを挙げていない。
V109、V137、V114、V115、V118、V121、V123 の差分を当てた後に V108 の差分は当たり、V109 と V108 を両方当てた版は build と runtest が緑だった。
