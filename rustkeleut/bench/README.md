# bench

RustKeleut の後段と Diktor の実行時間を測る道具である(計画 261024-1 の M93)。
測った値と、そこから決めたことは `doc/log/261028-1-m93.md` にある。

## 置き場所

| 場所 | 中身 |
| --- | --- |
| `programs/*.kel` | 計測のプログラム。どれも標準出力に結果を書くだけで、入力を読まない |
| `run.py` | `programs/` の各プログラムを設定ごとに何回か走らせ、経過時間の中央値と最小値を表にする |
| `corpus.py` | `conformance/corpus/` の小さなプログラムをすべて走らせ、設定ごとの合計の経過時間を出す |

## プログラム

| 名前 | 測るもの |
| --- | --- |
| `hello` | 起動と終了の費用 |
| `fib` | `Int32` の末尾でない再帰(`fib(32)`) |
| `tail32` | `Int32` の 2 引数の末尾再帰 1,000 万回と、相互の末尾再帰 1,000 万段 |
| `tail32b` | `tail64` と同じ本体を `Int32` で書いた末尾再帰 1,000 万回(`tail64` と比べて箱の費用を見る) |
| `args6` | 6 引数の末尾再帰 1,000 万回(引数の渡し方の費用) |
| `tail64`、`tailf64` | `Int64` と `Float64` の引数の末尾再帰 1,000 万回(境目の箱の費用) |
| `tailresume` | T の節の末尾 `resume` を 500 万回 |
| `gen` | G の節の生成器(1,000 回の yield を 1,000 回) |
| `gdepth1`〜`gdepth1000` | 深さ 1、10、100、1,000 の再帰の底からの yield を 2 万回(継続の写しの費用と深さの関係) |
| `abort` | N の節への脱出を、深さ 10 で 20 万回、深さ 1,000 で 2,000 回 |
| `records` | レコードの作り直し、`with` の更新、レコードのリストの走査 |
| `strings` | 文字列の連結、部分文字列、比較、`show` |
| `typecheck` | let 多相つきの小さな型推論器で、生成した式 12,000 個を検査する(コンパイラに近い負荷) |

## 使い方

`rustkeleut/` で次のように走らせる。
`keleut` は `cargo build --release` で組んだものを使う(LLVM の後段の設定を測るため、既定の feature `llvm` を外さない)。

```sh
python3 bench/run.py --diktor ../diktor/_build/install/default/bin/diktor \
  --configs ki,kj,kt1000,kw,dk --repeat 5 --json /tmp/bench.json
python3 bench/corpus.py --diktor ../diktor/_build/install/default/bin/diktor --configs ki,kt1,kt1000
```

設定の名前は次のとおりである。

| 設定 | コマンド |
| --- | --- |
| `ki` | `keleut run --backend interp` |
| `kj` | `keleut run --backend jit --tier jit-only` |
| `ktN` | `keleut run --backend jit --tier threshold=N`(`N` は 1、10、100、1000、10000、100000) |
| `kaS`、`kmS` | `--tier alternating=S` と `--tier mixed=S`(`S` は 1 か 2) |
| `kd` | `keleut run`(既定の後段と段) |
| `kw` | `keleut run --backend wasm` |
| `dk` | `diktor FILE.kel`(木の評価器。構文解析と型検査を含む) |
| `dkir` | `diktor --run-ir FILE.kir` |
| `dktc` | `diktor --type-check FILE.kel`(`dk` に含まれる型検査の時間の目安) |

- KIR は `diktor --emit-ir` で一時のディレクトリに書き、終われば消す。
  `--work DIR` を渡すと、そこに書いて残す。
- `dk`、`dkir`、`dktc` は遅いので、回数を `--slow-repeat`(既定 3)にする。
- どの設定の標準出力も、最初の設定の標準出力と同じでなければならない。
  違うか、終了状態が 0 でなければ、表の値に「誤」を付け、終了状態 1 で終わる。
- 既定では、1 つの設定の回を続けて走らせてから次の設定に移る。
  `--interleave` を渡すと、回ごとに全設定を 1 回ずつ順に回し、負荷の揺れを設定の間に散らす。
  `--warmup N` を渡すと、測る前に各設定を `N` 回走らせて捨てる。
- 表の値は経過時間の中央値(ミリ秒)である。
  `--json` は、すべての標本(経過時間、終了状態、標準出力の要約、最大の常駐量)を書く。
- 先頭の行に、日時、CPU、論理コアの数、カーネル、1 分の負荷の平均を書く。
  ほかの作業がコアを使っていると値が揺れるので、負荷の平均が 1 を超えるときは測り直す。
  測り直せないときは、負荷の平均を記録し、`--interleave` で設定を回ごとに回して、数 % の差を誤差とみなす。
- 回ごとに回しても、数 ms の計測や 1 回だけ外れた標本で、中央値から 30 % ほど離れる設定がある。
  中央値と最小値の両方を見て比べる。
