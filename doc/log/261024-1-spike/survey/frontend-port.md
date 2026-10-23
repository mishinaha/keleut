# フロントエンドを Diktor から Rust へ移す見積もり

対象は親 `keleut`(ja/bleeding、bcb16f1)とサブモジュール `diktor`(6e49bbb)である。
追跡されているファイルは変更していない。
作業の生成物は `doc/log/261024-1-spike/probe/fp/` の下にだけ置いた。
各節で【実測】と【推定】を分けて書く。

## 1. 部品の大きさと依存

### 1.1 行数【実測】

`wc -l` の総行数と、コメントを除いた空でない行(以下「コード行」)の数である。
コード行は自作の `fp/tools/codelines.py`(OCaml の入れ子コメントを除く簡易な除去器)で数えた。
文字リテラル `'"'` などで数え違う余地があるので、数十行の誤差を含む。

| ファイル | 章 | 総行数 | コード行 | 役割 |
|---|---|---:|---:|---|
| location.ml | — | 39 | 4 | 位置 |
| aux.ml | 1.18 | 108 | 15 | 通し番号、例外の語彙 |
| syntax.ml | 1 | 1182 | 277 | 名前のインターン、カインド、型、行、AST(Data ファンクタ) |
| lexer.ml | 2 | 1151 | 504 | 3 層の字句解析(生トークン、`{` の再分類、ASI) |
| parser.mly | 3 | 1428 | 551 | Menhir 文法と脱糖 |
| dump.ml | 4 | 417 | 157 | `--dump-ast` の S 式 |
| tree.ml | 5 | 312 | 47 | 精緻化木の注釈、resolved、証拠(evidence) |
| decls.ml | 6 | 1314 | 453 | 宣言表、可視性台帳、組み込み登録 |
| unify.ml | 8 | 1357 | 517 | 単一化、行、一般化、制約の伝播、証拠の穴 |
| show.ml | 9 | 556 | 189 | 型の表示 |
| exhaust.ml | 10 | 1148 | 357 | 網羅性と到達不能(Maranget の I / U) |
| elab.ml | 11 | 6972 | 3090 | 型推論の本体、module の平坦化、import の単位 |
| prims.ml | — | 237 | 35 | プリミティブの名前 |
| driver.ml | 16 | 1429 | 661 | CLI、終了コード、REPL |
| (参考)value / builtin / interp | 12〜14 | 2621 | 1064 | 評価器(フロントエンドの外) |
| (参考)prelude.kel | 15 | 527 | — | プレリュード |

フロントエンド(評価器 3 ファイルを除く)は総行数 16,050、コード行 約 6,860 である。
このうち elab.ml がコード行の約 45% を占める。
全体の約 4 分の 3 は日本語の散文(文芸的プログラミング)で、コードは従である。

elab.ml の節ごとのコード行のうち大きいもの【実測】:

| 節 | コード行 | 内容 |
|---|---:|---|
| §11.29 | 275 | let rec |
| §11.45 | 237 | ファイルの名前空間(import の単位) |
| §11.2 | 235 | 数値リテラルの型 |
| §11.39 | 196 | 宣言列を 4 回なめる |
| §11.42 | 172 | module の平坦化 |
| §11.40 | 151 | パス 2 の 1 歩 |
| §11.5 / §11.38 | 130 / 130 | 型エイリアスの展開 / インスタンス本体の検査 |
| §11.23 | 109 | handle の対象エフェクトの決定 |
| §11.28 | 106 | let 束縛 |

### 1.2 モジュールの依存【実測】

`cd diktor && ocamldep -modules lib/*.ml` の結果から、標準ライブラリを除いて抜き出した。

```
location  : (Lexing)
aux       : Location
syntax    : Aux Location
lexer     : Parser Syntax (Sedlexing)
parser    : Syntax Location (MenhirLib)       ← _build/default/lib/parser.ml
tree      : Aux Location Syntax
dump      : Lexer Prims Syntax Tree
prims     : Syntax
decls     : Aux Syntax Tree
unify     : Aux Decls Location Syntax Tree
show      : Aux Syntax Unify
exhaust   : Aux Decls Syntax Tree Unify
elab      : Aux Decls Exhaust Lexer Location Prims Show Syntax Tree Unify
driver    : (すべて) + Unix Sedlexing
```

