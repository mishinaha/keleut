構造的な `Eq` の比較が深い入れ子になっても、スタックオーバーフローにならずに比較できることを確かめるテストである。
期待出力は keleut run の結果で、Diktor の構文木の評価器の結果とも一致する。

構造的な `Eq` の比較は、値の入れ子 1 段ごとにランタイム関数からインタプリタを入れ子に呼び出し、1 段あたり約 1.5 KiB のネイティブスタックを使う。
ネイティブスタックの残りが少なくなると、新しいスタックを持つスレッドに実行を切り替えて比較を続けるので、512 KiB のスタックでも論理的な深さの上限まで比較できる。
出力の 1 行目は JIT コンパイルした関数が非末尾の再帰でネイティブスタックを残りわずかまで使った後に行う比較、2 行目はネイティブスタックをほとんど使っていない状態で行う 150 段の比較、3 行目と 4 行目は 20 万段の比較で、どのバックエンドでも同じ結果になる。

  $ cat > eq.kel <<'EOF'
  > newtype T = Leaf | Node({k: T})
  > type instance Eq[T] { let eq(a, b) = a match { case Leaf => b match { case Leaf => true case Node(_) => false } case Node(r) => b match { case Leaf => false case Node(s) => r == s } } }
  > let rec mk(n: Int32, acc: T): T = if n == 0 then acc else mk(n - 1, Node({k = acc}))
  > let rec f(n: Int32, x: T, y: T): Int32 = if n == 0 then (if x == y then 1 else 0) else 1 + f(n - 1, x, y)
  > echoln(show(f(100000, mk(35, Leaf), mk(35, Leaf))))
  > echoln(show(f(0, mk(150, Leaf), mk(150, Leaf))))
  > echoln(show(f(1000, mk(200000, Leaf), mk(200000, Leaf))))
  > echoln(show(f(1000, mk(200000, Leaf), mk(199999, Leaf))))
  > EOF
  $ keleut run --native-stack 512k eq.kel
  100001
  1
  1001
  1000
  $ keleut run --native-stack 512k --gc-stress eq.kel
  100001
  1
  1001
  1000
