# Diktor の精緻化木と、精緻化の後の中間言語(IR)が要るもの

調査の対象は diktor 6e49bbb(親 bcb16f1)である。
追跡済みのファイルは変更していない。
`dune build` を 1 回実行したので、追跡外の `_build/` が更新されている。
探査用のプログラムは `doc/log/261024-1-spike/probe/tree-probe/` の p1.kel〜p4.kel に置いた。

表記の約束は次のとおりである。

- **[実測]**：ソースの該当行を読んだか、コマンドを実行して確かめたこと。
- **[推論]**：実測した事実から導いた判断と提案。

## 0. 全体の要約

1. 精緻化木 `Tree.Tree` は、構文木 `Syntax.Make` に `ElabData.t` を付けたものである。
   `ElabData.t` のフィールドは oid、loc、`ty_field`、`resolved`、`dict` の 5 つで、後の 3 つは可変である(tree.ml:243-245)。
   精緻化は、構文解析が作ったノードのこの欄に書き込むだけである。
   ノードを作り直さず、脱糖も追加しない。[実測] tree.ml:239-251
2. 精緻化の後も、式のノードは構文の 25 種類のまま残る。
   残るものには、演算子、レコードの 4 操作、`Handle`、`Resume`、`Run` が含まれる。
   これを下ろす(lowering)段は存在しない。[実測] syntax.ml:1020-1045
3. 評価器(interp.ml)は木だけでは動かない。
   次のものを木の外から引いている。[実測]
   - インスタンスの頭の解決(`Decls.resolve_class` と `resolve_con`)
   - クラスメソッドの辞書の位置(`Unify.constraints_of` でスキーマから計算する)
   - 組み込みインスタンスの表(`Builtin.builtin_method`)
   - プリミティブの表(`Builtin.find_extern`)
   - 局所変数の名前を文字列で引く表
4. 同一性を表す値は、どれもプロセスの中でしか通用しない。
   `Type.intern` の oid、`GDecl` の束縛ノードの oid、穴の `h_id`、ハンドラの起動ごとの `inst` がそうである。
   そのため、バイナリの IR は独自の名前付けを持つ必要がある。[推論]
5. ノードの型(`ty_field`)は、式とパターンと let 束縛のノードに付いている。
   ただし一般化した後の共有された参照で、`Generic` と `Link` を含む。
   自由変数の集合、レコードの配置、末尾位置の印は、木のどこにも無い。[実測]

---

## 1. 精緻化の後に残る構文と、IR に要る構文

### 1.1 式(`exp'`、25 種類)[実測] syntax.ml:1020-1045

| 構文 | 精緻化が付ける情報 | 評価器の扱い(interp.ml) |
|---|---|---|
| `Bool` / `Text` | 型だけ | 定数 |
| `Number n` | 型(既定化した後の Int32、Int64、Float64) | `number_value` が評価のたびに `n_text` を型で読み直す(:218-230) |
| `Ident li` | `RVar gref`(トップレベルの値)、`RCtor`(引数のないコンストラクタ)、`DUse hs`(辞書の穴) | locals → `RVar` → `RCtor` の順に引く(:301-313) |
| `Hole` (`???`) | 型だけ | 実行時エラー |
| `Apply (f, argrec)` | 型。引数は常に `_item` 行のレコード式 | 左から右に評価する(:315-319) |
| `Construct (li, args)` | `RCtor (data, ctor, arg→field)` | ソースの順に評価し、宣言の順に格納する(:320-330) |
| `Variant (s, e)` | 型だけ | `VVariant (intern s, v)` |
| `BinOp (l, op, r)` | `DUse [h]`(`&&` と `||` には付かない) | `Prims.bin_op_sem` の表で `Cls.m` を呼ぶ。`!=` は `Eq.eq` の否定(:332-350) |
| `Not e` | 型 | 組み込み |
| `Neg e` | `DUse [h]` | `Neg.neg` を呼ぶ |
| `Lambda {l_params; l_body}` | 型(矢印の型にはエフェクト行が入る) | `VClosure`。環境をまるごと捕まえる |
| `Let (b, e)` / `LetRec (bs, e)` | 束縛ノードに型と `DAbs ks` | §14.8 の 3 段のバックパッチ |
| `Seq es` | 型 | 最後の要素だけが末尾位置 |
| `Match (e, clauses)` | 節にガードを付けられる | 節を上から順に照合する。決定木は作らない |
| `RecordEmpty` / `RecordExtend (rest, l, v)` | 型 | 先に v、後に rest を評価する(:388-392) |
| `RecordUpdate` / `RecordRestriction` / `RecordSelection` | 型 | 最も左の同名ラベルが対象になる(value.ml:322-358) |
| `Perform (li, arg)` | `ROp fullname` | `Effect.perform (Op (oid, v))` |
| `Handle (body, clauses)` | 節ごとに `ROp`、`RNeverOp`、`RReturnClause`、`RCancelClause` のどれか | `match_with` 1 つ(§14.9) |
| `Resume (e option)` | 型 | 背骨の外にあるときは `continue`。背骨の上にあるときは `eval_clause_tail` が扱う |
| `Run (h, e)` | 型 | 恒等写像(:413) |

