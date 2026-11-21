/* rtcore: RustKeleut のすべてのバックエンドが共有する、ランタイムの構成要素。

   フリースタンディングな(-ffreestanding。libc と OS に依存しない)C99 で書いてある。
   同じソースを x86_64 の静的ライブラリと wasm32 の rt.wasm にビルドし、両者は同じ結果を返す。
   外部に見えるシンボルは、すべて krt_ で始まる。

   rtcore は次の 4 つからなる。

   - f64show.c: Float64 の文字列化(LangSpec §16.2)。
   - shape.c: レコードのシェイプの intern、拡張、フィールドの除去、位置の検索。
   - bytes.c: 符号なしバイトの辞書順の比較。
   - musl/: musl の sin cos exp log(名前を krt_sin などに変えてビルドする)。

   wasm32 のホストは、線形メモリの中の領域を指す 32 ビットのアドレスで引数を渡す。
   入力のバイト列と出力の文字列は、krt_scratch が返す領域か、__heap_base より上の領域に置く。 */
#ifndef KRT_RTCORE_H
#define KRT_RTCORE_H

#include <stddef.h>
#include <stdint.h>

/* ---- libm(musl 1.2.6) ----
   LLVM が標準ライブラリの関数とみなさないように、名前に krt_ を付けてある。
   結果のビット列は、x86_64 と wasm32 で一致する。 */
double krt_sin(double x);
double krt_cos(double x);
double krt_exp(double x);
double krt_log(double x);

/* ---- Float64 の文字列化 ----
   krt_f64_show は x を LangSpec §16.2 の表記で out に書き、書いたバイト数を返す。
   out には KRT_F64_SHOW_MAX バイトの領域が必要である。
   書いた文字列の後ろに NUL を 1 バイト置くが、返すバイト数には含めない。

   NaN は符号とペイロードによらず nan になり、無限大は inf か -inf になる。
   絶対値が 1e16 未満の整数値は、-0.0 も含めて %.1f の形式になる。
   それ以外の値は、%.*g の形式で有効数字 p 桁に丸める。
   p は、丸めた結果を読み戻すと x になる桁数のうち最小のもの(1 から 17)である。
   そのうえで指数の + と先頭の 0 を取り除き、小数点も指数もなければ .0 を付ける。

   krt_f64_show_bits は x のビット列を受け取る。
   wasm32 のホストが、JS の Number を通さずに NaN の符号とペイロードを渡すための関数である。 */
#define KRT_F64_SHOW_MAX 32
size_t krt_f64_show(double x, char *out);
size_t krt_f64_show_bits(uint64_t bits, char *out);

/* ---- 作業用バッファ ----
   krt_scratch は、KRT_SCRATCH_SIZE バイトの静的な領域の先頭を返す。
   wasm32 のホストが、引数のバイト列と結果の文字列を受け渡すのに使う。
   x86_64 では使わなくてよい。
   領域は 1 つしかないので、複数のスレッドから同時に使ってはならない。 */
#define KRT_SCRATCH_SIZE 65536
char *krt_scratch(void);

/* ---- バイト列の比較 ----
   a と b を、バイトを符号なしの値とみなした辞書順で比較する。
   a が小さければ -1、等しければ 0、大きければ 1 を返す。
   片方が他方の接頭辞なら、短いほうが小さい。 */
int krt_bytes_compare(const uint8_t *a, size_t alen, const uint8_t *b, size_t blen);

/* ---- レコードのシェイプ ----
   シェイプは、レコードが持つラベルの番号の並びである。
   ラベルは、正準なレイアウト(canonical。一意に決まる形)で並べる。
   正準なレイアウトでは、ラベルの番号の昇順に並べ、同じラベルが重なるときは外側のものを先に置く。
   フィールドの値の配列も、このレイアウトの順に並べる。
   したがって、ラベル l の最も外側のフィールドの位置は、並びの中で l が最初に現れる位置である。
   異なるラベルの順序は型の一致に影響しない(LangSpec §7.2、§18.3)ので、このレイアウトは処理系の内部だけの規則である。

   シェイプ表は intern 表で、同じ並びには同じ番号を返す。
   番号は 0 から順に振り、空のシェイプは常に 0 である。
   表は呼び出し元が用意したメモリに置き、rtcore はメモリを確保しない。
   必要なバイト数は krt_shapes_bytes が返す。
   このメモリは 8 バイト境界に置く。
   表の容量が足りないと、新しいシェイプを作る操作は KRT_SHAPE_FULL を返し、表を変えない。
   そのとき呼び出し元は、大きな表を krt_shapes_init で作り、krt_shapes_copy でコピーしてから、同じ操作をやり直す。
   コピーしても番号は変わらない。

   1 つの表を複数のスレッドから同時に使ってはならない。 */
