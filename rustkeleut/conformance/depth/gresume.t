G の節(IRSpec 9.2 節の分類)の継続を重ねて再開したときの深さのテストである(IRSpec 11.3 節、11.4 節)。
期待出力は keleut run の結果である。

G の節の本体はハンドラの深さ h + 1 で実行され、非末尾の resume が再開するハンドラのフレームはその節と同じ深さで実行される。
生成器の yield のたびに非末尾の resume を重ねると、論理的な深さは yield 1 回ごとに 1 段ずつ深くなる。
既定の上限 L = 2^20 のとき、再開を L − 10 回重ねる呼び出しは値を返し、上限を超える回数の呼び出しは実行時エラーになって終了コード 3 で終了する。

  $ cat > chain.kel <<'EOF'
  > effect Gen = { yield: (Int32) => Unit }
  > let rec count(i: Int32, n: Int32): Unit @ {Gen} = if i == n then () else { perform yield(i); count(i + 1, n) }
  > let total(n: Int32): Int32 = count(0, n) handle { case yield(x) => { let r = resume(()); r + 1 } case return(u) => 0 }
  > echoln(show(total(1048566)))
  > echoln(show(total(1048576)))
  > EOF
  $ keleut run chain.kel
  1048566
  実行時エラー: 論理的な深さが限度 1048576 を超えました
  [3]

L − 20 段の再帰の底で G の操作を起こす。
継続は L − 20 段のフレームを持ち、再開した perform の深さ c + Δ は上限に収まる。

  $ cat > dive.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec dive(k: Int32): Int32 @ {Ask} = if k == 0 then perform ask() else 1 + dive(k - 1)
  > let g(k: Int32): Int32 = dive(k) handle { case ask() => { let r = resume(7); r + 1 } }
  > echoln(show(g(1048556)))
  > EOF
  $ keleut run dive.kel
  1048564
