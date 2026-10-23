# Diktor のエフェクトと巻き戻しの実装と、コンパイル方式の実装への要件

調査日 2026-10-24。対象は親 keleut bcb16f1、diktor 6e49bbb6bea2bfb161d6d2cdd1c21565c9edeb1a(`git rev-parse HEAD` で確認)。
tracked なファイルは変更していない。実験に使ったファイルは
`doc/log/261024-1-spike/probe/fx/` に置いた。

記号の約束。

- 【測定】: この調査でコマンドを実行して得た事実。出力は抜粋。
- 【コード】: ソースまたはテストの記述から読み取った事実。`file:line` を付ける。
- 【推論】: 上の事実から導いた判断。誤りうる。
- 【既知】: 先行事例についての筆者の知識。版や年は不確かなことがあり、この調査では確かめていない。

## 0. 要約

1. Diktor は Keleut のハンドラを OCaml 5 の `Effect.Deep.match_with` 1 つずつに写す(interp.ml:19-37、979)。`Effect.Shallow` は使っていない【測定】。
   仕様の §13.5 は「再開した計算の中でも同じハンドラが有効」と定めるので、深いハンドラが必須である。
2. `resume` はアフィンで second-class である。継続の寿命は操作節の起動の動的範囲を越えない(value.ml:186-228、§13.5)。
   したがってコピーできる継続(multi-shot)は要らず、ワンショットのスタック切り替えか、それに相当する仕組みで足りる【推論】。
3. 巻き戻しは、例外 `Unwind (inst, UValue v | UClause f)` を `discontinue` で捨てる継続へ送り込んで行う。
   途中のハンドラの `exnc` が cancel 節を走らせてから再送出するので、後始末の順序はスタックの入れ子の順序と一致する(interp.ml:643-662、990-1000)。
4. コンパイル方式で難しいのは、末尾でない `resume` だけである。
   末尾の `resume` しか持たない節と、`resume` しない節(再開しない操作の節を含む)は、継続を取り出さずに「その場で節を呼ぶ」「例外で巻き戻す」で実装できる【推論】。
5. Node 26.9.0(V8 14.6)では、WasmGC、`return_call`、exnref の `try_table` はフラグなしで動く。WasmFX(core stack switching)は `--experimental-wasm-wasmfx` が要る【測定】。
   JSPI はフラグなしで使えるが非同期であり、1 回の中断と再開に約 63 ns かかる(同期の呼び出しは約 4.8 ns)【測定】。
   Wasm の中では、スタックあふれもゼロ除算のトラップも `catch_all` で捕まえられない【測定】。
6. 推奨【推論】: IR に `handle`/`perform`/`resume` を高水準の命令として残し、節を静的に 3 種に分類する。
   (i) 末尾 resume だけの節と resume しない節は、その場で呼ぶ方式と例外で実装する。
   (ii) 末尾でない resume を持つ節だけ、継続を取り出す。
   (ii) の取り出しは、全バックエンドで共通に使える yield bubbling(Koka の generalized evidence passing と同系統)を基本とし、継続フレームを tier に依らない形にする。
   LLVM のネイティブ fiber は、測定で必要と分かったときに足す最適化とする。

## 1. 調べた範囲

- diktor/lib/interp.ml(第14章、とくに §14.9 ハンドラ 628-1105)
- diktor/lib/value.ml §12.4、§12.5(186-269)
- diktor/lib/builtin.ml §13.6 `with_runtime`(424-503)
- diktor/lib/driver.ml の終了コードと flush(180-224、922-930、1150-1201、1203-1300、1320-1390)
- テスト: never_ops.t、resume_guard.t、resume_sites.t、tail_calls.t、runtime_effects.t、effect_order.t、run_exit.t、eval.t(effects.kel 52 行目から sinkfail.kel 834 行目付近まで)
- 仕様: doc/LangSpec.md §5.7(476-)、§13.3〜§13.7(1312-1520)、§15.5(1762-)

`grep -rn "Shallow\|Effect\.\(perform\|Deep\)" lib/ bin/` の集計【測定】:

```
      5 lib/builtin.ml
     25 lib/interp.ml
      1 lib/syntax.ml   (コメント内の表)
      1 lib/value.ml    (resume 型の r_k)
```

`Shallow` は 0 件である。

## 2. 実装されている操作的意味論

以下の R1〜R10 は、コンパイル方式の実装が満たすべき要件として書く。各項目に、根拠のコードとそれを固定しているテストを付ける。

### 2.0 対応表(interp.ml:19-37 の要約)

| Keleut | Diktor(OCaml 5) |
|---|---|
| `e handle { … }` | `Effect.Deep.match_with` 1 つ(interp.ml:979) |
| `perform op(a)` | `Effect.perform (Op (oid, a))`(interp.ml:399-404、value.ml:261) |
| 末尾でない `resume(v)` | `Effect.Deep.continue r.r_k v`(interp.ml:406-412) |
| resume せずに節を抜ける | `discontinue k (Unwind (inst, UValue v))` |
| 再開しない操作の節 | 本体を評価する前に `discontinue k (Unwind (inst, UClause f))`、`exnc` で `f ()` |
| `case return(x)` | `retc`(親のスタックで走る) |
| `case cancel` | `exnc` の中の `run_cancel` |
| 最内一致 | `effc` が `None` を返せば外側へ |

### R1 ワンショット、アフィン、second-class

【コード】

