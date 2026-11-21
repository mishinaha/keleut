レコードを含む <値> の表記のテストである(IRSpec 付録 B)。
付録 B は、ラベルの違うフィールドどうしを並べる順序をリーダに任せている。
期待出力は RustKeleut の表記で書いてある。
Diktor は値を作った順に書き、RustKeleut は正準なレイアウトの順(ラベルの番号の昇順で、同じラベルのフィールドは最も外側から)に書く。

各プログラムは、同じ値を 2 つの形式で書く。
1 行目は cancel 節の抑制の記録で、kconform は、期待出力や Diktor と比較する前にレコードのフィールドを並べ替える。
そのため、この行では、フィールドの順序以外の表記を期待出力や Diktor と比較できる。
2 行目は最上位の実行時エラーで、kconform が正規化で種別名の後ろを切り捨てるので、期待出力や Diktor とは内容を比較しない。
バックエンドのグループ(ki kj kt ka km kw と kr-*)の中では正規化せずに比較するので、どちらの行でも、値を文字列化する 2 つの実装(crate keleut-rt の Rt::show と host の show)が同じ順序で書くことを検査できる。

型注釈のラベルの順序と違う順序で作ったレコード:

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

先に書いた別のレコードでラベルの番号が決まる場合:

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

同じラベルのフィールドは、どのリーダも最も外側から順に書く:

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

タプルを拡張したレコード(_item のフィールドと通常のラベルのフィールドが混ざる):

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
型検査が、各操作にガードのない節を要求するからである。
nomatch.kirt は、同じディレクトリの nomatch.kel を keleut emit-kir --text --no-prelude で書き出し、内側のハンドラ(main/inner)から b2 の節(op の項目と、節の関数 main/inner$h0.op.b2)を手で取り除いたものである。
nomatch.kel はプレリュードを使わないので、Unit の代わりに {} と書く。
nomatch.kir は、nomatch.kirt を keleut kir asm でバイナリに変換したものである。
テストの中で変換せずに変換済みの nomatch.kir を置くのは、shim が keleut kir asm をペア dk でも本物の keleut に渡すので、テストの中で変換すると基準のペアの結果が RustKeleut のアセンブラで決まるからである。
2 つが同じプログラムであることを、nomatch.kir をテキストに変換して確かめる:

  $ keleut kir dis nomatch.kir | cmp - nomatch.kirt && echo same
  same

<値> は、操作の名前と引数の組の表記(E.op({…},))である。
操作 b2 を、cancel 節の中から 1 回、最上位から 1 回起こす。
ペア dk、dk-ir、kf、x-ir では、shim がこの呼び出しを diktor --run-ir に渡すので、Diktor のリーダの結果とも比較する:

  $ keleut run nomatch.kir
  cancel 節で例外が抑制されました: handle のどの節にも一致しません: Two.b2({a = 1, b = 2, c = 3},)
  実行時エラー: handle のどの節にも一致しません: Two.b2({a = 1, b = 2, c = 3},)
  [3]
