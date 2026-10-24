# Node のスタックと WasmGC の配列の長さの測定

計画 `doc/log/261024-1-plan.md` の M79 の試作である。
Node 26.9.0 の V8 について、Wasm の非末尾の再帰の 1 段のバイト数、`--stack-size` と `Worker` の `stackSizeMb` の上限、WasmGC の配列の長さの上限を測る。
測定日は 2026-10-24 で、測った値と決めた値は `doc/log/261024-2-m79.md` の 1 節と 2 節に書いた。

## ファイル

| ファイル | 中身 | 必要な準備 |
|---|---|---|
| `depth.mjs` | `../wasm/out/tail.wasm` の `even_nt`(非末尾の相互再帰)が通る最大の段数を、上限 10,000,000 の二分探索で求める | `tail.wasm` を作る(下の手順) |
| `wk.mjs` | `depth.mjs` を `Worker` の `resourceLimits: {stackSizeMb: 1024}` で走らせる(計画の E12) | `depth.mjs` と同じ |
| `bigframe.mjs` | 1 段のバイト数の表(局所の数ごとと、引数の数ごと)、`--stack-size` の上限、`stackSizeMb` の上限を測り、`B_wasm` と `S` を決めて確かめる | なし |
| `arraylen.mjs` | `array.new` と `array.new_default` が受け付ける最大の長さと、それを超えたときの trap を測る | なし |
| `wasmgen.mjs` | `bigframe.mjs` と `arraylen.mjs` が使う、Wasm のバイナリを JS で組む小さな書き手 | なし |

`bigframe.mjs` と `arraylen.mjs` は、Wasm のモジュールを `wasmgen.mjs` でバイト列として組み、`WebAssembly.validate` で検証してから使う。
そのため、新しく clone した木でもビルドの手順は要らない。
`.wasm` のファイルは読まず、書きもしない。

`depth.mjs` だけは、`wasm/` の試作が `wat/tail.wat` から作る `out/tail.wasm` を `import.meta.url` からの相対パスで読む。
`out/` は `wasm/.gitignore` で除いてあるので、clone の後に一度作る。
無いときは、`depth.mjs` は作り方を標準エラーに書いて終了状態 2 で終わる。

`doc/log/261024-1-spike/probe/wk.mjs` は計画の作成中に E12 を測り直したもので、`wasm/depth.mjs` を指す。
M79 の受け入れが指すのは、このディレクトリの `wk.mjs` である。

## 手順

コマンドはリポジトリの根から実行する。

```sh
(cd doc/log/261024-1-spike/wasm && cargo run -q)        # out/tail.wasm を作り wasmparser で検証する
(ulimit -s 2000000; node --stack-size=1500000 doc/log/261024-1-spike/stack/depth.mjs)
node doc/log/261024-1-spike/stack/wk.mjs
node doc/log/261024-1-spike/stack/bigframe.mjs          # 約 70 秒。最大で 16 GiB のスタックに触れる
node doc/log/261024-1-spike/stack/bigframe.mjs --quick  # 1 段のバイト数の表だけ。約 15 秒
node doc/log/261024-1-spike/stack/arraylen.mjs          # 約 50 秒。最大で約 1 GiB の配列を作る
```

`bigframe.mjs` は、測る条件ごとに自分を子プロセスとして起動する。
`--stack-size` を変える子は `/bin/sh -c 'ulimit -s …; exec node …'` の下で走らせ、軟らかい `RLIMIT_STACK` を `--stack-size` より 64 MiB 大きくする。
このため、硬い限度(`ulimit -Hs`)がその値以上である必要がある。
この機械の硬い限度は `unlimited` である。
`Worker` の子は `ulimit` を変えない。
`Worker` のスタックはスレッドのスタックで、`RLIMIT_STACK` に縛られないからである。

## 測り方

`bigframe.mjs` の関数 `f` は、基底の場合を持たない自己再帰である。
各段で大域の計数器を 1 増やし、K 個の値を K 個の局所に読み、`f` を末尾でない位置で呼び、戻った後で K 個の局所をすべて使う。
読む先は、`i64` の版では線形メモリ(`i64.load`)、`eqref` の版では大域の GC 配列(`array.get`)である。
呼び出しがメモリと配列を書き換えうるので、どの段でも読みを呼び出しの後へ動かせず、K 個の値が呼び出しをまたいで生きる。
`RangeError` が起きた時点の計数器の値を、届いた段数とする。

この `f` の引数は `i64` の 1 個だけである。
引数の数の効き方は、KIR の WasmGC の関数の型の形(クロージャ、N 個の `eqref` の引数、フレームの P = N + 2 個の `eqref`)の版で別に測る(出力の 1b の表)。
この版の各段は、K 個の局所を GC 配列から読み、P 個の引数をすべて末尾でない自己の呼び出しに渡し、呼び出しの後で引数と局所をすべて使う。

1 段のバイト数は、`--stack-size` が 64 MiB と 256 MiB のときの段数の差で 192 MiB を割って求める。
差を取るので、最初の Wasm のフレームより下で使う量は消える。
段の層は、既定、`--liftoff-only`、`--no-liftoff`、`--no-liftoff --no-wasm-inlining` の 4 つを測る。

## 結果の要約

- 1 段のバイト数は、引数 1 個の版の既定と Liftoff で `8 × K + 64`(K は局所の数)である。
  TurboFan で自己の呼び出しのインライン展開を止めると `8 × K + 40` である。
  インライン展開ありの TurboFan の値(K = 0 で 8 バイト、`eqref` で 1 局所あたり 1.6 バイト)は、複数の段が 1 つのフレームを共有した結果で、1 段の費用ではない。
- 引数を呼び出しに渡す関数では、既定と Liftoff で、P が 4 以下のあいだは引数 1 個あたり 8 バイト、P が 18 から 66 のあいだは 1 個あたり 24 バイト増える。
  そのため、フレームの大きさの推定を `8 × K + 24 × P + 64` とした。
  測ったすべての組で、1 段はこの推定以下である。
- `--stack-size` は 2,097,151 KiB まで使える。
  2,097,152 KiB(2 GiB)以上は、試したどの値でも使えない。
  V8 が `Check failed: IsOnCentralStack()` で落ちる(SIGTRAP)か、起動の直後に `RangeError` で終わる(終了状態 7)。
- `Worker` の `stackSizeMb` は、試した最大の 16,384 まで、期待どおりの段数に届いた。
- `B_wasm` は 1,536 バイトにした。
  計画 6.10 節の初期値 2 KiB では、`(L+R) × 2048` が `--stack-size` の上限を超えるからである。
  推定がちょうど 1,536 バイトの関数は、引数の数に依らず `L + R` 段に届いた。
- WasmGC の配列の長さの上限は、i8 で `2^30 − 32`、`eqref` と `i64` で `2^27 − 8` である。
  どれも中身がおよそ 1 GiB になる長さで、超えると `RuntimeError: requested new array is too large` で trap する。
  `N_alloc_max` は `2^26` に決めた。
