# kir-fixtures

KIR の各バージョンのフィクスチャである(IRSpec 14 節)。
マイナーバージョンごとにディレクトリを 1 つ置き、そのバージョンのライタが書き出した正しいプログラムを入れる。
後のマイナーバージョンのリーダも、前のマイナーバージョンのフィクスチャをすべて読めなければならない。
そのため、フィクスチャは書き換えずに残し、新しいマイナーバージョンを定めたときは、そのバージョンのディレクトリを追加する。

## 1.0

| フィクスチャ | 作成元 | 内容 |
| --- | --- | --- |
| `closure`、`dict`、`handle`、`loop` | `doc/ir/examples/*.kel` | IRSpec の例。変数のキャプチャ、辞書、ハンドラ、末尾再帰 |
| `strings` | `crates/kir/tests/fixtures/src/strings.kel` | 文字列リテラルと数値の表示 |
| `never-ops` | `diktor/test/never_ops.t` の 10 番目のヒアドキュメント | `never` の印が付いた操作 |
| `sample` | `diktor/test/sample/stubs.kel`、`sample.kel` | 言語仕様のサンプルプログラムの全体 |
| `import` | `diktor/test/import.t` の `dia/` のファイル群 | 複数の単位(コンパイル単位)と初期化の順序 |
| `ops` | `1.0/ops.kel` | 式の `not` と `or`、範囲外の数値リテラル(`lit_error`。実行すると終了コード 3 で終了する)、型クラスの制約を持つ値の束縛(初期化手順 `def_dictabs`) |
| `loop-noncanonical.kir` | `loop.kir` | セクションのサイズをすべて 5 バイトの LEB128 で書き、スキップできるセクション 0x80、0x81、0x82 を追加した、正準でないバイナリ。対になる `.kirt` はない |

`ops.kir` と `ops.kirt` は、KIR 1.0 を書き出す Diktor が、`1.0/` の中で `ops.kel` から書き出したものである(`--emit-ir` と `--emit-ir-text`)。
`ops` は、ほかのフィクスチャに現れない構文を補う。

`loop-noncanonical.kir` と `ops` 以外の `.kir` と `.kirt` は、`crates/kir/tests/fixtures/` にある同じプログラムのファイル(Diktor 2f9300a の `--emit-ir` と `--emit-ir-text` の出力)の、ヘッダのバージョンを 1.0 に書き換えたものである。
KIR 1.0 を書き出す Diktor で同じ入力から書き出し直すと、単位の `paths` のファイル名以外はバイト単位で一致する。

## 確かめ方

`1.0/fixtures.t` は、次のことを確かめる。

- 各フィクスチャが `keleut kir verify` に通る。
- `.kir` と `.kirt` を、`keleut kir dis` と `keleut kir asm` で相互に変換できる。
- 各 `.kir` を実行した結果が期待出力と一致する。
- マイナーバージョンを 1 にしてスキップできるセクションを追加したファイルを、1.0 のリーダが実行できる。

`kconform --suite all` は、ペア `dk`、`dk-ir`、`kf`、`x-ir` で `keleut run FILE.kir` を `diktor --run-ir` に渡すので、Diktor のリーダも同じフィクスチャを読む。
`kconform kir-roundtrip` は、コーパスを確かめた後に、このディレクトリの下のすべての `.kir` を `diktor --run-ir` と `keleut run` で実行して出力を比較し、すべての `.kir` と `.kirt` を `keleut kir verify` で検証する。

```sh
target/release/kconform expect conformance/kir-fixtures/1.0/fixtures.t
target/release/kconform kir-roundtrip
```
