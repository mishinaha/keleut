//! ヒープ(計画 261024-1 の 6.5 節、D526)。
//!
//! 箱は 8 バイト境界の語の列で、動かない。
//! 回収は動かさないマークスイープで、印付け([`crate::gc`])とこのモジュールの掃除からなる。
//!
//! 小さな箱(見出しを含めて [`MAX_SMALL_WORDS`] 語以下)は、大きさの種類ごとのページ([`PAGE_WORDS`] 語)から切り出す。
//! ページは同じ大きさの区画の列で、区画は空きリストか、ページの末尾からの切り出しで配る。
//! 大きな箱は箱ごとにシステムから取る。
//! ページは `mmap` で取り、空になったページは掃除のときに手元に残すか(最大 [`POOL_PAGES`] 枚)システムへ返す。
//!
//! 見出しの下位 8 ビットは箱の種類で、空いた区画は見出しを 0 にする。
//! 見出しの次の 8 ビットは GC の印で、印付けが [`MARK`] を立て、掃除が消す。
//! 掃除は、印の無い箱を空きリストに戻し(大きな箱はシステムへ返し)、空きリストを作り直す。
//! したがって、確保した区画には、次の回収の前に種類の入った見出しを書かなければならない。
//!
//! 回収の時機はヒープではなく [`crate::Rt`] が決める([`Heap::should_collect`])。
//! 確保の関数 [`Heap::try_alloc`] は回収をしない。
//!
//! 領域を取れないとき(システムの確保の失敗)は、プロセスを止めずに [`OutOfMemory`] を返す。
//! 呼び出し側は回収してから試し直し、それでも取れなければ [`Heap::exhausted`] を呼んでメモリの不足(付録 B の 19 番)にする。
//!
//! メモリの不足の誤りを巻き戻す間に `cancel` 節が走れるよう、最初の確保のときに予備のページ([`RESERVE_PAGES`] 枚、1 MiB)を取っておく
//! (計画 6.5 節)。
//! [`Heap::exhausted`] は予備を空きページの手元に移す。
//! その後のページはまず手元から取り、手元が空ならシステムから取りにいく。
//! システムから取れたら予備の扱いをやめ、次の確保で新しい予備を取っておく。
//! [`RESERVE_SWITCH_WORDS`] 語(2 MiB)より大きな箱の失敗は、ほかの確保の余地を減らさないので、予備に切り替えない。
//! `cancel` 節の中のこの誤りは、ほかの脱出と同じく打ち切られる。
//!
//! 失敗を返す確保(表の `try_reserve` と大きな箱)は、大域の確保器の失敗を自分で扱う範囲で行う([`crate::alloc::handled`])。
//! 失敗しても大域の予備を使わず、呼び手の後退の道かメモリの不足に任せる。
//!
//! JIT のコードは、小さな箱を [`Heap::try_alloc`] の最初の 2 つの道(空きリストの先頭を外すか、切り出しの位置を進める)と同じ手順で、
//! 実行時の関数を呼ばずにその場で取る(確保の速い道、`keleut_interp::native` の冒頭)。
//! JIT のコードは [`fast`] の位置で、ヒープの欄 `since_gc`、`trigger` と大きさの種類の状態を直に読み書きする。
//! 速い道は、`since_gc >= trigger` なら取らずに実行時の関数に任せ、取ったら区画の語の数を `since_gc` に足す。
//! そのため、回収の時機は実行時の関数を通る確保と変わらない。
//! この手順と欄の配置を変えるときは、`keleut_llvm` の確保の速い道も合わせて変える。

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_void;

use crate::alloc::handled;

/// 確保の失敗。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfMemory;

/// 見出しの GC の印。
pub const MARK: u64 = 1 << 8;

/// ページの語の数(256 KiB)。
pub const PAGE_WORDS: usize = 1 << 15;

/// 小さな箱の語の数の上限(4 KiB)。
pub const MAX_SMALL_WORDS: usize = 512;

/// 予備のページの数(1 MiB)。
pub const RESERVE_PAGES: usize = 4;

/// 予備の語の数。
pub const RESERVE_WORDS: usize = RESERVE_PAGES * PAGE_WORDS;

/// この語の数より大きな箱の確保の失敗では、予備に切り替えない(2 MiB)。
pub const RESERVE_SWITCH_WORDS: usize = 1 << 18;

