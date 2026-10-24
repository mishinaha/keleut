/* rtcore: 符号なしバイトの辞書順の比較(rtcore.h を参照)。 */
#include "rtcore.h"

int krt_bytes_compare(const uint8_t *a, size_t alen, const uint8_t *b, size_t blen) {
  size_t n = alen < blen ? alen : blen;
  for (size_t i = 0; i < n; i++)
    if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
  return alen < blen ? -1 : alen > blen ? 1 : 0;
}
