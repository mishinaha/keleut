プリミティブと extern のテストである(IRSpec 付録 A.5 と付録 B)。
期待出力は、Diktor の構文木の評価器の結果と同じである。

付録 A.5 にない実装名の extern "prim" は、宣言しただけではエラーにならない:

  $ cat > pdecl.kel <<'EOF'
  > extern "prim" let __no_such(x: Int32): Int32
  > echoln("before")
  > EOF
  $ keleut run pdecl.kel
  before

呼び出すと、それまでの出力の後で実行時エラーになる:

  $ cat > pcall.kel <<'EOF'
  > extern "prim" let __no_such(x: Int32): Int32
  > echoln("before")
  > echoln(show(__no_such(1)))
  > EOF
  $ keleut run pcall.kel
  before
  実行時エラー: 未実装のプリミティブ: __no_such
  [3]

extern "C" も同じである:

  $ cat > cdecl.kel <<'EOF'
  > extern "C" let foo(x: Float64): Float64
  > extern "C" let sqrt(x: Float64): Float64
  > echoln(show(sqrt(2.0)))
  > EOF
  $ keleut run cdecl.kel
  1.4142135623730951

  $ cat > ccall.kel <<'EOF'
  > extern "C" let foo(x: Float64): Float64
  > echoln("before")
  > let y = foo(1.0)
  > EOF
  $ keleut run ccall.kel
  before
  実行時エラー: 未実装のプリミティブ: foo
  [3]

表示名が実装名と違うときは、診断に実装名も書く:

  $ cat > mun.kel <<'EOF'
  > module M { pub extern "prim" let __no_such(x: Int32): Int32 }
  > echoln("before")
  > echoln(show(M.__no_such(1)))
  > EOF
  $ keleut run mun.kel
  before
  実行時エラー: 未実装のプリミティブ: M.__no_such(実装名 __no_such が見つかりません)
  [3]

実装のない extern を値として渡しても、呼び出すまではエラーにならない:

  $ cat > pval.kel <<'EOF'
  > extern "prim" let __no_such(x: Int32): Int32
  > let ap(f: (Int32) => Int32, x: Int32): Int32 = f(x)
  > let g = __no_such
  > echoln("before")
  > echoln(show(ap(g, 1)))
  > EOF
  $ keleut run pval.kel
  before
  実行時エラー: 未実装のプリミティブ: __no_such
  [3]