- `resume` は値ではない。値の型 `t` に継続の構成子は無く、継続は環境のフィールド `env.resume : resume option` で運ぶ(value.ml:186-228)。
- `resume = { r_k; mutable r_used; mutable r_alive }`(value.ml:224-228)。
- `take_resume`(interp.ml:881-892)は、引数を先に評価してから `r_alive`(second-class)と `r_used`(アフィン)を検査し、`r_used <- true` にする。
  引数を先に評価するので、引数の評価中の例外では継続は未消費のまま例外の経路に乗る。
- 静的な検査は elab にある。ラムダや関数束縛の本体、ガード、return 節、cancel 節、再開しない操作の節の `resume` を拒否する(resume_sites.t、resume_guard.t、never_ops.t:202-226)。
- 2 回目の再開は「実行時エラー: resume は高々1回しか呼べません(アフィン)」で終了コード 3 になる(eval.t の affine.kel、resume_guard.t の matchguard2.kel)。
- OCaml の `Continuation_already_resumed` は driver の防御の節でだけ受ける(driver.ml:1365-1367)。

【推論】継続は節の外へ持ち出せない。そのため、節の起動が終わった時点で、継続は「消費済み(再開して戻ってきた)」か「破棄済み」のどちらかに確定する。
継続の寿命は LIFO に入れ子になる。継続をヒープに長く保持する必要も、コピーする必要もない。
ただし、操作節のガードの中の `resume` は禁止だが、操作節の本体の中の `match` のガードと `if` の条件にある `resume` は許される(resume_guard.t:1-27)。
そのため、1 本の実行経路で 2 回目の `resume` に到達することを静的には防げず、動的なフラグは要る。

【測定】入れ子の resume の振る舞いは、経路によって観測できる差がある(interp.ml:787-793 に記述あり)。

```
== nest_short2   (case ask(n) => resume(resume(n)))       … 節本体が resume そのものの短い経路
実行時エラー: resume は高々1回しか呼べません(アフィン)
== nest_spine    (case ask(n) => { let t = 0; resume(resume(n) + t) })  … 背骨をたどる経路
cont saw 1
実行時エラー: resume は高々1回しか呼べません(アフィン)
```

短い経路は、外側の `resume` の `r_used` を引数の評価より先に `true` にする(interp.ml:1064)。そのため、内側の resume が継続を再開する前に落ちる。
【推論】§13.5 は「2 回目の再開は実行時エラー」としか書いていない。評価順序(引数が先)に素直に従うのは背骨の経路のほうである。
Diktor と RustKeleut を差分試験するなら、この差を既知の差として扱うか、どちらかにそろえる必要がある。

### R2 継続の破棄と cancel の順序

【コード】interp.ml:643-662、990-1000、1077-1100。

1. resume せずに節が値 `v` を返すと、`discontinue k (Unwind (inst, UValue v))` を呼ぶ。
2. 例外は捨てる継続の中の perform の位置から送出され、内側の `match_with` の `exnc` を順に通る。
   自分宛てでない `exnc` は `run_cancel ()` を呼んでから `raise ex` する(interp.ml:997-1000)。
3. 自分の `exnc` は `Unwind (id, UValue v) when id = inst` を受け取り、`v` が handle 式の値になる(interp.ml:995)。

ハンドラが自分で継続を捨てたとき、そのハンドラ自身の return 節と cancel 節は走らない(interp.ml:680-686)。
テスト: eval.t の effects.kel(`cancel inner`、`cancel outer`、`err abort` の順)、never_ops.t の order.kel。

【推論】この順序は、perform の位置から handle の位置まで、スタックを上から下へたどる順序と同じである。
fiber の入れ子を使わなくても、普通の 1 本のスタックの例外の巻き戻しで同じ順序が得られる。

### R3 節の 3 つの経路と、ハンドラ自身の cancel 節

【コード】interp.ml:664-701、表は interp.ml:669-673。

| 節の終わり方 | 処理 | 自分の cancel |
|---|---|---|
| resume した後に値 `v` | `v` を返す | 走らない |
| resume せずに値 `v` | `discontinue k (Unwind (inst, UValue v))` | 走らない(自分宛ての Unwind) |
| 例外で脱出、resume 前 | `discontinue k ex` | 走る(ex は自分宛てでないので exnc が run_cancel) |
| 例外で脱出、resume 後 | `raise ex` | 走らない |

節の選択(パターン照合とガード)の例外も、`discontinue k ex` に乗せる(interp.ml:1036-1037)。
どの節にも一致しないときも、素の raise ではなく `discontinue` にする(interp.ml:1038-1043、到達しない防御の分岐)。

テスト(eval.t):

- clauseerr: `ask clause`、`cancel res`、`cancel ask`、ゼロ除算。resume 前の脱出では、捨てる継続の中の cancel の後に自分の cancel が走る。
- clauseresumed: resume 後の脱出なので `cancel ask` は出ない。
- clausestop: 節の本体から perform した操作を外側が resume しないと、`stop: no resume` の後に `cancel ask` が走る。
- tailresume: `resume(???)` の短い経路でも、`cancel r1`、`cancel outer` が走る。
- holecancel: 節の本体が `???` でも内側の cancel が走る。
- resumeinner / resumeinnerdeep / resumeinnerpre / resumeret / resumestop / resumeguard(eval.t:392-500): 再開した計算の中で同じハンドラの節を評価中に起きた誤り。

