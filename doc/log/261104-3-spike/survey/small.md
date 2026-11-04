# 調査: 小さな宿題 3 件(計画 261104-3 の (3))

- 日付: 2026-11-04
- 対象: 親 69febfd、diktor cccd8cb。`rustkeleut/target/release/keleut` は HEAD から組んだもの。
- 材料: `doc/log/261104-3-spike/probe/small/`(生成器、測り方、入力、結果の控え `results.txt`)。
- 測定の条件: 32 コア、31 GB、WSL2。
  測定の間、機械はほかの作業で負荷が高く(load average 9〜38)、壁時計の時間は回によって 2〜4 倍ぶれた。
  時間は CPU 時間(user + sys)で比べ、同じ時刻に並べて測ったものだけを比べる。
  最大の常駐の大きさ(`ru_maxrss`)は回によらず安定していた。

記録 261024-3 が「残した項目」(5668〜5680 行)に挙げた次の 3 件を調べた。

1. `keleut check` が型検査の文脈と木を解放しないこと(M98 の 6。5675 行)
2. `rustkeleut/lib/rt.kirt` と wide 形(M87 の 1 と 2。5676 行)
3. `conformance/float/` の cram が無いこと(M101 の 10。5673 行)

## 要約

- 1 は、`keleut check` についての記録の判断は正しいが、同じ `std::mem::forget` が `keleut run FILE.kel` と `keleut emit-kir` の道にもあり、型検査の文脈が実行の間ずっと常駐する。
  16 万個の宣言に 200 万要素のリストを作る処理を足した入力では、`keleut run` の最大の常駐の大きさが 917 MB(interp)と 1,023 MB(jit)で、解放すれば 722 MB になる【実測】。
  `ulimit -v` の下では、解放しないことが、通るはずのプログラムを 3 や 70 で終わらせる【実測】。
  `keleut run` と `keleut emit-kir` では解放し、`keleut check` は今のままにすることを勧める。
- 2 の 2 つは、どちらも作る理由が残っていない。
  `rt.kirt` の目的(組み込みの中の `perform` を取り出せること、深さを IRSpec 付録 A.4 に合わせること)は `keleut-lir` の LIR の関数で満たされており、全後段で Diktor と一致した【実測】。
  wide 形は、被演算子をはじめから 32 ビットの語に置く符号化で不要になっており、スロットが 20,002 個の関数も全後段で正しく走った【実測】。
  計画 261024-1 の D533 と 6.7 節の記述を新しい裁定で置き換えて閉じ、取り出しの経路の試験を 1 件足すことを勧める。
- 3 は、M101 で `conformance/README.md` に「`float/` は置かない」と書いて実質的に閉じている。
  ただし、浮動小数のリテラルの読み(フロントエンド)と、`kw` の後段での境界の値の印字は、cram の形では確かめていない。
  探った 26 件の境界の値は、試した 8 つの組み合わせ(`dk-ir`、`x-ir`、`keleut run` のフロントエンド 2 つ × 後段 3 つ)で一致した【実測】ので、不具合の手がかりは無い。
  これらを `prim/prim.t` に足すか `conformance/float/float.t` を作ることを勧める(小さな作業)。

## 1. `keleut check` が型検査の文脈と木を解放しないこと

### 記録の記述

- 記録 261024-3 の 4437〜4438 行(M98 の行ったこと): `keleut check` は文脈と木を `std::mem::forget` で捨てずにプロセスを終える。
  perf では、16 万個のトップレベルの宣言の入力で、終わりの解放が全体の約 6 % を占めていた。
- 同 4562〜4564 行(M98 の計画から外れたことの 6): プロセスはすぐに終わるので解放の時間を省いた。
  ライブラリの `check_files` と `check_program` は、呼び出し側が値を捨てる。
- 同 5675 行: 残した項目に挙げる。

### 今のコード

`forget` は 2 か所にある。

