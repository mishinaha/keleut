# conformance

RustKeleut の差分テストのテストデータである。
テストは `crates/kconform` の `kconform` で実行する。
コマンド行の例は「主な使い方」の節に示す。

## ファイル構成

| パス | 内容 |
| --- | --- |
| `known-diffs.toml`、`known-diffs-*.toml` | 既知の差分の一覧。`kconform` は `known-diffs.toml` と、同じディレクトリの `known-diffs-*.toml` を読み、どれにも登録のない差分が 1 件でもあれば失敗する。`known-diffs-*.toml` は一時的な登録に使うファイルで、現在は 1 つもない |
| `corpus/` | `kconform extract-corpus` が `diktor/test/*.t` のヒアドキュメントから抽出した `.kel` と、その一覧 `index.tsv` |
| `kir-bad/` | 不正な KIR。`keleut kir` がどれも終了コード 65 で拒否することを確かめる |
| `kir-fixtures/` | KIR の各バージョンのフィクスチャ(IRSpec 14 節)。マイナーバージョンごとのディレクトリ(現在は `1.0/`)に、Diktor と RustKeleut の両方のリーダが読む正しい `.kir` と `.kirt` を置く。`--suite all` のペア `dk`、`dk-ir`、`kf`、`x-ir` では `keleut run FILE.kir` が `diktor --run-ir` になるので、Diktor のリーダも同じフィクスチャを読む。`kconform kir-roundtrip` もフィクスチャを両方のリーダで読む |
| `init/`、`prim/`、`bytes/` | `keleut run` のテスト(初期化、プリミティブ、標準出力への出力の規則)。期待出力は `keleut run` の結果である |
| `float/` | `Float64` のリテラルの読み取りと、境界値の表示のテスト。範囲を超えるリテラル、最小の非正規化数と最大の有限値の前後の丸め、接尾辞 `f64` と桁区切り、構文エラーになる表記(1 件につき 1 ファイル)を確かめる。`a * b + c` を引数の値で計算しても定数の式で計算しても丸めが変わらないことも確かめる。期待出力は `keleut run` の結果で、Diktor の構文木の評価器(構文木を直接たどって評価する。`--run-ir` も `--via-ir` も付けない実行)とも一致する |
| `depth/` | 論理的な深さのテスト(IRSpec 11 節)。`keleut run --max-depth N` で実行し、`--suite depth` でペア `dk-ir` と RustKeleut のペアを比較する |
| `oom/` | メモリ不足のときに `cancel` 節を実行して終了コード 3 で終了することのテスト。`oom.t` は 1 回の確保の上限 `N_alloc_max` を超える確保を、`shape.t` はシェイプ表を拡張できない場合を確かめる。期待出力は `keleut run` の結果である。実行の方法と既知の差分は、表の後の `oom/` の説明に書く |
| `gc/` | GC のテスト。期待出力は `keleut run` の結果で、Diktor の構文木の評価器とも一致する。確かめる内容は、表の後の `gc/` の説明に書く |
| `stack/` | ネイティブスタックのテスト。`--native-stack 512k` で、JIT コンパイルした関数が非末尾の再帰を深く重ねた後に行う構造的な `Eq` の比較と、20 万段の入れ子になった値どうしの構造的な `Eq` の比較が、どのバックエンドでも同じ結果になることを確かめる。期待出力は `keleut run` の結果で、Diktor の構文木の評価器とも一致する |
| `frame/` | スロットの多い関数のフレームのテスト。`slots.t` は、300 個の `Int32` を生存させたまま `id` を呼ぶ関数を `awk` で生成し、スロット数が 256 を超える関数がすべてのペアで同じ結果になることを確かめる。期待出力は `keleut run` の結果で、Diktor の構文木の評価器とも一致する |
| `effects/` | G の節(IRSpec 9.2 節の分類)の継続のキャプチャと再開のテスト(`g.t`)と、ティアの境界をまたぐ継続のキャプチャと再開のテスト(`tier.t`)。`g.t` は、生成器、深い再帰の底の yield、2 回目の resume、再開の合間の GC、G と N と T の節の組み合わせ、メソッド呼び出しと引数の多い関数の中の yield、組み込みの `Array.each` と `pinned` の中の yield と Never を返す操作を扱う。`tier.t` は、ハンドラ式の評価と継続の再開を数千回繰り返し、ティアを切り替えるペア(`kt`、`ka`、`km`)で継続のキャプチャと再開がティアの境界をまたぐようにする。期待出力は `keleut run` の結果で、Diktor の構文木の評価器とも一致する |
| `recshow/` | レコードを含む `<値>` の表記のテスト(IRSpec 付録 B)。各値を、`cancel` 節の抑制の記録の行と、最上位の実行時エラーの行の 2 つの形式で書く。期待出力は `keleut run` の結果である。実行の方法と比較の方法は、表の後の `recshow/` の説明に書く |
| `frontend-depth/` | Rust のフロントエンド(crate `keleut-front`)の入れ子の上限のテスト(`parse.t` は `keleut dump ast`、`typecheck.t` は `keleut check`、`repl.t` は `keleut repl` で実行する)。Diktor の結果を期待値にできないので、`kconform expect` でだけ実行する |

