//! オブジェクトを移動しないマークスイープのヒープ。
//!
//! オブジェクトは 8 バイト境界に置いたワードの並びで、確保した後は移動しない。
//! GC は、マーク([`crate::gc`])と、このモジュールのスイープからなる。
//!
//! 小さなオブジェクト(ヘッダを含めて [`MAX_SMALL_WORDS`] ワード以下)は、サイズクラスごとのページ([`PAGE_WORDS`] ワード)から確保する。
//! ページは同じサイズのセルに分かれている。
//! セルは空きリストから配り、空きリストが空ならバンプ割り当てで配る。
//! バンプ割り当ては、ページのまだ配っていない部分を先頭から順に配る方法である。
//! 大きなオブジェクトは、1 個ずつグローバルアロケータで確保する。
//! ページは `mmap` で確保する。
//! 空になったページは、スイープのときにプールに残すか(最大 [`POOL_PAGES`] 枚)、OS に解放する。
//!
//! ヘッダの下位 8 ビットはオブジェクトの種類で、空きセルのヘッダは 0 である。
//! ヘッダの次の 8 ビットは GC が使い、マークのときに [`MARK`] を立て、スイープのときに下ろす。
//! スイープは、マークのない小さなオブジェクトのセルを空きリストに戻し、マークのない大きなオブジェクトをグローバルアロケータに解放して、空きリストを作り直す。
//! スイープは種類が 0 のセルを空きセルとして扱うので、確保したセルには、次の GC までに種類の入ったヘッダを書かなければならない。
//!
//! GC をいつ実行するかは、ヒープではなく [`crate::Rt`] が決める。
//! [`crate::Rt`] は [`Heap::should_collect`] で GC を始める条件を調べる。
//! 確保の関数 [`Heap::try_alloc`] は GC を実行しない。
//!
//! メモリを確保できないとき(`mmap` かグローバルアロケータが失敗したとき)は、プロセスを終了せずに [`OutOfMemory`] を返す。
//! 呼び出し元は GC を実行してから確保をやり直し、それでも確保できなければ [`Heap::exhausted`] を呼んで、メモリ不足の実行時エラー(IRSpec 付録 B の 19 番)にする。
//!
//! メモリ不足のエラーで巻き戻している間も `cancel` 節を実行できるように、最初の確保のときに予備のページ([`RESERVE_PAGES`] 枚、1 MiB)を確保しておく。
//! [`Heap::exhausted`] は予備のページを空きページのプールに移す。
//! その後は、新しいページをまずプールから取り出し、プールが空なら OS から確保しようとする。
//! OS からページを確保できたとき、または大きなオブジェクトを確保できたときは、予備のページを使っている状態を終える。
//! その次に新しいページか大きなオブジェクトを確保するときに、新しい予備のページを確保する。
//! [`RESERVE_SWITCH_WORDS`] ワード(2 MiB)より大きなオブジェクトの確保に失敗しても、ほかの確保に使えるメモリは減らないので、予備のページに切り替えない。
//! `cancel` 節の中で起きたメモリ不足のエラーは、ほかの脱出と同じく抑制される。
//!
//! 失敗を返す確保(管理用の `Vec` の `try_reserve` と大きなオブジェクト)は、[`crate::alloc::handled`] の範囲の中で行う。
//! これは、確保の失敗を呼び出し元が処理する範囲である。
//! この範囲の中で確保に失敗しても、グローバルアロケータは予備領域を使わない。
//! 失敗の後は、呼び出し元が代替の経路に切り替えるか、メモリ不足のエラーにする。
//!
//! JIT のコードは、小さなオブジェクトを、ランタイム関数を呼ばずにその場で確保する。
//! これを確保の高速経路(fast path)と呼ぶ(`keleut_interp::native` のモジュールの説明を参照)。
//! 手順は [`Heap::try_alloc`] の最初の 2 つの経路と同じで、空きリストの先頭のセルを外すか、バンプポインタを進める。
//! JIT のコードは、[`fast`] のオフセットを使って、ヒープのフィールド `since_gc`、`trigger` とサイズクラスごとの状態を直接読み書きする。
//! 高速経路は、`since_gc >= trigger` なら確保せずにランタイム関数を呼び、確保したらセルのワード数を `since_gc` に足す。
//! そのため、GC を始める条件は、ランタイム関数を通る確保と変わらない。
//! この手順とフィールドのレイアウトを変えるときは、`keleut_llvm` の確保の高速経路も合わせて変える。

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_void;