- `rustkeleut/crates/keleut-cli/src/check_cmd.rs:131-137`(`run_check`): `keleut check`。
- `rustkeleut/crates/keleut-cli/src/emit_kir_cmd.rs:208-210`(`build`): コメントは「`keleut check` と同じ理由」とする。

`build` は `keleut emit-kir` だけでなく、`program_for_run`(`emit_kir_cmd.rs:431` から)を通して `keleut run FILE.kel` の既定のフロントエンド(`--frontend rust`)からも呼ばれる(`run_cmd.rs:359-360`)。
`compat-diktor` の実行の道と、kconform の `kr-*` の組も同じ道を通る。
したがって `keleut run` では、型検査の文脈と木が、フロントエンドのスレッドが終わった後もプログラムの実行の間ずっと残る。
記録はこの影響に触れていない。

`keleut repl` は文脈を持ち続ける必要がある(M98 の 4466 行の `Checked` の `cx`)ので、この件の対象ではない。

### 測った値

入力は `probe/small/gen_decls.py`(N 個の `let x_i = i + 1`。M98 の速さの表の入力と同じ形)と `gen_block.py`(N 文のブロック)で作った。
`list` を付けた入力は、最後に 200 万要素のリストを作って数える(実行時のヒープを使う)。
測り方は `probe/small/measure.py`、値の控えは `probe/small/results.txt` にある。

`keleut check` と `diktor --type-check`(同じ時刻に測ったもの)【実測】:

| 入力 | keleut check | diktor --type-check |
| --- | --- | --- |
| 宣言 160,000 個 | 最大 606 MB | 最大 361 MB |
| 120,000 文のブロック | CPU 1.35 秒、最大 408 MB | CPU 3.17 秒、最大 171 MB |
| 宣言 20,000 / 40,000 / 80,000 個 | CPU 0.34 / 0.72 / 1.30 秒、最大 117 / 206 / 357 MB | CPU 0.36 / 0.74 / 1.38 秒、最大 53 / 97 / 184 MB |

最大の常駐の大きさは M98 の表(4526〜4527 行。541 MB と 382 MB)とほぼ同じで、Diktor の 1.7〜2.4 倍である。
宣言 160,000 個の CPU 時間は 1.9〜8.4 秒の間でぶれ、比べられる値にならなかった。

解放の費用を測るため、HEAD の `rustkeleut/` の写しの 2 か所の `forget` を、環境変数 `KELEUT_FREE_FRONT` があれば `drop` にする形に変えて組んだ(`probe/small/free-patch.md`。リポジトリには入れていない)。
同じバイナリで、`forget` と `drop` を比べた(5 回か 3 回の中央値)【実測】。

| コマンドと入力 | forget | drop |
| --- | --- | --- |
| `check`、宣言 160,000 個 | CPU 1.88 秒、最大 607 MB | CPU 2.02 秒(+7 %)、最大 602 MB |
| `check`、120,000 文のブロック | CPU 0.79 秒、最大 408 MB | CPU 0.94 秒(+18 %)、最大 408 MB |
| `run --backend interp`、宣言 160,000 個 + リスト | 最大 917 MB | 最大 722 MB |
| `run --backend jit`、同じ入力 | 最大 1,023 MB | 最大 722 MB |

- `keleut check` では、`drop` は最大の常駐の大きさを減らさず、時間だけを増やす。
  記録の判断(解放を省く)はこの入力でも成り立つ。
  増える割合は、M98 の「約 6 %」と入力が違い、ブロックでは 18 % だった。
- `keleut run` では、`drop` にすると最大の常駐の大きさが `keleut emit-kir` の最大(722 MB)と同じになる。
  フロントエンドの最大がそのまま全体の最大になり、実行の段の確保がその上に積まれなくなるからである。
  比べると、同じプログラムの `.kir` を `keleut run` に渡したときは、最大 431 MB(interp)と 500 MB(jit)である。
  小さな入力(宣言 10 個 + リスト)では `.kel` 90 MB、`.kir` 88 MB で、差は入力の大きさに比例して開く。
