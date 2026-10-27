//! 分割値スタック(計画 261024-1 の 6.5 節、D527)。
//!
//! フレームは 1 MiB の塊をつないだ値スタックに置く。
//! 塊の中の語には、区間の底(0)から数えた**論理的な位置**を振る。
//! 塊 `i` の先頭の位置は、前の塊の先頭の位置に、前の塊の語の数を足したものである。
//! フレームは塊をまたがない。
//! 塊の残りに入らないフレームは次の塊の先頭に置き、前の塊の残りは使わない。
//!
//! フレームの形は次のとおりで、インタプリタと JIT で同じにする。
//!
//! | 語 | 中身 |
//! | --- | --- |
//! | 0 | 関数の番号(下位 32 ビット)と再開点(上位 32 ビット) |
//! | 1 | フレームの深さ |
//! | 2 | `any` の区画のスロットの数(下位 32 ビット)と生の区画のスロットの数(上位 32 ビット) |
//! | 3 | 呼び出し元のフレームの論理的な位置。ネイティブの呼び出し元へ戻る印は [`NATIVE_CALLER`] |
//! | 4〜 | `any` の区画、続けて生の区画のスロット |
//!
//! 見出しは番地でなく論理的な位置を持つので、継続(M89)が区間を写し戻しても読み直せる。
//! ただし、論理的な位置は塊の境目の場所を含むので、区間を別の底へ一様にずらすと、フレームが塊をまたぎうる。
//! そのため、継続を写し戻すときは、フレームを 1 つずつ [`ValueStack::place`] で置き直し、3 番の語を書き直す。
//! 値スタックの大きさは限度にならない(限度は論理的な深さ)。
//!
//! 塊を取れないとき(システムの確保の失敗)は、プロセスを止めずに [`OutOfMemory`] を返す。
//! 呼び出し側はこれをヒープと同じくメモリの不足(付録 B の 19 番)にする。
//! 最初の塊も最初のフレームを置くときに取るので、空の値スタックを作る操作は失敗しない。
//! 最初の塊を取るときに予備の塊(1 MiB)も取っておく。
//! 塊を取れなかったら、予備を次に使える塊にしてから [`OutOfMemory`] を返す。
//! メモリの不足の誤りを巻き戻す間に走る `cancel` 節は、予備にフレームを積める(計画 6.5 節)。

use std::alloc::{Layout, alloc_zeroed};

use crate::heap::OutOfMemory;

/// フレームの見出しの語の数。
pub const HEADER_WORDS: usize = 4;

/// 呼び出し元がネイティブ(Rust の実行時の関数か、プログラムの開始)であることを表す見出しの 3 番の語。
pub const NATIVE_CALLER: u64 = u64::MAX;

/// 既定の塊の語の数(1 MiB)。
const CHUNK_WORDS: usize = 1 << 17;

struct Chunk {
    mem: Box<[u64]>,
    base: u64,
}

/// 使っている最後の塊の範囲(JIT がフレームを置く速い道で読む)。
///
/// `lo` と `hi` は塊の先頭と終わりの論理的な位置、`ptr` は塊の先頭の語の番地である。
/// 塊が無いときは `lo == hi == 0` で、速い道はどのフレームも置けない。
/// [`ValueStack`] は、使っている塊を変えるたびにこれを書き直すので、いつも使っている最後の塊を表す。
#[repr(C)]
#[derive(Debug, Default)]
pub struct StackWindow {
    /// 塊の先頭の論理的な位置。
    pub lo: u64,
    /// 塊の終わり(最後の語の次)の論理的な位置。
    pub hi: u64,
    /// 塊の先頭の語の番地。
    pub ptr: usize,
}

/// 分割値スタック。
pub struct ValueStack {
    chunks: Vec<Chunk>,
    /// 使っている最後の塊。
    cur: usize,
    /// 取っておいた予備の塊。
    spare: Option<Box<[u64]>>,
    /// 塊を取れなかった後に、次に使う予備の塊。
    armed: Option<Box<[u64]>>,
    /// 使っている最後の塊の範囲。
    win: StackWindow,
}

impl Default for ValueStack {
    fn default() -> Self {
        Self::new()
    }
}

/// `words` 語の、0 で埋めた塊の領域を取る。取れなければ [`OutOfMemory`] を返す。
fn new_mem(words: usize) -> Result<Box<[u64]>, OutOfMemory> {
    let layout = Layout::array::<u64>(words).map_err(|_| OutOfMemory)?;
    // SAFETY: words ≧ CHUNK_WORDS なので layout の大きさは 0 でない。
    let p = unsafe { alloc_zeroed(layout) }.cast::<u64>();
    if p.is_null() {
        return Err(OutOfMemory);
    }
    // SAFETY: p は Layout::array::<u64>(words) で取った、0 で埋めた words 語の領域であり、Box<[u64]> の解放の配置と同じである。
    Ok(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, words)) })
}

