標準出力のバイト列と出力の規約の試験である(計画 261024-1 の 6.5 節の出力、8.3 節、D530)。
文字列はバイト列で、Console.write は文字列のバイトをそのまま書く。

NUL と、文字の途中で切った不正な UTF-8 のバイト列もそのまま書く(期待は Diktor と同じ):

  $ cat > nul.kel <<'EOF'
  > echo("a\u0000b\U0001F600")
  > echo(__string_sub("あ", 0, 1))
  > echo(__string_sub("あ", 1, 2))
  > echo("\n")
  > EOF
  $ keleut run nul.kel | od -An -tx1
   61 00 62 f0 9f 98 80 e3 81 82 0a

1 MiB を超える文字列を、バッファの大きさ(64 KiB)をまたいで書く。
読み手が 1 秒遅れて読み始めるパイプでも、バイト列は欠けない(期待は Diktor と同じ):

  $ cat > big.kel <<'EOF'
  > let rec dbl(s: String, n: Int32): String = if n == 0 then s else dbl(s + s, n - 1)
  > let s = dbl("0123456789abcdef", 17)
  > echoln(show(__string_length(s)))
  > echo(s)
  > echo(s)
  > echoln("")
  > EOF
  $ keleut run big.kel | wc -c
  4194313
  $ keleut run big.kel | (sleep 1; cat) | cksum
  3609452285 4194313

非ブロッキングのパイプでは、書き込みが EAGAIN で返る。
keleut は待ってから書き直すので、バイト列は欠けず、終了状態は 0 である。
Diktor は Sys_blocked_io の例外で止まる(この試験は Diktor と一致しない):

  $ cat > nb.py <<'EOF'
  > import fcntl, os, subprocess, sys, time
  > r, w = os.pipe()
  > fcntl.fcntl(w, fcntl.F_SETFL, fcntl.fcntl(w, fcntl.F_GETFL) | os.O_NONBLOCK)
  > p = subprocess.Popen(sys.argv[1:], stdout=w)
  > os.close(w)
  > time.sleep(1)
  > n = 0
  > while True:
  >     b = os.read(r, 65536)
  >     if not b:
  >         break
  >     n += len(b)
  > print(p.wait(), n)
  > EOF
  $ python3 nb.py keleut run big.kel
  0 4194313

書けない標準出力(/dev/full)では、最初の書き込みの失敗で 74 になる。
小さな出力は終了の前の flush で失敗する(期待は Diktor と同じ。診断の名前だけが違う):

  $ printf 'echoln("hello")\n' > hello.kel
  $ keleut run hello.kel > /dev/full
  keleut: 標準出力に書き出せません: No space left on device
  [74]

バッファが満ちた時点で失敗すれば、プログラムはそこで止まる:

  $ keleut run big.kel > /dev/full
  keleut: 標準出力に書き出せません: No space left on device
  [74]

閉じた標準出力(>&-)でも、最初の書き込みの失敗で 74 になる。
小さな出力は終了の前の flush で、大きな出力はバッファが満ちた時点で失敗する(期待は Diktor と同じ。診断の名前だけが違う):

  $ keleut run hello.kel >&-
  keleut: 標準出力に書き出せません: Bad file descriptor
  [74]
  $ keleut run big.kel >&-
  keleut: 標準出力に書き出せません: Bad file descriptor
  [74]

何も書かないプログラムは、閉じた標準出力でも 0 で終わる(Diktor と同じ):

  $ printf 'let x = 1\n' > quiet.kel
  $ keleut run quiet.kel >&-

実行時エラーで止まるときは、flush の失敗を捨てて、実行時エラーの終了状態 3 で終わる(Diktor と同じ):

  $ printf 'echoln("hello")\necholn(show(1 / 0))\n' > m74.kel
  $ keleut run m74.kel > /dev/full
  実行時エラー: ゼロ除算です
  [3]

標準エラーにも書けないときは、診断を捨てる:

  $ keleut run m74.kel > /dev/null 2> /dev/full
  [3]

書き出しの失敗の種類に依らず、OS の誤りの文を診断に書いて 74 になる。
つながっていない AF_INET の SOCK_DGRAM のソケットと、つながっていない AF_UNIX の SOCK_STREAM のソケットを標準出力にする(期待は Diktor と同じ。診断の名前だけが違う):

  $ cat > sock.py <<'EOF'
  > import socket, subprocess, sys
  > kinds = {"inet-dgram": (socket.AF_INET, socket.SOCK_DGRAM), "unix-stream": (socket.AF_UNIX, socket.SOCK_STREAM)}
  > s = socket.socket(*kinds[sys.argv[1]])
  > print(subprocess.run(sys.argv[2:], stdout=s.fileno()).returncode)
  > EOF
  $ python3 sock.py inet-dgram keleut run hello.kel
  keleut: 標準出力に書き出せません: Destination address required
  74
  $ python3 sock.py unix-stream keleut run hello.kel
  keleut: 標準出力に書き出せません: Transport endpoint is not connected
  74
