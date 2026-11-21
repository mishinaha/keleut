GC のテストである。
期待出力は keleut run の結果で、Diktor の構文木の評価器の結果とも一致する。

100 万要素のリストを末尾呼び出しで作り、末尾再帰と非末尾の再帰で走査する。
マークは再帰せずにマークスタックを使って反復するので、ネイティブスタックが 512 KiB でも異常終了しない。
非末尾の再帰をする len は値スタックに 100 万段のフレームを積み、GC はそのすべてをルートとして走査する。

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

確保の合計が 512 MiB を大きく超えるプログラムを、仮想メモリを 512 MiB に制限したまま(ulimit -v 524288)最後まで実行できることを確かめる。
1 MiB の文字列を 1,000 回作り、10 万要素のリストを 30 回作る(合計は約 1.1 GiB)。
Node は仮想メモリを 512 MiB に制限すると起動できないので、512 MiB に制限する 2 つのコマンドには `--backend interp` を指定する。
後に書いたオプションが優先されるので、ペア `kw` でもインタプリタで実行される(kconform は `組の後段の上書き` の行で報告する)。
LLVM ペア `kj`、`kt`、`ka`、`km` では、kconform の shim がこのオプションを取り除くので、ペアのバックエンドとティアの設定で実行される。

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

ハンドラ記録の表、継続の表、退避した状態ワードの表の項目も、GC が解放する。
G の節で、非末尾の resume を 20 万回、末尾位置の resume を 300 万回行う。
GC が表の項目を解放しなければ、項目と引数の配列がたまり、仮想メモリの制限を超える。

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

構造的な Eq の比較は、ユーザ定義のインスタンス(ここでは Eq[P])の eq を、入れ子にしたインタプリタの呼び出しで実行する。
入れ子の呼び出しの中の確保で GC が起こっても、外側のフレームと、比較している両辺の値はルートとして残る。
List の Eq はプレリュードが定義する前提付きインスタンスなので、その実装の関数を評価している間は、呼び出しのオペランドもルートになる。

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

セルのワード数がページのワード数を割り切るサイズクラス(Cons は 4 ワード、フィールドが 6 個のデータは 8 ワード)で、バンプ割り当て中のページをちょうど使い切る。
そのページのバンプポインタはページの終わりを指し、同じサイズクラスでそれより前に確保したページの先頭と同じアドレスになることがある。
スイープは、その前に確保したページもスイープして、マークを消す。
消さなければ、次の GC でそのページのオブジェクトの子をたどらず、到達できるオブジェクトを解放してしまう。
8 ワードの C をちょうど 2 ページ分(8192 個)作り、1 ページ目の C だけが Cons のリストを持つ。

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

再利用する継続のコピー領域は、必要なサイズに近いものだけを選ぶ。
f の各段で、ハンドラ式の本体が 3,000 段の非末尾の再帰の底で big を起こし、big の節は深い継続を作って再開する。
再開した継続のコピー領域は、再利用の候補に入る。
続いて small の節が浅い継続を作り、f の再帰が 2,000 段に達するまで、その継続を生かしたままにする。
浅い継続が深い継続のコピー領域(約 3,000 フレーム分)を持てば、生きている継続ごとに余分なメモリが残り、仮想メモリの制限 1 GiB を超える。

  $ cat > contbuf.kel <<'EOF'
  > effect Two = { big: (Int32) => Unit, small: (Int32) => Unit }
  > let rec deep(d: Int32): Int32 @ {Two} = if d == 0 then { perform big(0); 0 } else { let r = deep(d - 1); r + 1 }
  > let rec f(i: Int32, n: Int32): Int32 = if i == n then 0 else {
  >   { let u = deep(3000); perform small(u); 0 } handle {
  >     case big(x) => { let u = resume(()); u + 1 }
  >     case small(x) => { let a = f(i + 1, n); let u = resume(()); u + a + 1 }
  >   }
  > }
  > echoln(show(f(0, 2000)))
  > EOF
  $ (ulimit -v 1048576; keleut run --backend interp contbuf.kel)
  4000
