型検査の限度の試験である(計画 261024-1 の 6.11 節、8.1 節、V213)。
diktor/test/run_exit.t の型検査の限度のブロックは OCAMLRUNPARAM で Diktor のスタックを絞るので、
kconform は除外し、同じ意図の試験をここに作り直す。
keleut check は Diktor の --type-check と同じ出力と終了状態を返す。

keleut check は、再帰する関数の入口でスタックの残りを調べ、足りなければ 1 で終わる。
--native-stack は型検査のスレッドのスタックの大きさを決め、構文解析の深さの限度をそのスタックに収まる深さまで下げる。
そのため、深い式は構文解析の限度で 2 になり、型検査の限度に届くのは推論した型が大きい入力である。
f_i を f_{i-1} を 2 回合成した関数とすると、f_16 の型は深さ 2^16 になる。

  $ awk 'BEGIN { print "echoln(\"before\")"; print "newtype B[A] = MkB(A)"; print "let r = {"; print "  let f0 = fn(x) => MkB(x)"; for (i = 1; i <= 16; i++) print "  let f" i " = fn(x) => f" i-1 "(f" i-1 "(x))"; print "  0"; print "}"; print "echoln(show(r))" }' > big.kel

4 MiB のスタックでは型検査の限度を超え、1 で終わる。
標準出力には、それまでの型の行に続けて限度の診断を出す:

  $ keleut check --native-stack 4m big.kel 2> /dev/null; echo "exit: $?"
  _ : {}
  ! 処理系の限度: 型検査で再帰が深すぎます(スタックオーバーフロー)
  exit: 1

--quiet(Diktor の実行の前段の型検査と同じ形)では、型の行を出さず、診断を標準エラーに出す:

  $ keleut check --quiet --native-stack 4m big.kel 2> /dev/null; echo "exit: $?"
  exit: 1
  $ keleut check --quiet --native-stack 4m big.kel 2>&1 > /dev/null; echo "exit: $?"
  ! 処理系の限度: 型検査で再帰が深すぎます(スタックオーバーフロー)
  exit: 1

既定のスタックでは、同じ入力は型検査に通る:

  $ keleut check big.kel; echo "exit: $?"
  _ : {}
  r : Int32
  _ : {}
  exit: 0

run_exit.t の 2 万段の 1 + (...) の式は、既定の限度で型検査に通り、型の行を出す:

  $ awk 'BEGIN { print "echoln(\"before\")"; s = "1"; for (i = 0; i < 20000; i++) s = "1 + (" s ")"; print "let x = " s; print "echoln(show(x))" }' > deep.kel
  $ keleut check deep.kel; echo "exit: $?"
  _ : {}
  x : Int32
  _ : {}
  exit: 0

同じ式に --native-stack 4m を与えると、構文解析の深さの限度が下がり、構文解析の限度の診断で 2 になる。
下げた限度の値は構築の種類で変わるので、数字を伏せて比べる:

  $ keleut check --native-stack 4m deep.kel 2>&1 | sed 's/限度 [0-9]* を/限度 N を/'
  deep.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 N を超えました)
  $ keleut check --native-stack 4m deep.kel 2> /dev/null; echo "exit: $?"
  exit: 2

ブロックは文を let の鎖に脱糖するので、2 万文のブロックは、ブロックを閉じる 1 回の還元で深さ 2 万の木になる。
小さいスタックでは、限度を超えたその木を捨てても落ちず、構文解析の限度の診断で 2 になる。
既定のスタックでは型検査に通る:

  $ awk 'BEGIN { print "let r = {"; for (i = 0; i < 20000; i++) print "  let x" i " = " i; print "  0"; print "}" }' > block.kel
  $ keleut check --native-stack 4m block.kel 2>&1 | sed 's/限度 [0-9]* を/限度 N を/'
  block.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 N を超えました)
  $ keleut check --native-stack 4m block.kel 2> /dev/null; echo "exit: $?"
  exit: 2
  $ keleut check block.kel; echo "exit: $?"
  r : Int32
  exit: 0

下げた限度の内側の平らなコードは、小さいスタックでも型検査に通る。
40 項の演算子の鎖と 40 段の else if の鎖である:

  $ awk 'BEGIN { s = "1"; for (i = 1; i < 40; i++) s = s " + 1"; print "let x = " s; s = "0"; for (i = 0; i < 40; i++) s = s " else if n == " i " then " i; sub(/^0 else /, "", s); print "let f(n: Int32): Int32 = " s " else 0" }' > flat.kel
  $ keleut check --native-stack 4m flat.kel; echo "exit: $?"
  x : Int32
  f : (Int32) => Int32
  exit: 0

既定のスタックでは、5 万段の else if の鎖も型検査に通る:

  $ awk 'BEGIN { s = ""; for (i = 0; i < 50000; i++) s = s "if n == " i " then " i " else "; print "let f(n: Int32): Int32 = " s "0" }' > elif.kel
  $ keleut check elif.kel; echo "exit: $?"
  f : (Int32) => Int32
  exit: 0

プレリュードの深さまで限度を下げられない小さいスタックは、使い方の誤りで 64 になる:

  $ keleut check --native-stack 300k flat.kel 2>&1 | head -n 1
  keleut: --native-stack is too small
  $ keleut check --native-stack 300k flat.kel 2> /dev/null; echo "exit: $?"
  exit: 64

import の鎖をたどる段は再帰しないので、長い鎖も小さいスタックで型検査に通る。
f0 が f1 を、f1 が f2 を、… と検索パスで import する 3,000 段の鎖である:

  $ mkdir chain
  $ awk 'BEGIN { n = 3000; for (i = 0; i < n; i++) { f = "chain/f" i ".kel"; if (i + 1 < n) { print "from \"f" i+1 "\" import v" i+1 > f; print "pub let v" i ": Int32 = v" i+1 " + 1" > f } else print "pub let v" i ": Int32 = 0" > f; close(f) } }'
  $ printf 'from "f0" import v0\nlet r = v0\n' > chain.kel
  $ keleut check --native-stack 4m --import-path chain chain.kel; echo "exit: $?"
  r : Int32
  exit: 0

構文解析の限度は作る木の深さで数える。
関数の適用はソースの入れ子 1 段で木が約 2 段深くなるので、5 万段の id(...) は型検査に通り、10 万段は構文解析の限度で 2 になる。
Diktor は 10 万段も型検査に通す(0 で終わる)。
この差は、計画から外れたこととして実施記録の M97 の節に書いてある:

  $ awk 'BEGIN { print "let id(x: Int32): Int32 = x"; s = "1"; for (i = 0; i < 50000; i++) s = "id(" s ")"; print "let x = " s }' > ids5.kel
  $ keleut check ids5.kel; echo "exit: $?"
  id : (Int32) => Int32
  x : Int32
  exit: 0
  $ awk 'BEGIN { print "let id(x: Int32): Int32 = x"; s = "1"; for (i = 0; i < 100000; i++) s = "id(" s ")"; print "let x = " s }' > ids10.kel
  $ keleut check ids10.kel; echo "exit: $?"
  ids10.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 200000 を超えました)
  exit: 2
