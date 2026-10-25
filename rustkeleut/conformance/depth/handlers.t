ハンドラ式の論理的な深さの試験である(IRSpec 11.2 節〜11.5 節、計画 261024-1 の 4.10 節)。
diktor/test/ir_depth.t の後半のブロックを、keleut run --max-depth で走らせる形に写した。
各ブロックの深さの計算は ir_depth.t の説明のとおりである。

T の節の本体は perform の深さ p + 1 で、G の節の本体はハンドラ式の深さ h + 1 で走る。

  $ cat > tg.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let rec dive(k: Int32): Int32 @ {Ask} = if k == 0 then perform ask() else 1 + dive(k - 1)
  > let t(k: Int32, n: Int32): Int32 = dive(k) handle { case ask() => resume(f(n)) }
  > let g(k: Int32, n: Int32): Int32 = dive(k) handle { case ask() => { let v = f(n); let r = resume(v); r } }
  > echoln(show(g(50, 96)))
  > echoln(show(t(50, 44)))
  > echoln(show(t(50, 45)))
  > EOF
  $ keleut run --max-depth 100 tg.kel
  146
  94
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > tg2.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let rec dive(k: Int32): Int32 @ {Ask} = if k == 0 then perform ask() else 1 + dive(k - 1)
  > let g(k: Int32, n: Int32): Int32 = dive(k) handle { case ask() => { let v = f(n); let r = resume(v); r } }
  > echoln(show(g(50, 96)))
  > echoln(show(g(50, 97)))
  > EOF
  $ keleut run --max-depth 100 tg2.kel
  146
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

入れ子のハンドラ式の中で、外側の G の節が c ≠ h の深さで再開すると、内側のハンドラの深さもずれる。
内側の return 節は限度の近くで走る(計画 4.10 節の conformance/depth/ の 1 つ目)。

  $ cat > shift.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > effect Tick = { tick: () => Unit }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let inner(n: Int32): Int32 @ {Tick} = { perform tick(); 0 } handle { case ask() => resume(0) case return(v) => f(n) }
  > let outer(n: Int32): Int32 = inner(n) handle { case tick() => { let r = resume(()); r } }
  > echoln(show(outer(95)))
  > echoln(show(outer(96)))
  > EOF
  $ keleut run --max-depth 100 shift.kel
  95
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

T の節の本体が外側の G の操作を起こし、外側が c ≠ h で再開すると、連鎖から外れていた内側のハンドラの深さもずれる(2 つ目)。

  $ cat > shiftt.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > effect Tick = { tick: () => Unit }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let inner(n: Int32): Int32 @ {Tick} = perform ask() handle { case ask() => { perform tick(); resume(1) } case return(v) => v + f(n) }
  > let outer(n: Int32): Int32 = inner(n) handle { case tick() => { let r = resume(()); r } }
  > echoln(show(outer(94)))
  > echoln(show(outer(95)))
  > EOF
  $ keleut run --max-depth 100 shiftt.kel
  95
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

継続の中の、まだ再開していない継続も一緒にずれる。
捨てたときは、その中のハンドラの cancel 節がずれた後の深さで走る。

  $ cat > k2.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > effect Tick = { tick: () => Unit }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let inner(n: Int32): Int32 @ {Tick} = { let a = perform ask(); a + f(n) } handle { case ask() => { perform tick(); let r = resume(1); r } }
  > let outer(n: Int32): Int32 = inner(n) handle { case tick() => { let r = resume(()); r } }
  > echoln(show(outer(91)))
  > echoln(show(outer(92)))
  > echoln(show(outer(93)))
  > EOF
  $ keleut run --max-depth 100 k2.kel
  92
  93
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > k2drop.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > effect Tick = { tick: () => Unit }
  > effect Zz = { zz: () => Unit }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let inner(m: Int32): Int32 @ {Tick, Console} = ({ let a = perform ask(); a } handle { case zz() => resume(()) case cancel => echoln(show(f(m))) }) handle { case ask() => { perform tick(); if 1 == 2 then { let r = resume(1); r } else 5 } }
  > let outerG(m: Int32): Int32 = inner(m) handle { case tick() => { let r = resume(()); r } }
  > let outerT(m: Int32): Int32 = inner(m) handle { case tick() => resume(()) }
  > echoln(show(outerG(93)))
  > echoln(show(outerG(94)))
  > echoln(show(outerT(94)))
  > echoln(show(outerT(95)))
  > EOF
  $ keleut run --max-depth 100 k2drop.kel
  93
  5
  cancel 節で例外が抑制されました: 論理的な深さが限度 100 を超えました
  5
  94
  5
  cancel 節で例外が抑制されました: 論理的な深さが限度 100 を超えました
  5

