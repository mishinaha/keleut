再帰群の中で前にある束縛から後ろにある束縛への参照は単相である(LangSpec §6.3)。
前にある束縛の本体が後ろの束縛の型パラメータを何かの型に決めると、2 つの剛定数か、
剛定数と具体的な型が一致せずに型エラーになる。その報告に、どの束縛のどの型パラメータが
相手の型に入り込んだかを添える。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

[A: Eq] と [B: Eq] を注釈した 2 つの関数が互いを呼ぶ形。f の本体が g を呼ぶ時点で、
g の引数の型が f の A に決まる。g の本体が f を呼ぶ位置(引数 x)で、A と g の B が
一致しない:

  $ cat > twotp.kel <<'KEL'
  > let rec f[A: Eq](x: A, y: A): Boolean = x == y && g(x, y)
  > and g[B: Eq](x: B, y: B): Boolean = x == y || f(x, y)
  > echoln(show(f(1, 2)))
  > KEL
  $ diktor twotp.kel
  ! twotp.kel:2:49: 型エラー: スコープ付きの型が一致しません: ς1 と ς2(再帰群の中で、前にある束縛から後ろにある束縛への参照は単相です。f の型パラメータ A が g の型に入り込んでいます。前にある束縛の本体が後ろの束縛の型パラメータを何かの型に決める形は、型検査に通りません。§6.3)
  [1]

2 つの束縛を逆の順に書いても、互いを呼ぶので同じ形で落ちる。報告は入れ替わった
名前を挙げる:

  $ cat > twotprev.kel <<'KEL'
  > let rec g[B: Eq](x: B, y: B): Boolean = x == y || f(x, y)
  > and f[A: Eq](x: A, y: A): Boolean = x == y && g(x, y)
  > echoln(show(f(1, 2)))
  > KEL
  $ diktor twotprev.kel
  ! twotprev.kel:2:49: 型エラー: スコープ付きの型が一致しません: ς1 と ς2(再帰群の中で、前にある束縛から後ろにある束縛への参照は単相です。g の型パラメータ B が f の型に入り込んでいます。前にある束縛の本体が後ろの束縛の型パラメータを何かの型に決める形は、型検査に通りません。§6.3)
  [1]

後ろの g の型パラメータの注釈を外すと、g の型は f の A から決まり、群の終わりに
一般化されるので通る:

  $ cat > twotpinfer.kel <<'KEL'
  > let rec f[A: Eq](x: A, y: A): Boolean = x == y && g(x, y)
  > and g(x, y) = x == y || f(x, y)
  > echoln(show(f(1, 2)))
  > KEL
  $ diktor twotpinfer.kel
  false

前にある束縛が後ろの束縛の型パラメータの位置に具体的な型の値を渡す形。衝突は g の
本体の型を g の名前の型と一致させるところで起きる:

  $ cat > concrete.kel <<'KEL'
  > let rec f[A](x: A): Int32 = g(1i32)
  > and g[B](y: B): Int32 = 0
  > echoln(show(f(1)))
  > KEL
  $ diktor concrete.kel
  ! concrete.kel:2:5: 型エラー: スコープ付きの型が一致しません: Int32 と ς1(再帰群の中で、前にある束縛から後ろにある束縛への参照は単相です。f の本体が、型パラメータを持つ g を参照しています。前にある束縛の本体が後ろの束縛の型パラメータを何かの型に決める形は、型検査に通りません。§6.3、§18.2)
  [1]

前にある束縛が後ろの束縛を参照しても、後ろの束縛の型パラメータを決めたのが後ろの
束縛自身の本体(多相再帰)なら、前の束縛を挙げない。この形は g だけの群でも、2 つを
逆の順に書いても同じく落ちる:

  $ cat > selfrec.kel <<'KEL'
  > let rec f(n: Int32): Int32 = { let k = g; n }
  > and g[B](y: B): Int32 = g(1i32)
  > echoln(show(f(1i32)))
  > KEL
  $ diktor selfrec.kel
  ! selfrec.kel:2:5: 型エラー: スコープ付きの型が一致しません: Int32 と ς1
  [1]

後ろの束縛が型パラメータを持たないときの衝突は、群の順序によらない型の誤りなので、
何も添えない:

  $ cat > notp.kel <<'KEL'
  > let rec f[A](x: A): Int32 = g(x)
  > and g(y: Int32): Int32 = 0
  > echoln(show(f(1)))
  > KEL
  $ diktor notp.kel
  ! notp.kel:2:5: 型エラー: スコープ付きの型が一致しません: ς1 と Int32
  [1]

群の中の本体で起きた、群の型パラメータと関わらない剛定数の衝突(別の run のヒープ)
にも添えない。f の h は bump の型に入り込んでいるが、衝突したのは bump の k と run の
剛定数である:

  $ cat > region.kel <<'KEL'
  > let rec f[h](a: MutableArray[h, Int32]): Unit = bump(a)
  > and bump[k](a: MutableArray[k, Int32]): Unit = run h2 { MutableArray.set(a, 0, 42) }
  > KEL
  $ diktor --type-check region.kel
  ! region.kel:2:74: 型エラー: スコープ付きの型が一致しません: ς1 と ς2
  [1]