構文解析の段階で脱糖されるので、木に現れないものは次のとおりである。[実測] syntax.ml の §1.16 の注釈、parser.mly:344-385
`if` は `Boolean` に対する 2 節の `Match` になる。
タプルは `_item` ラベルを積んだ `RecordExtend` の列になる。
`with` と末尾ブロックは、呼び出しの引数の行の末尾に足した `fn() => 残り` か `fn(pat) => 残り` になる(`append_last_arg` を共有する)。
パターンを左辺に持つ値束縛は `let_binding.lb_name : pat` のまま残る。

### 1.2 パターン(`pat'`、9 種類)[実測] syntax.ml:941-952

`PWildcard`、`PVar`、`PBool`、`PNumber`、`PText`、`PRecord (fields, rest option)`、`PCtor`、`PVariant`、`PAnnot` の 9 種類である。

- `PRecord` は第 2 要素の値で 3 つの状態を表す。
  `None` は閉じたタプル、`Some PWildcard` は開いたレコード、`Some p` は `...rest` である。
- `PCtor` には `RCtorPat (data, ctor, field→arg option)` が付く。
  照合では、`None` のフィールドに触れない(interp.ml:179-197)。
- `handle` の節の頭も `PCtor` の形をしている。
  節の分類は、節のノード(clause)の `resolved` に書かれる。
- `PNumber` の照合は、値の型で字面を読み直して比べる(:160-165)。
  精緻化は宣言の終わりに `check_pattern_numbers` を実行し、読めない字面を型エラーにする(elab.ml:248-)。
  そのため、型検査を通った後は、パターンの数値を定数に確定させてよい。[推論]
- `PAnnot` は実行時には何もしない。

### 1.3 宣言(`decl'`、10 種類)[実測] syntax.ml:1162-1172

| 宣言 | 実行時の扱い(interp.ml の `exec_decl`、:1313-1409) |
|---|---|
| `DType` / `DNewtype` / `DEffect` / `DClass` | 何もしない。実行時の表は Decls から作る |
| `DLet b` | 束縛ノードが `DAbs` を持てば `VDictAbs`(使うたびに評価する)。持たなければ評価して globals に置く |
| `DLetRec bs` | globals を共有するのでバックパッチしない |
| `DExp e` | `DAbs` を持てば評価しない(LangSpec §6.4) |
| `DInstance i` | 頭を**実行時に** Decls の同義語表で解決し、`instance_impls (cls, con)` に書く |
| `DExtern ex` | `Builtin.find_extern ~abi ex_prim` で実装を引く。見つからなければ、呼んだ時点で実行時エラーにする |
| `DModule` | 平坦化の後には到達しない(bug) |

import の宣言は `decl'` に含まれない。
単位の先頭の `import_decl list` として別に持つ(syntax.ml:136-138、driver.ml:460-467)。

### 1.4 IR に要る構文の最小集合 [推論]

