  $ cat > cx.kel <<'EOF2'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let base = {a = 1, b = 2}
  > let chk(r: {c: Int32, a: Int32, b: Int32}): Unit = r match { case {c = 9, a, b} => {} }
  > let r = (perform fail() handle {
  >     case a() => resume()
  >     case cancel => { echoln("inner cancel start"); chk({c = 3 extends base}); echoln("rest") }
  >   }) handle {
  >   case fail() => echoln("caught")
  > }
  > echoln("after")
  > EOF2
  $ diktor cx.kel
  ⚠ match が非網羅的です。例えば {c = 0, a = _, b = _} が漏れています
  caught
  inner cancel start
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {c = 3, a = 1, b = 2}
  after