use crate::alloc::handled;

/// メモリを確保できなかったことを表すエラー。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfMemory;

/// ヘッダのマークビット。GC がマークのときに立てる。
pub const MARK: u64 = 1 << 8;

/// 1 ページのワード数(256 KiB)。
pub const PAGE_WORDS: usize = 1 << 15;

/// 小さなオブジェクトのワード数の上限(4 KiB)。ヘッダを含めて数える。
pub const MAX_SMALL_WORDS: usize = 512;

/// 予備のページの枚数。合計で 1 MiB になる。
pub const RESERVE_PAGES: usize = 4;

/// 予備のページの合計のワード数。
pub const RESERVE_WORDS: usize = RESERVE_PAGES * PAGE_WORDS;

/// このワード数(2 MiB)より大きなオブジェクトの確保に失敗しても、予備のページに切り替えない。
pub const RESERVE_SWITCH_WORDS: usize = 1 << 18;

/// スイープの後にプールに残す空きページの枚数の上限。合計で 4 MiB になる。
pub const POOL_PAGES: usize = 16;

/// GC の間隔の最小値(8 MiB)。間隔は、前の GC の後に確保したワード数で数える。
pub const MIN_TRIGGER_WORDS: u64 = 1 << 20;

/// サイズクラスごとの、セルのワード数。
const CLASSES: [u16; 31] = [
    2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160,
    192, 224, 256, 320, 384, 448, 512,
];

const NCLASS: usize = CLASSES.len();

/// ワード数からサイズクラスの番号を求める表。
///
/// `CLASS_OF[w]` は、`w` ワードが入る最小のサイズクラスの番号である。
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

/// OS からページを 1 枚確保する。確保できなければ `None` を返す。
fn sys_page() -> Option<*mut u64> {
    // SAFETY: 無名マッピングを新しく作るだけである。
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

/// ページを OS に解放する。
fn sys_unpage(p: *mut u64) {
    // SAFETY: p は sys_page が返したページで、もう使わない。
    unsafe { munmap(p.cast(), PAGE_WORDS * 8) };
}

/// 1 つのサイズクラスのセルに分けたページ。先頭のアドレスとサイズクラスの番号を持つ。
struct Page {
    base: *mut u64,
    class: u8,
}

/// 大きなオブジェクト 1 個の情報。先頭のアドレス、確保に使った `Layout`、ワード数を持つ。
struct Large {
    p: *mut u64,
    layout: Layout,
    words: u64,
}

/// サイズクラスごとの確保の状態。
#[derive(Clone, Copy)]
struct Class {
    /// 空きリストの先頭。空きセルの 1 番のワードが次の空きセルを指す。
    free: *mut u64,
    /// バンプポインタ。バンプ割り当て中のページで、次に配るセルを指す。
    cur: *mut u64,
    /// バンプ割り当て中のページで配れる範囲の終わり。
    end: *mut u64,
}

impl Class {
    /// `base` から始まるページが、このサイズクラスのバンプ割り当て中のページなら真を返す。
    ///
    /// `cur` ではなく `end` で判定する。
    /// セルのワード数が [`PAGE_WORDS`] を割り切るサイズクラスでは、バンプ割り当て中のページを使い切ると `cur` がページの終わりを指し、
    /// そのアドレスが隣のページの先頭と一致することがあるからである。
    /// `end` は、バンプ割り当て中のページの先頭より大きく、そのページの終わり以下なので、ほかのページの範囲には入らない。
    /// `end` が null のとき(まだページがないとき)は、どのページにも偽を返す。
    #[inline]
    fn bump_in(&self, base: *mut u64) -> bool {
        let e = self.end as usize;
        e > base as usize && e <= base as usize + PAGE_WORDS * 8
    }
}

/// スイープ 1 回の結果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepStats {
    /// GC の後に残ったオブジェクトのワード数。小さなオブジェクトはセルのサイズで数える。
    pub live_words: u64,
    /// 解放したオブジェクトの数。
    pub freed: u64,
    /// OS に解放したページの枚数。
    pub released_pages: u64,
    /// 調べたセルと大きなオブジェクトの数。スイープの処理量を表す。
    pub scanned: u64,
}