整数の演算は、オーバーフローすると折り返し(2 の補数で下位のビットだけが残る)、除算は 0 に向けて切り捨てる。
Float64 の表記は LangSpec §16.2 に従い、NaN は符号によらず nan と書く:

  $ cat > num.kel <<'EOF'
  > echoln(show(2147483647 + 1))
  > echoln(show(-2147483648 / -1))
  > echoln(show(-2147483648 % -1))
  > echoln(show(-7 / 2))
  > echoln(show(-7 % 2))
  > echoln(show(7 % -2))
  > echoln(show(-(-2147483648)))
  > echoln(show(0xffffffffi32))
  > echoln(show(9223372036854775807i64 + 1i64))
  > echoln(show(-9223372036854775808i64 / -1i64))
  > echoln(show(0.1 + 0.2))
  > echoln(show(100000.0))
  > echoln(show(0.0001))
  > echoln(show(0.00001))
  > echoln(show(1e16))
  > echoln(show(1e15))
  > echoln(show(-0.0))
  > echoln(show(0.0 / 0.0))
  > echoln(show(-(0.0 / 0.0)))
  > echoln(show(1.0 / 0.0))
  > echoln(show(-1.0 / 0.0))
  > echoln(show(5e-324))
  > echoln(show(123456789012345680000.0))
  > echoln(show(0.0 == -0.0))
  > let nan = 0.0 / 0.0
  > echoln(show(nan == nan))
  > echoln(show(nan != nan))
  > echoln(show(nan < 1.0))
  > echoln(show(nan >= 1.0))
  > echoln(show((nan, 1) == (nan, 1)))
  > echoln(show({a = 0.0} == {a = -0.0}))
  > echoln(show(__f64_to_i32(-2.9)))
  > echoln(show(__f64_to_i32(2147483647.9)))
  > echoln(show(__f64_to_i64(-9223372036854775808.0)))
  > echoln(show(__i64_to_i32(4294967297i64)))
  > echoln(show(__i64_to_f64(9007199254740993i64)))
  > echoln(show(__i32_to_i64(-1)))
  > echoln(show(__i32_to_f64(-3)))
  > echoln(show(__string_length("あ")))
  > echoln(show("b" < "ab"))
  > echoln(show("é" > "z"))
  > echoln(show("abc" <= "abc"))
  > echoln(show(__string_length(__string_sub("あい", 1, 2))))
  > echoln(show(__string_sub("abc", 3, 0) == ""))
  > echoln(__show_int32(-5) + __string_concat("x", "y"))
  > echoln(show(true) + show(false == false))
  > EOF
  $ keleut run num.kel
  -2147483648
  -2147483648
  0
  -3
  -1
  1
  -2147483648
  -1
  -9223372036854775808
  -9223372036854775808
  0.30000000000000004
  100000.0
  0.0001
  1e-5
  1e16
  1000000000000000.0
  -0.0
  nan
  nan
  inf
  -inf
  5e-324
  1.2345678901234568e20
  true
  false
  true
  false
  false
  false
  true
  -2
  2147483647
  -9223372036854775808
  1
  9007199254740992.0
  -1
  -3.0
  3
  false
  true
  true
  2
  true
  -5xy
  truetrue

extern "C" の関数は、rtcore が持つ musl の sin、cos、exp、log と、IEEE 754 の sqrt である。
sin、cos、exp、log の最下位のビットは処理系が定める(LangSpec §17)ので、ここでは Diktor の glibc と結果が一致する入力だけを使う:

  $ cat > math.kel <<'EOF'
  > extern "C" let sin(x: Float64): Float64
  > extern "C" let cos(x: Float64): Float64
  > extern "C" let exp(x: Float64): Float64
  > extern "C" let log(x: Float64): Float64
  > extern "C" let sqrt(x: Float64): Float64
  > echoln(show(sin(0.0)) + " " + show(cos(0.0)) + " " + show(exp(0.0)) + " " + show(log(1.0)))
  > echoln(show(sqrt(16.0)) + " " + show(sqrt(-1.0)) + " " + show(log(0.0)) + " " + show(exp(1000.0)))
  > EOF
  $ keleut run math.kel
  0.0 1.0 1.0 0.0
  4.0 nan -inf inf

ゼロ除算は、Int32 でも Int64 でも実行時エラーになる:

  $ cat > div0.kel <<'EOF'
  > echo("partial")
  > let x = 1 / 0
  > EOF
  $ keleut run div0.kel
  partial実行時エラー: ゼロ除算です
  [3]

  $ cat > rem0.kel <<'EOF'
  > echoln(show(5i64 % 0i64))
  > EOF
  $ keleut run rem0.kel
  実行時エラー: ゼロ除算です
  [3]

Float64 から整数への変換は、先に NaN と範囲を検査する(範囲の上端は開いている):

  $ cat > nanconv.kel <<'EOF'
  > let x = __f64_to_i32(0.0 / 0.0)
  > EOF
  $ keleut run nanconv.kel
  実行時エラー: __f64_to_i32: NaN は整数に変換できません
  [3]

  $ cat > rng.kel <<'EOF'
  > let x = __f64_to_i64(9223372036854775807.0)
  > EOF
  $ keleut run rng.kel
  実行時エラー: __f64_to_i64: 変換結果が範囲外です: 9.223372036854776e18
  [3]

  $ cat > rng32.kel <<'EOF'
  > echoln(show(__f64_to_i32(-2147483648.9)))
  > let x = __f64_to_i32(2147483648.0)
  > EOF
  $ keleut run rng32.kel
  -2147483648
  実行時エラー: __f64_to_i32: 変換結果が範囲外です: 2147483648.0
  [3]

