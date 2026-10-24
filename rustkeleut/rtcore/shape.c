/* rtcore: レコードの形の表(rtcore.h の「レコードの形」を参照)。

   表は呼び出し側のメモリの上に、見出し、ハッシュの表、形の記録、ラベルの溜めの順に置く。
   ハッシュの表は開番地法で、各枠に形の番号 + 1(0 は空き)を入れる。
   枠の数は 2 の冪で、形の容量の 2 倍以上にするので、表の使用率は 1/2 を超えない。
   形の記録は、ラベルの溜めの中の位置、長さ、ハッシュ値の 3 つである。

   新しい形を作る操作は、まず候補の列をラベルの溜めの末尾に書く。
   同じ列がすでにあれば、溜めの末尾を進めずに既存の番号を返す。
   したがって、表を変えるのは新しい形を足すときだけである。 */
#include "rtcore.h"

typedef struct {
  uint32_t off;
  uint32_t len;
  uint32_t hash;
} shape_rec;

struct krt_shapes {
  uint32_t cap_shapes;
  uint32_t cap_labels;
  uint32_t nshapes;
  uint32_t nlabels;
  uint32_t hmask; /* ハッシュの表の枠の数 - 1 */
  uint32_t pad;
};

static uint32_t hash_cap(uint32_t cap_shapes) {
  uint32_t h = 4;
  while (h < cap_shapes && h < 0x40000000u) h <<= 1;
  return h < cap_shapes ? 0 : h << 1;
}

static uint32_t *slots(const krt_shapes *t) { return (uint32_t *)(uintptr_t)(t + 1); }

static shape_rec *recs(const krt_shapes *t) { return (shape_rec *)(slots(t) + t->hmask + 1); }

static uint32_t *pool(const krt_shapes *t) { return (uint32_t *)(recs(t) + t->cap_shapes); }

size_t krt_shapes_bytes(uint32_t cap_shapes, uint32_t cap_labels) {
  uint32_t h = hash_cap(cap_shapes);
  uint64_t bytes;
  if (cap_shapes == 0 || h == 0) return 0;
  bytes = sizeof(krt_shapes) + (uint64_t)h * 4 + (uint64_t)cap_shapes * sizeof(shape_rec) + (uint64_t)cap_labels * 4;
  if (bytes > (uint64_t)(size_t)-1) return 0;
  return (size_t)bytes;
}

/* 列の FNV-1a のハッシュ値。 */
static uint32_t hash_labels(const uint32_t *l, uint32_t n) {
  uint32_t h = 2166136261u;
  for (uint32_t i = 0; i < n; i++) {
    uint32_t v = l[i];
    for (int k = 0; k < 4; k++) {
      h ^= v & 0xffu;
      h *= 16777619u;
      v >>= 8;
    }
  }
  h ^= n;
  h *= 16777619u;
  return h;
}

/* 溜めの末尾に置いた長さ n の候補の列を intern する。 */
static uint32_t commit(krt_shapes *t, uint32_t n) {
  uint32_t *p = pool(t);
  const uint32_t *cand = p + t->nlabels;
  uint32_t h = hash_labels(cand, n);
  uint32_t *sl = slots(t);
  shape_rec *r = recs(t);
  uint32_t i = h & t->hmask;
  for (;; i = (i + 1) & t->hmask) {
    uint32_t s = sl[i];
    if (s == 0) break;
    {
      const shape_rec *q = &r[s - 1];
      if (q->hash == h && q->len == n) {
        const uint32_t *ql = p + q->off;
        uint32_t k = 0;
        while (k < n && ql[k] == cand[k]) k++;
        if (k == n) return s - 1;
      }
    }
  }
  if (t->nshapes == t->cap_shapes) return KRT_SHAPE_FULL;
  r[t->nshapes].off = t->nlabels;
  r[t->nshapes].len = n;
  r[t->nshapes].hash = h;
  sl[i] = t->nshapes + 1;
  t->nlabels += n;
  return t->nshapes++;
}

krt_shapes *krt_shapes_init(void *mem, uint32_t cap_shapes, uint32_t cap_labels) {
  krt_shapes *t = (krt_shapes *)mem;
  uint32_t h = hash_cap(cap_shapes);
  if (mem == 0 || krt_shapes_bytes(cap_shapes, cap_labels) == 0) return 0;
  t->cap_shapes = cap_shapes;
  t->cap_labels = cap_labels;
  t->nshapes = 0;
  t->nlabels = 0;
  t->hmask = h - 1;
  t->pad = 0;
  {
    uint32_t *sl = slots(t);
    for (uint32_t i = 0; i < h; i++) sl[i] = 0;
  }
  commit(t, 0);
  return t;
}

