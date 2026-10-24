/* rtcore: every translation unit taken from musl is compiled with
   -include krt_names.h, so that each external symbol it defines or
   references gets the krt_ prefix.  The musl sources stay unmodified.
   The x86_64 object then never collides with the libc of the host
   (glibc's sin, __sin and so on), and LLVM does not recognize the
   functions as the standard ones. */
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

/* exp.c and log.c use fma only when __FP_FAST_FMA is defined.  Both
   targets must take the same branch, and rtcore builds for the baseline
   x86_64 and wasm32, which have no fused multiply-add. */
#ifdef __FP_FAST_FMA
#error "rtcore: __FP_FAST_FMA must not be defined (build without -mfma)"
#endif

#endif
