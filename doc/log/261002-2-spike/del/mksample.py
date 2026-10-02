import sys
src = open('/home/sumito3478/repo/keleut/doc/sample.kel').read().split('\n')
L = {i+1: l for i, l in enumerate(src)}
edits = {}  # line -> list of replacement lines (None = keep)
def rep(n, new, old_prefix=None):
    if old_prefix is not None: assert L[n].startswith(old_prefix), (n, L[n])
    edits[n] = new if isinstance(new, list) else [new]
def rng(a, b, new, first=None):
    if first is not None: assert L[a].startswith(first), (a, L[a])
    edits[a] = new
    for k in range(a+1, b+1): edits[k] = []
# §1
rep(59, "// type Unit = {}                   // 標準ライブラリが持つ空レコード。`()` も同じものを指す", "type Unit  = {}")
# §6
rep(237, "// コンストラクタは複数の値を取れるので、値を二重に包まずに済む。右辺を省略すると、コンストラクタを持たない型になる（§13 の BigInt）。下の 3 つは標準ライブラリが持ち、宣言し直せない。", "// コンストラクタは複数の値を取れる")
rep(238, "// newtype Option[A] = Some(A) | None", "newtype Option[A]")
rep(239, "// newtype List[A]   = Cons(head: A, tail: List[A]) | Nil", "newtype List[A]")
rep(240, "// newtype Never                                 // コンストラクタを持たない型", "newtype Never")
# §8
rep(372, "// type class Add[A] { val add: (A, A) => A }", "type class Add[A]")
rep(373, "// type class Mul[A] { val mul: (A, A) => A }", "type class Mul[A]")
rep(374, "// （上の 2 つのクラスと下の 3 つのインスタンスは標準ライブラリが持ち、宣言し直せない）", "")
rep(375, "// type instance Add[Int32]  { let add(x, y) = __int32_add(x, y) }", "type instance Add[Int32]")
rep(376, "// type instance Mul[Int32]  { let mul(x, y) = __int32_mul(x, y) }", "type instance Mul[Int32]")
rep(377, "// type instance Add[String] { let add(x, y) = __string_concat(x, y) }", "type instance Add[String]")
rep(384, "// 構造的な導出を持つのは標準ライブラリの Eq だけで、導出を指定する構文はない。誰でも任意のクラスに導出を書けるようにすると、", "// 導出規則は、クラス宣言に 1 つだけ付ける")
rep(386, "// 標準ライブラリの宣言（構造的な導出は組み込みで、宣言には現れない）：", "type class Eq[A] {")
rep(387, "//   type class Eq[A] {", "  val eq:")
rep(388, "//     val eq: (A, A) => Boolean", "  derive structural")
rep(389, "//   }", "}")
rep(397, "// 構造的導出を持つ Eq のクラスパラメータのカインドは Type である。", "// `derive structural` を書けるのは")
rep(399, "// 利用者は導出を指定できないので、型引数を取るクラスに構造的導出が付くことはない。", "// カインドで判別できるので")
# §9
rep(463, "// effect Print   = { print: (String) => Unit }", "effect Print")
rep(464, "// effect Console = { write: (String) => Unit }   // ランタイムが提供する低レベルのエフェクト", "effect Console")
rep(465, "// （上の 2 つは標準ライブラリが持ち、宣言し直せない。ファイル入出力のエフェクトは標準ライブラリにない）", "effect Fs")
rep(536, "// ランタイムが実装を持つエフェクト（Console）は、利用者が handle で処理できない。", "// ランタイムが実装を持つエフェクト（Console、Async、Fs）")
rep(537, "// 利用者の handle で横取りできると、出力を握りつぶすハンドラが書けてしまうからである。", "// 「ランタイムが実装を持つ」")
rep(538, "// 操作を持たないラベル（Heap、Blocking）は、handle の節を書けないので、この規則とは関係しない。", "// もう 1 つは")
rep(539, "// Blocking は §12 の pinned で取り除く。", "// （Fs と、§9 の")
rep(540, "// perform はできる。出力先を差し替えたいときは、", "// perform はできる")
rep(541, "// Print のような自前のエフェクトを処理する（下の capture が例）。特別な印は設けず、名前で定める。", "// Print のような自前のエフェクトを処理する")
rng(595, 602, [
 "// ファイル入出力は標準ライブラリにない。ここでは C のバインディングとして宣言し、ブロックしうるので @ Blocking を付ける（§12）。",
 "extern \"C\" let file_open(path: String): Int32 @ Blocking",
 "extern \"C\" let file_read(h: Int32): String @ Blocking",
 "extern \"C\" let file_write(h: Int32, s: String): Unit @ Blocking",
 "extern \"C\" let file_close(h: Int32): Unit @ Blocking",
 "// サブエフェクティングがないので、ハンドラの外側の行は、body の行から File を除いたものと一致する。",
 "// with_file 自身が file_open と file_close で Blocking を起こすので、外側の行には Blocking が含まれる。そのため、body の行にも Blocking を書く。",
 "let with_file[A, E](path: String, body: () => A @ {File, Blocking extends E}): A @ {Blocking extends E} = {"], "// ファイルのプリミティブはランタイムが提供し")
rep(603, "  let h = file_open(path)", "  let h = __open(path)")
rep(605, "    case read()    => resume(file_read(h))", "    case read()")
rep(606, "    case write(s)  => resume(file_write(h, s))", "    case write(s)")
rep(607, "    case return(x) => { file_close(h); x }", "    case return(x)")
rep(608, "    case cancel    => file_close(h)", "    case cancel")
rep(618, "let copy(src: String, dst: String): Unit @ {Console, Blocking} = {", "let copy(")
rep(657, "// トップレベルに残せるエフェクトは、ランタイムが提供するもの（Console、Blocking）だけである。", "// トップレベルに残せるエフェクトは")
rep(658, "// Heap がこの 2 つに含まれないのは、Heap[h] がリージョン変数を引数に取るラベルであり、トップレベルには渡せる h がないためである。", "// Heap がこの 4 つに")
# §11
rep(722, "// 並列処理は純粋な計算に限るので、結果は決定的である。標準ライブラリはまだ持たないので、署名だけを示す。", "// 並列処理は純粋な計算に限る")
rep(726, "// 並行処理は、ランタイムが提供するエフェクトで表す。利用者はこれを処理できない（スケジューラを利用者に書かせない）。標準ライブラリはまだ Async を持たないので、ここで宣言する。", "// 並行処理は、ランタイムが提供する")
# §12
rep(804, "// 操作を持たないラベル（Heap、Blocking）は、どれも宣言が要らない。", "// 操作を持たないラベル（Heap、Blocking、Fs）")
rep(805, "// 標準ライブラリにある名前なので、利用者が同じ名前を宣言することはできない。", "// 利用者が同じ宣言をもう一度書いてもよい")
rep(806, "// ファイル入出力のように標準ライブラリにない機能は、§9 の with_file のように C のバインディングと Blocking で表す。", "// 構造が異なる再宣言は拒否する")
# §13
rep(831, "  pub newtype BigInt                          // 表現はまだ与えない（コンストラクタを持たない型）", "  pub newtype BigInt = ???")
# §14
rep(863, "//       構造的な Eq も同様で、Float64 のフィールドを 1 つ持つだけで、レコードの Eq が反射律を失う。", "//       derive structural の Eq も同様で")
out = []
for i in range(1, len(src)+1):
    if i in edits: out.extend(edits[i])
    else: out.append(L[i])
open(sys.argv[1], 'w').write('\n'.join(out))