循環は 1 か所だけで、unify と show の間にある。
unify.ml:125-137 が印字関数を `ref` の穴(`show_ref` / `show2_ref` / `show_pair_ref`)として置き、
show.ml:554-556 が起動時に埋める。
lexer と parser も相互に参照するが、lexer はトークン型を `Parser` から受け取るだけである。

【推定】Rust では、トークン型を lexer 側(または共通の crate)に置けばこの相互参照は消える。
unify と show の循環は、同じ crate の中なら関数の相互呼び出しで済む。

### 1.3 Rust に移すときの構造上の論点

【実測】

- 型変数は `TVar of tvar ref` の Union-Find で、状態は `Unbound` / `Link` / `Generic` / `Rigid` の 4 つ(syntax.ml:289-382 の §1.5)。
  一般化の可否はレベル(Rémy の方法)で決める。
- 型のセルの書き換えは `set_tv` / `set_klink` を通り、記録が有効なら `trail` に戻す閉包を積む(§1.5)。
  REPL が失敗した入力を戻すのはこの記録による。
- AST の全ノードは `Data.t * ノード` で、`Tree.ElabData` は `{ oid; loc; mutable ty_field; mutable resolved; dict }`(syntax.ml §1.13、tree.ml §5.2)。
  パース木がそのまま精緻化木になり、型検査は可変フィールドを埋める。
  一般化はセルをその場で書き換えるので、木に書いた型にも結果が届く。
  評価器は数値リテラルの既定化の結果を、木に書いた型から読む(tree.ml §5.2)。
- 型クラスの辞書は、型検査が木に書く証拠(`EvParam` / `EvInst` / `EvRecord` / `EvVariant` / `EvHole`)と
  ノードの `dict_info`(`DUse` / `DAbs` / `DMethod` / `DInstance`)で表す(tree.ml §5.1b)。
  辞書を明示的な引数に書き換える変換は無く、評価器(interp.ml)が証拠を読んで辞書を作る。
- 大域の可変状態(トップレベルの `ref` と `Hashtbl.create`)は decls / unify / elab / exhaust / syntax / aux に 81 か所ある
  (`grep -n "Hashtbl.create\|: .* ref = ref\|^let [a-z_]* = ref\|^  let [a-z_]* = ref"` の件数)。
  そのうち decls.ml が宣言表(datas、effects、classes、instances、可視性台帳など)の大半を持つ。

【推定】

- 型は arena(`Vec<TyNode>` と `TyId`)に置き、型変数は添字の Union-Find にする。
  REPL の取り消しは、閉包の trail を移すより、`ena` の snapshot/rollback か、宣言表ごとの複製で行うほうが単純である。
  `ena` 0.14.4 は crates.io にある(`cargo info ena`)。
- ノードには `NodeId` を振り、型、resolved、dict は `NodeId` で引く別表(`Vec<Option<_>>`)にする。
  可変フィールドを共有する OCaml の設計は、TyId を別表に書けば同じ効果になる(一般化が TyId の中身を書き換えるので)。
- 81 か所の大域状態は 1 つの文脈構造体(`Session` / `TypeCtx`)に集める。
  Diktor の大域状態はプレリュードと利用者の単位を同じ表に積む前提で書かれているので、
  この構造体の境界を決めることが移植の最初の設計判断になる。
- 辞書渡しを中間言語で明示するには、証拠を読んで辞書の引数と辞書の構築に変える変換が新たに要る。
  これは Rust 側だけでなく、Diktor が中間言語を出す場合も Diktor 側に新設する部品である(interp.ml が証拠を解釈している処理を、木から木への変換として書き直すことになる)。

## 2. 移植の順序(推奨)

【推定】各段の終わりに置く関門は、Diktor の cram ファイルの中で、その段だけで決まるものを選んだ。

1. 基盤: 位置、名前のインターン(`oid`)、カインド、型と行の表現(syntax.ml、aux.ml、location.ml)。
2. 字句解析(lexer.ml)。
   関門は `test/tokens.t`(111 コマンド)、`lexerr.t`、`utf8_position.t`、`string_bytes.t` の `--dump-tokens` と字句エラーの位置。
