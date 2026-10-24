# conformance

RustKeleut の差分試験の材料である。
走らせるのは `crates/kconform` の `kconform` で、コマンド行の形は計画 261024-1 の 8.1 節に従う。

## 置き場所

| 場所 | 中身 |
| --- | --- |
| `known-diffs.toml` | 既知の差の一覧。ここに無い差が 1 件でもあれば `kconform` は失敗する |
| `corpus/` | `kconform extract-corpus` が `diktor/test/*.t` のヒアドキュメントから抜き出した `.kel` と、その索引 `index.tsv` |
| `kir-bad/` | 不正な KIR。`keleut kir` がどれも 65 で拒否することを確かめる |

`init/`、`prim/`、`bytes/`、`effects/`、`float/`、`depth/`、`frontend-depth/`、`oom/`、`kir-fixtures/` は、それぞれを受け入れるマイルストーンで足す。
どれも cram の形の `.t` で書き、`.kel` は `.t` の横か `.t` のヒアドキュメントに置く。

## コーパス

`corpus/` は生成物だが、リポジトリに置く。
`--suite all` と `kconform kir-roundtrip` が、同じ版のコーパスを使うためである。
`diktor/test` を変えたら `kconform extract-corpus` で作り直す。
`kconform extract-corpus --check` は、コーパスが `diktor/test` と食い違えば 1 で終わる。

`index.tsv` の各行は、コーパスの中のパス、出所(`試験.t:行`)、旗である。
旗は、同じ `.t` の後のコマンドのうち、そのファイルを引数に取る最初の `diktor` の呼び出しから取る。
そのような呼び出しが無いファイルは、旗の欄を `?` にする。
`kir-roundtrip` はそのファイルを旗なしで書き出し、型検査の誤り(1)なら `--no-prelude` で試し直す。

## 主な使い方

`rustkeleut/` で次のように走らせる。

```sh
cargo build --release
target/release/kconform --pairs dk ../diktor/test/*.t   # shim が本物の diktor に渡すことの自己検査
target/release/kconform expect conformance/kir-bad/*.t  # 記録した期待とだけ比べる
target/release/kconform kir-roundtrip                   # コーパスの全件で KIR を往復させる
target/release/kconform extract-corpus                  # コーパスを作り直す
```

本物の diktor は、`--diktor` か環境変数 `KELEUT_DIKTOR` で指す。
どちらも無ければ `../diktor/_build/install/default/bin/diktor` を使う。