Diktor の木をほぼ 1 対 1 に写すとすれば、IR に要る式の種類は次のとおりである。

- 定数(型付きで、値は確定済み)
- 局所変数、大域の実体、引数のないコンストラクタ
- 適用(引数レコード)と、末尾適用の印
- コンストラクタの構築(格納の位置を確定したもの)
- ヴァリアントの構築
- メソッドの呼び出し(辞書、クラス、メソッド名)
- 短絡の `&&` と `||`、`!`
- クロージャ
- let と letrec、Seq
- Match(節、パターン、ガード)
- レコードの 5 操作
- perform、handle(節の種類つき)、resume(末尾 resume の印つき)
- 辞書の適用(`DUse`)と辞書の抽象(`DAbs`)

`Run` は消してよい。
ただし、リージョンの検査を IR の検証器で繰り返すのでなければ、という条件が付く。
`BinOp` と `Neg` は、メソッドの呼び出しに下ろしてよい。
LangSpec §5.7 は、演算子の適用を末尾呼び出しとして保証しない。
そのため、この下ろし方で意味は変わらない。

---

## 2. IR が運ぶ必要のある事実と、Diktor での所在

| 事実 | Diktor の所在 | 備考 |
|---|---|---|
| トップレベルの名前の解決 | `RVar (GBuiltin s \| GMethod "Cls.m" \| GDecl (bindnode_oid, name))`。書くのは elab.ml:1961, 1981 | 同名の再束縛は別の `GDecl` になる。oid はプロセスの中でしか通用しない |
| 局所の名前 | **解決されていない**。評価器が `SMap` を文字列で引く(interp.ml:303) | elab の `add_local` は、局所の名前で `tops` を隠す(elab.ml:136)。そのため、`RVar` と局所の名前が衝突することはない |
| 完全な操作名 | `ROp (intern "Eff.op")`(elab.ml:2313)、節の `ROp` / `RNeverOp`(elab.ml:3063) | 起点でない単位のエフェクトは `%N.Eff.op` と綴る(decls.ml:380) |
| 節の種類 | `ROp`、`RNeverOp`、`RReturnClause`、`RCancelClause`(elab.ml:3063, 3188, 3199) | `RNeverOp` は返り値の型が `Never` の操作である。`return` 節と `cancel` 節にはガードを書けない |
| コンストラクタのフィールドの順 | `RCtor (data, ctor, arg→field int array)`、`RCtorPat (data, ctor, field→arg option array)`(elab.ml:2528, 1766) | 値は `VData {d_ctor; d_fields}` で、型の oid を持たない |
| 辞書の証拠 | `DUse hs`(使う位置)、`DAbs ks`(束縛、extern、式文)、`DMethod (own, mk, adapter)`、`DInstance ks`(tree.ml:192-197) | 穴 `hole.h_sol` を `EvParam`、`EvInst`、`EvRecord`、`EvVariant`、`EvHole` の木として解く(tree.ml:183-190) |
| 辞書のパラメータの順序 | 正準順は `Unify.constraints_of` が決める(unify.ml:1261-1289)。型を左から深さ優先にたどり、`Generic` 変数が初めて現れた順に並べる。同じ変数の中では、予約述語を除いた `vcls` の順に並べる | `DUse` の穴の列と `DAbs` の鍵の列は、どちらもこの順序である |
| 数値リテラルの型 | ノードの `ty_field` を `repr` した結果の `TCon` の頭(interp.ml:218-230) | 値は木に無い。評価のたびに OCaml の `Int32.of_string` で読み直す |
| 末尾位置 | **印は無い**。評価器が OCaml の末尾呼び出しを保つことで実装している(interp.ml の冒頭の規約 1) | — |
| 末尾 resume | **印は無い**。`eval_handle` が起動のたびに `spine_has_resume` で調べる(interp.ml:939-947, 958) | 背骨は Seq の最後、Let と LetRec の続き、Match の節本体、`&&` と `||` の右辺、`Run` の本体である。`Handle` と `Lambda` には入らない。LangSpec §5.7 の「末尾 resume」と一致する |
| インスタンスの頭 (cls, con) | **木に無い**。`exec_decl` が `Decls.resolve_class` と `resolve_con` で引き直す(interp.ml:1334-1342) | `with_decl_scope` で、宣言の出身の module と単位を立ててから引く |
| メソッドが受け取る辞書のうち、クラスの辞書の位置 | **木に無い**。`method_selector` がスキーマから計算する(interp.ml:1236-1254) | — |
| extern の実装の鍵 | `ex_abi` と `ex_prim`(修飾しない元の名前)。syntax.ml:1149-1158 | 大域の鍵は `GDecl (declnode_oid, 修飾した ex_name)` |
| 単位と module の出身 | `Decls.decl_unit` と `decl_module`(oid → 単位、oid → module)。decls.ml:365, 475 | 評価器が引くのは、インスタンスの頭を解決するときだけである |