3. 構文解析と脱糖(parser.mly)と `--dump-ast`(dump.ml)。
   関門は `test/ast.t`(20 コマンド)、`trailing_block.t`、`trailing_comma.t`、`syntax_error_pos.t`、`nonfeatures.t` の受理と拒否と位置。
4. 宣言表と組み込み登録(decls.ml)。
5. 単一化と型の表示(unify.ml、show.ml)。
6. elab.ml を次の順で: 型式とエイリアス → パターン → 式 → let → let rec → newtype / effect / class / instance → module の平坦化 → import の単位。
   関門は `typecheck.t`(`--type-check --no-prelude`、76 コマンド)を最初に置き、
   次に `typecheck_m5.t` / `m6.t` / `m7.t`、プレリュード込みの検査、最後に `typecheck_sample.t`(sample.kel 全文)。
7. 網羅性(exhaust.ml)。関門は `empty_match.t`、`uninhabited_pattern.t` と `--strict-exhaustive` の終了コード。
8. ドライバの終了コード規約(driver.ml §16.8)。
9. 中間言語への出力(新設)。
10. REPL(最後。§5 を参照)。

## 3. 構文解析器の作り方

### 3.1 文法の性質【実測】

- 規則は 107 本(`%inline` を含めると 112 本)、トークンは 68 種である(lexer.ml の冒頭の表、parser.mly:526-539)。
- `%left` / `%right` / `%nonassoc` / `%prec` は 1 つも無い。
  優先順位はすべて非終端の階層で表す(parser.mly §3.18)。
- `lib/dune` は menhir を `--explain --strict --table` で走らせる(lib/dune、parser.mly §3.13)。
  conflict は 0 で、`--strict` があるので 1 個でも出ればビルドが落ちる。
- 単独で menhir 20260209 を走らせた結果(作業用のコピーに対して):
  - 既定(Pager)のオートマトンは 589 状態、`.conflicts` は 0 バイト。
  - `menhir --lalr --strict --explain --table` も終了コード 0、出力に conflict の語は 0 件。
    つまり文法は LALR(1) で conflict が無い(LALR の状態数も 589)。
  - `--canonical` の正準 LR(1) は 7,131 状態。
  - `--list-errors` は誤りに至る文を 311 個出す。
    `.messages` ファイルはリポジトリに無く、menhir の状態ごとの文面は使っていない。
- 構文エラーの文面は、文法の状態ではなくドライバの経験則で作る(driver.ml §16.3a、§16.3)。
  - `error_positions`(lexer.ml:998-1001): 前の行が続きを求めていたら、前のトークンの終わりを主な位置にし、
    合わなかったトークンの位置を添える。それ以外は合わなかったトークンの位置。
  - `glued_number`(lexer.ml §2.14): 数値の綴り間違いでトークンが割れた形を名指す。
  - `misused_reserved_word`(driver.ml:303-333): 予約語を識別子に置き換えて `I.offer` で進め、
    次のトークンを `I.acceptable` で試す。menhir の増分 API に依存している。
  - 文法の意味アクションが投げる `Syntax_error_at` は parser.mly に約 20 か所ある
    (末尾ブロック、pub、レコードのラベル、`with` の右辺など。`grep -n "error\|Error" lib/parser.mly`)。
- 字句解析は 3 層である(lexer.ml の冒頭の表、§2.9、§2.10)。
  層 2 は `{` の後の 2 トークンを見て `LBRACE_BLOCK` / `LBRACE_RECORD` / `LBRACE_TYPE` に分け、
  層 3 は region スタック(`RTop` / `RBlock` / `RSuppress` / `RClause n`)と 2 つの述語で改行を文区切りに昇格させるか捨てる。
  region を動かすのはトークンを消費者に渡すときだけ、という不変条件がある(lexer.ml:33-35)。

### 3.2 候補【推定。crates の版は `cargo info` で実測】

