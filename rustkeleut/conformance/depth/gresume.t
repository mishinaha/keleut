G の節の継続の再開を重ねる深さの試験である(IRSpec 11.3 節と 11.4 節、計画 261024-1 の 4.10 節と 6.10 節)。
記録は keleut run の結果である。

G の節の本体はハンドラの深さ h + 1 で走り、末尾でない resume が再開するハンドラのフレームはその節と同じ深さで走る。
生成器の yield ごとに末尾でない resume を重ねると、論理的な深さは 1 回の yield ごとに 1 段ずつ深くなる。
既定の限度 L = 2^20 の下で、L − 10 回の再開を重ねたものは 0 で終わり、限度を超える回数では 3 で終わる。

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
継続は L − 20 段のフレームを持ち、再開した perform の深さ c + Δ は限度に収まる。

  $ cat > dive.kel <<'EOF'
  > effect Ask = { ask: () => Int32 }
  > let rec dive(k: Int32): Int32 @ {Ask} = if k == 0 then perform ask() else 1 + dive(k - 1)
  > let g(k: Int32): Int32 = dive(k) handle { case ask() => { let r = resume(7); r + 1 } }
  > echoln(show(g(1048556)))
  > EOF
  $ keleut run dive.kel
  1048564
