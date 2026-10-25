# conformance

RustKeleut の差分試験の材料である。
走らせるのは `crates/kconform` の `kconform` で、コマンド行の形は計画 261024-1 の 8.1 節に従う。

## 置き場所

| 場所 | 中身 |
| --- | --- |
| `known-diffs.toml` | 既知の差の一覧。ここに無い差が 1 件でもあれば `kconform` は失敗する |
| `corpus/` | `kconform extract-corpus` が `diktor/test/*.t` のヒアドキュメントから抜き出した `.kel` と、その索引 `index.tsv` |
| `kir-bad/` | 不正な KIR。`keleut kir` がどれも 65 で拒否することを確かめる |
| `init/`、`prim/`、`bytes/` | `keleut run` の試験(初期化、プリミティブ、出力の規約)。記録は `keleut run` の結果である |
| `depth/` | 論理的な深さの試験(IRSpec 11 節)。`keleut run --max-depth N` で走らせ、`--suite depth` で組 `dk-ir` と RustKeleut の組の間で比べる |
| `oom/` | 1 回の確保の上限 `N_alloc_max` を超える確保がメモリの不足になり、`cancel` 節を走らせて 3 で終わる試験。記録は `keleut run` の結果である |
| `frontend-depth/` | Rust のフロントエンド(crate `keleut-front`)の入れ子の限度の試験。Diktor が基準にならないので `kconform expect` でだけ走らせる |

`effects/`、`float/`、`kir-fixtures/` は、それぞれを受け入れるマイルストーンで足す。
どれも cram の形の `.t` で書き、`.kel` は `.t` の横か `.t` のヒアドキュメントに置く。
`.t` のコマンドは `diktor` でなく `keleut` を呼ぶ。
`keleut run` の呼び出しは、shim が組ごとに振り分ける(次の「組」の節の表の右の欄)。

## 組

組は KIR の書き手と実行系の組である(計画 8.2 節)。
`--pairs` を省くと、いま使える組をすべて走らせる。
基準の組は、`--pairs` に書いた順に依らず、`dk` があれば `dk`、無ければ `dk-ir`、それも無ければ最初に挙げた組である。
ほかの組は基準の組と比べる。
要約の行の「基準」が、選んだ組を示す。
どの組も、`.t` に記録した期待とも比べる。

| 組 | `diktor` の実行の呼び出し | `keleut run` の呼び出し |
| --- | --- | --- |
| `dk` | 本物の diktor(木の評価器) | Diktor の対応する呼び出し |
| `dk-ir` | `diktor --emit-ir` の後の `diktor --run-ir` | 同じく |
| `kf` | 先に `keleut check` を通し、型検査で止まる入力はその結果を返す。通れば本物の diktor | Diktor の対応する呼び出し |
| `ki` | 本物の `diktor --emit-ir` が 0 なら `keleut run --backend interp --tier interp-only FILE.kir`、0 でなければその出力と終了状態 | `keleut run --backend interp --tier interp-only` |
| `x-ir` | `keleut emit-kir` の後の `diktor --run-ir` | 同じく |

組 `kf` は、Rust のフロントエンド(crate `keleut-front`。`keleut dump` と `keleut check` が使う)を Diktor と比べるために足した組である(M96)。
組 `ki` は Diktor が書いた KIR を RustKeleut のインタプリタで走らせ、後段だけを比べる。

`kf` と `ki` では、`--dump-tokens` と `--dump-ast` の呼び出しを `keleut dump` に、`--type-check` の呼び出しを `keleut check` に振り向ける。
`dk`、`dk-ir`、`x-ir` は、これらを本物の diktor に渡す。
`--type-check` を含まず `--strict-exhaustive` を含む呼び出しは、Diktor がプログラムを実行するので、組 `dk-ir` と `ki` では実行の呼び出しとして振り分ける。
Diktor の IR の旗(`--emit-ir`、`--run-ir`、`--max-depth` など)を含む呼び出しと `--repl` は、組 `x-ir` の書き出しを除いて本物の diktor に渡す。
`--extra=FLAG` は、組 `ki` の `keleut run` のすべての起動に `FLAG` を足す。
`keleut run` を Diktor で走らせる組(`dk`、`dk-ir`、`kf`、`x-ir`)では、テキストの KIR(`.kirt`)の実行を shim が断り、4 で終わる。
`diktor --run-ir` はバイナリの KIR しか読まないからである。