/// 掃除の後に手元に残す空きページの数の上限(4 MiB)。
pub const POOL_PAGES: usize = 16;

/// 回収の間隔の最小(前の回収の後に確保した語の数。8 MiB)。
pub const MIN_TRIGGER_WORDS: u64 = 1 << 20;

/// 大きさの種類(区画の語の数)。
const CLASSES: [u16; 31] = [
    2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160,
    192, 224, 256, 320, 384, 448, 512,
];

const NCLASS: usize = CLASSES.len();

/// 語の数から大きさの種類の番号を引く表。
const CLASS_OF: [u8; MAX_SMALL_WORDS + 1] = {
    let mut t = [0u8; MAX_SMALL_WORDS + 1];
    let mut w = 0;
    let mut c = 0;
    while w <= MAX_SMALL_WORDS {
        while (CLASSES[c] as usize) < w {
            c += 1;
        }
        t[w] = c as u8;
        w += 1;
    }
    t
};

unsafe extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64)
    -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
}

const PROT_READ_WRITE: i32 = 0x1 | 0x2;
const MAP_PRIVATE_ANONYMOUS: i32 = 0x02 | 0x20;

/// システムからページを 1 枚取る。
fn sys_page() -> Option<*mut u64> {
    // SAFETY: 無名の写像を新しく作るだけである。
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            PAGE_WORDS * 8,
            PROT_READ_WRITE,
            MAP_PRIVATE_ANONYMOUS,
            -1,
            0,
        )
    };
    if p as isize == -1 || p.is_null() {
        None
    } else {
        Some(p.cast())
    }
}

/// ページをシステムへ返す。
fn sys_unpage(p: *mut u64) {
    // SAFETY: p は sys_page が返したページで、もう使わない。
    unsafe { munmap(p.cast(), PAGE_WORDS * 8) };
}

/// 大きさの種類のページ。
struct Page {
    base: *mut u64,
    class: u8,
}

/// 大きな箱。
struct Large {
    p: *mut u64,
    layout: Layout,
    words: u64,
}

/// 大きさの種類ごとの状態。
#[derive(Clone, Copy)]
struct Class {
    /// 空きリストの先頭(区画の 1 番の語が次を指す)。
    free: *mut u64,
    /// 切り出し中のページの、次の区画と終わり。
    cur: *mut u64,
    end: *mut u64,
}

impl Class {
    /// `base` のページが切り出し中のページか。
    ///
    /// `cur` ではなく `end` で決める。
    /// 区画の語の数が [`PAGE_WORDS`] を割り切る種類では、切り出し中のページを使い切ると `cur` がページの終わりを指し、
    /// それは隣のページの先頭と同じ番地になりうるからである。
    /// `end` は切り出し中のページの先頭より大きく、終わり以下であり、ほかのページの範囲には入らない。
    /// `end` が空(まだページが無い)ならどのページにも当たらない。
    #[inline]
    fn bump_in(&self, base: *mut u64) -> bool {
        let e = self.end as usize;
        e > base as usize && e <= base as usize + PAGE_WORDS * 8
    }
}

/// 掃除の結果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepStats {
    /// 生き残った箱の語の数(区画の大きさで数える)。
    pub live_words: u64,
    /// 回収した箱の数。
    pub freed: u64,
    /// システムへ返したページの数。
    pub released_pages: u64,
    /// 見た区画と大きな箱の数(掃除の手間)。
    pub scanned: u64,
}

/// マークスイープのヒープ。
///
/// JIT のコードは欄を [`fast`] の位置で読み書きする。
/// 位置は `offset_of!` で構築ごとに決まるので、配置は Rust の既定のままでよい。
pub struct Heap {
    classes: [Class; NCLASS],
    pages: Vec<Page>,
    large: Vec<Large>,
    /// 空きページの手元。
    pool: Vec<*mut u64>,
    /// システムから取って持っている語の数(ページと大きな箱。手元と予備を除く)。
    held_words: u64,
    /// 前の回収の後に確保した語の数。
    since_gc: u64,
    /// 次の回収までに確保してよい語の数。
    trigger: u64,
    /// 次の掃除で、生き残った量に足して間隔を決める語の数([`Heap::set_root_words`])。
    root_words: u64,
    /// 試験のための、持つ語の数の上限。超えたらシステムの確保の失敗と同じに扱う。
    limit_words: Option<u64>,
    /// 予備のページ。
    reserve: Vec<*mut u64>,
    /// 予備を手元に移したか。
    in_reserve: bool,
}

