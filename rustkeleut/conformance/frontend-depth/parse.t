構文解析の上限のテストである。
diktor/test/run_exit.t の構文解析の上限のブロックは、OCAMLRUNPARAM で Diktor のスタックを絞るので、台帳の V213 の既知の差分として登録してある。
そのブロックが確かめる性質を、ここで RustKeleut の構文解析について確かめ直す。
keleut dump ast は、RustKeleut の字句解析と構文解析だけを実行する。

構文解析の上限は、作る木の深さで数え、既定では 200000 である。
構文解析のスタックはヒープ上の配列なので、スタックの段数は数えない。
入れ子にならない長い並びは、Diktor と同じく受理する。
たとえば、10 万個のトップレベルの宣言を受理する:

  $ awk 'BEGIN { for (i = 0; i < 100000; i++) print "let a" i " = " i; print "echoln(show(a99999))" }' > top.kel
  $ keleut dump ast top.kel > top.out; echo "exit: $?"; wc -l < top.out
  exit: 0
  100001

構文解析はブロックの文の並びを let の連鎖に脱糖するので、12 万文のブロックは深さ 12 万の木になる。
この深さは上限に収まるので、受理する:

  $ awk 'BEGIN { print "echoln(\"before\")"; print "let x = {"; for (i = 0; i < 120000; i++) print "  let a" i " = 1"; print "  1"; print "}"; print "echoln(show(x))" }' > seq.kel
  $ keleut dump ast seq.kel > seq.out; echo "exit: $?"; wc -l < seq.out
  exit: 0
  478892

同じファイルのトークン列も出力できる:

  $ keleut dump tokens seq.kel > seq.tok; echo "exit: $?"; tail -2 seq.tok
  exit: 0
  120005  )
  120006  <EOF>

括弧は木のノードを作らないので、25 万段の括弧も受理する:

  $ awk 'BEGIN { s = "1"; for (i = 0; i < 250000; i++) s = "(" s ")"; print "let x = " s }' > paren.kel
  $ keleut dump ast paren.kel; echo "exit: $?"
  (dlet (binding x = 1))
  exit: 0

run_exit.t にある 2 万段の 1 + (...) の式も受理する:

  $ awk 'BEGIN { s = "1"; for (i = 0; i < 20000; i++) s = "1 + (" s ")"; print "let x = " s }' > deep.kel
  $ keleut dump ast deep.kel > deep.out; echo "exit: $?"; wc -l < deep.out
  exit: 0
  20002

木の深さが上限を超えると、終了コード 2 で終了する。
診断は標準エラーに出力し、標準出力には何も出力しない。
30 万項の和は、左再帰なので構文解析のスタックは深くならないが、木の深さが上限を超える:

  $ awk 'BEGIN { s = "1"; for (i = 0; i < 300000; i++) s = s "+1"; print "let x = " s }' > plus.kel
  $ keleut dump ast plus.kel 2> /dev/null; echo "exit: $?"
  exit: 2
  $ keleut dump ast plus.kel > /dev/null; echo "exit: $?"
  plus.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 200000 を超えました)
  exit: 2

タプルの要素の選択 t._N は N 段の木になるので、N が上限を超えれば同じ診断になる:

  $ echo 'let x = t._300000' > strip.kel
  $ keleut dump ast strip.kel; echo "exit: $?"
  strip.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 200000 を超えました)
  exit: 2

--max-nest で上限を変えられる。
上限を超えたファイルより前に並べたファイルの出力は、診断より先に出る:

  $ printf 'let a = 1\n' > ok.kel
  $ awk 'BEGIN { print "let x = {"; for (i = 0; i < 1200; i++) print "  let a" i " = 1"; print "  1"; print "}" }' > seq1200.kel
  $ keleut dump ast --max-nest 1000 ok.kel seq1200.kel; echo "exit: $?"
  (dlet (binding a = 1))
  seq1200.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 1000 を超えました)
  exit: 2
  $ keleut dump ast --max-nest 2000 seq1200.kel > seq1200.out; echo "exit: $?"; wc -l < seq1200.out
  exit: 0
  3690

上限に必要なスタックが 8 GiB を超える値を指定すると、使い方のエラーになり、終了コード 64 で終了する:

  $ keleut dump ast --max-nest 1000000000000 ok.kel 2> err; echo "exit: $?"; head -1 err
  exit: 64
  keleut: bad --max-nest value: 1000000000000
