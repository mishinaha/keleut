インスタンスのメソッドを値束縛で書いたときの初期化式(LangSpec §12.2、§6.4)。
宣言の実行で 1 回だけ評価する初期化式は、トップレベルの評価と同じエフェクトを
起こせる。前提を持つインスタンスと制約を持つメソッドの初期化式はメソッドを使う
たびに評価するので、エフェクトを起こせない。署名の行と照らすのは初期化式の値の
関数の行で、初期化式が起こすエフェクトは含まない。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

初期化式は Console を起こせる。前提を持たないインスタンスでは宣言の実行で 1 回だけ
評価する:

  $ cat > instinitcon.kel <<'KEL'
  > newtype Foo = Foo
  > type instance Show[Foo] {
  >   let show: (Foo) => String = { echoln("init"); fn(x) => "foo" }
  > }
  > echoln(show(Foo))
  > echoln(show(Foo))
  > KEL
  $ diktor instinitcon.kel
  init
  foo
  foo

エフェクトを起こさない値でない初期化式は受理する。返す無名関数の決まっていない行は
空の行に決まる。組み込みのクラスでも、利用者のクラスの @ E のメソッドでも同じ:

  $ cat > instweakrow.kel <<'KEL'
  > newtype Foo = Foo
  > let idf[A](x: A): A = x
  > type instance Show[Foo] {
  >   let show: (Foo) => String = idf(fn(x) => "foo")
  > }
  > echoln(show(Foo))
  > KEL
  $ diktor instweakrow.kel
  foo
  $ cat > instweakrowuser.kel <<'KEL'
  > type class Sz[A] { val sz[E]: (A) => Int32 @ E }
  > newtype Foo = Foo
  > let idf[A](x: A): A = x
  > type instance Sz[Foo] {
  >   let sz = idf(fn(x: Foo) => 1i32)
  > }
  > echoln(show(sz(Foo)))
  > KEL
  $ diktor instweakrowuser.kel
  1

初期化式の中で処理した操作は、初期化式の行に残らない:

  $ cat > instinithandled.kel <<'KEL'
  > effect Ask = { ask: () => Int32 }
  > newtype Foo = Foo
  > type instance Show[Foo] {
  >   let show = { let n = perform ask(); fn(x: Foo) => "foo" } handle { case ask() => resume(1i32) }
  > }
  > echoln(show(Foo))
  > KEL
  $ diktor instinithandled.kel
  foo

効果多相の関数を初期化式で呼ぶ形は、その関数の行変数が返す関数の行と共有されても
受理する。初期化式の中の局所の束縛で呼んでも同じ:

  $ cat > instinitpoly.kel <<'KEL'
  > type class Sz[A] { val sz: (A) => Int32 }
  > newtype Foo = Foo
  > let memo[E](f: (Foo) => Int32 @ E): (Foo) => Int32 @ E = { let v = f(Foo); fn(x) => v }
  > type instance Sz[Foo] {
  >   let sz = memo(fn(x) => 7i32)
  > }
  > echoln(show(sz(Foo)))
  > KEL
  $ diktor instinitpoly.kel
  7
  $ cat > instinitpolylocal.kel <<'KEL'
  > type class Sz[A] { val sz: (A) => Int32 }
  > newtype Foo = Foo
  > newtype Bar = Bar
  > let memo[E](f: (Foo) => Int32 @ E): (Foo) => Int32 @ E = { let v = f(Foo); fn(x) => v }
  > type instance Sz[Bar] {
  >   let sz = { let m = memo(fn(x) => 3i32); fn(x: Bar) => m(Foo) }
  > }
  > echoln(show(sz(Bar)))
  > KEL
  $ diktor instinitpolylocal.kel
  3

