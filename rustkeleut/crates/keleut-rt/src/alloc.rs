//! 大域の確保器の包み、予備、役割の印、試験用の失敗(計画 261104-3 の 4.2 節、D568)。
//!
//! [`Wrap`] は `std::alloc::System` を包む大域の確保器である。
//! `keleut` の `main.rs` と試験の対象 `tests/alloc_wrap.rs` が `#[global_allocator]` に宣言する。
//! ほかのプログラム(kconform、ほかの crate の試験)は包まないので、このモジュールの関数はそこでは何もしない。
//!
//! ## 予備と役割
//!
//! [`init`] が起動のときに予備([`RESERVE_BYTES`])を `mmap` で取っておく。
//! 各スレッドは役割([`Role`])をスレッド局所の値に持ち、スレッドを作った直後に [`set_role`] で書く。
//! 役割を書かないスレッドは変換の役割である。
//! KIR の読み込みと検証のスレッド(crate kir)と LIR への下ろしのスレッド(crate keleut-lir)は、この crate に依らないので役割を書かず、この既定で変換の役割になる。
//! 主のスレッドは [`init`] がそのほかの役割にする。
//!
//! 確保が失敗し予備があれば、包みは予備を返し、役割の印を立てて確保をやり直す。
//! 包みは失敗した確保に null を返すだけで、`handle_alloc_error` を呼ばない。
//! 失敗を返さない呼び手(`Vec` など)が null を受け取ったときは、標準ライブラリが止める(SIGABRT、134)。
//!
//! 印は役割ごとに 1 つで、プロセスに共有する。
//! 見る側は [`take_flag`] で不可分に下ろすので、1 回のメモリの不足に 1 回だけ応える。
//!
//! | 役割 | 印を見る場所 |
//! | --- | --- |
//! | 実行 | `Rt` のヒープの確保の入口とその中の回収の後、値スタックの塊を取る道(`Stop::OutOfMemory` にする) |
//! | 変換 | 型検査の後、KIR の書き出しの後、KIR の読み込みと検証と下ろしの後([`conv_checkpoint`]。段の名前を付けた限度の行を書いて、変換の役割の終え方と同じ終了状態で終える) |
//! | 翻訳 | 関数の翻訳の後(その翻訳を捨て、以後の翻訳をやめ、予備を取り直す) |
//! | そのほか | 見ない |
//!
//! 翻訳の役割では、予備が無ければ(実行の役割がすでに使った)、予備を取り直してからそれを返し、印を立ててやり直す。
//! やり直しの後には、通ったかどうかによらず、すぐ予備を取り直す。
//! 翻訳の役割が予備を使ってから取り直すまでの間に、実行の役割の確保が予備なしで失敗しないようにするためである。
//! 取り直せなければ(OS が 8 MiB の写像を返さない)、ほかの役割と同じく null を返す。
//!
//! 変換の役割では、やり直しも失敗するか予備が無ければ、包みは限度の行([`LIMIT_LINE`])を `write(2)` で書き、`_exit` で終える。
//! 終了状態は、入力が `.kel` なら 1、`.kir` なら 65 である([`set_input`])。
//! 入力の種類を書かないコマンド(`keleut repl`)では、この終え方をせず、ほかの役割と同じく null を返す。
//!
//! ## 失敗を自分で扱う範囲
//!
//! 失敗の後の道を持つ確保(`try_reserve` の後退の道や、失敗をすぐメモリの不足にする確保)は、[`handled`] の範囲の中で行う。
//! 範囲の中の失敗では、包みは予備に触れず印も立てずに、すぐ null を返す。
//!
//! ## 試験用の失敗
//!
//! 環境変数 `KELEUT_TEST_ALLOC_FAIL` は、`ROLE:SITE:MODE:SKIP` を `,` で区切って並べたものである。
//! 役割 `ROLE`(`exec`、`conv`、`jit`)のスレッドが場所 `SITE`([`Site`] の名前)で行う確保の、`SKIP + 1` 回目を失敗させる。
//! 場所は、呼び手が [`site`] で確保の前後にスレッド局所の値として書く。
//! 変換の役割のスレッドがスレッド局所の場所を持たなければ、主のスレッドが [`conv_site`] で書いたプロセスの場所を使う。
//! KIR の処理のスレッドは、そのスレッドを作る呼び手の側で場所を書けないからである。
//!
//! - `soft` は 1 回だけ失敗させる。失敗は OS の失敗と同じく予備の道を通る。
//! - `hard` は `SKIP + 1` 回目から後のその場所の確保をすべて失敗させ、予備に触れない。
//!   変換の役割で範囲の外なら、包みはすぐ変換の役割の終え方を通る。
//!
//! 失敗させた最初のときに、標準エラーに `keleut: 試験: SITE の確保を失敗させました` の 1 行を書く。
//! 指定ごとに 1 回だけ書く。

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, AtomicUsize, Ordering};