末尾でない resume が限度を超えると、resume の直後の出力の前に 3 で終わる(3 つ目)。
継続はまだ使っていないので、ハンドラの cancel 節が走る。

  $ cat > over.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > effect Tick = { tick: () => Unit }
  > let rec dive(k: Int32): Int32 @ {Ask} = if k == 0 then perform ask() else 1 + dive(k - 1)
  > let d(k: Int32): Int32 = dive(k) handle {
  >   case ask() => { let r = resume(1) handle { case tick() => resume(()) }; echoln("after"); r }
  >   case cancel => echoln("cancel")
  > }
  > echoln(show(d(95)))
  > echoln(show(d(96)))
  > EOF
  $ keleut run --max-depth 100 over.kel
  after
  96
  cancel
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

深さの限度の誤りも、ハンドラを内側から巻き戻して cancel 節を走らせる。
伝播している間は予備 R = max(1024, L / 64) の中で cancel 節が走り、予備も超えた脱出は打ち切って記録する。

  $ cat > cancel.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let c2(): Int32 = (f(1000) handle { case ask() => resume(0) case cancel => echoln("cancel inner") }) handle { case ask() => resume(0) case cancel => echoln("cancel outer") }
  > echoln(show(c2()))
  > EOF
  $ keleut run --max-depth 100 cancel.kel
  cancel inner
  cancel outer
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > reserve.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let c1(m: Int32): Int32 = f(1000) handle { case ask() => resume(0) case cancel => echoln(show(f(m))) }
  > echoln(show(c1(1121)))
  > EOF
  $ keleut run --max-depth 100 reserve.kel
  1121
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > reserve2.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let c1(m: Int32): Int32 = f(1000) handle { case ask() => resume(0) case cancel => echoln(show(f(m))) }
  > echoln(show(c1(1122)))
  > EOF
  $ keleut run --max-depth 100 reserve2.kel
  cancel 節で例外が抑制されました: 論理的な深さが限度 100 を超えました
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]

cancel 節は走る前の限度を覚え、節を出るときに戻す。

  $ cat > rs.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > effect Fail = { fail: () => Never }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let inner() = { perform fail(); 0 } handle { case ask() => resume(0) case cancel => echoln(show(f(200))) }
  > let r = inner() handle { case fail() => 7 }
  > echoln(show(r))
  > echoln(show(f(500)))
  > EOF
  $ keleut run --max-depth 100 rs.kel
  cancel 節で例外が抑制されました: 論理的な深さが限度 100 を超えました
  7
  実行時エラー: 論理的な深さが限度 100 を超えました
  [3]
  $ cat > rs2.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > let z(d: Int32): Int32 = 1 / d
  > let c(): Int32 = (z(0) handle { case ask() => resume(0) case cancel => echoln(show(f(200))) }) handle { case ask() => resume(0) case cancel => echoln(show(f(500))) }
  > echoln(show(c()))
  > EOF
  $ keleut run --max-depth 100 rs2.kel
  cancel 節で例外が抑制されました: 論理的な深さが限度 100 を超えました
  cancel 節で例外が抑制されました: 論理的な深さが限度 100 を超えました
  実行時エラー: ゼロ除算です
  [3]

pinned が限度に届く再帰の本体を呼ぶ(4 つ目)は basic.t の pinned.kel にある。
