回収(計画 261024-1 の 6.5 節、D526)の試験である。
記録は keleut run の結果で、Diktor の木の評価器の結果とも同じである。

100 万要素のリストを末尾呼び出しで作り、末尾の再帰と末尾でない再帰で走査する。
印付けは明示の列で反復するので、ネイティブのスタックが 512 KiB でも落ちない。
末尾でない len は値スタックに 100 万段のフレームを積み、回収はそのすべてを根として走査する。

  $ cat > list.kel <<'EOF'
  > let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
  > let rec sum(xs: List[Int32], acc: Int64): Int64 = xs match { case Nil => acc case Cons(x, t) => sum(t, acc + __i32_to_i64(x)) }
  > let rec len(xs: List[Int32]): Int32 = xs match { case Nil => 0 case Cons(_, t) => 1 + len(t) }
  > let xs = build(1000000, Nil)
  > echoln(show(sum(xs, __i32_to_i64(0))))
  > echoln(show(len(xs)))
  > EOF
  $ keleut run --native-stack 512k list.kel
  500000500000
  1000000
  $ keleut run --native-stack 512k --gc-stress list.kel
  500000500000
  1000000

確保の量の合計が仮想メモリの上限 512 MiB を大きく超えるプログラムが、上限の下で完走する。
1 MiB の文字列を 1,000 回作り、10 万要素のリストを 30 回作る(合計は約 1.1 GiB)。
Node は仮想メモリの上限 512 MiB の下では起動できないので、上限を置く 2 つのコマンドは `--backend interp` を書く。
後に書いた旗が効くので、組 `kw` でもインタプリタで走る(kconform は「組の後段の上書き」として報告する)。
LLVM の後段の組 `kj`、`kt`、`ka`、`km` では、kconform の shim がこの旗を除くので、組の段で走る。

  $ cat > mem.kel <<'EOF'
  > let rec grow(s: String, n: Int32): String = if n == 0 then s else grow(s + s, n - 1)
  > let rec strs(n: Int32, k: Int32): Int32 = if n == 0 then k else strs(n - 1, k + __string_length(grow("ab", 19)) / 1048576)
  > echoln(show(strs(1000, 0)))
  > let rec lists(n: Int32, k: Int32): Int32 = if n == 0 then k else { let rec b(i: Int32, acc: List[Int32]): List[Int32] = if i == 0 then acc else b(i - 1, Cons(i, acc)); b(100000, Nil) match { case Cons(x, _) => lists(n - 1, k + x) case Nil => k } }
  > echoln(show(lists(30, 0)))
  > EOF
  $ (ulimit -v 524288; keleut run --native-stack 512k --backend interp mem.kel)
  1000
  30

ハンドラ記録、継続、退避した状態語の表の項目も回収する。
G の節の末尾でない resume を 20 万回、末尾の resume を 300 万回行う。
回収が表の項目を外さなければ、項目と引数の配列が積もって上限を超える。

  $ cat > gres.kel <<'EOF'
  > effect Tick = { tick: () => Unit }
  > let rec ticks(n: Int32): Unit @ {Tick} = if n == 0 then () else { perform tick(); ticks(n - 1) }
  > let count(n: Int32): Int32 = { ticks(n); 0 } handle { case tick() => { let r = resume(()); r + 1 } }
  > echoln(show(count(200000)))
  > ticks(3000000) handle { case tick() => if false then { let r = resume(()); r } else resume(()) }
  > echoln("G")
  > EOF
  $ (ulimit -v 524288; keleut run --native-stack 512k --backend interp gres.kel)
  200000
  G

構造的な Eq の比べ方は、項目の辞書の eq を入れ子のインタプリタで呼ぶ。
入れ子の中の確保で回収が起こっても、外側のフレームと比べている両辺が根に残る。
List の Eq は前提を持つ利用者のインスタンスなので、実装の関数を評価する間の呼び出しの被演算子も根になる。

  $ cat > eq.kel <<'EOF'
  > newtype P = P(Int32)
  > type instance Eq[P] { let eq(a, b) = a match { case P(x) => b match { case P(y) => show(x) + "!" == show(y) + "!" } } }
  > let rec loop(n: Int32, k: Int32): Int32 = if n == 0 then k else loop(n - 1, if {a = P(n), b = (P(n), "s" + show(n))} == {a = P(n), b = (P(n), "s" + show(n))} then k + 1 else k)
  > echoln(show(loop(2000, 0)))
  > let rec mk(n: Int32, acc: List[{v: P, w: String}]): List[{v: P, w: String}] = if n == 0 then acc else mk(n - 1, Cons({v = P(n), w = show(n)}, acc))
  > echoln(show(mk(3000, Nil) == mk(3000, Nil)))
  > echoln(show(mk(3000, Nil) == mk(2999, Nil)))
  > EOF
  $ keleut run --gc-stress eq.kel
  2000
  true
  false

区画の語の数がページの語の数を割り切る大きさの種類(Cons は 4 語、6 欄のデータは 8 語)で、切り出し中のページをちょうど使い切る。
そのページの切り出しの位置はページの終わりを指し、同じ種類の前のページの先頭と同じ番地になりうる。
掃除はその前のページも掃除し、印を消す。
消さなければ、次の回収でそのページの箱の子をたどらず、届く箱を回収してしまう。
8 語の C をちょうど 2 ページ分(8192 個)作り、1 ページ目の C だけが Cons のリストを持つ。

  $ cat > fullpage.kel <<'EOF'
  > newtype C = E | C(Int32, Int32, Int32, Int32, List[Int32], C)
  > let rec mkl(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else mkl(n - 1, Cons(n, acc))
  > let rec chain(n: Int32, l: List[Int32], prev: C): C = if n == 0 then prev else l match {
  >   case Cons(_, t) => chain(n - 1, t, C(n, 0, 0, 0, l, prev))
  >   case Nil => chain(n - 1, Nil, C(n, 0, 0, 0, Nil, prev))
  > }
  > let rec garbage(n: Int32, k: Int32): Int32 = if n == 0 then k else garbage(n - 1, k + (Cons(n, Cons(k, Nil)) match { case Cons(x, _) => x - n case Nil => 1 }))
  > let rec lsum(l: List[Int32], acc: Int32): Int32 = l match { case Nil => acc case Cons(x, t) => lsum(t, acc + x) }
  > let rec csum(c: C, acc: Int32): Int32 = c match { case E => acc case C(_, _, _, _, l, p) => csum(p, acc + lsum(l, 0) % 1000003) }
  > let c = chain(8192, mkl(4096, Nil), E)
  > echoln(show(garbage(3000000, 0)))
  > echoln(show(csum(c, 0)))
  > EOF
  $ keleut run fullpage.kel
  0
  1905818509
