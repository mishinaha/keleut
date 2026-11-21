1 回の確保の上限 N_alloc_max = 2^26 を超える確保が、メモリ不足の実行時エラーになることのテストである(IRSpec 11.6 節)。
メモリ不足のときも、ほかの実行時エラーと同じく巻き戻しを行い、cancel 節を内側から順に実行してから終了コード 3 で終了する。
確保の前に長さを上限と比較するので、この実行時エラーはマシンの空きメモリによらず決定的に起きる。
期待出力は keleut run の結果である。
Diktor には 1 回の確保の上限がなく、テストに使うマシンでは確保に成功して終了コード 0 で終了するので、keleut run の呼び出しを Diktor で実行するペア(dk、dk-ir、kf、x-ir)と期待出力との差分と、RustKeleut の 12 ペアと基準のペア dk との差分を、known-diffs.toml に台帳 V215 で登録してある(理由は `上限は I` で始まる)。

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

文字列を作るプリミティブも、結果の長さが N_alloc_max を超えればメモリ不足の実行時エラーになる。
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
