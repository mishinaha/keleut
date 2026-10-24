/* rtcore: replaces musl's src/include/features.h (the internal one that
   libm.h, exp_data.h and log_data.h include). */
#ifndef FEATURES_H
#define FEATURES_H

#define hidden __attribute__((__visibility__("hidden")))

#endif
