初期化の順序と、初期化済みのフラグのテストである(IRSpec 10 節)。
期待出力は、Diktor の構文木の評価器の結果と一致する。

グローバルを参照するときの初期化の検査を省いてよいのは、その参照を含むトップレベルの宣言か式文の初期化手順が、参照先のグローバルの初期化手順より後にあるときと、両者が同じ def_fns の初期化手順にあるときだけである(IRSpec 10.3 節)。
次の p の無名関数は、h の初期化手順より前に作られ、h の初期化の前に呼び出せるので、検査を残さなければならない:

  $ cat > rev.kel <<'EOF'
  > let p = {f = fn(x: Int32) => h(x)}
  > echoln("before")
  > echoln(show(p.f(1)))
  > let h(x: Int32): Int32 @ {} = x + 1
  > EOF
  $ keleut run rev.kel
  before
  実行時エラー: 未束縛の変数: h
  [3]

同じ無名関数を h の初期化の後に呼べば、検査に通る:

  $ cat > rev2.kel <<'EOF'
  > let p = {f = fn(x: Int32) => h(x)}
  > let h(x: Int32): Int32 @ {} = x + 1
  > echoln(show(p.f(1)))
  > EOF
  $ keleut run rev2.kel
  2

後で宣言する関数を呼ぶ関数は、それを呼び出した時点で後の関数がまだ初期化されていなければ、実行時エラーになる:

  $ cat > fwd.kel <<'EOF'
  > let f(): Int32 @ {} = g()
  > echoln("a")
  > let x = f()
  > let g(): Int32 @ {} = 1
  > EOF
  $ keleut run fwd.kel
  a
  実行時エラー: 未束縛の変数: g
  [3]

module の中のグローバルは、単位のキーを除いた修飾名で報告する:

  $ cat > fwdm.kel <<'EOF'
  > module M {
  >   pub let f(): Int32 @ {} = h()
  >   pub let y: Int32 = f()
  >   pub let h(): Int32 @ {} = 2
  > }
  > EOF
  $ keleut run fwdm.kel
  実行時エラー: 未束縛の変数: M.h
  [3]

トップレベルでのタプルとレコードの分解は def_pattern の初期化手順になり、束縛するグローバルを順に初期化する:

  $ cat > tup.kel <<'EOF'
  > let (a, b) = (1, "x")
  > let {u, v = w} = {u = 2, v = true}
  > echoln(show(a) + b + show(u) + show(w))
  > EOF
  $ keleut run tup.kel
  1x2true

照合に失敗する def_pattern は、それより前の文の出力の後で実行時エラーになる:

  $ cat > somefail.kel <<'EOF'
  > echoln("before")
  > let Some(c) = None
  > echoln("after")
  > EOF
  $ keleut run somefail.kel
  ⚠ match が非網羅的です。たとえば None が漏れています
  before
  実行時エラー: パターンに値が一致しません: None
  [3]

インスタンスは、def_instance の初期化手順を実行した後で使えるようになる。
それより前にメソッドを選ぶと、実行時エラーになる:

  $ cat > inst.kel <<'EOF'
  > type class Render[A] {
  >   val render: (A) => String
  > }
  > newtype L(String)
  > let use(x: L): String = render(x)
  > echoln("before")
  > echoln(use(L("q")))
  > type instance Render[L] {
  >   let render(x) = x match { case L(s) => s }
  > }
  > EOF
  $ keleut run inst.kel
  before
  実行時エラー: Render[L] のインスタンスは宣言より前の位置では使えません(実行はまだ実体を持ちません。インスタンス宣言を使用より前に置いてください)
  [3]

前提もメソッド自身の制約も持たない、値を束縛するメソッドは、def_instance が宣言の順に 1 回だけ評価する:

  $ cat > inst2.kel <<'EOF'
  > type class Sz[A] {
  >   val sz: (A) => Int32
  > }
  > newtype B(Int32)
  > type instance Sz[B] {
  >   let sz = { echoln("init sz"); fn(b) => 3 }
  > }
  > echoln("after")
  > echoln(show(sz(B(1))))
  > echoln(show(sz(B(1))))
  > EOF
  $ keleut run inst2.kel
  init sz
  after
  3
  3

範囲外の数値リテラルは、評価したときに初めて実行時エラーになる:

  $ cat > lit.kel <<'EOF'
  > let f() = 2147483648
  > echoln("before")
  > let x = f()
  > EOF
  $ keleut run lit.kel
  before
  実行時エラー: 数値リテラルが範囲外です: 2147483648
  [3]

??? も評価したときに初めて実行時エラーになる:

  $ cat > hole.kel <<'EOF'
  > let f(): Int32 = ???
  > echoln("ok")
  > let x = f()
  > EOF
  $ keleut run hole.kel
  ok
  実行時エラー: ??? に到達しました
  [3]
