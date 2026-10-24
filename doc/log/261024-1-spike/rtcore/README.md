# rtcore の試作: musl の libm

計画 261024-1 の M79 の試作である。
musl 1.2.6 の `sin cos exp log` を、libc なしで clang により x86_64 と wasm32 に組み、2 つの結果がビット列で一致するかを確かめる(台帳の V200)。
M85 の `rustkeleut/rtcore/` は、この組み方を引き継ぐ。

## 置き場所

- `musl/`: 上流から改変せずに写したソースと `COPYRIGHT`。
  版、入手先、sha256 は `musl/SOURCE.md` にある。
- `shim/`: libc のヘッダの代わりの最小のヘッダと、記号の名前を替える `krt_names.h`。
- `test/native.c`: x86_64 側の駆動部。
  入力を作り、`krt_*` と glibc の結果を求める。
- `test/check.mjs`: wasm32 側の駆動部。
  Node で `rt.wasm` を呼び、x86_64 版の結果と比べる。
- `Makefile`: 組み方と `check`。
  生成物は `out/` に置く(`rtcore/.gitignore` が無視する)。

## 実行

```sh
make -C doc/log/261024-1-spike/rtcore check
make -C doc/log/261024-1-spike/rtcore clean
```

`check` は次の順に進み、どれかが満たされなければ 0 でない状態で終わる。

1. `nm -u out/krt_musl.o` が空である(x86_64 版が libc の記号を参照しない)。
2. x86_64 版が定義する大域の記号が、すべて `krt_` で始まる。
3. `out/native` が 1,000,000 個の入力を作り、glibc との差を参考として出す。
4. `node test/check.mjs out` が `rt.wasm` の import が 0 個であることと、4 つの関数の x86_64 版との差の件数を出し、差があれば 1 で終わる。

## 組み方

計画 6.5 節の旗 `-std=c99 -O2 -ffreestanding -fno-builtin -ffp-contract=off -fno-math-errno` をすべてそのまま使い、wasm32 では `--target=wasm32 -nostdlib` を足す。
`rt.wasm` は `wasm-ld --no-entry --export=krt_sin --export=krt_cos --export=krt_exp --export=krt_log` で作る。

計画の旗に加えた旗と、その理由は次のとおりである。

- `-fno-stack-protector`(x86_64 だけ): この機械の clang は既定でスタックの保護を有効にし、`sin.c`、`cos.c`、`__rem_pio2.c`、`__rem_pio2_large.c` の 4 つが `__stack_chk_fail` を参照した。
- `-nostdlibinc -Ishim -Imusl/src/internal`: libc のヘッダを読まない。
  `stdint.h` と `float.h` は clang の組み込みのものを使い、`math.h`、`endian.h`、`features.h`、`fp_arch.h` は `shim/` のものを使う。
- `-include shim/krt_names.h`: musl のソースを改変せずに、外部の記号をすべて `krt_` で始まる名前にする(`sin` を `krt_sin`、`__rem_pio2` を `krt___rem_pio2` など)。
  x86_64 版が glibc の記号と衝突せず、LLVM も標準の関数とみなさない。
- `-Wall` と `-Wno-*`: musl の書き方に出る警告を抑えるだけで、生成するコードは変わらない。

`exp.c` と `log.c` は、`__FP_FAST_FMA` が定義されているときだけ `fma` を使う。
2 つの対象が同じ分岐を通るように、`krt_names.h` は `__FP_FAST_FMA` が定義されていれば `#error` にする。
`shim/math.h` も、`FLT_EVAL_METHOD` が 0 でなければ `#error` にする。

## 入力

`test/native.c` が xorshift の固定の種で決定的に作る 1,000,000 個である。

- 特殊な値と境界の値(±0、±inf、静かな NaN と信号の NaN、非正規化数の最小と最大、正規数の最小、最大の有限値、exp の溢れの境、pi/4 の付近、2^20 pi/2 の付近など)と、そのビット列で前後 16 個の値。
- 引数の簡約がもっとも難しい 6381956970095103 × 2^797 と、その前後 64 個の値。
- pi/2 の倍数の付近(2^1000 程度の大きさまで、前後 2 個の値を含む)。
  合わせて 200,000 個までである。
- 64 ビットの一様な乱数のビット列 300,000 個。
- 絶対値が 2^-60 から 2^60 の値 200,000 個、[-760, 720] の一様な値 100,000 個、1 の近傍 50,000 個、非正規化数 50,000 個、2^20 以上の値 50,000 個、2^-30 未満の正規数 50,000 個。

## 比べ方

非 NaN の結果はビット列で比べる。
NaN どうしは、符号と積載に依らず一致とみなす。
Keleut の `show` は NaN を常に `nan` と書くからである。
入力と結果は JS の `Number` を通るので、NaN の積載の一致はこの試験では確かめられない。

## 結果(2026-10-24)

clang 22.1.8、LLD 22.1.8、Node 26.9.0、glibc 2.44 で測った。

| 関数 | x86_64 と wasm32 の差 | glibc との差(参考) |
|---|---|---|
| `sin` | 0 | 19,378(すべて 1 ulp) |
| `cos` | 0 | 17,818(すべて 1 ulp) |
| `exp` | 0 | 138(すべて 1 ulp) |
| `log` | 0 | 2(すべて 1 ulp) |

`nm -u out/krt_musl.o` は空で、`rt.wasm` は 17,792 バイト、import は 0 個である。
`native.bin` の 2 か所のビットを反転させると、`check.mjs` は 2 件の差を出して 1 で終わった。
