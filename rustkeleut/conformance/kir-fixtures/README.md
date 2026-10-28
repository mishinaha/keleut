# kir-fixtures

KIR の各版の見本である(IRSpec 14 節)。
副版ごとにディレクトリを 1 つ置き、その版の書き手が書いた正しいプログラムを入れる。
後の副版の読み手も、前の副版の見本をすべて読まなければならない。
見本を書き換えずに残し、新しい副版を定めたときは、その副版のディレクトリを足す。

## 1.0

| 見本 | 元 | 中身 |
| --- | --- | --- |
| `closure`、`dict`、`handle`、`loop` | `doc/ir/examples/*.kel` | IRSpec の例。捕獲、辞書、ハンドラ、末尾の再帰 |
| `strings` | `crates/kir/tests/fixtures/src/strings.kel` | 文字列のリテラルと数値の表示 |
| `never-ops` | `diktor/test/never_ops.t` の 10 番目のヒアドキュメント | `never` の印の操作 |
| `sample` | `diktor/test/sample/stubs.kel`、`sample.kel` | 言語仕様の見本の全体 |
| `import` | `diktor/test/import.t` の `dia/` のファイル群 | 複数の単位と初期化の順 |
| `ops` | `1.0/ops.kel` | 式の `not` と `or`、範囲外の数値リテラル(`lit_error`。実行は 3 で終わる)、型クラスの制約を持つ値の束縛(初期化の `def_dictabs`) |
| `loop-noncanonical.kir` | `loop.kir` | 節の大きさをすべて 5 バイトの LEB128 で書き、読み飛ばせる節 0x80、0x81、0x82 を足した正準でないバイナリ。対の `.kirt` は無い |

`ops.kir` と `ops.kirt` は、版を 1.0 にした Diktor が `1.0/` の中で `ops.kel` から書いたものである(`--emit-ir` と `--emit-ir-text`)。
ほかの見本に現れない構文を補うために足した。

`loop-noncanonical.kir` と `ops` のほかの `.kir` と `.kirt` は、`crates/kir/tests/fixtures/` の同じプログラムの組(Diktor 2f9300a の `--emit-ir` と `--emit-ir-text` の出力)の、版の見出しを 1.0 に書き換えたものである。
版を 1.0 にした Diktor で同じ入力から書き出し直すと、単位の `paths` のファイル名のほかはバイト単位で一致する。

## 確かめ方

`1.0/fixtures.t` は、各見本が `keleut kir verify` に通ること、`.kir` と `.kirt` が `keleut kir dis` と `keleut kir asm` で互いに移ること、各 `.kir` を実行した結果を確かめる。
副版を 1 にして読み飛ばせる節を足したファイルを 1.0 の読み手が実行できることも確かめる。
`kconform --suite all` は、組 `dk`、`dk-ir`、`kf`、`x-ir` で `keleut run FILE.kir` を `diktor --run-ir` に渡すので、Diktor の読み手も同じ見本を読む。
`kconform kir-roundtrip` は、コーパスの確認の後に、このディレクトリの下のすべての `.kir` を `diktor --run-ir` と `keleut run` で走らせて出力を比べ、すべての `.kir` と `.kirt` を `keleut kir verify` に通す。

```sh
target/release/kconform expect conformance/kir-fixtures/1.0/fixtures.t
target/release/kconform kir-roundtrip
```