### 2.1 書き込みの位置の一覧 [実測]

```
$ grep -n "set_resolved\|set_dict" lib/elab.ml
1766 RCtorPat / 1961,1981 RVar / 1964,1984,2149,2161,4203 DUse / 2313 ROp
2528 RCtor / 3063 ROp|RNeverOp / 3188 RReturnClause / 3199 RCancelClause
3708,4169,6052,6105 DAbs / 5392 DInstance / 5435 DMethod
```

`set_ty` を呼ぶのは次の位置で、clause と decl のノードには型を書かない。
- 式:elab.ml:1891、2628、2640
- パターン:elab.ml:1660
- let 束縛:elab.ml:3650、4142

---

## 3. 木に無いが、型付きの IR やコンパイルする IR が欲しい事実

| 欲しい事実 | 現状 [実測] | 足す難しさ [推論] |
|---|---|---|
| ノードごとの型 | 式、パターン、let 束縛にはある。ただし共有された可変の参照で、`Link` の鎖と `Generic` を含む | 書き出すときに `repr` と正規化を施し、`Generic` を型パラメータの番号に写せばよい。小さい作業である。注意点は 2 つある。多相な関数の内側の型には `Generic` が残る。行変数とエフェクト行も同じ表現(`TRowExtend`)で残る |
| 自由変数(クロージャが捕まえる変数) | 無い。`VClosure` が `env` の全体(locals の Map と dicts)を捕まえる | 局所のスコープの解決(名前をスロットに写す)と一緒に、1 回の走査で求められる。辞書のパラメータ `dicts` も捕まえる対象に含める必要がある。中程度の作業で、OCaml で 300〜500 行と見込む |
| 関数のエフェクト行 | `Lambda` と束縛のノードの型 `TArrow (_, _, eff)` にある | 書き出すだけでよい。ただし開いた行は行変数である |
| レコードの配置 | 無い。値は順序付きの連想リスト `VRecord of (oid * t) list` で、先頭が最新である(value.ml:87) | Scoped Labels(同名のラベルの重複を許し、最も左を選ぶ)と行多相があるので、静的な配置は一般には決まらない。閉じた行で単相な場所だけを最適化する経路を作ることになる。IR の段では、ラベルで操作する命令のまま残すのが妥当である |
| 構造的な Eq に要る行の順 | `EvRecord` と `EvVariant` の証拠が、型の行の順のラベルと辞書を持つ(interp.ml:546-557) | 証拠をそのまま IR に書けばよい |
| 末尾呼び出しの印 | 無い | LangSpec §5.7 の規則を構文的にたどれば求まる。100〜200 行と見込む |
| 末尾 resume の印 | 無い。起動のたびに計算する | 同じ走査で求まる |
| 局所変数の解決 | 無い。文字列で引く | 上の「自由変数」と同じ段で求める |
| インスタンスの頭 | 実行時に解決する | 書き出す段で `Decls.with_decl_scope` を立てて解決し、(cls, con) の綴りを書けばよい。小さい作業である |
| メソッドの辞書の位置 | 実行時に計算する | 書き出す段で `constraints_of` を使って計算すればよい。小さい作業である |
| 数値リテラルの値 | 評価のたびに読み直す。範囲外は実行時エラーになる | 書き出す段で読み、読めなければ「評価したら実行時エラー」を表す命令にする。LangSpec §4.2 は、範囲外の字面を「評価したときに」実行時エラーにすると定める。そのため、型検査の段の誤りにしてはならない |
| パターンを照合する決定木 | 無い。上から順に照合する | IR には不要と考える。後段の最適化で作れる |
| 穴の解 | `h_sol` を経由する `EvHole` の鎖 | 書き出すときに平坦化する。`closed_ev` で辞書パラメータを含まないと分かる証拠には、定数の印を付ける |
| 未解決の穴 | `h_sol = None` の穴を評価すると `VDict DPending` になる(interp.ml:492)。value.ml によれば、これを作るのは REPL の表示だけである | REPL の外では、書き出す段で未解決の穴を拒否すればよい |

