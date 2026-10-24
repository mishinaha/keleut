/* rtcore: musl の libm.h のための最小の <endian.h> である。
   2 つの対象(x86_64 と wasm32)はどちらもリトルエンディアンである。 */
#ifndef _ENDIAN_H
#define _ENDIAN_H

#define __LITTLE_ENDIAN 1234
#define __BIG_ENDIAN 4321
#if !defined(__BYTE_ORDER__) || __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error "rtcore: little-endian target required"
#endif
#define __BYTE_ORDER __LITTLE_ENDIAN

#endif
