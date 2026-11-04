# 統括者の探り(計画 261104-3 の作成中)

- 日付: 2026-11-04(16:00〜16:27)。負荷の平均は 0.02〜2.8 だった。
- 版: 親 69febfd、diktor cccd8cb。`rustkeleut/target/release/keleut`(11:30 の組み)、Node 26.9.0。
- `c92.kel` は `../gaps/cancelg.t` のヒアドキュメント(形 c、`NV = 92`、`GADD=1`、N = 510,000)である。
  `c92m.kel` と `c92n.kel` は、その `go(510000)` を `sed` で `go(299000)` と `go(280000)` に替えたものである。

## 1. `keleut run`

| 入力 | 選択肢 | 出力 | 終了状態 | 時間 |
| --- | --- | --- | --- | --- |
| `c92.kel` | `--backend wasm --max-depth 1048576` | `2`、`keleut: 内部エラー: RangeError: Maximum call stack size exceeded` | 70 | 約 8 秒 |
| `c92.kel` | `--backend wasm --max-depth 1900000` | `2`、`bottom`、`7` | 0 | 約 9 秒 |
| `c92m.kel` | `--backend wasm --max-depth 600000` | `2`、`keleut: 内部エラー: RangeError: Maximum call stack size exceeded` | 70 | 3 秒 |
| `c92m.kel` | `--backend interp --max-depth 600000` | `2`、`bottom`、`7` | 0 | 2 秒 |
| `c92n.kel` | `--backend wasm --max-depth 600000` | `2`、`bottom`、`7` | 0 | 4 秒 |
| `deep.kel` | `--backend wasm --max-depth 2000000` | `1990000` | 0 | |
| `deep.kel` | `--backend wasm --max-depth 4000000` | `1990000` | 0 | |

- `--max-depth 1900000` と、`deep.kel` の 2 つは、スタックが `STACK_SIZE_MAX_KIB`(2,097,151 KiB)を超えるので、`stack_plan` が `None` を返し、host の `Worker` の道で走る。
- `--max-depth 600000` のスタックは `(600,000 + 9,375) × 1,536 + 32 MiB` ≒ 969.5 MB で、1 つの N あたり 3,365 バイト(gaps.md 1.3 節)なら N ≒ 288,000 が境である。
  N = 280,000 は通り、N = 299,000(論理的な深さ約 598,000)は通らなかった。

## 2. Diktor

| 入力 | コマンド | 結果 |
| --- | --- | --- |
| `c92.kel` | `diktor c92.kel`(木の評価器) | 600 秒の `timeout` で止まった(124) |
| `c92.kel` | `diktor --emit-ir c92.kir c92.kel` の後の `diktor --run-ir c92.kir` | 13 分 31 秒の時点で常駐 6.3 GB、出力なし。止めた |

この入力は Diktor を基準にする組では試験にできない。

## 3. 閉じた標準出力を Node の前に見分ける起動の台本(検証の後に追加)

`launcher.sh` は、fd 1 を fd 3 に複製できるかで閉じた標準出力を見分ける台本の試作である(指摘 facts-0、feasible-0 への答え)。
`: >&1` は bash で閉じた fd 1 を見分けなかったので、`true 3>&1` を使う。

| コマンド | 結果 |
| --- | --- |
| `bash launcher.sh >&-` | 文面を標準エラーに書いて 74 |
| `bash launcher.sh >/dev/null` | 0 |
| `bash launcher.sh \| cat` | `ok`、0 |

この機械の `/bin/sh` は bash である。
dash では試していない。