どのテストも cram 形式の `.t` で書き、`.kel` は `.t` と同じディレクトリか、`.t` のヒアドキュメントに置く。
`.t` のコマンドは `diktor` でなく `keleut` を呼ぶ。
`keleut run` の呼び出しは、shim がペアごとに振り分ける(「ペア」の節の表の右の列)。

`Float64` は 2 つのディレクトリで確かめる。
`float/` はリテラルの読み取りと境界値の表示を、`prim/` は演算、libm、整数への変換を扱う。
値を文字列化する関数そのものは、crate `keleut-rtcore` のテスト `tests/fltcmp.rs` も、glibc の出力と比較して確かめる。

`oom/shape.t` は、レコードの拡張を 3,000 段つないだ関数を、仮想メモリを制限して(`ulimit -v 524288`)実行する。
`.kel` は制限をかけずに `keleut emit-kir` で `.kir` に変換し、制限をかけた状態では `.kir` だけを `--backend interp` で実行する。
そのため、`gc/` のコマンドと同じく、ペア `kw` と `kr-kw` でもインタプリタで実行される。
LLVM ペア(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)では shim がこのオプションを取り除いて JIT バックエンドを選ぶが、3,000 段の関数はコンパイルのコストが `JIT_COST_MAX` を超えるので、インタプリタで実行される。
JIT コードがレコードを拡張する経路 `krt_rec_op` は、`crates/keleut-cli/tests/oom_cmd.rs` の `--tier jit-only` のテストが 300 段の連鎖で確かめる。
Diktor は `oom.t` でも `shape.t` でも終了コード 0 で終了する。
そこで、Diktor で実行するペアと期待出力との差分と、RustKeleut のペアと基準のペア `dk` との差分を、既知の差分に登録してある(`oom.t` は台帳 V215、`shape.t` は台帳 V240)。

`gc/` は次のことを確かめる。

- 100 万要素のリストを、`--native-stack 512k` で作って走査できる。
- 仮想メモリを制限したとき(`ulimit -v 524288`)に、確保の合計が制限を超えるプログラムを最後まで実行でき、エフェクトの表の項目も解放される。
- 構造的な `Eq` の入れ子の比較の途中で GC を実行できる。
- バンプ割り当て中のページをちょうど使い切ったときに、スイープできる。
- 生きている浅い継続が、再利用する大きなコピー領域(継続が持つフレームのコピーを置く領域)を持ち続けない(`ulimit -v 1048576` で確かめる)。

`gc/` の `ulimit -v` を使う 3 つのコマンドは `--backend interp` を指定するので、ペア `kw` でもインタプリタで実行される。
Node は、仮想メモリを 512 MiB に制限すると起動できないからである(台帳 V215)。
LLVM ペアでは shim がこのオプションを取り除くので、ペアのバックエンドとティアの設定で実行される。

`recshow/` のうち、付録 B の 9 番(ハンドラのどの節にも一致しない)はソースから書けない。
そこで、手で書いた `nomatch.kirt` を `keleut kir asm` で `nomatch.kir` に変換して実行し、cram の中で `keleut kir dis` の出力が `nomatch.kirt` と一致することを確かめる。
`nomatch.kel` は `.kirt` を作り直すための元のソースで、どのコマンドも実行しない。
期待出力や Diktor と比較するときは、抑制の記録の行はフィールドを並べ替えてから比較するので、フィールドの順序以外の表記の違いを検出できる。
実行時エラーの行は種別名の後ろを切り捨てるので、値の表記の違いを検出できない。
バックエンドのグループ(`ki kj kt ka km kw` と `kr-*`)の中では正規化せずに比較するので、値を文字列化する 2 つの実装(crate keleut-rt の `Rt::show` と host の `show`)が同じ順序で書くことを検査できる。
この検査は、同じグループのペアを 2 つ以上同時に実行したときだけ行われるので、少なくとも `ki` と `kw`、`kr-ki` と `kr-kw` を同時に実行する。

