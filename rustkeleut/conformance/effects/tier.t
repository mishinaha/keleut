段をまたぐ取り出しと再開の試験である(計画 261024-1 の 6.9 節)。
記録は keleut run の結果で、Diktor の木の評価器の結果とも同じである。
どのプログラムも、ハンドラと perform と継続の再開を、多くの関数の呼び出しの中で何度もくり返す。
そのため、段の設定の組(kt、ka、km)では、取り出しと再開が段の境目をまたぐ。

G の節と T の節を 3,000 回まわす。
組 kt では、初めはインタプリタで走り、翻訳のスレッドが翻訳を終えた関数から JIT のコードに移る。
組 ka では、関数の入口ごとに段が変わるので、perform のフレーム、節のフレーム、ハンドラのフレーム、継続を写し戻したフレームが段をまたぐ。
組 km では、関数ごとに段が決まり、種によって perform とハンドラの段の組み合わせが変わる。

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

深さ 40 までの再帰の各段で yield し、底で N の操作を起こすことを 2,000 回くり返す。
内側のハンドラの cancel 節が数えた回数も結果に含める。

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

T の節の本体が外側の G の操作を起こし、継続の中の内側のハンドラのフレームを外側の節の再開で写し戻す。
これを 3,000 回くり返す。

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

500 回目の計算で、再開した継続の中の節がゼロ除算を起こす。
誤りは継続の中のハンドラの cancel 節を走らせ、3 で終わる。

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
