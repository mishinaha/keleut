G の節(継続を末尾でなく再開するか、捨てるか、2 回以上再開しうる操作節)の取り出しと再開の試験である(IRSpec 9 節、計画 261024-1 の 6.6 節と 6.10 節)。
記録は keleut run の結果で、Diktor の木の評価器の結果とも同じである。
WasmGC の後段(組 kw)では、継続はフレームの記録の列で、再開は各フレームの再開の入口を最上段から順に呼ぶことである。

生成器。G の節の末尾の resume は一定の領域で 10 万回再開し、末尾でない resume は再開を 10 万段重ねる。

  $ cat > gen.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let rec count(i: Int32, n: Int32): Unit @ {Gen} = if i == n then () else { perform yield(i); count(i + 1, n) }
  > let sum(n: Int32): Int32 = run h {
  >   let acc = Ref.new(0)
  >   count(0, n) handle { case yield(x) => { Ref.set(acc, Ref.get(acc) + x); resume(()) } }
  >   Ref.get(acc)
  > }
  > let collect(n: Int32): List[Int32] = count(0, n) handle { case yield(x) => { let r = resume(()); Cons(x, r) } case return(u) => Nil }
  > let rec len(xs: List[Int32], a: Int32): Int32 = xs match { case Nil => a case Cons(_, t) => len(t, a + 1) }
  > let rec take(xs: List[Int32], k: Int32): String = if k == 0 then "" else xs match { case Nil => "" case Cons(x, t) => show(x) + " " + take(t, k - 1) }
  > echoln(show(sum(100000)))
  > let xs = collect(100000)
  > echoln(show(len(xs, 0)))
  > echoln(take(xs, 5))
  > EOF
  $ keleut run gen.kel
  704982704
  100000
  0 1 2 3 4 

深い再帰の底で yield する。
継続は 1,000 段のフレームを持ち、再開のたびにそのすべてを再開の入口から続ける。

  $ cat > deep.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let rec dive(d: Int32, k: Int32): Int32 @ {Gen} = if d == 0 then { perform yield(k); k } else 1 + dive(d - 1, k)
  > let rec many(i: Int32, n: Int32, d: Int32): Int32 @ {Gen} = if i == n then 0 else dive(d, i) + many(i + 1, n, d)
  > let go(n: Int32, d: Int32): Int32 = run h {
  >   let c = Ref.new(0)
  >   let r = many(0, n, d) handle { case yield(x) => { Ref.set(c, Ref.get(c) + x); let u = resume(()); u } }
  >   r + Ref.get(c)
  > }
  > echoln(show(go(100, 1000)))
  > echoln(show(go(1000, 10)))
  > EOF
  $ keleut run deep.kel
  109900
  1009000

継続は高々 1 回しか再開できない。
2 回目の resume は実行時エラー(付録 B の 7 番)で、1 回目の再開の出力の後に 3 で終わる。
継続は使ったので、ハンドラの cancel 節は走らない。

  $ cat > affine.kel <<'EOF'
  > effect Ask = { ask: (Int32) => Int32 }
  > let r = ({ let a = perform ask(1); echoln("cont saw " + show(a)); a }) handle {
  >   case ask(n) => { let x = resume(n + 1); echoln("first " + show(x)); let y = resume(n + 2); x + y }
  >   case cancel => echoln("cancel")
  > }
  > echoln(show(r))
  > EOF
  $ keleut run affine.kel
  cont saw 2
  first 2
  実行時エラー: resume は高々1回しか呼べません(アフィン)
  [3]

再開の間に多くの値を確保して捨てる(WasmGC の後段では V8 の回収が走る)。
継続のフレームの記録が持つ値は回収されない。

  $ cat > churn.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
  > let rec len(xs: List[Int32], a: Int32): Int32 = xs match { case Nil => a case Cons(_, t) => len(t, a + 1) }
  > let rec work(i: Int32, n: Int32, keep: List[Int32]): Int32 @ {Gen} = if i == n then len(keep, 0) else { perform yield(i); work(i + 1, n, keep) }
  > let r = work(0, 200, build(1000, Nil)) handle { case yield(x) => { let junk = build(20000, Nil); let r = resume(()); r + len(junk, 0) - 20000 } }
  > echoln(show(r))
  > EOF
  $ keleut run churn.kel
  1000

