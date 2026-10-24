/* rtcore の libm の試験の x86_64 側。
   決定的な入力の集合を作り、rtcore の krt_sin などの結果と、
   比較のために glibc の sin などの結果を求める。
   入力と krt_* の結果をファイルに書き、wasm32 側(check.mjs)が読む。

   使い方: native OUTDIR
   OUTDIR/inputs.bin   入力(N 個の f64、リトルエンディアン)
   OUTDIR/native.bin   krt_sin, krt_cos, krt_exp, krt_log の結果(関数ごとに N 個) */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

double krt_sin(double);
double krt_cos(double);
double krt_exp(double);
double krt_log(double);

#define N 1000000

static uint64_t st = 0x9e3779b97f4a7c15ULL;
static uint64_t rnd(void) {
  st ^= st >> 12;
  st ^= st << 25;
  st ^= st >> 27;
  return st * 0x2545f4914f6cdd1dULL;
}
static double asd(uint64_t u) { double d; memcpy(&d, &u, 8); return d; }
static uint64_t asu(double d) { uint64_t u; memcpy(&u, &d, 8); return u; }

static double in[N];
static size_t n = 0;
static void add(double x) { if (n < N) in[n++] = x; }
/* x と、ビット列で前後 k 個の値を足す(符号の境は越えない)。 */
static void add_nb(double x, int k) {
  uint64_t u = asu(x);
  add(x);
  for (int i = 1; i <= k; i++) {
    if ((u & 0x7fffffffffffffffULL) + i <= 0x7ff0000000000000ULL) add(asd(u + i));
    if ((u & 0x7fffffffffffffffULL) >= (uint64_t)i) add(asd(u - i));
  }
}
/* 符号、指数(偏りを含む)、仮数を乱数で作る。 */
static double rnd_exp(int lo, int hi) {
  uint64_t e = (uint64_t)(lo + (int)(rnd() % (uint64_t)(hi - lo + 1)));
  uint64_t m = rnd() & 0xfffffffffffffULL;
  uint64_t s = rnd() & 0x8000000000000000ULL;
  return asd(s | (e << 52) | m);
}
static double uni(double lo, double hi) {
  return lo + (hi - lo) * ((double)(rnd() >> 11) * 0x1p-53);
}