| 案 | 内容 | 利点 | 欠点 |
|---|---|---|---|
| A. 手書きの再帰下降 | 式は非終端の階層をそのまま関数にする(優先順位の宣言が無いので Pratt にする必要も無い) | 生成器への依存が無い。エラー回復や複数エラーの報告を後から足せる。ビルドが速い | 受理する言語が menhir 版とずれる危険がある。ずれは差分テストで見張るしかない |
| B. lalrpop 0.23.2 | LR(1)(lane table)と LALR を持つ生成器。外部の字句解析器を受けられる | .mly からほぼ機械的に移せる。LALR で conflict 0 と分かっているので、文法の同値性を生成器が見張ってくれる | 増分 API(`offer` / `acceptable`)が無い。生成コードが大きく、ビルドが遅くなる |
| C. grmtools(lrpar 0.15.0) | yacc 互換の LR 生成器 | yacc 形式に近い。エラー回復(CPCT+)を持つ | menhir の `%inline`、パラメータつき非終端、`$sloc` を書き換える必要がある |
| D. menhir の表を Rust に持ち込む | `--cmly` などで表を出して Rust の LR 駆動器で動かす | 状態まで Diktor と同じになる | 意味アクションは結局書き直しで、道具を自作することになる |

字句解析の層 1 だけは `logos` 0.16.1 で書けるが、層 2 と層 3 は手で移すしかない。
層 1 も 66 種のトークンとエスケープと UTF-8 の検査だけなので、手書きで足りる。

推奨は A(手書きの再帰下降)である。理由は次のとおり。

- 文法は先読みで決めやすい形に作ってある。
  `{` の 3 分類は字句の側で済んでおり(§2.9)、関数形の束縛とパターン束縛は「小文字の識別子の次が `(` か」で分かれる(parser.mly §3.16)。
  ラムダは `fn(` で始まる。
  矢印型 `(A, B) => C` は括弧の列を読んでから `=>` を見れば足りる。
- LangSpec §18(doc/LangSpec.md:1951-1956)は、表示する型、診断、警告の文面を評価の結果に含めない。
  文面は仕様の対象ではないので、menhir の状態に合わせる理由が無い。
- 予約語の誤用の判定は、どの案でも増分 API を使えない。
  手書きなら「予約語を識別子に置き換えたトークン列でもう一度パースし、誤りの位置が次のトークンより後ろへ進むか」で同じ判定を作れる。
- 受理する言語のずれは、§4 の差分テストで検出できる。
  B は保険として残せる。A で差分が収束しないときは、.mly を lalrpop に機械的に移して受理の基準にする。

## 4. Rust のフロントエンドを Diktor と突き合わせる方法

### 4.1 cram テストの規模【実測】

`diktor/test` に cram ファイルが 61 本あり、`^  \$ ` の行(コマンド)は合計 3,184 である。
そのうち diktor を呼ぶ行が 1,560、ほか(`cat` 1,051、`printf` 429、`echo` 35、`export` 35 など)が 1,624。

diktor の旗の組み合わせ(コマンド行から抽出):

| 旗 | 回数 |
|---|---:|
| `--type-check` | 666 |
| (旗なし。実行) | 536 |
| `--type-check --no-prelude` | 215 |
| `--repl` | 43 |
| `--dump-tokens` | 42 |
| `--dump-ast` | 21 |
| `--strict-exhaustive` を含むもの | 28 |
| `--prelude` | 10 |
| `--import-path` | 2 |

期待出力に固定された終了コードの行は 854 で、`[1]` 625、`[2]` 168、`[3]` 49、`[4]` 3、`[64]` 4、`[74]` 3、`[]` 2 である。
これとは別に `echo "exit: $?"` の形で固定したものもある。

期待出力の中の診断と型の行:

| 種類 | 行数 |
|---|---:|
| `: 型エラー: ` | 625 |
| `: 構文エラー: ` | 157 |
| `: 字句エラー: ` | 41 |
| `実行時エラー` を含む | 58 |
| `⚠ `(警告) | 83 |
| `名前 : 型` の形 | 933 |

診断の本文はすべて日本語で、cram が文字単位で固定している。

ファイル別のコマンド数(`grep -c '^  \$ '`)を領域ごとにまとめた。

