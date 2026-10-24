//! rtcore の FFI と、`rt.wasm` の置き場所。
//!
//! rtcore は自立した C99 の部品で、`rustkeleut/rtcore/` にある。
//! 同じソースを x86_64 の静的ライブラリ(この crate に結ぶ)と `rt.wasm`(WasmGC の後段が import する)に組む。
//! 2 つは同じ結果を返す。
//! C の API の約束は `rustkeleut/rtcore/rtcore.h` に書いてある。

use std::cmp::Ordering;
use std::ffi::c_char;

/// `rt.wasm` のバイト列。
pub const RT_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rt.wasm"));

/// build.rs が組んだ `rt.wasm` のパス。
pub const RT_WASM_PATH: &str = concat!(env!("OUT_DIR"), "/rt.wasm");

/// `rt.wasm` の写しと、試験が node の確認に渡すファイルを置くディレクトリ(`<target>/rtcore`)。
pub const STAGE_DIR: &str = env!("KRT_STAGE_DIR");

/// x86_64 の静的ライブラリのパス(記号の検査に使う)。
pub const STATIC_LIB_PATH: &str = env!("KRT_STATIC_LIB");

/// この crate を組んだ cargo の profile(`debug` か `release`)。
pub const BUILD_PROFILE: &str = env!("KRT_PROFILE");

/// この crate を組んだ対象の三つ組。
pub const BUILD_TARGET: &str = env!("KRT_TARGET");

/// `rt.wasm` が export する記号(`memory` を除く)。
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

/// `show` の結果の最大のバイト数(終端の NUL を含む)。
pub const F64_SHOW_MAX: usize = 32;

/// 形の表の操作の失敗(見つからない、または引数が不正)。
const SHAPE_NONE: u32 = 0xffff_ffff;
/// 形の表が満ちた。
const SHAPE_FULL: u32 = 0xffff_fffe;

/// C の宣言。意味は `rtcore.h` にある。
pub mod sys {
    use std::ffi::c_char;

    /// 形の表(中身は C の側だけが知る)。
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

/// `sin`(musl 1.2.6)。
pub fn sin(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_sin(x) }
}

/// `cos`(musl 1.2.6)。
pub fn cos(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_cos(x) }
}

/// `exp`(musl 1.2.6)。
pub fn exp(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_exp(x) }
}

/// `log`(musl 1.2.6)。
pub fn log(x: f64) -> f64 {
    // SAFETY: 純粋な関数で、ポインタを取らない。
    unsafe { sys::krt_log(x) }
}

/// `x` を LangSpec 16.2 節の表記で `buf` に書き、書いた部分を返す。
pub fn show_f64_into(x: f64, buf: &mut [u8; F64_SHOW_MAX]) -> &str {
    // SAFETY: rtcore は F64_SHOW_MAX バイト以内(NUL を含む)しか書かない。
    let n = unsafe { sys::krt_f64_show_bits(x.to_bits(), buf.as_mut_ptr().cast::<c_char>()) };
    // rtcore が書くのは ASCII だけである。
    std::str::from_utf8(&buf[..n]).expect("rtcore の show は ASCII を書く")
}

/// `x` を LangSpec 16.2 節の表記の文字列にする。
pub fn show_f64(x: f64) -> String {
    let mut buf = [0u8; F64_SHOW_MAX];
    show_f64_into(x, &mut buf).to_string()
}

/// 符号なしバイトの辞書順で `a` と `b` を比べる。
pub fn compare_bytes(a: &[u8], b: &[u8]) -> Ordering {
    // SAFETY: 2 つのスライスを長さどおりに読むだけである。
    match unsafe { sys::krt_bytes_compare(a.as_ptr(), a.len(), b.as_ptr(), b.len()) } {
        r if r < 0 => Ordering::Less,
        0 => Ordering::Equal,
        _ => Ordering::Greater,
    }
}

/// レコードの形の番号。空の形は `ShapeId::EMPTY` である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeId(pub u32);

impl ShapeId {
    /// 空の形。
    pub const EMPTY: ShapeId = ShapeId(0);
}

/// レコードの形の表(rtcore の `krt_shapes` を包む)。
///
/// 形はラベルの番号の列で、番号の昇順に並べ、同じラベルは外側のものを先に置く。
/// フィールドの値の配列もこの順に並べる。
/// 表が満ちたら、容量を倍にした表に写してやり直す。
/// 広げた容量を C の表が受け付けないとき(形が 2^30 個を超える、ラベルの数が u32 を超える、大きさが size_t を超える)は panic する。
pub struct ShapeTable {
    mem: Vec<u64>,
    cap_shapes: u32,
    cap_labels: u32,
}

impl Default for ShapeTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeTable {
    /// 小さな容量の空の表を作る。
    pub fn new() -> Self {
        Self::with_capacity(64, 256)
    }