【推論】resumeinner* の結果は「誤りが起きた瞬間に、そのハンドラが動的なハンドラの連鎖に入っているか」と「評価中の節が未消費の継続を持つか」の 2 点で決まる。

- resumeinnerdeep: 2 つの節がどちらも resume の中にいる間に、ハンドラの本体で 10 / 0 になる。fiber は動いており、ハンドラは連鎖に入っているので、cancel は 1 回走る。
- resumeinner: ask(0) の節が resume から戻った後に v / n で失敗する。ask(0) の resume が戻った時点で本体は retc まで終わっている。ハンドラはもう連鎖にいないので、cancel は走らない。
- resumeinnerpre: ask(0) の節が resume の前に失敗する。継続は未消費なので、discontinue によって cancel が 1 回走る。

§13.6 の長い段落(LangSpec.md:1430-1440)は、この動的な構造を言葉で書いたものである。
コンパイル方式の実装も、ハンドラの活性化の連鎖と、節の起動ごとの「継続が生きているか」を同じ構造で持てば、同じ結果になる。

### R4 再開しない操作(返り値の型が Never)の節

【コード】interp.ml:702-751、1045-1052、996。

- 節を選んだ後(ガードの評価は破棄の前。never_ops.t の guard.kel では `guard other` が `cancel res` より先に出る)、本体の閉包 `f` を作る。
- `discontinue k (Unwind (inst, UClause f))` を `effc` の末尾で呼ぶ。
- 捨てる継続の中の cancel 節が内側から順に走った後、自分の `exnc` が `f ()` を末尾で呼ぶ。その値が handle 式の値になる。
- 本体の誤りと、本体からの perform による外側の破棄は、ハンドラ式の後ろで起きたものとして扱う。自分の return 節と cancel 節は走らない(never_ops.t の clauseerror、rethrow、otherescape、askescape)。
- ガードの誤りと、ガードからの perform による外側の破棄では、継続はまだ生きているので、自分の cancel 節が走る(never_ops.t の guardescape、guarderror)。

末尾位置について(§5.7)。ハンドラ式が末尾位置にあれば、再開しない操作の節の本体も末尾位置にある。
tail_calls.t の retry.kel は、`l=100k` の下で 10 万回の再試行を完走する。
同じ形を再開する操作で書いた retryresume.kel は、1000 回なら通り、10 万回ではスタックあふれで落ちる。

### R5 cancel 節の実行文脈と、脱出の抑制

【コード】interp.ml:826-846、964-977。

- `run_cancel` は `exnc` の中、つまり fiber を巻き戻した後の親のスタックで走る。cancel 節の中の perform は自分の `effc` に届かず、外側のハンドラに届く(eval.t の cancelouter.kel)。
- cancel 節の中の例外はすべて捕まえて `cancel_log` に渡し、外へは出さない(interp.ml:969-974)。
  対象は `Runtime_error`、`Unwind`、`Sys_error`、その他すべてである。
  driver は、stdout を flush してから「cancel 節で例外が抑制されました: …」を stderr に書き、stderr を flush する(driver.ml:1192-1197)。
- 抑制した後は、元の例外の巻き戻しを続ける(`raise ex`、interp.ml:1000)。

この規則から、次の 5 つの込み入った振る舞いが生じる。どれも never_ops.t が固定している。

| テスト | 何が起きるか |
|---|---|
| cancelperform(never_ops.t:534-552) | 実行時エラーの巻き戻し中に cancel が再開しない操作を起こす。その破棄は cancel の打ち切りで止まり、`caught` は出ない。元のゼロ除算で終了コード 3 |
| cancelnormal(553-578) | 内側の stop による破棄の途中で、cancel が throw を起こす。throw のハンドラの節は走らず、stop の節の値 2 で本体が続く |
| cancelsame(583-603) | 破棄を進めている当のハンドラ H に、cancel が throw を送る。2 つ目の破棄は打ち切られ、1 つ目の破棄が続き、`H clause first` が走る |
| cancelresume(609-626) | cancel が H の再開する操作 ask を起こし、ask の節が末尾でない resume(5) を呼ぶ。再開した計算が 1 つ目の破棄の続きで終わり、throw の節の値 7 が resume(5) の戻り値になる(`ask clause got 7`、1007) |
| cancelresumevia / cancelresumetwo(632-688) | 上の組み合わせを 2 段にしたもの。各ハンドラの節の本体は 1 回だけ走る |

【推論】cancelresume は、コンパイル方式で最も壊しやすい場合である。
破棄の途中(巻き戻しの途中)で cancel 節が末尾でない resume の節を呼ぶので、継続の中に「進行中の巻き戻し」が含まれる。
また、破棄を進めているハンドラ H は、破棄が終わるまで有効なままでなければならない。cancel 節からの perform が H に届くからである。

### R6 末尾 resume の空間

【コード】interp.ml:753-814、897-947、1056-1100。

- 節本体の末尾の背骨(`Seq` の最後、`Let`/`LetRec` の続き、`Match` の節本体、`&&`/`||` の右辺、`Run` の本体)にある `Resume` は、`match … with exception` の包みの中では継続を再開しない。
  `eval_clause_tail` が `Clause_resume v` を返し、`effc` が包みを抜けた値の枝で `continue k v` を末尾で発行する(interp.ml:1090-1092)。
