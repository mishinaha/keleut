1 回の確保の上限 N_alloc_max = 2^26 を超える確保が、メモリの不足の実行時エラーになる試験である(計画 261024-1 の 6.5 節、IRSpec 11.6 節)。
メモリの不足はほかの実行時エラーと同じく巻き戻し、cancel 節を内側から走らせてから 3 で終わる。
長さを確保の前に上限と比べるので、機械の空きに依らず決定的に再現する。
記録は keleut run の結果である。
Diktor には 1 回の確保の上限が無く、この機械では確保に成功して 0 で終わるので、keleut run を Diktor で走らせる組(dk、dk-ir、kf、x-ir)の記録との差と、組 ki の基準の組 dk との差を、known-diffs.toml に理由「限度は I」で登録してある。

MutableArray.new に N_alloc_max + 1 の長さを渡す:

  $ cat > oom1.kel <<'EOF'
  > effect Res = { use: () => Unit }
  > let main(): Int32 = run h { let a = MutableArray.new(67108865, 0); MutableArray.length(a) } handle { case use() => resume(()) case cancel => echoln("cancel") }
  > echoln("before")
  > echoln(show(main()))
  > EOF
  $ keleut run oom1.kel
  before
  cancel
  実行時エラー: メモリ不足です
  [3]

文字列を作るプリミティブも、結果の長さが N_alloc_max を超えればメモリの不足になる。
2^27 バイトの文字列を作ろうとする:

  $ cat > oom2.kel <<'EOF'
  > effect Res = { use: () => Unit }
  > let rec grow(s: String, n: Int32): String = if n == 0 then s else grow(s + s, n - 1)
  > let main() = { let s = grow("a", 27); echoln("unreachable") } handle { case use() => resume(()) case cancel => echoln("cancel") }
  > echoln("before")
  > main()
  > EOF
  $ keleut run oom2.kel
  before
  cancel
  実行時エラー: メモリ不足です
  [3]
