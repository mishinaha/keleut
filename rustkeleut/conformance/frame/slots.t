スロットの多い関数のフレームの試験である(計画 261104-3 の D573)。
インタプリタのバイトコードは被演算子を 1 つずつ 32 ビットの語に置くので、スロットが 256 を超える関数も同じ形の命令で書く。
記録は keleut run の結果で、Diktor の木の評価器の結果とも同じである。

big は 300 個の Int32 を生かしたまま id を 300 回呼び、最後にすべてを足す。
どの値も最後の和まで生きるので、関数のスロットは 300 を超える:

  $ awk 'BEGIN { n = 300; print "let id(x: Int32): Int32 = x"; print "let big(k: Int32): Int32 = {"; for (i = 0; i < n; i++) print "  let v" i " = id(k + " i ")"; s = "v0"; for (i = 1; i < n; i++) s = s " + v" i; print "  " s; print "}"; print "echoln(show(big(1)))"; print "echoln(show(big(2)))" }' > slots.kel
  $ keleut run slots.kel
  45150
  45450
