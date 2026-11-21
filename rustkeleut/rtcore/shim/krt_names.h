/* rtcore: musl からコピーしたソースは、どれも -include krt_names.h を付けてコンパイルする。
   このヘッダは、ソースが定義または参照する外部シンボルの名前を、krt_ で始まる名前に置き換える。
   musl のソースは改変しない。
   こうすると、x86_64 では、オブジェクトファイルのシンボルが、リンクする libc のシンボル(glibc の sin、__sin など)と衝突しない。
   LLVM も、これらの関数を標準ライブラリの関数とみなさない。 */
#ifndef KRT_NAMES_H
#define KRT_NAMES_H

#define sin krt_sin
#define cos krt_cos
#define exp krt_exp
#define log krt_log
#define floor krt_floor
#define scalbn krt_scalbn

#define __sin krt___sin
#define __cos krt___cos
#define __rem_pio2 krt___rem_pio2
#define __rem_pio2_large krt___rem_pio2_large
#define __exp_data krt___exp_data
#define __log_data krt___log_data
#define __math_oflow krt___math_oflow
#define __math_uflow krt___math_uflow
#define __math_xflow krt___math_xflow
#define __math_divzero krt___math_divzero
#define __math_invalid krt___math_invalid

/* log.c は __FP_FAST_FMA が 0 でない値に定義されているときだけ __builtin_fma を使い、log_data.h と log_data.c は同じ条件で表 tab2 を省く。
   2 つのターゲットで結果を一致させるには、どちらも同じ分岐を通らなければならない。
   そこで rtcore は、x86_64 も wasm32 も、拡張命令を有効にしない既定の設定でビルドする。
   この設定では融合積和(FMA)の命令が使われない。
   __FP_FAST_FMA が定義されていれば、ビルドをエラーにする。
   -march=native や -mfma を付けてビルドすると __FMA__ だけが定義されることがあるので、__FMA__ が定義されているときもエラーにする。
   これらのオプションを付けたビルドでは、-ffp-contract=off が上書きされたときに、乗算と加算が 1 つの FMA 命令にまとめられることがある。 */
#if defined(__FP_FAST_FMA) || defined(__FMA__)
#error "rtcore: __FP_FAST_FMA and __FMA__ must not be defined (build without -mfma or -march=native)"
#endif

#endif
