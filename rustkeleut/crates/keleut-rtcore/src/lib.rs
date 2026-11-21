//! rtcore の FFI と、`rt.wasm` のバイト列とパス。
//!
//! rtcore は、フリースタンディングな(`-ffreestanding`。libc と OS に依存しない)C99 で書いたランタイムの構成要素で、ソースは `rustkeleut/rtcore/` にある。
//! 同じソースから、x86_64 の静的ライブラリと `rt.wasm` をビルドする。
//! 静的ライブラリはこの crate にリンクし、`rt.wasm` は WasmGC バックエンドが生成するモジュールが import する。
//! 2 つは同じ入力に同じ結果を返す。
//! C の API の仕様と事前条件は `rustkeleut/rtcore/rtcore.h` に書いてある。

use std::cmp::Ordering;
use std::ffi::c_char;

/// `rt.wasm` のバイト列。
pub const RT_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rt.wasm"));

/// build.rs がビルドした `rt.wasm` のパス。
pub const RT_WASM_PATH: &str = concat!(env!("OUT_DIR"), "/rt.wasm");

/// `rt.wasm` のコピーと、テストが Node での検査に渡すファイルを置くディレクトリ(`<target>/rtcore`)。
pub const STAGE_DIR: &str = env!("KRT_STAGE_DIR");

/// x86_64 の静的ライブラリのパス。シンボルを検査するテストが使う。
pub const STATIC_LIB_PATH: &str = env!("KRT_STATIC_LIB");

/// この crate をビルドした cargo の profile(`debug` か `release`)。
pub const BUILD_PROFILE: &str = env!("KRT_PROFILE");

/// この crate をビルドしたターゲットトリプル。
pub const BUILD_TARGET: &str = env!("KRT_TARGET");

/// `rt.wasm` が export するシンボル(`memory` を除く)。
pub const RT_WASM_EXPORTS: &[&str] = &[
    "krt_sin",
    "krt_cos",
    "krt_exp",
    "krt_log",
    "krt_f64_show",
    "krt_f64_show_bits",
    "krt_scratch",
    "krt_bytes_compare",
    "krt_shapes_bytes",
    "krt_shapes_init",
    "krt_shapes_copy",
    "krt_shapes_count",
    "krt_shapes_labels_used",
    "krt_shape_intern",
    "krt_shape_len",
    "krt_shape_labels",
    "krt_shape_find",
    "krt_shape_extend",
    "krt_shape_restrict",
    "__heap_base",
];

/// `show` の結果の最大バイト数(終端の NUL を含む)。
pub const F64_SHOW_MAX: usize = 32;

/// シェイプ表の操作が失敗したことを表す返り値。シェイプが指定のラベルを持たないときと、引数が不正なときに返る。
const SHAPE_NONE: u32 = 0xffff_ffff;
/// シェイプ表がいっぱいであることを表す返り値。
const SHAPE_FULL: u32 = 0xffff_fffe;

/// C の関数の宣言。各関数の仕様は `rtcore.h` にある。
pub mod sys {
    use std::ffi::c_char;

    /// シェイプ表。内容は C の側だけが知っている。
    #[repr(C)]
    pub struct KrtShapes {
        _private: [u8; 0],
    }

    unsafe extern "C" {
        pub fn krt_sin(x: f64) -> f64;
        pub fn krt_cos(x: f64) -> f64;
        pub fn krt_exp(x: f64) -> f64;
        pub fn krt_log(x: f64) -> f64;
        pub fn krt_f64_show(x: f64, out: *mut c_char) -> usize;
        pub fn krt_f64_show_bits(bits: u64, out: *mut c_char) -> usize;
        pub fn krt_bytes_compare(a: *const u8, alen: usize, b: *const u8, blen: usize) -> i32;
        pub fn krt_shapes_bytes(cap_shapes: u32, cap_labels: u32) -> usize;
        pub fn krt_shapes_init(mem: *mut u8, cap_shapes: u32, cap_labels: u32) -> *mut KrtShapes;
        pub fn krt_shapes_copy(dst: *mut KrtShapes, src: *const KrtShapes) -> i32;
        pub fn krt_shapes_count(t: *const KrtShapes) -> u32;
        pub fn krt_shapes_labels_used(t: *const KrtShapes) -> u32;
        pub fn krt_shape_intern(
            t: *mut KrtShapes,
            labels: *const u32,
            n: u32,
            pos: *mut u32,
        ) -> u32;
        pub fn krt_shape_len(t: *const KrtShapes, id: u32) -> u32;
        pub fn krt_shape_labels(t: *const KrtShapes, id: u32) -> *const u32;
        pub fn krt_shape_find(t: *const KrtShapes, id: u32, label: u32) -> u32;
        pub fn krt_shape_extend(t: *mut KrtShapes, id: u32, label: u32, pos: *mut u32) -> u32;
        pub fn krt_shape_restrict(t: *mut KrtShapes, id: u32, label: u32, pos: *mut u32) -> u32;
    }
}

