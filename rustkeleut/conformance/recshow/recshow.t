レコードを含む <値> の綴りの試験である(計画 261104-1 の 5.3 節、IRSpec 付録 B)。
付録 B は、レコードのラベルの違うフィールドどうしの順を読み手に任せる。
期待は RustKeleut の綴りで記録する。
Diktor は値を作った順で書き、RustKeleut は正準の配置の順(ラベルの番号の昇順、同じラベルは最も外側から)で書く。

各プログラムは同じ値を 2 つの形で書く。
1 行目は cancel 節の打ち切りの記録で、kconform は記録と Diktor との比較の前にレコードのフィールドを並べ替える(D560)。
そのため、この行ではフィールドの順以外の綴りを記録と Diktor に照らせる。
2 行目は最上位の実行時エラーで、正規化が種別の語の後ろを切るので、記録と Diktor とは中身を比べない。
後段の群の中(ki kj kt ka km kw と kr-*)は正規化せずに比べるので、2 つの行とも、印字の 2 つの実装(Rust の Rt::show と host の show)が同じ順で書くことを見張る。

型の注釈のラベルの順と違う順で作ったレコード:

  $ cat > t1.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let chk(r: {b: Int32, a: Int32}): Unit = r match { case {b = 9, a = 9} => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk({a = 1, b = 2}) }) handle { case fail() => {} }
  > let main = chk({a = 1, b = 2})
  > EOF
  $ keleut run t1.kel
  ⚠ match が非網羅的です。例えば {b = 0, a = _} が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {b = 2, a = 1}
  実行時エラー: match のどの節にも一致しません: {b = 2, a = 1}
  [3]

拡張したレコード:

  $ cat > t2.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let base = {a = 1, b = 2}
  > let chk(r: {c: Int32, a: Int32, b: Int32}): Unit = r match { case {c = 9, a = 9, b = 9} => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk({c = 3 extends base}) }) handle { case fail() => {} }
  > let main = chk({c = 3 extends base})
  > EOF
  $ keleut run t2.kel
  ⚠ match が非網羅的です。例えば {c = 0, a = _, b = _} が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {a = 1, b = 2, c = 3}
  実行時エラー: match のどの節にも一致しません: {a = 1, b = 2, c = 3}
  [3]

前に書いた別のレコードがラベルの番号を決める:

  $ cat > t3.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let q = {a = 7, b = 8, c = 9}
  > let chk(r: {b: Int32, a: Int32, c: Int32}): Unit = r match { case {b = 9, a = 9, c = 9} => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk({b = 1, a = 2, c = 3}) }) handle { case fail() => {} }
  > let main = chk({b = 1, a = 2, c = 3})
  > EOF
  $ keleut run t3.kel
  ⚠ match が非網羅的です。例えば {b = 0, a = _, c = _} が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {a = 2, b = 1, c = 3}
  実行時エラー: match のどの節にも一致しません: {a = 2, b = 1, c = 3}
  [3]

ヴァリアントの中のレコード:

  $ cat > t5.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > type V = #P({b: Int32, a: Int32}) | #Q
  > let chk(v: V): Unit = v match { case #P({b = 9, a = 9}) => {} case #Q => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk(#P({a = 2, b = 1})) }) handle { case fail() => {} }
  > let main = chk(#P({a = 2, b = 1}))
  > EOF
  $ keleut run t5.kel
  ⚠ match が非網羅的です。例えば #P({b = 0, a = _}) が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: #P({b = 1, a = 2})
  実行時エラー: match のどの節にも一致しません: #P({b = 1, a = 2})
  [3]

同じラベルのフィールドは、どの読み手も最も外側から書く:

  $ cat > t7.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let r = {x = 1 extends {x = 2, y = 3}}
  > let chk(): Unit = r match { case {x = 9, ...rest} => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk() }) handle { case fail() => {} }
  > let main = chk()
  > EOF
  $ keleut run t7.kel
  ⚠ match が非網羅的です。例えば {x = 0, x = _, y = _} が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {x = 1, x = 2, y = 3}
  実行時エラー: match のどの節にも一致しません: {x = 1, x = 2, y = 3}
  [3]

タプルの中のレコード:

  $ cat > t10.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let chk(t: ({b: Int32, a: Int32}, Int32)): Unit = t match { case ({b = 9, a = 9}, 0) => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk(({a = 1, b = 2}, 3)) }) handle { case fail() => {} }
  > let main = chk(({a = 1, b = 2}, 3))
  > EOF
  $ keleut run t10.kel
  ⚠ match が非網羅的です。例えば ({b = 0, a = _}, _) が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: ({b = 2, a = 1}, 3)
  実行時エラー: match のどの節にも一致しません: ({b = 2, a = 1}, 3)
  [3]

タプルを拡張したレコード(_item のフィールドと普通のラベルが混ざる):

  $ cat > t11.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let t = (1, 2)
  > let r = {z = 3 extends t}
  > let chk(): Unit = r match { case {z = 9, ...rest} => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk() }) handle { case fail() => {} }
  > let main = chk()
  > EOF
  $ keleut run t11.kel
  ⚠ match が非網羅的です。例えば {z = 0, _item = _, _item = _} が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {_item = 1, _item = 2, z = 3}
  実行時エラー: match のどの節にも一致しません: {_item = 1, _item = 2, z = 3}
  [3]

更新してから拡張したレコード:

  $ cat > t12.kel <<'EOF'
  > effect A = { a: () => Unit }
  > effect Fail = { fail: () => Unit }
  > let r = {a = 1}
  > let s = {r with a = 2}
  > let u = {b = 0 extends s}
  > let chk(): Unit = u match { case {b = 9, a = 9} => {} }
  > let c = (perform fail() handle { case a() => resume() case cancel => chk() }) handle { case fail() => {} }
  > let main = chk()
  > EOF
  $ keleut run t12.kel
  ⚠ match が非網羅的です。例えば {b = 0, a = _} が漏れています
  cancel 節で例外が抑制されました: match のどの節にも一致しません: {a = 2, b = 0}
  実行時エラー: match のどの節にも一致しません: {a = 2, b = 0}
  [3]

ハンドラのどの節にも一致しない操作(付録 B の 9 番)は、ソースからは書けない。
型検査が、各操作にガードの無い節を求めるからである。
nomatch.kirt は、横の nomatch.kel を keleut emit-kir --text --no-prelude で書き、内側のハンドラ(main/inner)から b2 の節(op の項目と節の関数 main/inner$h0.op.b2)を手で除いたものである。
nomatch.kel はプレリュードを使わないので、Unit の代わりに {} と書く。
nomatch.kir は、それを keleut kir asm で組んだものである。
組の中で組まないのは、shim が keleut kir asm を組 dk でも本物の keleut に渡すので、基準の組の結果が RustKeleut のアセンブラに依るからである。
2 つが同じプログラムであることを、印字して確かめる:

  $ keleut kir dis nomatch.kir | cmp - nomatch.kirt && echo same
  same

<値> は、操作の名前と引数の組の綴り(E.op({…},))である。
cancel 節の中から 1 度、最上位から 1 度、b2 を送る。
組 dk、dk-ir、kf、x-ir では、shim がこの呼び出しを diktor --run-ir に渡すので、Diktor の読み手とも比べる:

  $ keleut run nomatch.kir
  cancel 節で例外が抑制されました: handle のどの節にも一致しません: Two.b2({a = 1, b = 2, c = 3},)
  実行時エラー: handle のどの節にも一致しません: Two.b2({a = 1, b = 2, c = 3},)
  [3]
