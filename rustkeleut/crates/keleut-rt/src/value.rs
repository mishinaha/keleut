//! 一様表現の値のエンコーディングと、ヒープのオブジェクトのレイアウト。
//!
//! 一様表現の値(Rep `any`。値の種類によらず 1 ワードで持つ表現)は 64 ビットのワードである。
//! エンコーディングはバックエンドが決めるもので(`lir.md` 1.5 節)、このモジュールはインタプリタと JIT が使うものを定める。
//! ワード `w` のビットパターンと意味は次のとおりである。
//!
//! | ワード `w` のビットパターン | 意味 |
//! | --- | --- |
//! | 最下位ビットが 1 で、`(w as i64) >> 1` が `i32` に収まる | `Int32` の即値 `(n << 1) \| 1` |
//! | 最下位ビットが 1 で、上位の 8 ビットが `0x40 \| 種類` | `Int32` 以外の即値(Unit、真偽値、引数のないコンストラクタ、辞書) |
//! | 下位 3 ビットが 0 で、全体は 0 でない | 8 バイト境界に置いたオブジェクトへのポインタ |
//! | `0x2` | 巻き戻しの番兵(`lir.md` 16.1 節)。値としては現れず、レジスタに置くのは `$rt_handle`(ハンドラのフレームの関数。`lir.md` 16.3 節)の中だけである |
//!
//! `Int32` 以外の即値は上位の 2 ビットが `01` なので、符号拡張した `Int32` の即値と重ならない。
//! 即値の種類を上位のビットに置くのは、診断に値を表示するとき(IRSpec 付録 B の `<値>`)に、即値の種類をワードだけから判別する必要があるからである。
//!
//! オブジェクトの先頭のワード(ヘッダ)は、種類(下位 8 ビット)、GC のマーク(次の 8 ビット)、要素数(上位 32 ビット)を持つ。
//! 種類ごとのレイアウトは [`Kind`] の各バリアントに書く。

use std::fmt;

/// 一様表現の値を 1 個持つ 64 ビットのワード。
/// 巻き戻しの番兵もこの型で表す。
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
    /// 真偽値。ペイロードは 0 か 1。
    Bool = 1,
    /// 引数のないコンストラクタ。ペイロードは型(24 ビット)とコンストラクタ(24 ビット)。
    Ctor0 = 2,
    /// 組み込みのインスタンスの辞書。ペイロードはクラス(24 ビット)と型(24 ビット)。
    DictBuiltin = 3,
    /// 前提を持たないユーザ定義のインスタンスの辞書。ペイロードはインスタンスの番号。
    DictInst0 = 4,
}

const IMM_TAG: u64 = 0x40;
const PAYLOAD_MASK: u64 = (1u64 << 55) - 1;
const FIELD24: u64 = (1u64 << 24) - 1;

impl Word {
    /// Unit の即値。
    pub const UNIT: Word = Word::imm(ImmKind::Unit, 0);
    /// 偽の即値。
    pub const FALSE: Word = Word::imm(ImmKind::Bool, 0);
    /// 真の即値。
    pub const TRUE: Word = Word::imm(ImmKind::Bool, 1);
    /// 巻き戻しの番兵。
    /// 値としては現れない。
    pub const SENTINEL: Word = Word(0x2);

    const fn imm(kind: ImmKind, payload: u64) -> Word {
        Word(((IMM_TAG | kind as u64) << 56) | ((payload & PAYLOAD_MASK) << 1) | 1)
    }

    /// `Int32` の即値を作る。
    #[inline]
    pub fn i32(n: i32) -> Word {
        Word((((n as i64) << 1) | 1) as u64)
    }

    /// 真偽値の即値を作る。
    #[inline]
    pub fn bool(b: bool) -> Word {
        if b { Word::TRUE } else { Word::FALSE }
    }