/// `x` の正弦を返す(musl 1.2.6 の `sin`)。
pub fn sin(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_sin(x) }
}

/// `x` の余弦を返す(musl 1.2.6 の `cos`)。
pub fn cos(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_cos(x) }
}

/// `e` の `x` 乗を返す(musl 1.2.6 の `exp`)。
pub fn exp(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_exp(x) }
}

/// `x` の自然対数を返す(musl 1.2.6 の `log`)。
pub fn log(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_log(x) }
}

/// `x` を LangSpec §16.2 の表記で `buf` に書き、書いた部分を文字列として返す。
pub fn show_f64_into(x: f64, buf: &mut [u8; F64_SHOW_MAX]) -> &str {
    // SAFETY: rtcore が書くのは、NUL を含めて F64_SHOW_MAX バイト以内である。
    let n = unsafe { sys::krt_f64_show_bits(x.to_bits(), buf.as_mut_ptr().cast::<c_char>()) };
    // rtcore が書くのは ASCII だけである。
    std::str::from_utf8(&buf[..n]).expect("rtcore の show は ASCII を書く")
}

/// `x` を LangSpec §16.2 の表記で文字列にして返す。
pub fn show_f64(x: f64) -> String {
    let mut buf = [0u8; F64_SHOW_MAX];
    show_f64_into(x, &mut buf).to_string()
}

/// `a` と `b` を、バイトを符号なしの値とみなした辞書順で比較する。
pub fn compare_bytes(a: &[u8], b: &[u8]) -> Ordering {
    // SAFETY: 2 つのスライスを長さどおりに読むだけである。
    match unsafe { sys::krt_bytes_compare(a.as_ptr(), a.len(), b.as_ptr(), b.len()) } {
        r if r < 0 => Ordering::Less,
        0 => Ordering::Equal,
        _ => Ordering::Greater,
    }
}

/// レコードのシェイプの番号。空のシェイプは `ShapeId::EMPTY` である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeId(pub u32);

impl ShapeId {
    /// 空のシェイプ(番号 0)。
    pub const EMPTY: ShapeId = ShapeId(0);
}

/// シェイプ表を作れなかったこと、または拡張できなかったことを表すエラー。
///
/// メモリを確保できないときと、求める容量の表を作れないときに返す。
/// 表を作れないのは、シェイプの容量が 2^30 個を超えるとき、ラベルの容量が u32 に収まらないとき、表のサイズが size_t に収まらないときである。
/// [`ShapeTable::intern`] は、渡したラベルの個数が u32 に収まらないときと、位置を返す `Vec` を拡張できないときにも返す。
/// 失敗した操作は表を変更しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfMemory;

/// レコードのシェイプ表。rtcore の `krt_shapes` のラッパである。
///
/// シェイプはラベルの番号の並びである。
/// 番号の昇順に並べ、同じラベルが複数あるときは外側のものを先に置く。
/// この並びを正準なレイアウト(canonical。一意に決まる形)と呼ぶ。
/// フィールドの値の配列もこの順に並べる。
/// 表がいっぱいになったら、容量を 2 倍にした新しい表に内容をコピーして、操作をやり直す。
/// 新しい表のメモリを確保できないときと、拡張後の容量の表を作れないときは、表を変更せずに [`OutOfMemory`] を返す。
///
/// 表のメモリはグローバルアロケータの `alloc_zeroed` で確保し、確保の失敗を null で受け取る。
/// 各操作は、確保の失敗を呼び出し元が処理する範囲(keleut-rt の `alloc::handled`)の中で呼び出す。
/// 確保が失敗しても、グローバルアロケータの予備領域を使わないようにするためである。
pub struct ShapeTable {
    mem: Box<[u64]>,
    cap_shapes: u32,
    cap_labels: u32,
}

impl ShapeTable {
    /// 小さな容量(シェイプ 64 個、ラベル 256 個)の空の表を作る。
    pub fn new() -> Result<Self, OutOfMemory> {
        Self::with_capacity(64, 256)
    }

