閉じた標準出力への書き込みが失敗したときに、cancel 節を実行してから終了コード 74 で終了することのテストである(IRSpec 9.7 節)。
プログラムは diktor/test/eval.t の sinkfail.kel と同じで、バッファがいっぱいになった時点の flush が失敗する。

Rust のランタイムは、閉じた標準入出力(>&-)を起動時に /dev/null で開き直す。
keleut-rt の out.rs は、それより前に動く .init_array の関数で fcntl(1, F_GETFD) を呼び、ファイル記述子 1 が閉じていたかを記録して書き込みを EBADF で失敗させるので、keleut はこのテストで終了コード 74 で終了する。

  $ cat > sinkfail.kel <<'EOF2'
  > effect Res = { use: (String) => Unit }
  > let with_res[A, E](name: String, body: () => A @ {Res, Console extends E}): A @ {Console extends E} =
  >   body() handle {
  >     case use(s)    => resume(echoln(name + " uses " + s))
  >     case return(x) => { echoln("close " + name); x }
  >     case cancel    => __panic("cancel of " + name + " blew up")
  >   }
  > let rec loop(n: Int32): Unit @ Console = (n == 0) match {
  >   case true => {}
  >   case false => { echoln("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"); loop(n - 1) }
  > }
  > let main() = {
  >   with _ = with_res("r1")
  >   perform use("a")
  >   loop(100000)
  > }
  > main()
  > EOF2
  $ keleut run sinkfail.kel >&-
  cancel 節で例外が抑制されました: panic: cancel of r1 blew up
  keleut: 標準出力に書き出せません: Bad file descriptor
  [74]

防壁(cancel 節の実行中に、伝播中の状態を退避して守る手順)が抑制した例外を記録する前の flush が、標準出力への最初の出力エラーになる場合のテストである。
Diktor と同じく、抑制の記録を書かずに、出力エラーの診断だけを書いて、すぐに終了コード 74 で終了する(IRSpec 9.6 節の 4)。
残りの cancel 節も実行しない。

  $ cat > suppflush.kel <<'EOF2'
  > effect Fail = { throw: (String) => Never }
  > effect Res = { use: () => Unit }
  > echoln("before")
  > let r = ((({ perform throw("e"); 1 }) handle { case use() => resume(()) case cancel => __panic("in cancel") }) handle { case throw(e) => 2 })
  > echoln("after " + show(r))
  > EOF2
  $ keleut run suppflush.kel >&-
  keleut: 標準出力に書き出せません: Bad file descriptor
  [74]

cancel 節が、標準出力への出力エラーの後に書き込んでから脱出する場合のテストである。
出力エラーでバッファを捨てた後に書き足したバイトは、抑制の記録の前の flush で改めて書き出そうとして、また失敗する。
Diktor と同じく、抑制の記録を書かずに、出力エラーの診断だけを書いて、すぐに終了コード 74 で終了する(IRSpec 9.7 節)。

  $ cat > cancelwrite.kel <<'EOF2'
  > effect Res = { use: (String) => Unit }
  > let with_res[A, E](name: String, body: () => A @ {Res, Console extends E}): A @ {Console extends E} =
  >   body() handle {
  >     case use(s)    => resume(echoln(name + " uses " + s))
  >     case return(x) => { echoln("close " + name); x }
  >     case cancel    => { echoln("cancel " + name); __panic("in cancel " + name) }
  >   }
  > let rec big(n: Int32): Unit = if n == 0 then () else { echoln("line line line line line line line line line line " + show(n)); big(n - 1) }
  > let r = {
  >   with _ = with_res("r2")
  >   with _ = with_res("r1")
  >   perform use("a")
  >   big(3000)
  >   0
  > }
  > echoln(show(r))
  > EOF2
  $ keleut run cancelwrite.kel >&-
  keleut: 標準出力に書き出せません: Bad file descriptor
  [74]

標準出力が /dev/full の場合(ENOSPC)のテストである。
ENOSPC では、OCaml のチャネルと同じく、書けなかったバイトをバッファに残すので、抑制の記録の前の flush もまた失敗する。
Diktor と同じく、抑制の記録を書かずに、出力エラーの診断だけを書いて、すぐに終了コード 74 で終了する(IRSpec 9.7 節)。
EBADF で失敗する 26 行目のコマンドとは違い、抑制の記録は出ない。

  $ keleut run sinkfail.kel > /dev/full
  keleut: 標準出力に書き出せません: No space left on device
  [74]