- 背骨は `Handle` と `Lambda` に入らない。
- `spine_has_resume` は、ハンドラを起動するたびに各節の背骨を 1 回だけ調べる(interp.ml:952-962 の `Resumable tail`)。判定は節の単位である。
- 節本体が `Resume` そのものの節は、さらに短い経路を通る(interp.ml:1056-1076)。
- `eval_handle` は `match_with` を末尾で呼び、retc の再帰も空間を増やさない(tail_calls.t の retc.kel)。

【測定】(`OCAMLRUNPARAM` は既定値。fx/perfloop.kel と fx/perfnt.kel は 1 から n の perform をループで起こす)

```
tail    : case tick(x) => resume(1)                  n=1000000  0.56 s
nontail : case tick(x) => { let v = resume(1); v }  n=1000000  12.57 s
nt n=125000 0.41s maxrss=74MB    t n=125000 0.07s maxrss=11MB
nt n=250000 1.18s maxrss=140MB   t n=250000 0.14s maxrss=11MB
nt n=500000 3.73s maxrss=271MB   t n=500000 0.28s maxrss=11MB
```

末尾 resume は時間が線形で、メモリは一定である。末尾でない resume は、メモリが線形(1 回あたり約 540 B)で、時間が超線形(倍にすると約 3 倍)である。
【推論】超線形になるのは、親のスタックに積み上がった節の枠を、マイナー GC のたびに走査し直すためと思われる。確かめてはいない。
仕様は末尾でない resume の空間を保証していない(§5.7、tail_calls.t の nontail_resume.kel)。

### R7 実行時エラーの巻き戻し

【コード】

- 実行時エラーは OCaml の例外 `Runtime_error` である(value.ml:269)。`Stack_overflow` と `Out_of_memory` も同じく `exnc` を通る。
  途中のハンドラの cancel 節を内側から外側へ走らせ、driver の受け皿で終了コード 3 になる(driver.ml:1346-1380)。
- §15.5 は「実行時エラーが起きると、そのとき有効なハンドラを内側から外側の順に巻き戻して cancel 節を実行する」と定める。評価に要する領域が処理系の限度を超えた場合も実行時エラーに含める。
- 対話的な実行では、実行時エラーの後も実行が続く(§15.5 の最後の段落)。

【測定】(fx/so.kel、fx/errcancel.kel)

```
$ OCAMLRUNPARAM=l=100k diktor so.kel      # handle の本体で 100 万段の非末尾再帰
start
cancel ran
実行時エラー: スタックオーバーフロー(再帰が深すぎます)
exit 3
$ diktor errcancel.kel                    # 2 段のハンドラ、内側の cancel が __panic
cancel A
cancel 節で例外が抑制されました: panic: in cancel A
cancel B
実行時エラー: ゼロ除算です
exit 3
```

スタックあふれでも cancel 節は走る。
【推論】コンパイル方式で同じことをするには、スタックあふれを「捕まえられる実行時エラー」にしなければならない。そのうえで、cancel 節を走らせる余地のスタックが要る。

### R8 ハンドラの活性化ごとの同一性

【コード】`inst = new_oid ()` を handle の評価ごとに採番する(interp.ml:952)。AST のノード id を使うと、入れ子に起動した同じ handle 式(再帰、`with_file` の 2 回呼び出し)が、外側宛ての Unwind を飲み込む(interp.ml:633-641)。

【推論】コンパイル方式でも、巻き戻しの宛先は活性化ごとの印(prompt、marker)にする。
スタックをコピーしたり移動したりする方式では、フレームのアドレスは印として安定しない。単調増加のカウンタか、ヒープ上のハンドラ記録への参照を使う。

### R9 Console と出力の flush

【コード】

- `Console` は利用者が handle できない(runtime_effects.t:6-26)。`Console.write` を処理するのは、最も外側の `with_runtime` だけである(builtin.ml:485-503)。
- その節は `sink` に書いて `continue k ()` する、末尾 resume の形である。`sink` が例外を出したら、`discontinue k ex` で捨てる継続に届ける(builtin.ml:498-501)。
- eval.t の sinkfail.kel は、stdout を閉じた状態で 10 万行を書く。cancel 節の `__panic` が抑制ログに出て、「diktor: 標準出力に書き出せません: Bad file descriptor」で終了コード 74 になる。
- stdout はバッファ付きである。stderr へ書く直前には必ず stdout を flush する(driver.ml:224、1195-1196)。終了時は stdout、stderr の順に flush する(driver.ml:209-218)。
  cram が 2 本の出力を 1 本に併合しても、診断がプログラムの出力の後ろに出る(§15.5、run_exit.t の exitcancel)。
- プレリュードの `with_stdout` は `Print.print` を `Console.write` に翻訳する、末尾 resume の節である(eval.t の perf.kel、tail_calls.t の stdout.kel)。

【推論】`Console.write` と `with_stdout` は継続の取り出しを必要としない。ランタイムの直接の呼び出しで済む。
書き込みの失敗は、ランタイムの実行時エラーとして巻き戻しに乗せ、終了コード 74 に対応付ける。

### R10 その他

- 操作節はソースの順に試す。パターンが外れてもガードが偽でも次の節へ進む。ガードは resume の無い環境で評価する(interp.ml:850-858、1013-1031)。
- 外側への再送(どの節にも一致しない操作を外へ流すこと)は、仕様にも実装にも無い(interp.ml:862-874)。
- `Effect.Unhandled` は、型検査を通ったプログラムでは起きない防御の分岐である(driver.ml:1358-1364)。
- 1 つの handle は 1 つのエフェクトを処理する(§13.4)。エフェクト行は多相である(`body: () => A @ {Res, Console extends E}`)。
  そのため、ハンドラの探索はエフェクト行の型だけでは静的に決まらない【推論】。

