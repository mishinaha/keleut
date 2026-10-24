# rtcore

RustKeleut の全後段が共有する、自立した C99 の実行時の部品である。
libc にも OS にも依存しない。
同じソースを、`crates/keleut-rtcore/build.rs` が x86_64 の静的ライブラリと wasm32 の `rt.wasm` に組む。

## 置き場所

- `rtcore.h`: C の API と、その約束。
- `f64show.c`: `show(Float64)`(LangSpec 16.2 節)。
  多倍長の整数による正確な 10 進展開と、丸めの区間との正確な比較で、最短の桁数を求める。
- `shape.c`: レコードの形の表(intern、拡張、制限、位置の検索)。
  メモリは呼び出し側が渡す。
- `bytes.c`: 符号なしバイトの辞書順の比較。
- `musl/`: musl 1.2.6 の `sin cos exp log` とその依存を、改変せずに写したもの。
  版、出所、sha256 は `musl/SOURCE.md` に、ライセンスは `musl/COPYRIGHT` にある。
- `shim/`: libc のヘッダの代わりの最小のヘッダと、外部の記号を `krt_` で始まる名前に替える `krt_names.h`。

`musl/` と `shim/` は、M79 の試作 `doc/log/261024-1-spike/rtcore/` から写した。
`shim/` は写した後、コメントを日本語に改め、FMA の検査に `__FMA__` を足した。

## 組み方

旗は次のとおりである。

- 共通: `-std=c99 -O2 -ffreestanding -fno-builtin -ffp-contract=off -fno-math-errno -nostdlibinc -Ishim -Imusl/src/internal -I. -include shim/krt_names.h`
- x86_64: `--target=<対象> -fPIC -fno-stack-protector -ffunction-sections -fdata-sections` を足し、`llvm-ar` で静的ライブラリにする。
- wasm32: `--target=wasm32 -nostdlib` を足し、`wasm-ld --no-entry -z stack-size=1048576` と `--export=krt_...` で `rt.wasm` にする。

`-include shim/krt_names.h` はすべてのソースに渡す。
`krt_names.h` は musl の記号の名前を krt_ 付きに替え、あわせて `__FP_FAST_FMA` か `__FMA__` が定義された組み方(`-mfma`、`-march=native` など)を `#error` で止める。
名前の置き換えは `f64show.c`、`shape.c`、`bytes.c` の記号には当たらないが、FMA の検査はすべてのソースに効く。

どちらの対象も build.rs が clang を直に呼び、上の旗だけを渡す。
`CFLAGS` などの環境変数は読まない。

## 試験

`rustkeleut/` で次のコマンドを実行する。

```sh
cargo test -p keleut-rtcore
node crates/keleut-rtcore/tests/wasm_check.mjs
```

前者は x86_64 版を glibc と比べ、入力と基準を `target/rtcore/` に書く。
後者はそれを読み、`rt.wasm` を Node で確かめる。