---

## 4. 実行時の値の表現 [実測] value.ml:81-110 ほか

### 4.1 値

値の型は `VBool`、`VInt32`、`VInt64`、`VFloat64`、`VText`、`VRecord`、`VVariant`、`VData`、`VClosure`、`VPrim`、`VRef`、`VArray`、`VMutArray`、`VDict`、`VDictAbs` の 15 種類である。

- 数値の型は Int32、Int64、Float64 の 3 つだけである(LangSpec §4.2、elab.ml:274 の `absent_numeric`)。
- 文字列は OCaml の `string`、つまりバイト列である。`__string_length` はバイト数を返す。
- Unit は `VRecord []` である。
- 関数の呼び出し規約は、引数を `_item` の行のレコード 1 個に詰めて渡す形に統一されている(value.ml:156、interp.ml:431-446)。

### 4.2 辞書

辞書は `VDict` で、次の 4 つの形を持つ。

- `DInst (cls, con, 前提の辞書, メソッドのキャッシュ)`
- `DRecord`
- `DVariant`
- `DPending`(REPL の表示のためだけに作る)

辞書を受け取る値は `VDictAbs {da_arity; da_fn; da_last}` で表す。
`da_last` は、直前に受け取った辞書の列を物理等価で比べ、同じなら前の結果を返す 1 件のキャッシュである。

メソッドの選択は、呼ばれた時点で `instance_impls (cls, con)` を引いて行う。
表に無ければ、`Builtin.builtin_method` の組み込みの表に戻る(interp.ml:495-526)。
組み込みの表の行数は、`builtin_method_prims` が 39、`builtin_method_direct` が 5 である(builtin.ml §13.5)。

インスタンスのメソッドの呼び出し規約は次のとおりである(interp.ml:1343-1380、:499-507)。[実測]
メソッドの実装は、辞書の列 `前提の辞書 ++ メソッド自身の辞書` を受け取る関数である。
前の部分は `DInstance pk` の鍵に、後の部分は `DMethod` の `mk` の鍵に束縛する。
そのうえで束縛の値を評価し、`adapter` の穴を評価した辞書を実装に渡す。
`pk` と `mk` がどちらも空なら、宣言を実行したときに 1 回だけ評価する。
`select_method` は、`DInst` の前提の辞書 `ps` と、セレクタが受け取った残りの辞書 `own` を `ps @ own` の順に並べて渡す。
クラスのセレクタ `method_selector` は、受け取った辞書の列からクラスパラメータの位置の辞書を取り出し、残りを `own` にする。
[推論] この並びが、IR でのメソッド呼び出しの ABI になる。

辞書のパラメータは、環境の `dicts : DMap (vid, cls_oid)` に置く。
クロージャは環境とともに辞書も捕まえる。

### 4.3 辞書を受け取る値束縛

`DAbs` の付いた値束縛は、名前を使うたびに初期化式を評価する(interp.ml:585-588、LangSpec §6.4)。
これは最適化ではなく、仕様が定める意味論である。
IR には、0 引数の「辞書を受け取る関数」として書く必要がある。[推論]

