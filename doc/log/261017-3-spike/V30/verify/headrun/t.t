エフェクトの名前の前方参照(LangSpec §6.4)。
型の本体(newtype のフィールド、effect の操作の型、型クラスのメソッドの型)は、
後ろで宣言するエフェクトのラベルを書ける。
エフェクトの名前は宣言表に先に置き、操作はそのあとで書き込むので、
宣言の順序は受理を変えない。

  $ export PATH="<一時領域>/items/V30/verify/headrun/bin:$PATH"

newtype のフィールドが後ろの effect を参照し、実行もできる(effnt):

  $ cat > effnt.kel <<'EOF'
  > newtype Cb = Cb((String) => Unit @ {Log})
  > effect Log = { log: (String) => Unit }
  > let call(c: Cb): Unit @ {Log} = c match { case Cb(g) => g("hi") }
  > let n: Int32 = {
  >   call(Cb(fn(s) => perform log(s))) handle { case log(s) => resume(()) }
  >   42
  > }
  > echoln(show(n))
  > EOF
  $ diktor effnt.kel
  42

略記の @ Log、extends つきの行、和の後ろのコンストラクタのフィールドでも同じである(effntforms):

  $ cat > effntforms.kel <<'EOF'
  > newtype N1 = N1(() => Unit @ Log)
  > newtype N2[E] = N2(() => Unit @ {Log extends E})
  > newtype N3 = A3 | B3(() => Unit @ {Log})
  > effect Log = { log: (String) => Unit }
  > let f[E](x: N2[E]): N2[E] = x
  > EOF
  $ diktor --type-check effntforms.kel
  f : (N2[R1]) => N2[R1]

effect の操作の型と、型クラスのメソッドの型も、後ろの effect を参照できる(effop):

  $ cat > effop.kel <<'EOF'
  > effect A = { a: (() => Unit @ {B}) => Unit }
  > type class C[T] { val m: (T) => Unit @ {B} }
  > effect B = { b: () => Unit }
  > let g(): Unit @ {A} = perform a(fn() => {})
  > EOF
  $ diktor --type-check effop.kel
  g : () => {} @ {A}

どこにも宣言していないラベルは、これまでどおり未知のエフェクトである(effunknown):

  $ printf 'newtype N = N(() => Unit @ {Nope})\neffect Log = { log: (String) => Unit }\n' > effunknown.kel
  $ diktor --type-check effunknown.kel
  ! effunknown.kel:1:28: 型エラー: 未知のエフェクト: Nope
  [1]

名前の検査(重複、標準環境の名前)は名前を置く時点で行うので、本体の誤りより先に報告する。
型パラメータを付けた effect は、参照する側ではなく effect の宣言のほうで落ちる(effname):

  $ printf 'newtype N = N(() => Unit @ {Nope})\neffect Log = { log: (String) => Unit }\neffect Log = { log: (String) => Unit }\n' > effdup.kel
  $ diktor --type-check effdup.kel
  ! effdup.kel:3:1: 型エラー: effect Log が二重に宣言されています
  [1]
  $ printf 'newtype N = N(() => Unit @ {Nope})\neffect Console = { w: (String) => Unit }\n' > effstd.kel
  $ diktor --type-check effstd.kel
  ! effstd.kel:2:1: 型エラー: 標準環境の effect Console は再宣言できません
  [1]
  $ printf 'newtype N = N(() => Unit @ {Log})\neffect Log[A] = { log: (String) => Unit }\n' > effparams.kel
  $ diktor --type-check effparams.kel
  ! effparams.kel:2:1: 型エラー: effect 宣言に型パラメータは書けません(sample.kel §9)
  [1]

対話的な実行では、前方参照は 1 つの入力の中に限る(LangSpec §6.6)。
受理しなかった入力の effect の名前は後の入力から見えず、同じ effect を宣言し直せる(effrepl):

  $ diktor --repl <<'EOF2'
  > effect Log = { log: (String) => Unit }; newtype N = N(() => Unit @ {Nope})
  > effect Log = { log: (String) => Unit }
  > newtype P = P(() => Unit @ {Later})
  > effect Later = { later: () => Unit }
  > newtype P = P(() => Unit @ {Later}); newtype Q = Q(() => Unit @ {Later2}); effect Later2 = { l2: () => Unit }
  > EOF2
  ! <stdin>:1:68: 型エラー: 未知のエフェクト: Nope
  ! <stdin>:3:28: 型エラー: 未知のエフェクト: Later