## スイート

`--suite all`(既定)は、`diktor/test/*.t` の全ブロック、コーパスの全件、`conformance/` のうち `corpus/`、`depth/`、`frontend-depth/` を除く全部の `.t` を走らせる。
コーパスには記録が無いので、組が 2 つ以上のときだけ走らせ、組の間でだけ比べる。
`--suite depth` は `conformance/depth/` だけを走らせる。
深さの試験は `dk-ir` と RustKeleut の組で比べ、`dk` とは比べない(計画 8.2 節)。
そのため `--pairs` を省くと `dk` と `kf` を除いた組を走らせ、`dk-ir` を基準にする。
`kf` も Diktor の木の評価器で走り、`keleut run --max-depth` の限度を持たないからである。
`--pairs` に `dk` か `kf` を書くと 64 で終わる。
深さの試験では、どの組でも終了状態 70(予備の検査)を失敗にする。
どのスイートでも、走らせる試験が 1 つも無ければ 1 で終わる。
`FILES` を与えたときは、スイートの代わりにその `.t` と `.kel` を走らせる。

## 既知の差

既知の差の一覧は TOML の一部の形で書く。
`kconform` は `known-diffs.toml` と、その横の `known-diffs-*.toml` を名前の順に読む(`--known-diffs FILE` を与えたときはそのファイルだけ)。

```toml
[[diff]]
file = "diktor/test/run_exit.t"   # 試験のパスの末尾。コーパスなら conformance/corpus/DIR/NNNN-NAME.kel
line = 168                        # `  $ ` の行の番号。コーパスは 1
pairs = ["dk-ir", "ki"]           # 差のある組(`*` はすべての組)
reason = "限度は I(§15.5)"
ledger = "V213"
when = "unimplemented"            # 省略できる
against = "record"                # 省略できる。"record" か "pair"
```

組の間の差は、基準でない側の組の名前で照らし合わせる。
RustKeleut の後段の組(`ki kj kt ka km kw`。`ka` と `km` は種ごとの実体)は、既知の差の一覧に関係なく、互いに完全に一致しなければならない(D499)。
`kconform` は、これらの組のうち誤りの無い最初の組を基準にし、ほかの組の結果を正規化せずに比べる。
後段の間の差は、次の一時の例外を除いて、すべて登録の無い差に数える。
例外は、`when = "unimplemented"` の項目が差のある組を名前で挙げ(`*` では当たらない)、`against` が `"record"` でないときである。
このとき、その組の結果が `keleut: 未実装` の行を含み、その行より前の行が基準の後段の組の結果の先頭の行と正規化をせずに一致すれば、項目が当たる。
計画の M94 は、後段 `kw` が G の節を持つプログラムを 4 で止めるブロックを登録し、M95 でその登録を消す。
D499 の文言どおりではこの登録ができないので、この例外を M88 で足した。
M95 でこの例外を消す。
`when = "unimplemented"` の項目は、その組の結果が `keleut: 未実装` の行を含み、その行より前の行が比べる相手の先頭の行と(正規化の後で)一致するときだけ当たる。
未実装の機能に届いて 4 で終わるブロックの一時の登録に使う。
未実装に届く前の出力の違いと、未実装に届かなくなったブロックの差は隠さない。
`against` は項目が許す比較を決める。
`against = "record"` の項目は記録との差だけに、`against = "pair"` の項目は基準の組との差だけに当たり、省くと両方に当たる。
記録が `keleut run` の結果である `conformance/bytes/` では、記録と違うのは Diktor で走らせる組で、基準の組 `dk` と違うのは組 `ki` なので、2 つを分けて登録する。
`kconform expect` は記録とだけ比べるので、`against = "pair"` の項目を使わない。