    /// 引数のないコンストラクタの即値を作る。
    /// 型とコンストラクタの番号は 2^24 未満でなければならない。
    pub fn ctor0(ty: u32, ctor: u32) -> Word {
        assert!(u64::from(ty) <= FIELD24 && u64::from(ctor) <= FIELD24);
        Word::imm(ImmKind::Ctor0, (u64::from(ty) << 24) | u64::from(ctor))
    }

    /// 組み込みのインスタンスの辞書の即値を作る。
    /// クラスと型の番号は 2^24 未満でなければならない。
    pub fn dict_builtin(class: u32, ty: u32) -> Word {
        assert!(u64::from(class) <= FIELD24 && u64::from(ty) <= FIELD24);
        Word::imm(
            ImmKind::DictBuiltin,
            (u64::from(class) << 24) | u64::from(ty),
        )
    }

    /// 前提を持たないユーザ定義のインスタンスの辞書の即値を作る。
    pub fn dict_inst0(inst: u32) -> Word {
        Word::imm(ImmKind::DictInst0, u64::from(inst))
    }

    /// 即値であれば真を返す。
    #[inline]
    pub fn is_imm(self) -> bool {
        self.0 & 1 == 1
    }

    /// オブジェクトへのポインタであれば真を返す。
    #[inline]
    pub fn is_ptr(self) -> bool {
        self.0 & 7 == 0 && self.0 != 0
    }

    /// `Int32` の即値であれば真を返す。
    #[inline]
    pub fn is_i32(self) -> bool {
        let v = (self.0 as i64) >> 1;
        self.is_imm() && v == v as i32 as i64
    }

    /// `Int32` の即値が表す整数を返す。
    /// `Int32` の即値であることは呼び出し元が保証し、この関数は検査しない。
    #[inline]
    pub fn as_i32(self) -> i32 {
        ((self.0 as i64) >> 1) as i32
    }

    /// 真偽値の即値が表す真偽値を返す。
    /// 真偽値の即値であることは呼び出し元が保証し、この関数は検査しない。
    #[inline]
    pub fn as_bool(self) -> bool {
        (self.0 >> 1) & 1 == 1
    }

    /// `Int32` 以外の即値の種類を返す。
    /// そのような即値でなければ `None` を返す。
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

    /// 即値のペイロード(55 ビット)を返す。
    #[inline]
    pub fn payload(self) -> u64 {
        (self.0 >> 1) & PAYLOAD_MASK
    }

    /// ペイロードの下位 48 ビットを、上位 24 ビットと下位 24 ビットに分け、タプルで返す。
    /// `Ctor0` と `DictBuiltin` の即値に使う。
    pub fn payload_pair(self) -> (u32, u32) {
        let p = self.payload();
        (((p >> 24) & FIELD24) as u32, (p & FIELD24) as u32)
    }

    /// オブジェクトへのポインタを返す。
    #[inline]
    pub fn ptr(self) -> *mut u64 {
        self.0 as *mut u64
    }

    /// オブジェクトへのポインタからワードを作る。
    #[inline]
    pub fn from_ptr(p: *mut u64) -> Word {
        Word(p as u64)
    }
}

