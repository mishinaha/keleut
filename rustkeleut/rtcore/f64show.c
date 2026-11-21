/* rtcore: Float64 の文字列化(LangSpec §16.2)。

   有限で整数でない値と、絶対値が 1e16 以上の整数値は、次の方法で書く。
   有効数字の桁数 p を 1 から 17 まで増やし、p 桁に丸めた 10 進の値が x の丸めの区間に入る最初の p で書く。
   丸めの区間とは、最近接偶数への丸めで読み戻すと x になる実数の集合である。
   p 桁に丸めた値が丸めの区間に入ることは、その値を読み戻すと x になることと同値である。
   そこで、strtod を使わずに、区間の端との正確な比較で判定する。

   区間の端は、x と隣の浮動小数点数との中点である。
   x の絶対値を m × 2^e(m は整数)と書くと、上側の端は (2m + 1) × 2^(e-1) である。
   下側の端は通常 (2m - 1) × 2^(e-1) である。
   ただし、x が 2^-1022 より大きい 2 の冪のときは、すぐ下の浮動小数点数との間隔が半分なので (4m - 1) × 2^(e-2) になる。
   以下ではこれを狭い場合と呼ぶ。
   最小の正規数 2^-1022 は、すぐ下の浮動小数点数が最大の非正規化数で間隔が変わらないので、狭い場合に含めない。
   区間が端を含むのは m が偶数のときで、このとき端の値は偶数への丸めで x になる。

   そこで、狭い場合は s = e - 2、それ以外は s = e - 1 とし、F を s ≥ 0 なら 2^s、s < 0 なら 5^-s とする。
   狭い場合は X = 4m、それ以外は X = 2m とする。
   すると、x と下側の端は、整数 X × F と (X - 1) × F に 10^min(s, 0) を掛けた値になる。
   上側の端は、狭い場合は整数 (X + 2) × F に、それ以外は (X + 1) × F に、同じ 10^min(s, 0) を掛けた値になる。
   3 つは同じ 10 の冪を単位とする整数なので、候補の d × 10^q も同じ単位の整数に直して比較すればよい。
   x の正確な 10 進展開は X × F の 10 進の桁そのものであり、p 桁への丸めはこの桁の並びから偶数への丸めで作る。
   これは glibc の %.*g と同じ丸めである。

   多倍長整数は、10^9 を基数とする 32 ビットの桁の配列で表す。
   最大の値は 5^1075 × 2^55 より小さく、10 進で 768 桁以下である。 */
#include "rtcore.h"

#define BIG_LIMBS 96
#define BASE 1000000000u
#define MAX_DIGITS (BIG_LIMBS * 9)

typedef struct {
  uint32_t n;              /* 使っている桁の個数(値が 0 なら 0) */
  uint32_t d[BIG_LIMBS];   /* 下位の桁が先 */
} big;

static void big_set_u64(big *b, uint64_t v) {
  b->n = 0;
  while (v != 0) {
    b->d[b->n++] = (uint32_t)(v % BASE);
    v /= BASE;
  }
}

static void big_copy(big *dst, const big *src) {
  dst->n = src->n;
  for (uint32_t i = 0; i < src->n; i++) dst->d[i] = src->d[i];
}

/* b に m を掛ける(m < 2^32)。 */
static void big_mul_small(big *b, uint32_t m) {
  uint64_t carry = 0;
  for (uint32_t i = 0; i < b->n; i++) {
    uint64_t t = (uint64_t)b->d[i] * m + carry;
    b->d[i] = (uint32_t)(t % BASE);
    carry = t / BASE;
  }
  while (carry != 0) {
    b->d[b->n++] = (uint32_t)(carry % BASE);
    carry /= BASE;
  }
}

static void big_mul_pow2(big *b, uint32_t k) {
  for (; k >= 31; k -= 31) big_mul_small(b, 1u << 31);
  if (k > 0) big_mul_small(b, 1u << k);
}

static void big_mul_pow5(big *b, uint32_t k) {
  /* 5^13 = 1220703125 < 2^32 */
  static const uint32_t p5[14] = {1u, 5u, 25u, 125u, 625u, 3125u, 15625u, 78125u, 390625u,
                                  1953125u, 9765625u, 48828125u, 244140625u, 1220703125u};
  for (; k >= 13; k -= 13) big_mul_small(b, p5[13]);
  if (k > 0) big_mul_small(b, p5[k]);
}

/* b に 10^k を掛ける。 */
static void big_mul_pow10(big *b, uint32_t k) {
  static const uint32_t p10[9] = {1u, 10u, 100u, 1000u, 10000u, 100000u, 1000000u, 10000000u, 100000000u};
  uint32_t limbs = k / 9;
  if (b->n == 0) return;
  big_mul_small(b, p10[k % 9]);
  if (limbs > 0) {
    for (uint32_t i = b->n; i-- > 0;) b->d[i + limbs] = b->d[i];
    for (uint32_t i = 0; i < limbs; i++) b->d[i] = 0;
    b->n += limbs;
  }
}

