閉じた標準出力への書き込みが cancel 節を走らせてから 74 で終わる試験である(計画 261024-1 の 6.5 節の出力。diktor/test/eval.t:820-848 を写したもの)。
ハンドラと cancel 節を使うので、M89 の受け入れで確かめる(計画 9 節の M87 と M89 の行)。

Rust の実行時は、閉じた標準入出力(>&-)を起動のときに /dev/null で開き直す。
keleut-rt の out.rs が、.init_array の関数で起動の前に fcntl(1, F_GETFD) を呼んで記述子 1 が閉じていたかを記録するので、keleut はこの試験で 74 で終わる。

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

防壁が抑制した例外を記録する前の flush が、標準出力への最初の失敗になる試験である。
Diktor と同じく、記録を書かずに、書き込みの失敗の診断だけを出してすぐに 74 で終わる(IRSpec 9.6 節の 4)。
残りの cancel 節も走らせない。

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

cancel 節が、標準出力への書き込みの失敗の後に書き込んでから脱出する試験である。
失敗で捨てたバッファの後に書き足したものは、防壁の記録の前の flush で改めて書き出しを試み、また失敗する。
Diktor と同じく、記録を書かずに、書き込みの失敗の診断だけを出してすぐに 74 で終わる(IRSpec 9.7 節)。

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

標準出力が /dev/full のとき(ENOSPC)の試験である。
OCaml のチャネルは、ENOSPC では書けなかったバイトをバッファに残すので、防壁の記録の前の flush もまた失敗する。
Diktor と同じく、記録を書かずに、書き込みの失敗の診断だけを出してすぐに 74 で終わる(IRSpec 9.7 節)。
EBADF で失敗する 26 行目の場合とは違い、抑制の記録は出ない。

  $ keleut run sinkfail.kel > /dev/full
  keleut: 標準出力に書き出せません: No space left on device
  [74]