/// マークスイープのヒープ。
///
/// JIT のコードは、[`fast`] のオフセットを使ってフィールドを読み書きする。
/// オフセットは `offset_of!` でビルドごとに求めるので、構造体のレイアウトは Rust の既定のままでよい。
pub struct Heap {
    classes: [Class; NCLASS],
    pages: Vec<Page>,
    large: Vec<Large>,
    /// 空きページのプール。
    pool: Vec<*mut u64>,
    /// OS とグローバルアロケータから確保して使っているワード数。ページと大きなオブジェクトを数え、プールと予備のページは数えない。
    held_words: u64,
    /// 前の GC の後に確保したワード数。
    since_gc: u64,
    /// 次の GC までに確保してよいワード数。
    trigger: u64,
    /// 次のスイープが GC のしきい値 `trigger` を決めるときに、GC の後に残った量に足すワード数。[`Heap::set_root_words`] で設定する。
    root_words: u64,
    /// テスト用の、`held_words` の上限。これを超える確保は、OS が確保に失敗したときと同じように扱う。
    limit_words: Option<u64>,
    /// 予備のページ。
    reserve: Vec<*mut u64>,
    /// 予備のページをプールに移して使っている間は真。
    in_reserve: bool,
}

// SAFETY: Heap はページと大きなオブジェクトを所有し、それらはほかのスレッドから参照されない。
// ミューテータのスレッドは 1 つで、Heap はそのスレッドへ移動するだけである。
unsafe impl Send for Heap {}

/// JIT のコードの確保の高速経路が使う、[`Heap`] のフィールドのオフセットとサイズクラスの表。
pub mod fast {
    use super::{CLASS_OF, CLASSES, Class, Heap, MAX_SMALL_WORDS};
    use std::mem::{offset_of, size_of};

    /// `since_gc`(前の GC の後に確保したワード数)のオフセット。
    pub const SINCE_GC: u64 = offset_of!(Heap, since_gc) as u64;
    /// `trigger`(次の GC までに確保してよいワード数)のオフセット。
    pub const TRIGGER: u64 = offset_of!(Heap, trigger) as u64;
    /// サイズクラスごとの状態を並べた配列の、先頭のオフセット。
    pub const CLASSES_AT: u64 = offset_of!(Heap, classes) as u64;
    /// サイズクラス 1 つ分の状態のバイト数。
    pub const CLASS_BYTES: u64 = size_of::<Class>() as u64;
    /// サイズクラスの状態の中で、空きリストの先頭を持つフィールドのオフセット。空きセルの 1 番のワードが次の空きセルを指す。
    pub const FREE: u64 = offset_of!(Class, free) as u64;
    /// サイズクラスの状態の中で、バンプポインタを持つフィールドのオフセット。バンプポインタは、バンプ割り当て中のページで次に配るセルを指す。
    pub const CUR: u64 = offset_of!(Class, cur) as u64;
    /// サイズクラスの状態の中で、バンプ割り当て中のページの終わりを持つフィールドのオフセット。
    pub const END: u64 = offset_of!(Class, end) as u64;

    /// `words` ワード(ヘッダを含む)のオブジェクトの、サイズクラスの番号とセルのワード数を返す。
    ///
    /// `words` が 0 のときと、大きなオブジェクト([`MAX_SMALL_WORDS`] ワードを超える)のときは `None` を返す。
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
    /// 空のヒープを作る。
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