- `keleut run` の時間は、`forget` と `drop` で差が見えなかった(interp 4.65 秒と 4.46 秒、jit 4.16 秒と 4.15 秒。負荷のぶれの範囲)。

`ulimit -v` の下で、`keleut run --native-stack 512k --backend interp` を宣言 160,000 個 + リストの入力で走らせた【実測】。

| `ulimit -v`(KiB) | forget | drop |
| --- | --- | --- |
| 1,500,000 | 0 | 0 |
| 1,450,000 / 1,400,000 | 3(実行時エラー: メモリ不足です) | 0 |
| 1,350,000 / 1,300,000 / 1,250,000 | 70(KIR を符号化できません: KIR の処理のスレッドを作れません) | 0 |
| 1,200,000 | 134(memory allocation of 20971520 bytes failed) | 134(同じ) |

- 同じ入力の `.kir` は 1,250,000 で 0 になる。
  `keleut emit-kir` も 1,250,000 で、`forget` は 70、`drop` は 0 だった。
- `forget` は、約 200 MB の余裕を失わせ、本来通るプログラムを 3 か 70 で終わらせる。
- 70 と 134 は、この件とは別の問題である。
  70 は、仮想メモリの上限の下でスレッドを作れないことを内部の誤りとして扱うもので、134 はフロントエンドの中の確保の失敗(V230 と同じ形)である。
  70 はフロントエンドが型検査と KIR の書き出しを終えた後に起こり、型検査に通ったプログラムが内部の誤りの終了状態で止まる。
  どちらも計画 261104-3 の (2)(実行時の確保の制限)で扱うのがよい【推論】。

小さい `--native-stack` の下で `drop` が再帰であふれないかも確かめた【実測】。
記録の 4441〜4442 行は、限度を超えた木を再帰で捨てるとスタックがあふれたことを書いている。
そこで、限度の下で受け付ける最大の深さの木を、`drop` で `check` と `run` に通した。

| `--native-stack` | ブロック | `1 + (...)` | `fn(x) =>` の入れ子 |
| --- | --- | --- | --- |
| 512k | 124 文: 0 / 0 | 125 段: 0 / 0 | 125 段: 0 / 0 |
| 1m | 380 文: 0 / 0 | 381 段: 0 / 0 | 381 段: 0 / 0 |
| 4m | 1,788 文: 0 / 65 | 1,789 段: 0 / 65 | 1,789 段: 0 / 0 |

(各欄は `check` と `run` の終了状態。)
4m の `run` の 65 は「KIR の検証で入れ子が深すぎます」で、HEAD の `forget` でも同じになる。
記録の M101 の計画から外れたことの 8(5455 行)が書いた既知の振る舞いで、`drop` とは関わらない。
`frontend-depth/typecheck.t` の `big.kel`(推論した型が深い入力)も、4m で `check` と `run` が 1、既定のスタックの `run` が 0 で、`forget` と同じだった。
既定のスタックでの 99,000 段の `1 + (...)` も、`drop` で落ちなかった。
受け付ける木の深さは `--native-stack` から決めた入れ子の限度で抑えられ、捨てるときの 1 段は型検査の 1 段より小さい、と考えられる【推論】。
ただし、試したのは 4 つの形(ブロック、`1 + (...)`、`fn`、`big.kel`)だけなので、作業の段では、`ast::dismantle` と同じく明示の作業の列で捨てるかどうかを、ほかの形(パターン、型、`match` の入れ子)でも確かめる必要がある。

### 直すとしたら

- `emit_kir_cmd.rs` の `build` で、`program_of_checked` の後に `drop(c)` にする(`forget` を除く)。
  `keleut emit-kir` と `keleut run` の両方に効く。
  `keleut emit-kir` だけは `forget` のままにする選択もあるが、上の表のとおり `emit-kir` 自身も `ulimit -v` で余裕を失うので、両方 `drop` にするのが簡単である。
