ティアの境界をまたぐ、継続のキャプチャと再開のテストである。
期待出力は keleut run の結果で、Diktor の構文木の評価器の結果とも一致する。
どのプログラムも、多くの関数呼び出しの中で、ハンドラ式の評価、perform、継続の再開を何度も繰り返す。
そのため、ティアを切り替えるペア(kt、ka、km)では、継続のキャプチャと再開がティアの境界をまたぐ。

G の節と T の節(IRSpec 9.2 節の分類)を使う計算を 3,000 回繰り返す。
ペア kt では、どの関数も初めはインタプリタで実行され、コンパイルスレッドがコンパイルを終えた関数から順に JIT コードでの実行に切り替わる。
ペア ka では、関数に入るたびにティアが変わるので、perform のフレーム、操作節のフレーム、ハンドラのフレーム、継続から値スタックに復元したフレームがティアの境界をまたぐ。
ペア km では、ティアが関数ごとに決まり、シードによって perform とハンドラのティアの組み合わせが変わる。

  $ cat > loop.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > effect Ask = { ask: (Int32) => Int32 }
  > let rec count(i: Int32, n: Int32): Unit @ {Gen} = if i == n then () else { perform yield(i); count(i + 1, n) }
  > let sum(n: Int32): Int32 = run h {
  >   let acc = Ref.new(0)
  >   count(0, n) handle { case yield(x) => { Ref.set(acc, Ref.get(acc) + x); let u = resume(()); u } }
  >   Ref.get(acc)
  > }
  > let rec asks(i: Int32, n: Int32, a: Int32): Int32 @ {Ask} = if i == n then a else asks(i + 1, n, a + perform ask(i))
  > let twice(n: Int32): Int32 = asks(0, n, 0) handle { case ask(x) => resume(x * 2) }
  > let rec rounds(k: Int32, a: Int32): Int32 = if k == 0 then a else rounds(k - 1, a + sum(50) + twice(30))
  > echoln(show(rounds(3000, 0)))
  > EOF
  $ keleut run loop.kel
  6285000

最大 40 段の再帰の各段で yield し、底で N の操作(返り値の型が Never の操作)を起こす計算を、2,000 回繰り返す。
内側のハンドラの cancel 節が実行された回数も結果に含める。

  $ cat > abort.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > effect Fail = { fail: (String) => Never }
  > let rec dig(d: Int32): Int32 @ {Gen, Fail} = if d == 0 then perform fail("bottom") else { perform yield(d); 1 + dig(d - 1) }
  > let attempt[h](c: Ref[h, Int32], d: Int32): Int32 @ {Heap[h]} =
  >   (dig(d) handle { case yield(x) => { let u = resume(()); u } case cancel => Ref.set(c, Ref.get(c) + 1) }) handle { case fail(m) => 0 - d }
  > let rec go[h](c: Ref[h, Int32], k: Int32, a: Int32): Int32 @ {Heap[h]} = if k == 0 then a else go(c, k - 1, a + attempt(c, k % 40))
  > let r = run h {
  >   let c = Ref.new(0)
  >   let s = go(c, 2000, 0)
  >   s * 10000 + Ref.get(c)
  > }
  > echoln(show(r))
  > EOF
  $ keleut run abort.kel
  -389998000

内側の G の節(ask)の本体が、外側の G の節が受け取る操作(tick)を起こし、外側の節が継続を再開するときに、継続の中にある内側のハンドラのフレームを値スタックに復元する。
この計算を 3,000 回繰り返す。

  $ cat > relay.kel <<'EOF'
  > effect Tick = { tick: (Int32) => Unit }
  > effect Ask = { ask: () => Int32 }
  > let inner(n: Int32): Int32 @ {Tick} = { let a = perform ask(); let b = perform ask(); a * 10 + b } handle {
  >   case ask() => { perform tick(n); let r = resume(n); perform tick(r); r + 1 }
  > }
  > let outer(n: Int32): Int32 = run h {
  >   let s = Ref.new(0)
  >   let r = inner(n) handle { case tick(t) => { Ref.set(s, Ref.get(s) + t); let r = resume(()); r * 2 } }
  >   r + Ref.get(s)
  > }
  > let rec loop(k: Int32, a: Int32): Int32 = if k == 0 then a else loop(k - 1, (a + outer(k % 9)) % 1000003)
  > echoln(show(loop(3000, 0)))
  > EOF
  $ keleut run relay.kel
  497794

500 回目の計算で、再開した継続の中で実行される操作節がゼロ除算を起こす。
この実行時エラーで、継続の中にあるハンドラの cancel 節が実行され、終了コード 3 で終了する。

  $ cat > late.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let rec count(i: Int32, n: Int32): Unit @ {Gen} = if i == n then () else { perform yield(i); count(i + 1, n) }
  > let total(n: Int32, bad: Int32): Int32 = run h {
  >   let acc = Ref.new(0)
  >   count(0, n) handle { case yield(x) => { Ref.set(acc, Ref.get(acc) + 1000 / (x - bad)); let u = resume(()); u } case cancel => echoln("cancel") }
  >   Ref.get(acc)
  > }
  > let rec rounds(k: Int32, a: Int32): Int32 = if k == 0 then a else { let b = if k == 1 then 7 else 0 - 1; rounds(k - 1, a + total(20, b)) }
  > echoln(show(rounds(500, 0)))
  > EOF
  $ keleut run late.kel
  cancel
  実行時エラー: ゼロ除算です
  [3]
