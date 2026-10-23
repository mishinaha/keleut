# 261024-1: RustKeleut の実装計画(段階的な納品とリスクの先払いを軸にした案)

- 日付: 2026-10-24
- 決定: 仕様の所有者は 2026-10-24 に、Keleut のコンパイラと実行系の本実装を、Diktor とは別に Rust で keleut リポジトリの中に作ると決めた。
  理由は、Keleut が GC を前提とする言語で、GC の負荷を減らす仕組みが少ないことにある。
  コンパイラを GC の無い言語で書けば、コンパイルの速さで有利になる、と所有者は判断した。
  実装の名前は RustKeleut、コマンドの名前は `keleut` である。
  所有者は次の 4 点を求めた。
  1. Diktor と RustKeleut の両方が扱える中間言語(IR)とそのバイナリ表現を定める。
  2. RustKeleut のバックエンドは WasmGC、バイトコードインタプリタ、LLVM(ORCv2)の 3 つとする。
  3. できれば、バイトコードインタプリタと LLVM のコードが、段階的コンパイルのように互いに飛び移れるようにする。
  4. WasmGC は当面 Node.js で動けばよい。
  判断事項には推奨案を付け、そのまま実施できる計画にすることも求めた。
- 対象: 親リポジトリ `keleut`(新設する `rustkeleut/`、新設する `doc/IRSpec.md`、`AGENTS.md`)と、サブモジュール `diktor`。
- 前提: 親の HEAD は bcb16f1、diktor の HEAD は 6e49bbb である(どちらもブランチ `ja/bleeding`)。
  `doc/LangSpec.md` は 2316 行、diktor の cram は 61 本、3,184 コマンドである。
  道具の版は実測で、rustc 1.98.1(rustup なし、std は `x86_64-unknown-linux-gnu` だけ)、LLVM 22.1.8(共有ライブラリ)、llvm-sys 221.1.1、node 26.9.0(V8 14.6.202.34)、wasm-encoder と wasmparser 0.262.0 である。
  下調べの試作は `doc/log/261024-1-spike/llvm` と `doc/log/261024-1-spike/wasm` にある。
- 根拠の資料: 本計画は 7 本の調査報告に基づく。
  本文では次の略号で引く。
  - 〔tree〕Diktor の精緻化木と IR が要るもの
  - 〔fid〕実行系が一致させる意味規則の目録
  - 〔fx〕Diktor のエフェクトと巻き戻しの実装
  - 〔llvm〕LLVM ORCv2 の下調べ
  - 〔wasm〕Node 26 の WasmGC の下調べ
  - 〔front〕フロントエンドを Rust へ移す見積もり
  - 〔tg〕段階的コンパイルとメモリ管理
- 方針: 
  - 端から端まで通る経路を最初に作る。
    経路は「Diktor が IR を書く → RustKeleut が読む → バイトコードインタプリタで実行する → Diktor と差分試験する」である。
    WasmGC、LLVM、段の切り替えはその後に足し、フロントエンドの移植は最後に行う。
  - Diktor の cram を、全マイルストーンを通じて正解の基準(オラクル)にする。
    cram のファイルは書き換えず、`diktor` という名前の shim を PATH の先頭に置いて RustKeleut に流す(〔front〕4.2 (c) で仕組みを実測済み)。
  - マイルストーンは、それぞれ単独で検証できる大きさに切る。
    各マイルストーンの完了条件は、その時点までにできた部品だけで走るコマンドで書く。
  - 判断事項は推奨案で確定する(所有者の指示)。
    推奨案、代案、根拠は本文の各節と本計画 10 節の表に書く。
- 番号: 裁定は D499 から、台帳は V191 から、マイルストーンは M79 から、所有者への問いは P232 から使う。
- 読み方: 本計画の節は「本計画 4.3 節」と書き、`§` は `doc/LangSpec.md` の節に限って使う。
  「(測定)」は報告が実行して得た事実、「(推論)」は報告または本計画の判断である。

---

## 0. 全体の構成

```
 .kel ──Diktor(OCaml)──▶ 精緻化木 ──lower(新設)──▶ KIR ──▶ .kir(バイナリ)
                                            │                   │
                                            ▼                   ▼
                                  diktor --run-ir        keleut(Rust)
                                  (参照の IR 評価器)      ├ 読み込みと検証
                                                          ├ バイトコード ─┬ インタプリタ
                                                          │               └ LLVM ORCv2(同じフレームの形)
                                                          └ WasmGC 出力 ── node で実行
 後期: .kel ──keleut のフロントエンド(Rust)──▶ KIR(Diktor の KIR とテキストで一致させる)
```

完成までの道筋を 4 つの段に分ける。

| 段 | マイルストーン | 到達点 |
|---|---|---|
| A 端から端まで | M79〜M86 | Diktor が KIR を書き、Diktor と RustKeleut のインタプリタの両方が KIR を実行し、cram の実行のコマンドが全部通る |
| B ネイティブ | M87〜M89 | LLVM の JIT と段の切り替え。インタプリタだけ、JIT だけ、混在の 3 つの設定で cram が通る |
| C WasmGC | M90〜M91 | Node で cram の実行のコマンドが通る |
| D フロントエンド | M92〜M98 | IR の凍結(1.0)と、Rust のフロントエンド。cram の全コマンドが RustKeleut だけで通る |

---

## 1. 中間言語 KIR

### 1.1 名前、水準、型

- **D499(名前と拡張子)**: IR の名前を **KIR**(Keleut Intermediate Representation)とする。
  バイナリは拡張子 `.kir`、テキスト形は S 式で拡張子 `.kirs` とする。
  仕様の文書は親の `doc/IRSpec.md` に置く(LangSpec と同じ階層。Diktor と RustKeleut のどちらにも属さないため)。
- **D500(水準)**: KIR は精緻化の後、型検査と辞書の解決が済んだ段の言語である。
  名前の解決、局所変数のスロット化、自由変数、末尾位置、末尾 resume、節の分類、インスタンスの頭、メソッドの辞書の位置、数値リテラルの値は、すべて書き出す側で確定させる。
  Diktor の評価器が木の外から引いている 5 種類の情報(〔tree〕0 節の 3)を、KIR は自分の中に持つ。
  読む側が Diktor の Decls の表に頼らずに実行できることを、KIR の要件とする。
- **D501(式の形)**: 本体は「構造化した制御を持つ A 正規形」とする。
  演算の引数は原子(局所変数、定数、大域の参照)に限り、複合した式は `let` で名前を付ける。
  `match`、`if`、`handle` は入れ子のブロックを持つ式のまま残す。
  - 理由: LangSpec §5.1 の評価順(呼び出し先が先、引数は左から右、名前付きコンストラクタ引数は書いた順、レコードの拡張は追加のフィールドが先)を、`let` の並びとして明示できる(〔fid〕4 節)。
    コンストラクタの格納の順(宣言の順)と評価の順(書いた順)の区別も、`let` の後に `ctor` を置くだけで表せる。
  - 構造化した制御を残すのは、WasmGC が構造化した制御を要求するためである(本計画 3.7 節)。
  - 代案: 基本ブロックと分岐の CFG。インタプリタと LLVM には向くが、WasmGC で relooper が要る。採らない。
- **D502(型の扱い)**: KIR は「表現の型」を持つ。
  表現の型は `i32`、`i64`、`f64`、`bool`、`ref`(一様な語)の 5 つである。
  局所変数、関数の引数と戻り値、`let` の束縛に付ける。
  書き出す側は、ノードの `ty_field` を `repr` して頭が `Int32` / `Int64` / `Float64` / `Boolean` ならその型、それ以外(型変数、`Generic`、レコード、データ、関数など)は `ref` にする。
  型の形が違う境目には `box.*` / `unbox.*` を書き出す側が入れる。
  - 理由: 〔tg〕2.2 節は、単相の箇所の数値を箱に入れないために、IR に少なくとも表現の種類が要ると結論した。
    完全な型(行、型変数、エフェクト)を書くと、Diktor 側で `Generic` を番号に写す正規化と、RustKeleut 側の型の検証器が要る(〔tree〕3 節)。
    表現の型だけなら、書き出しは `repr` と頭の判定で済む。
  - 例外として、関数のエフェクト行は書く(D503)。
  - 代案 1: 型なし。WasmGC の i32 への写しと LLVM の箱なしの数値が失われる。採らない。
  - 代案 2: 完全な型。将来の最適化(行の配置の静的決定)には要るが、v1 の要件ではない。
    KIR 1.x の任意の節(本計画 1.6 節の節 0x81)として後で足せるよう、番号だけ予約する。
- **D503(エフェクト行)**: 関数ごとに、エフェクト行を「閉じたラベルの集合と、開いているかの印」で書く。
  `Lambda` と束縛のノードの `TArrow (_, _, eff)` から写す(〔tree〕3 節)。
  WasmGC が一般の resume のために変換する関数の集合を、この情報で決める(本計画 3.8 節)。

### 1.2 プログラムの単位と名前

- **D504(1 プログラム 1 ファイル)**: v1 の `.kir` は、プレリュードと全ての単位を初期化の順に並べた 1 つのプログラムである。
  単位の表は、単位の番号(Diktor の採番。起点 0、標準環境 -1、依存される側から 1、2、…)と、診断のためのパスを持つ。
  初期化の順は LangSpec §15.4 の帰りがけ順で、Diktor の `units`(`prelude :: flat`、driver.ml:820)の並びをそのまま書く(〔tree〕5.1 節、6.1 節)。
  - 理由: Diktor はこの並びを既に持っている。単位ごとの分割と結合器は、v1 の要件(差分試験)に要らない。
  - 代案: 単位ごとの `.kir` と結合。分割コンパイルの段で要るので、KIR 1.x の課題として台帳に残す(V197)。
- **D505(安定した名前)**: プロセスの中でしか通用しない識別(型の oid、束縛ノードの oid、穴の `h_id`)を KIR に書かない(〔tree〕0 節の 4)。
  KIR の中の実体は、種類ごとの表の添字で指す。
  添字は IRSpec が定める走査の順で振る。
  - 単位を初期化の順に、単位の中は宣言を書いた順に進む。
  - 宣言の中は、式を左から深さ優先の前順にたどり、`Lambda` に出会った順に関数の番号を振る。
  - 局所変数は、関数ごとに束縛に出会った順に 0 から振る(引数、辞書の引数、捕まえた変数、本体の順)。
  - ラベル、文字列、操作は、初めて現れた順に表へ入れる。

  表示の名前(`%1.M.f` の形の綴り、〔tree〕6.2 節)は、デバッグの節に別に持つ。
  この順を守れば、後で Rust のフロントエンドが書く KIR と、Diktor が書く KIR を、テキストでバイト単位に比べられる(本計画 4 節)。