// SAFETY: Heap はページと大きな箱を所有し、それらはほかのスレッドから参照されない。
// ミューテータは 1 本で、Heap はそのスレッドへ移すだけである。
unsafe impl Send for Heap {}

/// JIT のコードの確保の速い道が読み書きする、[`Heap`] の欄のバイトの位置と大きさの種類の表。
pub mod fast {
    use super::{CLASS_OF, CLASSES, Class, Heap, MAX_SMALL_WORDS};
    use std::mem::{offset_of, size_of};

    /// `since_gc`(前の回収の後に確保した語の数)。
    pub const SINCE_GC: u64 = offset_of!(Heap, since_gc) as u64;
    /// `trigger`(次の回収までに確保してよい語の数)。
    pub const TRIGGER: u64 = offset_of!(Heap, trigger) as u64;
    /// 大きさの種類ごとの状態の列の先頭。
    pub const CLASSES_AT: u64 = offset_of!(Heap, classes) as u64;
    /// 大きさの種類ごとの状態の大きさ。
    pub const CLASS_BYTES: u64 = size_of::<Class>() as u64;
    /// 大きさの種類の状態の中の、空きリストの先頭(区画の 1 番の語が次を指す)。
    pub const FREE: u64 = offset_of!(Class, free) as u64;
    /// 大きさの種類の状態の中の、切り出し中のページの次の区画。
    pub const CUR: u64 = offset_of!(Class, cur) as u64;
    /// 大きさの種類の状態の中の、切り出し中のページの終わり。
    pub const END: u64 = offset_of!(Class, end) as u64;