## ペア

ペアは、KIR のライタ(KIR を書き出す処理系)と、その KIR を実行する処理系の組み合わせである。
kconform の出力と本書のコードブロックの中のコメントは、ペアを「組」、バックエンドを「後段」、テストを「試験」、既知の差分を「既知の差」、期待出力を「記録」、上限を「限度」、オプションを「旗」、ティアを「段」と書く。
`--pairs` を省くと、現在使えるペアをすべて実行する。
`--no-default-features` でビルドした `keleut` では LLVM ペア(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)を除き、除いたペアを標準エラーに書く(本節のペア `kj` の説明を参照)。
基準のペアは、`--pairs` に書いた順序によらず、`dk` があれば `dk`、なければ `dk-ir`、それもなければ最初に挙げたペアである。
ほかのペアは基準のペアと比較する。
要約の行の「基準」が、選んだペアを示す。
どのペアも、`.t` に書いた期待出力とも比較する。

| ペア | `diktor` の実行の呼び出し | `keleut run` の呼び出し |
| --- | --- | --- |
| `dk` | 本物の diktor(構文木の評価器) | Diktor の対応する呼び出し |
| `dk-ir` | `diktor --emit-ir` の後の `diktor --run-ir` | 左の列と同じ |
| `kf` | 先に `keleut check` を実行し、型検査でエラーになる入力はその結果を返す。型検査に通れば本物の diktor に渡す | Diktor の対応する呼び出し |
| `ki` | `keleut compat-diktor --frontend diktor --backend interp --tier interp-only --` に同じ引数を渡す。`keleut run --frontend diktor` が PATH の `diktor`(shim)を `--emit-ir` で呼び、shim はその呼び出しを本物の diktor に渡す。本物の `diktor --emit-ir` の終了コードが 0 でなければ、`keleut run` は同じ終了コードで終了し、出力は `diktor --emit-ir` が書いたものだけになる | `keleut run --backend interp --tier interp-only` |
| `kj` | `ki` と同じく、`keleut compat-diktor --frontend diktor --backend jit --tier jit-only --` に同じ引数を渡す | `keleut run --backend jit --tier jit-only` |
| `kt` | `ki` と同じく、`keleut compat-diktor --frontend diktor --tier threshold=1 --` に同じ引数を渡す | `keleut run --tier threshold=1` |
| `ka` | シード 1〜8 のそれぞれで、`keleut compat-diktor --frontend diktor --tier alternating=SEED --` に同じ引数を渡す | `keleut run --tier alternating=SEED` |
| `km` | シード 1〜4 のそれぞれで、`keleut compat-diktor --frontend diktor --tier mixed=SEED --` に同じ引数を渡す | `keleut run --tier mixed=SEED` |
| `kw` | `ki` と同じく、`keleut compat-diktor --frontend diktor --backend wasm --` に同じ引数を渡す | `keleut run --backend wasm` |
| `x-ir` | `keleut emit-kir` の後の `diktor --run-ir` | 左の列と同じ |
| `kr-ki` | `keleut compat-diktor --frontend rust --backend interp --tier interp-only --` に同じ引数を渡す | `keleut run --frontend rust --backend interp --tier interp-only` |
| `kr-kj` | `keleut compat-diktor --frontend rust --backend jit --tier jit-only --` に同じ引数を渡す | `keleut run --frontend rust --backend jit --tier jit-only` |
| `kr-kt` | `keleut compat-diktor --frontend rust --tier threshold=1 --` に同じ引数を渡す | `keleut run --frontend rust --tier threshold=1` |
| `kr-ka` | シード 1〜8 のそれぞれで、`keleut compat-diktor --frontend rust --tier alternating=SEED --` に同じ引数を渡す | `keleut run --frontend rust --tier alternating=SEED` |
| `kr-km` | シード 1〜4 のそれぞれで、`keleut compat-diktor --frontend rust --tier mixed=SEED --` に同じ引数を渡す | `keleut run --frontend rust --tier mixed=SEED` |
| `kr-kw` | `keleut compat-diktor --frontend rust --backend wasm --` に同じ引数を渡す | `keleut run --frontend rust --backend wasm` |