項目の行がコマンドの行を指していなければ、`kconform` は 64 で終わる。
次の項目も誤りで、`kconform` は 64 で終わる。
組の名前が `*`、計画 8.2 節の組の名前、`kr-` で始まる名前のどれでもないもの、台帳の番号が `V` と数字の形でないもの、ファイル、行、組、`when` が同じで許す比較が重なる項目である(ファイルをまたいでも調べる)。
`FILES` を与えずにスイートを走らせるときは、どの試験のファイル(`diktor/test`、`conformance/` の全部の `.t`、コーパス)にも当たらない項目も誤りにする。
`--verbose` は、当たった既知の差、使われなかった既知の差、正規化の前だけの不一致を印字する。

M88 は、評価が `handle` に届くブロックを `known-diffs-unimplemented.toml` に `when = "unimplemented"` で一時に登録した(台帳 V219)。
M89 でエフェクトを入れて、このファイルを消した。
`kconform --pairs dk,ki --suite all` は、このファイルなしで差 0 件である。

比べる前に、期待と結果の両方の行を計画 8.1 節の規則で正規化する。
診断の種別の語(型エラー、実行時エラーなど)に `: ` が続けばその後ろを切り、先頭が `⚠ ` の行は `⚠` だけにする。

## 出力の整え方

各コマンドの出力は、dune 3.21.1 の cram と同じ順に整える。
まず端末の色の指定を除く。
次に、`/` で始まり空白、改行、CR、タブ、`:` を含まない語ごとに、語の頭が `TMPDIR` なら `$TMPDIR` に、作業ディレクトリなら `$TESTCASE_ROOT` に 1 度だけ置き換える。
語の途中に現れるパスは置き換えない。
語の切り方と `$TESTCASE_ROOT` への置き換えは dune と同じである。
`$TMPDIR` への置き換えは kconform だけの正規化である。
dune の試験のシェルの `TMPDIR` は dune の作る `/tmp/build_*_dune` で、dune はこれを置き換えない。
最後に、シェルの誤りの文面の頭のスクリプトの位置を除く。

行は LF で切り、LF の直前の CR を 1 つ落とす。
LF の無い最後の部分は、CR 1 つだけなら捨てる。
これは `diktor/test/via_ir_run.py` と同じ規則である。

`.t` の読み方も dune 3.21.1 の cram と同じで、`via_ir_run.py` の `parse_cram` と同じく行ごとに 3 つの状態を移る。
注釈の行の後に続く 2 つの空白で始まる行と、コマンドの出力の後の空白 1 つだけの行に `  $ ` の行が続くときのその行は、どのコマンドにも付かない。
dune はこれらの行を捨てるので、kconform もこれらを記録との差として報告する。

## コーパス

`corpus/` は生成物だが、リポジトリに置く。
`--suite all` と `kconform kir-roundtrip` が、同じ版のコーパスを使うためである。
`diktor/test` を変えたら `kconform extract-corpus` で作り直す。
`kconform extract-corpus --check` は、コーパスが `diktor/test` と食い違えば 1 で終わる。

`index.tsv` の各行は、コーパスの中のパス、出所(`試験.t:行`)、旗である。
旗は、同じ `.t` の後のコマンドのうち、そのファイルを引数に取る最初の `diktor` の呼び出しから取る。
そのような呼び出しが無いファイルは、旗の欄を `?` にする。
`--suite all` は、各ファイルを `diktor 旗 FILE` の 1 つのコマンドとして走らせる(旗が `?` なら旗なし)。
`kir-roundtrip` はそのファイルを旗なしで書き出し、型検査の誤り(1)なら `--no-prelude` で試し直す。

## 主な使い方

`rustkeleut/` で次のように走らせる。

```sh
cargo build --release
target/release/kconform --pairs dk,ki --suite all              # Diktor と keleut run を比べる
target/release/kconform                                        # 使える組をすべて走らせる
target/release/kconform --pairs dk --check-shim ../diktor/test/*.t  # shim が本物の diktor に渡すことの自己検査
target/release/kconform expect conformance/kir-bad/*.t         # 記録した期待とだけ比べる
target/release/kconform kir-roundtrip                          # コーパスの全件で KIR を往復させる
target/release/kconform extract-corpus                         # コーパスを作り直す
```

本物の diktor は、`--diktor` か環境変数 `KELEUT_DIKTOR` で指す。
どちらも無ければ `../diktor/_build/install/default/bin/diktor` を使う。
