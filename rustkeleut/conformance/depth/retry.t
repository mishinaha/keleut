上限 10 のとき、防壁と再試行の組み合わせと、末尾 resume の各形が 10 万回完走することのテストである(IRSpec 9.6 節の 4、11.2 節)。
diktor/test/tail_calls.t のうち OCAMLRUNPARAM でスタックを絞ったブロック(既知の差分に台帳 V213 で登録してある)の中で、loops.t が扱っていないブロックが確かめる性質を、論理的な深さで確かめ直す。
プログラムは tail_calls.t のものと同じである。

cancel 節が再開しない操作を起こして始まった破棄を、防壁(cancel 節の実行中に、伝播中の状態を退避して守る手順)が抑制し、その後に操作節の本体から末尾呼び出しで再試行する。
cancel 節が起こした操作を受け取るのが、外側のハンドラ(retrycancel.kel)でも、再開する操作の節から再開しない操作を起こして外側に送るハンドラ(retrycancelvia.kel)でも、破棄を進めているハンドラ自身(retrycancelsame.kel)でも、深さは再試行の回数に応じて増えない。
抑制するたびに、同じ抑制の記録を 1 行書く。

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

cancel 節が起こした操作を受け取った外側のハンドラが、return 節から末尾呼び出しで再帰しても、深さは回数に応じて増えない。

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

操作節の末尾 resume の 5 つの形(節が resume だけの形、文の後、match の節、run の本体、&& の右辺)を確かめる。

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

プレリュードの with_stdout の中で実行するループを確かめる。

  $ cat > stdout.kel <<'EOF'
  > let rec loop(n: Int32): Unit @ Print = (n == 0) match { case true => () case false => { perform print(""); loop(n - 1) } }
  > with_stdout(fn() => loop(100000))
  > echoln("ok")
  > EOF
  $ keleut run --max-depth 10 stdout.kel
  ok