/* dst = a × v(v < 10^18)。 */
static void big_mul_u64(big *dst, const big *a, uint64_t v) {
  uint32_t v0 = (uint32_t)(v % BASE), v1 = (uint32_t)(v / BASE);
  uint32_t n = a->n + 2;
  for (uint32_t i = 0; i < n; i++) dst->d[i] = 0;
  for (uint32_t i = 0; i < a->n; i++) {
    uint64_t t0 = (uint64_t)a->d[i] * v0 + dst->d[i];
    dst->d[i] = (uint32_t)(t0 % BASE);
    uint64_t t1 = (uint64_t)a->d[i] * v1 + dst->d[i + 1] + t0 / BASE;
    dst->d[i + 1] = (uint32_t)(t1 % BASE);
    uint64_t carry = t1 / BASE;
    for (uint32_t j = i + 2; carry != 0; j++) {
      uint64_t t = (uint64_t)dst->d[j] + carry;
      dst->d[j] = (uint32_t)(t % BASE);
      carry = t / BASE;
    }
  }
  while (n > 0 && dst->d[n - 1] == 0) n--;
  dst->n = n;
}

/* a += b × k(k は 1 か 2)。 */
static void big_add_mul(big *a, const big *b, uint32_t k) {
  uint64_t carry = 0;
  uint32_t n = a->n > b->n ? a->n : b->n;
  for (uint32_t i = 0; i < n; i++) {
    uint64_t t = (i < a->n ? a->d[i] : 0) + (uint64_t)(i < b->n ? b->d[i] : 0) * k + carry;
    a->d[i] = (uint32_t)(t % BASE);
    carry = t / BASE;
  }
  if (carry != 0) a->d[n++] = (uint32_t)carry;
  a->n = n;
}

/* a -= b(a ≥ b)。 */
static void big_sub(big *a, const big *b) {
  int64_t borrow = 0;
  for (uint32_t i = 0; i < a->n; i++) {
    int64_t t = (int64_t)a->d[i] - (i < b->n ? b->d[i] : 0) - borrow;
    borrow = t < 0;
    if (t < 0) t += BASE;
    a->d[i] = (uint32_t)t;
  }
  while (a->n > 0 && a->d[a->n - 1] == 0) a->n--;
}

static int big_cmp(const big *a, const big *b) {
  if (a->n != b->n) return a->n < b->n ? -1 : 1;
  for (uint32_t i = a->n; i-- > 0;)
    if (a->d[i] != b->d[i]) return a->d[i] < b->d[i] ? -1 : 1;
  return 0;
}

/* b の 10 進の桁を上位から digits に書き、桁数を返す(b > 0)。
   digits の各要素は文字ではなく、0 から 9 の値である。 */
static uint32_t big_digits(const big *b, char *digits) {
  uint32_t len = 0, top = b->d[b->n - 1];
  char tmp[9];
  uint32_t k = 0;
  while (top != 0) {
    tmp[k++] = (char)(top % 10);
    top /= 10;
  }
  while (k > 0) digits[len++] = tmp[--k];
  for (uint32_t i = b->n - 1; i-- > 0;) {
    uint32_t v = b->d[i];
    for (int j = 8; j >= 0; j--) {
      digits[len + (uint32_t)j] = (char)(v % 10);
      v /= 10;
    }
    len += 9;
  }
  return len;
}

static size_t put_str(char *out, size_t at, const char *s) {
  while (*s) out[at++] = *s++;
  return at;
}

static size_t put_u64(char *out, size_t at, uint64_t v) {
  char tmp[20];
  int k = 0;
  do {
    tmp[k++] = (char)('0' + v % 10);
    v /= 10;
  } while (v != 0);
  while (k > 0) out[at++] = tmp[--k];
  return at;
}

/* p 個の 10 進の桁の並び ds(先頭は 0 でない)と、先頭の桁の 10 進の指数 x10 を、%.*g の形式で out に書く。
   ただし LangSpec §16.2 の表記に合わせて、指数の + と先頭の 0 を書かず、小数点も指数もなければ .0 を付ける。 */
static size_t format_g(char *out, size_t at, const char *ds, int p, int x10) {
  int nd = p;
  int has_point = 0;
  while (nd > 1 && ds[nd - 1] == 0) nd--;
  if (x10 < -4 || x10 >= p) {
    out[at++] = (char)('0' + ds[0]);
    if (nd > 1) {
      out[at++] = '.';
      for (int i = 1; i < nd; i++) out[at++] = (char)('0' + ds[i]);
    }
    out[at++] = 'e';
    if (x10 < 0) out[at++] = '-';
    return put_u64(out, at, (uint64_t)(x10 < 0 ? -x10 : x10));
  }
  if (x10 >= 0) {
    for (int i = 0; i <= x10; i++) out[at++] = (char)('0' + ds[i]);
    if (nd > x10 + 1) {
      out[at++] = '.';
      has_point = 1;
      for (int i = x10 + 1; i < nd; i++) out[at++] = (char)('0' + ds[i]);
    }
  } else {
    out[at++] = '0';
    out[at++] = '.';
    has_point = 1;
    for (int i = 0; i < -x10 - 1; i++) out[at++] = '0';
    for (int i = 0; i < nd; i++) out[at++] = (char)('0' + ds[i]);
  }
  if (!has_point) at = put_str(out, at, ".0");
  return at;
}

