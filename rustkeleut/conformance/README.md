# conformance

RustKeleut の差分試験の材料である。
走らせるのは `crates/kconform` の `kconform` で、コマンド行の形は計画 261024-1 の 8.1 節に従う。

## 置き場所

| 場所 | 中身 |
| --- | --- |
| `known-diffs.toml`、`known-diffs-*.toml` | 既知の差の一覧。`kconform` は `known-diffs.toml` と、その横の `known-diffs-*.toml`(一時の登録。いまは無い)を読み、どれにも無い差が 1 件でもあれば失敗する |
| `corpus/` | `kconform extract-corpus` が `diktor/test/*.t` のヒアドキュメントから抜き出した `.kel` と、その索引 `index.tsv` |
| `kir-bad/` | 不正な KIR。`keleut kir` がどれも 65 で拒否することを確かめる |
| `kir-fixtures/` | KIR の各版の見本(IRSpec 14 節)。副版ごとのディレクトリ(いまは `1.0/`)に、両方の読み手が読む正しい `.kir` と `.kirt` を置く。`--suite all` の組 `dk`、`dk-ir`、`kf`、`x-ir` では `keleut run FILE.kir` が `diktor --run-ir` になるので、Diktor の読み手も同じ見本を読む。`kconform kir-roundtrip` も見本を両方の読み手で読む |
| `init/`、`prim/`、`bytes/` | `keleut run` の試験(初期化、プリミティブ、出力の規約)。記録は `keleut run` の結果である |
| `depth/` | 論理的な深さの試験(IRSpec 11 節)。`keleut run --max-depth N` で走らせ、`--suite depth` で組 `dk-ir` と RustKeleut の組の間で比べる |
| `oom/` | 1 回の確保の上限 `N_alloc_max` を超える確保がメモリの不足になり、`cancel` 節を走らせて 3 で終わる試験。記録は `keleut run` の結果である |
| `gc/` | 回収の試験(計画 6.5 節)。100 万要素のリストを `--native-stack 512k` で作って走査する試験、`ulimit -v 524288` の下で確保の合計が上限を超えるプログラムとエフェクトの表の項目を回収する試験、構造的な `Eq` の入れ子の中の回収の試験、切り出し中のページをちょうど使い切ったときの掃除の試験。記録は `keleut run` の結果で、Diktor の木の評価器とも一致する。`ulimit -v 524288` の 2 つのコマンドは `--backend interp` を書くので、組 `kw` でもインタプリタで走る(Node は仮想メモリの上限 512 MiB の下で起動できない。台帳 V215)。LLVM の後段の組(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)では shim がこの旗を除くので、組の段で走る |
| `stack/` | ネイティブのスタックの試験。`--native-stack 512k` で、JIT の関数の深い末尾でない再帰の後の構造的な `Eq` の比べ方と、20 万段の構造的な `Eq` の入れ子が、どの後段でも同じ結果になることを確かめる。記録は `keleut run` の結果で、Diktor の木の評価器とも一致する |
| `effects/` | G の節の継続の取り出しと再開の試験(`g.t`。生成器、深い再帰の底の yield、2 回目の resume、再開の間の回収、G と N と T の節の重なり、メソッドの呼び出しと引数の多い関数の中の yield)と、ハンドラと継続の再開を何千回もくり返し、段の設定の組(`kt`、`ka`、`km`)で取り出しと再開が段の境目をまたぐ試験(`tier.t`)。記録は `keleut run` の結果で、Diktor の木の評価器とも一致する |
| `frontend-depth/` | Rust のフロントエンド(crate `keleut-front`)の入れ子の限度の試験(`parse.t` は `keleut dump ast`、`typecheck.t` は `keleut check`、`repl.t` は `keleut repl` で走らせる)。Diktor が基準にならないので `kconform expect` でだけ走らせる |