- **D506(初期化と未初期化の検査)**: 大域の実体は「初期化済みの印を持つセル」とする。
  初期化の前に読むと実行時エラー(未束縛の変数)になる(LangSpec §6.4、〔fid〕10 節の `fwd.kel`)。
  単位の初期化は `init` の節の命令列で、各要素は次のどれかである。
  - `(def g e)`: `e` を評価して `g` に置く。
  - `(exec e)`: 式文を評価して捨てる。
  - `(instance i)`: インスタンス `i` を使える状態にする。前提も辞書の引数も無い値束縛のメソッドは、ここで 1 回だけ評価する(LangSpec §12.2、〔fid〕8 節の `inst2.kel`)。
    宣言より前にインスタンスを使うと実行時エラーにする(`inst.kel`)。
- **D507(使うたびに評価する値)**: 制約付きで一般化した値束縛と式文(Diktor の `DAbs` を持つもの)は、大域の種類を `per_use` とする。
  `per_use` の実体は辞書を引数に取る 0 引数の関数で、読むたびに初期化式を評価する(LangSpec §6.4、§12.2)。
  実装は結果を保存してはならない。
  ただし初期化式が構文上 `Lambda` であるときは評価に観測できる作用が無いので、実装は結果を保存してよい、と IRSpec に書く(Diktor の `da_last` の 1 件のキャッシュ、〔tree〕4.2 節、を正当化する条件)。
  `DAbs` を持つ式文は宣言の位置で評価しないので、`init` に現れない。

### 1.3 値と式の構成

IRSpec に載せる式の一覧を示す(テキスト形の綴り)。
原子 `a` は `(local n)`、`(global g)`、各種の定数である。

| 分類 | 式 | 備考 |
|---|---|---|
| 定数 | `(i32 n)` `(i64 n)` `(f64 0xBITS)` `(bool b)` `(unit)` `(str s)` | f64 は 64 ビットの 16 進で書く。範囲外の数値リテラルは定数にせず `(error lit-range "字面")` にする(LangSpec §4.2、〔tree〕3 節) |
| 束縛 | `(let x rep e body)` `(seq e1 e2)` `(letrec ((x (closure f a…))…) body)` | `letrec` の右辺はクロージャに限る(P236) |
| 関数 | `(closure f a…)` | `f` は関数の表の添字。`a…` は捕まえる値で、関数の宣言の `captures` と同じ順 |
| 呼び出し | `(call g a…)` `(call.clo c a…)` と、それぞれの `tail` 付き | `g` は既知の大域の関数への直接の呼び出し。末尾の印は書き出す側が LangSpec §5.7 の規則で付ける。演算子の適用には付けない |
| 組み込み | `(prim p a…)` `(extern x a…)` | `p` は IRSpec の付録の表の番号(48 個、〔fid〕13.1 節)。`x` は extern の表の添字 |
| データ | `(ctor d k a…)` `(ctor0 d k)` `(variant l a)` | `ctor` の引数は宣言の順。評価の順は前の `let` が決める |
| レコード | `(rec.empty)` `(rec.extend l v r)` `(rec.select l r)` `(rec.update l v r)` `(rec.restrict l r)` | 最も外側の同名ラベルが対象(Scoped Labels、〔tree〕4.4 節)。タプルは `_item` の積み重ね |
| 分岐 | `(if a e1 e2)` `(match a (case pat guard body)…)` `(let.pat pat a body)` | 照合は上から順。どの節にも合わなければ実行時エラー |
| 辞書 | `(dict.inst i a…)` `(dict.builtin b)` `(dict.record (l a)…)` `(dict.variant (l a)…)` `(method c m a)` `(dict.apply g a…)` | 本計画 1.4 節 |
| エフェクト | `(perform o a)` `(handle …)` `(resume a)` `(resume.tail a)` | 本計画 1.5 節 |
| 表現の変換 | `(box.i32 a)` `(unbox.i32 a)` ほか i64、f64、bool | |
| 誤り | `(error code a…)` | `???` への到達、範囲外のリテラル。文面の表は IRSpec の付録 |

- **D508(パターン)**: パターンは決定木にせず、`match` の節の列のまま KIR に残す。
  パターンの形は Diktor の 9 種類(〔tree〕1.2 節)から `PAnnot` を除いた 8 種類である。
  数値のパターンは値の型で読んだ定数に確定させる(型検査を通った後は読めることが保証される、〔tree〕1.2 節)。
  コンストラクタのパターンはフィールドの位置で書き、照合しないフィールドは `_` にする。
  - 理由: 決定木の構成でガードの評価順(上から、不一致の節のガードは評価しない、〔fid〕7 節)を崩す危険を、書き出す側に負わせない。
    Diktor の評価器と同じ順で照合するので、Diktor の参照の IR 評価器(本計画 2.3 節)は節を上から試すだけで済む。
    決定木への変換は RustKeleut の中の下ろしで 1 回だけ行い、全バックエンドで共有する。
- **D509(クロージャと自由変数)**: `Lambda` は関数の表に持ち上げる。
  関数の宣言は `params`、`dicts`(辞書の引数)、`captures`(捕まえる変数とその表現の型)を持つ。
  書き出す側は、局所変数のスロット化と同じ 1 回の走査で自由変数を求める。
  捕まえる対象には辞書の引数も含める(〔tree〕3 節)。
- **D510(呼び出し規約)**: 引数は位置で渡す。
  Diktor の「引数を `_item` の行のレコード 1 個に詰める」規約(〔tree〕4.1 節)は KIR に持ち込まない。
  関数の値の呼び出し(`call.clo`)は、全ての引数と戻り値を `ref` で渡す一様な規約とする。
  既知の大域の関数への `call` は、関数の宣言の表現の型で渡す。
  バックエンドは関数ごとに、表現の型の入口と一様な入口の 2 つを作る(一様な入口は KIR に書かない)。
- **D511(名目的な型とヴァリアント)**: `datas` の節にデータ型ごとのコンストラクタの名前とフィールドの数を書く。
  値は「コンストラクタの番号とフィールドの列」で、型の識別は持たない(Diktor の `VData` と同じ)。
  構造的ヴァリアントのラベルはプログラム全体のラベルの表の添字で、ペイロードは常に 1 つである(引数なしは Unit)。

### 1.4 型クラスと辞書

- **D512(辞書を明示する)**: 辞書渡しを KIR で明示する。
  Diktor の評価器が証拠(`EvParam`、`EvInst`、`EvRecord`、`EvVariant`、`EvHole`)を読んで作っている辞書を、書き出す側が式に変える(〔front〕1.3 節の最後)。
  - `EvParam` は辞書の引数の局所変数、`EvInst` は `(dict.inst i 前提の辞書…)`、組み込みのインスタンスは `(dict.builtin b)`、`EvRecord` と `EvVariant` は行の順のラベルと辞書の列にする。
  - `EvHole` の鎖は書き出すときに平坦化する。未解決の穴は REPL の外では起きないので、書き出す側が拒否する(終了状態 4)。
  - 辞書の引数の順序は `Unify.constraints_of` の正準順に従う(〔tree〕2 節)。
- **D513(インスタンスとメソッドの規約)**: インスタンスの表は、クラス、頭、前提の数、メソッドごとの実装の関数を持つ。
  頭の解決(`Decls.resolve_class` と `resolve_con`)は書き出す側が `with_decl_scope` の中で行う(〔tree〕3 節)。
  メソッドの実装は「前提の辞書の後にメソッド自身の辞書を並べた列」を受け取る(〔tree〕4.2 節)。
  `(method c m d)` は辞書 `d` からメソッドを取り出す。
  クラスの辞書の位置は書き出す側が計算して、`method` の呼び出しの引数の並びに確定させる。
- **D514(組み込みの表)**: 組み込みのプリミティブ(48)、extern "C"(5)、組み込みのインスタンスのメソッド(`builtin_method_prims` 39 と `builtin_method_direct` 5)、組み込みの値(12)、組み込みのクラスに、IRSpec の付録で番号を振る(〔tree〕5.2 節、〔fid〕13 節)。
  付録の表を正とし、Diktor の `--dump-ir-tables` と `keleut ir tables` の出力を付録から抜き出した表と比べる cram を置く(本計画 5.4 節)。
  `Array.each` と `pinned` は利用者の関数を呼ぶので、Rust で書かず KIR で書いた「実行時の関数」として各実装が持つ(本計画 3.4 節)。

### 1.5 エフェクト

- **D515(高水準の命令として残す)**: `handle`、`perform`、`resume`、`resume.tail` を KIR の命令として持ち、下ろし方はバックエンドが決める(〔fx〕8 節の 1)。
  `handle` の形は次のとおりである。

  ```
  (handle E tail? body
    (op o N|T|G pat guard? clause-body)…
    (return x e)?        ; 省略すれば恒等
    (cancel e)?)
  ```

  `tail?` はハンドラ式が末尾位置にあることを示し、`return` 節と再開しない操作の節の本体の末尾性を決める(LangSpec §5.7、〔fx〕R4)。
- **D516(節の分類を書き出す側で確定する)**: 操作節を 3 種に分ける(〔fx〕6.0 節)。
  - **N**: 返り値の型が `Never` の操作の節(Diktor の `RNeverOp`)。
  - **T**: `resume` が節の本体の末尾の背骨にしか無い節。背骨は `Seq` の最後、`Let`/`LetRec` の続き、`Match` の節本体、`&&`/`||` の右辺、`Run` の本体で、`Handle` と `Lambda` に入らない(〔tree〕2 節、Diktor の `spine_has_resume`)。
  - **G**: それ以外。

  背骨の上の `resume` は `resume.tail`、背骨の外は `resume` と書く。
  Diktor は分類を起動のたびに計算しているが(interp.ml:939-962)、KIR では書き出しのときに 1 回だけ計算する。
- **D517(活性化の印)**: 巻き戻しの宛先は、ハンドラの起動ごとに振る 64 ビットの単調増加の番号とする(〔fx〕R8、9 節)。
  フレームの番地は使わない。
- **D518(意味の基準)**: KIR の `handle` の意味は LangSpec §13.3〜§13.7 そのものとし、IRSpec は〔fx〕5 節の Q1〜Q10 を要件として引く。
  入れ子の `resume(resume(x))` の観測の差(〔fx〕R1 の測定)は、評価順どおりの背骨の経路の振る舞い(内側の `resume` が先に再開し、外側で 2 回目の検査に落ちる)を正とする。
  Diktor の短い経路(interp.ml:1064 で外側の `r_used` を先に立てる)は、Diktor の参照の IR 評価器では最初から背骨の経路で実装する。
  Diktor の木の評価器を合わせるかは P237 で問う(V191)。