    /// ヒープのアドレスを返す。JIT のコードの確保の高速経路が、このアドレスと [`fast`] のオフセットでフィールドを読み書きする。
    ///
    /// 返したポインタは、`Heap` の値を移動しない間だけ使える。
    pub fn fast_ptr(&mut self) -> *mut Heap {
        self
    }

    /// 予備のページをプールに移して使っている間は真を返す(テスト用)。
    pub fn in_reserve(&self) -> bool {
        self.in_reserve
    }

    /// [`Heap::held_words`] の上限を設定する(テスト用)。
    pub fn set_limit_words(&mut self, limit: Option<u64>) {
        self.limit_words = limit;
    }

    /// OS とグローバルアロケータから確保して使っているワード数を返す。ページと大きなオブジェクトを数え、プールと予備のページは数えない。
    pub fn held_words(&self) -> u64 {
        self.held_words
    }

    /// 前の GC の後に確保したワード数を返す。
    pub fn since_gc(&self) -> u64 {
        self.since_gc
    }

    /// 次のスイープが GC のしきい値を決めるときに足すワード数 `words` を設定する。
    ///
    /// GC([`crate::gc`])は、ルートの走査の処理量に相当する値として、走査したフレームの数に比例するワード数をスイープの前に設定する。
    pub fn set_root_words(&mut self, words: u64) {
        self.root_words = words;
    }

    /// GC を始める条件を満たしていれば真を返す。
    ///
    /// 条件は、前の GC の後に確保した量がしきい値 `trigger` 以上であることである。
    /// しきい値は、前の GC の後に残った量とルートの走査の分([`Heap::set_root_words`])の和と、最小の間隔 [`MIN_TRIGGER_WORDS`] の大きいほうである。
    #[inline]
    pub fn should_collect(&self) -> bool {
        self.since_gc >= self.trigger
    }

    /// 次に GC を始める条件を満たす時点を、現在から間隔 1 つ分だけ後にずらす。間隔は前のスイープで決めた値である。
    ///
    /// GC([`crate::gc`])がマークを中断したときに呼ぶ。
    /// マークを中断するとスイープを実行しないので、そのままでは、前の GC の後に確保した量が GC を始める条件を満たしたまま残る。
    /// すると次の確保(`cancel` 節の中の確保)ですぐにまた GC を始める条件を満たすので、確保した量を 0 に戻す。
    pub fn defer_collect(&mut self) {
        self.since_gc = 0;
    }

    fn over_limit(&self, words: u64) -> bool {
        matches!(self.limit_words, Some(l) if self.held_words + words > l)
    }

    /// サイズクラス `class` の新しいページを用意する。プールにページがあれば取り出し、プールが空なら OS から確保する。
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

    /// 予備のページを持っていなければ確保する。予備のページをプールに移して使っている間は確保しない。
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

    /// GC を実行しても `words` ワードを確保できなかったときに呼ぶ。予備のページが残っていれば、プールに移す。
    ///
    /// [`RESERVE_SWITCH_WORDS`] より大きなオブジェクトの確保に失敗したときは、予備のページに切り替えない。
    /// テスト用の上限([`Heap::set_limit_words`])を設定していれば、移したページの分だけ上限を上げる。
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