size_t krt_f64_show_bits(uint64_t bits, char *out) {
  int neg = (int)(bits >> 63);
  uint32_t be = (uint32_t)(bits >> 52) & 0x7ffu;
  uint64_t frac = bits & 0xfffffffffffffull;
  size_t at = 0;
  uint64_t m;
  int e;

  /* 非有限値は、整数値の分岐より前に、決まった文字列で書く。
     NaN の符号は書かない。 */
  if (be == 0x7ffu) {
    at = put_str(out, 0, frac != 0 ? "nan" : neg ? "-inf" : "inf");
    out[at] = 0;
    return at;
  }
  if (neg) out[at++] = '-';
  if (be == 0) {
    m = frac;
    e = -1074;
  } else {
    m = frac | (1ull << 52);
    e = (int)be - 1075;
  }

  /* 絶対値が 1e16 未満の整数値(0 と -0 を含む)は %.1f の形式で書く。 */
  {
    int is_int = 0;
    uint64_t iv = 0;
    if (m == 0) {
      is_int = 1;
    } else if (e >= 0) {
      if (e <= 10 && (m << e) < 10000000000000000ull) {
        is_int = 1;
        iv = m << e;
      }
    } else if (e >= -52 && (m & ((1ull << -e) - 1)) == 0) {
      is_int = 1;
      iv = m >> -e;
    }
    if (is_int) {
      at = put_u64(out, at, iv);
      at = put_str(out, at, ".0");
      out[at] = 0;
      return at;
    }
  }

  {
    int narrow = frac == 0 && be > 1;
    int s = narrow ? e - 2 : e - 1;
    uint64_t X = narrow ? 4 * m : 2 * m;
    int inclusive = (m & 1) == 0;
    int t = s < 0 ? s : 0; /* 3 つの整数の単位は 10^t */
    big f, nx, nlo, nhi, cand;
    char digits[MAX_DIGITS];
    uint32_t len, lastnz;
    int x10_exact;
    char ds[17];

    big_set_u64(&f, 1);
    if (s >= 0)
      big_mul_pow2(&f, (uint32_t)s);
    else
      big_mul_pow5(&f, (uint32_t)-s);
    big_mul_u64(&nx, &f, X);
    big_copy(&nlo, &nx);
    big_sub(&nlo, &f);
    big_copy(&nhi, &nx);
    big_add_mul(&nhi, &f, narrow ? 2 : 1);

    len = big_digits(&nx, digits);
    lastnz = len - 1;
    while (digits[lastnz] == 0) lastnz--;
    x10_exact = (int)len - 1 + t;

    for (int p = 1; p <= 17; p++) {
      uint64_t d = 0;
      int x10 = x10_exact;
      int exact;
      for (int i = 0; i < p; i++) d = d * 10 + (uint32_t)(i < (int)len ? digits[i] : 0);
      exact = (uint32_t)p > lastnz;
      if (!exact) {
        /* p 桁目より下に 0 でない桁がある。
           偶数への丸めで p 桁にし、丸めの区間と比較する。 */
        int r = digits[p];
        int rest = lastnz > (uint32_t)p;
        uint64_t pow = 1;
        int lo_ok, hi_ok, c;
        for (int i = 0; i < p; i++) pow *= 10;
        if (r > 5 || (r == 5 && (rest || (d & 1)))) {
          d++;
          if (d == pow) {
            d = pow / 10;
            x10++;
          }
        }
        /* 候補は d × 10^(x10 - p + 1) で、単位 10^t では d × 10^(x10 - p + 1 - t) である。
           p < len なので、この指数は正である。 */
        big_set_u64(&cand, d);
        big_mul_pow10(&cand, (uint32_t)(x10 - p + 1 - t));
        c = big_cmp(&cand, &nlo);
        lo_ok = inclusive ? c >= 0 : c > 0;
        c = big_cmp(&cand, &nhi);
        hi_ok = inclusive ? c <= 0 : c < 0;
        if (!(lo_ok && hi_ok) && p < 17) continue;
      }
      for (int i = p; i-- > 0;) {
        ds[i] = (char)(d % 10);
        d /= 10;
      }
      at = format_g(out, at, ds, p, x10);
      break;
    }
  }
  out[at] = 0;
  return at;
}

size_t krt_f64_show(double x, char *out) {
  union {
    double d;
    uint64_t u;
  } v;
  v.d = x;
  return krt_f64_show_bits(v.u, out);
}

static char scratch[KRT_SCRATCH_SIZE];

char *krt_scratch(void) { return scratch; }
