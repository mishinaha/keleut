/* rtcore: the minimal <math.h> that the imported musl sources need.
   Only the names they use are provided; see krt_names.h for renaming. */
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
