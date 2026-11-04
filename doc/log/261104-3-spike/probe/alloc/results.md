# 項目 (2) の測定の結果(2026-11-04)

機械は 32 コア、31 GB、WSL2。
`keleut` と `kconform` は HEAD(親 69febfd)から作った release の組み、Diktor は `diktor/_build/install/default/bin/diktor`(cccd8cb)。
時間と最大の常駐の大きさは `measure.py`(`os.wait4` の rusage)で 1 回ずつ測った。
`--as N` は `ulimit -v N`(KiB)と同じである。

## 1. 拡張の鎖(V229)

`run_chain.sh` と `gen_chain.py` で作った。
KIR は `keleut emit-kir` で書き、同じ `.kir` を各後段と `diktor --run-ir` で走らせた。

| 種類と段の数 | `emit-kir` | `ki` | `kj`(既定の段) | `kw` | `dk-ir` |
| --- | --- | --- | --- | --- | --- |
| top 3,000 | 6.09 秒、443 MB | 0.29 秒、125 MB | | | 0.05 秒、12 MB |
| top 6,000 | 45.21 秒、1,660 MB | 0.53 秒、399 MB | | | 0.03 秒、15 MB |
| top 12,000 | 226.13 秒、6,523 MB | 1.82 秒、1,482 MB | | | 0.05 秒、21 MB |
| fun 2,000 | | 0.13 秒、98 MB | 0.13 秒、133 MB | 0.19 秒、133 MB | |
| fun 4,000 | 4.12 秒、1,273 MB | 0.43〜0.49 秒、268 MB | 0.47 秒、304 MB | 0.53 秒、358 MB | 0.01 秒、11 MB |
| fun 8,000 | 19.19 秒、4,960 MB | 1.78 秒、944 MB | 1.81 秒、979 MB | 1.77 秒、1,225 MB | 0.01 秒、14 MB |
| same 8,000 | 10.91 秒、2,810 MB | 0.62 秒、486 MB | | | 0.03 秒、17 MB |
| same 16,000 | 47.71 秒、11,084 MB | 2.50 秒、1,810 MB | | | 0.07 秒、25 MB |

`KELEUT_GC_STATS=1 keleut run --backend interp` の「持っている語」は、fun 2,000 で 1,591,314、4,000 で 1,419,740、8,000 で 1,461,045 だった(GC のヒープは約 11〜13 MB で変わらない)。

フロントエンドの比較(同じ `.kel`):

| 種類と段の数 | `diktor --type-check` | `keleut check` |
| --- | --- | --- |
| top 3,000 | 5.80 秒、577 MB | 2.78 秒、498 MB |
| top 6,000 | 27.17 秒、2,448 MB | 12.09 秒、1,894 MB |
| fun 4,000 | 18.58 秒、1,498 MB | 4.09 秒、1,273 MB |

## 2. 形の表の確保の失敗(V229)

`gen_chain.py func N` は、鎖を `cancel` 節を持つハンドラの中で呼び、前に `before` を書く。
期待(§15.5)は `before`、`cancel`、`実行時エラー: メモリ不足です`、3 である。

| 入力 | 条件 | 結果 |
| --- | --- | --- |
| func 8,000 の `.kir` | `--as` 4,000,000、既定の `--native-stack` | `ki`、`kj` とも 0(`8000`) |
| 同上 | `--as` 2,000,000 と 1,500,000、既定の `--native-stack` | `ki`、`kj` とも 134、標準出力は空 |
| 同上 | `--as` 1,200,000 以下、既定の `--native-stack` | 70(`実行のスレッドを作れません(1157627904 バイトのスタック)`) |
| 同上 | `--native-stack 16m` | 65(`KIR の読み込みで入れ子が深すぎます`) |
| 同上 | `--native-stack 64m`、`--as` 1,500,000 / 1,000,000 / 600,000 / 400,000 | 134(`memory allocation of 1207959576 / 603979800 / 301989912 / 75497496 bytes failed`)、標準出力は空 |
| func 3,000 の `.kir` | 制限なし、`--native-stack 64m` | 0.92 秒、268 MB、0 |
| 同上 | `--native-stack 64m`、`--as` 524,288 / 400,000 / 300,000 | `ki`、`kj` とも 134、標準出力は空。524,288 では `memory allocation of 150994968 bytes failed` |
| func 3,000 の `.kel` | `--native-stack 64m`、`--as` 524,288 | 134(`memory allocation of 22120 bytes failed`。フロントエンドの型検査の中) |
| func 3,000 の `.kir` | `diktor --run-ir`、`--as` 300,000 | `before`、`3000`、0 |

gdb の backtrace(`ulimit` は被検プロセスだけに掛けた):

- func 3,000 の `.kir`、524,288:`ShapeTable::grow` ← `ShapeTable::extend` ← `Rt::rec_extend` ← `Interp::exec_inner`。
- func 3,000 の `.kel`、524,288:`RawVec::grow_one` ← `keleut_front::types::Tc::extend` ← `Tc::map_generics_with`(入れ子)。

