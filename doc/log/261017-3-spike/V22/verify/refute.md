# V22 の下書きの反証

作業領域は `items/V22/verify/`。
写し `verify/diktor` は HEAD の写しに `V22.patch` を当てたもの、`verify/v1` は周回を外した変種、`verify/v2` はラベルの判定を外した変種、`verify/w` は HEAD に `items/V124/V124.patch` だけを当てたもの、`verify/c` は `V22.patch` の上に `items/V124/stage1.patch` を当てたもの、`verify/d` は `V22.patch` の上に `V124.patch` を当てて衝突を手で解いたものである。

## 1. 裏取り

`lib/elab.ml` の行番号は、次の 1 つを除いて HEAD(39d4091)と一致した。
`claim_type_name "effect"` は `:4782` ではなく `:4785` にある。

`:38`(章の表の 1b の行)、`:507-508`(`speculating` の注釈)、`:854-880`(`elab_con_args`)、`:865-867`(飛ばす読み)、`:917`、`:1165`、`:1179`、`:1188`、`:1228`、`:3590-3637`、`:3607-3611`、`:3713-3728`(`speculate_newtype`)、`:3821`(`Decls.add_effect`)、`:4640-4641`、`:4788-4795` はいずれも記述どおりだった。
`Hashtbl.mem Decls.effects` を判定に使う箇所は `elab.ml` に 4 か所(917、1179、1188、1228)あり、ほかは `:4710` の `taken Decls.effects` だけなので「呼び出し 4 か所」は正しい。
`test/kinds.t` の `:161-164`、`:286-290`、`:300-308`、`:310-312`、`:314-323`、`:324-332` も記述どおりだった。
`doc/LangSpec.md:641`(§6.4)の文面も記述どおりである。
`V22.patch` は 100 行である。

(7) の「`ex/` の 11 本」は誤りで、`ex/` には 12 本ある。
変わる 7 本と変わらない 4 本に `residual2.kel`(変更前も試作も `f : (A) => A`)が数えられていない。

(1) の「ずれは 8 行で当たる」は一部だけ正しい。
`stage1.patch` を `V22.patch` の後に当てると、1 つ目の hunk は 8 行、2 つ目の hunk は 33 行ずれて当たった(実測)。

## 2. 実測の再現

`ex/` の 12 本を HEAD と試作で走らせ、下書きの表と `results.head.txt` / `results.proto.txt` と同じ出力を得た。
specunit2、speclab、bare、rowalias、modfwd、residual、run の 7 本が変わり、typo、typealias、efflater、v30、residual2 の 5 本は変わらない。

2 つの変更がそれぞれ要るという主張も再現した。
周回を外した `v1` では specunit2 と speclab と bare は通り、residual は `4:34: 行カインドではない型パラメータです: E` で落ちる。
ラベルの判定を外した `v2` では specunit2、speclab、bare、residual がすべて HEAD と同じ診断で落ちる。

## 3. 試作の差分

`V22.patch` は `patch -p1` で当たり、`dune build` は警告だけで通った。
`dune runtest` の出力は担当の `runtest.log` と一致し、差分は `test/kinds.t` の specunit2 と speclab の 2 ブロックだけだった。
`litcheck.py --old` は code の不一致だけを報告し、marker は 0 件だった。

## 4. 退行の探索

HEAD が受理するプログラムで、試作が拒否するか出力を変える形は見つからなかった。
書いたプログラムは `verify/ex/` にある。

| プログラム | 形 | HEAD | 試作 |
|---|---|---|---|
| `r3b_run_nested_rev.kel` | ラベルつきの行を渡す newtype、入れ子のハンドラ、実行 | `pos` `k`、0 | 同じ |
| `r4_pubparam.kel` | `pub newtype`、型パラメータ 2 つ、`{Log extends R}` | `f : (A[A]) => A[A]` ほか、0 | 同じ |
| `r5_modshadow.kel` | module の中の Type エイリアス `Log` と、外の effect `Log` | `f : (M.A) => M.A`、0 | 同じ |
| `r6_guard_mod.kel` | module の中の前方参照、ガード、実行 | `big`、0 | 同じ |
| `r7_import.kel` | import した effect と newtype に `{}` を渡す、実行 | `imp`、0 | 同じ |
| `r9_repl.txt` | REPL。1b で落ちた effect の後の入力、入力をまたぐ effect | 4 行の出力、0 | 同じ |
| `r11_link.kel` | 300 個の newtype の連鎖 | 出力なし、0 | 同じ |
| `r13_nolabel.kel` | ラベルの無い版の種別の衝突 | `1:61` の診断、1 | 同じ |

