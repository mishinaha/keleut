インスタンスのメソッドの仮引数のうち、型注釈のない変数と _ の型は、クラス宣言の署名の
対応する引数の型に決まる(LangSpec §12.2)。仮引数に署名の型を注釈した関数と同じく、
本体の数値リテラルの型と網羅性の検査はその型に基づく。返り値の型と最外の行は署名から
決めない。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

署名の引数が Never なら、match {} は網羅的である。関数束縛、値束縛の無名関数、let rec
のどれでも同じ:

  $ cat > insnever.kel <<'KEL'
  > type class D1[A] { val d1: (A) => String }
  > type class D2[A] { val d2: (A) => String }
  > type class D3[A] { val d3: (A) => String }
  > type instance D1[Never] { let d1(n) = n match {} }
  > type instance D2[Never] { let d2 = fn(n) => n match {} }
  > type instance D3[Never] { let rec d3(n) = n match {} }
  > KEL
  $ diktor --strict-exhaustive --type-check insnever.kel

同じ理由で、署名の引数が Never か閉じた構造的ヴァリアントのとき、case _ の節には
一致する値が無いという警告や冗長の警告が出る:

  $ cat > inswild.kel <<'KEL'
  > type class Describe[A] { val describe: (A) => String }
  > type instance Describe[Never] { let describe(n) = n match { case _ => "never" } }
  > type instance Describe[Int32] { let describe(n) = show(n) }
  > type class Lv[A] { val lv: (A, #Lo | #Hi) => String }
  > type instance Lv[Int32] { let lv(n, l) = l match { case #Lo => "lo" case #Hi => "hi" case _ => "other" } }
  > echo(describe(3i32))
  > echo(lv(1, #Hi))
  > KEL
  $ diktor inswild.kel
  ⚠ このパターンには一致する値がありません
  ⚠ 第 3 節は到達不能です(冗長)
  3hi
  $ diktor --strict-exhaustive --type-check inswild.kel
  ⚠ このパターンには一致する値がありません
  ⚠ 第 3 節は到達不能です(冗長)
  _ : {}
  _ : {}
  [1]

本体の数値リテラルは、署名の引数の型に決まる:

  $ cat > inslit.kel <<'KEL'
  > type class Pick[A] { val pick: (A, A) => A }
  > type instance Pick[Int64] { let pick(a, b) = if a > 10 then a else b }
  > echo(show(pick(3i64, 20i64)))
  > KEL
  $ diktor inslit.kel
  20

閉じた構造的ヴァリアントの引数の漏れは、型エラーではなく網羅漏れの警告になる:

  $ cat > insvariant.kel <<'KEL'
  > type class Lv[A] { val lv: (A, #Lo | #Hi) => String }
  > type instance Lv[Int32] { let lv(n, l) = l match { case #Lo => "lo" } }
  > echo(lv(1, #Lo))
  > KEL
  $ diktor insvariant.kel
  ⚠ match が非網羅的です。例えば #Hi(_) が漏れています
  lo
  $ diktor --strict-exhaustive insvariant.kel
  ⚠ match が非網羅的です。例えば #Hi(_) が漏れています
  [1]

渡した型と本体が食い違うと、注釈を書いた関数と同じく、本体の食い違った位置に出る:

  $ cat > insbody.kel <<'KEL'
  > type class Describe[A] { val describe: (A) => String }
  > type instance Describe[Int32] { let describe(n) = __string_concat(n, "x") }
  > KEL
  $ diktor --type-check insbody.kel
  ! insbody.kel:2:67: 型エラー: 型が一致しません: Int32 と String
  [1]

返り値の型は署名から決めないので、リテラルは既定の Int32 になり、署名と照らした
時点で落ちる:

  $ cat > insret.kel <<'KEL'
  > type class Sz[A] { val size: (A) => Int64 }
  > type instance Sz[String] { let size(s) = 0 }
  > echo(show(size("a")))
  > KEL
  $ diktor insret.kel
  ! insret.kel:2:32: 型エラー: インスタンスメソッド size がクラス宣言の型を満たしません(型が一致しません: Int32 と Int64)
  [1]

注釈を書いた仮引数には署名の型を渡さない:

  $ cat > insannot.kel <<'KEL'
  > type class Describe[A] { val describe: (A) => String }
  > type instance Describe[String] { let describe(n: Int32) = "x" }
  > KEL
  $ diktor --type-check insannot.kel
  ! insannot.kel:2:38: 型エラー: インスタンスメソッド describe がクラス宣言の型を満たしません(型が一致しません: Int32 と String)
  [1]

前提を持つインスタンスでも、前提のクラスを本体で使う形は変わらない:

  $ cat > inspremise.kel <<'KEL'
  > newtype Box[A] = Box(A)
  > type instance[A: Show] Show[Box[_]] { let show(b) = b match { case Box(x) => "Box(" + show(x) + ")" } }
  > type instance[A: Eq] Eq[Box[_]] { let eq(a, b) = (a, b) match { case (Box(x), Box(y)) => x == y } }
  > echo(show(Box(3i32)))
  > echo(show(Box(1i32) == Box(1i32)))
  > echo(show(Box("a") == Box("b")))
  > KEL
  $ diktor inspremise.kel
  Box(3)truefalse
  $ diktor --strict-exhaustive --type-check inspremise.kel
  _ : {}
  _ : {}
  _ : {}