- `keleut check` は、記録の判断どおり `forget` のままでよい。
- 捨てる処理はフロントエンドのスレッドの中で行う(`build` はそのスレッドで動く)。
  解放がフロントエンドのスレッドの malloc のアリーナに返るだけで、実行のスレッドがそれを再利用できるかどうかは確かめていない。
  ただし、上の `ulimit -v` の結果では、`drop` で仮想メモリの余裕が戻っている【実測】。
- 変更は 2〜3 行で、試験を合わせても小さい。

### 試験の設計

- 4 MB の入力をヒアドキュメントで cram に置くのは現実的でないので、cram ではなく `crates/keleut-cli/tests/run_cmd.rs` に置く。
  宣言を数万個並べた `.kel` を試験の中で作り、`sh -c 'ulimit -v N; keleut run --native-stack 512k --backend interp …'` で走らせて 0 で終わることを確かめる。
  N は、`.kir` から走らせると通り、`forget` の版では通らない値を、入力の大きさとともに測って決める。
  機械や割り付け器で値が変わるので、余裕を大きめに取る(たとえば `.kir` の最大の 1.3 倍)【推論】。
- 小さい `--native-stack` で、受け付ける最大の深さの木(ブロック、`1 + (...)`、`fn`、パターン、型、`match`)を `keleut run` に通し、134 で落ちないことを確かめる試験を同じ所に置く。
  `conformance/frontend-depth/` の cram に、`keleut run --native-stack 512k` の場合を足してもよい。

## 2. `rustkeleut/lib/rt.kirt` と wide 形

### 記録の記述

- 記録 261024-3 の 2085〜2089 行(M87 の計画から外れたことの 1): `rt.kirt` は書かなかった。
  D533 の理由(利用者の関数を呼ぶ組み込みの中の呼び出しも再開点を持ち、中の `perform` も取り出せる)は、M86 が `keleut-lir` で作った LIR の関数で満たしており、`rt.kirt` を書くには別の KIR のプログラムの関数の表をつなぐ仕組みが要るが、振る舞いは変わらない。
  `with_runtime` の結線を含めて M89 で決める、とした。
- 同 2634〜2637 行(M89 の計画から外れたことの 6): `rt.kirt` は書かなかった。
  `Array.each` と `pinned` は LIR の関数のままで、`with_runtime` の結線は `runtime` の印の付いた `Console.write` を `console_write` に下ろすことで足りる。
- 同 2090〜2092 行(M87 の計画から外れたことの 2): 計画 6.7 節の wide 形は作らなかった。
  被演算子を 1 つずつ 32 ビットの語に置くので、スロットの数に依らず 1 つの形で足りる。
  語の数が増える費用は M93 の計測で確かめる、とした。
- 同 5676 行: 残した項目に挙げる。

### それぞれが何か

- `rt.kirt` は、計画 261024-1 の 4.9 節(437〜441 行)、ファイルの木(651 行)、裁定 D533(1379 行)にある、組み込み(`Array.each`、`pinned`、`with_runtime` の結線)を KIR のテキストで書くファイルである。
  `rustkeleut/lib/` は存在しない【実測】。
  リポジトリの中で `rt.kirt` を挙げるのは、計画 261024-1 と記録だけである(AGENTS.md、`rustkeleut/doc/`、crate のコードには無い)【実測】。
- wide 形は、KIR や IRSpec のものではなく、インタプリタのバイトコード(計画 261024-1 の 6.7 節、945 行「スロットが 256 を超える関数には wide 形を使う」)の命令の形である。
  IRSpec と `rustkeleut/doc/lir.md` に wide という語は無い【実測】。

### 今のコード

