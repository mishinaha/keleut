型エイリアスの型引数も、パラメータのカインドが決まる前に読む位置がある。後ろで
宣言するエイリアスに {} を渡しても、エイリアスを先に宣言した形と同じ型になる。
{} はエイリアスの本体が E を使う位置(ここでは @ の右)で空のエフェクト行と読む:

  $ cat > aliasunit.kel <<'EOF'
  > type Unit = {}
  > type C19 = A19[{}]
  > type A19[E] = (Int32) => Unit @ E
  > let f(g: C19): Unit = g(1)
  > EOF
  $ diktor --type-check --no-prelude aliasunit.kel
  f : ((Int32) => {} @ {}) => {}

本体が E を値の型の位置で使うエイリアスでは、同じ {} を空レコード型と読む:

  $ cat > aliasunit2.kel <<'EOF'
  > type Unit = {}
  > type C20 = A20[{}]
  > type A20[E] = (E, Int32)
  > let v: C20 = ({}, 2)
  > EOF
  $ diktor --type-check --no-prelude aliasunit2.kel
  v : ({}, Int32)

エイリアスの連鎖と {extends R} の形。中継のエイリアス B21 のパラメータも、
末端の A21 が決めるカインドを受け取る。{extends R} の R は行のカインドを持つ:

  $ cat > aliasunit3.kel <<'EOF'
  > type Unit = {}
  > type C21[R] = B21[{extends R}]
  > type D21 = B21[{}]
  > type B21[E] = A21[E]
  > type A21[E] = (Int32) => Unit @ E
  > let f[R](g: C21[R]): Unit @ R = g(1)
  > let h(g: D21): Unit = g(2)
  > EOF
  $ diktor --type-check --no-prelude aliasunit3.kel
  f : ((Int32) => {}) => {}
  h : ((Int32) => {} @ {}) => {}

newtype のフィールドからエイリアスへ行を渡す形も、宣言順に依存しない。具体的な
行({Print})を渡す形は、エイリアスを newtype より前に宣言しても、かつては
読み分けの時点でエイリアスのパラメータのカインドが決まっていなかった:

  $ cat > ntalias.kel <<'EOF'
  > type Unit = {}
  > effect Print = { print: (String) => Unit }
  > newtype Callback22[E] = MkCb22(() => Unit @ E)
  > type Cb22[E] = Callback22[E]
  > newtype N22 = MkN22(Cb22[{Print}], Cb22[{}])
  > newtype M22 = MkM22(A22[{}])
  > type A22[E] = (Int32) => Unit @ E
  > let f(n: N22, m: M22): Int32 = 0
  > EOF
  $ diktor --type-check --no-prelude ntalias.kel
  f : (N22, M22) => Int32

エイリアスの本体の中で、行のパラメータより右に同じ宣言群のエフェクトラベルがある
形。投機はラベルの名前だけを知っていればよいので、effect の操作の登録(1b)を
待たずにパラメータのカインドが決まる:

  $ cat > aliaslab.kel <<'EOF'
  > type Unit = {}
  > effect Log = { log: (String) => Unit }
  > type Cb23[E] = (() => Unit @ E, () => Unit @ {Log})
  > newtype N23 = MkN23(Cb23[{}])
  > newtype M23 = MkM23(Cb23[{Log}])
  > let f(n: N23, m: M23): Int32 = 0
  > EOF
  $ diktor --type-check --no-prelude aliaslab.kel
  f : (N23, M23) => Int32

投機の中でラベルとして読むのは、effect として宣言した名前だけである。宣言の
無い名前と、effect でない型の名前は、投機の外と同じ診断になる:

  $ cat > aliaslab2.kel <<'EOF'
  > type Unit = {}
  > newtype Q24 = MkQ24(Int32)
  > type C24 = A24[{}]
  > type A24[E] = (Int32) => Unit @ {Q24 extends E}
  > EOF
  $ diktor --type-check --no-prelude aliaslab2.kel
  ! aliaslab2.kel:4:33: 型エラー: 型 Q24 はエフェクトではありません(ここにはエフェクト行が要ります)
  [1]
  $ cat > aliaslab3.kel <<'EOF'
  > type Unit = {}
  > type C25 = A25[{}]
  > type A25[E] = (Int32) => Unit @ {Nope extends E}
  > EOF
  $ diktor --type-check --no-prelude aliaslab3.kel
  ! aliaslab3.kel:3:33: 型エラー: 未知のエフェクト: Nope
  [1]

