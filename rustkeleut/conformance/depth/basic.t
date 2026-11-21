ハンドラ式を含まないプログラムで論理的な深さを確かめるテストである(IRSpec 11.2 節、11.3 節)。
diktor/test/ir_depth.t の前半のブロックを、keleut run --max-depth で実行する形に書き直した。
ペア dk-ir では、shim が --max-depth を diktor --run-ir に渡す。

非末尾の再帰は、上限 100 のとき f(98) まで実行でき、f(99) で上限を超える。
末尾呼び出しによる 10 万回のループは、深さを増やさない。

  $ cat > nontail.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let rec loop(n: Int32): Int32 = if n == 0 then 0 else loop(n - 1)
  > echoln(show(loop(100000)))
  > echoln(show(f(98)))
  > echoln(show(f(99)))
  > echoln("not reached")
  > EOF
  $ keleut run --max-depth 100 nontail.kel
  0
  98
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

Array.each と pinned は、非末尾呼び出しで呼ぶと呼び出し元より 1 段深い位置で実行され、ユーザの関数をさらに 1 段深い位置で呼ぶ(IRSpec 11.2 節、付録 A.4)。

  $ cat > each.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let arr = run h { let a = MutableArray.new(1, 96); MutableArray.freeze(a) }
  > Array.each(arr, fn(x) => echoln(show(f(x))))
  > Array.each(arr, fn(x) => echoln(show(f(x + 1))))
  > EOF
  $ keleut run --max-depth 100 each.kel
  96
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > pinned.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > echoln(show(pinned(fn() => f(97))))
  > echoln(show(pinned(fn() => f(98))))
  > EOF
  $ keleut run --max-depth 100 pinned.kel
  97
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

相互の末尾呼び出し、メソッドを通した末尾呼び出し、辞書を渡す末尾呼び出しは、上限 10 のときも 10 万回続けられる。

  $ cat > tails.kel <<'EOF'
  > let rec even(n: Int32): Boolean = if n == 0 then true else odd(n - 1)
  > and odd(n: Int32): Boolean = if n == 0 then false else even(n - 1)
  > type class Step[A] { val step: (A, Int32) => Int32 }
  > type instance Step[Int32] { let rec step(x, n) = if n == 0 then x else step(x, n - 1) }
  > let rec count[A: Eq](x: A, n: Int32): Int32 = if n == 0 then 0 else if x == x then count(x, n - 1) else 1
  > echoln(show(even(100000)))
  > echoln(show(step(7, 100000)))
  > echoln(show(count("a", 100000)))
  > EOF
  $ keleut run --max-depth 10 tails.kel
  true
  7
  0

演算子の呼び出し(opcall)は末尾位置にあっても 1 段と数え、辞書を渡す非末尾の再帰も 1 段ずつ数える。

  $ cat > opcall.kel <<'EOF'
  > let add(a: Int32, b: Int32): Int32 = a + b
  > let rec f(n: Int32): Int32 = n match { case 0 => add(1, 2) case _ => 1 + f(n - 1) }
  > echoln(show(f(98)))
  > echoln(show(f(99)))
  > EOF
  $ keleut run --max-depth 100 opcall.kel
  101
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > dict.kel <<'EOF'
  > let rec g[A: Eq](x: A, n: Int32): Int32 = if n == 0 then 0 else { let r = g(x, n - 1); if x == x then r + 1 else r }
  > echoln(show(g(1, 50)))
  > echoln(show(g(1, 1000)))
  > EOF
  $ keleut run --max-depth 100 dict.kel
  50
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

式の入れ子は深さに数えない。

  $ cat > nest.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + { let a = { let b = { let c = { let d = { let e = { let g = { let h = { let i = f(n - 1); i }; h }; g }; e }; d }; c }; b }; a }
  > echoln(show(f(98)))
  > echoln(show(f(99)))
  > EOF
  $ keleut run --max-depth 100 nest.kel
  98
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