計画 6.1 節と 8.1 節が挙げる `float/` のディレクトリは置かない。
Float64 の印字は、`prim/prim.t`(LangSpec §16.2 の綴り、libm、Float64 から整数への変換)と、crate `keleut-rtcore` の試験 `tests/fltcmp.rs`(glibc の印字との比べ合わせ。M85)で確かめる。
どれも cram の形の `.t` で書き、`.kel` は `.t` の横か `.t` のヒアドキュメントに置く。
`.t` のコマンドは `diktor` でなく `keleut` を呼ぶ。
`keleut run` の呼び出しは、shim が組ごとに振り分ける(次の「組」の節の表の右の欄)。

## 組

組は KIR の書き手と実行系の組である(計画 8.2 節)。
`--pairs` を省くと、いま使える組をすべて走らせる。
`--no-default-features` で組んだ `keleut` では LLVM の後段の組(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)を除き、除いた組を標準エラーに書く(下の `kj` の段落を見よ)。
基準の組は、`--pairs` に書いた順に依らず、`dk` があれば `dk`、無ければ `dk-ir`、それも無ければ最初に挙げた組である。
ほかの組は基準の組と比べる。
要約の行の「基準」が、選んだ組を示す。
どの組も、`.t` に記録した期待とも比べる。

| 組 | `diktor` の実行の呼び出し | `keleut run` の呼び出し |
| --- | --- | --- |
| `dk` | 本物の diktor(木の評価器) | Diktor の対応する呼び出し |
| `dk-ir` | `diktor --emit-ir` の後の `diktor --run-ir` | 同じく |
| `kf` | 先に `keleut check` を通し、型検査で止まる入力はその結果を返す。通れば本物の diktor | Diktor の対応する呼び出し |
| `ki` | `keleut compat-diktor --frontend diktor --backend interp --tier interp-only --` に同じ引数。`keleut run --frontend diktor` が PATH の `diktor`(shim)を `--emit-ir` で呼び、shim はそれを本物の diktor に渡す。0 でなければその出力と終了状態 | `keleut run --backend interp --tier interp-only` |
| `kj` | `ki` と同じく、`keleut compat-diktor --frontend diktor --backend jit --tier jit-only --` に同じ引数 | `keleut run --backend jit --tier jit-only` |
| `kt` | `ki` と同じく、`keleut compat-diktor --frontend diktor --tier threshold=1 --` に同じ引数 | `keleut run --tier threshold=1` |
| `ka` | 種 1〜8 のそれぞれで、`keleut compat-diktor --frontend diktor --tier alternating=SEED --` に同じ引数 | `keleut run --tier alternating=SEED` |
| `km` | 種 1〜4 のそれぞれで、`keleut compat-diktor --frontend diktor --tier mixed=SEED --` に同じ引数 | `keleut run --tier mixed=SEED` |
| `kw` | `ki` と同じく、`keleut compat-diktor --frontend diktor --backend wasm --` に同じ引数 | `keleut run --backend wasm` |
| `x-ir` | `keleut emit-kir` の後の `diktor --run-ir` | 同じく |
| `kr-ki` | `keleut compat-diktor --frontend rust --backend interp --tier interp-only --` に同じ引数 | `keleut run --frontend rust --backend interp --tier interp-only` |
| `kr-kj` | `keleut compat-diktor --frontend rust --backend jit --tier jit-only --` に同じ引数 | `keleut run --frontend rust --backend jit --tier jit-only` |
| `kr-kt` | `keleut compat-diktor --frontend rust --tier threshold=1 --` に同じ引数 | `keleut run --frontend rust --tier threshold=1` |
| `kr-ka` | 種 1〜8 のそれぞれで、`keleut compat-diktor --frontend rust --tier alternating=SEED --` に同じ引数 | `keleut run --frontend rust --tier alternating=SEED` |
| `kr-km` | 種 1〜4 のそれぞれで、`keleut compat-diktor --frontend rust --tier mixed=SEED --` に同じ引数 | `keleut run --frontend rust --tier mixed=SEED` |
| `kr-kw` | `keleut compat-diktor --frontend rust --backend wasm --` に同じ引数 | `keleut run --frontend rust --backend wasm` |