トップレベルで起こせないエフェクトを初期化式で起こすと、エフェクトの名前とメソッドの
名前を挙げて拒否する。位置はメソッドの束縛である:

  $ cat > instinitask.kel <<'KEL'
  > effect Ask = { ask: () => Int32 }
  > type class Sz[A] { val sz: (A) => Int32 }
  > newtype Foo = Foo
  > type instance Sz[Foo] {
  >   let sz: (Foo) => Int32 = { let n = perform ask(); fn(x) => n }
  > }
  > echoln(show(sz(Foo)))
  > KEL
  $ diktor instinitask.kel
  ! instinitask.kel:5:7: 型エラー: エフェクト Ask をインスタンスメソッド sz の初期化式では実行できません(初期化式が起こせるのはトップレベルの評価と同じエフェクトです)
  [1]
  $ cat > instinitheap.kel <<'KEL'
  > type class Sz[A] { val sz: (A) => Int32 }
  > newtype Foo = Foo
  > type instance Sz[Foo] {
  >   let sz: (Foo) => Int32 = { let r = Ref.new(0i32); fn(x) => 1i32 }
  > }
  > echoln(show(sz(Foo)))
  > KEL
  $ diktor instinitheap.kel
  ! instinitheap.kel:4:7: 型エラー: エフェクト Heap をインスタンスメソッド sz の初期化式では実行できません(初期化式が起こせるのはトップレベルの評価と同じエフェクトです)
  [1]

前提を持つインスタンスの初期化式はメソッドを使うたびに評価するので、Console も
起こせない:

  $ cat > instinitperuse.kel <<'KEL'
  > type class Sz[A] { val sz: (A) => Int32 }
  > newtype Box[A] = Box(A)
  > type instance Sz[Int32] { let sz(x: Int32) = x }
  > type instance[A: Sz] Sz[Box[_]] {
  >   let sz = { echoln("init"); fn(b: Box[A]) => b match { case Box(x) => sz(x) } }
  > }
  > let pure(n: Int32): Int32 @ {} = sz(Box(n))
  > echoln("start")
  > let k = pure(5i32)
  > echoln(show(k))
  > KEL
  $ diktor instinitperuse.kel
  ! instinitperuse.kel:5:7: 型エラー: エフェクト Console をインスタンスメソッド sz の初期化式では実行できません(メソッドを使うたびに評価する初期化式は、エフェクトを起こせません)
  [1]

ゼロ除算は行に現れないので、初期化式に書いても型検査を通る。前提を持たない
インスタンスでは、インスタンス宣言を実行する時点で実行時エラーになる(LangSpec §6.4):

  $ cat > inidiv.kel <<'KEL'
  > newtype N = N(Int32)
  > let z = 0
  > echoln("a")
  > type instance Show[N] { let show: (N) => String = { let q = 1 / z; fn(x) => "n" } }
  > echoln("b")
  > KEL
  $ diktor inidiv.kel
  a
  実行時エラー: ゼロ除算です
  [3]

前提を持つインスタンスでは初期化式をメソッドを使うたびに評価するので、実行時エラーは
メソッドを使う時点で起きる(LangSpec §6.4):

  $ cat > inipremdiv.kel <<'KEL'
  > newtype Box[A] = Box(A)
  > let z = 0
  > echoln("a")
  > type instance [A: Show] Show[Box[_]] {
  >   let show: (Box[A]) => String = { let q = 1 / z; fn(b) => "box" }
  > }
  > echoln("b")
  > echoln(show(Box(1)))
  > KEL
  $ diktor inipremdiv.kel
  a
  b
  実行時エラー: ゼロ除算です
  [3]

エフェクトを起こさない初期化式は、構文上の値でなくても受理する。組み込みのクラスでも
利用者のクラスと同じである:

  $ cat > inipure.kel <<'KEL'
  > newtype N = N(Int32)
  > let pre = "n"
  > type instance Show[N] { let show: (N) => String = { let k = 1 + 2; fn(v) => pre + Show.show(k) } }
  > echoln(show(N(0)))
  > KEL
  $ diktor inipure.kel
  n3

対話的な実行でも同じ規則で、トップレベルの行が Print を含むので、Print を起こす
初期化式も受理する:

  $ diktor --repl <<'EOF2'
  > type class Sz[A] { val sz: (A) => Int32 }
  > newtype Foo = Foo
  > type instance Sz[Foo] { let sz = { perform print("init"); fn(x: Foo) => 1i32 } }
  > echoln(show(sz(Foo)))
  > let memo[E](f: (Foo) => Int32 @ E): (Foo) => Int32 @ E = { let v = f(Foo); fn(x) => v }
  > newtype Bar = Bar
  > type instance Sz[Bar] { let sz = { let m = memo(fn(x) => 3i32); fn(x: Bar) => m(Foo) } }
  > echoln(show(sz(Bar)))
  > EOF2
  init1
  memo : ((Foo) => Int32) => (Foo) => Int32 = <fn>
  3
