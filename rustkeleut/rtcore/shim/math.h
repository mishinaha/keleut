/* rtcore: musl からコピーしたソースが読む <math.h> の代わりである。
   関数は、コピーしたソースが定義するか呼び出すものだけを宣言する。
   libm.h が使う hidden を定義するために features.h も読む。
   float_t は float、double_t は double と定義する。
   この定義が正しいのは FLT_EVAL_METHOD が 0 のときだけなので、0 でないターゲットでは #error でコンパイルを止める。
   関数の名前は krt_names.h が krt_ で始まる名前に置き換える。 */
#ifndef _MATH_H
#define _MATH_H

#include <features.h>
#include <float.h>

#if FLT_EVAL_METHOD != 0
#error "rtcore: FLT_EVAL_METHOD must be 0 on every target"
#endif
typedef float float_t;
typedef double double_t;

#define INFINITY __builtin_inff()
#define NAN __builtin_nanf("")

double sin(double);
double cos(double);
double exp(double);
double log(double);
double floor(double);
double scalbn(double, int);

#endif
