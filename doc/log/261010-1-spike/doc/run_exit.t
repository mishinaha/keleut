実行の開始と終了(LangSpec §15.5)。終了状態、診断と警告の出力先、実行時エラーの
前に書いた標準出力の扱いを固定する。診断の文面そのものは各機能のテストが持つ。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

正常終了は 0。起点のトップレベルを上から評価し終えたら終わる:

  $ cat > exitok.kel <<'EOF'
  > echoln("hello")
  > let x = 1 + 2
  > echoln(show(x))
  > EOF
  $ diktor exitok.kel; echo "exit: $?"

型検査で拒否したプログラムは 1。初期化を始めないので、型エラーより前の式文も
評価しない。診断は標準エラーに出る:

  $ cat > exittype.kel <<'EOF'
  > echoln("before")
  > let x: Int32 = "s"
  > EOF
  $ diktor exittype.kel 2> /dev/null; echo "exit: $?"
  $ diktor exittype.kel > /dev/null
  [1]

import の誤りも 1:

  $ printf 'from "./nosuch" import x\necholn("a")\n' > exitimport.kel
  $ diktor exitimport.kel 2> /dev/null; echo "exit: $?"

構文エラー、字句エラー、不正な UTF-8 は 2:

  $ printf 'echoln("before")\nlet x = (1 +\n' > exitparse.kel
  $ diktor exitparse.kel 2> /dev/null; echo "exit: $?"
  $ printf 'echoln("before")\nlet x = "\\q"\n' > exitlex.kel
  $ diktor exitlex.kel 2> /dev/null; echo "exit: $?"
  $ printf 'echoln("before")\nlet x = "\377"\n' > exitutf8.kel
  $ diktor exitutf8.kel 2> /dev/null; echo "exit: $?"

実行時エラーは 3。止まるまでに標準出力へ書いた内容は残り、診断は標準エラーに出る:

  $ cat > exitrt.kel <<'EOF'
  > echo("before ")
  > echoln("line")
  > let z = 0
  > echoln(show(1 / z))
  > echoln("after")
  > EOF
  $ diktor exitrt.kel 2> /dev/null; echo "exit: $?"
  $ diktor exitrt.kel > /dev/null
  [3]

標準出力と標準エラーを 1 本にまとめると、診断はそれまでの出力の後ろに出る。
cancel 節は実行時エラーの巻き戻しでも走る(§13.6):

  $ cat > exitcancel.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let z = 0
  > echoln("start")
  > let r = (perform ask() / z) handle {
  >   case ask() => resume(1)
  >   case cancel => echoln("cancel ran")
  > }
  > echoln("after")
  > EOF
  $ diktor exitcancel.kel 2>&1; echo "exit: $?"

警告は標準エラーに、初期化の前に出る。警告があっても実行する:

  $ cat > warnrun.kel <<'EOF'
  > echoln("before")
  > let f(n: Int32): String = n match { case 0 => "zero" }
  > echoln(f(0))
  > EOF
  $ diktor warnrun.kel 2> /dev/null; echo "exit: $?"
  $ diktor warnrun.kel 2>&1 > /dev/null
  $ diktor warnrun.kel 2>&1

警告を誤りとして扱う旗では、実行せずに 1 で終わる:

  $ diktor --strict-exhaustive warnrun.kel 2> /dev/null; echo "exit: $?"

初期化前の呼び出し(§6.4)。後ろで宣言する関数は完全な署名があれば型検査に通るが、
初期化の前に呼ぶと実行時エラーになる。インスタンスも同じ:

  $ cat > initcall.kel <<'EOF'
  > let f(): Int32 = g()
  > echoln("before")
  > echoln(show(f()))
  > let g(): Int32 @ {} = 42
  > EOF
  $ diktor initcall.kel 2> /dev/null; echo "exit: $?"
  $ cat > initinst.kel <<'EOF'
  > newtype L(String)
  > type class R[A] { val r: (A) => String }
  > let use(x: L): String = r(x)
  > echoln("before")
  > echoln(use(L("a")))
  > type instance R[L] { let r(x) = x match { case L(s) => s } }
  > EOF
  $ diktor initinst.kel 2> /dev/null; echo "exit: $?"