## 3. ヒープを使い尽くす例の終了状態(V230)

`oomlist.kel` を `--native-stack 64m` で、`sweep_as.sh` で走らせた。
「出力」は標準出力の行数で、期待は 2(`before` と `cancel`)である。

| `--as` | `--backend interp` | `--tier jit-only` | `--tier threshold=1` | `--tier alternating=1` |
| --- | --- | --- | --- | --- |
| 150,000〜200,000 | 127 | 127 | 127 | 127 |
| 225,000〜275,000 | 70 | 70 | 70 | 70 |
| 300,000 | 3、出力 2 | 70 | 70 | 70 |
| 325,000 | 3、出力 2 | 134 | 134 | 134 |
| 350,000 | 134 | 134 | 134 | 134 |
| 375,000 | 3、出力 2 | 3、出力 0 | 134 | 3、出力 0 |
| 400,000 | 3、出力 2 | 3、出力 2 | 134 | 3、出力 2 |
| 425,000〜700,000 | 3、出力 2 | 3、出力 2 | 3、出力 2 | 3、出力 2 |

各行の中身:

- 127:動的リンクの失敗(`libicudata.so.78: failed to map segment from shared object`)。
- 70:`KIR の処理のスレッドを作れません` か `実行のスレッドを作れません(67108864 バイトのスタック)`。
- 350,000 の `interp` の 134:`memory allocation of 12 bytes failed`。gdb の backtrace は `RawVec::handle_error` ← `keleut_lir::live::Points::live` ← `gc::Frames::scan` ← `Frames::walk` ← `Rt::collect` ← `Rt::alloc` ← `Rt::ctor_new`。
- 325,000 の `jit-only` の 134:`LLVM ERROR: out of memory` と `Buffer allocation failed`。
- 400,000 の `threshold=1` の 134:`terminate called after throwing an instance of 'St9bad_alloc'`。
- 375,000 の `jit-only` の 3、出力 0:標準エラーは `実行時エラー: メモリ不足です` だけ。`KELEUT_JIT_STATS=1` では翻訳した関数 1、`interp_stub` からの起動 0 で、`before` を書く前に止まっていた。

`oomlist.kel` を `keleut run --backend wasm`(制限なし)で走らせると、24 秒後に V8 の `FATAL ERROR: Ineffective mark-compacts near heap limit` で Node が SIGABRT になり、`keleut` は 70 で終わった。
標準出力は空(`before` も失われた)だった。

Diktor:

| 例 | 条件 | 結果 |
| --- | --- | --- |
| `oomlist.kel`(`diktor`)と `.kir`(`diktor --run-ir`) | `--as` 300,000、600,000 | 134(`Fatal error: allocation failure during minor GC`) |
| 1 億要素の `MutableArray.new` を `cancel` 節の中で | `--as` 600,000 | `before`、`cancel`、`実行時エラー: メモリ不足です`、3 |

## 4. 値スタックの塊(M90 の 4)

`chunks.kel` の `DEPTH` を 10,000,000 にし、`--max-depth 20000000` で走らせた。

| MODE | `ki` | `kj` |
| --- | --- | --- |
| 1(深い再帰だけ) | 0.95 秒、944 MB | 0.72 秒、1,735 MB |
| 2(リストの作り直しだけ) | 1.19 秒、157 MB | 0.39 秒、195 MB |
| 3(深い再帰の後にリストの作り直し) | 2.32 秒、1,072 MB | 1.96 秒、1,863 MB |

`ki`、`--native-stack 64m`、`.kir` の入力で、`--as` を変えた結果:

| `--as` | MODE 1 | MODE 2 | MODE 3 |
| --- | --- | --- | --- |
| 1,100,000 | 3 | 0 | 3 |
| 1,200,000 | 3 | 0 | 3 |
| 1,300,000 | 0(942 MB) | 0(155 MB) | 3(948 MB) |

## 5. 使われていない継続への注入(M89 の残した制限の 1)

`continj.kel` の `DEPTH` を 1,000 と 3,000,000 にし、`--backend interp --max-depth 5000000 --native-stack 64m`、`--as` 800,000 と 1,500,000 で走らせた。
4 つとも `before`、`inner cancel`、`own cancel`、`outer cancel`、`実行時エラー: メモリ不足です`、3 だった。
制限は再現しなかった。

## 6. Worker の resourceLimits(V215)

`node v8worker.mjs`(Node 26.9.0)は、`maxOldGenerationSizeMb: 64` の Worker がヒープを使い尽くした後、主スレッドが `error` の `ERR_WORKER_OUT_OF_MEMORY` と `exit` の 1 を受け取り、0.3 秒で 3 で終わった。
主スレッドは止まらなかった。
