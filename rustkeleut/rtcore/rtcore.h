/* rtcore: RustKeleut の全後段が共有する、自立した C99 の実行時の部品。

   libc にも OS にも依存しない。
   同じソースを x86_64 の静的ライブラリと wasm32 の rt.wasm に組み、両者は同じ結果を返す。
   外部に見える記号は、すべて krt_ で始まる。

   部品は次の 4 つである。

   - f64show.c: Float64 の文字列化(LangSpec 16.2 節)。
   - shape.c: レコードの形の intern、拡張、制限、位置の検索。
   - bytes.c: 符号なしバイトの辞書順の比較。
   - musl/: musl の sin cos exp log(krt_sin などの名前に替えて組む)。

   wasm32 のホストは、線形メモリの上の領域を指す 32 ビットの番地で引数を渡す。
   入力のバイト列や出力の文字列の置き場所には、krt_scratch が返す領域か、__heap_base より上の領域を使う。 */
#ifndef KRT_RTCORE_H
#define KRT_RTCORE_H

#include <stddef.h>
#include <stdint.h>

/* ---- libm(musl 1.2.6) ----
   LLVM が標準の関数とみなさないよう、名前に krt_ を付けてある。
   結果は x86_64 と wasm32 でビット列が一致する。 */
double krt_sin(double x);
double krt_cos(double x);
double krt_exp(double x);
double krt_log(double x);

/* ---- Float64 の文字列化 ----
   krt_f64_show は x を LangSpec 16.2 節の表記で out に書き、書いたバイト数を返す。
   out には KRT_F64_SHOW_MAX バイトの領域が要る。
   書いた文字列の後ろに NUL を 1 バイト置くが、返すバイト数には含めない。

   NaN は符号と積載に依らず nan、無限大は inf と -inf になる。
   絶対値が 1e16 未満の整数値は %.1f の形(-0.0 を含む)になる。
   それ以外は、読み戻すと x になる最短の有効数字の桁数 p(1 から 17)で %.*g の形に丸め、
   指数の + と先頭の 0 を除き、小数点も指数もなければ .0 を足す。

   krt_f64_show_bits は x のビット列を受け取る。
   wasm32 のホストが JS の Number を通さずに NaN の符号と積載を渡すための入口である。 */
#define KRT_F64_SHOW_MAX 32
size_t krt_f64_show(double x, char *out);
size_t krt_f64_show_bits(uint64_t bits, char *out);

/* ---- 作業領域 ----
   KRT_SCRATCH_SIZE バイトの静的な領域の先頭を返す。
   wasm32 のホストが、引数のバイト列と結果の文字列を受け渡すのに使う。
   x86_64 では使わなくてよい(大域の状態なので、複数のスレッドから同時に使ってはならない)。 */
#define KRT_SCRATCH_SIZE 65536
char *krt_scratch(void);

/* ---- バイト列の比較 ----
   a と b を、バイトを符号なしの値とみなした辞書順で比べる。
   a が小さければ -1、等しければ 0、大きければ 1 を返す。
   片方が他方の接頭辞なら、短い方が小さい。 */
int krt_bytes_compare(const uint8_t *a, size_t alen, const uint8_t *b, size_t blen);

/* ---- レコードの形 ----
   形は、レコードが持つラベルの番号の列である。
   列は正準の配置に並べる。
   ラベルの番号の昇順に並べ、同じラベルが重なるときは外側のものを先に置く。
   フィールドの値の配列は、この配置の順に並べる。
   したがって、ラベル l の最も外側のフィールドの位置は、列の中で l が最初に現れる位置である。
   ラベルの並べ方は評価の結果に効かない(LangSpec 18.3 節)ので、この配置は処理系の内部の約束である。

   形の表は intern の表で、同じ列には同じ番号を返す。
   番号は 0 から順に振り、空の形は常に 0 である。
   表は呼び出し側が用意したメモリに置く(rtcore は確保をしない)。
   必要なバイト数は krt_shapes_bytes が返し、領域は 8 バイト境界に置く。
   表が満ちたら、各操作は KRT_SHAPE_FULL を返し、表を変えない。
   呼び出し側は、大きな表を krt_shapes_init で作り、krt_shapes_copy で写してから、同じ操作をやり直す。
   写しても番号は変わらない。

   1 つの表を複数のスレッドから同時に使ってはならない。 */
#define KRT_SHAPE_NONE 0xffffffffu /* 見つからない、または引数が不正 */
#define KRT_SHAPE_FULL 0xfffffffeu /* 表が満ちた */

typedef struct krt_shapes krt_shapes;

/* 形を cap_shapes 個まで、ラベルを合計 cap_labels 個まで持つ表のバイト数。
   容量が大きすぎて size_t に収まらないときは 0 を返す。 */
size_t krt_shapes_bytes(uint32_t cap_shapes, uint32_t cap_labels);
/* mem に空の表を作り、空の形を番号 0 で入れる。
   mem は krt_shapes_bytes(cap_shapes, cap_labels) バイト以上で、8 バイト境界に置く。
   cap_shapes が 0 か、容量が大きすぎるときは NULL を返す。 */
krt_shapes *krt_shapes_init(void *mem, uint32_t cap_shapes, uint32_t cap_labels);
/* src のすべての形を、番号を保って空の表 dst に写す。
   成功すれば 0、dst の容量が足りないか dst が空でなければ -1 を返す。 */
int krt_shapes_copy(krt_shapes *dst, const krt_shapes *src);
/* 表にある形の数と、使ったラベルの数。 */
uint32_t krt_shapes_count(const krt_shapes *t);
uint32_t krt_shapes_labels_used(const krt_shapes *t);

/* 書いた順のラベルの列(先に書いたものが外側)から形を作って intern し、番号を返す。
   pos が NULL でなければ、pos[i] に、書いた i 番目のフィールドの正準の配置での位置を書く。
   計算量は n の 2 乗である。 */
uint32_t krt_shape_intern(krt_shapes *t, const uint32_t *labels, uint32_t n, uint32_t *pos);
/* 形のラベルの数と、正準の配置のラベルの列。
   id が不正なら、それぞれ KRT_SHAPE_NONE と NULL を返す。
   列を指すポインタは、表のメモリを呼び出し側が解放も再初期化もしない間は有効である(表への操作も写しも、既存の列を動かさない)。 */
uint32_t krt_shape_len(const krt_shapes *t, uint32_t id);
const uint32_t *krt_shape_labels(const krt_shapes *t, uint32_t id);
/* ラベル label の最も外側のフィールドの位置。
   持たなければ KRT_SHAPE_NONE を返す。 */
uint32_t krt_shape_find(const krt_shapes *t, uint32_t id, uint32_t label);
/* 形 id の外側に label を重ねた形の番号を返し、*pos に新しいフィールドの位置を書く。
   新しいフィールドは、同じラベルの既存のフィールドより前(外側)に入る。 */
uint32_t krt_shape_extend(krt_shapes *t, uint32_t id, uint32_t label, uint32_t *pos);
/* 形 id から label の最も外側のフィールドを除いた形の番号を返し、*pos に除いた位置を書く。
   label を持たなければ KRT_SHAPE_NONE を返す。 */
uint32_t krt_shape_restrict(krt_shapes *t, uint32_t id, uint32_t label, uint32_t *pos);

#endif