- `rustkeleut/crates/keleut-lir/src/wrappers.rs:1-10` が、組み込みの値の本体を LIR の関数のクロージャとして作ると書き、`Array.each` と `pinned` の呼び出しを末尾でない呼び出しとして数える(IRSpec 付録 A.4。`doc/IRSpec.md:1001`、`1809`、`1815`)。
  `wrappers.rs:183-230` が本体と `Array.each` の内側のループ(自己の末尾呼び出し)を作り、`program.rs:222-233` が `Array.each` と `pinned` を yield しうる関数として登録する。
- 深さは `conformance/depth/basic.t:22-40`(`each.kel`、`pinned.kel`)が確かめる。
  Diktor の側は `diktor/test/ir_depth.t:47-64` である。
- `rustkeleut/crates/keleut-interp/src/bytecode.rs:6-13` が、先頭の語(命令の番号 8 ビット、細目 24 ビット)に続けて被演算子を 1 語ずつ置き、wide 形を持たないと書いている。

### 測った値

`Array.each` と `pinned` の中の `perform`【実測】:
D533 の目的である「組み込みの中の `perform` の取り出し」を直に確かめる試験は、`conformance/` と `diktor/test/` に無かった(`Array.each` か `pinned` と `handle` を同時に書いたファイルを grep で探し、`trailing_block.t`、`ir_depth.t` などの当たりはどれも `perform` を組み込みの中で行わない)。
そこで `probe/small/eachfx.kel` を書いた。
G の節の末尾でない `resume` で `Array.each` の中の yield を集める、末尾の `resume` で足す、Never を返す操作で `Array.each` の途中から抜ける、`pinned` の中で yield して再開する、の 4 つである。
期待は Diktor の木の評価器の出力(`eachfx.expected`)である。
`dk-ir`、`keleut run --frontend rust|diktor` × `--backend interp`、`wasm`、`jit`(`--tier jit-only`、`threshold=1`、`alternating=1..3`、`mixed=1..2`)、`interp --gc-stress` のどれも差 0 だった。

スロットが 256 を超える関数【実測】:
`probe/small/gen_slots.py N` は、N 個の `Int32` を生かしたまま `id` を呼ぶ関数を書く。
`keleut dump bytecode` の最大の関数は、N = 300 で any 3、raw 302、8,104 語、N = 3,000 で raw 3,002、81,004 語、N = 20,000 で raw 20,002、540,004 語だった。
3 つとも、Diktor の木の評価器、`keleut run --backend interp`、`wasm`、`jit --tier jit-only` の出力が一致した。
既存の試験では、`crates/keleut-cli/tests/wasm_cmd.rs:215-290` が 400 個と 6,000 個の値を生かす大きなフレームの関数をインタプリタと WasmGC で比べる(`KELEUT_DIKTOR` を与えたときだけ走る)。

バイトコードの語の密度【実測】:
`conformance/corpus/` の 1,091 本を `keleut dump bytecode` にかけた(プレリュードの関数を本ごとに数える)。
関数 74,339 個、スロットが 256 を超える関数は 0 個、スロットの最大は 14、語は 2,418,324、命令は 832,041、被演算子の語は 1,586,283 で、そのうち 1,492,443(94.1 %)が 256 未満の値だった。
M87 が M93 に送った「語の数が増える費用」は、M93 の記録(`doc/log/261028-1-m93.md`)にも記録 261024-3 の M93 の節(3460 行から)にも、バイトコードの語の数としては測られていない【実測(grep)】。
上の 94.1 % が、いま手元にある唯一の数である。
被演算子を 8 ビットに詰める狭い形を作れば語の数は減りうるが、インタプリタは既定の段(`--tier threshold=1000`)で最初の 1,000 回の呼び出しにしか使わないので、効く場面は短いプログラムとインタプリタだけの構築に限られる【推論】。

### 直すとしたら

- `rt.kirt`: 書かない。
  新しい裁定で D533 の「`rt.kirt` に KIR で書く」を「`keleut-lir` が LIR の関数として作る」に置き換え、理由に M87 と M89 の記録と上の取り出しの結果を挙げる。
  計画 261024-1 は完了した計画なので本文は変えず、新しい計画の台帳で閉じるのがよい【推論】。