試作が受理を広げる形として、下書きに無いものを 4 つ測った。
`r3_run_nested.kel`(`Holder[F] = MkHolder(Box[{Tick extends F}])` を `Box` の前に置く、実行)は HEAD が `3:34: エフェクトラベルはこの位置(レコード型)では使えません` で落ち、試作は `pos` `k` を出す。
`r8_prelude.kel`(差し替えたプレリュードの中の specunit2 の形)は HEAD が `4:40: 行カインドではない型パラメータです: E` で落ち、試作は `x : Wrap` を出す。
`r2_alias_gap.kel`(エイリアス経由で行を渡す newtype を前に置く形)は HEAD が `5:23` で落ち、試作は `f : (Z) => Z` を出す。
`r10_chain.kel`(300 個の newtype が順に後ろの newtype へ `{Log}` を渡す連鎖)は HEAD が `3:42` で落ち、試作は通る。
このとき周回は 300 回近く走るが、所要時間は 0.073 秒だった(HEAD は 0.004 秒で落ちる)。

両方が拒否するプログラムで診断が変わる形がある。
下書きはこの分類を書いていない。

```
effect Log = { log: (String) => Unit }
newtype B[E] = MkB((() => Unit @ E, () => Unit @ {Log})) | MkC(List[E])
```

HEAD は `r12_diagmove2.kel:2:34: 型エラー: 行カインドではない型パラメータです: E` を出す。
試作は `r12_diagmove2.kel:2:64: 型エラー: 型構成子 List の第1引数のカインドが一致しません: Type を期待しましたが R1 は Row です` を出す。
どちらも終了コードは 1 である。
前方参照の無い 1 つの newtype でも変わる。
ラベルを `{}` に替えた `r13_nolabel.kel` では HEAD も試作と同じ `List` の位置の診断を出すので、試作はラベルの有無による診断の違いを無くす向きに変えている。
`r1_diagmove.kel`(`B[E]` の後に `A = MkA(B[Int32])` を置く)も、HEAD の `3:34: 行カインドではない型パラメータです: E` から、試作の `4:19: 型 Int32 はエフェクトではありません(ここにはエフェクト行が要ります)` に変わる。
`V124.patch` だけを当てた写しも r1 で同じ診断を出す。

## 5. 仕様との突き合わせ

仕様を変えないという判断は §18 と §15.5 と矛盾しない。
§18.2 の並べ替えの性質は、互いを参照する宣言を条件 1 で除くので、V22 の形には掛からない。
§18.4 の 6 つ目の除外(`LangSpec.md:1869`)は後ろのエイリアスへ渡す `{}` の話で、V124 の範囲である。
付録 A の文法は変わらない。

(4) の「§6.4 は宣言順の非依存を無条件に書いており」は文面の読みとして強すぎる。
`:641` が書くのは「後ろで宣言する型名も参照できる」ことで、受理が宣言順に依存しないとは書いていない。
仕様を変えないという結論には影響しない。

## 6. ほかの項目との重なり

V124 の推奨案は `V124.patch` で、下書きが比べた `stage1.patch` は V124 の代案 A である。
`V124.patch` には D[V124-3] があり、投機の間だけ型の名前空間に effect として主張された名前をラベルとして読む(`V124.patch` の `@@ -1233` の hunk、`!speculating && args = [] && Hashtbl.find_opt Decls.type_namespace ... = Some "effect"`)。
これは D[V22-1] と同じ判定で、範囲が狭い(波括弧の中の引数の無いラベルだけ)。
`V124.patch` だけを当てた写しで、specunit2、speclab、rowalias、modfwd、run が通り、bare と residual が HEAD と同じ診断で落ちた(実測)。
したがって (1) の「V124 とは原因が別なので、1 つの修正では両方を閉じられない」と「触る関数が重ならない」は、V124 の推奨案に対しては誤りである。

`V124.patch` を `V22.patch` の後に当てると、15 の hunk のうち 1 つ(`@@ -4791`、1a の後始末のループに `T.DType t -> speculate_alias env t` を足す hunk)が衝突した。
これは D[V22-2] が書き換えるループそのものである。
`speculate_all` の中に同じ分岐を足して手で解くと、`dune runtest` の差分は無く(V124 の期待値が specunit2 と speclab をすでに通る形にしている)、V124 の `ex/` の全プログラムは `V124.patch` だけの写しと同じ出力になり、V22 の `ex/` は試作と同じ出力になった。
`test/kinds.t` の specunit2、speclab の期待値と前書き `:287-290`、`:310-312` は、両方の差分が書き換える。

V30 は P[V22-1] の代案で同じ関数 `is_effect_name` を触る。
V56 は `find_alias`(`:912` から)の可視性の検査で、D[V22-1] が `:917` を書き換える関数と同じである。
V57 は `elab_eff` の波括弧の中の `env.types` の判定(`:1223`)で、D[V22-1] が `:1228` を書き換える分岐の直前である。
V29 は投機が届かない形として残る、と下書きが書くとおりである。
