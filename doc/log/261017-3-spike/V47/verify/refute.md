# V47 の下書きの反証

## 裏取りの方法

HEAD の写し(snap/diktor)を verify/diktor に写し、V47.patch を当てようとした。
hunk 3(`EIdent` の分岐の置き換え)が当たらず、hunk 2 の `unknown_type` は HEAD に無い `absent_numeric` を参照する。
したがって差分は HEAD には当たらない(下書きの記述どおり)。
合成の木 all/diktor を verify/diktor-allbase(変更なし)と verify/diktor-all(V47.patch を当てたもの)に写し、両方で dune build と dune runtest を走らせた。
どちらの runtest も緑である(runtest.allbase.log、runtest.patched.log は空)。
新しい visibility.t を変更なしのコードで走らせた結果は runtest.crossed.log にある。

## 確かめた主張

- HEAD の行番号: `elab.ml:604`(未知の型: n)、`:568`(EApply の分岐の頭)、`:941`(unknown_effect)、`:1165`(elab_eff)、`:1180`、`:1188`、`:1239`(最後の分岐)、`decls.ml:491`(check_con_visible)は写しと一致する。
- `:530-542` は EIdent の分岐の後半で、分岐の頭は `:516` である。
- LangSpec §15.1 は `doc/LangSpec.md:1447-1481` で、§15.2 が 1483 から始まる。
- visrow4 と visrow5 は `test/visibility.t:198-205` にある。
- diktor の doc/log/260926-1-plan.md の 89、243-245、2716-2717、4408 行と、260926-2-m34.md の 164、339 行は、下書きの言うとおり V47 を記述している。
- 例の出力: ex と exb を HEAD、合成の木、試作で走らせ直した。HEAD と合成の木の出力は一致し、担当の results.head.txt、results.new.txt、results.b.head.txt、results.b.new.txt とも一致した。
- 文面が変わるのは a1、a2、a5、a7、a8、a9、a11、a12、a13、a17、a18、a20、b2、b3 の 14 例で、終了コードと位置は変わらない(実測)。
- 新しい cram を変更なしのコードで走らせると、vis4c、vis4e、visrow4b、visrow5b、visrow4c、visrow4d の 6 コマンドが落ち、vis4d と visrow4e は通る(実測)。
- 新しい cram は 2 ブロック、コマンド 16 行、プログラム 8 本である。
- 差分の行数は誤りである。見出しを除いた増加は 83 行で、76 は空行 7 行を数えていない(減少の 19 行は正しい)。
- 下書きの (5) の「§11.3 の前に unknown_type を置く」は誤りである。試作は §11.3 の記事(合成の木の 586 行)の後、`speculating`(720 行)と `elab_type`(738 行)の間に置いており、§11.3 の中である。
- 下書きの (1) と (5) が挙げるコメントの変更は、§11.3 と §11.6 の記事、最後の分岐のコメントの 3 つだけだが、試作には `unknown_type` の頭のコメント(5 行)と、`elab_eff` の新しい分岐の中のコメント(4 行)もある。
- litcheck(--old に合成の木の elab.ml)は marker 0 件で、code の不一致はコードを変えた位置だけである(実測)。litwrap の報告は既存の表の幅だけである。

## 退行の探索

次のプログラムを HEAD、合成の木、試作で走らせた(verify/ex)。
受理と出力が 3 つで一致しないものは無かった。
r1(前方参照のエイリアス `@ W[{}]`)、r2(コンパニオンの EffectRow エイリアス `module W { pub type W[E] }` を非修飾で `@ W[{}]`)、r3(module の中の非修飾の `W[E]`、`V[E] = W[E]`、`{Console extends W[{}]}`、外の `@ M.W[{}]`)、r4(`Cb[W[E]]` と `@ W[E]`)、r5(`@ Heap[h]` の略記と `run h`)、r6(import した module の `M.W[{}]` と import したエイリアス `W2[{}]`)、r8(利用者の effect を含むエイリアス、ガード、入れ子のハンドラ)、r9(トップレベルの effect `Log` と module の同名の EffectRow エイリアス `Log[E]`)、r12(module の pub let の型引数を渡す `W[E]`)、r10(REPL で宣言と使用を別の行に書く)、r11(--prelude で差し替えたプレリュードの `CW[{}]`)。
r7(型クラスのメソッドの `W[{}]`)は HEAD が「行カインドではない型パラメータです: E」で拒否し、合成の木と試作が受理する。
差は合成の木の別の項目によるもので、試作は合成の木と一致する。
拒否される境界の形(e1: 非 pub のコンパニオン、e4: 別の module の非 pub の W、e5 と e6: REPL で失敗した行の後の参照)も、文面のほかは一致する。
e5 と e6 では、失敗した行の候補は巻き戻され、案内は出ない(3 つとも)。

ソースから導いたこと: 新しい分岐の条件(env.types に無い、resolve_con の先が aliases にも con_kinds にも無い)は、HEAD の `elab_type` の `EApply` の分岐が `:604` に落ちる条件と同じ表を同じ順で引く。
違いは `check_con_visible` を通らないことだけで、`con_visibility` に項目を入れるのは平坦化の `claim_con`(newtype と型エイリアス)だけである。
その oid が aliases にも con_kinds にも無いまま `elab_eff` に届く経路は見つけていない。

## 仕様との関係

LangSpec に「未知の型」「未知のエフェクト」「修飾してください」の文面は無い(grep)。
§15.5 の終了状態、§18 の保存する性質、付録 A の文法に触れる変更は無い。
受理と拒否が変わらないので、仕様を変えないという判断は妥当である。

## 判断事項について

P[V47-1] と P[V47-2] の推奨に異論は無い。
補足が 2 つある。
1 つ目: 新しい分岐の条件は `elab_type` の名前の引き方(env.types、resolve_con、aliases、con_kinds)を写したもので、`elab_type` の `EApply` の分岐に検査が増えると(たとえば数値型の noimpl)、エフェクト位置ではその前に未知のエフェクトで落ちる。記事にこの対応を 1 文書くと保守しやすい。
2 つ目: P[V47-1] の代案への批判(修飾しても行にならない名前を案内する)は、型の位置にも同じ形で当てはまる。P[V47-2] の後、型の位置の `W[Console]`(pub の EffectRow エイリアス)にも「M.W と修飾してください」が付き(e3)、修飾するとカインドの誤りになる(e2)。これは引数の無い `x: W` で HEAD から出ている振る舞い(e7)と同じなので、退行ではない。

## ほかの項目との重なり

V50 は同じ `elab_eff` の `EApply` の分岐の条件(`not (SMap.mem n env.types)`)と §11.6 の記事を触りうる。
V50 が型パラメータとエイリアスの優先を変えると、V47 の新しい分岐の条件 `not (SMap.mem n env.types)` も合わせる必要がある。
V44、V48、V49、V53 とは関数も cram も重ならない(V53 の test/verify_fixes.t は V47 が触らない)。
V114(absent_numeric)が V47 より先に入らないと、差分は当たらない。
