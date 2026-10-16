コンストラクタの実引数とフィールドの型の単一化が行の不一致で落ちたとき、
フィールドの型に行の閉じた矢印があれば、newtype のフィールドの矢印の読み方(LangSpec §13.2)を
診断に添える。エイリアスを展開して現れる矢印と、型引数の中の矢印も数える。行パラメータを取る
型の型引数に書いた閉じた行(Cb[{}] の {})も数える:

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

  $ cat > ctorbox.kel <<'KEL'
  > newtype Box = Box(() => Int32)
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let x = Box(b)
  > KEL
  $ diktor --type-check ctorbox.kel
  b : () => Int32 @ {Console}
  ! ctorbox.kel:3:9: 型エラー: ラベル Console がありません(行は閉じています)(コンストラクタ Box のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]
  $ cat > ctoralias.kel <<'KEL'
  > type F = () => Int32
  > newtype Box = Box(F)
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let x = Box(b)
  > KEL
  $ diktor --type-check ctoralias.kel
  b : () => Int32 @ {Console}
  ! ctoralias.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)(コンストラクタ Box のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]
  $ cat > ctornested.kel <<'KEL'
  > newtype L = L(List[() => Int32])
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let x = L(Cons(b, Nil))
  > KEL
  $ diktor --type-check ctornested.kel
  b : () => Int32 @ {Console}
  ! ctornested.kel:3:9: 型エラー: ラベル Console がありません(行は閉じています)(コンストラクタ L のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]
  $ cat > ctorrowarg.kel <<'KEL'
  > newtype Cb[R] = Cb(() => Int32 @ R)
  > newtype W = W(Cb[{}])
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let w = W(Cb(b))
  > KEL
  $ diktor --type-check ctorrowarg.kel
  b : () => Int32 @ {Console}
  ! ctorrowarg.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)(コンストラクタ W のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]

フィールドの型が型パラメータのときは、閉じた行は別の実引数から型パラメータに流れ込んだもので、
フィールドの矢印の読み方は原因でない。この読み方を添えず、型変数の仮引数を持つ関数の呼び出し
と同じく単一化の報告だけを出す。名前の参照でない式(mkp())を置いた形と、型注釈の中の
入れ子の矢印が閉じた名前(ys)を置いた形:

  $ cat > ctortp.kel <<'KEL'
  > newtype Pair[A] = Pair(A, A)
  > let mkp(): () => Int32 = fn() => 1
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let p = Pair(mkp(), b)
  > KEL
  $ diktor --type-check ctortp.kel
  mkp : () => () => Int32 @ {}
  b : () => Int32 @ {Console}
  ! ctortp.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)
  [1]
  $ cat > ctortpname.kel <<'KEL'
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let ys: List[() => Int32] = Nil
  > let xs = Cons(b, ys)
  > KEL
  $ diktor --type-check ctortpname.kel
  b : () => Int32 @ {Console}
  ys : List[() => Int32 @ {}]
  ! ctortpname.kel:3:10: 型エラー: ラベル Console がありません(行は閉じています)
  [1]

フィールドの矢印の行が行変数のときも、読み方を添えない。2 つのフィールドが同じ行変数を
共有し、一方に閉じた行の関数を置いた形:

  $ cat > ctorrowvar.kel <<'KEL'
  > newtype Two[R] = Two(() => Int32 @ R, () => Int32 @ R)
  > let mkp(): () => Int32 = fn() => 1
  > let b(): Int32 @ Console = { echo("b"); 2 }
  > let t = Two(mkp(), b)
  > KEL
  $ diktor --type-check ctorrowvar.kel
  mkp : () => () => Int32 @ {}
  b : () => Int32 @ {Console}
  ! ctorrowvar.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)
  [1]

矢印も閉じた行も無いフィールドで、レコードのフィールドが欠けた形も、読み方を添えない:

  $ cat > ctorrecnoarrow.kel <<'KEL'
  > newtype R = R({f: Int32, g: Int32})
  > let x = R({f = 1})
  > KEL
  $ diktor --type-check ctorrecnoarrow.kel
  ! ctorrecnoarrow.kel:2:9: 型エラー: ラベル g がありません(行は閉じています)
  [1]

判定はフィールドの型に閉じた行を書いた位置があるかどうかだけを見て、失敗した位置も、
欠けたラベルがエフェクトかどうかも見ない。行の閉じた矢印と型パラメータを両方含むフィールドで
型パラメータの側で落ちた形、引数の個数が合わない形(_item)、閉じた矢印を持つレコードの
フィールドが欠けた形でも、読み方を添える:

  $ cat > ctormixed.kel <<'KEL'
  > newtype Q[A] = Q(List[(A) => Int32], List[(A) => Int32])
  > let k(g: () => Int32): Int32 = 0
  > let k2(g: () => Int32 @ Console): Int32 = 0
  > let q = Q(Cons(k, Nil), Cons(k2, Nil))
  > KEL
  $ diktor --type-check ctormixed.kel
  k : (() => Int32 @ {}) => Int32
  k2 : (() => Int32 @ {Console}) => Int32
  ! ctormixed.kel:4:9: 型エラー: ラベル Console がありません(行は閉じています)(コンストラクタ Q のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]
  $ cat > ctorarity.kel <<'KEL'
  > newtype Box = Box((Int32) => Int32)
  > let k(): Int32 = 1
  > let x = Box(k)
  > KEL
  $ diktor --type-check ctorarity.kel
  k : () => Int32
  ! ctorarity.kel:3:9: 型エラー: ラベル _item がありません(行は閉じています)(コンストラクタ Box のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]
  $ cat > ctorrecmiss.kel <<'KEL'
  > newtype R = R({f: () => Int32, g: Int32})
  > let x = R({f = fn() => 1})
  > KEL
  $ diktor --type-check ctorrecmiss.kel
  ! ctorrecmiss.kel:2:9: 型エラー: ラベル g がありません(行は閉じています)(コンストラクタ R のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)
  [1]
