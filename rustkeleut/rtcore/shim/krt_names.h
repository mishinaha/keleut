/* rtcore: musl から写した翻訳単位はすべて -include krt_names.h で組み、
   定義または参照する外部の記号に krt_ を付ける。
   musl のソースは改変しない。
   こうすると、x86_64 のオブジェクトが組み込み先の libc(glibc の sin、__sin など)とぶつからず、
   LLVM もこれらの関数を標準の関数とみなさない。 */
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

/* exp.c と log.c は、__FP_FAST_FMA が定義されたときだけ fma を使う。
   2 つの対象は同じ分岐を通らなければならず、rtcore は融合積和の無い基準の x86_64 と wasm32 に向けて組む。
   -march=native や -mfma で組むと __FMA__ だけが定義されることがあるので、それも止める。
   その組み方では、-ffp-contract=off が上書きされたときに積和が融合されうる。 */
#if defined(__FP_FAST_FMA) || defined(__FMA__)
#error "rtcore: __FP_FAST_FMA and __FMA__ must not be defined (build without -mfma or -march=native)"
#endif

#endif