表の「`diktor` の実行の呼び出し」の列は、Diktor の実行の呼び出し(オプションなしか、`--prelude`、`--no-prelude`、`--import-path`、`--strict-exhaustive` だけを付けた呼び出し)の転送先である。
`keleut compat-diktor` は、`--` の前に置いたペアのオプションをバックエンドのオプションとして読み、`--` の後の Diktor のオプションを Diktor と同じように解釈して、`keleut run` の処理を実行する。
入力ファイルは、名前が `.kir` で終わっていても、Diktor と同じくソースとして読む。
UTF-8 でない引数を含む呼び出しは、どのペアでも本物の diktor に渡す。
keleut は UTF-8 でない引数を使い方のエラーにするからである。
`ki` から `kr-kw` までのペアでも、`--type-check` は `keleut check`、`--dump-tokens` と `--dump-ast` は `keleut dump`、`--repl` は `keleut repl` に転送し、使い方のエラーになる呼び出しは本物の diktor に渡す。

`keleut run` の既定のフロントエンドは Rust(`--frontend rust`)である。
Diktor が KIR を書くペア(`ki`、`kj`、`kt`、`ka`、`km`、`kw`)は、ペアのオプションの先頭に `--frontend diktor` を置く。
これらのペアでは、表の右の列の `keleut run` にも、このオプションが先頭に付く。
`.t` の中の `keleut run FILE.kel` は、`--frontend diktor` が付くと PATH の `diktor`(shim)を `--emit-ir` で呼び、shim はその呼び出しを本物の diktor に渡す。
kconform は `.t` を実行するシェルの環境から `KELEUT_DIKTOR` を取り除くので、`keleut run` は必ず shim を呼ぶ。
`kconform expect` が `keleut run` に追加するオプション(ペア `ki` と同じ)にも `--frontend diktor` が入っている。

ペア `kf` は、Rust のフロントエンド(crate `keleut-front`。`keleut dump` と `keleut check` が使う)を Diktor と比較する。
ペア `ki` は、Diktor が書いた KIR を RustKeleut のインタプリタで実行し、バックエンドだけを比較する。
ペア `kw` は、同じ KIR を WasmGC バックエンド(crate `keleut-wasm`。Node 26.9.0 以上で動く)で実行する。
`kw` の shim の振り分けは `ki` と同じである。

ペア `kj` は、同じ KIR を LLVM の JIT バックエンド(crate `keleut-llvm`)で実行する。
JIT コンパイルできる関数は、`handle`、`resume`、`perform` を含む関数も含めて、最初に呼ぶときにコンパイルし、JIT コードで実行する。
`kj` の shim の振り分けは `ki` と同じである。
ペア `kt`、`ka`、`km` は、同じ KIR を、ティアを切り替える設定で実行する。
`kt` は、インタプリタで 1 回呼んだ関数を、コンパイルスレッドで完了を待たずにコンパイルする。
`ka` は、JIT コンパイルした関数に入るたびに、シードから決まる擬似乱数でティアを選ぶ。
`km` は、シードで決まる半分の関数だけをコンパイルする。
これらのペアのオプションは `--tier` が `interp-only` でなく、`--backend` を含まないので、`keleut run` はバックエンド `jit` で実行する。
`ka` と `km` はシードごとに別の構成として実行し、既知の差分の一覧ではシードを付けない名前で照合する。

`kj` は、feature `llvm` を有効にしてビルドした `keleut` でだけ実行できる。
`llvm` は既定の feature なので、`cargo build --release` でビルドした `keleut` で実行できる。
`cargo build --release --no-default-features` でビルドした `keleut` では、どのコマンドも未実装を表す終了コード 4 で終了する。
`kt`、`ka`、`km` も同じである。
kconform は `keleut --version` の `backends:` の行で、`keleut` がバックエンド `jit` を持つかどうかを調べる。
持たないとき、`--pairs` を省いた実行はこれらのペアを除き、除いたペアを標準エラーに書く。
`--pairs` でこれらのペアを指定した実行は、使い方のエラー(終了コード 64)で終了する。

ペア `kr-ki` は、Rust のフロントエンドが書いた KIR をインタプリタで実行する。
KIR は `keleut run --frontend rust` がプロセスの中で書き、ファイルを経由しない。
ペア `kr-kj`、`kr-kt`、`kr-ka`、`kr-km`、`kr-kw` は、同じく Rust のフロントエンドが書いた KIR を、それぞれ `kj`、`kt`、`ka`、`km`、`kw` と同じバックエンドの設定で実行する。
`kr-ka` と `kr-km` は、`ka` と `km` と同じく、シードごとに別の構成として実行する。
`kr-kj`、`kr-kt`、`kr-ka`、`kr-km` は、`kj` と同じく feature `llvm` を有効にしてビルドした `keleut` でだけ実行でき、`--no-default-features` でビルドした `keleut` での扱いも `kj` と同じである。