impl ValueStack {
    /// 塊を持たない空の値スタック。最初の塊は最初の [`ValueStack::place`] で取る。
    pub fn new() -> ValueStack {
        ValueStack {
            chunks: Vec::new(),
            cur: 0,
            spare: None,
            armed: None,
            win: StackWindow::default(),
        }
    }

    /// 使っている最後の塊の範囲を書き直す。
    #[inline]
    fn sync_win(&mut self) {
        let c = &mut self.chunks[self.cur];
        self.win = StackWindow {
            lo: c.base,
            hi: c.base + c.mem.len() as u64,
            ptr: c.mem.as_mut_ptr() as usize,
        };
    }

    /// 使っている最後の塊の範囲の番地(JIT の文脈に置く)。
    ///
    /// 範囲は値スタックの中にあるので、値スタックが動かない間だけ使える。
    pub fn window_ptr(&mut self) -> *mut StackWindow {
        &mut self.win
    }

    /// 新しい塊の領域。取れなければ予備を次に使える塊にして失敗を返し、予備があればそれを使う。
    fn take_mem(&mut self, words: usize) -> Result<Box<[u64]>, OutOfMemory> {
        if let Some(m) = self.armed.take() {
            if m.len() >= words {
                return Ok(m);
            }
            self.armed = Some(m);
        }
        match new_mem(words.max(CHUNK_WORDS)) {
            Ok(m) => Ok(m),
            Err(e) => {
                if let Some(sp) = self.spare.take() {
                    self.armed = Some(sp);
                }
                Err(e)
            }
        }
    }

    /// つないだ塊の数(試験と計測用)。
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// 論理的な位置 `pos` の語へのポインタ。`pos` はつないだ塊のどれかの中にある。
    ///
    /// 使っている最後の塊より前を指すときは、そこまで塊を戻る(フレームを捨てて呼び出し元へ戻るとき)。
    /// 後ろを指すときは、そこまで塊を進む。
    /// どの塊にも入らない位置は処理系の不具合なので、release の組みでも panic にする(スレッドが落ちて 70 になる)。
    #[inline]
    pub fn ptr_at(&mut self, pos: u64) -> *mut u64 {
        let old = self.cur;
        while pos < self.chunks[self.cur].base {
            self.cur -= 1;
        }
        while self.cur + 1 < self.chunks.len() && pos >= self.chunks[self.cur + 1].base {
            self.cur += 1;
        }
        if self.cur != old {
            self.sync_win();
        }
        let c = &mut self.chunks[self.cur];
        let off = (pos - c.base) as usize;
        assert!(
            off < c.mem.len(),
            "値スタックの位置 {pos} がどの塊にも入りません"
        );
        // SAFETY: off は塊の中にある。
        unsafe { c.mem.as_mut_ptr().add(off) }
    }

    /// 論理的な位置 `pos` の語へのポインタを、使っている塊を変えずに求める(GC がフレームをたどるとき)。
    ///
    /// [`ValueStack::ptr_at`] は使っている塊を動かし、それは次の [`ValueStack::place`] の置き場所に効くので、
    /// 実行の途中の GC はこちらを使う。
    /// どの塊にも入らない位置は処理系の不具合なので panic にする。
    pub fn ptr_at_ro(&self, pos: u64) -> *const u64 {
        let i = self.chunks.partition_point(|c| c.base <= pos);
        assert!(i > 0, "値スタックの位置 {pos} がどの塊にも入りません");
        let c = &self.chunks[i - 1];
        let off = (pos - c.base) as usize;
        assert!(
            off < c.mem.len(),
            "値スタックの位置 {pos} がどの塊にも入りません"
        );
        // SAFETY: off は塊の中にある。
        unsafe { c.mem.as_ptr().add(off) }
    }