左の欄の行き先は、Diktor の実行の呼び出し(旗なし、`--prelude`、`--no-prelude`、`--import-path`、`--strict-exhaustive` だけの呼び出し)である(計画 8.1 節の shim の表。M101)。
`keleut compat-diktor` は `--` の前の組の選択肢を後段の選択肢として読み、`--` の後の Diktor の旗を Diktor と同じに解釈して `keleut run` を呼ぶ。
入力のファイルは、名前が `.kir` で終わっても Diktor と同じくソースとして読む。
UTF-8 でない引数を含む呼び出しは、どの組でも本物の diktor に渡す(keleut は UTF-8 でない引数を使い方の誤りにする)。
`ki` から `kr-kw` までの組でも、`--type-check` は `keleut check`、`--dump-tokens` と `--dump-ast` は `keleut dump`、`--repl` は `keleut repl` に振り向け、使い方の誤りになる呼び出しは本物の diktor に渡す。

`keleut run` の既定のフロントエンドは、M99 から Rust(`--frontend rust`)である。
Diktor が KIR を書く組(`ki`、`kj`、`kt`、`ka`、`km`、`kw`)は、組の選択肢の先頭に `--frontend diktor` を置く。
表の右の欄の `keleut run` には、どれもこの旗が前に付く。
`.t` の中の `keleut run FILE.kel` は、`--frontend diktor` で PATH の `diktor`(shim)を `--emit-ir` で呼び、shim はそれを本物の diktor に渡す。
kconform は `.t` を走らせるシェルから `KELEUT_DIKTOR` を除くので、`keleut run` はいつも shim を呼ぶ。
`kconform expect` が `keleut run` に足す選択肢(組 `ki` と同じ)も `--frontend diktor` を含む。

組 `kf` は、Rust のフロントエンド(crate `keleut-front`。`keleut dump` と `keleut check` が使う)を Diktor と比べるために足した組である(M96)。
組 `ki` は Diktor が書いた KIR を RustKeleut のインタプリタで走らせ、後段だけを比べる。
組 `kw` は同じ KIR を WasmGC の後段(crate `keleut-wasm`。Node 26.9.0 以上で走る)で走らせる(M94)。
`kw` の shim の振り分けは `ki` と同じである。
組 `kj` は同じ KIR を LLVM の JIT の後段(crate `keleut-llvm`)で走らせる(M91)。
JIT にできる関数(M92 から `handle`、`resume`、`perform` を含む関数も)は、初めて呼ぶときに翻訳した JIT のコードで走る。
組 `kt`、`ka`、`km` は、同じ KIR を段の切り替え(M92)の設定で走らせる。
`kt` はインタプリタで 1 回呼んだ関数を翻訳のスレッドで待たずに翻訳し、`ka` は JIT の関数の入口ごとに種から決まる擬似乱数で段を選び、`km` は種で決まる半分の関数だけを翻訳する。
`--tier` が `interp-only` でなく `--backend` が無いので、`keleut run` は後段 `jit` で走る。
`ka` と `km` は種ごとに 1 つの実体として走り、既知の差の一覧では種を付けない名前で照らし合わせる。
`kj` の shim の振り分けは `ki` と同じである。
`kj` は feature `llvm` で組んだ `keleut` でだけ走る。
`llvm` は既定の feature なので、`cargo build --release` で組んだ `keleut` で走る。
`cargo build --release --no-default-features` で組んだ `keleut` では、どのコマンドも未実装の 4 で終わる。
`kt`、`ka`、`km` も同じである。
kconform は `keleut --version` の `backends:` の行で、`keleut` が後段 `jit` を持つかを確かめる。
持たないとき、`--pairs` を省いた実行はこれらの組を除き、除いた組を標準エラーに書く。
`--pairs` でこれらの組を名指した実行は、使い方の誤り(64)で終わる。
組 `kr-ki` は Rust のフロントエンドが書いた KIR をインタプリタで走らせる(M100)。
KIR は `keleut run --frontend rust` がプロセスの中で書き、ファイルを介さない。
組 `kr-kj`、`kr-kt`、`kr-ka`、`kr-km`、`kr-kw` は、同じく Rust のフロントエンドが書いた KIR を、それぞれ `kj`、`kt`、`ka`、`km`、`kw` と同じ後段の設定で走らせる(M99)。
`kr-ka` と `kr-km` は、`ka` と `km` と同じく種ごとに 1 つの実体として走る。
`kr-kj`、`kr-kt`、`kr-ka`、`kr-km` は `kj` と同じく feature `llvm` で組んだ `keleut` でだけ走り、`--no-default-features` で組んだ `keleut` での扱いも `kj` と同じである。
`kw` で V8 のヒープそのものが尽きたときは、Node が SIGABRT で終わり、`keleut run` は 3 でなく 70 で終わる。
このとき `cancel` 節は走らず、ホストのバッファに残っていた標準出力は失われる。
`ERROR(OutOfMemory)` と終了状態 3 になるのは `N_alloc_max` を超える確保だけで、`oom/` はそれを確かめる。
これは §15.5 からの既知の逸脱(P252、台帳 V215。計画 6.5 節)で、既知の差には登録していない。
LLVM の後段の組(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)でも、プロセスのメモリ(仮想メモリの上限、Rust と LLVM のヒープ)が尽きると、`keleut run` は 3 でなく SIGABRT(終了状態 134)で終わることがある。
このときも `cancel` 節は走らず、バッファに残っていた標準出力は失われる。
止まる場所は、LLVM の確保(ORC の `JIT session error: Cannot allocate memory` の後の確保の失敗)のことも、`Interp::fx_call` の `Vec` のような失敗を返さない小さな確保のことも、ある。
`ERROR(OutOfMemory)` と終了状態 3 を保証するのは、GC のヒープが尽きたときと `N_alloc_max` を超える確保だけである。
V215 と同じく既知の逸脱として扱い(M92 の記録)、差分試験には入れず、既知の差にも登録していない。

