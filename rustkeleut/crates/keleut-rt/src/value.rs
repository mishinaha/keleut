//! 一様な語と、ヒープの箱の配置(計画 261024-1 の 6.5 節、D525)。
//!
//! 一様な語は 64 ビットである。
//!
//! | 語の形 | 意味 |
//! | --- | --- |
//! | 最下位ビットが 1 で、`(語 as i64) >> 1` が `i32` に収まる | `Int32` の即値 `(n << 1) \| 1` |
//! | 最下位ビットが 1 で、上位の 8 ビットが `0x40 \| 種類` | `Int32` 以外の即値(Unit、真偽値、引数の無いコンストラクタ、辞書) |
//! | 下位 3 ビットが 0 で 0 でない | 8 バイト境界の箱へのポインタ |
//! | `0x2` | 巻き戻しの番兵(値としては現れない。ハンドラのフレームの関数の中でだけレジスタに置く) |
//!
//! `Int32` 以外の即値は上位の 2 ビットが `01` なので、符号を広げた `Int32` の即値と重ならない。
//! 計画 6.5 節は `Int32` の即値の形だけを定め、ほかの即値の形を定めていない。
//! 診断の印字(付録 B の `<値>`)が即値の種類を語だけから知る必要があるので、種類を上位の語に置いた。
//!
//! 箱の先頭の語(見出し)は、種類(下位 8 ビット)、GC の印(次の 8 ビット)、要素の数(上位 32 ビット)を持つ。
//! 種類ごとの配置は [`Kind`] の各項目に書く。

use std::fmt;

/// 一様な語。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct Word(pub u64);

impl fmt::Debug for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Word({:#x})", self.0)
    }
}

/// `Int32` 以外の即値の種類(上位 8 ビットの下位 6 ビット)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ImmKind {
    /// Unit(空のレコード)。
    Unit = 0,
    /// 真偽値。積載は 0 か 1。
    Bool = 1,
    /// 引数の無いコンストラクタ。積載は型(24 ビット)とコンストラクタ(24 ビット)。
    Ctor0 = 2,
    /// 組み込みのインスタンスの辞書。積載はクラス(24 ビット)と型(24 ビット)。
    DictBuiltin = 3,
    /// 前提を持たない利用者のインスタンスの辞書。積載はインスタンスの番号。
    DictInst0 = 4,
}

const IMM_TAG: u64 = 0x40;
const PAYLOAD_MASK: u64 = (1u64 << 55) - 1;
const FIELD24: u64 = (1u64 << 24) - 1;

impl Word {
    /// Unit。
    pub const UNIT: Word = Word::imm(ImmKind::Unit, 0);
    /// 偽。
    pub const FALSE: Word = Word::imm(ImmKind::Bool, 0);
    /// 真。
    pub const TRUE: Word = Word::imm(ImmKind::Bool, 1);
    /// 巻き戻しの番兵。
    pub const SENTINEL: Word = Word(0x2);

    const fn imm(kind: ImmKind, payload: u64) -> Word {
        Word(((IMM_TAG | kind as u64) << 56) | ((payload & PAYLOAD_MASK) << 1) | 1)
    }

    /// `Int32` の即値。
    #[inline]
    pub fn i32(n: i32) -> Word {
        Word((((n as i64) << 1) | 1) as u64)
    }

    /// 真偽値の即値。
    #[inline]
    pub fn bool(b: bool) -> Word {
        if b { Word::TRUE } else { Word::FALSE }
    }

    /// 引数の無いコンストラクタの即値。型とコンストラクタは 2^24 未満でなければならない。
    pub fn ctor0(ty: u32, ctor: u32) -> Word {
        assert!(u64::from(ty) <= FIELD24 && u64::from(ctor) <= FIELD24);
        Word::imm(ImmKind::Ctor0, (u64::from(ty) << 24) | u64::from(ctor))
    }

    /// 組み込みのインスタンスの辞書の即値。
    pub fn dict_builtin(class: u32, ty: u32) -> Word {
        assert!(u64::from(class) <= FIELD24 && u64::from(ty) <= FIELD24);
        Word::imm(
            ImmKind::DictBuiltin,
            (u64::from(class) << 24) | u64::from(ty),
        )
    }

