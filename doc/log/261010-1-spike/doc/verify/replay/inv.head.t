保存する性質(LangSpec §18)。各ブロックは、変換の前と後のプログラムを 1 組ずつ
実行し、標準出力のバイト列と終了状態が一致するかを inv が報告する。診断と警告の
文面は名前と位置を含むので比べない(§18 の「評価の結果」の定義)。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"
  $ inv() { diktor "$1" > "$1.out" 2> /dev/null; a=$?; diktor "$2" > "$2.out" 2> /dev/null; b=$?; if [ $a = $b ] && cmp -s "$1.out" "$2.out"; then echo "same: exit $a"; else echo "differ: exit $a / $b"; fi; }

§18.1 束縛名の付け替え。関数、値、引数、パターンの変数、型パラメータ、行変数を
一斉に付け替える:

  $ cat > alpha1.kel <<'EOF'
  > let apply[A, B, E](f: (A) => B @ E, x: A): B @ E = f(x)
  > let twice[E](f: () => Unit @ E): Unit @ E = { f(); f() }
  > let rec len[A](xs: List[A]): Int32 = xs match { case Nil => 0 case Cons(_, t) => 1 + len(t) }
  > let total = {
  >   let n = apply(fn(v) => v + 1, 41)
  >   let m = len(Cons(n, Cons(n, Nil)))
  >   n + m
  > }
  > twice { echoln(show(total)) }
  > EOF
  $ cat > alpha2.kel <<'EOF'
  > let call[X, Y, Q](g: (X) => Y @ Q, y: X): Y @ Q = g(y)
  > let double[Rw](h: () => Unit @ Rw): Unit @ Rw = { h(); h() }
  > let rec count[T](ys: List[T]): Int32 = ys match { case Nil => 0 case Cons(_, rest) => 1 + count(rest) }
  > let sum_ = {
  >   let a = call(fn(w) => w + 1, 41)
  >   let b = count(Cons(a, Cons(a, Nil)))
  >   a + b
  > }
  > double { echoln(show(sum_)) }
  > EOF
  $ inv alpha1.kel alpha2.kel
  same: exit 0
  $ cat alpha1.kel.out
  44
  44

§18.2 依存の無い宣言の並べ替え。型、関数、値の宣言を、参照の向きを保たずに入れ替える
(式文は動かさない):

  $ cat > reorder1.kel <<'EOF'
  > newtype Shape = Circle(Float64) | Square(Float64)
  > let area(s: Shape): Float64 = s match { case Circle(r) => r * r * 3.0 case Square(a) => a * a }
  > let unit_sq = Square(1.0)
  > let label = "area"
  > echoln(label + " " + show(area(unit_sq)))
  > EOF
  $ cat > reorder2.kel <<'EOF'
  > let label = "area"
  > newtype Shape = Circle(Float64) | Square(Float64)
  > let unit_sq = Square(1.0)
  > let area(s: Shape): Float64 = s match { case Circle(r) => r * r * 3.0 case Square(a) => a * a }
  > echoln(label + " " + show(area(unit_sq)))
  > EOF
  $ inv reorder1.kel reorder2.kel
  same: exit 0

型の宣言は、間に値の宣言を挟んでも後ろの型を参照できる(§6.4):

  $ cat > reordertype1.kel <<'EOF'
  > type A = B
  > newtype B = B(Int32)
  > let x = 1
  > let y: A = B(x)
  > echoln("ok")
  > EOF
  $ cat > reordertype2.kel <<'EOF'
  > type A = B
  > let x = 1
  > newtype B = B(Int32)
  > let y: A = B(x)
  > echoln("ok")
  > EOF
  $ inv reordertype1.kel reordertype2.kel
  same: exit 0

再帰群の中の順序。注釈の無い f が、後ろの閉じた行の g を呼ぶ(案 U の署名の先置き):

  $ cat > reorderrec1.kel <<'EOF'
  > let rec f() = { println("a"); g() }
  > and g(): Unit @ {} = ()
  > with_stdout(f)
  > EOF
  $ cat > reorderrec2.kel <<'EOF'
  > let rec g(): Unit @ {} = ()
  > and f() = { println("a"); g() }
  > with_stdout(f)
  > EOF
  $ inv reorderrec1.kel reorderrec2.kel
  differ: exit 1 / 0

