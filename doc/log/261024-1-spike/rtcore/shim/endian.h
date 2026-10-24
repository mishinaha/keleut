/* rtcore: the minimal <endian.h> for musl's libm.h.  Both targets
   (x86_64 and wasm32) are little endian. */
#ifndef _ENDIAN_H
#define _ENDIAN_H

#define __LITTLE_ENDIAN 1234
#define __BIG_ENDIAN 4321
#if !defined(__BYTE_ORDER__) || __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error "rtcore: little-endian target required"
#endif
#define __BYTE_ORDER __LITTLE_ENDIAN

#endif