### 4.4 レコード(Scoped Labels)

- 選択と更新は、最も左の同名ラベルを対象にする。
- 制限は、最も左のフィールドを 1 つ消す。消した後は、隠れていた同名のフィールドが見えるようになる。
- 更新はフィールドの物理的な位置を保つ。

実測の結果を次に示す。
```
r2 : {x: String, x: Int32, y: Int32}
a : String ("s")
b : Int32  (1)  ← (r2 \ x).x
```

### 4.5 構造的ヴァリアント

値は `VVariant (label_oid, payload)` で、ペイロードは常に 1 つの値である。

### 4.6 構造的な等価

閉じた行の型だけが対象である。
`EvRecord` の行の順にラベルを取り、両辺から最も左のフィールドを取り出して比べる(interp.ml:546-552)。
したがって、`{p=1,q=2} == {q=2,p=1}` は true である(p1 で実測)。

開いた行を比べようとすると型エラーになる(p1 の最初の版で実測)。
```
! p1.kel:7:10: 型エラー: 行変数を含む型 #Foo(Int32) | _R1 に Eq の構造的導出は適用できません(行が閉じていません)
```

### 4.7 リージョン

`Ref` は `VRef of t ref`、`MutableArray` は `VMutArray of t array` である。
`freeze` はコピーを作って `VArray` を返す(interp.ml:1192-1196)。

`Run` は実行時には恒等写像で、安全性は型(剛定数とレベル)だけが保証する。
`run` の外の `MutableArray.new` は型エラーになる(p4 で実測)。
```
! p4.kel:1:9: 型エラー: ラベル Heap がありません(行は閉じています)
```

組み込みの操作は `GBuiltin "Ref.new"` などの名前で globals に置く。
該当するのは、`Ref` の 3 操作、`Array` の 3 操作、`MutableArray` の 5 操作、`pinned` である(interp.ml:1146-1208)。

### 4.8 extern "C"

本物の FFI は無い。
`sin`、`cos`、`sqrt`、`exp`、`log` の 5 つを OCaml で実装した表があるだけである(builtin.ml `c_prims`、LangSpec §17)。
ほかの名前を宣言することはできるが、呼ぶと実行時エラーになる。
制約付きの型パラメータを持つ extern は、受け取った辞書を捨てる(interp.ml:1398-1404)。

### 4.9 プリミティブ

`extern "prim"` の実装は `Builtin.prims` の表にある。
表の行数は 47 で、型はプレリュードの extern 宣言が与える(prelude.kel:251-300)。

### 4.10 プレリュードと組み込み

型クラスと組み込みの値は、Decls の初期化時に登録される(decls.ml の `builtin_value_refs`、:1203-1214)。
プレリュードが宣言するのは次のものである。
- `Option`、`List`
- エフェクト `Console` と `Print`
- extern の宣言
- `with_stdout`、`println` など
- `Show` と `Eq` のインスタンス(`List` と `Option` に対するもの)

最も外側のハンドラ `Builtin.with_runtime` は、`Console.write` を受け取って sink へ送る。
REPL では `Print.print` も sink へ送る(builtin.ml §13.6)。

### 4.11 エフェクト

Diktor は Keleut のエフェクトを OCaml 5 の `Effect.Deep` にそのまま写す(interp.ml の冒頭の表)。

- 継続を捨てるときは `Unwind (inst, UValue v | UClause f)` を送る。
- 起動ごとの `inst` で、ハンドラの起動を区別する。
- `cancel` 節は `exnc` の中で走らせ、中で起きた例外を抑制して記録する。
- `r_used` と `r_alive` で、resume のアフィン性(高々 1 回)と second-class を実行時に検査する。

[推論] これらは、RustKeleut の IR では明示的な命令とランタイムの規約として書き直す必要がある。
該当するのは、handle の枠、操作節の選択(上から順に試して次へ進む)、3 つの経路(resume した後の値、resume しない値、例外)、never 節で先に巻き戻すこと、cancel 節の抑制である。
Diktor は、意味論のこの部分を OCaml のランタイムに暗黙に頼って実装している。