uint32_t krt_shapes_count(const krt_shapes *t) { return t->nshapes; }

uint32_t krt_shapes_labels_used(const krt_shapes *t) { return t->nlabels; }

uint32_t krt_shape_len(const krt_shapes *t, uint32_t id) {
  return id < t->nshapes ? recs(t)[id].len : KRT_SHAPE_NONE;
}

const uint32_t *krt_shape_labels(const krt_shapes *t, uint32_t id) {
  return id < t->nshapes ? pool(t) + recs(t)[id].off : 0;
}

int krt_shapes_copy(krt_shapes *dst, const krt_shapes *src) {
  const shape_rec *r = recs(src);
  const uint32_t *sp = pool(src);
  if (dst->nshapes != 1 || src->nlabels > dst->cap_labels || src->nshapes > dst->cap_shapes) return -1;
  for (uint32_t id = 1; id < src->nshapes; id++) {
    uint32_t *dp = pool(dst) + dst->nlabels;
    for (uint32_t k = 0; k < r[id].len; k++) dp[k] = sp[r[id].off + k];
    if (commit(dst, r[id].len) != id) return -1;
  }
  return 0;
}

/* 列 l(長さ n、昇順)で label が最初に現れる位置、または入るべき位置。 */
static uint32_t lower_bound(const uint32_t *l, uint32_t n, uint32_t label) {
  uint32_t lo = 0, hi = n;
  while (lo < hi) {
    uint32_t mid = lo + (hi - lo) / 2;
    if (l[mid] < label)
      lo = mid + 1;
    else
      hi = mid;
  }
  return lo;
}

uint32_t krt_shape_find(const krt_shapes *t, uint32_t id, uint32_t label) {
  const uint32_t *l;
  uint32_t n, i;
  if (id >= t->nshapes) return KRT_SHAPE_NONE;
  l = pool(t) + recs(t)[id].off;
  n = recs(t)[id].len;
  i = lower_bound(l, n, label);
  return i < n && l[i] == label ? i : KRT_SHAPE_NONE;
}

uint32_t krt_shape_intern(krt_shapes *t, const uint32_t *labels, uint32_t n, uint32_t *pos) {
  uint32_t *out;
  if (n > t->cap_labels - t->nlabels) return KRT_SHAPE_FULL;
  out = pool(t) + t->nlabels;
  /* 安定な整列。書いた i 番目の位置は、より小さいラベルの数と、前に書いた同じラベルの数の和である。 */
  for (uint32_t i = 0; i < n; i++) {
    uint32_t at = 0;
    for (uint32_t j = 0; j < n; j++)
      if (labels[j] < labels[i] || (labels[j] == labels[i] && j < i)) at++;
    out[at] = labels[i];
    if (pos) pos[i] = at;
  }
  return commit(t, n);
}

uint32_t krt_shape_extend(krt_shapes *t, uint32_t id, uint32_t label, uint32_t *pos) {
  const uint32_t *l;
  uint32_t *out;
  uint32_t n, at, res;
  if (id >= t->nshapes) return KRT_SHAPE_NONE;
  n = recs(t)[id].len;
  if (n == 0xffffffffu) return KRT_SHAPE_NONE;
  if (n + 1 > t->cap_labels - t->nlabels) return KRT_SHAPE_FULL;
  l = pool(t) + recs(t)[id].off;
  out = pool(t) + t->nlabels;
  at = lower_bound(l, n, label);
  for (uint32_t i = 0; i < at; i++) out[i] = l[i];
  out[at] = label;
  for (uint32_t i = at; i < n; i++) out[i + 1] = l[i];
  res = commit(t, n + 1);
  if (res != KRT_SHAPE_FULL) *pos = at;
  return res;
}

uint32_t krt_shape_restrict(krt_shapes *t, uint32_t id, uint32_t label, uint32_t *pos) {
  const uint32_t *l;
  uint32_t *out;
  uint32_t n, at, res;
  at = krt_shape_find(t, id, label);
  if (at == KRT_SHAPE_NONE) return KRT_SHAPE_NONE;
  n = recs(t)[id].len;
  if (n - 1 > t->cap_labels - t->nlabels) return KRT_SHAPE_FULL;
  l = pool(t) + recs(t)[id].off;
  out = pool(t) + t->nlabels;
  for (uint32_t i = 0; i < at; i++) out[i] = l[i];
  for (uint32_t i = at + 1; i < n; i++) out[i - 1] = l[i];
  res = commit(t, n - 1);
  if (res != KRT_SHAPE_FULL) *pos = at;
  return res;
}
