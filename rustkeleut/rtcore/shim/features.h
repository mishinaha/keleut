/* rtcore: musl の内部ヘッダ src/include/features.h の代わりで、hidden だけを定義する。
   このヘッダは、exp_data.h と log_data.h が直接読み、libm.h が shim の math.h を通して読む。 */
#ifndef FEATURES_H
#define FEATURES_H

#define hidden __attribute__((__visibility__("hidden")))

#endif
