構造的な `Eq` の入れ子とネイティブのスタックの試験である。
記録は keleut run の結果で、Diktor の木の評価器の結果とも同じである。

構造的な `Eq` の比べ方は、値の入れ子の段ごとに実行時の関数からインタプリタを入れ子に呼び、ネイティブのスタックを 1 段に約 1.5 KiB 使う。
入れ子がスタックの下限に近づくと、新しいスタックのスレッドに移って続けるので、512 KiB のスタックでも論理的な深さの限度まで比べられる。
JIT の関数の末尾でない再帰がスタックを下限の近くまで使った後の比べ方(1 行目)も、浅い位置からの 150 段の比べ方(2 行目)も、20 万段の比べ方(3 行目と 4 行目)も、どの後段でも同じ結果になる。

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
