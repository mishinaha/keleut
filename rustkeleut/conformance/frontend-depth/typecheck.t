型検査の上限のテストである。
diktor/test/run_exit.t の型検査の上限のブロックは、OCAMLRUNPARAM で Diktor のスタックを絞るので、台帳の V213 の既知の差分として登録してある。
そのブロックが確かめる性質を、ここで keleut check について確かめ直す。
keleut check は、Diktor の --type-check と同じ出力と終了コードを返す。

keleut check は、再帰する関数の入口でスタックの残りを調べ、足りなければ終了コード 1 で終了する。
--native-stack は型検査のスレッドのスタックサイズを決め、構文解析の深さの上限をそのスタックに収まる深さまで下げる。
そのため、深い式は構文解析の上限を超えて終了コード 2 になり、型検査の上限に達するのは、推論した型が大きい入力である。
f_i を、f_{i-1} を 2 回合成した関数とすると、f_16 の型は深さ 2^16 になる。

  $ awk 'BEGIN { print "echoln(\"before\")"; print "newtype B[A] = MkB(A)"; print "let r = {"; print "  let f0 = fn(x) => MkB(x)"; for (i = 1; i <= 16; i++) print "  let f" i " = fn(x) => f" i-1 "(f" i-1 "(x))"; print "  0"; print "}"; print "echoln(show(r))" }' > big.kel

4 MiB のスタックでは型検査の上限を超え、終了コード 1 で終了する。
標準出力には、それまでに検査した束縛の型を表示する行に続けて、上限の診断を出力する:

  $ keleut check --native-stack 4m big.kel 2> /dev/null; echo "exit: $?"
  _ : {}
  ! 処理系の限度: 型検査で再帰が深すぎます(スタックオーバーフロー)
  exit: 1

--quiet(Diktor が実行の前に行う型検査と同じ形式)では、型を表示する行を出力せず、診断を標準エラーに出力する:

  $ keleut check --quiet --native-stack 4m big.kel 2> /dev/null; echo "exit: $?"
  exit: 1
  $ keleut check --quiet --native-stack 4m big.kel 2>&1 > /dev/null; echo "exit: $?"
  ! 処理系の限度: 型検査で再帰が深すぎます(スタックオーバーフロー)
  exit: 1

既定のスタックでは、同じ入力が型検査に通る:

  $ keleut check big.kel; echo "exit: $?"
  _ : {}
  r : Int32
  _ : {}
  exit: 0

run_exit.t にある 2 万段の 1 + (...) の式は、既定の上限で型検査に通り、型を表示する行が出力される:

  $ awk 'BEGIN { print "echoln(\"before\")"; s = "1"; for (i = 0; i < 20000; i++) s = "1 + (" s ")"; print "let x = " s; print "echoln(show(x))" }' > deep.kel
  $ keleut check deep.kel; echo "exit: $?"
  _ : {}
  x : Int32
  _ : {}
  exit: 0

同じ式に --native-stack 4m を指定すると、構文解析の深さの上限が下がり、構文解析の上限の診断を出して終了コード 2 で終了する。
下がった上限の値はデバッグビルドとリリースビルドで異なるので、数字を伏せて比較する:

  $ keleut check --native-stack 4m deep.kel 2>&1 | sed 's/限度 [0-9]* を/限度 N を/'
  deep.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 N を超えました)
  $ keleut check --native-stack 4m deep.kel 2> /dev/null; echo "exit: $?"
  exit: 2

構文解析はブロックの文の並びを let の連鎖に脱糖するので、2 万文のブロックは、ブロックを閉じる 1 回の還元で深さ 2 万の木になる。
小さいスタックでは、上限を超えたその木を破棄するときにも異常終了せず、構文解析の上限の診断を出して終了コード 2 で終了する。
既定のスタックでは型検査に通る:

  $ awk 'BEGIN { print "let r = {"; for (i = 0; i < 20000; i++) print "  let x" i " = " i; print "  0"; print "}" }' > block.kel
  $ keleut check --native-stack 4m block.kel 2>&1 | sed 's/限度 [0-9]* を/限度 N を/'
  block.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 N を超えました)
  $ keleut check --native-stack 4m block.kel 2> /dev/null; echo "exit: $?"
  exit: 2
  $ keleut check block.kel; echo "exit: $?"
  r : Int32
  exit: 0

