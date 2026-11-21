上限 10 と 10000 のとき、一定のメモリで実行できるはずの繰り返しが完走し、そうでない再帰が終了コード 3 で終了することのテストである(IRSpec 11.2 節)。
diktor/test/tail_calls.t のうち OCAMLRUNPARAM でスタックを絞ったブロック(既知の差分に台帳 V213 で登録してある)が確かめる性質を、論理的な深さで確かめ直す。

上限 10 のとき、T の節の末尾 resume、G の節の末尾 resume、N の節からの再試行を 10 万回続けられる(T、G、N は IRSpec 9.2 節の分類で、プログラムは ir_depth.t の loops.kel と同じ)。

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

上限 10000 のとき、末尾呼び出し、T と G の節の末尾 resume、N の節からの再試行が 10 万回完走する。

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

非末尾の再帰は、上限を超えて終了コード 3 で終了する。

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

G の節の非末尾の resume は、再開のたびに 1 段ずつ深くなるので、上限を超えて終了コード 3 で終了する。

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