G の節と N の節と T の節を重ねる。
G の節の本体が外側の N の操作を起こすと、継続を捨て、継続の中の cancel 節が内側から走る。
T の節は外側の G の節の継続にも含まれる。

  $ cat > mixed.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > effect Fail = { fail: (String) => Never }
  > effect Ask = { ask: () => Int32 }
  > let rec count(i: Int32, n: Int32): Unit @ {Gen, Ask} = if i == n then () else { let a = perform ask(); perform yield(i + a); count(i + 1, n) }
  > let r = (count(0, 5) handle {
  >   case yield(x) => { echoln("y " + show(x)); if x == 13 then perform fail("thirteen") else { let u = resume(()); u } }
  >   case cancel => echoln("cancel gen")
  > }) handle {
  >   case ask() => resume(10)
  > } handle {
  >   case fail(m) => { echoln("failed " + m); () }
  > }
  > let r2 = ((count(0, 3) handle {
  >   case ask() => { echoln("ask"); resume(100) }
  > }) handle {
  >   case yield(x) => { echoln("y2 " + show(x)); let u = resume(()); u }
  > })
  > echoln("done")
  > EOF
  $ keleut run mixed.kel
  y 10
  y 11
  y 12
  y 13
  cancel gen
  failed thirteen
  ask
  y2 100
  ask
  y2 101
  ask
  y2 102
  done

T の節の本体が外側の G の操作を起こし、内側のハンドラの外で継続が再開される。
内側のハンドラの G の節が外側の G の操作を起こすと、外側の継続は内側の節のフレームと、内側のハンドラのフレームを含む。

  $ cat > nest.kel <<'EOF'
  > effect Tick = { tick: (Int32) => Unit }
  > effect Ask = { ask: () => Int32 }
  > let inner(n: Int32): Int32 @ {Tick} = { let a = perform ask(); let b = perform ask(); a * 10 + b } handle {
  >   case ask() => { perform tick(n); let r = resume(n); perform tick(r); r + 1 }
  >   case return(v) => { perform tick(v); v }
  > }
  > let outer(n: Int32): Int32 = inner(n) handle { case tick(t) => { echoln("tick " + show(t)); let r = resume(()); r * 2 } }
  > echoln(show(outer(3)))
  > let t2 = (perform ask() + perform ask()) handle { case ask() => { perform tick(7); resume(5) } } handle { case tick(t) => { echoln("t " + show(t)); let r = resume(()); r + 100 } }
  > echoln(show(t2))
  > EOF
  $ keleut run nest.kel
  tick 3
  tick 3
  tick 33
  tick 33
  tick 34
  1120
  t 7
  t 7
  210

辞書を通るメソッドの呼び出し、メソッドの値、引数の多い関数の中で yield する。
引数が 32 個を超える関数は、残りの引数を 1 つの配列で受け取る(WasmGC の後段)。

  $ cat > via.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > type class Run[A] { val run_[E]: (A, () => Unit @ E) => Int32 @ E }
  > type instance Run[Int32] { let run_[E](n: Int32, f: () => Unit @ E): Int32 @ E = { f(); f(); n } }
  > type instance[A: Run] Run[List[_]] { let run_[E](xs: List[A], f: () => Unit @ E): Int32 @ E = xs match { case Nil => 0 case Cons(x, t) => run_(x, f) + run_(t, f) } }
  > let k = run_
  > let r = k(Cons(1, Cons(2, Nil)), fn() => perform yield(1)) handle { case yield(x) => { let u = resume(()); u + 1000 } }
  > echoln(show(r))
  > let rec many(d: Int32, p0: Int32, p1: Int32, p2: Int32, p3: Int32, p4: Int32, p5: Int32, p6: Int32, p7: Int32, p8: Int32, p9: Int32, p10: Int32, p11: Int32, p12: Int32, p13: Int32, p14: Int32, p15: Int32, p16: Int32, p17: Int32, p18: Int32, p19: Int32, p20: Int32, p21: Int32, p22: Int32, p23: Int32, p24: Int32, p25: Int32, p26: Int32, p27: Int32, p28: Int32, p29: Int32, p30: Int32, p31: Int32, p32: Int32, p33: Int32, p34: Int32): Int32 @ {Gen} =
  >   if d == 0 then { perform yield(p0 + p34); 0 } else { perform yield(d); let r = many(d - 1, p0 + 1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18, p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34 + 2); r + p0 + p33 + p34 }
  > let s = many(20, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34) handle { case yield(x) => { let u = resume(()); u + x } }
  > echoln(show(s))
  > EOF
  $ keleut run via.kel
  4003
  2214

