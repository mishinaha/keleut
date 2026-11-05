形の表を広げられないときに、拡張がメモリの不足の実行時エラーになる試験である(計画 261104-3 の 4.3 節、D569)。
メモリの不足はほかの実行時エラーと同じく巻き戻し、cancel 節を走らせてから 3 で終わる。
記録は keleut run の結果である。

関数の中で、2 つのフィールドを足す拡張を 3,000 段つなぐ。
RustKeleut の形の表は鎖の長さの 2 乗で大きくなるので、仮想メモリの上限 512 MiB の下で表を広げられなくなる(台帳 V240)。
Diktor はこの上限の下でも 0 で終わる(before と 3000)。
そこで、keleut run を Diktor で走らせる組(dk、dk-ir、kf、x-ir)の記録との差と、RustKeleut の 12 組の基準の組 dk との差を、known-diffs.toml に台帳 V240 で登録してある。

.kel を上限の下で読ませると、フロントエンドが先にメモリを使い切る。
そこで KIR は上限を置かずに書き、上限の下では .kir だけを走らせる。
Node は仮想メモリの上限 512 MiB の下では起動できないので、上限を置くコマンドは --backend interp を書く。
組 kw と kr-kw でもインタプリタで走る(kconform は「組の後段の上書き」として報告する)。
LLVM の後段の組では shim がこの旗を除いて JIT の後段を選ぶが、f は翻訳の費用が JIT_COST_MAX を超えてインタプリタに残るので、拡張はインタプリタで行う(JIT の拡張は tests/oom_cmd.rs の jit-only の例が確かめる)。

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