    /// `words` 語(見出しを含む。1 以上)の箱の大きさの種類の番号と、区画の語の数。
    ///
    /// 大きな箱([`MAX_SMALL_WORDS`] 語を超える)なら `None` である。
    pub fn class_of(words: usize) -> Option<(usize, usize)> {
        if words == 0 || words > MAX_SMALL_WORDS {
            return None;
        }
        let c = CLASS_OF[words] as usize;
        Some((c, CLASSES[c] as usize))
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    /// 空のヒープ。
    pub fn new() -> Heap {
        let empty = Class {
            free: std::ptr::null_mut(),
            cur: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
        };
        Heap {
            classes: [empty; NCLASS],
            pages: Vec::new(),
            large: Vec::new(),
            pool: Vec::new(),
            held_words: 0,
            since_gc: 0,
            trigger: MIN_TRIGGER_WORDS,
            root_words: 0,
            limit_words: None,
            reserve: Vec::new(),
            in_reserve: false,
        }
    }

    /// ヒープの番地(JIT のコードの確保の速い道が [`fast`] の位置で欄を読み書きする)。
    ///
    /// ヒープが動かない間だけ使える。
    pub fn fast_ptr(&mut self) -> *mut Heap {
        self
    }

    /// 予備を使い始めたか(試験用)。
    pub fn in_reserve(&self) -> bool {
        self.in_reserve
    }

    /// 持つ語の数の上限を決める(試験用)。
    pub fn set_limit_words(&mut self, limit: Option<u64>) {
        self.limit_words = limit;
    }

    /// システムから取って持っている語の数(ページと大きな箱)。
    pub fn held_words(&self) -> u64 {
        self.held_words
    }

    /// 前の回収の後に確保した語の数。
    pub fn since_gc(&self) -> u64 {
        self.since_gc
    }

    /// 次の掃除で決める回収の間隔に、根の走査の手間に見合う語の数 `words` を足す。
    ///
    /// GC([`crate::gc`])は、走査したフレームの数に比例する値を掃除の前に置く。
    pub fn set_root_words(&mut self, words: u64) {
        self.root_words = words;
    }

    /// 回収の時機か(前の回収の後に確保した量が、前の回収で生き残った量と根の走査の分の和と、最小の間隔の大きいほうを超えた)。
    #[inline]
    pub fn should_collect(&self) -> bool {
        self.since_gc >= self.trigger
    }

    /// 次の回収の時機を、今から 1 つの間隔(前の掃除で決めたもの)の後にずらす。
    ///
    /// GC([`crate::gc`])が印付けを打ち切ったときに呼ぶ。
    /// 掃除をしないので、前の回収の後に確保した量はそのままでは回収の時機を超えたままになり、
    /// 次の確保(`cancel` 節の確保)がすぐまた回収を起こすからである。
    pub fn defer_collect(&mut self) {
        self.since_gc = 0;
    }

    fn over_limit(&self, words: u64) -> bool {
        matches!(self.limit_words, Some(l) if self.held_words + words > l)
    }

    /// 新しいページ。手元から取り、無ければシステムから取る。
    fn new_page(&mut self, class: usize) -> Result<*mut u64, OutOfMemory> {
        handled(|| self.pages.try_reserve(1)).map_err(|_| OutOfMemory)?;
        let p = match self.pool.pop() {
            Some(p) => p,
            None => {
                if self.over_limit(PAGE_WORDS as u64) {
                    return Err(OutOfMemory);
                }
                let p = sys_page().ok_or(OutOfMemory)?;
                self.in_reserve = false;
                p
            }
        };
        self.held_words += PAGE_WORDS as u64;
        self.pages.push(Page {
            base: p,
            class: class as u8,
        });
        Ok(p)
    }

    /// 予備のページを取っておく。
    fn ensure_reserve(&mut self) -> Result<(), OutOfMemory> {
        if self.in_reserve || !self.reserve.is_empty() {
            return Ok(());
        }
        handled(|| self.reserve.try_reserve(RESERVE_PAGES)).map_err(|_| OutOfMemory)?;
        handled(|| self.pool.try_reserve(RESERVE_PAGES + POOL_PAGES)).map_err(|_| OutOfMemory)?;
        for _ in 0..RESERVE_PAGES {
            match sys_page() {
                Some(p) => self.reserve.push(p),
                None => {
                    for p in self.reserve.drain(..) {
                        sys_unpage(p);
                    }
                    return Err(OutOfMemory);
                }
            }
        }
        Ok(())
    }

    /// 確保に失敗した(回収しても取れなかった)。予備が残っていれば手元に移す。
    ///
    /// [`RESERVE_SWITCH_WORDS`] より大きな箱の失敗は、予備に切り替えない。
    pub fn exhausted(&mut self, words: usize) -> OutOfMemory {
        if words > RESERVE_SWITCH_WORDS || self.reserve.is_empty() {
            return OutOfMemory;
        }
        if handled(|| self.pool.try_reserve(self.reserve.len())).is_ok() {
            self.pool.append(&mut self.reserve);
            self.in_reserve = true;
            if let Some(l) = &mut self.limit_words {
                *l = l.saturating_add(RESERVE_WORDS as u64);
            }
        }
        OutOfMemory
    }

    /// `words` 語(見出しを含む。1 以上)の箱の領域を確保する。中身は初期化しない。回収はしない。
    #[inline]
    pub fn try_alloc(&mut self, words: usize) -> Result<*mut u64, OutOfMemory> {
        debug_assert!(words >= 1);
        if words <= MAX_SMALL_WORDS {
            let c = CLASS_OF[words] as usize;
            let cw = CLASSES[c] as usize;
            let k = &mut self.classes[c];
            if !k.free.is_null() {
                let p = k.free;
                // SAFETY: 空きリストの区画の 1 番の語は次の空きの区画を指す(区画は 2 語以上)。
                k.free = unsafe { *p.add(1) } as *mut u64;
                self.since_gc += cw as u64;
                return Ok(p);
            }
            if (k.end as usize).wrapping_sub(k.cur as usize) >= cw * 8 && !k.cur.is_null() {
                let p = k.cur;
                // SAFETY: p から cw 語は切り出し中のページの中にある。
                k.cur = unsafe { p.add(cw) };
                self.since_gc += cw as u64;
                return Ok(p);
            }
            self.ensure_reserve()?;
            let p = self.new_page(c)?;
            let k = &mut self.classes[c];
            let cells = PAGE_WORDS / cw;
            // SAFETY: ページは PAGE_WORDS 語で、cells × cw 語はその中にある。
            unsafe {
                k.cur = p.add(cw);
                k.end = p.add(cells * cw);
            }
            self.since_gc += cw as u64;
            return Ok(p);
        }
        self.ensure_reserve()?;
        if self.over_limit(words as u64) {
            return Err(OutOfMemory);
        }
        handled(|| self.large.try_reserve(1)).map_err(|_| OutOfMemory)?;
        let bytes = words.checked_mul(8).ok_or(OutOfMemory)?;
        let layout = Layout::from_size_align(bytes, 8).map_err(|_| OutOfMemory)?;
        // 失敗は呼び手が回収とメモリの不足で扱うので、失敗を自分で扱う範囲で取る(大域の確保器の予備に触れない)。
        // SAFETY: layout の大きさは 0 でない。
        let p = handled(|| unsafe { alloc(layout) }).cast::<u64>();
        if p.is_null() {
            return Err(OutOfMemory);
        }
        self.in_reserve = false;
        self.large.push(Large {
            p,
            layout,
            words: words as u64,
        });
        self.held_words += words as u64;
        self.since_gc += words as u64;
        Ok(p)
    }

    /// 掃除。印の付いた箱の印を消し、印の無い箱を空きにする。空きリストを作り直す。
    ///
    /// 呼ぶ前に、生きているすべての箱に [`MARK`] を立てておく。
    pub fn sweep(&mut self) -> SweepStats {
        let mut st = SweepStats::default();
        for k in &mut self.classes {
            k.free = std::ptr::null_mut();
        }
        let mut keep = Vec::new();
        if handled(|| keep.try_reserve(self.pages.len())).is_err() {
            // 作り直す列を取れないときは、ページを返さずにその場で掃除する。
            for i in 0..self.pages.len() {
                let (base, c) = (self.pages[i].base, self.pages[i].class as usize);
                let live = self.sweep_page(base, c, &mut st);
                st.live_words += live;
            }
        } else {
            let pages = std::mem::take(&mut self.pages);
            for pg in pages {
                let c = pg.class as usize;
                let live = self.sweep_page(pg.base, c, &mut st);
                let in_page = self.classes[c].bump_in(pg.base);
                if live == 0 && !in_page {
                    // 空きリストに入れた区画を外す(このページの区画だけが先頭から続く)。
                    self.unlink_page(pg.base, c);
                    self.held_words -= PAGE_WORDS as u64;
                    if self.pool.len() < POOL_PAGES && handled(|| self.pool.try_reserve(1)).is_ok()
                    {
                        self.pool.push(pg.base);
                    } else {
                        sys_unpage(pg.base);
                        st.released_pages += 1;
                    }
                } else {
                    if live == 0 {
                        // 切り出し中のページが空になった。先頭から切り出し直す(次の掃除で見る区画が減る)。
                        self.unlink_page(pg.base, c);
                        self.classes[c].cur = pg.base;
                    }
                    st.live_words += live;
                    keep.push(pg);
                }
            }
            self.pages = keep;
        }
        st.scanned += self.large.len() as u64;
        let mut i = 0;
        while i < self.large.len() {
            let l = &self.large[i];
            // SAFETY: 大きな箱の先頭は見出しである。
            let h = unsafe { *l.p };
            if h & MARK != 0 {
                // SAFETY: 同じ。
                unsafe { *l.p = h & !MARK };
                st.live_words += l.words;
                i += 1;
            } else {
                let l = self.large.swap_remove(i);
                // SAFETY: l.p は l.layout で alloc した領域である。
                unsafe { dealloc(l.p.cast(), l.layout) };
                self.held_words -= l.words;
                st.freed += 1;
            }
        }
        self.since_gc = 0;
        self.trigger = st
            .live_words
            .saturating_add(self.root_words)
            .max(MIN_TRIGGER_WORDS);
        st
    }

    /// ページの区画を掃除し、空きの区画を空きリストに足す。生きている語の数を返す。
    fn sweep_page(&mut self, base: *mut u64, c: usize, st: &mut SweepStats) -> u64 {
        let cw = CLASSES[c] as usize;
        let k = self.classes[c];
        let cells = PAGE_WORDS / cw;
        // 切り出し中のページでは、まだ配っていない区画を見ない。
        let limit = if k.bump_in(base) {
            (k.cur as usize - base as usize) / (cw * 8)
        } else {
            cells
        };
        let mut live = 0;
        st.scanned += limit as u64;
        let mut free = self.classes[c].free;
        for j in (0..limit).rev() {
            // SAFETY: 区画 j はページの中にあり、配った区画の見出しは初期化してある。
            unsafe {
                let p = base.add(j * cw);
                let h = *p;
                if h & 0xff != 0 {
                    if h & MARK != 0 {
                        *p = h & !MARK;
                        live += cw as u64;
                        continue;
                    }
                    st.freed += 1;
                }
                *p = 0;
                *p.add(1) = free as u64;
                free = p;
            }
        }
        self.classes[c].free = free;
        live
    }

    /// 空のページの区画を空きリストの先頭から外す。
    fn unlink_page(&mut self, base: *mut u64, c: usize) {
        let end = base as usize + PAGE_WORDS * 8;
        let k = &mut self.classes[c];
        while !k.free.is_null() && (k.free as usize) >= base as usize && (k.free as usize) < end {
            // SAFETY: 空きリストの区画の 1 番の語は次を指す。
            k.free = unsafe { *k.free.add(1) } as *mut u64;
        }
    }

    /// すべての生きている箱(配った区画と大きな箱で、見出しが空きでないもの)の先頭を順に渡す。
    ///
    /// 印付けの溢れ([`crate::gc`])の後の走査に使う。
    pub fn for_each_object(&self, mut f: impl FnMut(*mut u64)) {
        for pg in &self.pages {
            let c = pg.class as usize;
            let cw = CLASSES[c] as usize;
            let k = self.classes[c];
            let cells = PAGE_WORDS / cw;
            let limit = if k.bump_in(pg.base) {
                (k.cur as usize - pg.base as usize) / (cw * 8)
            } else {
                cells
            };
            for j in 0..limit {
                // SAFETY: 区画 j はページの中にある。
                let p = unsafe { pg.base.add(j * cw) };
                // SAFETY: 配った区画の見出しは初期化してある。
                if unsafe { *p } & 0xff != 0 {
                    f(p);
                }
            }
        }
        for l in &self.large {
            f(l.p);
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        for pg in &self.pages {
            sys_unpage(pg.base);
        }
        for &p in self.pool.iter().chain(&self.reserve) {
            sys_unpage(p);
        }
        for l in &self.large {
            // SAFETY: 各箱は同じ layout で alloc したものである。
            unsafe { dealloc(l.p.cast(), l.layout) };
        }
    }
}

/// Rust の実行時の関数が、確保をまたいで値を持つための根の枠(計画 6.5 節の `Root`)。
///
/// 値を積み、使い終えたら積む前の長さに戻す。
/// GC([`crate::gc`])は、この列のすべての語を根として走査する。
/// 動かさない GC なので、積んだ語を読み直す必要は無いが、積んだ値を確保の後に使うときは
/// その値がこの枠かほかの根から届くことを確かめる。
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

    /// 積んだ語の列。
    pub fn as_slice(&self) -> &[u64] {
        &self.stack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(h: &mut Heap, words: usize) -> *mut u64 {
        let p = h.try_alloc(words).unwrap();
        // SAFETY: p は words 語の新しい領域である。
        unsafe {
            *p = crate::value::header(crate::value::Kind::Array, (words - 1) as u32);
        }
        p
    }

    #[test]
    fn 境界にそろった領域を返し大きな箱にも領域を取る() {
        let mut h = Heap::new();
        let a = obj(&mut h, 3);
        let b = obj(&mut h, 3);
        assert_eq!(a as usize % 8, 0);
        assert_eq!(b as usize - a as usize, 24);
        let big = obj(&mut h, MAX_SMALL_WORDS + 1);
        assert_eq!(big as usize % 8, 0);
        assert_eq!(h.held_words(), (PAGE_WORDS + MAX_SMALL_WORDS + 1) as u64);
    }

    #[test]
    fn 印の無い箱を回収して同じ区画を配り直す() {
        let mut h = Heap::new();
        let a = obj(&mut h, 4);
        let b = obj(&mut h, 4);
        let big = obj(&mut h, 1000);
        // SAFETY: a は生きている箱である。
        unsafe { *a |= MARK };
        let st = h.sweep();
        assert_eq!(st.freed, 2);
        assert_eq!(st.live_words, 4);
        // SAFETY: 同じ。
        assert_eq!(unsafe { *a } & MARK, 0);
        assert_eq!(h.held_words(), PAGE_WORDS as u64);
        let c = obj(&mut h, 4);
        assert_eq!(c, b);
        let _ = big;
    }

    #[test]
    fn 空のページは手元に戻る() {
        let mut h = Heap::new();
        // 2 枚目のページを使わせる。
        let n = PAGE_WORDS / 8 + 1;
        for _ in 0..n {
            obj(&mut h, 8);
        }
        assert_eq!(h.held_words(), 2 * PAGE_WORDS as u64);
        let st = h.sweep();
        // 切り出し中の 2 枚目は残し、1 枚目を手元に戻す。
        assert_eq!(st.freed, n as u64);
        assert_eq!(h.held_words(), PAGE_WORDS as u64);
    }

    #[test]
    fn 確保の失敗の後は予備から切り出し予備も尽きたら失敗する() {
        let mut h = Heap::new();
        obj(&mut h, 8);
        h.set_limit_words(Some(h.held_words()));
        // 今のページの残りは使える。
        let per_page = PAGE_WORDS / 8;
        for _ in 1..per_page {
            obj(&mut h, 8);
        }
        assert_eq!(h.try_alloc(8), Err(OutOfMemory));
        assert!(!h.in_reserve());
        h.exhausted(8);
        assert!(h.in_reserve());
        for _ in 0..RESERVE_PAGES * per_page {
            obj(&mut h, 8);
        }
        assert_eq!(h.try_alloc(8), Err(OutOfMemory));
    }

    #[test]
    fn 大きな箱の失敗は予備に切り替えない() {
        let mut h = Heap::new();
        h.set_limit_words(Some(100));
        assert_eq!(h.try_alloc(RESERVE_SWITCH_WORDS + 1), Err(OutOfMemory));
        h.exhausted(RESERVE_SWITCH_WORDS + 1);
        assert!(!h.in_reserve());
        assert!(h.try_alloc(50).is_err());
    }

    #[test]
    fn 予備の後もシステムから取れれば予備の扱いをやめる() {
        let mut h = Heap::new();
        obj(&mut h, 8);
        h.set_limit_words(Some(h.held_words()));
        h.exhausted(8);
        assert!(h.in_reserve());
        // メモリが空いた(試験の上限を外す)。大きな箱はシステムから取る。
        h.set_limit_words(None);
        obj(&mut h, 1000);
        assert!(!h.in_reserve());
        // システムから取る次の確保で予備を取り直し、その後の失敗でまた予備に切り替わる。
        obj(&mut h, 1000);
        h.exhausted(8);
        assert!(h.in_reserve());
    }

    #[test]
    fn 使い切った切り出し中のページの隣のページも掃除する() {
        // 区画の語の数が PAGE_WORDS を割り切る種類で、ページを 3 枚ちょうど使い切る。
        // 切り出し中のページの cur はその終わりを指し、mmap が隣に置いた前のページの先頭と一致しうる。
        let mut h = Heap::new();
        let n = 3 * PAGE_WORDS / 4;
        let mut objs = Vec::with_capacity(n);
        for _ in 0..n {
            let p = obj(&mut h, 4);
            // SAFETY: p は生きている箱である。
            unsafe { *p |= MARK };
            objs.push(p);
        }
        assert_eq!(h.held_words(), 3 * PAGE_WORDS as u64);
        let mut seen = 0;
        h.for_each_object(|_| seen += 1);
        assert_eq!(seen, n);
        let st = h.sweep();
        assert_eq!(st.freed, 0);
        assert_eq!(st.live_words, 3 * PAGE_WORDS as u64);
        let mut marked = 0;
        h.for_each_object(|p| {
            // SAFETY: p は生きている箱の先頭である。
            if unsafe { *p } & MARK != 0 {
                marked += 1;
            }
        });
        assert_eq!(marked, 0);
        // 次の確保は生きている区画と重ならない。
        let set: std::collections::HashSet<usize> = objs.iter().map(|&p| p as usize).collect();
        for _ in 0..PAGE_WORDS / 4 {
            let p = obj(&mut h, 4);
            assert!(!set.contains(&(p as usize)));
        }
    }

    #[test]
    fn 大きさの種類の表() {
        for (w, &c) in CLASS_OF.iter().enumerate().skip(1) {
            let c = c as usize;
            assert!(CLASSES[c] as usize >= w.max(2));
            if c > 0 {
                assert!((CLASSES[c - 1] as usize) < w);
            }
        }
    }
}