- wide 形: 作らない。
  計画 6.7 節の要求(スロットが 256 を超える関数を扱う)は、はじめから 32 ビットの被演算子で満たしている。
  狭い形による語の数の削減は、急がない最適化として計画 261104-3 の (5) の延期の項目に並べるか、採らないとして閉じる。
  決めるなら、`bench/run.py` の `ki` の構成で、狭い形の試作と比べる測定が要る。

### 試験の設計

- `eachfx.kel` の 4 つの場合を `conformance/effects/g.t` に足す(記録は `keleut run` の結果で、Diktor の木の評価器とも一致する、という `effects/` の形に合う)。
  `--suite all` で全組にかかる。
- スロットが 256 を超える関数は、`gen_slots.py 300` 程度の大きさなら cram のヒアドキュメントに `awk` で作れる(`frontend-depth/typecheck.t` の `big.kel` と同じ作り方)。
  `conformance/prim/` か新しいディレクトリに置けば、`KELEUT_DIKTOR` が無くても全組で走る。

## 3. `conformance/float/` の cram が無いこと

### 記録の記述

- 記録 261024-3 の 5461〜5464 行(M101 の計画から外れたことの 10): 計画 6.1 節と 8.1 節が挙げる `float` の cram は作らなかった。
  どのマイルストーンの行もこの試験を割り当てておらず、Float64 の印字は `conformance/prim/prim.t`(LangSpec §16.2 の綴り、libm、Float64 から整数への変換)と、crate `keleut-rtcore` の `tests/fltcmp.rs`(glibc の印字との比べ合わせ。M85)の 2 か所で確かめている。
  `conformance/README.md` の文を、この 2 か所を指す文に改めた。
- 同 5673 行: 残した項目に挙げる。

### 今の状態

- `rustkeleut/conformance/README.md:23-24` が、`float/` を置かないことと、上の 2 か所で確かめることを書いている。
  つまり、ディレクトリを作らないことは M101 で決まっている。
  ただし M101 の理由は、どのマイルストーンもこの試験を割り当てていなかったことで、置くと害があると判断したわけではないので、文面の決定として戻せる。
- `conformance/prim/prim.t:72-143` が整数と Float64 の演算と印字(`0.1 + 0.2`、`100000.0`、`0.0001`、`1e-5`、`1e16`、`1e15`、`-0.0`、`nan`、`inf`、`-inf`、`5e-324`、`1.2345678901234568e20`、NaN の比較、`{a = 0.0} == {a = -0.0}`、変換)を、`prim.t:173-183` が libm の 5 関数を、`prim.t:202-207` 以降が Float64 から整数への変換の範囲を確かめる。
  cram なので全組(`kw` を含む)で走る。
- `rustkeleut/crates/keleut-rtcore/tests/fltcmp.rs:1-8, 19` が、1,998,964 件の候補と境界の値で、`show(Float64)` を glibc の `%.*g` の探索と比べる。
  wasm32 版の確認は `tests/wasm_check.mjs` を node で手で走らせるもので、`cargo test` からは呼ばれない(`grep -rn wasm_check crates` の当たりはコメントと試験の入力の書き出しだけ)【実測】。
- リテラルの読みは、keleut-front の `ocaml_float_of_string`(`crates/keleut-front/src/types/emit.rs:2847-2873`)が `strtod` と同じく正しく丸める Rust の `parse::<f64>` で行う。
  言語の規則は `doc/sample.kel:103-114` にある。

### 測った値