/// 予備の大きさ(8 MiB)。
pub const RESERVE_BYTES: usize = 8 << 20;

/// 変換の役割で予備を使い切ったときに標準エラーに書く行。
///
/// 確保をせずに書ける固定の文である。
/// どの段の確保が尽きたかを包みは知らないので、段の名前の代わりに「型検査か実行の前の変換」と書く。
pub const LIMIT_LINE: &str = "keleut: 処理系の限度: 型検査か実行の前の変換でメモリが足りません\n";

/// 変換の役割の印を段ごとに見たときの行の、段の名前の前の部分([`conv_checkpoint`])。
pub const STAGE_LIMIT_PREFIX: &str = "keleut: 処理系の限度: ";

/// 変換の役割の印を段ごとに見たときの行の、段の名前の後の部分([`conv_checkpoint`])。
pub const STAGE_LIMIT_SUFFIX: &str = "でメモリが足りません\n";

/// 変換の役割の印を見る段。行には段の名前([`Stage::name`])を書く。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 構文解析と型検査(フロントエンドのスレッド)。
    Check,
    /// KIR の書き出し。
    KirWrite,
    /// KIR の読み込み。
    KirRead,
    /// KIR の検証。
    KirVerify,
    /// LIR への下ろし。
    Lower,
}

impl Stage {
    /// 行に書く段の名前。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Check => "型検査",
            Stage::KirWrite => "KIR の書き出し",
            Stage::KirRead => "KIR の読み込み",
            Stage::KirVerify => "KIR の検証",
            Stage::Lower => "LIR への下ろし",
        }
    }
}

/// スレッドの役割。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 主のスレッド(引数の解析、ファイルの読み込み、スレッドの起動と後始末)。
    Other = 0,
    /// 実行のスレッドと、構造的な `Eq` が移るスレッド。
    Exec = 1,
    /// フロントエンドのスレッドと KIR の処理のスレッド(読み込み、検証、LIR への下ろし)。
    Conv = 2,
    /// LLVM の翻訳のスレッド。
    Jit = 3,
}

impl Role {
    fn from_u8(x: u8) -> Role {
        match x {
            0 => Role::Other,
            1 => Role::Exec,
            3 => Role::Jit,
            _ => Role::Conv,
        }
    }
}

/// 試験用の失敗で狙う確保の場所。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// 回収の中。
    Collect = 1,
    /// レコードのフィールドの写し(`rec_extend`、`rec_restrict`、`rec_update`)。
    RecFields = 2,
    /// `call_clause` と `perform` の引数の列。
    CallArgs = 3,
    /// 値スタックの塊(予備の塊を含む)。
    Chunk = 4,
    /// `cont_resume` が値スタックに継続を置き直すときの塊。
    Resume = 5,
    /// `cont_make` の写し。
    Cont = 6,
    /// 形の表の広げ直し。
    Shape = 7,
    /// 型検査(構文解析と KIR の組み立てを含む、フロントエンドのスレッドの全体)。
    Check = 8,
    /// KIR の書き出し、読み込み、検証、下ろし。
    Kir = 9,
    /// 翻訳のスレッドの Rust の側の確保(LLVM の IR を組む前後)。
    Jit = 10,
    /// 退避した状態語の表を広げる確保と、空きを取り直す確保(`Rt::pending_box`)。
    Pending = 11,
}

/// 場所の名前(添字は [`Site`] の値)。
const SITE_NAMES: [&str; 12] = [
    "",
    "collect",
    "rec_fields",
    "call_args",
    "chunk",
    "resume",
    "cont",
    "shape",
    "check",
    "kir",
    "jit",
    "pending",
];

/// 変換の役割の終え方を決める入力の種類。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// 終え方をしない(`keleut repl` と、種類を書かないコマンド)。
    None = 0,
    /// Keleut のソース(終了状態 1)。
    Kel = 1,
    /// KIR(終了状態 65)。
    Kir = 2,
}