## 3. OCaml 5 のエフェクトハンドラへの依存

Diktor が依存している性質は次のとおりである【コード+推論】。

1. 深いハンドラ: `continue` した継続の中でも同じハンドラが有効である(§13.5 の要求)。浅いハンドラでは、再開のたびにハンドラを付け直す必要があり、仕様の意味と合わない。
2. `effc` の節は fiber の外、親のスタックで走る。そのため、節の中の perform は自分を飛ばして外側へ届く(§13.4 の最後の文)。
3. `exnc` と `retc` も親のスタックで走る。cancel 節と return 節は外側の行で評価される(§13.4)。
4. `discontinue k ex`: 中断した perform の位置に例外を送り込み、その fiber の中の `exnc` を内側から順に通す。R2 から R4 の順序はこれで決まる。
5. ワンショット: 2 度目の `continue` は OCaml が拒否するが、Diktor はその前に自前で検査する。
6. 末尾位置の `continue`/`discontinue`/`match_with` は OCaml の末尾呼び出しになり、空間を使わない(R6)。
7. fiber のスタックはヒープ上で伸びる。そのため、深い再帰は遅れて `Stack_overflow` や `Out_of_memory` になる(interp.ml:1459-1468)。
8. `Stack_overflow` は捕まえられる例外で、`exnc` を通る(R7)。

コンパイル方式で再現しなければならないのは 1〜4、6、8 である。5 は自前のフラグで足りる。7 は Diktor の実装の事情で、要件ではない。

## 4. 実行環境の測定(Node 26 の WebAssembly)

【測定】`node -p process.versions.v8` → `14.6.202.34-node.32`。

`node --v8-options` の抜粋:

```
--experimental-wasm-wasmfx (enable core stack switching for Wasm (experimental))  default: --no-experimental-wasm-wasmfx
--experimental-wasm-growable-stacks (enable growable stacks for jspi ...)           default: --no-...
--experimental-wasm-exnref (enable exnref for Wasm)                                  default: --experimental-wasm-exnref
--experimental-wasm-legacy-eh                                                         default: --experimental-wasm-legacy-eh
--wasm-stack-switching-stack-size ... default 984 (kB)
```

手で組み立てた最小のモジュール(fx/wasmprobe.js、fx/jspi.js、fx/sofx.js)の結果:

```
{ "tail_call": 50000000,            # return_call で 5000 万回の自己末尾呼び出しが完走
  "gc_struct": 42,                  # struct.new / struct.get
  "exnref_try_table": 7,            # tag + throw + try_table(catch)
  "wasmfx_cont_type_validates": false,
  "wasmfx_error": "WebAssembly.Module(): core stack switching not enabled (enable with --experimental-wasm-wasmfx) @+14",
  "jspi": { "Suspending": "function", "promising": "function" } }
--experimental-wasm-wasmfx を付けると  "wasmfx_cont_type_validates": true
sync result 1000000 ns/switch 4.80     # import 呼び出し 100 万回
jspi result 1000000 ns/switch 62.57    # Suspending import による中断と再開 100 万回
h  escaped to JS: RangeError Maximum call stack size exceeded   # try_table(catch_all) の中の無限再帰
dz escaped to JS: RuntimeError divide by zero                  # try_table(catch_all) の中の i32.div_s 0
```

ここから言えること:

- WasmGC、末尾呼び出し、例外処理(exnref)は、フラグなしで使える。
- WasmFX は Node 26 ではフラグなしでは使えない。「フラグなし」という制約の下では、選択肢 (c) は除外される。
- JSPI は使えるが、promise と microtask を経由するので非同期である。中断した Wasm のスタックを Wasm の側から同期的に再開する手段は無い【既知】。
  【推論】ハンドラの節(親)と本体(子)の間を同期的に行き来する用途には合わない。
- Wasm の中では、スタックあふれとトラップを捕まえられない。
  【推論】R7 を WasmGC で満たすには、次の 2 つが要る。
  (1) ゼロ除算などは、トラップになる命令の前に明示的に検査し、Keleut の実行時エラーの tag を throw する。
  (2) スタックあふれは、深さのカウンタで先に検出して throw するか、JS の側で RangeError を受けてから、ヒープ上に持つ cancel の連鎖を JS から走らせる。

## 5. コンパイル方式への要件(第 2 節からの導出)

| 番号 | 要件 | 固定しているテスト |
|---|---|---|
| Q1 | ワンショットのアフィン検査(節の起動ごとのフラグ)。継続は節の外へ出ない | affine、matchguard2 |
| Q2 | 破棄は内側から外側へ cancel を走らせる。自分で捨てたハンドラの return と cancel は走らせない | effects、order |
| Q3 | 節の 3 経路と、resume の前か後かによる自分の cancel の有無 | clauseerr、clauseresumed、clausestop、tailresume、resumeinner* |
| Q4 | Never 操作: ガード → 破棄 → ハンドラの外で本体を末尾位置で評価 | order、guard、guardescape、retry |
| Q5 | cancel 節: 外側の文脈で走り、すべての脱出(破棄も)を打ち切ってログに出し、元の巻き戻しを続ける。破棄中のハンドラは有効なまま | cancelperform、cancelnormal、cancelsame、cancelresume、cancelresumevia、cancelresumetwo |
| Q6 | 末尾 resume と、Never 節の本体、return 節の本体が一定空間 | tail_calls.t 全体(retrycancel* を含む) |
| Q7 | 真正末尾呼び出し(§5.7)。高階関数、辞書渡し、相互再帰を含む | tail_calls.t の self、mutual、hof、method、dict |
| Q8 | 実行時エラー(スタックあふれを含む)で cancel を内側から実行し、終了コード 3。REPL では続行 | run_exit.t の exitcancel、so.kel(この調査) |
| Q9 | 活性化ごとの巻き戻しの宛先 | (with_file の入れ子、interp.ml:633-641) |
| Q10 | Console: 末尾 resume の直接呼び出し、書き込み失敗は巻き戻して 74、stdout を flush してから stderr | sinkfail、exitcancel、perf |