    /// 前提を持たない利用者のインスタンスの辞書の即値。
    pub fn dict_inst0(inst: u32) -> Word {
        Word::imm(ImmKind::DictInst0, u64::from(inst))
    }

    /// 即値か。
    #[inline]
    pub fn is_imm(self) -> bool {
        self.0 & 1 == 1
    }

    /// 箱へのポインタか。
    #[inline]
    pub fn is_ptr(self) -> bool {
        self.0 & 7 == 0 && self.0 != 0
    }

    /// `Int32` の即値か。
    #[inline]
    pub fn is_i32(self) -> bool {
        let v = (self.0 as i64) >> 1;
        self.is_imm() && v == v as i32 as i64
    }

    /// `Int32` の即値の値。
    #[inline]
    pub fn as_i32(self) -> i32 {
        ((self.0 as i64) >> 1) as i32
    }

    /// 真偽値の即値の値。
    #[inline]
    pub fn as_bool(self) -> bool {
        (self.0 >> 1) & 1 == 1
    }

    /// `Int32` 以外の即値の種類。
    pub fn imm_kind(self) -> Option<ImmKind> {
        if !self.is_imm() || self.is_i32() || (self.0 >> 62) != 1 {
            return None;
        }
        Some(match (self.0 >> 56) & 0x3f {
            0 => ImmKind::Unit,
            1 => ImmKind::Bool,
            2 => ImmKind::Ctor0,
            3 => ImmKind::DictBuiltin,
            4 => ImmKind::DictInst0,
            _ => return None,
        })
    }

    /// 即値の積載。
    #[inline]
    pub fn payload(self) -> u64 {
        (self.0 >> 1) & PAYLOAD_MASK
    }

    /// 積載の上位 24 ビットと下位 24 ビット(`Ctor0` と `DictBuiltin`)。
    pub fn payload_pair(self) -> (u32, u32) {
        let p = self.payload();
        (((p >> 24) & FIELD24) as u32, (p & FIELD24) as u32)
    }

    /// 箱へのポインタ。
    #[inline]
    pub fn ptr(self) -> *mut u64 {
        self.0 as *mut u64
    }

    /// ポインタから語を作る。
    #[inline]
    pub fn from_ptr(p: *mut u64) -> Word {
        Word(p as u64)
    }
}

/// 箱の種類(見出しの下位 8 ビット)。
///
/// 配置は語の列で、0 番は見出しである。`n` は見出しの要素の数である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// `Int64`。1 番が値。
    I64 = 1,
    /// `Float64`。1 番がビット列。
    F64 = 2,
    /// 文字列。`n` がバイトの数で、1 番からバイト列(8 バイトに切り上げる)。
    Str = 3,
    /// レコード。1 番が形の番号、2 番から `n` 個のフィールド(形の順)。
    Record = 4,
    /// データ。1 番が `型 << 32 | コンストラクタ`、2 番から `n` 個のフィールド(宣言の順)。
    Data = 5,
    /// ヴァリアント。1 番がラベル、2 番が積載値。
    Variant = 6,
    /// クロージャ。1 番が関数、2 番から `n` 個の捕獲。
    Closure = 7,
    /// メソッドの値。1 番が `クラス << 32 | メソッド`、2 番が辞書、3 番から `n` 個のメソッド自身の辞書。
    Method = 8,
    /// 前提を持つ利用者のインスタンスの辞書。1 番がインスタンス、2 番から `n` 個の前提の辞書。
    DictInst = 9,
    /// 構造的な `Eq` のレコードの辞書。1 番から `n` 組の(ラベル、辞書)。
    DictRecord = 10,
    /// 構造的な `Eq` のヴァリアントの辞書。配置は `DictRecord` と同じ。
    DictVariant = 11,
    /// `Ref`。1 番が値。
    Ref = 12,
    /// `Array`。1 番から `n` 個の要素。
    Array = 13,
    /// `MutableArray`。1 番から `n` 個の要素。
    MArray = 14,
    /// 実装名が付録 A.5 に無い `extern` の大域の値。1 番が大域の番号。
    ExternUnimpl = 15,
    /// ハンドラ記録。1 番が実行時の表の添字(`crate::effect`)。
    Handler = 16,
    /// 継続。1 番が実行時の表の添字。
    Cont = 17,
    /// 退避した状態語。1 番が実行時の表の添字。
    Pending = 18,
}

