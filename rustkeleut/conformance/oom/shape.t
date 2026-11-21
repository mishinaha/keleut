シェイプ表を拡張できないときに、レコードの拡張がメモリ不足の実行時エラーになることのテストである。
メモリ不足のときも、ほかの実行時エラーと同じく巻き戻しを行い、cancel 節を実行してから終了コード 3 で終了する。
期待出力は keleut run の結果である。

関数の中で、フィールドを 2 つ追加するレコードの拡張を 3,000 段つなぐ。
RustKeleut のシェイプ表は連鎖の長さの 2 乗に比例して大きくなるので、仮想メモリを 512 MiB に制限すると(ulimit -v 524288)、表を拡張できなくなる(台帳 V240)。
Diktor は、同じ制限をかけても終了コード 0 で終了し、before と 3000 を書く。
そこで、keleut run の呼び出しを Diktor で実行するペア(dk、dk-ir、kf、x-ir)と期待出力との差分と、RustKeleut の 12 ペアと基準のペア dk との差分を、known-diffs.toml に台帳 V240 で登録してある。

制限をかけた状態で .kel を読み込むと、フロントエンドが先にメモリを使い切る。
そこで、KIR は制限をかけずに書き出し、制限をかけた状態では .kir だけを実行する。
Node は仮想メモリを 512 MiB に制限すると起動できないので、制限をかけるコマンドには --backend interp を書く。
そのため、ペア kw と kr-kw でもインタプリタで実行される(kconform は `組の後段の上書き` の行で報告する)。
LLVM ペアでは shim がこのオプションを取り除いて JIT バックエンドを選ぶが、f はコンパイルのコストが JIT_COST_MAX を超えてインタプリタに残るので、レコードの拡張はインタプリタが行う(JIT コードが行う拡張は、crates/keleut-cli/tests/oom_cmd.rs の --tier jit-only のテストが確かめる)。

  $ cat > gen.awk <<'EOF'
  > BEGIN {
  >   n = 3000
  >   print "effect Res = { use: () => Unit }"
  >   print "let f(): Int32 = {"
  >   print "  let s0 = {a = 0}"
  >   for (i = 1; i <= n; i++) printf "  let s%d = {a = %d, b%d = 1 extends s%d}\n", i, i, i % 7, i - 1
  >   printf "  s%d.a\n}\n", n
  >   print "let main(): Int32 = run h { f() } handle { case use() => resume(()) case cancel => echoln(\"cancel\") }"
  >   print "echoln(\"before\")"
  >   print "echoln(show(main()))"
  > }
  > EOF
  $ awk -f gen.awk > chain.kel
  $ keleut emit-kir -o chain.kir chain.kel
  $ (ulimit -v 524288; keleut run --native-stack 64m --backend interp chain.kir)
  before
  cancel
  実行時エラー: メモリ不足です
  [3]