型パラメータを持つ束縛の群(判断 9-2 で署名の先置きを広げた後):

  $ cat > reorderrectp1.kel <<'EOF'
  > let rec f[A](x: A): Unit @ Print = { println("f"); g(x) }
  > and g[A](x: A): Unit @ {} = ()
  > with_stdout(fn() => f(1))
  > EOF
  $ cat > reorderrectp2.kel <<'EOF'
  > let rec g[A](x: A): Unit @ {} = ()
  > and f[A](x: A): Unit @ Print = { println("f"); g(x) }
  > with_stdout(fn() => f(1))
  > EOF
  $ inv reorderrectp1.kel reorderrectp2.kel
  differ: exit 1 / 0

除外 1:一般化しない束縛 r を共有する 2 つの宣言は、互いを参照しなくても入れ替えられない。
整数リテラルの型は、トップレベルの宣言の終わりで Int32 に決まる(§4.2)ので、先に
r(1) を書くと r の型が Int32 に決まる:

  $ cat > weakorder1.kel <<'EOF'
  > let idf[A](x: A): A = x
  > let r = idf(fn(y) => y)
  > let a = r(1)
  > let b: Int64 = r(2)
  > echoln(show(a) + show(b))
  > EOF
  $ cat > weakorder2.kel <<'EOF'
  > let idf[A](x: A): A = x
  > let r = idf(fn(y) => y)
  > let b: Int64 = r(2)
  > let a = r(1)
  > echoln(show(a) + show(b))
  > EOF
  $ inv weakorder1.kel weakorder2.kel
  differ: exit 1 / 0