---

## 5. `--emit-ir` と IR の読み込みを差し込む位置、および規模の見積もり

### 5.1 差し込む位置 [実測] driver.ml

- `run_with` の `Run` の分岐(:1176-1197)は、次の順に処理する。
  1. `let units = type_check_files ~quiet:true options`
  2. `Interp.run_units ~sink:print_string units`

  `units : T.decl list list` は `prelude :: flat`(driver.ml:820)である。
  先頭がプレリュードで、その後に単位が帰りがけ順に並び、起点が最後に来る。
  **この 2 つの間が書き出しを差し込む位置になる。**
  型検査が終わった時点で、Decls の表(同義語、クラス、単位)は揃っている。
  そのため、インスタンスの頭の解決とメソッドの辞書の位置も、ここで計算できる。[推論]
- 旗を足すには、`mode` に変種を足し(:91)、`parse_args` に節を足す(:139-158)。
  `usage` の文言も変える必要がある。
  `test/smoke.t` が `usage` の全文を期待値に持つので、ゴールデンの更新が要る(driver.ml:66-68 の注釈)。
- API の経路 `eval_string`(:705-713)は、CLI と同じ経路を通す方針である(§16.5)。
  IR の経路にも、対になる関数を用意する必要がある。[推論]
- 読み込みには新しいモードを足す(例:`--run-ir FILE`)。
  型検査を通らずに、`Ir_load.read` から `Ir_eval.run` を呼ぶ。
  ただし、プレリュードの扱いを先に決める必要がある。
  IR にプレリュードを含めて自己完結させるか、プレリュードを別の IR 単位として先に読むかである。[推論]

### 5.2 規模の見積もり [推論。根拠は実測した行数]

実測した現在の行数は次のとおりである。
`col0-comment-block-lines` は、行の先頭(0 桁目)から始まるコメントの塊の行数である。
```
interp total=1618 col0-comment-block-lines=717 code-ish=901
driver total=1429 col0-comment-block-lines=657 code-ish=772
elab   total=6972 col0-comment-block-lines=3206 code-ish=3766
```

| 部品 | コードの行数(見込み) | 文芸的な注釈を含めた行数(見込み) |
|---|---|---|
| IR の型定義(ir.ml) | 150〜250 | 400〜600 |
| 木から IR への下ろし(局所のスロット化、自由変数、末尾と末尾 resume の印、証拠の平坦化、インスタンスの頭、メソッドの辞書の位置、リテラルの確定、名前の安定化) | 900〜1400 | 2000〜3000 |
| バイナリの書き出し | 300〜500 | 600〜1000 |
| バイナリの読み込みと検証 | 400〜700 | 800〜1400 |
| IR の評価器(interp.ml の約 900 行と同じ程度) | 900〜1300 | 1800〜2600 |
| driver の結線と旗 | 60〜120 | 150〜250 |
| 合計 | **約 2700〜4300** | **約 5800〜8900** |

組み込みの表を IR の仕様の側でデータとして定めれば、両方の実装の整合を保ちやすい。
対象は、`prims`(47)、`c_prims`(5)、`builtin_method_*`(44)、組み込みの値(12)、組み込みのクラスである。[推論]

---

## 6. 初期化の順、単位、REPL の状態(IR のリンク単位に関わること)

### 6.1 単位の順 [実測]

- `load_units` は、起点のファイル群から import を深さ優先にたどり、帰りがけ順に単位を並べる(driver.ml:563-595)。
  ファイルの同一性は `realpath` で決め、循環は誤りにする。
- 単位の番号は、依存される側から 1、2、… と振る。
  起点は 0、標準環境は -1 である(decls.ml:355-357)。
- コマンド行で渡した複数のファイルは、連結して 1 個の起点の単位にする(driver.ml:809)。
- LangSpec §15.4 は、初期化の順を「起点から import を書いた順に深さ優先でたどったときの帰りがけ順」と定める。
  各ファイルは 1 回だけ初期化し、起点を最後に初期化する。

