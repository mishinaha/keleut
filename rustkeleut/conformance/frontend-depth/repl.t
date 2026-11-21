対話的な実行(REPL)で、フロントエンドの上限を確かめるテストである。
diktor/test/run_exit.t の対話的な実行のブロックは、OCAMLRUNPARAM で Diktor のスタックを絞るので、台帳の V213 の既知の差分として登録してある。
そのブロックが確かめる性質を、ここで keleut repl について確かめ直す。
keleut repl は、構文解析か型検査の上限を超えた入力を受理せず、続く入力を処理して終了コード 0 で終了する。

keleut repl の --native-stack は、フロントエンドのスレッドのスタックサイズを決める。
構文解析の深さの上限も、keleut check と同じく、そのスタックに収まる深さまで下がる。

2 万文のブロックは、ブロックを閉じる 1 回の還元で深さ 2 万の木になる。
4 MiB のスタックでは構文解析の上限を超えるので、その入力だけを受理しない。
下がった上限の値はデバッグビルドとリリースビルドで異なるので、数字を伏せて比較する:

  $ awk 'BEGIN { print "let y = 1"; s = "let x = {"; for (i = 0; i < 20000; i++) s = s " let a" i " = 1;"; print s " 1 }"; print "echoln(show(y))" }' > seqrepl.txt
  $ { keleut repl --native-stack 4m < seqrepl.txt; echo "exit: $?"; } | sed 's/限度 [0-9]* を/限度 N を/'
  y : Int32 = 1
  <stdin>: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 N を超えました)
  1
  exit: 0

既定のスタックでは、同じ入力を受理する:

  $ keleut repl < seqrepl.txt; echo "exit: $?"
  y : Int32 = 1
  x : Int32 = 1
  1
  exit: 0

--max-nest で上限を指定したときは、上限の値がビルドの種類によらない。
深さ 200 の式は上限 100 を超えて受理されず、その後の入力は処理される:

  $ awk 'BEGIN { print "let y = 1"; s = "1"; for (i = 0; i < 200; i++) s = "1 + (" s ")"; print "let x = " s; print "let z = 2"; print "echoln(show(y + z))" }' > nestrepl.txt
  $ keleut repl --max-nest 100 < nestrepl.txt; echo "exit: $?"
  y : Int32 = 1
  <stdin>: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 100 を超えました)
  z : Int32 = 2
  3
  exit: 0

run_exit.t にある 2 万段の 1 + (...) の式は、既定の上限で受理する:

  $ awk 'BEGIN { print "let y = 1"; s = "1"; for (i = 0; i < 20000; i++) s = "1 + (" s ")"; print "let x = " s; print "echoln(show(y))" }' > deeprepl.txt
  $ keleut repl < deeprepl.txt; echo "exit: $?"
  y : Int32 = 1
  x : Int32 = 20001
  1
  exit: 0

深さ 10 万の式は構文解析と型検査の上限に収まるが、KIR の入れ子の上限 N_nest(IRSpec 13.9 節)を超える。
keleut repl は各入力を KIR に変換して実行するので、この入力には処理系の限度の診断を出して受理せず、束縛 x は後の入力から参照できない。
diktor --repl は KIR を作らないので、同じ入力を受理する。
keleut run --frontend rust と diktor --emit-ir も、同じ上限で KIR の書き出しを中止する:

  $ awk 'BEGIN { print "let y = 1"; s = "1"; for (i = 0; i < 100000; i++) s = "1 + (" s ")"; print "let x = " s; print "echoln(show(y))"; print "x + y" }' > kirrepl.txt
  $ keleut repl < kirrepl.txt; echo "exit: $?"
  y : Int32 = 1
  <stdin>: 処理系の限度: KIR の書き出しで入れ子が深すぎます
  1
  ! <stdin>:4:1: 型エラー: 未束縛の変数: x
  exit: 0

型検査の上限に達するのは、推論した型が大きい入力である(typecheck.t と同じ形の入力を使う)。
f_i を、f_{i-1} を 2 回合成した関数とすると、f_16 の型は深さ 2^16 になる。
4 MiB のスタックでは型検査の上限を超え、その入力の束縛 r は後の入力から参照できない:

  $ awk 'BEGIN { print "let y = 1"; print "let r = {"; print "  let f0 = fn(x) => Some(x)"; for (i = 1; i <= 16; i++) print "  let f" i " = fn(x) => f" i-1 "(f" i-1 "(x))"; print "  0"; print "}"; print "echoln(show(y))"; print "r" }' > bigrepl.txt
  $ keleut repl --native-stack 4m < bigrepl.txt; echo "exit: $?"
  y : Int32 = 1
  ! 処理系の限度: 型検査で再帰が深すぎます(スタックオーバーフロー)
  1
  ! <stdin>:23:1: 型エラー: 未束縛の変数: r
  exit: 0

既定のスタックでは、同じ入力を受理する:

  $ keleut repl < bigrepl.txt; echo "exit: $?"
  y : Int32 = 1
  r : Int32 = 0
  1
  _ : Int32 = 0
  exit: 0

--native-stack が小さすぎて、スタックに収まる上限が下限に満たないときは、使い方のエラーになる:

  $ keleut repl --native-stack 300k < /dev/null > /dev/null 2>&1; echo "exit: $?"
  exit: 64