- **D519(リージョン)**: `run` は KIR に残さない(実行時は恒等、〔fid〕9 節)。
  `Ref`、`Array`、`MutableArray` の操作は組み込みの値として `prim` で呼ぶ。
  リージョンの安全性は型検査が保証するので、KIR の検証器は検査しない。
- **D520(extern)**: extern の表は ABI(`prim` / `C`)、元の名前、引数の数を持つ。
  実装の無い "C" の名前は、読み込みでも結合でも誤りにせず、呼んだ時点で実行時エラーにする(LangSpec §17、〔fid〕12 節の `ext.kel`)。
  制約付きの型パラメータを持つ extern は辞書を受け取らない形で書く(Diktor は辞書を捨てている、〔tree〕4.8 節)。

### 1.6 バイナリ形とテキスト形

- **D521(バイナリの形)**: 

  ```
  magic   00 4B 49 52            ("\0KIR")
  version u16 major, u16 minor   (リトルエンディアン、固定幅)
  section* : id u8, size uLEB128(u32), payload
  ```

  整数は uLEB128 と sLEB128、f64 は 8 バイトのリトルエンディアンである。
  文字列は「バイト数の uLEB128 とバイト列」で、UTF-8 であることを要求しない(LangSpec §4.4 の文字列はバイト列で、`\u0000` も不正な UTF-8 もありうる、〔fid〕3 節)。
  節は次の順に高々 1 回ずつ現れる。

  | id | 節 | 内容 |
  |---|---|---|
  | 1 | strings | 文字列の表(定数とラベルと名前が共有) |
  | 2 | labels | レコードとヴァリアントのラベル(strings の添字) |
  | 3 | units | 単位の番号とパス |
  | 4 | effects | エフェクトと操作(Never の印、`Console` などの実行時が処理する印) |
  | 5 | datas | データ型とコンストラクタ |
  | 6 | classes | クラスとメソッドの名前 |
  | 7 | externs | extern の表 |
  | 8 | globals | 大域の実体(種類 `value` / `func` / `per_use`、単位) |
  | 9 | instances | インスタンスの表 |
  | 10 | funcs | 関数の宣言と本体 |
  | 11 | init | 単位ごとの初期化の命令列 |
  | 0x80 | debug | 表示の名前と位置(読み飛ばしてよい) |
  | 0x81 | types | 完全な型(予約。v1 では書かない) |

  式は前置の命令番号(u8)と、命令ごとに決まった被演算子で書く。
  id が 0x80 以上の節は、読む側が知らなければ読み飛ばしてよい。
  0x80 未満の未知の節と未知の命令番号は、読み込みの誤りにする。
- **D522(テキスト形)**: テキスト形は S 式で、IRSpec が正準の印字(1 行 1 式の字下げ、表の並び、空白)を定める。
  文字列は `"` で囲み、0x20〜0x7E 以外のバイトと `"` `\` を `\xHH` で書く。
  Diktor の `--emit-ir-text` と `keleut ir dump` は同じ正準の印字を出し、バイト単位で比べる。
  テキスト形を読む機能は RustKeleut にだけ持たせる(`keleut ir asm`)。実行時の関数(D514)と手書きの試験に使う。
- **D523(互換の方針)**: major 0 の間(M98 まで)は、読む側は major と minor の完全な一致を要求する。
  M92 で 1.0 に凍結した後は次のとおりにする。
  - minor の更新では、読み飛ばせる節(0x80 以上)の追加と、新しい命令の追加だけを許す。
    読む側は、自分の minor 以下のファイルを読む。
  - 既存の命令の意味と符号を変えるときは major を上げる。
    RustKeleut は最新の major だけを読む。
  - 互換の試験として、各 minor の見本の `.kir` を `rustkeleut/conform/kir-fixtures/` に置き続ける。
- **D524(IRSpec の置き場所と構成)**: `doc/IRSpec.md` は日本語で書き、節を「1 適用範囲、2 プログラムの構成、3 名前と採番、4 表現の型、5 式、6 辞書、7 エフェクト、8 初期化、9 バイナリ形、10 テキスト形、11 検証の規則、付録 A 組み込みの表、付録 B 実行時エラーの番号と文面」とする。
  意味は LangSpec を引いて書き、LangSpec の規則を繰り返さない。

### 1.7 検証の規則(IRSpec 11 節)

検証器は RustKeleut の `keleut-ir` に置き、読み込みの直後に必ず走らせる。
Diktor の読む側は同じ規則の部分集合(添字の範囲と表現の型)を検査する。

- 添字(局所、大域、関数、ラベル、文字列、インスタンス、操作)が表の範囲にある。
- 表現の型が一致する(`prim` の引数、`box`/`unbox` の前後、`call` の引数と関数の宣言)。
- `resume` と `resume.tail` は G と T の節の本体の中にだけ現れ、`Lambda` の本体、ガード、`return` 節、`cancel` 節、N の節には現れない(〔fx〕R1 の静的な検査と同じ)。
- `resume.tail` は背骨の上にあり、`resume` は背骨の外にある。節の分類(N/T/G)が本体と矛盾しない。
- `tail` の印は LangSpec §5.7 の末尾位置にだけ付く。
- `letrec` の右辺はクロージャである。

検証に落ちた `.kir` は実行せず、終了状態 65 で止める(処理系定義の値、〔fid〕0 節の I)。

---

## 2. Diktor の側

### 2.1 部品

| 部品 | ファイル(新設) | コード行の見込み | 注釈を含めた行数の見込み |
|---|---|---|---|
| KIR の型定義 | `lib/kir.ml` | 150〜250 | 400〜600 |
| 木から KIR への下ろし | `lib/lower.ml` | 900〜1400 | 2000〜3000 |
| テキスト形の印字 | `lib/kir_text.ml` | 150〜250 | 300〜500 |
| バイナリの書き出しと読み込み | `lib/kir_bin.ml` | 600〜900 | 1200〜1800 |
| 参照の IR 評価器 | `lib/kir_eval.ml` | 900〜1300 | 1800〜2600 |
| driver の結線と旗 | `lib/driver.ml` の追記 | 60〜120 | 150〜250 |
| 合計 | | 約 2800〜4200 | 約 5900〜8700 |

見込みは〔tree〕5.2 節の表に、テキスト形の印字を足したものである。
新しいファイルも既存のとおり文芸的な記事(column 0 の `(* … *)`)で書き、`tools/litcheck.py` の規約を守る。

### 2.2 下ろし(`lower.ml`)の作業

〔tree〕3 節の表の項目を、この順で 1 回の走査にまとめる。

1. 局所変数のスロット化と自由変数(辞書の引数を含む)。
2. 末尾位置と背骨の計算、節の分類 N/T/G。
3. 証拠の平坦化と辞書の式への変換、インスタンスの頭とメソッドの辞書の位置。
4. 数値リテラルの確定(範囲外は `error lit-range`)、パターンの数値の確定。
5. `BinOp` と `Neg` をメソッドの呼び出しに下ろす(`!=` は `Eq.eq` の否定、`&&`/`||` は `if`。〔tree〕1.4 節)。
6. A 正規形への変換と、評価の順の明示。
7. 表現の型の付与と `box`/`unbox` の挿入。
8. 本計画 1.2 節の順での採番。

差し込む位置は `run_with` の `Run` の分岐の、`type_check_files` と `Interp.run_units` の間である(driver.ml:1176-1197、〔tree〕5.1 節)。

### 2.3 参照の IR 評価器を作る

- **D525(Diktor に IR の評価器を持たせる)**: Diktor に、KIR を読んで実行する参照の評価器(`kir_eval.ml`)を作る。
  - 理由 1: 所有者は「Diktor と RustKeleut の両方が扱える IR」を求めた。
  - 理由 2: `diktor --via-ir`(型検査 → 下ろし → バイナリ → 読み込み → IR 評価器)で cram を走らせれば、Rust のコードが 1 行も無い段階で、下ろしとバイナリ形の誤りを cram の全ての実行のコマンドで検出できる。
    RustKeleut との差分試験で食い違いが出たとき、下ろしの誤りか RustKeleut の誤りかを切り分けられる。
  - 費用: コード行 900〜1300(〔tree〕5.2 節)。
    値(value.ml)と組み込み(builtin.ml)は木の評価器と共有し、エフェクトは木の評価器と同じく OCaml 5 の `Effect.Deep` に写すので、新しく書くのは式の評価とパターンの照合とハンドラの結線である。
  - 代案: 書き出しだけを作り、評価は RustKeleut に任せる。最初の差分試験が早く始まるが、食い違いの切り分けができない。採らない。
- **D526(旗)**: Diktor に次の旗を足す。
  - `--emit-ir FILE`: 型検査の後に KIR のバイナリを書き、実行しない。型検査の誤りでは何も書かず、今までどおり終了状態 1 か 2 で終わる。
  - `--emit-ir-text`: KIR のテキスト形を標準出力に書く。
  - `--run-ir FILE`: 型検査を通らずに `.kir` を読んで IR 評価器で実行する。
  - `--via-ir`: 実行のモードで、木の評価器の代わりに「書き出し → 読み込み → IR 評価器」で実行する。
  - `--dump-ir-tables`: 付録 A の表を印字する。

  `usage` の文面が変わるので `test/smoke.t` の期待値を別のコミットで更新する(〔tree〕5.1 節)。
- **D527(cram の統合)**: diktor の cram に次の 3 本を足す。
  - `test/ir_text.t`: 代表的なプログラム(各構文 1 本、20 本程度)の `--emit-ir-text` の出力を固定する。
  - `test/ir_roundtrip.t`: `--emit-ir` の後に `--run-ir` を走らせ、木の評価器と同じ出力と終了状態になることを固定する。
  - `test/ir_tables.t`: `--dump-ir-tables` を固定する。

  これとは別に、cram 全体を `--via-ir` で走らせる手順を `tools/via-ir.sh` に置く(本計画 5.1 節の shim の Diktor 版)。

---

## 3. RustKeleut の構成

### 3.1 ディレクトリと crate

- **D528(置き場所)**: RustKeleut は親リポジトリの `rustkeleut/` に置き、ここを cargo の workspace の根にする。
  サブモジュールにはしない(所有者の指示は「keleut リポジトリの中」で、IRSpec と同じ履歴で動かすため)。

```
rustkeleut/
  Cargo.toml            workspace。edition 2024、rust-version 1.98
  Cargo.lock            版を固定する(wasm-encoder の API は版ごとに変わる、〔wasm〕7 節)
  crates/
    keleut-ir/          KIR の型、バイナリの読み書き、テキストの印字と読み込み、検証器、共通の下ろし(決定木、表現の解析)
    keleut-rt/          値の表現、GC、文字列、数値、Float64 の show、プリミティブ、実行時エラー、Console
    keleut-vm/          バイトコード、下ろし、インタプリタ、エフェクト、入口の表、段の切り替えの枠組み
    keleut-llvm/        llvm-sys の薄い包み、バイトコードから LLVM IR、ORCv2 LLJIT(cargo の feature `llvm`)
    keleut-wasm/        WasmGC の出力(wasm-encoder)、Node 側の実行時(JS)、検証(wasmparser)
    keleut-front/       字句、構文、型検査、KIR の書き出し(M93 から)
    keleut-cli/         バイナリ `keleut`
  runtime/
    builtins.kirs       KIR で書いた実行時の関数(Array.each、pinned、with_runtime の結線)
    node/keleut-host.mjs WasmGC の実行時(入出力、Float64 の show)
  conform/              差分試験の道具と、仕様の観点の試験プログラム
