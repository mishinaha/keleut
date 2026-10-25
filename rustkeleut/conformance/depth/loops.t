限度 10 と 10000 の下で、一定の領域で走るべき繰り返しが完走し、そうでない再帰が 3 で終わることの試験である(計画 261024-1 の 8.1 節、9 節の M89 の行)。
diktor/test/tail_calls.t の OCAMLRUNPARAM で絞ったブロック(V213 で除外する)の意図を、論理的な深さで作り直した。

限度 10 の下で、T の節の末尾の resume、G の節の末尾の resume、N の節からの再試行が 10 万回続く(ir_depth.t の loops.kel)。

  $ cat > loops.kel <<'EOF'
  > effect Tick = { tick: () => Unit }
  > effect Fail = { fail: () => Never }
  > let rec ticks(n: Int32): Unit @ {Tick} = if n == 0 then () else { perform tick(); ticks(n - 1) }
  > let rec retry(n: Int32): Int32 = (if n == 0 then 0 else { perform fail(); 1 }) handle { case fail() => retry(n - 1) }
  > ticks(100000) handle { case tick() => resume(()) }
  > echoln("T")
  > ticks(100000) handle { case tick() => if false then { let r = resume(()); r } else resume(()) }
  > echoln("G")
  > echoln(show(retry(100000)))
  > EOF
  $ keleut run --max-depth 10 loops.kel
  T
  G
  0

限度 10000 の下で、末尾呼び出し、T と G の節の末尾の resume、N の再試行が 10 万回完走する。

  $ cat > loops10k.kel <<'EOF'
  > effect Tick = { tick: () => Unit }
  > effect Fail = { fail: () => Never }
  > let rec loop(n: Int32): Int32 = if n == 0 then 0 else loop(n - 1)
  > let rec ticks(n: Int32): Unit @ {Tick} = if n == 0 then () else { perform tick(); ticks(n - 1) }
  > let rec retry(n: Int32): Int32 = (if n == 0 then 0 else { perform fail(); 1 }) handle { case fail() => retry(n - 1) }
  > echoln(show(loop(100000)))
  > ticks(100000) handle { case tick() => resume(()) }
  > echoln("T")
  > ticks(100000) handle { case tick() => if false then { let r = resume(()); r } else resume(()) }
  > echoln("G")
  > echoln(show(retry(100000)))
  > EOF
  $ keleut run --max-depth 10000 loops10k.kel
  0
  T
  G
  0

末尾でない再帰は 3 で終わる。

  $ cat > nontail10k.kel <<'EOF'
  > let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)
  > echoln(show(f(9998)))
  > echoln(show(f(100000)))
  > echoln("not reached")
  > EOF
  $ keleut run --max-depth 10000 nontail10k.kel
  9998
  実行時エラー: 論理的な深さが限度 10000 を超えました
  [3]

G の節の末尾でない resume は、再開のたびに 1 段ずつ深くなるので 3 で終わる。

  $ cat > gen10k.kel <<'EOF'
  > effect Tick = { tick: () => Unit }
  > let rec ticks(n: Int32): Unit @ {Tick} = if n == 0 then () else { perform tick(); ticks(n - 1) }
  > let count(n: Int32): Int32 = { ticks(n); 0 } handle { case tick() => { let r = resume(()); r + 1 } }
  > echoln(show(count(3000)))
  > echoln(show(count(100000)))
  > echoln("not reached")
  > EOF
  $ keleut run --max-depth 10000 gen10k.kel
  3000
  実行時エラー: 論理的な深さが限度 10000 を超えました
  [3]
