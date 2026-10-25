//! ヒープ(計画 261024-1 の 6.5 節)。
//!
//! 箱は 8 バイト境界の語の列で、動かない。
//! M87 の確保は回収をしない。
//! 大きな塊(既定は 8 MiB)を順に切り出す。
//! 今の塊の残りに収まらない箱は、塊の 4 分の 1(2 MiB)より大きければ専用の塊を取り、今の塊はそのまま使い続ける。
//! それ以下なら残りを捨てて新しい塊を取る。
//! 回収(動かさないマークスイープ、D526)は M90 で、この確保の関数を大きさの種類ごとのページと空きリストに置き換える。
//! 箱の見出しは GC の印の欄を持ち([`crate::value::header`])、根の規律の `Root`([`Roots`])は今から使う。
//!
//! 塊を取れないとき(システムの確保の失敗)は、プロセスを止めずに [`OutOfMemory`] を返す。
//! 呼び出し側はこれをメモリの不足(付録 B の 19 番)にする。
//!
//! メモリの不足の誤りを巻き戻す間に `cancel` 節が走れるよう、最初の確保のときに予備の塊([`RESERVE_WORDS`] 語)を取っておく
//! (計画 6.5 節)。
//! 塊を取れなかったら、予備を今の塊にしてから [`OutOfMemory`] を返す。
//! その後の確保は予備から切り出す。
//! 予備に入らない確保は、システムからもう一度塊を取りにいき、取れなければ [`OutOfMemory`] を返す。
//! 取れたら予備の扱いをやめ、次の確保で新しい予備を取っておく。
//! 専用の塊を取る大きな箱の失敗は、ほかの確保の余地を減らさないので、予備に切り替えない。
//! `cancel` 節の中のこの誤りは、ほかの脱出と同じく打ち切られる。

use std::alloc::{Layout, alloc, dealloc};

/// 確保の失敗。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfMemory;

/// 既定の塊の語の数(8 MiB)。
const CHUNK_WORDS: usize = 1 << 20;

/// メモリの不足の後に使う予備の塊の語の数(1 MiB)。
pub const RESERVE_WORDS: usize = 1 << 17;

/// 回収をしない確保器。
pub struct Heap {
    chunks: Vec<(*mut u64, Layout)>,
    cur: *mut u64,
    end: *mut u64,
    allocated_words: u64,
    /// 試験のための確保の上限(語の数)。超えたら塊を取れないときと同じに扱う。
    limit_words: Option<u64>,
    /// 予備の塊(取っておいたもの)。
    reserve: Option<*mut u64>,
    /// 予備を使い始めたか。
    in_reserve: bool,
}

// SAFETY: Heap は塊を所有し、塊はほかから参照されない。ミューテータは 1 本で、Heap はそのスレッドへ移すだけである。
unsafe impl Send for Heap {}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    /// 空のヒープ。
    pub fn new() -> Heap {
        Heap {
            chunks: Vec::new(),
            cur: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            allocated_words: 0,
            limit_words: None,
            reserve: None,
            in_reserve: false,
        }
    }

    /// 予備を使い始めたか(試験用)。
    pub fn in_reserve(&self) -> bool {
        self.in_reserve
    }

    /// 確保に失敗した。予備が残っていれば今の塊にしてから失敗を返す。
    ///
    /// 専用の塊を取る大きさ(塊の 4 分の 1 より大きい)の失敗は、予備に切り替えずに失敗を返す。
    fn exhausted(&mut self, words: usize) -> OutOfMemory {
        if words > CHUNK_WORDS / 4 {
            return OutOfMemory;
        }
        if let Some(p) = self.reserve.take() {
            self.cur = p;
            // SAFETY: 予備の塊は RESERVE_WORDS 語である。
            self.end = unsafe { p.add(RESERVE_WORDS) };
            self.in_reserve = true;
            if let Some(l) = &mut self.limit_words {
                *l = l.saturating_add(RESERVE_WORDS as u64);
            }
        }
        OutOfMemory
    }

    /// 確保した語の数の上限を決める(試験用)。
    pub fn set_limit_words(&mut self, limit: Option<u64>) {
        self.limit_words = limit;
    }

    /// これまでに確保した語の数。
    pub fn allocated_words(&self) -> u64 {
        self.allocated_words
    }

    fn new_chunk(&mut self, words: usize) -> Result<*mut u64, OutOfMemory> {
        let bytes = words.checked_mul(8).ok_or(OutOfMemory)?;
        let layout = Layout::from_size_align(bytes, 8).map_err(|_| OutOfMemory)?;
        // SAFETY: layout の大きさは 0 でない(words ≧ 1)。
        let p = unsafe { alloc(layout) }.cast::<u64>();
        if p.is_null() {
            return Err(OutOfMemory);
        }
        self.chunks.push((p, layout));
        Ok(p)
    }

    /// `words` 語(見出しを含む。1 以上)の箱の領域を確保する。中身は初期化しない。
    #[inline]
    pub fn alloc(&mut self, words: usize) -> Result<*mut u64, OutOfMemory> {
        debug_assert!(words >= 1);
        if self.reserve.is_none() && !self.in_reserve {
            self.reserve = Some(self.new_chunk(RESERVE_WORDS)?);
        }
        if let Some(l) = self.limit_words
            && self.allocated_words + words as u64 > l
        {
            return Err(self.exhausted(words));
        }
        let room = (self.end as usize - self.cur as usize) / 8;
        if words <= room {
            let p = self.cur;
            // SAFETY: p から words 語は今の塊の中にある。
            self.cur = unsafe { p.add(words) };
            self.allocated_words += words as u64;
            return Ok(p);
        }
        // 予備を使っている間も塊を取りにいく。取れたら予備の扱いをやめる。
        if words > CHUNK_WORDS / 4 {
            let p = self.new_chunk(words).map_err(|_| self.exhausted(words))?;
            self.in_reserve = false;
            self.allocated_words += words as u64;
            return Ok(p);
        }
        let p = self
            .new_chunk(CHUNK_WORDS)
            .map_err(|_| self.exhausted(words))?;
        self.in_reserve = false;
        // SAFETY: 新しい塊は CHUNK_WORDS 語で、words はそれより小さい。
        unsafe {
            self.cur = p.add(words);
            self.end = p.add(CHUNK_WORDS);
        }
        self.allocated_words += words as u64;
        Ok(p)
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        for &(p, layout) in &self.chunks {
            // SAFETY: 各塊は同じ layout で alloc したものである。
            unsafe { dealloc(p.cast(), layout) };
        }
    }
}