組み込みの Array.each と pinned の中の perform を取り出す。
組み込みの本体は LIR の関数なので、その中の呼び出しも再開点を持ち、継続は組み込みのフレームを含む。
G の節の末尾でない resume で、Array.each の中の yield を集める:

  $ cat > each_collect.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let arr = run h { let m = MutableArray.new(5, 0); MutableArray.set(m, 0, 10); MutableArray.set(m, 1, 20); MutableArray.set(m, 2, 30); MutableArray.set(m, 3, 40); MutableArray.set(m, 4, 50); MutableArray.freeze(m) }
  > let collect(): List[Int32] = Array.each(arr, fn(x) => perform yield(x)) handle { case yield(x) => { let r = resume(()); Cons(x, r) } case return(u) => Nil }
  > let rec show_list(xs: List[Int32]): String = xs match { case Nil => "." case Cons(x, t) => show(x) + " " + show_list(t) }
  > echoln(show_list(collect()))
  > EOF
  $ keleut run each_collect.kel
  10 20 30 40 50 .

G の節の末尾の resume で、Array.each の中の yield の値を足す:

  $ cat > each_sum.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let arr = run h { let m = MutableArray.new(5, 0); MutableArray.set(m, 0, 10); MutableArray.set(m, 1, 20); MutableArray.set(m, 2, 30); MutableArray.set(m, 3, 40); MutableArray.set(m, 4, 50); MutableArray.freeze(m) }
  > let sum(): Int32 = run h {
  >   let acc = Ref.new(0)
  >   Array.each(arr, fn(x) => perform yield(x)) handle { case yield(x) => { Ref.set(acc, Ref.get(acc) + x); resume(()) } }
  >   Ref.get(acc)
  > }
  > echoln(show(sum()))
  > EOF
  $ keleut run each_sum.kel
  150

Never を返す操作で、Array.each の途中から抜ける。
操作を処理したハンドラが自分で継続を破棄するので、そのハンドラ自身の cancel 節は走らず、操作節の値がハンドラ式の値になる:

  $ cat > each_stop.kel <<'EOF'
  > effect Stop = { stop: (Int32) => Never }
  > let arr = run h { let m = MutableArray.new(5, 0); MutableArray.set(m, 0, 10); MutableArray.set(m, 1, 20); MutableArray.set(m, 2, 30); MutableArray.set(m, 3, 40); MutableArray.set(m, 4, 50); MutableArray.freeze(m) }
  > let first_big(): Int32 = { Array.each(arr, fn(x) => if x > 25 then perform stop(x) else echoln("skip " + show(x))); 0 } handle { case stop(x) => x case cancel => echoln("cancel") }
  > echoln(show(first_big()))
  > EOF
  $ keleut run each_stop.kel
  skip 10
  skip 20
  30

pinned の中で yield し、G の節の末尾でない resume で再開する:

  $ cat > pinned_yield.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let p(): Int32 = pinned(fn() => { perform yield(7); 8 }) handle { case yield(x) => { echoln("pinned yield " + show(x)); let r = resume(()); r + 1 } }
  > echoln(show(p()))
  > EOF
  $ keleut run pinned_yield.kel
  pinned yield 7
  9