impl Kind {
    fn from_u8(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::I64,
            2 => Kind::F64,
            3 => Kind::Str,
            4 => Kind::Record,
            5 => Kind::Data,
            6 => Kind::Variant,
            7 => Kind::Closure,
            8 => Kind::Method,
            9 => Kind::DictInst,
            10 => Kind::DictRecord,
            11 => Kind::DictVariant,
            12 => Kind::Ref,
            13 => Kind::Array,
            14 => Kind::MArray,
            15 => Kind::ExternUnimpl,
            16 => Kind::Handler,
            17 => Kind::Cont,
            18 => Kind::Pending,
            _ => return None,
        })
    }
}

/// 見出しの語を作る。
#[inline]
pub fn header(kind: Kind, n: u32) -> u64 {
    (u64::from(n) << 32) | kind as u64
}

/// 箱の種類。`w` は箱へのポインタでなければならない。
///
/// # Safety
///
/// `w` は実行時が確保した生きている箱を指す。
#[inline]
pub unsafe fn kind_of(w: Word) -> Option<Kind> {
    // SAFETY: 呼び出し側の約束による。
    Kind::from_u8(unsafe { *w.ptr() } as u8)
}

/// 箱の要素の数。
///
/// # Safety
///
/// `w` は実行時が確保した生きている箱を指す。
#[inline]
pub unsafe fn count_of(w: Word) -> u32 {
    // SAFETY: 呼び出し側の約束による。
    (unsafe { *w.ptr() } >> 32) as u32
}

/// 箱の `i` 番の語を読む。
///
/// # Safety
///
/// `w` は実行時が確保した生きている箱を指し、`i` はその箱の語の数より小さい。
#[inline]
pub unsafe fn get(w: Word, i: usize) -> u64 {
    // SAFETY: 呼び出し側の約束による。
    unsafe { *w.ptr().add(i) }
}

/// 箱の `i` 番の語を書く。
///
/// # Safety
///
/// `get` と同じ。
#[inline]
pub unsafe fn set(w: Word, i: usize, v: u64) {
    // SAFETY: 呼び出し側の約束による。
    unsafe { *w.ptr().add(i) = v }
}

/// 文字列の箱のバイト列。
///
/// # Safety
///
/// `w` は生きている文字列の箱を指し、返すスライスを使う間は箱が生きている。
#[inline]
pub unsafe fn str_bytes<'a>(w: Word) -> &'a [u8] {
    // SAFETY: 呼び出し側の約束による。文字列の箱は 1 番の語からバイト列を持つ。
    unsafe {
        let n = count_of(w) as usize;
        std::slice::from_raw_parts(w.ptr().add(1).cast::<u8>(), n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 即値の種類を区別する() {
        for n in [0, 1, -1, i32::MAX, i32::MIN, 12345] {
            let w = Word::i32(n);
            assert!(w.is_i32());
            assert_eq!(w.as_i32(), n);
            assert_eq!(w.imm_kind(), None);
        }
        for (w, k) in [
            (Word::UNIT, ImmKind::Unit),
            (Word::TRUE, ImmKind::Bool),
            (Word::FALSE, ImmKind::Bool),
            (Word::ctor0(3, 7), ImmKind::Ctor0),
            (Word::dict_builtin(1, 2), ImmKind::DictBuiltin),
            (Word::dict_inst0(9), ImmKind::DictInst0),
        ] {
            assert!(!w.is_i32(), "{w:?}");
            assert!(!w.is_ptr());
            assert_eq!(w.imm_kind(), Some(k));
        }
        assert!(Word::TRUE.as_bool());
        assert!(!Word::FALSE.as_bool());
        assert_eq!(Word::ctor0(3, 7).payload_pair(), (3, 7));
        assert_eq!(Word::dict_inst0(9).payload(), 9);
        assert!(!Word::SENTINEL.is_ptr() && !Word::SENTINEL.is_imm());
    }
}
