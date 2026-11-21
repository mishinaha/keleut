Float64 のリテラルの読み取りと、境界値の表示のテストである。
演算、libm、整数への変換は prim/prim.t で確かめる。
期待出力は keleut run の結果で、Diktor の構文木の評価器の結果とも一致する。
ペア kr-* と x-ir ではフロントエンド(crate keleut-front)によるリテラルの読み取りを、ペア kw と kr-kw では wasm32 での表示を確かめる。

リテラルの表記と境界値を確かめる。
小数点の後ろの省略と指数の大文字、範囲を超える値(inf と 0.0)、最小の非正規化数の半分の前後、2^53 を超える整数の丸め、最大の有限値の前後の丸め、正規化数の境界、10 の累乗の表示を並べる:

  $ cat > lit.kel <<'EOF'
  > echoln(show(1.))
  > echoln(show(1.e5))
  > echoln(show(1E5))
  > echoln(show(1.5e-3))
  > echoln(show(1e309))
  > echoln(show(1e-400))
  > echoln(show(2.4703282292062327e-324))
  > echoln(show(2.4703282292062328e-324))
  > echoln(show(9007199254740993.0))
  > echoln(show(9007199254740995.0))
  > echoln(show(1.7976931348623158e308))
  > echoln(show(1.7976931348623159e308))
  > echoln(show(0.1e1))
  > echoln(show(123456789012345678901234567890.0))
  > echoln(show(0.000000000000000000000000000001))
  > echoln(show(4.9406564584124654e-324))
  > echoln(show(2.2250738585072011e-308))
  > echoln(show(2.2250738585072014e-308))
  > echoln(show(1e0 + 1e-16))
  > echoln(show(3.0 * 0.1))
  > echoln(show(-1e16))
  > echoln(show(9999999999999998.0))
  > echoln(show(1e22))
  > echoln(show(1e23))
  > echoln(show(0.1 * 3.0 == 0.3))
  > echoln(show(__f64_to_i64(9.2233720368547748e18)))
  > EOF
  $ keleut run lit.kel
  1.0
  100000.0
  100000.0
  0.0015
  inf
  0.0
  0.0
  5e-324
  9007199254740992.0
  9007199254740996.0
  1.7976931348623157e308
  inf
  1.0
  1.2345678901234568e29
  1e-30
  5e-324
  2.225073858507201e-308
  2.2250738585072014e-308
  1.0
  0.30000000000000004
  -1e16
  9999999999999998.0
  1e22
  1e23
  false
  9223372036854774784

接尾辞 f64 と桁区切りの _ を確かめる。
0x10f64 は、f64 の部分も 16 進の数字として読まれるので、Int32 の 69476 になる。
桁の多い 10 進の整数に f64 を付けると、Float64 に丸められる:

  $ cat > suffix.kel <<'EOF'
  > echoln(show(3f64))
  > echoln(show(1_000.5))
  > echoln(show(0x10f64))
  > echoln(show(0x1_0f64))
  > echoln(show(99999999999999999999f64))
  > echoln(show(1e5f64))
  > echoln(show(1.5f64))
  > EOF
  $ keleut run suffix.kel
  3.0
  1000.5
  69476
  69476
  1e20
  100000.0
  1.5

f64 まで 16 進の数字として読んだ値が Int32 の範囲を超えると、そのリテラルを評価したときに実行時エラーになり、終了コード 3 で終了する:

  $ printf 'echoln("before")\necholn(show(0xffffffffffffffffff_f64))\n' > hexrange.kel
  $ keleut run hexrange.kel
  before
  実行時エラー: 数値リテラルが範囲外です: 0xffffffffffffffffff_f64
  [3]

次の表記は構文エラーになり、終了コード 2 で終了する。
指数には桁区切りを書けず、2 進と 8 進の整数には f64 を付けられず、16 進の浮動小数点数の表記はない:

  $ printf 'echoln(show(1_0e1_0))\n' > e_sep.kel
  $ keleut run e_sep.kel
  e_sep.kel:1:13: 構文エラー: 数値リテラル 1_0e1 の直後に _0 が続いています(数値リテラルの綴りを確認してください)
  [2]
  $ printf 'echoln(show(0b101f64))\n' > e_bin.kel
  $ keleut run e_bin.kel
  e_bin.kel:1:13: 構文エラー: 数値リテラル 0b101 の直後に f64 が続いています(数値リテラルの綴りを確認してください)
  [2]
  $ printf 'echoln(show(0o17f64))\n' > e_oct.kel
  $ keleut run e_oct.kel
  e_oct.kel:1:13: 構文エラー: 数値リテラル 0o17 の直後に f64 が続いています(数値リテラルの綴りを確認してください)
  [2]
  $ printf 'echoln(show(0x1p3))\n' > e_hexp.kel
  $ keleut run e_hexp.kel
  e_hexp.kel:1:13: 構文エラー: 数値リテラル 0x1 の直後に p3 が続いています(数値リテラルの綴りを確認してください)
  [2]
  $ printf 'echoln(show(0x1.8))\n' > e_hexdot.kel
  $ keleut run e_hexdot.kel
  e_hexdot.kel:1:13: 構文エラー: 数値リテラル 0x1 の直後に .8 が続いています(数値リテラルの綴りを確認してください)
  [2]
  $ printf 'echoln(show(0x1.8f64))\n' > e_hexdotf.kel
  $ keleut run e_hexdotf.kel
  e_hexdotf.kel:1:13: 構文エラー: 数値リテラル 0x1 の直後に .8f64 が続いています(数値リテラルの綴りを確認してください)
  [2]
  $ printf 'echoln(show(0x1p3f64))\n' > e_hexpf.kel
  $ keleut run e_hexpf.kel
  e_hexpf.kel:1:13: 構文エラー: 数値リテラル 0x1 の直後に p3f64 が続いています(数値リテラルの綴りを確認してください)
  [2]

a * b + c を、関数の引数として受け取った値と、同じ値の定数式で計算する。
どのペアも積を丸めてから足すので、3 組の値のどれでも結果は 0.0 になる。
積と和を 1 回の丸めにまとめる(fma に縮約する)と、順に 5.551115123125783e-17、-4.930380657631324e-32、-5.551115123125783e-17 になる。
LLVM バックエンドの最適化と定数畳み込みが IEEE 754 の丸めを変えないことを確かめる:

  $ cat > arith.kel <<'EOF'
  > let fma_form(a: Float64, b: Float64, c: Float64): Float64 = a * b + c
  > echoln(show(fma_form(0.1, 10.0, -1.0)))
  > echoln(show(0.1 * 10.0 + -1.0))
  > echoln(show(fma_form(1.0000000000000002, 0.9999999999999998, -1.0)))
  > echoln(show(1.0000000000000002 * 0.9999999999999998 + -1.0))
  > echoln(show(fma_form(1.0 / 3.0, 3.0, -1.0)))
  > echoln(show(1.0 / 3.0 * 3.0 + -1.0))
  > EOF
  $ keleut run arith.kel
  0.0
  0.0
  0.0
  0.0
  0.0
  0.0