| 領域 | ファイル(コマンド数) |
|---|---|
| 字句と ASI | tokens(111)、lexerr(12)、utf8_position(28)、string_bytes(9)、reserved_word(61) |
| 構文と脱糖 | ast(20)、trailing_block(93)、trailing_comma(19)、syntax_error_pos(39)、type_parens(10)、if(43)、nonfeatures(69) |
| 型推論の核 | typecheck(76)、typecheck_m5(22)、typecheck_m6(68)、typecheck_m7(75)、typecheck_sample(2)、arity(32)、literal_default(23)、numeric(95)、neg_rem(20)、rec_tparam_note(37)、scope_escape_tparam(14)、errloc(16) |
| 行とエフェクト | annot_rows(280)、builtin_rows(20)、ctor_field_row(23)、rigid_empty_row(25)、effect_order(22)、never_ops(80)、handle_clause_loc(43)、resume_guard(12)、resume_sites(14)、runtime_effects(23)、blocking_top(13)、pub_effect_note(27) |
| 型クラスと辞書 | classes(72)、dictionary_passing(73)、premise(73)、instance_init(26)、instance_param_types(29) |
| カインド、newtype、リージョン | kinds(238)、region(72) |
| 網羅性 | empty_match(38)、uninhabited_pattern(15) |
| module、可視性、import | pub(61)、visibility(141)、import(195)、std_names(24)、decl_pos(19) |
| extern | extern_c(26)、extern_c_blocking(19) |
| 評価と終了 | eval(104)、run_exit(51)、tail_calls(42)、resolved_names(13)、invariants(80) |
| REPL | repl(27)。ほか 14 ファイルにも `--repl` がある |
| 横断 | smoke(8)、spec_gaps(23)、verify_fixes(139) |

フロントエンドだけで決まるもの(字句、構文、型推論、行、型クラス、カインド、網羅性、module)が過半を占める。

性能の参考: 作業用のコピーで `dune build --cache=disabled` は 1.98 秒、`dune test --cache=disabled` は 2.32 秒(32 コア、user 10.3 秒)。
元の作業木では `dune test --force` も cram を再実行せず 0.07 秒で返るので、この数字は使えない。
`diktor --type-check sample/stubs.kel sample/sample.kel`(プレリュード込み)は 0.008 秒で終わる。

### 4.2 方法(価値の高い順)【推定。ただし (c) の仕組みは実測で確かめた】

(a) 終了コードと受理の一致。
すべての cram の入力について、Diktor と RustKeleut の終了コードが一致することを求める。
LangSpec §15.5 が定める 0 / 1 / 2 / 3 に加え、処理系が定める 4 / 64 / 74 も Diktor に合わせると、cram ファイルをそのまま使い回せる。

(b) `--type-check` の型の行。
933 行の `名前 : 型` は、型推論の結果をほぼ直接に見る強い基準である。
ただし型変数の名前(`A`、`B`、`R1`、剛定数の `ς`、show.ml §9.1 の `show_all`、§9.3 のタプルの再糖衣化、§9.6 の行の省略)まで一致させる必要がある。
show.ml はコード行 189 と小さいので、規則をそのまま移すのが安い。
型の文面も LangSpec §18 の外だが、Rust の内部の基準として固定する価値がある。

(c) cram ファイルを無改変で RustKeleut に流す。
`diktor` という名前の shim を PATH の先頭に置き、別の dune プロジェクトで cram を走らせる方法が使える。
作業用のディレクトリに `(cram (deps %{bin:diktor} ...))` だけの dune プロジェクトを作り、
`bin/diktor` を「引数を記録して本物の diktor を exec する」シェルスクリプトにして、
smoke.t、typecheck.t、tokens.t を `PATH=$W/bin:$PATH dune test --root $W` で走らせた。
終了コード 0(差分なし)で、shim は 102 回呼ばれた(環境変数 `SHIM_LOG` も cram の中まで届いた)。
shim の行き先を `keleut` に替えれば、ディレクトリの構成、`--no-prelude` / `--prelude` / `--import-path`、
`printf` で作った入力をすべて保ったまま RustKeleut を試せる。
診断の本文は、shim の出力と期待出力の両方を同じ規則で正規化してから比べる
(`: 型エラー: ` / `: 構文エラー: ` / `: 字句エラー: ` / `実行時エラー: ` の後ろを切り、`⚠ ` の行は本文を捨てて件数だけ残す)。
外部の cram 実行器(`cram`、`prysk`)はこの環境に入っていない(`which` で確認)ので、dune の cram を使うのが手軽である。