    /// シェイプを `cap_shapes` 個、ラベルを合計 `cap_labels` 個まで持てる空の表を作る。
    ///
    /// `cap_shapes` が 0 のときは 1 として扱う。
    pub fn with_capacity(cap_shapes: u32, cap_labels: u32) -> Result<Self, OutOfMemory> {
        let cap_shapes = cap_shapes.max(1);
        let mem = Self::fresh(cap_shapes, cap_labels)?;
        Ok(ShapeTable {
            mem,
            cap_shapes,
            cap_labels,
        })
    }

    /// 表を置くための、0 で埋めたメモリを確保する。
    ///
    /// `vec![0; n]` と同じく `alloc_zeroed` で確保するので、使わないページにはアクセスしない。
    fn alloc(cap_shapes: u32, cap_labels: u32) -> Result<Box<[u64]>, OutOfMemory> {
        // SAFETY: 引数の値から計算するだけで、メモリを読み書きしない。
        let bytes = unsafe { sys::krt_shapes_bytes(cap_shapes, cap_labels) };
        if bytes == 0 {
            return Err(OutOfMemory);
        }
        let words = bytes.div_ceil(8);
        let layout = std::alloc::Layout::array::<u64>(words).map_err(|_| OutOfMemory)?;
        // SAFETY: bytes は 0 でないので、layout のサイズは 0 でない。
        let p = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<u64>();
        if p.is_null() {
            return Err(OutOfMemory);
        }
        // SAFETY: p は Layout::array::<u64>(words) で確保した、0 で埋めた words ワードの領域を指す。
        // このレイアウトは、Box<[u64]> が解放に使うレイアウトと一致する。
        Ok(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, words)) })
    }

    /// 指定した容量の表のメモリを確保し、空の表として初期化する。
    fn fresh(cap_shapes: u32, cap_labels: u32) -> Result<Box<[u64]>, OutOfMemory> {
        let mut mem = Self::alloc(cap_shapes, cap_labels)?;
        // SAFETY: mem は krt_shapes_bytes が返したバイト数以上の領域で、8 バイト境界にある。
        let t = unsafe { sys::krt_shapes_init(mem.as_mut_ptr().cast(), cap_shapes, cap_labels) };
        if t.is_null() {
            return Err(OutOfMemory);
        }
        Ok(mem)
    }

    fn raw(&self) -> *const sys::KrtShapes {
        self.mem.as_ptr().cast()
    }

    fn raw_mut(&mut self) -> *mut sys::KrtShapes {
        self.mem.as_mut_ptr().cast()
    }

    /// 容量を 2 倍にした新しい表を作り、内容をコピーする。
    ///
    /// ラベルの容量は、16 に満たなければ 16 として 2 倍にする。
    /// 新しい表を作れなければ、現在の表をそのまま残して [`OutOfMemory`] を返す。
    fn grow(&mut self) -> Result<(), OutOfMemory> {
        let cap_shapes = self.cap_shapes.checked_mul(2).ok_or(OutOfMemory)?;
        let cap_labels = self.cap_labels.max(16).checked_mul(2).ok_or(OutOfMemory)?;
        let mut mem = Self::fresh(cap_shapes, cap_labels)?;
        // SAFETY: mem は新しい容量で初期化した表で、self.mem も初期化済みの表である。
        // 新しい表は古い表のシェイプとラベルをすべて持てる容量なので、コピーは失敗しない。
        unsafe {
            assert_eq!(sys::krt_shapes_copy(mem.as_mut_ptr().cast(), self.raw()), 0);
        }
        self.mem = mem;
        self.cap_shapes = cap_shapes;
        self.cap_labels = cap_labels;
        Ok(())
    }

    /// `f` を実行し、表がいっぱいなら表を拡張して `f` をやり直す。
    fn retry(&mut self, mut f: impl FnMut(*mut sys::KrtShapes) -> u32) -> Result<u32, OutOfMemory> {
        loop {
            let r = f(self.raw_mut());
            if r != SHAPE_FULL {
                return Ok(r);
            }
            self.grow()?;
        }
    }

    /// 表にあるシェイプの個数を返す。
    pub fn count(&self) -> u32 {
        // SAFETY: 初期化済みの表である。
        unsafe { sys::krt_shapes_count(self.raw()) }
    }

    /// 書いた順のラベル(先に書いたものが外側)からシェイプを作り、その番号を返す。
    ///
    /// `positions` には、`i` 番目に書いたフィールドの、正準なレイアウトでの位置を入れる。
    /// `positions` を拡張するための確保が失敗したときも [`OutOfMemory`] を返す。
    pub fn intern(
        &mut self,
        labels: &[u32],
        positions: &mut Vec<u32>,
    ) -> Result<ShapeId, OutOfMemory> {
        let n = u32::try_from(labels.len()).map_err(|_| OutOfMemory)?;
        positions.clear();
        positions
            .try_reserve_exact(labels.len())
            .map_err(|_| OutOfMemory)?;
        positions.resize(labels.len(), 0);
        let pos = positions.as_mut_ptr();
        // SAFETY: labels と positions はどちらも n 個の要素を持つ。
        let r = self.retry(|t| unsafe { sys::krt_shape_intern(t, labels.as_ptr(), n, pos) })?;
        assert_ne!(r, SHAPE_NONE);
        Ok(ShapeId(r))
    }

    /// シェイプ `id` のラベルの並びを、正準なレイアウトの順で返す。
    pub fn labels(&self, id: ShapeId) -> &[u32] {
        // SAFETY: 初期化済みの表を読むだけである。返すスライスが指す領域は、self を借用している間は移動しない。
        unsafe {
            let n = sys::krt_shape_len(self.raw(), id.0);
            assert_ne!(n, SHAPE_NONE, "シェイプの番号が不正");
            if n == 0 {
                return &[];
            }
            std::slice::from_raw_parts(sys::krt_shape_labels(self.raw(), id.0), n as usize)
        }
    }

    /// シェイプ `id` の中で、`label` の最も外側のフィールドの位置を返す。
    /// `id` が `label` を持たなければ `None` を返す。
    pub fn find(&self, id: ShapeId, label: u32) -> Option<u32> {
        // SAFETY: 初期化済みの表を読むだけである。
        let r = unsafe { sys::krt_shape_find(self.raw(), id.0, label) };
        (r != SHAPE_NONE).then_some(r)
    }

    /// `id` の外側に `label` のフィールドを追加したシェイプと、新しいフィールドの位置を返す。
    pub fn extend(&mut self, id: ShapeId, label: u32) -> Result<(ShapeId, u32), OutOfMemory> {
        let mut pos = 0u32;
        let p = &mut pos as *mut u32;
        // SAFETY: p は有効な u32 を指す。
        let r = self.retry(|t| unsafe { sys::krt_shape_extend(t, id.0, label, p) })?;
        assert_ne!(r, SHAPE_NONE, "シェイプの番号が不正");
        Ok((ShapeId(r), pos))
    }

    /// `id` から `label` の最も外側のフィールドを取り除いたシェイプと、取り除いたフィールドの位置を返す。
    /// `id` が `label` を持たなければ `Ok(None)` を返す。
    pub fn restrict(
        &mut self,
        id: ShapeId,
        label: u32,
    ) -> Result<Option<(ShapeId, u32)>, OutOfMemory> {
        let mut pos = 0u32;
        let p = &mut pos as *mut u32;
        // SAFETY: p は有効な u32 を指す。
        let r = self.retry(|t| unsafe { sys::krt_shape_restrict(t, id.0, label, p) })?;
        Ok((r != SHAPE_NONE).then_some((ShapeId(r), pos)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_examples() {
        let cases: &[(f64, &str)] = &[
            (100000.0, "100000.0"),
            (0.0001, "0.0001"),
            (1e-5, "1e-5"),
            (1e16, "1e16"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.5, "1.5"),
            (-2.5, "-2.5"),
            (0.1, "0.1"),
            (1.0 / 3.0, "0.3333333333333333"),
            (2f64.powi(70), "1.1805916207174113e21"),
            (f64::from_bits(1), "5e-324"),
            (f64::MAX, "1.7976931348623157e308"),
            (1e23, "1e23"),
            (9999999999999998.0, "9999999999999998.0"),
            (123456.789, "123456.789"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (f64::NAN, "nan"),
            (-f64::NAN, "nan"),
        ];
        for &(x, s) in cases {
            assert_eq!(show_f64(x), s, "{x:e}");
        }
    }

    #[test]
    fn bytes_order() {
        assert_eq!(compare_bytes(b"", b""), Ordering::Equal);
        assert_eq!(compare_bytes(b"a", b"ab"), Ordering::Less);
        assert_eq!(compare_bytes(b"\xff", b"\x01"), Ordering::Greater);
        assert_eq!(compare_bytes(b"abc", b"abd"), Ordering::Less);
        assert_eq!(compare_bytes(b"abc", b"abc"), Ordering::Equal);
    }

    #[test]
    fn libm_basic() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(log(1.0), 0.0);
    }
}