/// オブジェクトの種類(ヘッダの下位 8 ビット)。
///
/// オブジェクトはワードの並びで、0 番のワードはヘッダである。
/// 各バリアントの説明の「`i` 番」はオブジェクトの `i` 番のワードを指し、`n` はヘッダが持つ要素数である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// `Int64`。1 番が値。
    I64 = 1,
    /// `Float64`。1 番がビット列。
    F64 = 2,
    /// 文字列。`n` がバイト数で、1 番からバイト列(領域は 8 バイト単位に切り上げる)。
    Str = 3,
    /// レコード。1 番がシェイプの番号、2 番から `n` 個のフィールド(シェイプの順)。
    Record = 4,
    /// 名目的な型の値のうち、引数のあるコンストラクタで作ったもの(`lir.md` 8 節のデータ)。1 番が `型 << 32 | コンストラクタ`、2 番から `n` 個のフィールド(宣言の順)。
    Data = 5,
    /// ヴァリアント。1 番がラベル、2 番がペイロード。
    Variant = 6,
    /// クロージャ。1 番が関数、2 番から `n` 個のキャプチャ。
    Closure = 7,
    /// メソッドの値。1 番が `クラス << 32 | メソッド`、2 番が辞書、3 番から `n` 個のメソッド自身の辞書。
    Method = 8,
    /// ユーザ定義の前提付きインスタンスの辞書。1 番がインスタンス、2 番から `n` 個の前提の辞書。
    DictInst = 9,
    /// 構造的な `Eq` のレコードの辞書。1 番から `n` 組の(ラベル、辞書)。
    DictRecord = 10,
    /// 構造的な `Eq` のヴァリアントの辞書。レイアウトは `DictRecord` と同じ。
    DictVariant = 11,
    /// `Ref`。1 番が値。
    Ref = 12,
    /// `Array`。1 番から `n` 個の要素。
    Array = 13,
    /// `MutableArray`。1 番から `n` 個の要素。
    MArray = 14,
    /// 実装名が IRSpec 付録 A.5 にない `extern` のグローバル変数の値。1 番がグローバルの番号。
    ExternUnimpl = 15,
    /// ハンドラ記録。1 番がランタイムの表の添字(`crate::effect`)。
    Handler = 16,
    /// 継続。1 番がランタイムの表の添字。
    Cont = 17,
    /// 退避した状態ワード。1 番がランタイムの表の添字。
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

/// 種類 `kind` と要素数 `n` からヘッダのワードを作る。
/// GC のマークは立てない。
#[inline]
pub fn header(kind: Kind, n: u32) -> u64 {
    (u64::from(n) << 32) | kind as u64
}

/// オブジェクトの種類を返す。
/// ヘッダの種類が [`Kind`] のどれでもなければ `None` を返す。
///
/// # Safety
///
/// `w` は、ランタイムが確保した、生きているオブジェクトへのポインタである。
#[inline]
pub unsafe fn kind_of(w: Word) -> Option<Kind> {
    // SAFETY: 呼び出し元が、w は生きているオブジェクトを指すという事前条件を満たす。
    Kind::from_u8(unsafe { *w.ptr() } as u8)
}

/// オブジェクトのヘッダが持つ要素数を返す。
///
/// # Safety
///
/// `w` は、ランタイムが確保した、生きているオブジェクトへのポインタである。
#[inline]
pub unsafe fn count_of(w: Word) -> u32 {
    // SAFETY: 呼び出し元が、w は生きているオブジェクトを指すという事前条件を満たす。
    (unsafe { *w.ptr() } >> 32) as u32
}

/// オブジェクトの `i` 番のワードを読む。
///
/// # Safety
///
/// `w` は、ランタイムが確保した、生きているオブジェクトへのポインタであり、`i` はそのオブジェクトのワード数より小さい。
#[inline]
pub unsafe fn get(w: Word, i: usize) -> u64 {
    // SAFETY: 呼び出し元が、w は生きているオブジェクトを指し、i はそのワード数より小さいという事前条件を満たす。
    unsafe { *w.ptr().add(i) }
}

/// オブジェクトの `i` 番のワードに `v` を書く。
///
/// # Safety
///
/// 事前条件は `get` と同じである。
#[inline]
pub unsafe fn set(w: Word, i: usize, v: u64) {
    // SAFETY: 呼び出し元が、w は生きているオブジェクトを指し、i はそのワード数より小さいという事前条件を満たす。
    unsafe { *w.ptr().add(i) = v }
}

/// 文字列オブジェクトのバイト列を返す。
///
/// # Safety
///
/// `w` は生きている文字列オブジェクトへのポインタであり、返したスライスを使う間はそのオブジェクトが生きている。
#[inline]
pub unsafe fn str_bytes<'a>(w: Word) -> &'a [u8] {
    // SAFETY: 呼び出し元が、w は生きている文字列オブジェクトを指すという事前条件を満たす。
    // 文字列オブジェクトは 1 番のワードからバイト列を持つ。
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