(d) 入力の .kel を抜き出して直接比べる。
`fp/tools/extract.py` で `cat > *.kel <<'EOF'` のヒアドキュメントを抜き出すと 1,046 ファイルになり、取りこぼしは 0 だった。
それぞれを `diktor --type-check` にかけた終了コードは 0 が 513、1 が 500、2 が 31、4 が 2 である。
ただしこの抜き出しは `printf`(429)と `echo`(35)で作った入力、複数ファイルの import、`--no-prelude` などの旗を落とすので、
この内訳は目安であって基準ではない。基準には (c) を使う。

(e) `--dump-tokens` と `--dump-ast` の一致。
`--dump-ast` の S 式は位置を含まない(例: `(dlet (binding f (params x) = (+ x 1)))`)ので、構文解析器の比較にそのまま使える。
`--dump-tokens` は ASI を固定する唯一の手段である(lexer.ml の冒頭、driver.ml §16.1)。
RustKeleut に同じ形式の出力を持たせることを推奨する。
diktor/README.md:23-25 は「他の実装との読み合わせには使えません」と書いているので、計画でこの文を改める必要がある。

(f) 中間言語のテキストの比較。
Diktor には現在、中間言語を出す部品が無い(評価器は精緻化木を直接たどる)。
この比較は、中間言語の仕様と Diktor 側の出力器ができてから使える。
比べるときは、生成した名前(oid 由来)を出現順に振り直し、トップレベルの宣言の順序を正規化する必要がある。

### 4.3 位置の数え方【実測】

位置の桁はコードポイントで数える(sedlex の `pos_cnum`)。

- `let x = "あ" +⏎` は `c.kel:1:14: 構文エラー: この位置の後ろ、2:1 のトークンが文法に合いません`(終了コード 2)。
  `+` の終わりはコードポイントで 14 桁目、バイトなら 16 桁目になる。
- `let x = "あ" + )⏎` は `c2.kel:1:15: 構文エラー: 付近のトークンを確認してください`。`)` はコードポイントで 15 桁目。
- 不正な UTF-8 は、不正なバイトの位置を示す(utf8_position.t:7-37。例 `instr.kel:2:10`、`trunc.kel:2:1`)。

【推定】Rust の文字列はバイトで添字を取るので、位置の表示ではコードポイントに数え直す必要がある。
行頭のバイト位置を控え、表示のときだけ数え直せば、字句解析そのものはバイトで進められる。

## 5. 後回しにする REPL と診断の振る舞い【推定】

REPL(`--repl`、LangSpec §6.6、driver.ml §16.6b)はまるごと後回しにする。
cram で 15 ファイル、43 コマンドである。
必要な部品は次のとおりで、どれもバッチの型検査には要らない。

- 宣言表の写しと復元(decls.ml §6.14)、単一化の記録による取り消し(syntax.ml §1.5)、評価器の書き込みの取り消し(interp.ml §14.15)
- 続きの行を待つ判定(`Parser'.Error` の時点で `prev = Some EOF`、閉じていない文字列とコメントの字句エラー)
- `Show` を満たすかどうかを型検査器に問い合わせて値を表示する処理、`:type` / `:reset` / `:quit`
- トップレベルに `Print` を足す規則

診断で後回しにしてよいもの:

- 予約語の誤用の判定(driver.ml §16.3a)。汎用の構文エラーで代えても終了コードは 2 のまま
- `glued_number` による数値の綴り間違いの名指し(lexer.ml §2.14)
- `error_positions` の 2 位置の形(前の行の終わりと、合わなかったトークン)。最初は合わなかったトークンの位置だけでよい
- 日本語の本文の一字一句(LangSpec §18 が評価の結果から外している)

最初から合わせておくもの:

- 終了コード(§4.2 (a))
- 診断の位置(`ファイル:行:桁`)と種別の語(型エラー、構文エラー、字句エラー、未実装、import エラー)
- 型エラーは最初の 1 つで打ち切ること(elab.ml §11.43)と、そこまでの型の行を出すこと
- 警告の有無と件数(83 行)。`--strict-exhaustive` の終了コード 1 がこれに依存する
- 診断の出力先(実行時は標準エラー、`--type-check` の型の行は標準出力)

## 6. セルフホストとブートストラップの記述の所在【実測】

次のコマンドで探した(`doc/log/261024-1-spike` は除外)。