`kw` で V8 のヒープそのものが尽きたときは、Node が SIGABRT で終了し、`keleut run` は終了コード 3 でなく 70 で終了する。
このとき `cancel` 節は実行されず、ホストのバッファに残っていた標準出力は失われる。
`ERROR(OutOfMemory)` と終了コード 3 になるのは、`N_alloc_max` を超える確保と、ホストが線形メモリを拡張できないためにシェイプ表を作れないか拡張できないときだけである。
`oom/oom.t` は前者を確かめ、後者は `crates/keleut-cli/tests/wasm_cmd.rs` が、テスト用に線形メモリの拡張を制限する `KELEUT_TEST_WASM_MEM_GROW` で確かめる。
ホストが文字列の受け渡しに使う 64 KiB のバッファは起動時に確保して拡張しないので、シェイプ表が線形メモリを使い切った後でも、`cancel` 節は出力できる。
V8 のヒープが尽きて終了コード 70 で終了することは、§15.5 との既知の相違(台帳 V215)で、既知の差分には登録していない。

LLVM ペア(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)でも、プロセスのメモリ(仮想メモリの制限、Rust と LLVM のヒープ)が尽きると、`keleut run` は終了コード 3 でなく SIGABRT(終了コード 134)で終了することがある。
このときも `cancel` 節は実行されず、バッファに残っていた標準出力は失われる。
異常終了する箇所は、LLVM の中の確保(ORC の `JIT session error: Cannot allocate memory` の後の確保の失敗)のことも、`Interp::fx_call` の `Vec` のように失敗を返さない小さな確保のこともある。
`ERROR(OutOfMemory)` と終了コード 3 を保証するのは、GC のヒープが尽きたときと、`N_alloc_max` を超える確保だけである。
V215 と同じく既知の相違として扱い、差分テストには入れず、既知の差分にも登録していない。

shim はペアのオプションを先に、コマンドの引数を後に並べ、`keleut run` は後に書いたオプションを優先する。
LLVM ペア(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)では、shim がコマンドの `--backend` を取り除き、ペアのバックエンドで実行する。
shim は、LLVM ペアかどうかを、ペアの名前でなくペアのオプションで判別する。
`--backend jit` があるか、`--backend` がなく `interp-only` でない `--tier` があれば、LLVM ペアである。
それ以外のペアでは、コマンドの `--backend` がペアのバックエンドより優先される。
どちらの場合も、ペアのバックエンドと違う `--backend` を書いたコマンドを、kconform は `組の後段の上書き` の行で報告する。
現在は、`gc/gc.t` の `ulimit -v` を使う 3 つのコマンドと、`oom/shape.t` の `ulimit -v` を使うコマンドが報告される。
これらのコマンドは、ペア `kw` ではインタプリタで実行されるので WasmGC バックエンドの比較にならず、LLVM ペアでは `--backend` を取り除かれて、ペアのバックエンドとティアの設定で実行される。

`dk`、`dk-ir`、`x-ir` を除くすべてのペア(`kf`、RustKeleut のバックエンドのペア、`kr-*`)は、`--dump-tokens` と `--dump-ast` の呼び出しを `keleut dump` に、`--type-check` の呼び出しを `keleut check` に転送する。
`dk`、`dk-ir`、`x-ir` は、これらを本物の diktor に渡す。
`--type-check` を含まず `--strict-exhaustive` を含む呼び出しは、Diktor がプログラムを実行するので、ペア `dk-ir` と `ki` では実行の呼び出しとして振り分ける。
Diktor の IR のオプション(`--emit-ir`、`--run-ir`、`--max-depth` など)を含む呼び出しは、ペア `x-ir` の KIR の書き出しを除いて、本物の diktor に渡す。
`--repl` の呼び出しは、RustKeleut のバックエンドのペア(`ki`、`kj`、`kw`、`kr-ki` など)では、ペアのオプションと `--no-prelude`、`--prelude` を付けて `keleut repl` に転送し、`dk`、`dk-ir`、`kf`、`x-ir` では本物の diktor に渡す。
`keleut repl` はペアのオプション(`--backend`、`--tier`、`--frontend`)を受け付けて無視し、Rust のフロントエンドとインタプリタで実行する。
そのため、どのバックエンドのペアでも REPL のブロックの出力は同じになり、バックエンドのペアが互いに完全に一致するという条件を REPL のブロックでも満たす。
ペア `ki` でも、REPL の KIR のライタは Rust のフロントエンドである。
入力ファイルを渡した `--repl` の呼び出し(使い方のエラー)は、どのペアでも本物の diktor に渡す。