## 6. 選択肢の評価

### 6.0 前提にする節の静的な分類【推論】

Diktor の `spine_has_resume` と `RNeverOp` の印から、各操作節は静的に次の 3 種に分けられる。
IR を作る段階で分類を確定できる。

- **N(never)**: 再開しない操作の節。resume を持てない。
- **T(tail)**: resume が背骨の末尾にしか無い節。resume が無い枝を含んでもよい(その枝は破棄になる)。
  例: `case use(s) => resume(echoln(...))`、`case test() => { let t = true; t && resume(true) }`
- **G(general)**: 背骨の外に resume がある節。例: `case print(m) => { let {value, output} = resume(); ... }`(effects.kel の capture)、`if resume(true) then …`(resume_guard.t の ifcond.kel)。

N と T の節は、継続を取り出さずに実装できる。

- T の節: perform の位置で、ハンドラの外側の文脈に切り替えてから節を普通の関数として呼ぶ。末尾の resume は、節が perform の位置へ値を返すことで実現する(節の枠はそこで消えるので Q6)。
  resume しない枝は、節の値を持ってハンドラの枠まで巻き戻す(Q2)。
  節の中で起きた例外は、まず節の中のハンドラ、次に perform の位置からハンドラまでの間のハンドラ、次に自分(継続は未消費なので cancel を走らせる)、次に外側の順に巻き戻る。
  これは §13.6 が定める順序と同じである(Q3)。
- N の節: ガードを perform の位置で評価してから(Q4)、宛先が自分の活性化である例外を投げる。ハンドラの枠で受けてから、本体を末尾呼び出しで評価する。

G の節があるハンドラだけ、継続の取り出しが要る。選択肢 (a)〜(d) が違いを生むのは、この部分である。
tail_calls.t が一定空間を要求する場合は、すべて T か N の節である。同じファイルにある G の節(nontail_resume.kel の `{ let u = resume(); u }`、retryresumepath.kel の `case ask() => { let x = resume(1); x }`)は、どちらも `l=100k` で落ちることを期待している。G は仕様が空間を保証しない場合にあたるので、この分類は仕様の保証の境界と一致する。プレリュードの `with_stdout` と eval.t の perf.kel も T である(interp.ml:787)。

### 6.1 (a) エフェクトを起こす関数の選択的 CPS とヒープ上の継続

方式: 型(エフェクト行)を見て、G のハンドラに届きうる関数を CPS に変換する。継続はヒープ上の閉包にする。

| 観点 | 評価 |
|---|---|
| ワンショット | 閉包にフラグを持たせれば足りる |
| cancel の順序 | ハンドラの枠を継続の連鎖の中の印として残せば、破棄のときに連鎖を内側からたどって cancel を走らせられる。Q5(破棄の途中の G 節)には「巻き戻しの途中」という状態を継続に含める必要があり、実装は込み入る |
| 末尾 resume の空間 | 継続の呼び出しが真の末尾呼び出しであれば一定。Wasm の return_call、LLVM の musttail で保証できる |
| 真正末尾呼び出し | CPS の呼び出しはすべて末尾呼び出しになるので相性はよい。ただし直接形式の関数と CPS の関数の境目で包みが要る |
| interp と JIT の相互呼び出し | 両 tier が同じ CPS の規約を共有しなければならない。多相な行(`E`)の関数は、両方の版を作るか、常に CPS にする必要がある |
| WasmGC / Node 26 | フラグ不要。閉包は GC の struct で表せる |

短所【推論】: エフェクト多相の高階関数(map など)がほぼすべて CPS 側に入る。速度と生成コードの量の両方で不利になる。

### 6.2 (b) ネイティブのスタック切り替え(fiber)

方式: G のハンドラの本体を、別のスタック(mmap で確保しガードページを付けたもの、分割スタック、またはコピーで伸ばすスタック)で走らせる。OCaml 5 と同じ構造になる。

| 観点 | 評価 |
|---|---|
| ワンショット | 一致する。R1 により寿命は LIFO なので、スタックのコピーは要らない |
| cancel の順序 | fiber の入れ子をそのままたどるので一致する。Diktor と同じ構造なので、Q5 の 5 つのテストも同じ論理で通るはず |
| 末尾 resume の空間 | 切り替えを末尾で行えば一定。T の節では fiber 自体が不要 |
| 真正末尾呼び出し | fiber とは独立。LLVM の musttail と、呼び出し規約(tailcc など)をそろえる必要がある |
| interp と JIT の相互呼び出し | 強み。fiber は、上に積まれているのが interp の枠か JIT の枠かを区別しない。ただし、interp の値スタックとフレームの配列を、fiber ごとに持つ(またはネイティブスタック上に置く)必要がある |
| WasmGC / Node 26 | 使えない(WasmFX が要る)。Wasm のバックエンドには別の方式が要る |