下がった上限に収まる、入れ子の浅いコードは、小さいスタックでも型検査に通る。
40 項の演算子の連鎖と、40 段の else if の連鎖で確かめる:

  $ awk 'BEGIN { s = "1"; for (i = 1; i < 40; i++) s = s " + 1"; print "let x = " s; s = "0"; for (i = 0; i < 40; i++) s = s " else if n == " i " then " i; sub(/^0 else /, "", s); print "let f(n: Int32): Int32 = " s " else 0" }' > flat.kel
  $ keleut check --native-stack 4m flat.kel; echo "exit: $?"
  x : Int32
  f : (Int32) => Int32
  exit: 0

既定のスタックでは、5 万段の else if の連鎖も型検査に通る:

  $ awk 'BEGIN { s = ""; for (i = 0; i < 50000; i++) s = s "if n == " i " then " i " else "; print "let f(n: Int32): Int32 = " s "0" }' > elif.kel
  $ keleut check elif.kel; echo "exit: $?"
  f : (Int32) => Int32
  exit: 0

上限をプレリュードの深さまで下げられないほど小さいスタックを指定すると、使い方のエラーになり、終了コード 64 で終了する:

  $ keleut check --native-stack 300k flat.kel 2>&1 | head -n 1
  keleut: --native-stack is too small
  $ keleut check --native-stack 300k flat.kel 2> /dev/null; echo "exit: $?"
  exit: 64

import の連鎖をたどる処理は再帰しないので、長い連鎖も小さいスタックで型検査に通る。
f0 が f1 を、f1 が f2 を、というように検索パスから import する 3,000 段の連鎖で確かめる:

  $ mkdir chain
  $ awk 'BEGIN { n = 3000; for (i = 0; i < n; i++) { f = "chain/f" i ".kel"; if (i + 1 < n) { print "from \"f" i+1 "\" import v" i+1 > f; print "pub let v" i ": Int32 = v" i+1 " + 1" > f } else print "pub let v" i ": Int32 = 0" > f; close(f) } }'
  $ printf 'from "f0" import v0\nlet r = v0\n' > chain.kel
  $ keleut check --native-stack 4m --import-path chain chain.kel; echo "exit: $?"
  r : Int32
  exit: 0

構文解析の上限は、作る木の深さで数える。
関数適用は、ソースの入れ子 1 段につき木が約 2 段深くなるので、5 万段の id(...) は型検査に通り、10 万段は構文解析の上限を超えて終了コード 2 で終了する。
Diktor は 10 万段の式も型検査に通す(終了コード 0 で終了する)。
この差分は、読み込みに必要なメモリが処理系の限度を超えた場合(LangSpec §15.5)にあたり、上限の大きさは処理系ごとに違ってよい:

  $ awk 'BEGIN { print "let id(x: Int32): Int32 = x"; s = "1"; for (i = 0; i < 50000; i++) s = "id(" s ")"; print "let x = " s }' > ids5.kel
  $ keleut check ids5.kel; echo "exit: $?"
  id : (Int32) => Int32
  x : Int32
  exit: 0
  $ awk 'BEGIN { print "let id(x: Int32): Int32 = x"; s = "1"; for (i = 0; i < 100000; i++) s = "id(" s ")"; print "let x = " s }' > ids10.kel
  $ keleut check ids10.kel; echo "exit: $?"
  ids10.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 200000 を超えました)
  exit: 2

else if の連鎖も、ソースの入れ子 1 段につき木が 2 段深くなる。
5 万段の連鎖は上の elif.kel のとおり型検査に通り、10 万段の連鎖は構文解析の上限を超えて終了コード 2 で終了する。
Diktor は 10 万段の連鎖も型検査に通す(終了コード 0 で終了する)。
この差分も、読み込みに必要なメモリが処理系の限度を超えた場合(LangSpec §15.5)にあたる:

  $ awk 'BEGIN { printf "let f(n: Int32): Int32 = "; for (i = 0; i < 100000; i++) printf "if n == %d then %d else ", i, i; print "0" }' > elif10.kel
  $ keleut check elif10.kel; echo "exit: $?"
  elif10.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 200000 を超えました)
  exit: 2