static void gen(void) {
  static const uint64_t specials[] = {
    0x0000000000000000ULL, 0x8000000000000000ULL, /* ±0 */
    0x7ff0000000000000ULL, 0xfff0000000000000ULL, /* ±inf */
    0x7ff8000000000000ULL, 0xfff8000000000000ULL, /* 静かな NaN */
    0x7ff0000000000001ULL, 0xfff0000000000001ULL, /* 信号の NaN */
    0x7ff4000000000000ULL, 0x7fffffffffffffffULL, 0xffffffffffffffffULL,
    0x7ff8000000000123ULL,
    0x0000000000000001ULL, 0x8000000000000001ULL, /* 最小の非正規化数 */
    0x000fffffffffffffULL, 0x800fffffffffffffULL, /* 最大の非正規化数 */
    0x0010000000000000ULL, 0x8010000000000000ULL, /* 最小の正規数 */
    0x7fefffffffffffffULL, 0xffefffffffffffffULL, /* 最大の有限値 */
    0x3ff0000000000000ULL, 0xbff0000000000000ULL, /* ±1 */
    0x3fe0000000000000ULL, 0x4000000000000000ULL,
    0x40862e42fefa39efULL, /* exp が溢れる境 709.78... */
    0xc0874910d52d3051ULL, /* exp が 0 になる境 -745.13... */
    0xc086232bdd7abcd2ULL, /* exp の結果が非正規化数になる境 -708.39... */
    0x3fe921fb54442d18ULL, 0xbfe921fb54442d18ULL, /* ±pi/4 */
    0x3ff921fb54442d18ULL, 0x400921fb54442d18ULL, 0x401921fb54442d18ULL,
    0x3e40000000000000ULL, 0x3e50000000000000ULL, /* 2^-27、2^-26 */
    0x413921fb54442d18ULL, 0x413921fb54442d19ULL, /* 2^20 pi/2 の付近 */
    0x4330000000000000ULL, 0x4340000000000000ULL, /* 2^52、2^53 */
    0x3fee000000000000ULL, 0x3ff1000000000000ULL, /* log の近傍の境 */
    0x3c90000000000000ULL, 0x3ca0000000000000ULL,
  };
  for (size_t i = 0; i < sizeof specials / sizeof specials[0]; i++) add_nb(asd(specials[i]), 16);
  /* 引数の簡約がもっとも難しい値 6381956970095103 * 2^797 とその近傍。 */
  add_nb(asd(((uint64_t)(797 + 52 + 1023) << 52) | (6381956970095103ULL & 0xfffffffffffffULL)), 64);

  /* pi/2 の倍数の付近。小さい倍数から 2^1000 の大きさまで。 */
  while (n < 200000) {
    int e = (int)(rnd() % 64);
    double k = (double)(rnd() >> (64 - 1 - e)) + 1.0;
    double x = k * 0x1.921fb54442d18p0;
    if (rnd() & 1) x = ldexp(x, (int)(rnd() % 960));
    if (rnd() & 1) x = -x;
    add_nb(x, 2);
  }
  /* 64 ビットの一様な乱数のビット列。 */
  while (n < 500000) add(asd(rnd()));
  /* 絶対値が 2^-60 から 2^60 の値。 */
  while (n < 700000) add(rnd_exp(1023 - 60, 1023 + 60));
  /* exp の意味のある範囲。 */
  while (n < 800000) add(uni(-760.0, 720.0));
  /* log の 1 の近傍。 */
  while (n < 850000) add(1.0 + ldexp(uni(-1.0, 1.0), -(int)(rnd() % 50)));
  /* 非正規化数。 */
  while (n < 900000) add(asd((rnd() & 0x800fffffffffffffULL) | 1));
  /* 大きな引数(2^20 以上)。 */
  while (n < 950000) add(rnd_exp(1023 + 20, 2046));
  /* 小さな引数(2^-30 未満の正規数)。 */
  while (n < N) add(rnd_exp(1, 1023 - 30));
}

static int same(double a, double b) {
  return asu(a) == asu(b) || (isnan(a) && isnan(b));
}

int main(int argc, char **argv) {
  if (argc != 2) { fprintf(stderr, "usage: native OUTDIR\n"); return 2; }
  gen();
  static double (*const krt[4])(double) = { krt_sin, krt_cos, krt_exp, krt_log };
  /* volatile にして、glibc の関数の呼び出しを畳み込ませない。 */
  double (*volatile glibc[4])(double) = { sin, cos, exp, log };
  static const char *const name[4] = { "sin", "cos", "exp", "log" };
  static double out[4][N];
  char path[4096];
  snprintf(path, sizeof path, "%s/inputs.bin", argv[1]);
  FILE *f = fopen(path, "wb");
  if (!f || fwrite(in, 8, N, f) != N || fclose(f)) { perror(path); return 2; }
  printf("inputs: %zu\n", n);
  printf("vs glibc (informative; NaN counts as equal to NaN):\n");
  for (int j = 0; j < 4; j++) {
    long d = 0, d1 = 0;
    for (size_t i = 0; i < N; i++) {
      double r = krt[j](in[i]);
      double g = glibc[j](in[i]);
      out[j][i] = r;
      if (!same(r, g)) {
        d++;
        uint64_t a = asu(r), b = asu(g);
        if (a - b == 1 || b - a == 1) d1++;
      }
    }
    printf("  %s: %ld differ (%ld by 1 ulp)\n", name[j], d, d1);
  }
  snprintf(path, sizeof path, "%s/native.bin", argv[1]);
  f = fopen(path, "wb");
  if (!f) { perror(path); return 2; }
  for (int j = 0; j < 4; j++)
    if (fwrite(out[j], 8, N, f) != N) { perror(path); return 2; }
  if (fclose(f)) { perror(path); return 2; }
  return 0;
}