`--extra=FLAG` は、RustKeleut のバックエンドのペア(`ki`、`kw`、`kr-ki` など)が実行するすべての `keleut run` と `keleut repl` に `FLAG` を追加する。
`keleut repl` では、`--native-stack` はフロントエンドのスレッドのスタックサイズを指定する。
リリースビルドでは `--extra=--native-stack 512k` で構文解析の入れ子の上限が 128 段に下がり、デバッグビルドでは `keleut repl` が使い方のエラー(終了コード 64)で終了する。
GC を重点的にテストするときは `--extra=--gc-stress` を付ける。
`keleut run --gc-stress` は、確保箇所(最上段のフレームの関数と点の組)を初めて通る確保では必ず GC を実行し、それ以外では、前回の GC の処理量を W として、確保を約 W / 64 回行うごとに GC を実行する。
この間隔には擬似乱数で変動を加える。
実行したどの確保箇所でも少なくとも 1 回は GC を実行するが、確保のたびに GC を実行するわけではない。
呼び出し元のフレームのルートがテストされるのは、そのフレームが中断している間に GC が起きたときだけである。
`--gc-stress` が有効なのはインタプリタと JIT(`--backend interp` と `jit`)だけである。
`keleut run` の `--native-stack` は、インタプリタと JIT のスタックのほかに、Rust のフロントエンドのスレッドと、KIR の読み込みと検証のスレッドのスタックサイズも決める。
`--backend wasm` はどちらのオプションも受け付けるが、バックエンドには使わない。
GC は V8 が行い、Node のスタックサイズは深さの上限 L から決めるからである。
そのため、`--extra=--gc-stress` を付けてもペア `kw` の GC はテストされない。
`keleut run` を Diktor で実行するペア(`dk`、`dk-ir`、`kf`、`x-ir`)では、テキストの KIR(`.kirt`)の実行を shim が拒否し、終了コード 4 で終了する。
`diktor --run-ir` はバイナリの KIR しか読めないからである。

## スイート

`--suite all`(既定)は、`diktor/test/*.t` の全ブロック、コーパスの全件、`conformance/` のうち `corpus/`、`depth/`、`frontend-depth/` を除くすべての `.t` を実行する。
コーパスには期待出力がないので、ペアが 2 つ以上のときだけ実行し、ペアの間でだけ比較する。
`--suite depth` は `conformance/depth/` だけを実行する。
`dk` と `kf` は Diktor の構文木の評価器で実行するので、`keleut run --max-depth` の上限が適用されない。
そのため、深さのテストは `dk-ir` と RustKeleut のペアで比較し、`--pairs` を省くと `dk` と `kf` を除いたペアを実行して `dk-ir` を基準にする。
`--pairs` に `dk` か `kf` を書くと終了コード 64 で終了する。
`--suite depth` を LLVM ペア(`--pairs` を省いたときの既定のペアにも含まれる)で実行するときは、`--jobs` を 6 程度にする。
32 コア、31 GB のマシンで既定の並列数(CPU の数)にすると、ペア `ka` と `kr-ka` の `depth/gresume.t` がメモリ不足になり、OOM killer に強制終了される(終了コード 137)。
深さのテストでは、どのペアでも、終了コード 70 を失敗として扱う。
スタック残量の検査で終了したときの終了コードだからである(原因は調べないので、ほかの内部エラーの 70 も失敗になる)。
どのスイートでも、実行するテストが 1 つもなければ終了コード 1 で終了する。
`FILES` を与えたときは、スイートの代わりにその `.t` と `.kel` を実行する。

## 既知の差分

既知の差分の一覧は、TOML のサブセットで書く。
`kconform` は `known-diffs.toml` と、同じディレクトリの `known-diffs-*.toml` を名前順に読む。
`--known-diffs FILE` を与えたときは、そのファイルだけを読む。

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