shim は組の選択肢を先に、コマンドの引数を後に並べ、`keleut run` は後に書いた旗を使う。
LLVM の後段の組(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)では、shim がコマンドの `--backend` を除き、組の後段で走らせる。
shim は LLVM の後段の組かどうかを、組の名前でなく組の選択肢(`--backend jit` か、`--backend` が無く `interp-only` でない `--tier`)で見分ける。
そのほかの組では、コマンドの `--backend` が組の後段に勝つ。
どちらの場合も、組の後段と違う `--backend` を書いたコマンドを、kconform は「組の後段の上書き」の行で報告する。
いまは、`gc/gc.t` の `ulimit -v 524288` の 2 つのコマンドが報告される。
組 `kw` ではインタプリタで走り(組の比べ方から漏れる)、LLVM の後段の組では旗を除いて組の段で走る。

`dk`、`dk-ir`、`x-ir` を除くすべての組(`kf`、RustKeleut の後段の組、`kr-*`)は、`--dump-tokens` と `--dump-ast` の呼び出しを `keleut dump` に、`--type-check` の呼び出しを `keleut check` に振り向ける。
`dk`、`dk-ir`、`x-ir` は、これらを本物の diktor に渡す。
`--type-check` を含まず `--strict-exhaustive` を含む呼び出しは、Diktor がプログラムを実行するので、組 `dk-ir` と `ki` では実行の呼び出しとして振り分ける。
Diktor の IR の旗(`--emit-ir`、`--run-ir`、`--max-depth` など)を含む呼び出しは、組 `x-ir` の書き出しを除いて本物の diktor に渡す。
`--repl` の呼び出しは、RustKeleut の後段の組(`ki`、`kj`、`kw`、`kr-ki` など)では `keleut repl` に組の選択肢と `--no-prelude`、`--prelude` を付けて振り向け、`dk`、`dk-ir`、`kf`、`x-ir` では本物の diktor に渡す(M100)。
`keleut repl` は組の選択肢(`--backend`、`--tier`、`--frontend`)を受けて捨て、Rust のフロントエンドとインタプリタで走る。
そのため、どの後段の組でも REPL のブロックの出力は同じで、後段の組の間の完全な一致の比べが成り立つ。
組 `ki` でも、REPL の KIR の書き手は Rust である。
入力のファイルを渡した `--repl` の呼び出し(使い方の誤り)は、どの組でも本物の diktor に渡す。
`--extra=FLAG` は、RustKeleut の後段の組(`ki`、`kw`、`kr-ki` など)の `keleut run` のすべての起動と、`keleut repl` の起動に `FLAG` を足す。
`keleut repl` では `--native-stack` がフロントエンドのスレッドのスタックになる。
リリースの構築では `--extra=--native-stack 512k` で構文解析の限度が 128 段に下がり、最適化なしの構築では `keleut repl` が使い方の誤り(64)で終わる。
回収の試験は `--extra=--gc-stress` で回す。
`keleut run --gc-stress` は、確保の場所(最上段のフレームの関数と点の組)を初めて通る確保で必ず回収し、それ以外では前の回収の手間の 64 分の 1 ほどの確保の回数ごとに回収する(間隔は擬似乱数で揺らす)。
実行したどの確保の場所でも少なくとも 1 回は回収するが、確保のたびには回収しない。
呼び出し元のフレームの根が試されるのは、そのフレームが止まっている間に回収が起こったときだけである。
`--gc-stress` が効くのはインタプリタと JIT(`--backend interp` と `jit`)だけである。
`keleut run` の `--native-stack` は、インタプリタと JIT のスタックとともに、Rust のフロントエンドのスレッドと KIR の読み込みと検証のスレッドのスタックも決める(計画 6.11 節。M101)。
`--backend wasm` はどちらの旗も受け付け、後段には使わない(回収は V8 が行い、Node のスタックは L から決める)。
そのため、`--extra=--gc-stress` を付けても組 `kw` の回収は試されない。
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
`--suite depth` を LLVM の後段の組(`--pairs` を省いたときの既定の組も含む)で走らせるときは、`--jobs` を 6 ほどにする。
32 コア、31 GB の機械で既定の並列の数(CPU の数)にすると、組 `ka` と `kr-ka` の `depth/gresume.t` がメモリ不足で OOM killer に止められる(終了状態 137。M93、M101)。
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
Rust のフロントエンドで走る組(`kr-ki kr-kj kr-kt kr-ka kr-km kr-kw`。`kr-ka` と `kr-km` は種ごとの実体)も、互いに完全に一致しなければならない。
`kconform` は、書き手ごとに 2 つの群(`ki kj kt ka km kw` と `kr-*`)を作り、群ごとに誤りの無い最初の組を基準にして、ほかの組の結果を正規化せずに比べる。
書き手の違う組(`ki` と `kr-ki` など)は、診断の文面の接頭辞が違ってよい(V217)ので、この比べ方では比べない。
後段の間の差は、どの項目にも当たらず、すべて登録の無い差に数える。
M88 は、M94 の一時の登録(下の V220)のために、`when = "unimplemented"` の項目が後段の間の差にも当たる一時の例外を足した。
M95 で G の節を入れたので、その登録とともにこの例外を消した。
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

M94 は、組 `kw` が G の節を選んで未実装の 4 で止まるブロックを、`known-diffs-wasm-g.toml` に `when = "unimplemented"` で一時に登録した(台帳 V220)。
M95 で G の節を入れて、このファイルを消した。
`kconform --pairs ki,kw --suite all` と `--suite depth` は、このファイルなしで差 0 件である。
`known-diffs.toml` の、組 `ki` を挙げる項目には組 `kw` も挙げた。
2 つの後段は互いに一致するので、`ki` と記録や Diktor との差は `kw` にも同じく現れる。

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

本物の diktor は、`--diktor` か環境変数 `KELEUT_DIKTOR` で指す。
どちらも無ければ `../diktor/_build/install/default/bin/diktor` を使う。
