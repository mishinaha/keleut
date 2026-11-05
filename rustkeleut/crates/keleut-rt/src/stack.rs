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
//! 継続にメモリの不足を注入するときは、置き直さずに、継続の写しの領域をそのまま 1 つの塊としてつなぐ([`ValueStack::link_copy`])。
//! そのため、塊の大きさは塊ごとに異なりうる。
//!
//! 戻るときには塊を返さない。
//! 回収の後に、生きているフレームの最も上を含む塊の次の 1 つより後ろの塊を返す([`ValueStack::release_after_collect`])。
//! 値スタックの大きさは限度にならない(限度は論理的な深さ)。
//!
//! 塊を取れないとき(システムの確保の失敗)は、プロセスを止めずに [`OutOfMemory`] を返す。
//! 呼び出し側はこれをヒープと同じくメモリの不足(付録 B の 19 番)にする。
//! 最初の塊も最初のフレームを置くときに取るので、空の値スタックを作る操作は失敗しない。
//! 最初の塊を取るときに予備の塊(1 MiB)も取っておく。
//! 塊を取れなかったら、予備を次に使える塊にしてから [`OutOfMemory`] を返す。
//! 塊は大域の確保器から取るので、確保器が予備を返してやり直したときも、塊を取った後に実行の役割の印を見てメモリの不足にする
//! ([`crate::alloc`]、計画 261104-3 の D568)。
//! メモリの不足の誤りを巻き戻す間に走る `cancel` 節は、予備にフレームを積める(計画 6.5 節)。

use std::alloc::{Layout, alloc_zeroed};

use crate::heap::OutOfMemory;

/// フレームの見出しの語の数。
pub const HEADER_WORDS: usize = 4;

/// 呼び出し元がネイティブ(Rust の実行時の関数か、プログラムの開始)であることを表す見出しの 3 番の語。
pub const NATIVE_CALLER: u64 = u64::MAX;

/// 既定の塊の語の数(1 MiB)。
const CHUNK_WORDS: usize = 1 << 17;

