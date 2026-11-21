/* rtcore: musl の libm.h が読む <endian.h> の代わりで、libm.h が使うマクロだけを定義する。
   ターゲットの x86_64 と wasm32 はどちらもリトルエンディアンである。
   それ以外のバイト順のターゲットでは、#error でコンパイルを止める。 */
#ifndef _ENDIAN_H
#define _ENDIAN_H

#define __LITTLE_ENDIAN 1234
#define __BIG_ENDIAN 4321
#if !defined(__BYTE_ORDER__) || __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error "rtcore: little-endian target required"
#endif
#define __BYTE_ORDER __LITTLE_ENDIAN

#endif
