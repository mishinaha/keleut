KIR 1.0 のフィクスチャである(IRSpec 14 節)。
どのフィクスチャも、読み込みと検証に通る。

  $ for f in *.kir *.kirt; do keleut kir verify "$f" || echo "拒否: $f"; done

同じ名前の .kir と .kirt は、同じプログラムの正準なバイナリと正準なテキストである。
バイナリを keleut kir dis でテキストに変換すると .kirt に一致し、テキストを keleut kir asm でバイナリに変換すると .kir に一致する。

  $ for t in *.kirt; do b="${t%t}"; keleut kir dis "$b" | cmp -s - "$t" || echo "dis が違う: $b"; keleut kir asm "$t" -o asm.out && cmp -s asm.out "$b" || echo "asm が違う: $t"; done

loop-noncanonical.kir は、loop.kir のセクションのサイズをすべて 5 バイトの LEB128 で書き、スキップできるセクション 0x80、0x81、0x82 を追加した、正準でないバイナリである。
リーダは、これを loop.kir と同じプログラムとして読む。

  $ keleut kir dis loop-noncanonical.kir | cmp - loop.kirt && echo same
  same

各フィクスチャを実行する。
ペア dk、dk-ir、kf、x-ir では、shim がこの呼び出しを diktor --run-ir に渡すので、Diktor のリーダも同じフィクスチャを読む。

  $ keleut run closure.kir; echo "exit: $?"
  exit: 0
  $ keleut run dict.kir; echo "exit: $?"
  exit: 0
  $ keleut run handle.kir; echo "exit: $?"
  exit: 0
  $ keleut run loop.kir; echo "exit: $?"
  exit: 0
  $ keleut run loop-noncanonical.kir; echo "exit: $?"
  exit: 0
  $ keleut run import.kir; echo "exit: $?"
  init leaf
  init left
  init right
  init main
  23
  exit: 0
  $ keleut run never-ops.kir; echo "exit: $?"
  ok
  exit: 0
  $ keleut run ops.kir; echo "exit: $?"
  false
  3
  実行時エラー: 数値リテラルが範囲外です: 99999999999
  exit: 3
  $ keleut run sample.kir; echo "exit: $?"
  test
  test
  exit: 0
  $ keleut run strings.kir; echo "exit: $?"
  19
  false
  true
  -0.0025false
  9223372036854775807-9223372036854775808
  1x
  exit: 0

リーダが調べるのはメジャーバージョンだけで、マイナーバージョンがいくつでも読む。
次のコマンドは、strings.kir のマイナーバージョンを 1 に、セクション数を 12 にして、末尾に内容が 3 バイトのスキップできるセクション 0x83 を追加したファイルを作る。
1.0 のリーダは、これを strings.kir と同じように実行する。

  $ { head -c 6 strings.kir; printf '\001\000\000\000\000\000\014\000\000\000'; tail -c +17 strings.kir; printf '\203\003abc'; } > minor.kir && keleut run minor.kir; echo "exit: $?"
  19
  false
  true
  -0.0025false
  9223372036854775807-9223372036854775808
  1x
  exit: 0