    /// `words` ワード(ヘッダを含む)のオブジェクトの領域を確保する。`words` は 1 以上でなければならない。領域の内容は初期化しない。GC は実行しない。
    #[inline]
    pub fn try_alloc(&mut self, words: usize) -> Result<*mut u64, OutOfMemory> {
        debug_assert!(words >= 1);
        if words <= MAX_SMALL_WORDS {
            let c = CLASS_OF[words] as usize;
            let cw = CLASSES[c] as usize;
            let k = &mut self.classes[c];
            if !k.free.is_null() {
                let p = k.free;
                // SAFETY: 空きリストのセルの 1 番のワードは次の空きセルを指す(セルは 2 ワード以上ある)。
                k.free = unsafe { *p.add(1) } as *mut u64;
                self.since_gc += cw as u64;
                return Ok(p);
            }
            if (k.end as usize).wrapping_sub(k.cur as usize) >= cw * 8 && !k.cur.is_null() {
                let p = k.cur;
                // SAFETY: p から cw ワードはバンプ割り当て中のページの中にある。
                k.cur = unsafe { p.add(cw) };
                self.since_gc += cw as u64;
                return Ok(p);
            }
            self.ensure_reserve()?;
            let p = self.new_page(c)?;
            let k = &mut self.classes[c];
            let cells = PAGE_WORDS / cw;
            // SAFETY: ページは PAGE_WORDS ワードで、cells × cw ワードはその中にある。
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
        // 確保の失敗は呼び出し元が GC とメモリ不足のエラーで処理するので、`handled` の範囲の中で確保する(グローバルアロケータの予備領域を使わない)。
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

    /// スイープを実行する。マークのあるオブジェクトのマークを下ろし、マークのないオブジェクトを解放して、空きリストを作り直す。
    ///
    /// 最後に、前の GC の後に確保したワード数を 0 に戻し、次の GC のしきい値を決める。
    /// 呼ぶ前に、生きているすべてのオブジェクトに [`MARK`] を立てておく。
    pub fn sweep(&mut self) -> SweepStats {
        let mut st = SweepStats::default();
        for k in &mut self.classes {
            k.free = std::ptr::null_mut();
        }
        let mut keep = Vec::new();
        if handled(|| keep.try_reserve(self.pages.len())).is_err() {
            // 残すページを入れる `Vec` を確保できないときは、空のページを解放せずに、ページの一覧をそのまま使ってスイープする。
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
                    // このページのセルを空きリストから外す。直前の `sweep_page` が追加したので、空きリストの先頭にはこのページのセルだけが続いている。
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
                        // バンプ割り当て中のページが空になった。バンプポインタをページの先頭に戻す(次のスイープで調べるセルが減る)。
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
            // SAFETY: 大きなオブジェクトの先頭のワードはヘッダである。
            let h = unsafe { *l.p };
            if h & MARK != 0 {
                // SAFETY: 上と同じ。
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

    /// ページのセルをスイープし、空きセルを空きリストに追加する。生きているオブジェクトのワード数を返す。
    fn sweep_page(&mut self, base: *mut u64, c: usize, st: &mut SweepStats) -> u64 {
        let cw = CLASSES[c] as usize;
        let k = self.classes[c];
        let cells = PAGE_WORDS / cw;
        // バンプ割り当て中のページでは、まだ配っていないセルを調べない。
        let limit = if k.bump_in(base) {
            (k.cur as usize - base as usize) / (cw * 8)
        } else {
            cells
        };
        let mut live = 0;
        st.scanned += limit as u64;
        let mut free = self.classes[c].free;
        for j in (0..limit).rev() {
            // SAFETY: セル j はページの中にあり、配ったセルのヘッダは初期化してある。
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

    /// 空になったページのセルを、空きリストの先頭から外す。
    fn unlink_page(&mut self, base: *mut u64, c: usize) {
        let end = base as usize + PAGE_WORDS * 8;
        let k = &mut self.classes[c];
        while !k.free.is_null() && (k.free as usize) >= base as usize && (k.free as usize) < end {
            // SAFETY: 空きリストのセルの 1 番のワードは次の空きセルを指す。
            k.free = unsafe { *k.free.add(1) } as *mut u64;
        }
    }

    /// ヒープにあるすべてのオブジェクトの先頭のアドレスを、順に `f` に渡す。
    ///
    /// 対象は、配ったセルのうちヘッダが空きでないものと、すべての大きなオブジェクトである。
    /// GC([`crate::gc`])が、マークスタックがオーバーフローした後の走査と、マークを中断したときにマークを下ろす走査に使う。
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
                // SAFETY: セル j はページの中にある。
                let p = unsafe { pg.base.add(j * cw) };
                // SAFETY: 配ったセルのヘッダは初期化してある。
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
            // SAFETY: 各オブジェクトは同じ layout で alloc した領域である。
            unsafe { dealloc(l.p.cast(), l.layout) };
        }
    }
}

/// Rust で書いたランタイム関数が、確保をまたいで値を保持するためのルートスタック。
///
/// 値を積み、使い終えたら積む前の長さに戻す。
/// GC([`crate::gc`])は、このスタックのすべてのワードをルートとして走査する。
/// GC はオブジェクトを移動しないので、積んだワードを確保の後に読み直す必要はない。
/// ただし、値を確保の後に使うときは、呼び出し元が、その値にこのスタックかほかのルートから到達できるようにしておく。
#[derive(Default)]
pub struct Roots {
    stack: Vec<u64>,
}

impl Roots {
    /// 値を積み、積む前の長さを返す。
    ///
    /// 返した値は、積んだ値の添字として [`Roots::get`] に、元の長さとして [`Roots::truncate`] に渡せる。
    pub fn push(&mut self, w: u64) -> usize {
        self.stack.push(w);
        self.stack.len() - 1
    }

    /// 添字 `i` の値を読む。
    pub fn get(&self, i: usize) -> u64 {
        self.stack[i]
    }

    /// 長さを `len` に戻す。
    pub fn truncate(&mut self, len: usize) {
        self.stack.truncate(len);
    }

    /// 現在の長さを返す。
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// 何も積んでいなければ真を返す。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// 積んだワードをスライスで返す。
    pub fn as_slice(&self) -> &[u64] {
        &self.stack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(h: &mut Heap, words: usize) -> *mut u64 {
        let p = h.try_alloc(words).unwrap();
        // SAFETY: p は words ワードの新しい領域である。
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
        // SAFETY: a は生きているオブジェクトである。
        unsafe { *a |= MARK };
        let st = h.sweep();
        assert_eq!(st.freed, 2);
        assert_eq!(st.live_words, 4);
        // SAFETY: 上と同じ。
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
        // バンプ割り当て中の 2 枚目は残し、1 枚目をプールに戻す。
        assert_eq!(st.freed, n as u64);
        assert_eq!(h.held_words(), PAGE_WORDS as u64);
    }

    #[test]
    fn 確保の失敗の後は予備から切り出し予備も尽きたら失敗する() {
        let mut h = Heap::new();
        obj(&mut h, 8);
        h.set_limit_words(Some(h.held_words()));
        // 現在のページの残りは使える。
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
        // テスト用の上限を外して、メモリが空いた状況を作る。大きなオブジェクトはグローバルアロケータで確保できる。
        h.set_limit_words(None);
        obj(&mut h, 1000);
        assert!(!h.in_reserve());
        // 次に大きなオブジェクトを確保するときに予備のページを確保し直し、その後の失敗でまた予備のページに切り替わる。
        obj(&mut h, 1000);
        h.exhausted(8);
        assert!(h.in_reserve());
    }

    #[test]
    fn 使い切った切り出し中のページの隣のページも掃除する() {
        // セルのワード数が PAGE_WORDS を割り切るサイズクラスで、ページを 3 枚ちょうど使い切る。
        // バンプ割り当て中のページの cur はそのページの終わりを指し、mmap が隣に置いた前のページの先頭と一致することがある。
        let mut h = Heap::new();
        let n = 3 * PAGE_WORDS / 4;
        let mut objs = Vec::with_capacity(n);
        for _ in 0..n {
            let p = obj(&mut h, 4);
            // SAFETY: p は生きているオブジェクトである。
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
            // SAFETY: p は生きているオブジェクトの先頭である。
            if unsafe { *p } & MARK != 0 {
                marked += 1;
            }
        });
        assert_eq!(marked, 0);
        // 次の確保は生きているセルと重ならない。
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