```
grep -rn -i "セルフホスト\|ブートストラップ\|self-host\|selfhost\|bootstrap\|オラクル\|oracle" \
  AGENTS.md doc/LangSpec.md doc/sample.kel doc/log diktor/README.md diktor/doc diktor/lib \
  diktor/tools diktor/dune-project diktor/diktor.opam
```

親リポジトリに `README.md` は無い(`ls README.md` が失敗)。
`doc/LangSpec.md`、`doc/sample.kel`、`diktor/lib`、`diktor/dune-project` には該当が無い。

| 場所 | 内容 |
|---|---|
| AGENTS.md:6 | `diktor/` is a Git submodule containing the OCaml bootstrap interpreter. |
| diktor/README.md:3 | Diktor は OCaml で書かれた Keleut プログラミング言語のブートストラップインタプリタです。 |
| diktor/README.md:4-5 | 将来のセルフホスト版 Keleut コンパイラのテストオラクルとなることを目指しています。 |
| diktor/README.md:23-25 | `--dump-ast` と `--dump-tokens` は diktor 固有で、他の実装との読み合わせには使えません(§4.2 (e)) |
| diktor/README.md:109-111 | 名前の由来。目標が他の実装のブートストラップを助けることだから |
| diktor/diktor.opam:4 | synopsis: "Diktor is a bootstrap interpreter of the Keleut programming language, written in OCaml." |
| doc/log/260912-1-status.md:23 | ブートストラップインタプリタ(将来のセルフホスト版コンパイラのテストオラクルを目指す処理系) |
| doc/log/260912-1-status.md:110 | 旧時代の README も「セルフホスト実装のブートストラップを助ける」ことを目標に掲げている |
| doc/log/260912-1-status.md:509 | (FFI の判定)セルフホスト時に本物のバックエンドを持つときまで持ち越す |
| doc/log/260912-1-status.md:643-644 | 8. セルフホスト版 Keleut コンパイラの計画。セルフホスト側の計画文書はまだない |
| diktor/doc/log/260829-1-plan.md:9 | セルフホスティングコンパイラのブートストラップとテストオラクルに使う |
| diktor/doc/log/260829-1-plan.md:138 | 延期 = セルフホスト版まで不要 |
| diktor/doc/log/260829-1-plan.md:165 | (Async など)スケジューラは数百行、オラクルに不要 |
| diktor/doc/log/260829-1-plan.md:230 | diktor.opam の synopsis を「Keleut のブートストラップ用インタプリタ」に |
| diktor/doc/log/260829-4-plan.md:6067 | セルフホスト時に本物のバックエンドを持つときまで持ち越す |
| diktor/doc/log/260829-4-plan.md:6097 | 真の部分型付けはセルフホスト版でも入れない方針を推す |
| diktor/doc/log/260919-1-plan.md:6879 | README の「他の実装との読み合わせには使えません」は将来のセルフホスト版との対照に効く約束である |

【推定】書き換える対象は現在の状態を述べる文書(AGENTS.md:6、diktor/README.md:3-5 / 23-25 / 109-111、diktor/diktor.opam:4)である。
`doc/log` の過去の記録は当時のまま残す、という規則が diktor/README.md:18-21 にある
(「`doc/log` の過去エントリの行番号は当時のまま残します」)。
過去の記録は書き換えず、新しい計画文書の中で「セルフホストの構想を RustKeleut に置き換える」ことと、
上の記述の扱い(260912-1-status.md:643-644 の未決事項 8 がこの計画で閉じること、
延期項目の「セルフホスト版まで」が「RustKeleut まで」と読み替わること)を書くのがよい。

## 7. 付記: 作業で使ったファイル

- `fp/tools/codelines.py`: コード行の数え方
- `fp/tools/extract.py`: cram からヒアドキュメントの .kel を抜き出す
- `fp/corpus/`: 抜き出した 1,046 ファイル、`fp/tc_exit.txt`: その終了コード
- `fp/shimproj/`: shim で cram を走らせた dune プロジェクト
- `fp/diktor-copy/`: 時間を測るための作業用のコピー(新しくビルドすると parser.mly:88 で warning 60 が出る。既存の挙動)

`fp/` は `doc/log/261024-1-spike/probe/fp/` である。