__string_sub に範囲外の引数を渡すと、実行時エラーになる:

  $ cat > sub.kel <<'EOF'
  > echoln(__string_sub("abc", 1, 2))
  > let s = __string_sub("abc", 2, 2)
  > EOF
  $ keleut run sub.kel
  bc
  実行時エラー: __string_sub: 範囲外です
  [3]

  $ cat > subneg.kel <<'EOF'
  > let s = __string_sub("abc", -1, 1)
  > EOF
  $ keleut run subneg.kel
  実行時エラー: __string_sub: 範囲外です
  [3]

__panic は、引数のメッセージをそのまま書く:

  $ cat > panic.kel <<'EOF'
  > echoln("p")
  > let x: Int32 = __panic("boom \"q\"")
  > EOF
  $ keleut run panic.kel
  p
  実行時エラー: panic: boom "q"
  [3]

組み込みの値 Ref、Array、MutableArray:

  $ cat > arr.kel <<'EOF'
  > let x = run h { let a = MutableArray.new(3, 0)
  >  MutableArray.set(a, 0, 5)
  >  let f = MutableArray.freeze(a)
  >  MutableArray.set(a, 0, 9)
  >  Array.get(f, 0) + MutableArray.get(a, 0) + Array.length(f) + MutableArray.length(a) }
  > echoln(show(x))
  > let r = run h { let c = Ref.new(1)
  >  Ref.set(c, Ref.get(c) + 41)
  >  Ref.get(c) }
  > echoln(show(r))
  > let s = run h { let a = MutableArray.new(4, "x")
  >  let f = MutableArray.freeze(a)
  >  let acc = Ref.new("")
  >  Array.each(f, fn(e) => Ref.set(acc, Ref.get(acc) + e))
  >  Ref.get(acc) }
  > echoln(s)
  > let y = run h { let a = MutableArray.new(-1, 0)
  >  0 }
  > EOF
  $ keleut run arr.kel
  20
  42
  xxxx
  実行時エラー: MutableArray.new: 長さが負です
  [3]

  $ cat > idx.kel <<'EOF'
  > let y = run h { let a = MutableArray.new(2, 0)
  >  MutableArray.get(a, 2) }
  > EOF
  $ keleut run idx.kel
  実行時エラー: 配列の範囲外です
  [3]

  $ cat > idx2.kel <<'EOF'
  > let y = run h { let a = MutableArray.freeze(MutableArray.new(2, 0))
  >  Array.get(a, -1) }
  > EOF
  $ keleut run idx2.kel
  実行時エラー: 配列の範囲外です
  [3]

  $ cat > idx3.kel <<'EOF'
  > let y = run h { let a = MutableArray.new(2, 0)
  >  MutableArray.set(a, 5, 1)
  >  0 }
  > EOF
  $ keleut run idx3.kel
  実行時エラー: 配列の範囲外です
  [3]

照合に失敗した値は、Diktor が値を表示するときと同じ形式で書く:

  $ cat > matchfail.kel <<'EOF'
  > let f(x: Option[Int32]): Int32 = x match { case Some(n) => n }
  > echoln("w")
  > let y = f(None)
  > EOF
  $ keleut run matchfail.kel
  ⚠ match が非網羅的です。例えば None が漏れています
  w
  実行時エラー: match のどの節にも一致しません: None
  [3]

  $ cat > matchfail2.kel <<'EOF'
  > let f(x: (Int32, String, Option[Int64])): Int32 = x match { case (1, _, _) => 1 }
  > let y = f((2, "a\"\n\té", Some(3i64)))
  > EOF
  $ keleut run matchfail2.kel
  ⚠ match が非網羅的です。例えば (0, _, None) が漏れています
  実行時エラー: match のどの節にも一致しません: (2, "a\"\n\t\195\169", Some(3))
  [3]