    /// 形を `cap_shapes` 個、ラベルを合計 `cap_labels` 個まで持てる空の表を作る。
    pub fn with_capacity(cap_shapes: u32, cap_labels: u32) -> Self {
        let cap_shapes = cap_shapes.max(1);
        let mut mem = Self::alloc(cap_shapes, cap_labels);
        // SAFETY: mem は krt_shapes_bytes のバイト数以上で、8 バイト境界にある。
        let t = unsafe { sys::krt_shapes_init(mem.as_mut_ptr().cast(), cap_shapes, cap_labels) };
        assert!(!t.is_null(), "形の表の容量が大きすぎる");
        ShapeTable {
            mem,
            cap_shapes,
            cap_labels,
        }
    }

    fn alloc(cap_shapes: u32, cap_labels: u32) -> Vec<u64> {
        // SAFETY: 引数を読むだけである。
        let bytes = unsafe { sys::krt_shapes_bytes(cap_shapes, cap_labels) };
        assert!(bytes != 0, "形の表の容量が大きすぎる");
        vec![0u64; bytes.div_ceil(8)]
    }

    fn raw(&self) -> *const sys::KrtShapes {
        self.mem.as_ptr().cast()
    }

    fn raw_mut(&mut self) -> *mut sys::KrtShapes {
        self.mem.as_mut_ptr().cast()
    }

    /// 容量を倍にした表に写す。
    fn grow(&mut self) {
        let cap_shapes = self.cap_shapes.checked_mul(2).expect("形の表が大きすぎる");
        let cap_labels = self
            .cap_labels
            .max(16)
            .checked_mul(2)
            .expect("形の表が大きすぎる");
        let mut mem = Self::alloc(cap_shapes, cap_labels);
        // SAFETY: mem は新しい表の大きさを持ち、self.mem は初期化した表である。
        unsafe {
            let dst = sys::krt_shapes_init(mem.as_mut_ptr().cast(), cap_shapes, cap_labels);
            assert!(!dst.is_null());
            assert_eq!(sys::krt_shapes_copy(dst, self.raw()), 0);
        }
        self.mem = mem;
        self.cap_shapes = cap_shapes;
        self.cap_labels = cap_labels;
    }

    /// 満ちたら広げて、`f` をやり直す。
    fn retry(&mut self, mut f: impl FnMut(*mut sys::KrtShapes) -> u32) -> u32 {
        loop {
            let r = f(self.raw_mut());
            if r != SHAPE_FULL {
                return r;
            }
            self.grow();
        }
    }

    /// 表にある形の数。
    pub fn count(&self) -> u32 {
        // SAFETY: 初期化した表である。
        unsafe { sys::krt_shapes_count(self.raw()) }
    }

    /// 書いた順のラベル(先に書いたものが外側)から形を作る。
    ///
    /// `positions` には、書いた i 番目のフィールドの、正準の配置での位置が入る。
    pub fn intern(&mut self, labels: &[u32], positions: &mut Vec<u32>) -> ShapeId {
        let n = u32::try_from(labels.len()).expect("ラベルが多すぎる");
        positions.clear();
        positions.resize(labels.len(), 0);
        let pos = positions.as_mut_ptr();
        // SAFETY: labels と positions はどちらも n 個の要素を持つ。
        let r = self.retry(|t| unsafe { sys::krt_shape_intern(t, labels.as_ptr(), n, pos) });
        assert_ne!(r, SHAPE_NONE);
        ShapeId(r)
    }

    /// 形のラベルの列(正準の配置)。
    pub fn labels(&self, id: ShapeId) -> &[u32] {
        // SAFETY: 初期化した表を読むだけである。返す列は self の借用の間は動かない。
        unsafe {
            let n = sys::krt_shape_len(self.raw(), id.0);
            assert_ne!(n, SHAPE_NONE, "形の番号が不正");
            if n == 0 {
                return &[];
            }
            std::slice::from_raw_parts(sys::krt_shape_labels(self.raw(), id.0), n as usize)
        }
    }

    /// `label` の最も外側のフィールドの位置。
    pub fn find(&self, id: ShapeId, label: u32) -> Option<u32> {
        // SAFETY: 初期化した表を読むだけである。
        let r = unsafe { sys::krt_shape_find(self.raw(), id.0, label) };
        (r != SHAPE_NONE).then_some(r)
    }

    /// `id` の外側に `label` を重ねた形と、新しいフィールドの位置。
    pub fn extend(&mut self, id: ShapeId, label: u32) -> (ShapeId, u32) {
        let mut pos = 0u32;
        let p = &mut pos as *mut u32;
        // SAFETY: p は生きている u32 を指す。
        let r = self.retry(|t| unsafe { sys::krt_shape_extend(t, id.0, label, p) });
        assert_ne!(r, SHAPE_NONE, "形の番号が不正");
        (ShapeId(r), pos)
    }

    /// `id` から `label` の最も外側のフィールドを除いた形と、除いた位置。
    /// `label` を持たなければ `None` を返す。
    pub fn restrict(&mut self, id: ShapeId, label: u32) -> Option<(ShapeId, u32)> {
        let mut pos = 0u32;
        let p = &mut pos as *mut u32;
        // SAFETY: p は生きている u32 を指す。
        let r = self.retry(|t| unsafe { sys::krt_shape_restrict(t, id.0, label, p) });
        (r != SHAPE_NONE).then_some((ShapeId(r), pos))
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