```

依存は `keleut-ir ← keleut-rt ← keleut-vm ← keleut-llvm`、`keleut-ir ← keleut-wasm`、`keleut-ir ← keleut-front` である。
外部の crate は、llvm-sys `=221.1.1`(`prefer-dynamic`、〔llvm〕)、wasm-encoder と wasmparser `=0.262.0`(〔wasm〕7 節)、試験用の wat に限って始める。
inkwell は使わない(0.11.0 は LLJIT を包まない、〔llvm〕)。
LLVM の無い環境でも `cargo build --no-default-features` でインタプリタと WasmGC だけを組めるようにする。

### 3.2 コマンド `keleut`

| サブコマンド | 働き | 入るマイルストーン |
|---|---|---|
| `keleut --version` | 版と KIR の版 | M79 |
| `keleut ir dump F.kir` / `ir verify` / `ir tables` / `ir asm F.kirs -o F.kir` | KIR の道具 | M82 |
| `keleut run F.kir` | KIR を実行する | M83 |
| `keleut run F.kel …` | `.kel` を実行する。M97 までは `--frontend=diktor` が既定で、`$KELEUT_DIKTOR`(既定は PATH の `diktor`)に `--emit-ir` で書かせてから実行する | M83 |
| `run` の旗 `--backend=vm\|wasm`、`--tier=interp\|jit\|auto\|mixed:SEED`、`--jit-threshold=N`、`--stack-limit=WORDS`、`--prelude`、`--no-prelude`、`--import-path` | | M83〜M90 |
| `keleut build F --target=wasm -o DIR` | `.wasm` と起動用の `.mjs` を書く | M90 |
| `keleut check F.kel`、`keleut dump-tokens`、`keleut dump-ast` | フロントエンド | M93〜M96 |
| `keleut repl` | 対話的な実行 | M98 |

`--frontend=diktor` の委譲で、CLI の形を最初から最終の形にし、shim(本計画 5.1 節)をマイルストーンをまたいで変えずに済ませる。

### 3.3 読み込み、検証、バイトコード

- **D529(KIR とバイトコードの関係)**: バイトコードは RustKeleut の内部の形で、ファイルに書かない。
  読み込みと検証の後、関数を最初に呼ぶときに KIR の関数をバイトコードに下ろす(遅延の下ろし)。
  KIR の `match` は、この下ろしで「上から順に試す」照合の列にする(D508。決定木は M86 で共通の下ろしに入れ、LLVM と WasmGC に分かれる前に固める)。
  LLVM はバイトコードから生成する(本計画 3.6 節)。
  WasmGC はバイトコードではなく、共通の下ろしを済ませた KIR から生成する(構造化した制御が要るため)。
- **D530(インタプリタの形)**: レジスタ方式のバイトコードで、Rust の平坦なループで実行する。
  Keleut から Keleut への呼び出しで Rust の再帰を使わない。
  - フレームはファイバーごとの**値スタック**の上に置く。
    形は「ヘッダ 3 語(関数の番号と pc、呼び出し元のフレームの位置、戻り値のレジスタ)とレジスタの列」である(〔tg〕3.2 節)。
  - レジスタの表現の型は関数ごとに静的に決まり、関数の表に持つ。GC はこの表で、どのスロットが参照かを知る。
  - 末尾呼び出しはフレームを再利用してループの先頭へ戻る(〔fid〕5 節の (c))。
  - 命令の符号は 32 ビットの語の列で、命令ごとに被演算子の語数が決まる。
  - レコードの選択と更新は呼び出しの位置ごとのインラインキャッシュを持つ(〔tg〕2.2 節)。

### 3.4 実行時

- **D531(値の表現)**: 一様な語(`ref`)は 64 ビットで、最下位ビットが 1 なら即値、0 ならポインタとする(〔tg〕2.2 節)。
  - Int32 は `(n << 1) | 1`、Boolean と Unit と引数のないコンストラクタも即値にする。
  - Int64 と Float64 は `ref` の位置に置くときだけ箱に入れる。表現の型が `i64` / `f64` のレジスタでは箱に入れない。
  - ヒープのオブジェクトは 8 バイト境界で、先頭の 1 語に種類、大きさ、GC の印を置く。
  - レコードは「形の記述子(ラベルの列。intern する)へのポインタと値の配列」、データは「コンストラクタの番号と値の配列」、ヴァリアントは「ラベルの番号と値」、クロージャは「関数の番号と捕まえた値」である。
  - NaN ボックス化は採らない(Int64 が箱に入ることは変わらず、WasmGC の表現との差が大きくなる、〔tg〕2.2 節)。
- **D532(GC)**: v1 は動かさないマークスイープを 1 スレッドで行う。
  大きさの種類ごとのページと空きリストで割り当てる。
  根は、全ファイバーの値スタック(止まっている継続を含む)、大域の表、ハンドラの記録、Rust のハンドルの枠である(〔tg〕5.1 節)。
  安全点は割り当てと呼び出しに限る。
  前回の GC の後に生き残った量の 2 倍を割り当てたら次の GC を始める。
  - 代案 1: Perceus 型の参照カウント(〔tg〕案 C)。M89 の後に試作して比べる課題として台帳に残す(V196)。
  - 代案 2: LLVM の statepoint。LLVM 22.1.8 の `rewrite-statepoints-for-gc` が `musttail` を壊すことを測っている(〔tg〕5.4 節)。採らない。
- **D533(文字列)**: 長さ付きの不変のバイト列とする。
  比較は符号なしバイトの辞書順、長さはバイト数、`__string_sub` はバイト単位で範囲を検査する(〔fid〕3 節)。
  標準出力へはバイト列をそのまま書く。
- **D534(Float64 の show)**: `keleut-rt` に LangSpec §16.2 の規則を実装する。
  有効数字 `p` を 1 から 17 まで増やし、Rust の `format!("{:.*e}", p-1, f)` の結果を読み戻して一致した最初の `p` を使う。
  〔fid〕2 節が 2,000,000 個の値で glibc の `%.*g` と差 0 件を測った方式である。
  Rust の `{}`(最短の往復表記)は 522 件違うので使わない。
  同じ関数を実行時エラーの文面の値の表示にも使う。
- **D535(数値の検査)**: 整数の `+ - *` と符号反転は折り返す。
  除算と剰余は、除数が 0 なら実行時エラー、`MIN / -1` は `MIN`、`MIN % -1` は 0 とする。
  `__f64_to_i32` と `__f64_to_i64` は NaN、無限大、範囲外(`t >= -2^31 && t < 2^31` の外。上端は開区間)を実行時エラーにしてから切り捨てる(〔fid〕1 節)。
  インタプリタは `wrapping_*` と事前の検査、LLVM は分岐を先に置いて `sdiv`/`srem`/`fptosi` の未定義動作を避け、WasmGC は trap する命令の前に検査を置く。
  この検査を入れる責任は、各バックエンドの下ろしが負う。KIR の `prim` は検査を含んだ意味で定める。
- **D536(実行時エラーと終了状態)**: 実行時エラーは「巻き戻し」の一種として扱う(本計画 3.5 節)。
  有効なハンドラの `cancel` 節を内側から外側へ走らせ、標準出力を flush してから標準エラーに文面を書き、終了状態 3 で終わる(LangSpec §15.5)。
  文面は IRSpec の付録 B の番号と文面の表から作り、Diktor と同じ日本語の文面を出す(cram の `実行時エラー` を含む 58 行をそのまま通すため。〔front〕4.1 節)。
  `cancel` 節の中の脱出は打ち切って記録し(「cancel 節で例外が抑制されました: …」)、元の巻き戻しを続ける(〔fx〕R5)。
  標準出力に書けなければ巻き戻して終了状態 74 にする(〔fx〕R9)。
- **D537(スタックの限度)**: 限度は「全ファイバーの値スタックの語数の合計」で数え、既定は 128M 語とする。
  この値は OCaml 5 の既定の `l` と同じ大きさと見込んだもので、確かめていない。
  〔fid〕5 節は Diktor が 1 億段の非末尾再帰を RSS 1.8 GB まで溢れずに進めたことを測っており、見込みと合わない可能性がある。
  M85 で `OCAMLRUNPARAM=v=0x400` の表示か深い再帰の実測で Diktor の既定の限度を確かめ、既定値を決め直す。
  `--stack-limit=WORDS` と環境変数 `KELEUT_STACK_LIMIT` で変えられる。
  限度を超えたら実行時エラー(スタックオーバーフロー)として巻き戻す。
  - 値スタックは仮想アドレスを大きく予約し、実メモリは使った分だけ割り当てる。
  - LLVM のコードとインタプリタの入れ子(本計画 3.6 節)はネイティブのスタックも使うので、実行は専用のスレッドで行い、JIT の関数の入口とインタプリタの入れ子の入口でネイティブのスタックポインタも検査する(〔tg〕3.2 節)。
    スレッドのスタックの大きさは環境変数 `KELEUT_NATIVE_STACK` で決め、既定は 4 GiB の予約とする。
    このスレッドの大きさは `ulimit -s` に縛られないので、ネイティブのスタックが伸びないことの試験は `KELEUT_NATIVE_STACK=512k` で行う(本計画 5.5 節)。
  - 限度の大きさは処理系定義(LangSpec §15.5)で、限度を超えたときの扱いだけが一致の対象である(〔fid〕5 節)。
- **D538(Console と実行時の関数)**: `Console.write` は、最も外側に実行時が置く T のハンドラとする(〔fx〕R9)。
  `with_runtime` の結線、`Array.each`、`pinned` は `runtime/builtins.kirs` に KIR で書き、`keleut ir asm` でバイナリにして `keleut-vm` に埋め込む。
  利用者の関数を Rust の中から呼ばないので、`Array.each` の中の `perform` も普通に継続へ取り込める(〔fid〕9 節)。
- **D539(extern "C" の数学関数)**: インタプリタと LLVM は `sin cos exp log` を C の libm(この機械では glibc)で呼ぶ。
  Diktor(OCaml の glibc の呼び出し)と同じ値になる。
  WasmGC で最下位ビットが一致しない可能性は P233 で扱う。

### 3.5 エフェクト: 値スタックの区間と、状態を返す巻き戻し

〔fx〕は全バックエンドで yield bubbling、〔tg〕はネイティブのファイバーを推した。
本計画は、インタプリタと LLVM で「フレームを常に値スタックの上に置き、ネイティブのスタックは捨ててよいものとする」設計を採る。

- **D540(区間)**: `handle` の本体は新しい値スタックの**区間**(ファイバー)で走らせる。
  区間は連結リストで、ハンドラの記録(エフェクト、活性化の番号、節、親の区間)を持つ。
  - **T の節**: `perform` の位置の区間の上で、節を普通の呼び出しとして走らせる。
    節を評価する間だけ、ハンドラの連鎖の指す位置をそのハンドラの外側へずらす(LangSpec §13.4 の「節の中の操作は外側が受け取る」)。
    `resume.tail` は節のフレームを捨てて `perform` の位置へ値を返す。末尾 resume の空間は一定になる(〔fx〕R6、Q6)。
    `resume` しない枝は、節の値を持ってハンドラまで巻き戻す。
  - **N の節**: ガードを `perform` の位置で評価し、宛先が自分の活性化である巻き戻しを始める。
    ハンドラに着いたら、親の区間で本体を末尾の位置として走らせる(〔fx〕R4、Q4)。
  - **G の節**: `perform` の位置からハンドラまでの区間の列を切り離して、ワンショットの継続にする。
    節はハンドラの親の区間で走り、`resume` は区間の列をつなぎ直す。
    `resume` は値として持ち出せないので(LangSpec §13.5)、複製も長期の保持も要らない(〔fx〕R1)。
- **D541(巻き戻しをフレームにする)**: 巻き戻し(実行時エラー、N の節、`resume` しない節、継続の破棄)は、値スタックに「巻き戻しフレーム」(宛先の活性化の番号と運ぶ値)を積んで進める。
  ハンドラのフレームに着くと、`cancel` 節を巻き戻しフレームの上で普通のコードとして走らせ、戻ったら巻き戻しを続ける。
  - 理由: 〔fx〕R5 の cancelresume は、巻き戻しの途中で `cancel` 節が G の節を呼び、継続の中に「進行中の巻き戻し」が含まれる。
    巻き戻しの状態が値スタックのフレームなら、G の節の継続の切り離しにそのまま含まれる。
    Diktor の `exnc` の中の `run_cancel` と同じ順序になる(〔fx〕R2、Q2、Q5)。
- **D542(LLVM のコードとの共存: 状態を返す)**: JIT のコードは、呼び出しの戻り値に加えて ctx の状態の語を持つ。
  状態は `NORMAL`、`YIELD`(G の継続の切り離しが進行中)、`UNWIND`(巻き戻しが進行中)の 3 つである。
  - JIT の関数は、末尾でない呼び出しの後に状態を 1 回比べ、`NORMAL` でなければそのまま呼び出し元へ戻る。
    状態を `UNWIND` にしうる全ての箇所も同じ扱いにする。
    実行時の関数の呼び出し(`perform`、ハンドラの操作、割り当て)の後には比較を置く。
    実行時エラーになりうる `prim`(ゼロ除算、`__f64_to_i32`/`__f64_to_i64`、`__string_sub`、配列の添字、`(error …)`)は、検査を JIT のコードの中に展開し、失敗の枝で実行時の関数を呼んで `UNWIND` を立ててから戻る。
    こうしないと JIT の中のゼロ除算で `cancel` 節が走らない(〔fid〕1 節、〔fx〕Q8)。
  - JIT の関数は呼び出しの前に、生きている値を値スタックのスロットに書き、pc に当たる呼び出しの位置の番号を書く(〔tg〕案 A の規約で既に要るもの)。
    JIT のフレームの形はインタプリタのフレームと同じなので、戻った後の値スタックのフレームは、そのままインタプリタが続きを実行できる。
  - `YIELD` と `UNWIND` は、そのフレームの範囲を持つ最も内側のインタプリタのループ(`interp_run` の活性化)まで戻り、そこで D540 と D541 の処理をする。
  - G の継続を再開すると、切り離した区間のフレームはインタプリタで続きを実行する。
    フレームを返し終えて次に関数を呼べば、入口の表を通って JIT のコードへ戻る。
  - 理由: 継続の取り出しが「JIT のフレームからインタプリタのフレームへの脱最適化」と同じ操作になり、逆最適化の仕組みを別に作らずに済む(〔fx〕6.4 節)。
    ネイティブのスタックを切り替えないので、ファイバーのプール、ガードページ、ネイティブのスタックの GC の走査が要らない。
    JIT は landing pad も eh_frame の登録も要らない(〔llvm〕は ORC での巻き戻しを試していない)。
  - 費用: 末尾でない呼び出しのたびに比較と分岐が 1 つ増える。
    G の `perform` のたびに、ハンドラより上の JIT のフレームがインタプリタへ落ちる。
    G は仕様が空間を保証しない場合にあたり(〔fx〕6.0 節)、プレリュードの `with_stdout` も T なので、v1 ではこの費用を受け入れる。
  - 代案: ネイティブのファイバー(corosensei、往復 1.8 ns、毎回 mmap すると 7.1 µs、〔tg〕6.2 節)。
    G の節が多いプログラムで状態の比較と脱最適化の費用が測れたときに足す最適化として、台帳に残す(V195)。
- **D543(段ごとの対応)**: 〔fx〕5 節の要件を、どの部品で満たすかを次に示す。

| 要件 | インタプリタ | LLVM | WasmGC |
|---|---|---|---|
| Q1 ワンショット | 節の起動ごとの使用済みと生存の印 | インタプリタに任せる | 継続の構造体の印 |
| Q2〜Q4 破棄と cancel の順序、N の節 | 巻き戻しフレーム(D541) | `UNWIND` を返してインタプリタに任せる | exnref の tag と、ハンドラごとの `try_table` |
| Q5 cancel の中の G | 巻き戻しフレームが継続に入る | インタプリタに任せる | 巻き戻しの状態をヒープのフレームに置く(M91) |
| Q6 末尾 resume | T の節は呼び出しと戻り | T の節は呼び出しと戻り | T の節は呼び出しと戻り |
| Q7 末尾呼び出し | フレームの再利用 | `tailcc` と `musttail` | `return_call` / `return_call_ref` |
| Q8 実行時エラーとスタックの限度 | 巻き戻しフレーム、値スタックの限度 | 入口の検査と `UNWIND` | 深さのカウンタと明示の検査と tag |
| Q9 活性化の宛先 | 64 ビットの番号 | 同左 | 同左 |
| Q10 Console | 実行時の T のハンドラ | 同左 | JS の import |

### 3.6 LLVM ORCv2 と段の切り替え

- **D544(llvm-sys の包み)**: `keleut-llvm` は llvm-sys 221.1.1 の C API を直に呼ぶ薄い包みとする(〔llvm〕の計画への示唆)。
  LLVMContext は模塊ごとに作り、`LLVMOrcCreateNewThreadSafeContextFromLLVMContext` で包む(LLVM 21 以降に `LLVMOrcThreadSafeContextGetContext` が無いため)。
  Rust の関数は `LLVMOrcAbsoluteSymbols` で絶対記号として見せる。
  最適化は関数ごとに `default<O2>` を走らせる(小さな関数で 1 個 約 1.9 ms、〔llvm〕)。
- **D545(呼び出し規約)**: JIT の全ての関数を `tailcc i64 @f(ptr %ctx)` の 1 つのプロトタイプにする。
  引数は値スタックに積み、戻り値は 64 ビットの語で返す。
  関数の宣言の戻り値の表現の型(D510)が `i32`/`i64`/`f64`/`bool` なら、語は箱に入れない生のビット列である。
  一様な入口(`call.clo` の相手)だけが `ref` の語を返す。
  全ての呼び出しは関数の番号ごとの**入口の表**を通る間接呼び出しで、末尾呼び出しは `musttail` にする。
  - 根拠: 1e8 回の交互の末尾呼び出しが `ulimit -s 512` の下で完走し、`musttail` を外すと SIGSEGV になった(〔tg〕3.3 節)。
    `tailcc` の musttail は引数の数が違っても通るが、`tailcc` から `ccc` への musttail は検証器が拒む(〔llvm〕)。
  - Rust は `tailcc` の関数を定義できず、`become` も安定版に無い(〔tg〕3.1 節)。
    そのため、インタプリタへの入口のスタブと最上位の入口を、起動時に生成する小さな LLVM の模塊として作る。
- **D546(段の切り替え)**: 
  - インタプリタは関数の入口で呼び出しの回数を数え、`--jit-threshold`(既定 1000。計測で M88 に決め直す)を超えたらコンパイルを頼む。
  - コンパイルは別のスレッドで行い、終わったら入口の表の 1 語を原子的に書き換える(〔llvm〕の tier の実験)。
  - JIT からインタプリタへの末尾呼び出しは、入口の表がスタブを指すだけで、同じ `musttail` で済む。
  - インタプリタから JIT への末尾呼び出しは、`interp_run` が「この入口へ末尾呼び出しせよ」という要求を返し、スタブが `musttail` する。最上位のループから入ったときはループがトランポリンになる(〔tg〕3.2 節)。
  - OSR と逆最適化(投機の取り消し)は作らない。Keleut には反復の構文が無く、熱い反復は必ず関数の入口を通る(〔tg〕3.6 節)。
- **D547(段階的に JIT の対象を広げる)**: M87 の JIT は、`handle`、`resume`、`resume.tail` を含まない関数だけをコンパイルする。
  `perform` は実行時の関数の呼び出しにし、T の節はインタプリタで走らせる。
  M89 で `handle` を含む関数をコンパイルの対象に加える。ハンドラの記録と区間は M89 でも実行時の関数で作る。
- **D548(GC との関係)**: JIT のコードは安全点(割り当てと呼び出し)をまたいで生きる参照を値スタックのスロットに置く。
  GC は動かさないので、戻った後に読み直す必要は無い(〔tg〕案 A)。
  shadow-stack も statepoint も使わない。
- **D549(試験の設定)**: `--tier` を次の 4 つにする。
  - `interp`: JIT を使わない。
  - `jit`: 対象の関数を全て最初の呼び出しの前にコンパイルする(しきい値 0、同期)。
  - `auto`: 回数で切り替える(既定)。
  - `mixed:SEED`: SEED から決まる半分の関数だけをコンパイルする。
    インタプリタと JIT の境目を、末尾呼び出しと G の継続がまたぐ形を作るための設定である。

### 3.7 WasmGC

- **D550(出力と実行)**: `keleut-wasm` は wasm-encoder 0.262.0 でモジュールを組み、wasmparser で検証する(〔wasm〕7 節)。
  Rust の std は x86_64 しか無いので、Rust のコードを wasm32 にはしない。実行時の補助は JS で書く(`runtime/node/keleut-host.mjs`)。
  `keleut run --backend=wasm` は `.wasm` と `.mjs` を一時ディレクトリに書き、`node` を子プロセスで起動して、標準出力と終了状態をそのまま返す。
  使う機能は、フラグなしで動くと測ったもの(GC の構造体と配列、i31ref、`ref.test`/`br_on_cast`、`call_ref`、`return_call`/`return_call_ref`、exnref)に限る(〔wasm〕8 節)。
- **D551(値の表現)**: 一様な語は `eqref` とする。
  Int32 は i31 に収まる値だけ `i31ref`、収まらない値は `struct { i32 }` にする。
  Int64 と Float64 は `ref` の位置でだけ箱に入れる。
  文字列は `(array (mut i8))`、レコードは「形の記述子と `(array eqref)`」、データは「番号と `(array eqref)`」の構造体である。
  メモリは V8 の GC が管理し、RustKeleut の GC は使わない(〔tg〕7.2 節)。
- **D552(入出力)**: 標準出力へは、wasm が線形メモリの小さなバッファにバイトを写し、JS が `fs.writeSync(1, new Uint8Array(mem.buffer, p, n))` で書く(〔wasm〕4 節の方法 B)。
  JS の文字列に変換しない。終了の前に必ず書き終える(〔fid〕10 節)。
- **D553(Float64 の show)**: JS の数値の整形(`toString`、`toPrecision`、`toFixed`)は使わない(399,620 件中 113 件の差と `-0.0` の符号の欠落を測った、〔fid〕2 節)。
  `keleut-host.mjs` に、BigInt で 2 進の値を正確な 10 進に直し、`p` 桁に偶数丸めして `Number()` で読み戻す探索を書く。
  Rust の実装(D534)と、〔fid〕の 2,000,000 個の値と境界値で一致を確かめる。
- **D554(実行時エラー)**: trap とスタックあふれは wasm の `catch_all` で捕まえられない(〔fx〕4 節の測定)。
  そのため、ゼロ除算、`trunc`、配列の範囲外、null の参照、`ref.cast` の失敗になりうる箇所の前に、出力する側が明示の検査を置き、Keleut の実行時エラーの tag を throw する。
  スタックの限度は大域の深さのカウンタで数え、関数の入口で比べる。
  wasm の非末尾の再帰は既定の `--stack-size=984` で約 15,689 段しか伸びない(〔wasm〕2 節)ので、`.mjs` は `node --stack-size=…` を指定して起動し、カウンタの限度をその深さより小さく取る。
  下ろしの後に、trap する命令の前に検査があることを確かめる検査器を `keleut-wasm` の試験に置く。
- **D555(N と T の節)**: T の節はその場の呼び出し、N の節と `resume` しない節と実行時エラーは exnref の tag で巻き戻す。
  ハンドラごとに `try_table` を置き、自分宛てでない tag を受けたら `cancel` 節を走らせて `throw_ref` する(〔fx〕R2)。
  M90 は G の節を持たないプログラムだけを扱い、G の節を持つプログラムは終了状態 4(未実装)で止める。

### 3.8 WasmGC の G の節

- **D556(状態機械への変換)**: M91 で、G の節を持つプログラムに限り、G のハンドラに届きうる関数を状態機械に変換する。
  - 対象の関数は、プログラム全体の G のハンドラが処理するエフェクトの集合と、関数のエフェクト行(D503)が交わるか、行が開いている関数である。
    G のハンドラが無いプログラムでは、変換は何もしない。
  - 対象の関数は、`perform` に届きうる末尾でない呼び出しの後で状態を比べる。
    `YIELD` なら、生きている値と再開の位置の番号を、関数ごとの GC の構造体に詰めて継続の鎖に足してから戻る(遅延した継続の取り出し)。
  - 再開は、関数ごとの「再開の入口」が位置の番号で分岐して本体の途中へ入る。
  - 巻き戻しの途中の G(Q5)は、`cancel` 節を走らせる部分も状態機械のフレームとして表す。
  - 代案 1: JSPI。フラグなしで使えるが非同期で、深いハンドラの節と本体の間の同期的な往復に合わない(〔fx〕4 節)。
  - 代案 2: WasmFX。フラグが要り、`switch` でプロセスが落ちる(〔wasm〕6 節)。Node でフラグが外れたら置き換える候補として台帳に残す(V194)。

---

## 4. フロントエンドの Rust への移植

- **D557(時期)**: フロントエンドは最後(M93〜M98)に移す。
  それまで `keleut run F.kel` は Diktor に KIR を書かせる(本計画 3.2 節の `--frontend=diktor`)。
  - 理由: フロントエンドはコード行 約 6,860、うち elab.ml が約 45% で(〔front〕1.1 節)、最も大きい部品である。
    先にバックエンドを固めれば、Rust のフロントエンドの出力を KIR のテキストで Diktor と比べられ、型検査の誤りと下ろしの誤りを切り分けられる。
- **D558(順序と関門)**: 〔front〕2 節の順で移し、各段の関門を cram の部分集合にする。
  1. 字句(関門 `tokens.t`、`lexerr.t`、`utf8_position.t`、`string_bytes.t` の `--dump-tokens`)。
  2. 構文と脱糖(関門 `ast.t`、`trailing_block.t`、`trailing_comma.t`、`syntax_error_pos.t`、`nonfeatures.t` の `--dump-ast`)。構文解析器は手書きの再帰下降とする(〔front〕3.2 節の案 A。文法は LALR(1) で conflict 0、優先順位の宣言が無い)。
  3. 宣言表、単一化、型の表示(関門 `typecheck.t` の `--no-prelude`)。型は arena と添字の Union-Find、大域の状態 81 か所は 1 つの文脈の構造体に集める(〔front〕1.3 節)。
  4. elab の残りと網羅性(関門 `typecheck_m5/m6/m7.t`、`typecheck_sample.t`、`empty_match.t`、`uninhabited_pattern.t`)。
  5. KIR の書き出し。Diktor の `lower.ml` と同じ採番(D505)で書き、`--emit-ir-text` とバイト単位で比べる。
  6. ドライバの終了状態と import(関門 `run_exit.t`、`import.t`)。
  7. REPL(関門 `repl.t` と `--repl` を含む 14 本)。
- **D559(診断の文面)**: 終了状態、診断の位置(`ファイル:行:桁`、桁はコードポイントで数える、〔front〕4.3 節)、診断の種別、警告の件数、`名前 : 型` の行は最初から Diktor に合わせる。
  診断の本文の一字一句は LangSpec §18 の評価の結果の外なので、shim の比較で正規化してよい(〔front〕4.2 (c))。
  正規化しない一致は M98 の目標にとどめる。

---

## 5. 試験の方針

### 5.1 shim による cram の差分試験

- **D560(道具)**: `rustkeleut/conform/cram-shim/` に、〔front〕4.2 (c) で実測した形の dune プロジェクトを置く。
  `diktor/test/*.t` と `diktor/test/sample/` を作業用のディレクトリに写し、`PATH` の先頭に `bin/diktor`(shim)を置いて `dune test --root` で走らせる。
  cram のファイルは書き換えない。
  shim は引数を見て次のように振り分ける。

  | 呼び出し | M97 まで | M98 から |
  |---|---|---|
  | 実行(旗なし、`--prelude`、`--import-path`、`--no-prelude`) | 本物の diktor に `--emit-ir` で書かせ、終了状態が 0 なら `keleut run` を実行 | `keleut run` |
  | `--type-check`、`--dump-tokens`、`--dump-ast`、`--strict-exhaustive` | 本物の diktor | `keleut` の対応するサブコマンド |
  | `--repl` | 本物の diktor | `keleut repl` |

  型検査で落ちるプログラムでは、`--emit-ir` が今までどおりの診断と終了状態 1 か 2 を出すので、cram の期待値はそのまま通る。
  環境変数 `KELEUT_CONFORM` で `keleut run` の設定(`interp`、`jit`、`mixed:SEED`、`wasm`)を選ぶ。
- **D561(スタックの限度の写し)**: cram は `OCAMLRUNPARAM=l=100k diktor …` の形で限度を絞る(`tail_calls.t` ほか)。
  shim は `OCAMLRUNPARAM` の `l=` の値(語数。`k` と `M` の接尾辞を読む)を `--stack-limit` に写す。
  単位を OCaml の語数にそろえたので(D537)、絞った試験は「末尾の 10 万回は通り、非末尾の深い再帰は落ちる」という同じ結果になる見込みである。
  見込みが外れる試験は、M85 で限度の数え方(フレームのヘッダの語数)を調整して合わせ、合わないものを台帳に載せる。
- **D562(既知の差の一覧)**: 一致しない cram のコマンドは `conform/known-diffs.toml` に、cram のファイル、コマンドの行、理由、台帳の番号を書いて登録する。
  登録の無い不一致が 1 件でもあれば試験を落とす。

### 5.2 Diktor だけでの IR の試験

`diktor/tools/via-ir.sh` は shim の Diktor 版で、実行のコマンドを `diktor --via-ir` に写して cram を走らせる。
M81 から M84 の完了条件は、これが全部の cram で差分なしになることである。

### 5.3 コーパスの差分試験

- `conform/corpus/` に、cram のヒアドキュメントから抜き出した 1,046 本の `.kel`(〔front〕4.2 (d))と、〔fid〕の試験プログラム(`num.kel`、`order.kel`、`never2.kel`、`cancelerr.kel` など)を置く。
- `keleut-conform corpus` は各プログラムを Diktor と `keleut run` の全ての設定で実行し、標準出力のバイト列と終了状態を比べる(LangSpec §18 の評価の結果)。
  標準エラーは、標準出力との相対の順序だけを比べる(〔fid〕0 節の D)。
- Float64 の show は、〔fid〕の 2,000,000 個の値を Rust、JS、Diktor の 3 つで印字して比べる専用の試験にする。

### 5.4 IR の試験

- 往復: `diktor --emit-ir-text` と `keleut ir dump`(`diktor --emit-ir` の出力を読んだもの)の一致を、コーパスの全てのプログラムで確かめる。
- 表: `diktor --dump-ir-tables`、`keleut ir tables`、IRSpec 付録 A から抜き出した表の 3 つを比べる。
- 検証器: 壊した `.kir`(添字の範囲外、表現の型の不一致、背骨の外の `resume.tail`)を `conform/kir-bad/` に置き、終了状態 65 を確かめる。
- 互換: `conform/kir-fixtures/` の見本を毎回読む(D523)。

### 5.5 段の組み合わせ

`keleut-conform` は cram の shim とコーパスを、`interp`、`jit`、`mixed:1`〜`mixed:4`、`wasm` の 7 つの設定で走らせる。
`tail_calls.t` と `never_ops.t` は `KELEUT_NATIVE_STACK=512k` でも走らせ、JIT と境目のスタブがネイティブのスタックを伸ばさないことを確かめる(〔tg〕3.3 節は主スレッドで `ulimit -s 512` を使ったが、RustKeleut は専用のスレッドで走るので `ulimit` は効かない。D537)。

---

## 6. マイルストーン(M79 から M98)

### 6.1 共通の完了条件

- diktor 側: `dune build` が既存の Warning 60 だけで通り、`dune runtest` が緑で、`python3 tools/litcheck.py` が通る。
- RustKeleut 側: `cargo build --release`、`cargo test`、`cargo clippy -- -D warnings` が通る。`llvm` の feature を外した `cargo build --no-default-features` も通る。
- 期待値の更新は実装と別のコミットにする(AGENTS.md)。
- 規模は S(コード 1,000 行未満)、M(1,000〜3,000 行)、L(3,000 行超)で書く。

### 6.2 各マイルストーン

#### M79 IRSpec の初版と workspace の骨組み(規模 S、依存なし)

- 成果物: `doc/IRSpec.md` の初版(本計画 1 節の全て。付録 A と B を含む)、`rustkeleut/` の workspace と空の crate、`keleut --version`。
- 文書: `AGENTS.md` の Project Structure に `rustkeleut/` と `doc/IRSpec.md` を足し、6 行目の「OCaml bootstrap interpreter」を「OCaml reference interpreter (test oracle for RustKeleut)」に改める。
  diktor の `README.md` の 3〜5 行目を「Keleut の参照インタプリタで、RustKeleut の試験の基準(オラクル)と KIR の参照実装を兼ねる」の趣旨に、`diktor.opam` の 4 行目の synopsis を同じ趣旨に改める(P232)。
  `doc/log` の過去の記録は書き換えない(diktor/README.md:18-21 の規則)。260912-1-status.md の未決事項 8「セルフホスト版 Keleut コンパイラの計画」は本計画で閉じたものとし、本計画の記録に書く。
- 完了条件: `cd rustkeleut && cargo build --release && ./target/release/keleut --version` が KIR 0.1 を表示する。

#### M80 Diktor: 下ろしとテキスト形(実行時が処理するエフェクトだけ)(規模 L、依存 M79)

- 範囲: 利用者の `handle` を含まないプログラム全て。
  `perform` は含む(`echo` と `echoln` は `perform write` で、実行時の Console のハンドラが受ける。〔fid〕13.3 節)。
- 成果物: `lib/kir.ml`、`lib/lower.ml`、`lib/kir_text.ml`、`--emit-ir-text`、`test/ir_text.t`。利用者の `handle` を含むプログラムには終了状態 4 を返す。
- 完了条件: `dune runtest`。`test/ir_text.t` の 20 本の出力を目で確かめた記録を実施記録に書く。

#### M81 Diktor: バイナリ形と参照の IR 評価器(規模 L、依存 M80)

- 成果物: `lib/kir_bin.ml`、`lib/kir_eval.ml`、`--emit-ir`、`--run-ir`、`--via-ir`、`--dump-ir-tables`、`test/ir_roundtrip.t`、`test/ir_tables.t`、`tools/via-ir.sh`。
- 完了条件: `tools/via-ir.sh` で、利用者の `handle` を含まない実行のコマンドが全て差分なしになる。含むものは終了状態 4 の不一致として数を記録する。

#### M82 RustKeleut: KIR の読み込み、印字、検証(規模 M、依存 M81 の `kir_bin.ml`)

- M81 のうちバイナリの書き出しと読み込み(`kir_bin.ml` と `--emit-ir`)を先にコミットすれば、M82 はそこから始められる。参照の IR 評価器(`kir_eval.ml`)を待たない。

- 成果物: `keleut-ir`、`keleut ir dump/verify/tables/asm`、`conform/kir-bad/`。
- 成果物にコーパス(`conform/corpus/`。〔front〕4.2 (d) の抜き出しの道具で作る 1,046 本)の取り込みを含める。
- 完了条件: コーパスのうち `diktor --emit-ir-text` が終了状態 0 を返すプログラムの全てで、`diktor --emit-ir-text` と `keleut ir dump` が一致し、`keleut ir verify` が通る。`conform/kir-bad/` が全て終了状態 65 になる。

#### M83 RustKeleut: 実行時とインタプリタ(実行時が処理するエフェクトだけ)(規模 L、依存 M82)

- 成果物: `keleut-rt`(値、GC、文字列、数値の検査、Float64 の show、プリミティブ、実行時エラーの文面)、`keleut-vm`(バイトコード、インタプリタ、Console、`runtime/builtins.kirs`)、`keleut run`、`--frontend=diktor`、`conform/cram-shim/`、`keleut-conform`。
- 完了条件: `KELEUT_CONFORM=interp` の shim で、利用者の `handle` を含まない実行のコマンドが全て通る。Float64 の show の 2,000,000 個の試験が Diktor と一致する。
- 端から端までの経路は、ここで初めて Rust まで通る。

#### M84 Diktor: エフェクトの全体(規模 M、依存 M81)

- 成果物: `lower.ml` の `handle`、節の分類、`resume.tail`、`tail` の印。`kir_eval.ml` のハンドラ(`Effect.Deep` に写す。入れ子の `resume` は背骨の経路、D518)。
- 完了条件: `tools/via-ir.sh` で cram の全ての実行のコマンドが差分なしになる。ただし入れ子の `resume` の短い経路の 1 件(V191)は既知の差として登録する。
- M83 と並行して進められる。

#### M85 RustKeleut: インタプリタのエフェクト(規模 L、依存 M83、M84)

- 成果物: 区間、ハンドラの記録、T/N/G、巻き戻しフレーム、`cancel` の抑制と記録、スタックの限度、shim の `OCAMLRUNPARAM` の写し。
- 完了条件: `KELEUT_CONFORM=interp` の shim で cram の全ての実行のコマンドが通る(`known-diffs.toml` の登録を除く)。`never_ops.t` の cancelperform、cancelnormal、cancelsame、cancelresume、cancelresumevia、cancelresumetwo が個別に通ることを実施記録に書く。

#### M86 仕様の観点の試験とコーパスの整備(規模 S、依存 M85)

- 成果物: 〔fid〕の試験プログラムを期待値つきで `conform/corpus/` に足す、`keleut-conform corpus`、CI 用の 1 本のスクリプト `rustkeleut/conform/run-all.sh`、`match` の決定木(`keleut-ir` の共通の下ろし。バックエンドが分かれる前に入れる)。
- 完了条件: `run-all.sh interp` が緑。段 A の完了。

#### M87 LLVM: 関数単位の JIT(規模 L、依存 M86)

- 成果物: `keleut-llvm`、入口の表、スタブ、`tailcc` の規約、状態の語と `YIELD`/`UNWIND` の戻り、ネイティブのスタックの検査、`--tier=jit`。対象は `handle` を含まない関数(D547)。
- 完了条件: `run-all.sh jit` が緑。`KELEUT_NATIVE_STACK=512k` で `tail_calls.t` が通る。

#### M88 段の切り替え(規模 M、依存 M87)

- 成果物: 呼び出しの回数、別スレッドのコンパイル、入口の書き換え、`--tier=auto` と `mixed:SEED`、しきい値の計測と既定値の決定。
- 完了条件: `run-all.sh mixed` が `mixed:1`〜`mixed:4` で緑。インタプリタと JIT の境目を末尾呼び出しで 1e8 回またぐ試験と、G の継続が JIT のフレームを含む試験が通る。

#### M89 LLVM: ハンドラを含む関数と最適化(規模 M、依存 M88)

- 成果物: `handle` を含む関数のコンパイル、T の節の速い経路、レコードのインラインキャッシュの JIT 版、性能の計測の記録。
- 完了条件: `run-all.sh jit mixed` が緑。〔fid〕の `tail.kel`、`tres.kel`、`mutual.kel` の時間を Diktor と比べた表を実施記録に書く。

#### M90 WasmGC: N と T の節まで(規模 L、依存 M86)

- 成果物: `keleut-wasm`、`keleut-host.mjs`(入出力、BigInt の Float64 の show)、`keleut build --target=wasm`、`--backend=wasm`、trap の前の検査の検査器。
- 完了条件: `run-all.sh wasm` で、G の節を持たない全てのプログラムが通る。G の節を持つものは終了状態 4 の既知の差として登録する。
- M87〜M89 と並行して進められる(依存は M86 だけ)。

#### M91 WasmGC: G の節(規模 L、依存 M90)

- 成果物: 状態機械への変換(D556)、継続の構造体、巻き戻しの途中の G。
- 完了条件: `run-all.sh wasm` が緑(登録した差を除く)。段 C の完了。

#### M92 KIR 1.0 の凍結(規模 S、依存 M85、M89、M91)

- 成果物: IRSpec を 1.0 にし、`conform/kir-fixtures/` に見本を置き、版の検査を D523 の 1.0 の規則にする。
- 完了条件: `diktor --emit-ir` が KIR 1.0 を書き、`keleut ir verify` が見本を全て読む。

#### M93 フロントエンド: 字句(規模 M、依存 M92)

- 完了条件: shim の `--dump-tokens` を `keleut dump-tokens` へ向けて、`tokens.t`、`lexerr.t`、`utf8_position.t`、`string_bytes.t` が通る。diktor の README の 23〜25 行目(「他の実装との読み合わせには使えません」)を、RustKeleut との読み合わせに使う旨に改める(P232)。

#### M94 フロントエンド: 構文(規模 M、依存 M93)

- 完了条件: `ast.t`、`trailing_block.t`、`trailing_comma.t`、`syntax_error_pos.t`、`nonfeatures.t`、`reserved_word.t` が通る。

#### M95 フロントエンド: 型の核(規模 L、依存 M94)

- 完了条件: `typecheck.t`(`--no-prelude`)、`kinds.t`、`numeric.t`、`literal_default.t` が通る。

#### M96 フロントエンド: elab の全体と網羅性(規模 L、依存 M95)

- 完了条件: `--type-check` を含む全ての cram のコマンドが shim で通る(診断の本文は正規化)。

#### M97 フロントエンド: KIR の書き出しと単独の実行(規模 M、依存 M96)

- 成果物: `keleut-front` の KIR の書き出し、`--frontend=rust` を既定にする。
- 完了条件: コーパスの全てで、`keleut` の KIR のテキストと `diktor --emit-ir-text` がバイト単位で一致する。shim を M98 の列(本計画 5.1 節の表)に切り替えて、REPL を除く cram が通る。

#### M98 REPL と文書の仕上げ(規模 M、依存 M97)

- 成果物: `keleut repl`(入力ごとの KIR の単位を足し、受理しなかった入力の単位を捨てる。〔tree〕6.5 節)。
- 文書: `AGENTS.md` の Build, Test, and Development Commands に `rustkeleut/` のコマンド(`cargo build`、`cargo test`、`conform/run-all.sh`)を足す。diktor の README の 109〜111 行目の名前の由来の文はそのまま残し、目的の文だけを改めたことを実施記録に書く。
- 完了条件: shim で cram の全てのコマンドが `keleut` だけで通る。段 D の完了。

### 6.3 依存の図

```
M79 → M80 → M81 の kir_bin ──→ M82 → M83 ──┐
            M81 の kir_eval ──→ M84 ─────────┴→ M85 → M86 ─┬→ M87 → M88 → M89 ─┐
                                                          └→ M90 → M91 ────────┴→ M92 → M93 → … → M98
```

---

## 7. 台帳(V191 から)

| 番号 | 名前 | 内容 | 扱い |
|---|---|---|---|
| V191 | nested-resume | `resume(resume(n))` が、Diktor の木の評価器の短い経路と背骨の経路で観測が違う(〔fx〕R1 の測定) | KIR は背骨の経路を正とする(D518)。木の評価器を合わせるかは P237 |
| V192 | float-pattern | Float64 のリテラルパターンの等値(IEEE の `==` かビット等値か)が仕様に書かれていない(〔fid〕7 節) | P234 |
| V193 | libm-wasm | `sin cos exp log` の最下位ビットが V8 と glibc で違いうる(〔fid〕12 節) | P233 |
| V194 | wasmfx | WasmFX は Node 26 でフラグが要り `switch` で落ちる(〔wasm〕6 節) | 延期。フラグが外れたら D556 の置き換えを検討する |
| V195 | native-fiber | LLVM の G の節をネイティブのファイバーで速くする(〔tg〕6 節) | 延期。M89 の計測で G の費用が問題になったとき |
| V196 | perceus | 参照カウント(Perceus)と追跡型 GC の比較(〔tg〕案 C) | 延期。M89 の後に試作 |
| V197 | split-kir | 単位ごとの `.kir` と結合(分割コンパイル) | 延期。KIR 1.x の課題 |
| V198 | readme-oracle | diktor の README の「他の実装との読み合わせには使えません」(23〜25 行目)が、RustKeleut との読み合わせの方針と合わない(〔front〕4.2 (e)) | M93 で改める |
| V199 | wasm-depth | wasm の非末尾の再帰が既定で約 15,689 段しか伸びず、Diktor の既定(1 GiB)より大幅に浅い(〔wasm〕2 節) | 限度は処理系定義。`--stack-size` の指定で緩め、深い再帰の cram は既知の差として登録しうる |

---

## 8. リスク

| リスク | 影響 | 備え |
|---|---|---|
| 下ろし(`lower.ml`)の誤りが、RustKeleut の誤りと区別できない | 差分試験の切り分けの手戻り | Diktor の参照の IR 評価器で cram を先に通す(D525、M81、M84) |
| 辞書の明示化(D512)で、辞書の引数の順序やメソッドの位置を Diktor の評価器と違えて書く | 型クラスのプログラムの誤動作 | `dictionary_passing.t`、`premise.t`、`instance_init.t`、`classes.t` を M81 の `via-ir` の関門に入れる |
| 巻き戻しフレームと区間(D540、D541)が、cancel の中の G(Q5)を正しく表せない | `never_ops.t` の 6 つの試験が落ちる | M85 の完了条件で 6 つを個別に確かめる。Diktor の参照の IR 評価器の振る舞いと 1 歩ずつ比べる |
| 状態を返す方式(D542)の比較と分岐の費用が大きい | JIT の速さが出ない | M89 で測る。問題になれば V195 のファイバーを足す。フレームの形を変えないので、取り替えは JIT の部品に閉じる |
| `OCAMLRUNPARAM` の写し(D561)で、限度に触れる cram の結果が Diktor と分かれる | 不一致の誤検出 | M85 でフレームのヘッダの語数を調整する。調整で合わないものは既知の差に登録し、限度が処理系定義であることを理由に書く |
| 4 GiB のネイティブのスタックの予約が WSL2 の overcommit の設定で失敗する | 起動の失敗 | 予約の大きさを `KELEUT_NATIVE_STACK` で変えられるようにし、失敗したら 256 MiB で再試行する |
| WasmGC の状態機械の変換(M91)が大きくなる | M91 の遅れ | M90 で G を持たないプログラムを先に全部通す。M91 は段 B と段 D を止めない |
| JS の BigInt による Float64 の show が遅い | 数値を多く印字するプログラムが遅い | 速さは一致の要件ではない。M90 で測り、必要なら 2 の冪の付近だけ BigInt にする |
| llvm-sys の版が LLVM の主版に結び付く | LLVM 23 への更新で組めなくなる | `=221.1.1` に固定し、`llvm` の feature を外しても組めるようにする(〔llvm〕) |
| wasm-encoder の API が版ごとに変わる | 更新のたびの手直し | `=0.262.0` に固定する(〔wasm〕7 節) |
| フロントエンドの手書きの構文解析器が menhir 版と受理する言語でずれる | 構文の差 | `--dump-ast` の一致を関門にする。収束しなければ lalrpop 0.23.2 に .mly を写す(〔front〕3.2 節の案 B) |
| 判断事項(P232 以降)を所有者が覆す | 設計の変更 | 各裁定に代案を書いた。IR の形に効くのは P235 と P236 だけで、M80 の前に答えがあれば手戻りは無い |

---

## 9. 進め方の原則

- マイルストーンは本計画 6.3 節の依存の順に進める。M83 と M84、M87〜M89 と M90〜M91 は並行してよい。
- 各マイルストーンの後に検証者を置き、検証者には一時領域に写したバイナリだけを使わせる。リポジトリの編集と git の書き込みを禁じる。
- RustKeleut のコミットの scope は `rustkeleut:`、IRSpec は `irspec:`、diktor の中は既存の `diktor:` と `test:` を使う。
- cram の期待値を変えるのは diktor 側の新しい旗(`smoke.t` の usage)と新しい cram の追加だけである。既存の cram の期待値を RustKeleut のために変えない。

---

## 10. 仕様の所有者への問い(P232 から)

本計画は推奨案で進める(所有者の指示)。
答えが無ければ本計画の決定のとおりに進める。

| 番号 | 問い | 本計画の決定 | 節 |
|---|---|---|---|
| P232 | Diktor の README、opam の synopsis、AGENTS.md の「ブートストラップ」「セルフホスト版のテストオラクル」の文をどう改めるか | 「Keleut の参照インタプリタで、RustKeleut の試験の基準と KIR の参照実装を兼ねる」の趣旨に改める。名前の由来(README 109〜111 行目)は残す。過去の `doc/log` は書き換えない | 6.2 節 M79、M93 |
| P233 | `sin cos exp log` の最下位ビットの一致を、全実装に求めるか | 求めない。LangSpec §17 に「この 4 つの結果の最下位のビットは処理系定義」と足し(`sqrt` は正しく丸めた結果のまま)、差分試験は、この 4 つの結果を印字するプログラムを WasmGC の設定で既知の差として扱う。ネイティブは glibc を呼んで Diktor と一致させる | 3.4 節 D539 |
| P234 | Float64 のリテラルパターンの等値は IEEE の `==` かビット等値か | Diktor の現在の振る舞いを M80 で測り、それを IRSpec と LangSpec §11.1 に書く。測る前の推奨は IEEE の `==`(`case 0.0` が `-0.0` に一致し、NaN のパターンは一致しない)で、Float64 の `Eq` と同じにする | 1.3 節 |
| P235 | KIR に完全な型を載せるか | v1 は表現の型とエフェクト行だけにし、完全な型は節 0x81 として予約する | 1.1 節 D502 |
| P236 | `letrec` の右辺をクロージャに限ってよいか | 限る。M80 で cram とコーパスに関数でない `let rec` の右辺があるかを調べ、あれば書き出す側が先にクロージャへ包む下ろしを足す | 1.3 節 |
| P237 | 入れ子の `resume(resume(n))` について、Diktor の木の評価器の短い経路を背骨の経路に合わせるか | 合わせる(評価順どおりにする)。M84 で別のコミットにし、`resume_guard.t` か `eval.t` に 1 ブロック足す | 1.5 節 D518 |
| P238 | RustKeleut を親リポジトリの `rustkeleut/` に置くか、サブモジュールにするか | `rustkeleut/` に置く。IRSpec と同じ履歴で変えるため | 3.1 節 D528 |
| P239 | 実行時エラーの文面を Diktor と同じ日本語にするか | 同じにする(IRSpec 付録 B)。cram を書き換えずに通すため | 3.4 節 D536 |
| P240 | KIR の読み込みと検証に落ちたときの終了状態 | 65(処理系定義) | 1.7 節 |
| P241 | WasmGC は `node` を子プロセスで起動する形でよいか | よい。`keleut build --target=wasm` の出力は `node DIR/main.mjs` で単独でも走る | 3.7 節 D550 |
| P242 | JIT のしきい値の既定値 | 1000 で始め、M88 の計測で決め直して実施記録に書く | 3.6 節 D546 |
| P243 | フロントエンドの診断の本文を Diktor と一字一句そろえるか | M96 までは正規化して比べ、そろえるのは M98 の目標にとどめる(LangSpec §18 は文面を評価の結果に含めない) | 4 節 D559 |

---

## 11. 裁定の一覧

裁定は本文に番号つきで書いた。
D499〜D503 は KIR の名前、水準、形、型、エフェクト行、D504〜D507 はプログラムの単位、採番、初期化、D508〜D514 は式、パターン、クロージャ、呼び出し規約、データ、辞書、組み込みの表、D515〜D520 はエフェクト、リージョン、extern、D521〜D524 はバイナリ形、テキスト形、互換、置き場所である。
D525〜D527 は Diktor の側、D528〜D539 は RustKeleut の構成と実行時、D540〜D543 はエフェクトの実装、D544〜D549 は LLVM と段の切り替え、D550〜D556 は WasmGC、D557〜D559 はフロントエンド、D560〜D562 は試験である。
次の番号は D563、V200、M99、P244 である。