/// つないだ塊。
///
/// 塊の語の数は `mem` の長さである。
/// 継続の写しをつないだ塊([`ValueStack::link_copy`])は、写しの `Vec` の長さちょうどで、[`CHUNK_WORDS`] より小さいことがある。
struct Chunk {
    mem: Vec<u64>,
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
///
/// 確保は大域の確保器を通り、失敗を自分で扱う範囲の外で行う(`crate::alloc`)。
/// 失敗すれば大域の予備を返してやり直すので、その後は実行の役割の印が立ちうる。
/// 印は呼び手([`ValueStack::take_mem`])が見る。
fn new_mem(words: usize) -> Result<Box<[u64]>, OutOfMemory> {
    let _site = crate::alloc::site_if_none(crate::alloc::Site::Chunk);
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
    ///
    /// 取った後に実行の役割の印を見て下ろす(`crate::alloc`)。
    /// 印が立っていれば、大域の確保器が予備を返したか、ほかの確保が先に予備を使ったので、メモリの不足にする。
    /// 取れた塊は次に使える塊にするので、予備の使われ方の順は、大域の予備、取れた塊、値スタックの予備の塊となる。
    /// 取れなければ、印の有無によらず印を下ろし、値スタックの予備の塊を次に使える塊にする(1 回のメモリの不足に 1 回だけ応える)。
    fn take_mem(&mut self, words: usize) -> Result<Box<[u64]>, OutOfMemory> {
        if let Some(m) = self.armed.take() {
            if m.len() >= words {
                return Ok(m);
            }
            self.armed = Some(m);
        }
        let r = new_mem(words.max(CHUNK_WORDS));
        let flagged = crate::alloc::take_flag(crate::alloc::Role::Exec);
        match r {
            Ok(m) if !flagged => Ok(m),
            Ok(m) => {
                self.armed = Some(m);
                Err(OutOfMemory)
            }
            Err(e) => {
                if let Some(sp) = self.spare.take() {
                    self.armed = Some(sp);
                }
                Err(e)
            }
        }
    }

    /// 回収の後に、使っていない塊を返す。
    ///
    /// `live` は生きているフレームのうち最も上にあるものの論理的な位置で、無ければ [`NATIVE_CALLER`] である。
    /// そのフレームを含む塊と、その次の 1 つの塊を残し、それより後ろの塊を捨てる。
    /// 次の 1 つを残すのは、塊の境目で呼び出しと戻りを繰り返すときに、塊を取り直し続けないためである。
    /// 次の 1 つが写しの塊で、後ろに既定の大きさ以上の塊があれば、写しの塊を捨ててその塊を残す。
    /// ただし、使っている最後の塊が次の 1 つなら、置いたばかりのフレームがありうるので、写しの塊のまま残す。
    /// 予備の塊は捨てない。
    ///
    /// 使っている最後の塊が捨てた塊なら、残した最後の塊に下ろして範囲を書き直す。
    /// JIT のコードは戻るときに実行時を呼ばないので、使っている最後の塊は最上段のフレームより後ろに残りうる。
    /// JIT のコードは範囲を置き場所を決めるたびに文脈の `win` から読むので、書き直した範囲は次の置き場所から効く。
    /// 置いたばかりで最上段にまだなっていないフレームは、置いたフレームの塊かその次の塊にあり、どちらも残る。
    pub fn release_after_collect(&mut self, live: u64) {
        if self.chunks.is_empty() || live == NATIVE_CALLER {
            return;
        }
        let i = self.chunks.partition_point(|c| c.base <= live);
        let n = i.max(1);
        let keep = n + 1;
        if self.chunks.len() <= keep {
            return;
        }
        // 使っている最後の塊が次の塊なら、置いたばかりのフレームがそこにありうるので、写しの塊でも残す。
        let moved = self.cur != n && self.drop_copies_before_full(n);
        self.chunks.truncate(keep);
        if moved && self.cur > n {
            self.cur = n;
        }
        if self.cur >= keep {
            self.cur = keep - 1;
        }
        if moved || self.cur == keep - 1 {
            self.sync_win();
        }
    }

    /// 塊 `i` 以降がどれも使っていない塊であるときに、塊 `i` が写しの塊([`CHUNK_WORDS`] より小さい)で、後ろに既定の大きさ以上の塊があれば、その手前までの塊を捨てる。
    ///
    /// 捨てたら、既定の大きさ以上の塊を塊 `i` にし、前の塊の終わりを先頭の位置にして真を返す。
    /// 写しの塊を残して既定の大きさの塊を捨てると、次に塊の境目を越えるフレームが塊を取り直すので、注入のたびに塊を取ることになる。
    /// 捨てる塊の領域は返すだけで、確保はしない。
    fn drop_copies_before_full(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.chunks.len() || self.chunks[i].mem.len() >= CHUNK_WORDS {
            return false;
        }
        let Some(k) = self.chunks[i + 1..]
            .iter()
            .position(|c| c.mem.len() >= CHUNK_WORDS)
        else {
            return false;
        };
        self.chunks.drain(i..i + 1 + k);
        let p = &self.chunks[i - 1];
        self.chunks[i].base = p.base + p.mem.len() as u64;
        true
    }

    /// 継続の写し `mem` を、位置 `top`(今のフレームの終わり)を含む塊の次の塊としてつなぎ、その先頭の論理的な位置を返す。
    ///
    /// 継続にメモリの不足を注入するとき([`ValueStack::place`] でフレームを 1 つずつ置き直す代わり)に使う。
    /// 写しのフレームは最下段から順に隙間なく並んでいるので、塊の中の位置がそのままフレームの位置になる。
    /// 見出しの 1 番と 3 番の語は呼び手が書き直す。
    /// 今の塊の `top` から後ろの残りは使わない。
    ///
    /// 写しの領域をそのまま塊にするので、塊の領域は取らない。
    /// それまでの次の塊は、つないだ塊の次に移して残す(巻き戻しの中の `cancel` 節のフレームが、写しの塊に入らないときに使う)。
    /// それまでの次の塊が前の注入の写しの塊で、後ろに既定の大きさ以上の塊があれば、写しの塊を捨ててその塊を移す。
    /// 塊の列を広げる確保だけが要りうる。
    /// それを取れなければ、`mem` を返して値スタックを変えない。
    pub fn link_copy(&mut self, top: u64, mem: Vec<u64>) -> Result<u64, (OutOfMemory, Vec<u64>)> {
        assert!(
            !self.chunks.is_empty() && !mem.is_empty(),
            "空の値スタックか空の写しをつなごうとしました"
        );
        while self.cur > 0 && top < self.chunks[self.cur].base {
            self.cur -= 1;
        }
        let i = self.cur + 1;
        if self.chunks.len() <= i
            && self.chunks.len() == self.chunks.capacity()
            && crate::alloc::handled(|| self.chunks.try_reserve(1)).is_err()
        {
            self.sync_win();
            return Err((OutOfMemory, mem));
        }
        self.drop_copies_before_full(i);
        let base = {
            let c = &self.chunks[self.cur];
            c.base + c.mem.len() as u64
        };
        let next = (i < self.chunks.len()).then(|| self.chunks.swap_remove(i));
        self.chunks.truncate(i);
        let end = base + mem.len() as u64;
        self.chunks.push(Chunk { mem, base });
        if let Some(mut n) = next
            && crate::alloc::handled(|| self.chunks.try_reserve(1)).is_ok()
        {
            n.base = end;
            self.chunks.push(n);
        }
        self.cur = i;
        self.sync_win();
        Ok(base)
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
    /// 次の塊が写しの塊で、後ろに既定の大きさ以上の塊があれば、写しの塊を捨ててその塊を次の塊にする。
    /// 新しい塊を取れなければ [`OutOfMemory`] を返し、値スタックは変えない。
    pub fn place(&mut self, top: u64, words: usize) -> Result<u64, OutOfMemory> {
        if self.chunks.is_empty() {
            debug_assert_eq!(top, 0);
            if self.spare.is_none() && self.armed.is_none() {
                let sp = new_mem(CHUNK_WORDS);
                if crate::alloc::take_flag(crate::alloc::Role::Exec) {
                    self.armed = sp.ok();
                    return Err(OutOfMemory);
                }
                self.spare = Some(sp?);
            }
            let mem = self.take_mem(words)?;
            self.chunks.push(Chunk {
                mem: mem.into_vec(),
                base: 0,
            });
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
        self.drop_copies_before_full(i);
        if i < self.chunks.len() && self.chunks[i].mem.len() >= words {
            self.chunks[i].base = next_base;
            self.chunks.truncate(i + 1);
        } else {
            let mem = self.take_mem(words)?;
            self.chunks.truncate(i);
            self.chunks.push(Chunk {
                mem: mem.into_vec(),
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

    /// 1 フレームが半分の塊を占めるフレームを `n` 個積み、各フレームの位置を返す。
    fn descend(s: &mut ValueStack, n: usize) -> Vec<u64> {
        let fw = CHUNK_WORDS / 2 + 1;
        let mut top = 0;
        let mut ps = Vec::new();
        for _ in 0..n {
            let p = s.place(top, fw).unwrap();
            check_win(s);
            ps.push(p);
            top = p + fw as u64;
        }
        ps
    }

    #[test]
    fn 深く置いて戻った後の回収で塊を返す() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 20);
        assert_eq!(s.chunk_count(), 20);
        // 最下段のフレームに戻ってから回収する(インタプリタの戻り)。
        s.ptr_at(ps[0]);
        check_win(&s);
        s.release_after_collect(ps[0]);
        check_win(&s);
        assert!(s.chunk_count() <= 2);
        assert!(s.spare.is_some());
        // 残した次の塊は使い直す。
        assert_eq!(s.place(ps[0] + 10, CHUNK_WORDS), Ok(CHUNK_WORDS as u64));
        check_win(&s);
        assert_eq!(s.chunk_count(), 2);
    }

    #[test]
    fn 使っている塊が最上段より後ろに残っていても回収で塊を返す() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 20);
        // JIT のコードの戻りは使っている塊を動かさない。最上段は塊 4 のフレームとする。
        assert_eq!(s.cur, 19);
        s.release_after_collect(ps[4]);
        check_win(&s);
        assert_eq!(s.chunk_count(), 6);
        assert_eq!(s.cur, 5);
        // 最上段とその上の位置には書ける。
        // SAFETY: どちらも残した塊の中を指す。
        unsafe {
            *s.ptr_at(ps[4]) = 1;
            *s.ptr_at(ps[5]) = 2;
        }
        check_win(&s);
        // 最上段の上にフレームを置ける。
        let fw = CHUNK_WORDS / 2 + 1;
        assert_eq!(s.place(ps[4] + fw as u64, fw), Ok(ps[5]));
        check_win(&s);
        // 生きているフレームが無ければ何もしない。
        s.release_after_collect(NATIVE_CALLER);
        assert_eq!(s.chunk_count(), 6);
    }

    #[test]
    fn 継続の写しを塊としてつなぎ次の塊を後ろに残す() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 3);
        // 塊 0 のフレームの上に、塊 0 の残りより小さい写しをつなぐ。
        s.ptr_at(ps[0]);
        let fw = (CHUNK_WORDS / 2 + 1) as u64;
        let copy: Vec<u64> = (0..100).collect();
        let at = s.link_copy(ps[0] + fw, copy).unwrap();
        check_win(&s);
        assert_eq!(at, CHUNK_WORDS as u64);
        assert_eq!(s.cur, 1);
        // それまでの塊 1 は写しの塊の次に移り、塊 2 は捨てる。
        assert_eq!(s.chunk_count(), 3);
        assert_eq!(s.chunks[2].base, at + 100);
        // SAFETY: 写しの塊の中を指す。
        assert_eq!(unsafe { *s.ptr_at(at + 99) }, 99);
        // 写しの塊に入るフレームはそこに、入らないフレームは移した次の塊に置く。
        assert_eq!(s.place(at + 90, 10), Ok(at + 90));
        assert_eq!(s.place(at + 90, 11), Ok(at + 100));
        check_win(&s);
        assert_eq!(s.chunk_count(), 3);
        // 戻ると塊 0 の位置を読める。
        // SAFETY: 塊 0 の中を指す。
        unsafe { *s.ptr_at(ps[0]) = 5 };
        check_win(&s);
    }

    #[test]
    fn 注入を繰り返しても境目を越えるフレームは既定の大きさの塊を使い直す() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, CHUNK_WORDS - 5), Ok(0));
        let top = (CHUNK_WORDS - 5) as u64;
        assert_eq!(s.place(top, 10), Ok(CHUNK_WORDS as u64));
        let full = s.chunks[1].mem.as_ptr();
        for r in 0..100 {
            // 塊 0 のフレームの上に 100 語の写しをつなぎ、継続のフレームが戻った後に境目を越えるフレームを置く。
            s.ptr_at(0);
            let at = s.link_copy(top, (0..100).collect()).unwrap();
            check_win(&s);
            assert_eq!(at, CHUNK_WORDS as u64);
            assert_eq!(s.chunk_count(), 3);
            s.ptr_at(0);
            check_win(&s);
            if r % 3 == 2 {
                // 生きているフレームが塊 0 にあるときの回収は、写しの塊でなく既定の大きさの塊を残す。
                s.release_after_collect(0);
                check_win(&s);
                assert_eq!(s.chunk_count(), 2);
                assert_eq!(s.chunks[1].mem.as_ptr(), full);
                assert_eq!(s.chunks[1].base, CHUNK_WORDS as u64);
            }
            // 写しの塊に入るフレームも入らないフレームも、既定の大きさの塊に置く。
            let first = if r % 2 == 0 { 10 } else { 200 };
            assert_eq!(s.place(top, first), Ok(CHUNK_WORDS as u64));
            let b = s.place(CHUNK_WORDS as u64 + first as u64, 95).unwrap();
            assert_eq!(b, CHUNK_WORDS as u64 + first as u64);
            check_win(&s);
            assert_eq!(s.chunk_count(), 2);
            // 塊を取り直していない。
            assert_eq!(s.chunks[1].mem.as_ptr(), full);
        }
    }

    #[test]
    fn 次の塊が無い所に写しをつなぐと入らないフレームは予備の塊に置く() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 2);
        let fw = (CHUNK_WORDS / 2 + 1) as u64;
        let top = ps[1] + fw;
        // 塊を取れずに失敗して、予備の塊が次に使える塊になる(ふつうの resume の失敗)。
        assert_eq!(s.place(top, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        let sp = s.armed.as_ref().unwrap().as_ptr();
        assert_eq!(s.chunk_count(), 2);
        // 最上段を含む最後の塊に写しをつなぐと、写しの塊の後ろには塊が無い。
        let at = s.link_copy(top, (0..100).collect()).unwrap();
        check_win(&s);
        assert_eq!(at, 2 * CHUNK_WORDS as u64);
        assert_eq!(s.chunk_count(), 3);
        assert_eq!(s.cur, 2);
        // 写しの塊に入らないフレームは、新しく取らずに予備の塊に置く。
        assert_eq!(s.place(at + 90, 55), Ok(at + 100));
        check_win(&s);
        assert!(s.armed.is_none() && s.spare.is_none());
        assert_eq!(s.chunk_count(), 4);
        assert_eq!(s.chunks[3].mem.as_ptr(), sp);
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