短所【推論】:

- GC のルートの走査が、すべての fiber のスタックに及ぶ。LLVM の statepoint で正確な走査をするか、保守的に走査するか、shadow stack を使うかの選択が、fiber の数だけ効いてくる。
- スタックあふれを捕まえられる実行時エラーにするには(Q8)、プロローグでのスタック上限の検査(V8 や Go と同様)か、ガードページとシグナルハンドラ、それに cancel を走らせる予備の領域が要る。
- extern C の呼び出しは、小さい fiber のスタックで走らせないほうがよい。OCaml 5 は C の呼び出しの間、システムのスタックに切り替える【既知】。
- 実装の Rust crate の候補【既知】: corosensei(wasmer が使用)、wasmtime-fiber、context(Boost.Context の移植)。

### 6.3 (c) WasmFX(stack switching 提案)を V8 で使う

| 観点 | 評価 |
|---|---|
| ワンショット | 提案の継続はワンショットであり、一致する【既知】 |
| cancel の順序 | `resume` のハンドラ表と、`resume_throw`(継続に例外を送り込む命令)で、Diktor の discontinue と同じ構造を作れる【既知】 |
| 末尾 resume の空間 | 継続の再開を末尾で行えるかは、提案の細部と V8 の実装に依存する。未確認 |
| 真正末尾呼び出し | return_call は使える【測定】 |
| interp と JIT の相互呼び出し | Wasm の中で完結するので、Wasm 版の中では問題にならない |
| WasmGC / Node 26 | **不適合**。`--experimental-wasm-wasmfx` が要る【測定】 |

結論: 「フラグなしの Node 26」という制約がある当面は、採れない。将来の最適化の経路として、IR の設計を (b) や (c) に写せる形にしておく価値はある【推論】。

### 6.4 (d) 例外と CPS の混成

方式: 6.0 の分類で、N と T の節は「その場で呼ぶ」と「例外」で実装し、G の節だけを CPS(または yield bubbling)にする。

Koka の generalized evidence passing の形にすると、G の perform は「yield 中」の印を立てて普通に戻る。
呼び出し元はそれぞれ、呼び出しの後に印を検査し、自分の残りの計算を継続のフレームとしてヒープに積んでから戻る(遅延して継続を取り出す)。
目的のハンドラの枠に着いたら、積まれたフレームの列が継続になる【既知】。

| 観点 | 評価 |
|---|---|
| ワンショット | フラグで足りる |
| cancel の順序 | N と T の破棄は、ネイティブの例外(LLVM なら Itanium の巻き戻しと自前の personality、Wasm なら exnref、interp なら Rust の Result の伝播)で、スタックの順序どおりになる。G の破棄は、積まれたフレームの列を内側からたどり、ハンドラの枠で cancel を走らせる |
| Q5 | 破棄の途中に cancel 節が G 節を呼ぶと、巻き戻しの途中で yield が起きる。landing pad の側で「cancel を終えたら元の例外を再送出する」フレームを積んでから戻る規約にすれば表現できる。ただし、最も込み入る部分であり、never_ops.t の 5 つのテストを最初から回帰テストに入れる必要がある |
| 末尾 resume の空間 | T の節は呼び出しと戻りだけなので一定。G の継続の再開も、フレームの再入を末尾呼び出しにすれば一定 |
| 真正末尾呼び出し | 末尾呼び出しの後には印の検査が要らない(残りの計算が無い)ので、musttail と return_call を保てる |
| interp と JIT の相互呼び出し | 継続のフレームを「IR の関数 id、再開点の番号、生きている値」という tier に依らない形にすれば、JIT が積んだフレームを interp が再開することも、その逆もできる。継続の取り出しが、そのまま脱最適化(deopt)の点になる |
| WasmGC / Node 26 | フラグ不要。印は多値の戻り値かグローバル変数、フレームは GC の struct で表せる |

短所【推論】:

- エフェクトを起こしうる非末尾呼び出しのたびに、分岐 1 つの費用がかかる。行が閉じていて、利用者が handle できるエフェクトを含まない関数(空の行、`Console`、`Heap`、`Blocking` だけの行)では、検査を省ける。
- 再開点ごとにフレームの型と再入の入口ができるので、コードは大きくなる。

### 6.5 比較のまとめ【推論】

| 要件 | (a) 選択的 CPS | (b) native fiber | (c) WasmFX | (d) 混成 |
|---|---|---|---|---|
| Q1 ワンショット | ○ | ○ | ○ | ○ |
| Q2-Q4 順序 | ○(印の連鎖) | ◎(Diktor と同型) | ◎(同型) | ○(N/T は例外、G は連鎖) |
| Q5 cancel 中の G | △ | ◎ | ◎ | △(規約が要る) |
| Q6 末尾 resume | ○(末尾呼び出しが前提) | ○ | 未確認 | ◎(T は呼び出しだけ) |
| Q7 末尾呼び出し | ○ | ○(規約をそろえる) | ○ | ○ |
| Q8 実行時エラー | ○ | △(スタック検査が要る) | △(トラップは捕まえられない) | ○ |
| tier 間の相互呼び出し | △(規約の共有) | ◎(ただし GC 走査が重い) | — | ○(フレームを tier に依らない形に) |
| Node 26 でフラグなし | ○ | ×(Wasm では不可) | × | ○ |
| 全バックエンドで共通 | ○ | × | × | ○ |