`probe/small/floatlit.kel` は、`prim.t` に無い 26 件の境界の値を書く。
`1.`、`1.e5`、`1E5`、`1e309`(`inf`)、`1e-400`(`0.0`)、最小の非正規化数の半分の前後(`2.4703282292062327e-324` は `0.0`、`…28e-324` は `5e-324`)、2^53 + 1 と 2^53 + 3 の丸め、最大の有限値の丸め(`1.7976931348623158e308` と `…59e308`)、正規化数の境(`2.2250738585072011e-308`)、`1e22`、`1e23`、`9999999999999998.0`、`-1e16` などである。
期待は Diktor の木の評価器の出力(`floatlit.expected`)で、`dk-ir`、`x-ir`(`keleut emit-kir` の後の `diktor --run-ir`)、`keleut run --frontend rust|diktor` × `--backend interp|jit|wasm` のどれも差 0 だった【実測】。

接尾辞と桁区切りの 15 件(`3f64`、`1_000.5`、`0x10f64`、`1e5f64`、`1.5f64`、`99999999999999999999f64`、`0xffffffffffffffffff_f64`、`1_0e1_0`、`0b101f64`、`0o17f64`、16 進の浮動小数の綴り `0x1p3`、`0x1.8` など)も、Diktor と `keleut run` で出力と終了状態が一致した【実測】。
`0x10f64` は `f64` を 16 進の数字として読み、`Int32` の 69476 になる。
16 進の浮動小数の綴りは、どちらも同じ構文エラー(2)になる。

### 直すとしたら

- 不具合の手がかりは見つからなかった。
  作業は、上の境界の値を cram にして全組で見張ることだけである。
- 置き場所は 2 案ある。
  - (a) `prim/prim.t` に段落を足す。
    README を変えずに済む。
  - (b) `conformance/float/float.t` を作り、README の 23〜24 行を、`float/` がリテラルの読みと境界の値の印字を、`prim/` が演算と libm を受け持つ形に改める。
    計画 6.1 節と 8.1 節の当初の配置に戻る。
- どちらでも、`kr-*` と `x-ir` の組がフロントエンドのリテラルの読みを、`kw` と `kr-kw` が wasm32 の印字を、`cargo test` の外で手で走らせる `wasm_check.mjs` に頼らずに見張れるようになる。
  量は 30〜40 行の cram である。
  (a) のほうが小さいが、M101 が README に書いた決定を変えずに済むことと、`prim.t` がすでに長いことのどちらを重く見るかで決める【推論】。

### 試験の設計

- `floatlit.kel` の 26 件と、接尾辞と桁区切りの 15 件(構文エラーになるものは 1 件ずつのファイル)を cram にする。
  記録は `keleut run` の結果とし、Diktor の木の評価器の出力と同じことを作業の段で確かめる(上の結果のとおり一致する)。
- 演算の境界(`fma` の縮約が起こりうる `a * b + c` の形、JIT の定数畳み込みに入る定数式と実行時の値の式の比較)も 2〜3 件足すと、LLVM の後段の最適化が IEEE 754 の丸めを変えないことを見張れる【推論】。
  今回の探りでは、トップレベルの定数式だけを `--tier jit-only` で確かめた。

## 計画への提案

| 件 | 勧める扱い | 量 |
| --- | --- | --- |
| 1 | `emit_kir_cmd.rs` の `build` の `forget` を `drop` にする。`keleut check` は今のまま。`run_cmd.rs` の試験に `ulimit -v` の場合と、小さいスタックで最大の深さの木を捨てる場合を足す | コード数行、試験 50〜100 行 |
| 2 | 新しい裁定で D533(`rt.kirt`)と計画 6.7 節の wide 形を置き換えて閉じる。`effects/g.t` に組み込みの中の `perform` の 4 件、スロットが 256 を超える関数の cram を足す | 試験 40〜60 行 |
| 3 | 境界の値の cram を `prim/prim.t` に足すか `conformance/float/` を作る | 試験 30〜40 行 |

どれも振る舞いを変えるのは 1 の `keleut run` と `keleut emit-kir` のメモリの使い方だけで、出力と終了状態は変わらない(`ulimit -v` の下で通る範囲が広がることを除く)。
1 の `ulimit -v` の下の 70 と 134 は、計画 261104-3 の (2)(V230 など)で扱う項目として引き渡す。