#define KRT_SHAPE_NONE 0xffffffffu /* 見つからないか、引数が不正である */
#define KRT_SHAPE_FULL 0xfffffffeu /* 表がいっぱいである */

typedef struct krt_shapes krt_shapes;

/* シェイプを cap_shapes 個まで、ラベルを合計 cap_labels 個まで持つ表のバイト数を返す。
   cap_shapes が 0 のとき、cap_shapes が 2^30 を超えるとき、バイト数が size_t に収まらないときは 0 を返す。 */
size_t krt_shapes_bytes(uint32_t cap_shapes, uint32_t cap_labels);
/* mem に空の表を作り、空のシェイプを番号 0 で入れる。
   mem は krt_shapes_bytes(cap_shapes, cap_labels) バイト以上の領域で、8 バイト境界に置く。
   mem が NULL のときと、krt_shapes_bytes が 0 を返す容量のときは NULL を返す。 */
krt_shapes *krt_shapes_init(void *mem, uint32_t cap_shapes, uint32_t cap_labels);
/* src のすべてのシェイプを、番号を保って空の表 dst にコピーする。
   成功すれば 0 を返す。
   dst の容量が足りないか、dst が空の表でなければ -1 を返す。 */
int krt_shapes_copy(krt_shapes *dst, const krt_shapes *src);
/* 表にあるシェイプの個数と、使ったラベルの個数を返す。 */
uint32_t krt_shapes_count(const krt_shapes *t);
uint32_t krt_shapes_labels_used(const krt_shapes *t);

/* 書いた順のラベルの並びからシェイプを作って intern し、番号を返す。
   先に書いたラベルが外側である。
   pos が NULL でなければ、書いた i 番目のフィールドが正準なレイアウトで占める位置を pos[i] に書く。
   KRT_SHAPE_FULL を返したときの pos の内容は不定である。
   処理時間は n の 2 乗に比例する。 */
uint32_t krt_shape_intern(krt_shapes *t, const uint32_t *labels, uint32_t n, uint32_t *pos);
/* シェイプのラベルの個数と、正準なレイアウトに並べたラベルの並びを返す。
   id が不正なら、それぞれ KRT_SHAPE_NONE と NULL を返す。
   並びを指すポインタは、呼び出し元が表のメモリを解放も再初期化もしない間は有効である。
   表への操作も、表のコピーも、既存の並びを動かさないからである。 */
uint32_t krt_shape_len(const krt_shapes *t, uint32_t id);
const uint32_t *krt_shape_labels(const krt_shapes *t, uint32_t id);
/* ラベル label の最も外側のフィールドの位置を返す。
   シェイプ id が label を持たないか、id が不正なら KRT_SHAPE_NONE を返す。 */
uint32_t krt_shape_find(const krt_shapes *t, uint32_t id, uint32_t label);
/* シェイプ id の外側に label を重ねたシェイプの番号を返し、*pos に新しいフィールドの位置を書く。
   新しいフィールドは、同じラベルの既存のフィールドより前(外側)に入る。
   id が不正なら KRT_SHAPE_NONE を返す。 */
uint32_t krt_shape_extend(krt_shapes *t, uint32_t id, uint32_t label, uint32_t *pos);
/* シェイプ id から label の最も外側のフィールドを取り除いたシェイプの番号を返し、*pos に取り除いた位置を書く。
   シェイプ id が label を持たないか、id が不正なら KRT_SHAPE_NONE を返す。 */
uint32_t krt_shape_restrict(krt_shapes *t, uint32_t id, uint32_t label, uint32_t *pos);

#endif