## 7. 先行事例【既知】

- **OCaml 5**(Sivaramakrishnan ほか、PLDI 2021 "Retrofitting Effect Handlers onto OCaml"): ヒープに確保し、倍にしてコピーで伸ばす fiber。ワンショット、深いハンドラ。C の呼び出しはシステムのスタックで行う。Diktor はこの上に乗っている。
- **Koka**: "Effect Handlers, Evidently"(ICFP 2020)の evidence passing は、ハンドラの証拠をベクトルで渡し、tail-resumptive な操作をその場の呼び出しにする。
  "Generalized Evidence Passing for Effect Handlers"(ICFP 2021)は、multi-prompt と yield bubbling で C へ落とす。印の検査を 1 つの分岐にする。6.4 の手本にあたる。
- **libmprompt / libmpeff**(Leijen、2021 ごろ): C の multi-prompt 区切り継続。仮想メモリで伸びる gstack とスタックの切り替えを使う。6.2 の手本にあたる。
- **Effekt**: capability passing による字句的なハンドラ。JS へは CPS(またはモナド的な変換)で、LLVM 向けには自前の分割スタックを持つ実装がある、と記憶している。論文名と版は確かめていない。
- **Lexa**(OOPSLA 2024 ごろ "Lexical Effect Handlers, Directly"): 字句的なハンドラを、LLVM 上でスタックの切り替えにより直接実装する。末尾 resume の扱いの参考になる。
- **WasmFX**(Phipps-Costin ほか、OOPSLA 2023 "Continuing WebAssembly with Effect Handlers"): stack switching 提案の元。wasmtime に実験的な実装がある。V8 はフラグの下にある(第 4 節の測定)。
- **wasm_of_ocaml / js_of_ocaml**: OCaml 5 のエフェクトを、JSPI を使う方式と CPS 変換の方式で選べる、と記憶している。WasmGC の上で OCaml のエフェクトを扱う直接の先例である。
- **Swift の async**(LLVM の coro async lowering と swifttailcc): 関数を中断点で分割し、ヒープの文脈と musttail でつなぐ。LLVM で選択的 CPS を行うときの部品として使える。

## 8. 計画への提案【推論】

1. **IR の命令**: `handle`(節の表、return 節、cancel 節を持つ)、`perform`、`resume`、`resume_tail` を高水準の命令として持つ。下げ方はバックエンドごとに決める。
   各操作節に分類 N/T/G を、各 handle に「G の節を含むか」を、IR の属性として持たせる。分類は elab の後に 1 回だけ計算する(Diktor は起動ごとに `spine_has_resume` を計算している)。
2. **意味の基準**: 第 5 節の Q1〜Q10 と、それを固定する cram テスト(never_ops.t、tail_calls.t、eval.t の effects 系、run_exit.t)を、IR とバイナリ表現の適合試験として共有する。
   Diktor が IR を解釈できるようになったら、同じ IR で Diktor と RustKeleut を差分試験する。
3. **段階**:
   - 段 1: bytecode interp。継続は VM のフレーム列の切り出しで表す。R1 により LIFO なので安い。
   - 段 2: WasmGC。N と T は直接呼び出しと exnref、G は yield bubbling。実行時エラーは明示的な検査と tag で表す(トラップは捕まえられない)。スタックあふれは深さのカウンタで検出する。
   - 段 3: LLVM ORCv2。interp と同じフレームの形で yield bubbling を行う。継続の取り出しを、JIT の枠から interp の枠への脱最適化の点として使う。ネイティブ fiber は、測定で bubbling の費用が問題になったときに追加する。
4. **既知の差の扱い**: 入れ子の `resume(resume(x))` の観測の差(R1 の測定)を、IR の仕様でどちらかに決める。推奨は、評価順序どおりの背骨の経路の振る舞いである。
5. **Console**: ランタイムの組み込みの、末尾 resume のハンドラとする。stdout はバッファし、stderr へ書く前と終了時に flush する。書き込みの失敗は巻き戻して終了コード 74 にする。

## 9. 確認事項と推奨案【推論】

| 確認事項 | 推奨案 | 理由 |
|---|---|---|
| G 節の継続を、全バックエンドで同じ方式にするか | yield bubbling で統一し、LLVM の fiber は後で足す最適化にする | WasmFX が使えず、JSPI は非同期。interp と JIT の間でフレームを移せる |
| 実行時エラーの伝播 | LLVM はネイティブの巻き戻し(ORC が eh_frame を登録)、Wasm は exnref の tag、interp は Result。REPL 以外では、ヒープ上のハンドラの連鎖をたどって cancel を走らせてから終了する道も残す | Wasm ではトラップとスタックあふれを捕まえられない |
| スタックあふれ | 深さのカウンタ(Wasm)と、プロローグでのスタック上限の検査(LLVM)で、実行時エラーとして送出する | Q8 と so.kel の振る舞い |
| 入れ子の resume の差 | 背骨の経路の振る舞い(内側の resume が先に再開する)を正とし、Diktor の短い経路を合わせるかは別に決める | §13.5 と評価順序 |
| 活性化の印 | 単調増加の 64 bit カウンタ | フレームのアドレスはスタックの移動で変わる |