/// Rust の実行時の関数が、確保をまたいで値を持つための根の枠(計画 6.5 節の `Root`)。
///
/// 値を積み、使い終えたら積む前の長さに戻す。
/// M87 には回収が無いので、根は読まれない。
/// M90 の GC は、この列を根として走査する。
#[derive(Default)]
pub struct Roots {
    stack: Vec<u64>,
}

impl Roots {
    /// 値を積み、積む前の長さを返す。
    pub fn push(&mut self, w: u64) -> usize {
        self.stack.push(w);
        self.stack.len() - 1
    }

    /// 積んだ値を読む。
    pub fn get(&self, i: usize) -> u64 {
        self.stack[i]
    }

    /// 長さを `len` に戻す。
    pub fn truncate(&mut self, len: usize) {
        self.stack.truncate(len);
    }

    /// 今の長さ。
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// 空か。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 境界にそろった領域を返し大きな箱にも塊を取る() {
        let mut h = Heap::new();
        let a = h.alloc(3).unwrap();
        let b = h.alloc(5).unwrap();
        assert_eq!(a as usize % 8, 0);
        assert_eq!(b as usize - a as usize, 24);
        let big = h.alloc(CHUNK_WORDS).unwrap();
        assert_eq!(big as usize % 8, 0);
        let c = h.alloc(1).unwrap();
        assert_eq!(c as usize - b as usize, 40);
        assert_eq!(h.allocated_words(), 9 + CHUNK_WORDS as u64);
        h.set_limit_words(Some(h.allocated_words() + 1));
        assert!(h.alloc(1).is_ok());
        assert_eq!(h.alloc(1), Err(OutOfMemory));
    }

    #[test]
    fn 確保の失敗の後は予備から切り出し予備も尽きたら失敗する() {
        let mut h = Heap::new();
        h.set_limit_words(Some(10));
        assert!(h.alloc(10).is_ok());
        assert!(!h.in_reserve());
        assert_eq!(h.alloc(1), Err(OutOfMemory));
        assert!(h.in_reserve());
        // 予備の中では確保が通る。
        for _ in 0..(RESERVE_WORDS / 1024) {
            assert!(h.alloc(1024).is_ok());
        }
        assert_eq!(h.alloc(1), Err(OutOfMemory));
    }

    #[test]
    fn 大きな箱の失敗は予備に切り替えない() {
        let mut h = Heap::new();
        h.set_limit_words(Some(100));
        assert_eq!(h.alloc(CHUNK_WORDS), Err(OutOfMemory));
        assert!(!h.in_reserve());
        assert!(h.alloc(50).is_ok());
        assert!(!h.in_reserve());
    }

    #[test]
    fn 予備の後もシステムから塊を取れれば予備の扱いをやめる() {
        let mut h = Heap::new();
        h.set_limit_words(Some(10));
        assert!(h.alloc(10).is_ok());
        assert_eq!(h.alloc(1), Err(OutOfMemory));
        assert!(h.in_reserve());
        // メモリが空いた(試験の上限を外す)。予備に入らない確保は新しい塊から取る。
        h.set_limit_words(None);
        assert!(h.alloc(RESERVE_WORDS + 1).is_ok());
        assert!(!h.in_reserve());
        // 次の確保で予備を取り直し、その後の失敗でまた予備に切り替わる。
        assert!(h.alloc(1).is_ok());
        h.set_limit_words(Some(h.allocated_words()));
        assert_eq!(h.alloc(1), Err(OutOfMemory));
        assert!(h.in_reserve());
        assert!(h.alloc(1).is_ok());
    }
}
