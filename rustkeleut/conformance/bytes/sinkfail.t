閉じた標準出力への書き込みが cancel 節を走らせてから 74 で終わる試験である(計画 261024-1 の 6.5 節の出力。diktor/test/eval.t:820-848 を写したもの)。
ハンドラと cancel 節を使うので、M87 では確かめず、M89 の受け入れで確かめる(計画 9 節の M87 と M89 の行)。

Rust の実行時は、閉じた標準入出力(>&-)を起動のときに /dev/null で開き直す。
keleut がこの試験で 74 になるには、起動の前に記述子 1 が閉じていたことを知る手段が要る(M89 で決める)。

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