### 6.2 名前の綴り [実測] decls.ml:316-339, 380

起点でない単位の型、コンストラクタ、エフェクト、クラス、module の名前には、`%N.` の印を付けて intern する。
module の中の宣言は `%N.M.T` になる。

次のものには印を付けない。
- トップレベルの値(束縛ノードの oid で区別する)
- レコードとヴァリアントのラベル
- 短い操作名

[推論] 操作名は、`ROp` が完全な綴りを持つので、`%N.Eff.op` の形になりうる。
IR の名前空間では、単位の印と、単位の中の宣言の番号を、安定した鍵として使うのが自然である。

### 6.3 単位の中の初期化 [実測]

単位の中では、宣言を書いた順に `exec_decl` で実行する(interp.ml:1445)。

globals は可変のハッシュ表で、実体を呼び出しの時点で引く。
そのため、前方参照は遅延束縛で通る。
初期化の前に呼ぶと「未束縛の変数」の実行時エラーになる(LangSpec §6.4、interp.ml §14.14)。

インスタンスの表も、宣言の順に書く。
宣言より前の位置でインスタンスを使うと、実行時エラーになる(interp.ml:512-517)。

[推論] したがって IR は、大域の実体を「初期化済みかどうかの印を持つセル」として表す必要がある。
単位の初期化コードは、宣言の順に並べた命令列になる。

### 6.4 プログラムの開始 [実測]

`start_program` は、組み込みの値とクラスのメソッドのセレクタを globals に置く(interp.ml:1435-1443)。
クラスのセレクタは、`Decls.classes` を名前順に並べて登録する。

その後、`with_runtime` の中で単位を順に実行する。
型検査に渡すときはプレリュードを分けて扱うが、実行するときは連結する(driver.ml §16.7)。

### 6.5 REPL [実測]

REPL には、入力ごとの取り消しの機構が 4 つある。
1. `Interp.journal` と `jreplace`:globals と `instance_impls` への書き込みを記録して戻す(interp.ml:1297-1307, 1556-1618)
2. `Decls.snapshot`
3. `Elab.snapshot`
4. 単一化のセルの trail(`Type.trailing`)

入力は前の入力の後の環境から始まり、受理しなかった入力の書き込みは戻す(LangSpec §6.6)。
`:reset` は、プレリュードだけを処理した状態に戻す(`Decls.reset`、driver.ml:1090)。

REPL では、トップレベルの行に `Print` が加わり、`Print.print` を出力先へ送る。

[推論] IR の段で REPL を扱うなら、入力ごとに追加の IR 単位を作る形になる。
受理しなかった入力は、その単位を捨てるだけで取り消せる。
ただし、一般化しなかった束縛の型変数が後の入力で決まる規則(LangSpec §6.6)がある。
そのため、型付きの IR では、前の入力の単位の型が後から確定しうることに注意が要る。

---

## 7. 実測した探査の記録

```
$ diktor --type-check p1.kel   (修正後)
r : {x: Int32, y: Int32}
r2 : {x: String, x: Int32, y: Int32}
...
twice : [A: Add] (A) => A @ {}
t : (Int32, Float64, Int64)
$ diktor p1.kel → s / 1 / true / true / 42 / 3.0 、exit=0
$ diktor --type-check p2.kel   (let f = Some)
! p2.kel:1:9: 型エラー: コンストラクタ Some の引数が不足しています(式では全フィールド必須)
$ diktor p3.kel   (let k = 4294967295)
k : Int32 は型検査を通り、実行すると「実行時エラー: 数値リテラルが範囲外です: 4294967295」、exit=3
$ diktor --type-check p4.kel   (run の外の MutableArray.new)
! p4.kel:1:9: 型エラー: ラベル Heap がありません(行は閉じています)
```

p2 から、コンストラクタは第一級の値ではないことが分かる。
式の中では、すべてのフィールドを与えて呼ぶ必要がある。
したがって、IR にコンストラクタ関数を用意する必要は無い。

p3 は、範囲外のリテラルを実行時に検査する必要があることを示す。