`ledger` に書く台帳の番号は、`doc/log` の課題の V 番号である。
ペアの間の差分は、基準でない側のペアの名前で照合する。
RustKeleut のバックエンドのペア(`ki kj kt ka km kw`。`ka` と `km` はシードごとの個々の構成)は、既知の差分の一覧に関係なく、互いに完全に一致しなければならない。
Rust のフロントエンドを使うペア(`kr-ki kr-kj kr-kt kr-ka kr-km kr-kw`。`kr-ka` と `kr-km` はシードごとの個々の構成)も、互いに完全に一致しなければならない。
`kconform` は、ライタごとに 2 つのグループ(`ki kj kt ka km kw` と `kr-*`)を作り、グループごとにエラーのない最初のペアを基準にして、ほかのペアの結果を正規化せずに比較する。
ライタの違うペア(`ki` と `kr-ki` など)は、診断メッセージの接頭辞が違ってよい(台帳 V217)ので、この方法では比較しない。
バックエンドの間の差分は、一覧のどの項目にも該当せず、すべて未登録の差分として数える。
`when = "unimplemented"` の項目も、バックエンドの間の差分には該当しない。
`when = "unimplemented"` の項目は、そのペアの結果が `keleut: 未実装` の行を含み、その行より前の行が比較対象の先頭の行と(正規化の後で)一致するときだけ該当する。
未実装の機能を実行しようとして終了コード 4 で終了するブロックを、一時的に登録するために使う。
未実装の機能に達する前の出力の違いと、未実装の機能に達しなくなったブロックの差分は隠さない。
現在は、`when` を持つ項目は 1 つもない。
`against` は、項目が許す比較を決める。
`against = "record"` の項目は期待出力との差分だけに、`against = "pair"` の項目は基準のペアとの差分だけに該当し、省くと両方に該当する。
期待出力が `keleut run` の結果である `conformance/bytes/` では、期待出力と違うのは Diktor で実行するペアで、基準のペア `dk` と違うのはペア `ki` なので、2 つを分けて登録する。
`kconform expect` は期待出力とだけ比較するので、`against = "pair"` の項目を使わない。

項目の行がコマンドの行を指していなければ、`kconform` は終了コード 64 で終了する。
次の項目もエラーで、`kconform` は終了コード 64 で終了する。

- ペアの名前が、`*`、「ペア」の節の表にある名前、`kr-` で始まる名前のどれでもない項目
- 台帳の番号が `V` と数字の形式でない項目
- ファイル、行、ペア、`when` が同じで、許す比較が重なる項目(一覧のファイルをまたいでも調べる)

`FILES` を与えずにスイートを実行するときは、どのテストのファイル(`diktor/test`、`conformance/` のすべての `.t`、コーパス)にも該当しない項目もエラーにする。
`--verbose` は、該当した既知の差分、使われなかった既知の差分、正規化の前だけの不一致を表示する。

`known-diffs.toml` の、ペア `ki` を挙げる項目にはペア `kw` も挙げてある。
2 つのバックエンドは互いに一致するので、`ki` と期待出力や Diktor との差分は、`kw` にも同じように現れるからである。

REPL の表示の行(`名前 : 型 = 値`)でレコードのフィールドを並べる順序は、処理系が決める。
Diktor はレコードを作った順に書き、`keleut repl` は表示する型にフィールドが並ぶ順(レイアウトを持たない値ではラベルのバイト列の順)に書くので、`diktor/test` の `--repl` のブロックがこの順序だけで違うことがある。
現在のテストスイートには該当するコマンドがなく、`known-diffs.toml` に項目を置いていない。
該当するコマンドが現れたら、そのコマンドをペア `ki`〜`kw` と `kr-*` の 12 ペアで登録し、台帳を V221、理由を「処理系が決める表示の順序の差」とする。
正規化でこの順序を並べ替えることはしない。
cram は標準出力と標準エラーを 1 本にまとめて読むので、REPL の表示の行とプログラムが書いた行を区別できず、並べ替えると評価の結果の本当の差分を隠すことがあるからである。

比較の前に、期待出力と結果の両方の行を、次の規則で正規化する。
まず、`実行時エラー: ` または `cancel 節で例外が抑制されました: ` の直後に付録 B の 3 番、4 番、9 番のメッセージ(`パターンに値が一致しません: `、`match のどの節にも一致しません: `、`handle のどの節にも一致しません: `)が続く行では、その後ろの `<値>` に含まれるレコードのフィールドを、ラベルのバイト列の順に安定に並べ替える(`kconform::fieldsort`)。
IRSpec 付録 B が、ラベルの違うフィールドどうしの順序をリーダに任せているからである。
Diktor は値を作った順に書き、RustKeleut は正準なレイアウトの順に書くので、並べ替えないと、順序だけの差分が未登録の差分になる。
次に、診断の種別名(型エラー、実行時エラーなど)に `: ` が続けばその後ろを切り捨て、先頭が `⚠ ` の行は `⚠` だけにする。
実行時エラーの行はここで種別名の後ろを切り捨てるので、並べ替えが比較の結果を変えるのは抑制の記録の行だけである。
抑制の記録の行は切り捨てずに並べ替えるので、フィールドの順序以外の表記は、期待出力とも Diktor とも比較する。
レコードを含まない抑制の記録の行は並べ替えても変わらないので、行の全体を比較する。
トークンに区切れない値(閉じていない文字列、対応しない括弧など)の行は並べ替えない。
プログラムが標準出力に同じ形式の行を書けば、その行も並べ替える。
cram の出力からは区別できないためで、種別名の後ろを切り捨てる規則も同じ性質を持つ。
ライタごとのバックエンドのグループの中の比較には、この正規化を適用しない。

