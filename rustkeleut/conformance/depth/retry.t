限度 10 の下で、防壁と再試行の組と、末尾の resume の形が 10 万回完走することの試験である(計画 261024-1 の 6.6 節、8.1 節)。
diktor/test/tail_calls.t の OCAMLRUNPARAM で絞ったブロック(V213 で除外する)のうち、loops.t が写していないものを、論理的な深さで作り直した。
プログラムは tail_calls.t のものと同じである。

cancel 節から起こした再開しない操作による破棄を防壁が打ち切った後に、節の本体から末尾で再試行する。
打ち切るのが外側のハンドラ(retrycancel.kel)でも、再開する操作の節から送出を受けるハンドラ(retrycancelvia.kel)でも、破棄を進めている当のハンドラ(retrycancelsame.kel)でも、深さは再試行の回数で増えない。
打ち切るたびに同じ記録を 1 行書く。

  $ cat > retrycancel.kel <<'EOF'
  > effect Fail = { throw: (String) => Never }
  > effect Stop = { stop: () => Never }
  > effect Res = { use: () => Unit }
  > let rec loop(n: Int32): Int32 =
  >   if n == 0 then 0 else
  >   ({ perform use(); perform stop(); 1 }) handle {
  >     case use() => resume()
  >     case cancel => perform throw("from cancel")
  >   } handle {
  >     case stop() => loop(n - 1)
  >   }
  > let r = loop(100000) handle {
  >   case throw(e) => { echoln("caught " + e); 1 }
  > }
  > echoln(show(r))
  > EOF
  $ keleut run --max-depth 10 retrycancel.kel 2> err.txt
  0
  $ sort err.txt | uniq -c | sed 's/^ *//'
  100000 cancel 節で例外が抑制されました: cancel 節から操作の巻き戻しで脱出しようとしました

  $ cat > retrycancelvia.kel <<'EOF'
  > effect Fail = { throw: (String) => Never }
  > effect Ask = { ask: () => Int32 }
  > effect Stop = { stop: () => Never }
  > effect Res = { use: () => Unit }
  > let rec loop(n: Int32): Int32 =
  >   if n == 0 then 0 else
  >   ({ perform use(); perform stop(); 1 }) handle {
  >     case use() => resume()
  >     case cancel => { let a = perform ask(); () }
  >   } handle {
  >     case stop() => loop(n - 1)
  >   }
  > let r = (loop(100000) handle {
  >   case ask() => perform throw("from ask")
  > }) handle {
  >   case throw(e) => { echoln("caught " + e); 1 }
  > }
  > echoln(show(r))
  > EOF
  $ keleut run --max-depth 10 retrycancelvia.kel 2> err.txt
  0
  $ sort err.txt | uniq -c | sed 's/^ *//'
  100000 cancel 節で例外が抑制されました: cancel 節から操作の巻き戻しで脱出しようとしました

  $ cat > retrycancelsame.kel <<'EOF'
  > effect Fail = { throw: (String) => Never }
  > effect Res = { use: () => Unit }
  > let rec loop(n: Int32): Int32 =
  >   if n == 0 then 0 else
  >   ({ perform use(); perform throw("first") }) handle {
  >     case use() => resume()
  >     case cancel => perform throw("from cancel")
  >   } handle {
  >     case throw(e) => loop(n - 1)
  >   }
  > echoln(show(loop(100000)))
  > EOF
  $ keleut run --max-depth 10 retrycancelsame.kel 2> err.txt
  0
  $ sort err.txt | uniq -c | sed 's/^ *//'
  100000 cancel 節で例外が抑制されました: cancel 節から操作の巻き戻しで脱出しようとしました

打ち切られた外側のハンドラの return 節から末尾で再帰しても、深さは回数で増えない。

  $ cat > retcancelret.kel <<'EOF'
  > effect Fail = { throw: (String) => Never }
  > effect Stop = { stop: () => Never }
  > effect Res = { use: () => Unit }
  > let rec loop(n: Int32): Int32 =
  >   if n == 0 then 0 else
  >   (({ perform use(); perform stop(); 1 }) handle {
  >     case use() => resume()
  >     case cancel => perform throw("from cancel")
  >   } handle {
  >     case stop() => 5
  >   }) handle {
  >     case throw(e) => { echoln("caught " + e); 1 }
  >     case return(x) => loop(n - 1)
  >   }
  > echoln(show(loop(100000)))
  > EOF
  $ keleut run --max-depth 10 retcancelret.kel 2> err.txt
  0
  $ sort err.txt | uniq -c | sed 's/^ *//'
  100000 cancel 節で例外が抑制されました: cancel 節から操作の巻き戻しで脱出しようとしました

操作節の末尾の resume の 5 つの形(節が resume だけの形、文の後、match の腕、run の本体、&& の右辺)である。

  $ cat > resume.kel <<'EOF'
  > effect Tick = { tick: () => Unit }
  > effect Ask = { ask: (Int32) => Int32 }
  > effect Test = { test: () => Boolean }
  > let rec ticks(n: Int32): Unit @ Tick = (n == 0) match { case true => () case false => { perform tick(); ticks(n - 1) } }
  > let rec asks(n: Int32, acc: Int32): Int32 @ Ask =
  >   (n == 0) match { case true => acc case false => asks(n - 1, acc + perform ask(n)) }
  > let rec tests(n: Int32): Boolean @ Test = (n == 0) match { case true => true case false => perform test() && tests(n - 1) }
  > ticks(100000) handle { case tick() => resume() }
  > echoln("bare")
  > let total = run h {
  >   let c = Ref.new(0)
  >   ticks(100000) handle { case tick() => { Ref.set(c, Ref.get(c) + 1); resume() } }
  >   Ref.get(c)
  > }
  > echoln(show(total))
  > echoln(show(asks(100000, 0) handle { case ask(x) => (x > 0) match { case true => resume(1) case false => resume(0) } }))
  > ticks(100000) handle { case tick() => run h { let r = Ref.new(()); resume(Ref.get(r)) } }
  > echoln("run")
  > echoln(show(tests(100000) handle { case test() => { let t = true; t && resume(true) } }))
  > EOF
  $ keleut run --max-depth 10 resume.kel
  bare
  100000
  100000
  run
  true

プレリュードの with_stdout の中のループである。

  $ cat > stdout.kel <<'EOF'
  > let rec loop(n: Int32): Unit @ Print = (n == 0) match { case true => () case false => { perform print(""); loop(n - 1) } }
  > with_stdout(fn() => loop(100000))
  > echoln("ok")
  > EOF
  $ keleut run --max-depth 10 stdout.kel
  ok