/// 予備の領域。無ければ null である。
static RESERVE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// 役割ごとの印(添字は [`Role`] の値)。
static FLAGS: [AtomicBool; 4] = [const { AtomicBool::new(false) }; 4];

/// 入力の種類([`Input`] の値)。
static INPUT: AtomicU8 = AtomicU8::new(Input::None as u8);

/// 試験用の失敗の指定があるか。
static INJECT_ON: AtomicBool = AtomicBool::new(false);

/// 主のスレッドが書いた、変換の役割の場所。
static CONV_SITE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static ROLE: Cell<u8> = const { Cell::new(Role::Conv as u8) };
    static SITE: Cell<u8> = const { Cell::new(0) };
    static HANDLED: Cell<u32> = const { Cell::new(0) };
}

/// 試験用の失敗の指定の 1 つ。
struct Entry {
    role: AtomicU8,
    site: AtomicU8,
    hard: AtomicBool,
    skip: AtomicU64,
    count: AtomicU64,
    told: AtomicBool,
}

impl Entry {
    const fn new() -> Entry {
        Entry {
            role: AtomicU8::new(0),
            site: AtomicU8::new(0),
            hard: AtomicBool::new(false),
            skip: AtomicU64::new(0),
            count: AtomicU64::new(0),
            told: AtomicBool::new(false),
        }
    }
}

/// 指定の数の上限。
const MAX_ENTRIES: usize = 8;

static ENTRIES: [Entry; MAX_ENTRIES] = [const { Entry::new() }; MAX_ENTRIES];
static N_ENTRIES: AtomicUsize = AtomicUsize::new(0);

/// 試験用に失敗させた確保の数。
static INJECTED: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64)
    -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
    fn write(fd: i32, buf: *const c_void, n: usize) -> isize;
    fn _exit(status: i32) -> !;
}

const PROT_READ_WRITE: i32 = 0x1 | 0x2;
const MAP_PRIVATE_ANONYMOUS: i32 = 0x02 | 0x20;

/// 大域の確保器の包み。
pub struct Wrap;

// SAFETY: 確保と解放は System にそのまま任せ、失敗したときだけ予備を返してやり直す。
unsafe impl GlobalAlloc for Wrap {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: 呼び手が GlobalAlloc::alloc の約束を守る。
        unsafe { self.with(|| System.alloc(l)) }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: 同じ。
        unsafe { self.with(|| System.alloc_zeroed(l)) }
    }

    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: 同じ。
        unsafe { System.dealloc(p, l) }
    }

    #[inline]
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        // SAFETY: 同じ。失敗した realloc は元の領域を残すので、やり直してよい。
        unsafe { self.with(|| System.realloc(p, l, n)) }
    }
}

impl Wrap {
    /// 確保 `f` を、試験用の失敗と失敗の後の道を付けて行う。
    #[inline(always)]
    unsafe fn with(&self, f: impl Fn() -> *mut u8) -> *mut u8 {
        if INJECT_ON.load(Ordering::Relaxed)
            && let Some(hard) = injected()
        {
            return if hard { hard_failure() } else { failed(&f) };
        }
        let p = f();
        if !p.is_null() {
            return p;
        }
        failed(&f)
    }
}

/// 確保が失敗した(試験用の `soft` の失敗を含む)。
#[cold]
#[inline(never)]
fn failed(retry: &dyn Fn() -> *mut u8) -> *mut u8 {
    if in_handled() {
        return std::ptr::null_mut();
    }
    let role = role();
    let jit = role == Role::Jit;
    if release_reserve() || (jit && reacquire() && release_reserve()) {
        FLAGS[role as usize].store(true, Ordering::Release);
        let p = retry();
        if jit {
            reacquire();
        }
        if !p.is_null() {
            return p;
        }
    }
    if role == Role::Conv && input() != Input::None {
        conv_exit();
    }
    std::ptr::null_mut()
}

/// 試験用の `hard` の失敗。予備には触れない。
#[cold]
#[inline(never)]
fn hard_failure() -> *mut u8 {
    if !in_handled() && role() == Role::Conv && input() != Input::None {
        conv_exit();
    }
    std::ptr::null_mut()
}

/// 予備を返す。予備があれば真。
fn release_reserve() -> bool {
    let p = RESERVE.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if p.is_null() {
        return false;
    }
    // SAFETY: p は reacquire が取った RESERVE_BYTES の写像で、もう誰も持たない。
    unsafe { munmap(p, RESERVE_BYTES) };
    true
}