§18.3 異なるラベルの順序。レコード型、レコード式、レコードのパターン、構造的ヴァリアント型:

  $ cat > label1.kel <<'EOF'
  > let p: {x: Int32, y: String} = {x = 1, y = "s"}
  > let f[R](r: {x: Int32, y: String extends R}): String = r.y + show(r.x)
  > let v: #A(Int32) | #B = #A(1)
  > let {x, y} = {x = 1, y = 2}
  > echoln(f(p) + show(x + y) + (v match { case #A(n) => show(n) case #B => "b" }))
  > EOF
  $ cat > label2.kel <<'EOF'
  > let p: {y: String, x: Int32} = {y = "s", x = 1}
  > let f[R](r: {y: String, x: Int32 extends R}): String = r.y + show(r.x)
  > let v: #B | #A(Int32) = #A(1)
  > let {y, x} = {x = 1, y = 2}
  > echoln(f(p) + show(x + y) + (v match { case #A(n) => show(n) case #B => "b" }))
  > EOF
  $ inv label1.kel label2.kel
  same: exit 0
  $ cat label1.kel.out
  s131

コンストラクタの名前付き引数の順序(引数の式がエフェクトを起こさない場合):

  $ cat > labelnamed1.kel <<'EOF'
  > newtype P = P(a: Int32, b: String)
  > let p = P(a = 1, b = "x")
  > echoln(p match { case P(a, b) => b + show(a) })
  > EOF
  $ cat > labelnamed2.kel <<'EOF'
  > newtype P = P(a: Int32, b: String)
  > let p = P(b = "x", a = 1)
  > echoln(p match { case P(a, b) => b + show(a) })
  > EOF
  $ inv labelnamed1.kel labelnamed2.kel
  same: exit 0

エフェクト行のラベルの順序。同じ操作名を持つ 2 つのエフェクトは、行の順序では選ばず、
修飾を求める(提案 6-1)。どちらの順序でも型検査に通らない:

  $ cat > labelrow1.kel <<'EOF'
  > effect A = { ask: () => Int32 }
  > effect B = { ask: () => Int32 }
  > let q(): Int32 @ {A, B} = perform ask()
  > let r = ((q() handle { case A.ask() => resume(1) }) handle { case B.ask() => resume(2) })
  > echoln(show(r))
  > EOF
  $ sed 's/@ {A, B}/@ {B, A}/' labelrow1.kel > labelrow2.kel
  $ inv labelrow1.kel labelrow2.kel
  differ: exit 0 / 0

除外 2:レコード式に書いたフィールドの順序は評価の順序を決める(§7.2)。
フィールドの式がエフェクトを起こすと、出力が変わる:

  $ cat > evalorder1.kel <<'EOF'
  > let n(s: String): Int32 @ Console = { echo(s); 1 }
  > let p = {x = n("x"), y = n("y")}
  > echoln("")
  > EOF
  $ cat > evalorder2.kel <<'EOF'
  > let n(s: String): Int32 @ Console = { echo(s); 1 }
  > let p = {y = n("y"), x = n("x")}
  > echoln("")
  > EOF
  $ inv evalorder1.kel evalorder2.kel
  differ: exit 0 / 0
  $ cat evalorder1.kel.out evalorder2.kel.out
  xy
  yx

§18.4 型エイリアスへの置換。注釈の矢印をエイリアスにしても、@ Console の文脈から
呼べる(案 U):

  $ cat > alias1.kel <<'EOF'
  > let k: (Int32) => Int32 = fn(x) => x
  > let use(): Int32 @ Console = k(1)
  > echoln(show(use()))
  > EOF
  $ cat > alias2.kel <<'EOF'
  > type F = (Int32) => Int32
  > let k: F = fn(x) => x
  > let use(): Int32 @ Console = k(1)
  > echoln(show(use()))
  > EOF
  $ inv alias1.kel alias2.kel
  differ: exit 0 / 1

エフェクト行の別名と、行を取る型パラメータを持つエイリアス:

  $ cat > aliasrow1.kel <<'EOF'
  > let f(): Unit @ {Print, Console} = { println("p"); echoln("c") }
  > newtype Cb[E] = Cb(() => Unit @ E)
  > let run_cb[E](c: Cb[E]): Unit @ E = c match { case Cb(g) => g() }
  > with_stdout(f)
  > run_cb(Cb(fn() => echoln("cb")))
  > EOF
  $ cat > aliasrow2.kel <<'EOF'
  > type Out: EffectRow = {Print, Console}
  > type Fn[E] = () => Unit @ E
  > let f(): Unit @ Out = { println("p"); echoln("c") }
  > newtype Cb[E] = Cb(Fn[E])
  > let run_cb[E](c: Cb[E]): Unit @ E = c match { case Cb(g) => g() }
  > with_stdout(f)
  > run_cb(Cb(fn() => echoln("cb")))
  > EOF
  $ inv aliasrow1.kel aliasrow2.kel
  same: exit 0

除外 3:pub の宣言の注釈に直接書いた入れ子の矢印には @ が要る(§15.2)が、
エイリアスの本体はこの要求を受けない:

  $ cat > aliaspub1.kel <<'EOF'
  > pub let h(f: (Int32) => Int32): Int32 = f(1)
  > EOF
  $ cat > aliaspub2.kel <<'EOF'
  > pub type F = (Int32) => Int32
  > pub let h(f: F): Int32 = f(1)
  > EOF
  $ inv aliaspub1.kel aliaspub2.kel
  differ: exit 1 / 0

除外 4:インスタンスの対象は型構成子に限る(§12.2)ので、エイリアスに置き換えられない:

  $ cat > aliasinst.kel <<'EOF'
  > newtype Seq[A] = Empty | More(A, Seq[A])
  > type S = Seq[Int32]
  > type class Size[A] { val size: (A) => Int32 }
  > type instance Size[S] { let size(x) = 0 }
  > EOF
  $ diktor --type-check aliasinst.kel
  ! aliasinst.kel:4:1: 型エラー: 未知の型構成子: S
  [1]

§18.5 対話的な実行で失敗した入力を挟む。型エラー、実行時エラー、構文エラーの入力を
挟んでも、受理した入力の表示は変わらない。差分は失敗した入力の行の追加(+)だけで、
実行時エラーまでに書いた出力(half)は残る。4 行目の失敗は g の弱い型変数を Boolean に決めたが、
戻るので 8 行目の g(N(1)) が通る。7 行目の失敗はインスタンスを宣言したが、戻るので
N(2) は構造に沿って表示する:

  $ cat > ok.in <<'EOF'
  > let idf[A](x: A): A = x
  > let g = idf(fn(y) => y)
  > newtype N = N(Int32)
  > g(N(1))
  > N(2)
  > EOF
  $ cat > failing.in <<'EOF'
  > let idf[A](x: A): A = x
  > let bad1: Int32 = "s"
  > let g = idf(fn(y) => y)
  > let bad2 = { let w = g(true); 1 / 0 }
  > newtype N = N(Int32)
  > let bad3 = { newtype M = M(Int32); 1 }
  > type instance Show[N] { let show(x) = "n" }; let bad4 = 1 / 0
  > g(N(1))
  > echoln("half"); let bad5 = 1 / 0
  > N(2)
  > EOF
  $ diktor --repl < ok.in > ok.out
  $ diktor --repl < failing.in > failing.out
  $ diff ok.out failing.out | grep '^[<>]' | sed 's/^>/+/; s/^</-/'
  + ! <stdin>:2:5: 型エラー: 注釈された型を満たしません(型が一致しません: Int32 と String)
  + 実行時エラー: ゼロ除算です
  + <stdin>:7:1: 構文エラー: この宣言はブロック内では使えません(let / let rec / 式のみ)
  + 実行時エラー: ゼロ除算です
  + half
  + 実行時エラー: ゼロ除算です

