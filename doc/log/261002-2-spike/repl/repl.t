前の入力の束縛を次の入力で使え、同じ名前の再束縛は、前の束縛を捕捉した関数に影響しない:

  $ diktor --repl <<'EOF2'
  > let a = 1
  > let geta(): Int32 = a
  > let a = "s"
  > a
  > geta()
  > EOF2
  a : Int32 = 1
  geta : () => Int32 = <fn>
  a : String = "s"
  _ : String = "s"
  _ : Int32 = 1

型エラーの入力は、単一化の結果も含めて全部戻る。
f の弱い型変数 _A は、失敗した入力の中で Int32 に決まったが、次の入力では String に決められる:

  $ diktor --repl <<'EOF2'
  > let id[A](x: A): A = x
  > let f = id(fn(y) => y)
  > let g = f(1); let bad = 1 + "a"
  > f("a")
  > g
  > EOF2
  id : (A) => A = <fn>
  f : (_A) => _A = <fn>
  ! <stdin>:3:25: 型エラー: String は Integral のインスタンスではありません
  _ : String = "a"
  ! <stdin>:5:1: 型エラー: 未束縛の変数: g

型エラーの入力の宣言は表に残らない。打ち直した newtype が通る:

  $ diktor --repl <<'EOF2'
  > newtype T = A | Nil
  > newtype T = A | B
  > B
  > newtype T = C
  > EOF2
  ! <stdin>:1:1: 型エラー: コンストラクタ Nil が二重に宣言されています(コンストラクタ名は大域一意)
  _ : T = B
  ! <stdin>:4:1: 型エラー: newtype T が二重に宣言されています

実行時エラーの入力は、入力の単位で戻る。同じ入力の中で先に済んだ束縛も残らない:

  $ diktor --repl <<'EOF2'
  > let a = 2
  > let a = 3; let z = 5; echoln("before"); 1 / 0
  > a
  > z
  > EOF2
  a : Int32 = 2
  before
  実行時エラー: ゼロ除算です
  _ : Int32 = 2
  ! <stdin>:4:1: 型エラー: 未束縛の変数: z

括弧や文字列の途中なら続きの行を待つ。パースが通った時点で入力が終わるので、
次の行の and は続きにならない:

  $ diktor --repl <<'EOF2'
  > let h(n: Int32): Int32 = {
  >   n * 2
  > }
  > h(21)
  > "a
  > b"
  > let rec ev(n: Int32): Boolean = n == 0 || od(n - 1)
  > and od(n: Int32): Boolean = n != 0 && ev(n - 1)
  > EOF2
  h : (Int32) => Int32 = <fn>
  _ : Int32 = 42
  _ : String = "a\nb"
  ! <stdin>:7:43: 型エラー: 未束縛の変数: od
  <stdin>:8:1: パースエラー(付近のトークンを確認してください)

println はトップレベルで使える。Print はランタイムのハンドラが標準出力へつなぐ:

  $ diktor --repl <<'EOF2'
  > println("hello")
  > with_stdout(fn() => println("inner"))
  > EOF2
  hello
  inner

Ref は入力をまたいで持てない(§13.7):

  $ diktor --repl <<'EOF2'
  > let r = run h { Ref.new(0) }
  > EOF2
  ! <stdin>:1:9: 型エラー: スコープ付きの型 ς1 がスコープの外に漏れています

:type と :reset:

  $ diktor --repl <<'EOF2'
  > :type fn(a) => a
  > let x = 1
  > :reset
  > x
  > EOF2
  (_A) => _A
  x : Int32 = 1
  ! <stdin>:4:1: 型エラー: 未束縛の変数: x

前の入力の module の中の値と同じ名前のトップレベルの値は、入力の順によらず拒否する。
ファイルでは同じ組み合わせを平坦化が拒否する:

  $ diktor --repl <<'EOF2'
  > module M { let foo: Int32 = 2; pub let g(): Int32 = foo }
  > let foo = "s"
  > M.g()
  > let bar = 1
  > module N { let bar: Int32 = 2 }
  > EOF2
  M.foo : Int32 = 2
  M.g : () => Int32 = <fn>
  ! 型エラー: トップレベルの foo は、前の入力の module M の foo と同名です(module 内の名前とトップレベル名は同名にできません)
  _ : Int32 = 2
  bar : Int32 = 1
  ! <stdin>:5:12: 型エラー: module N の bar はトップレベルの bar と同名です(module 内の名前とトップレベル名は同名にできません)

後の入力で宣言したクラスのメソッドは、同じ名前の既存の束縛を覆わない(型検査の先勝ちと一致させる):

  $ diktor --repl <<'EOF2'
  > let sh2 = 1
  > type class S2[X] { val sh2: (X) => Int32 }
  > sh2
  > type instance S2[String] { let sh2(s: String): Int32 = 7 }
  > S2.sh2("a")
  > EOF2
  sh2 : Int32 = 1
  _ : Int32 = 1
  _ : Int32 = 7