/// 予備を取る(すでにあれば何もしない)。取れたか、すでにあれば真。
///
/// 翻訳の役割が予備を使った後(包みの中と翻訳のスレッド)、翻訳の前に予備が無いときと、`keleut repl` の入力の前に呼ぶ。
/// 取れなければ予備なしで続ける。
pub fn reacquire() -> bool {
    if !RESERVE.load(Ordering::Acquire).is_null() {
        return true;
    }
    // SAFETY: 無名の写像を新しく作るだけである。
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            RESERVE_BYTES,
            PROT_READ_WRITE,
            MAP_PRIVATE_ANONYMOUS,
            -1,
            0,
        )
    };
    if p as isize == -1 || p.is_null() {
        return false;
    }
    if RESERVE
        .compare_exchange(std::ptr::null_mut(), p, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // ほかのスレッドが先に取った。
        // SAFETY: p は上で取った写像で、まだ誰も持たない。
        unsafe { munmap(p, RESERVE_BYTES) };
    }
    true
}

/// 試験用の失敗の指定で失敗させた確保の数(試験用。[`configure`] で 0 に戻る)。
pub fn injected_count() -> u64 {
    INJECTED.load(Ordering::Relaxed)
}

/// 予備を持っているか(試験用)。
pub fn has_reserve() -> bool {
    !RESERVE.load(Ordering::Acquire).is_null()
}

/// 起動の処理。今のスレッドをそのほかの役割にし、予備を取り、`KELEUT_TEST_ALLOC_FAIL` を読む。
///
/// 仮想メモリの上限があれば、先に [`limit_arenas`] で glibc の確保の領域の数を抑える。
/// 変数を読めなければ、誤りの文面を返す(呼び手が使い方の誤りにする)。
/// ほかのスレッドを作る前に呼ぶ。
pub fn init() -> Result<(), String> {
    limit_arenas();
    set_role(Role::Other);
    reacquire();
    match std::env::var("KELEUT_TEST_ALLOC_FAIL") {
        Ok(s) => configure(&s),
        Err(std::env::VarError::NotPresent) => Ok(()),
        Err(_) => Err("KELEUT_TEST_ALLOC_FAIL is not valid UTF-8".to_string()),
    }
}

/// 試験用の失敗の指定(`KELEUT_TEST_ALLOC_FAIL` の値)を置き換える。空なら指定を消す。
///
/// 回数の数えも 0 に戻す。
/// ほかのスレッドが確保している間に呼ばない(起動のときと、試験の中で呼ぶ)。
pub fn configure(spec: &str) -> Result<(), String> {
    INJECT_ON.store(false, Ordering::SeqCst);
    N_ENTRIES.store(0, Ordering::SeqCst);
    INJECTED.store(0, Ordering::SeqCst);
    let mut n = 0;
    for item in spec.split(',').filter(|s| !s.is_empty()) {
        let bad = || format!("KELEUT_TEST_ALLOC_FAIL: cannot read {item:?} (ROLE:SITE:MODE:SKIP)");
        let parts: Vec<&str> = item.split(':').collect();
        let [role, site, mode, skip] = parts[..] else {
            return Err(bad());
        };
        let role = match role {
            "exec" => Role::Exec,
            "conv" => Role::Conv,
            "jit" => Role::Jit,
            _ => return Err(bad()),
        };
        let Some(site) = SITE_NAMES.iter().skip(1).position(|s| *s == site) else {
            return Err(bad());
        };
        let hard = match mode {
            "soft" => false,
            "hard" => true,
            _ => return Err(bad()),
        };
        let skip: u64 = skip.parse().map_err(|_| bad())?;
        if n == MAX_ENTRIES {
            return Err(format!(
                "KELEUT_TEST_ALLOC_FAIL: at most {MAX_ENTRIES} entries"
            ));
        }
        let e = &ENTRIES[n];
        e.role.store(role as u8, Ordering::SeqCst);
        e.site.store(site as u8 + 1, Ordering::SeqCst);
        e.hard.store(hard, Ordering::SeqCst);
        e.skip.store(skip, Ordering::SeqCst);
        e.count.store(0, Ordering::SeqCst);
        e.told.store(false, Ordering::SeqCst);
        n += 1;
    }
    N_ENTRIES.store(n, Ordering::SeqCst);
    INJECT_ON.store(n > 0, Ordering::SeqCst);
    Ok(())
}

