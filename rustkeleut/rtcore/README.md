# rtcore

RustKeleut のすべてのバックエンドが共有する、ランタイムの構成要素である。
フリースタンディングな(`-ffreestanding`。libc と OS に依存しない)C99 で書いてある。
同じソースを、`crates/keleut-rtcore/build.rs` が x86_64 の静的ライブラリと wasm32 の `rt.wasm` にビルドする。

## ファイル構成

- `rtcore.h`: C の API と、その仕様と事前条件。
- `f64show.c`: `show(Float64)`(LangSpec §16.2)。
  多倍長整数による正確な 10 進展開と、丸めの区間との正確な比較で、読み戻すと元の値になる最小の桁数を求める。
- `shape.c`: レコードのシェイプ表(intern、拡張、フィールドの除去、位置の検索)。
  メモリは呼び出し元が渡す。
- `bytes.c`: 符号なしバイトの辞書順の比較。
- `musl/`: musl 1.2.6 の `sin cos exp log` と、それらが依存するファイルを、改変せずにコピーしたもの。
  バージョン、入手元、sha256 は `musl/SOURCE.md` に、ライセンスは `musl/COPYRIGHT` にある。
- `shim/`: libc のヘッダの代わりに使う最小限のヘッダと、外部シンボルを `krt_` で始まる名前に変える `krt_names.h`。
  `shim/` は、musl のソースをビルドするために rtcore が用意したヘッダである。

## ビルド方法

コンパイルオプションは次のとおりである。

- 共通: `-std=c99 -O2 -ffreestanding -fno-builtin -ffp-contract=off -fno-math-errno -nostdlibinc -Ishim -Imusl/src/internal -I. -include shim/krt_names.h`
- x86_64: `--target=<ターゲットトリプル> -fPIC -fno-stack-protector -ffunction-sections -fdata-sections` を追加し、`llvm-ar` で静的ライブラリにする。
  `<ターゲットトリプル>` は、cargo が環境変数 `TARGET` で渡すターゲットトリプルである。
- wasm32: `--target=wasm32 -nostdlib` を追加し、`wasm-ld --no-entry -z stack-size=1048576` と `--export=krt_...` で `rt.wasm` にする。
  `krt_` で始まるシンボルのほかに、`__heap_base` も export する。

共通のオプションには、このほかに警告の設定(`-Wall` と、いくつかの `-Wno-...`)が加わる。

`-include shim/krt_names.h` はすべてのソースに渡す。
`krt_names.h` は、musl のシンボルの名前を `krt_` で始まる名前に変える。
あわせて、`__FP_FAST_FMA` か `__FMA__` が定義されるビルド(`-mfma`、`-march=native` など)を `#error` で止める。
`f64show.c`、`shape.c`、`bytes.c` には名前を変える対象のシンボルがないが、`krt_names.h` はこれらのソースでも FMA を検査する。

どちらのターゲットでも、build.rs が clang を直接呼び出し、上のオプションだけを渡す。
`CFLAGS` などの環境変数は読まない。

## テスト

`rustkeleut/` で次のコマンドを実行する。

```sh
cargo test -p keleut-rtcore
node crates/keleut-rtcore/tests/wasm_check.mjs
```

`cargo test -p keleut-rtcore` は x86_64 版をテストする。
`show(Float64)` の結果は、glibc の `snprintf` で作った期待値と比較する。
NaN だけは glibc に渡さず、`nan` と比較する。
`sin cos exp log` は、glibc との差分を参考として表示するだけで、合否には使わない。
このテストは、入力と、wasm32 版の検査に使う値(`show(Float64)` の期待値と、`sin cos exp log` の x86_64 版の結果)を `target/rtcore/` に書く。
件数とビルドの情報を書いた `fltcmp-meta.json` と `libm-meta.json` も同じディレクトリに書く。
`wasm_check.mjs` はこれらのファイルを読み、`rt.wasm` を Node で検査する。