    /// 位置 `top`(今のフレームの終わり)の後に `words` 語のフレームを置く場所を決め、その論理的な位置を返す。
    ///
    /// 今の塊に入らなければ次の塊(無ければ新しく作る)の先頭にする。
    /// 新しい塊を取れなければ [`OutOfMemory`] を返し、値スタックは変えない。
    pub fn place(&mut self, top: u64, words: usize) -> Result<u64, OutOfMemory> {
        if self.chunks.is_empty() {
            debug_assert_eq!(top, 0);
            if self.spare.is_none() && self.armed.is_none() {
                self.spare = Some(new_mem(CHUNK_WORDS)?);
            }
            let mem = self.take_mem(words)?;
            self.chunks.push(Chunk { mem, base: 0 });
            self.cur = 0;
            self.sync_win();
            return Ok(0);
        }
        let old = self.cur;
        while self.cur > 0 && top < self.chunks[self.cur].base {
            self.cur -= 1;
        }
        if self.cur != old {
            self.sync_win();
        }
        {
            let c = &self.chunks[self.cur];
            if top >= c.base {
                let off = (top - c.base) as usize;
                if off + words <= c.mem.len() {
                    return Ok(top);
                }
            }
        }
        let next_base = {
            let c = &self.chunks[self.cur];
            c.base + c.mem.len() as u64
        };
        let i = self.cur + 1;
        if i < self.chunks.len() && self.chunks[i].mem.len() >= words {
            self.chunks[i].base = next_base;
            self.chunks.truncate(i + 1);
        } else {
            let mem = self.take_mem(words)?;
            self.chunks.truncate(i);
            self.chunks.push(Chunk {
                mem,
                base: next_base,
            });
        }
        self.cur = i;
        self.sync_win();
        Ok(next_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 範囲 `win` が使っている塊 `chunks[cur]` と一致することを確かめる(JIT のコードが頼る性質)。
    fn check_win(s: &ValueStack) {
        if s.chunks.is_empty() {
            return;
        }
        let c = &s.chunks[s.cur];
        assert_eq!(s.win.lo, c.base);
        assert_eq!(s.win.hi, c.base + c.mem.len() as u64);
        assert_eq!(s.win.ptr, c.mem.as_ptr() as usize);
    }

    #[test]
    fn 入らないフレームは次の塊に置き戻ると前の塊に戻る() {
        let mut s = ValueStack::new();
        assert_eq!(s.chunk_count(), 0);
        let a = s.place(0, 10).unwrap();
        check_win(&s);
        assert_eq!(a, 0);
        assert_eq!(s.chunk_count(), 1);
        let b = s.place(10, CHUNK_WORDS - 5).unwrap();
        check_win(&s);
        assert_eq!(b, CHUNK_WORDS as u64);
        assert_eq!(s.chunk_count(), 2);
        let p = s.ptr_at(b);
        check_win(&s);
        // SAFETY: p は塊の中を指す。
        unsafe { *p = 7 };
        let q = s.ptr_at(a);
        check_win(&s);
        // SAFETY: q は塊の中を指す。
        unsafe { *q = 3 };
        // 塊 0 に戻ったので、次のフレームは塊 0 の続きに置ける。
        assert_eq!(s.place(10, 10), Ok(10));
        check_win(&s);
        // 大きなフレームは専用の大きさの塊を作る。
        let big = s.place(20, CHUNK_WORDS * 3).unwrap();
        check_win(&s);
        assert_eq!(big, CHUNK_WORDS as u64);
        let p = s.ptr_at(big + (CHUNK_WORDS * 3 - 1) as u64);
        check_win(&s);
        // SAFETY: p は塊の中を指す。
        unsafe { *p = 1 };
    }

    #[test]
    fn 塊を取れなければ値スタックを変えずに失敗を返す() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert_eq!(s.chunk_count(), 0);
        assert_eq!(s.place(0, 10), Ok(0));
        check_win(&s);
        assert_eq!(s.place(10, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert_eq!(s.chunk_count(), 1);
        assert_eq!(s.place(10, 10), Ok(10));
        check_win(&s);
    }

    #[test]
    fn 塊を取れなかった後は予備の塊にフレームを置ける() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, 10), Ok(0));
        check_win(&s);
        assert!(s.spare.is_some());
        assert_eq!(s.place(10, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert!(s.armed.is_some() && s.spare.is_none());
        // 今の塊に入らないフレームは、予備の塊に置く。
        assert_eq!(s.place(10, CHUNK_WORDS), Ok(CHUNK_WORDS as u64));
        check_win(&s);
        assert!(s.armed.is_none());
        assert_eq!(s.chunk_count(), 2);
    }

    #[test]
    fn 範囲は次の塊の使い直しと戻った後の失敗でも使っている塊と一致する() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, 10), Ok(0));
        check_win(&s);
        let b = s.place(10, CHUNK_WORDS - 5).unwrap();
        check_win(&s);
        assert_eq!(s.chunk_count(), 2);
        // 塊 0 に戻ってから入らないフレームを置くと、残っている塊 1 を使い直す。
        s.ptr_at(0);
        check_win(&s);
        assert_eq!(s.place(10, CHUNK_WORDS - 5), Ok(b));
        check_win(&s);
        assert_eq!(s.chunk_count(), 2);
        // 塊 1 から塊 0 の位置に戻って置けずに失敗しても、範囲は戻った先の塊 0 を指す。
        assert_eq!(s.place(10, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert_eq!(s.cur, 0);
        // 使い直した塊にもう一度進む。
        s.ptr_at(b);
        check_win(&s);
        assert_eq!(s.cur, 1);
    }
}