/// この確保を試験用に失敗させるなら、`hard` かどうかを返す。
#[cold]
#[inline(never)]
fn injected() -> Option<bool> {
    let role = role();
    let mut site = SITE.try_with(Cell::get).unwrap_or(0);
    if site == 0 && role == Role::Conv {
        site = CONV_SITE.load(Ordering::Relaxed);
    }
    if site == 0 {
        return None;
    }
    let n = N_ENTRIES.load(Ordering::Acquire);
    for e in &ENTRIES[..n] {
        if e.role.load(Ordering::Relaxed) != role as u8 || e.site.load(Ordering::Relaxed) != site {
            continue;
        }
        let k = e.count.fetch_add(1, Ordering::Relaxed);
        let skip = e.skip.load(Ordering::Relaxed);
        let hard = e.hard.load(Ordering::Relaxed);
        if (hard && k >= skip) || (!hard && k == skip) {
            INJECTED.fetch_add(1, Ordering::Relaxed);
            if !e.told.swap(true, Ordering::Relaxed) {
                write_err("keleut: 試験: ");
                write_err(SITE_NAMES[site as usize]);
                write_err(" の確保を失敗させました\n");
            }
            return Some(hard);
        }
    }
    None
}

/// 標準エラーに確保をせずに書く。
fn write_err(s: &str) {
    let mut b = s.as_bytes();
    while !b.is_empty() {
        // SAFETY: b は有効なバイト列である。
        let r = unsafe { write(2, b.as_ptr().cast(), b.len()) };
        if r <= 0 {
            return;
        }
        b = &b[r as usize..];
    }
}

/// 変換の役割の終え方。限度の行を書き、入力が `.kel` なら 1、`.kir` なら 65 でプロセスを終える。
///
/// 確保をせず、終了の処理も走らせない。
pub fn conv_exit() -> ! {
    write_err(LIMIT_LINE);
    exit_conv()
}

/// 変換の役割の印を見る。立っていれば(入力の種類があれば)、限度の行を書いて変換の役割の終え方で終える。
///
/// 行は `keleut: 処理系の限度: 段でメモリが足りません` で、段は `stage` の名前である。
/// 終了状態は、入力が `.kel` なら 1、`.kir` なら 65 である。
/// 行は確保をせずに書く。
pub fn conv_checkpoint(stage: Stage) {
    if take_flag(Role::Conv) && input() != Input::None {
        write_err(STAGE_LIMIT_PREFIX);
        write_err(stage.name());
        write_err(STAGE_LIMIT_SUFFIX);
        exit_conv();
    }
}

/// 入力が `.kel` なら 1、`.kir` なら 65 でプロセスを終える。確保をせず、終了の処理も走らせない。
fn exit_conv() -> ! {
    let code = if input() == Input::Kir { 65 } else { 1 };
    // SAFETY: _exit は libc の関数で、どのスレッドから呼んでもよい。
    unsafe { _exit(code) }
}

/// 仮想メモリの上限の下で、glibc の確保の領域(arena)の数の上限。
pub const ARENA_MAX_UNDER_LIMIT: i32 = 2;

/// 仮想メモリの上限(`RLIMIT_AS`)があれば、glibc の確保の領域の数を [`ARENA_MAX_UNDER_LIMIT`] までにする。
///
/// glibc は、確保するスレッドごとに領域を作り、1 つの領域につき 64 MiB の仮想メモリを予約する(上限は既定でコアの数の 8 倍)。
/// 構造的な `Eq` が移るスレッドや翻訳のスレッドが同時にいくつも生きると、この予約が上限の多くを占め、
/// スタックとヒープに残る分が足りなくなる。
/// 上限が無ければ何もしない。
/// 確保の領域を作る前、ほかのスレッドを作る前に呼ぶ([`init`])。
pub fn limit_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn mallopt(param: i32, value: i32) -> i32;
        }
        /// glibc の `M_ARENA_MAX`。
        const M_ARENA_MAX: i32 = -8;
        if address_space_limit().is_some() {
            // SAFETY: mallopt は glibc の関数で、ほかのスレッドがまだ無いときに呼ぶ。失敗しても振る舞いは変わらない。
            unsafe {
                mallopt(M_ARENA_MAX, ARENA_MAX_UNDER_LIMIT);
            }
        }
    }
}