## 出力の後処理

各コマンドの出力は、dune 3.21.1 の cram と同じ順序で後処理する。
まず端末の色の指定を取り除く。
次に、`/` で始まり、空白、改行、CR、タブ、`:` を含まない単語ごとに、単語の先頭が `TMPDIR` の値なら `$TMPDIR` に、作業ディレクトリなら `$TESTCASE_ROOT` に、1 度だけ置き換える。
単語の途中に現れるパスは置き換えない。
単語の区切り方と `$TESTCASE_ROOT` への置き換えは dune と同じである。
`$TMPDIR` への置き換えは kconform だけが行う。
dune のテストのシェルの `TMPDIR` は dune が作る `/tmp/build_*_dune` で、dune はこれを置き換えない。
最後に、シェルのエラーメッセージの先頭にあるスクリプトの位置を取り除く。

行は LF で区切り、LF の直前の CR を 1 つ取り除く。
LF で終わらない最後の部分は、CR 1 つだけなら捨てる。
これは `diktor/test/via_ir_run.py` と同じ規則である。

`.t` の読み方も dune 3.21.1 の cram と同じで、`via_ir_run.py` の `parse_cram` と同じく、行ごとに 3 つの状態を遷移する。
コメントの行の後に続く、2 つの空白で始まる行は、どのコマンドにも属さない。
コマンドの出力の後の空白 1 つだけの行も、その次が `  $ ` の行であれば、どのコマンドにも属さない。
dune はこれらの行を捨て、捨てた行は dune では期待出力との差分になる。
kconform も、これらの行を期待出力との差分として報告する。

## コーパス

`corpus/` は生成物だが、リポジトリに置く。
`--suite all` と `kconform kir-roundtrip` が、同じ内容のコーパスを使うためである。
`diktor/test` を変えたら `kconform extract-corpus` で作り直す。
`kconform extract-corpus --check` は、コーパスが `diktor/test` と食い違っていれば終了コード 1 で終了する。

`index.tsv` の各行は、コーパスの中のパス、抽出元(`テスト.t:行`)、オプションである。
オプションは、同じ `.t` の後ろのコマンドのうち、そのファイルを引数に取る最初の `diktor` の呼び出しから取る。
そのような呼び出しがないファイルは、オプションの列を `?` にする。
`--suite all` は、各ファイルを `diktor オプション FILE` の 1 つのコマンドとして実行する(`オプション` は `index.tsv` のそのファイルの行のオプションで、`?` なら何も付けない)。
`kir-roundtrip` は、各ファイルを `index.tsv` のオプションで KIR に書き出す。
オプションが `?` のファイルは、オプションなしで書き出し、失敗すれば(終了コードが 0 でなければ)`--no-prelude` を付けてやり直す。

## 主な使い方

`rustkeleut/` で次のように実行する。

```sh
cargo build --release
target/release/kconform --pairs dk,ki --suite all              # Diktor と keleut run を比べる
target/release/kconform                                        # 使える組をすべて走らせる(--no-default-features の keleut では LLVM の組を除く)
target/release/kconform --pairs ki,kj --suite all              # JIT をインタプリタと比べる
target/release/kconform --pairs ki,kj,kt,ka,km --suite all     # 段の切り替えの組も比べる
target/release/kconform --pairs dk --check-shim ../diktor/test/*.t  # shim が本物の diktor に渡すことの自己検査
target/release/kconform --pairs dk,kr-ki --suite all           # Rust のフロントエンドとインタプリタを Diktor と比べる
target/release/kconform --pairs dk,kr-ki,kr-kj,kr-kt,kr-ka,kr-km,kr-kw,x-ir --suite all  # Rust のフロントエンドを各後段で比べる
target/release/kconform expect conformance/frontend-depth/repl.t  # REPL が限度の後で入力を続けること
target/release/kconform expect conformance/kir-bad/*.t         # 記録した期待とだけ比べる
target/release/kconform kir-roundtrip                          # コーパスの全件で KIR を往復させる
target/release/kconform extract-corpus                         # コーパスを作り直す
```

本物の diktor の場所は、`--diktor` か環境変数 `KELEUT_DIKTOR` で指定する。
どちらもなければ `../diktor/_build/install/default/bin/diktor` を使う。