/// 仮想メモリの軟らかい上限(`RLIMIT_AS`。バイト)を返す。上限が無ければ `None` である。
pub fn address_space_limit() -> Option<usize> {
    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, rlim: *mut RLimit) -> i32;
    }
    /// Linux の `RLIMIT_AS`。
    const RLIMIT_AS: i32 = 9;
    let mut r = RLimit { cur: 0, max: 0 };
    // SAFETY: r は呼び出しの間だけ使う有効な領域である。
    if unsafe { getrlimit(RLIMIT_AS, &mut r) } != 0 || r.cur == u64::MAX {
        return None;
    }
    usize::try_from(r.cur).ok()
}

/// 今のスレッドの役割を書く。
pub fn set_role(r: Role) {
    let _ = ROLE.try_with(|c| c.set(r as u8));
}

/// 今のスレッドの役割。
pub fn role() -> Role {
    Role::from_u8(ROLE.try_with(Cell::get).unwrap_or(Role::Conv as u8))
}

/// 入力の種類を書く。
pub fn set_input(i: Input) {
    INPUT.store(i as u8, Ordering::Relaxed);
}

fn input() -> Input {
    match INPUT.load(Ordering::Relaxed) {
        1 => Input::Kel,
        2 => Input::Kir,
        _ => Input::None,
    }
}

/// 役割 `r` の印が立っていれば下ろして真を返す。
#[inline]
pub fn take_flag(r: Role) -> bool {
    let f = &FLAGS[r as usize];
    f.load(Ordering::Relaxed) && f.swap(false, Ordering::AcqRel)
}

/// 役割 `r` の印が立っているか(下ろさない。試験用)。
pub fn flagged(r: Role) -> bool {
    FLAGS[r as usize].load(Ordering::Acquire)
}

fn in_handled() -> bool {
    HANDLED.try_with(Cell::get).unwrap_or(0) > 0
}

/// `f` を失敗を自分で扱う範囲で走らせる。
///
/// 範囲の中の確保の失敗では、包みは予備に触れず印も立てずに、すぐ null を返す。
/// 範囲は入れ子にできる。
#[inline]
pub fn handled<R>(f: impl FnOnce() -> R) -> R {
    struct Leave;
    impl Drop for Leave {
        fn drop(&mut self) {
            let _ = HANDLED.try_with(|c| c.set(c.get() - 1));
        }
    }
    let _ = HANDLED.try_with(|c| c.set(c.get() + 1));
    let _leave = Leave;
    f()
}

/// 場所の印の範囲。落としたときに前の場所に戻す。
#[must_use]
pub struct SiteGuard {
    prev: u8,
    global: bool,
}

/// 場所の印を書かなかった範囲の [`SiteGuard::prev`]。
const INACTIVE: u8 = u8::MAX;

impl Drop for SiteGuard {
    #[inline]
    fn drop(&mut self) {
        if self.prev == INACTIVE {
            return;
        }
        if self.global {
            CONV_SITE.store(self.prev, Ordering::Relaxed);
        } else {
            let _ = SITE.try_with(|c| c.set(self.prev));
        }
    }
}

/// 今のスレッドの確保の場所を `s` にする(試験用の失敗の指定が無ければ何もしない)。
#[inline]
pub fn site(s: Site) -> SiteGuard {
    if !INJECT_ON.load(Ordering::Relaxed) {
        return SiteGuard {
            prev: INACTIVE,
            global: false,
        };
    }
    let prev = SITE.try_with(|c| c.replace(s as u8)).unwrap_or(INACTIVE);
    SiteGuard {
        prev,
        global: false,
    }
}

/// 今のスレッドが場所を持たないときだけ、確保の場所を `s` にする。
///
/// 値スタックの塊が使う。
/// `cont_resume` の塊のように、外側の呼び手が場所を書いたときは、そちらを残す。
#[inline]
pub fn site_if_none(s: Site) -> SiteGuard {
    if !INJECT_ON.load(Ordering::Relaxed) || SITE.try_with(Cell::get).unwrap_or(1) != 0 {
        return SiteGuard {
            prev: INACTIVE,
            global: false,
        };
    }
    site(s)
}

/// 変換の役割のスレッドが場所を持たないときに使う、プロセスの場所を `s` にする。
///
/// 主のスレッドが、KIR の処理のスレッドを作る呼び出しの前後に書く。
#[inline]
pub fn conv_site(s: Site) -> SiteGuard {
    if !INJECT_ON.load(Ordering::Relaxed) {
        return SiteGuard {
            prev: INACTIVE,
            global: true,
        };
    }
    SiteGuard {
        prev: CONV_SITE.swap(s as u8, Ordering::Relaxed),
        global: true,
    }
}
