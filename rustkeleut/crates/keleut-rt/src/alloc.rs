//! グローバルアロケータのラッパと、その予備領域、スレッドの役割ごとのフラグ、テスト用の確保失敗を定義する。
//!
//! [`Wrap`] は `std::alloc::System` を包むグローバルアロケータである。
//! `keleut` の `main.rs` とテスト `tests/alloc_wrap.rs` が `#[global_allocator]` に指定する。
//! それ以外のプログラム(kconform や、ほかの crate のテスト)は [`Wrap`] を使わないので、そこではこのモジュールの関数は何もしない。
//!
//! ## 予備領域と役割
//!
//! [`init`] は、起動時に予備領域([`RESERVE_BYTES`] バイト)を `mmap` で確保しておく。
//! 各スレッドは役割([`Role`])をスレッドローカルの値として持ち、スレッドを作った直後に [`set_role`] で設定する。
//! 役割を設定しないスレッドは、変換スレッド([`Role::Conv`])として扱う。
//! KIR の読み込みと検証を行うスレッド(crate kir)と、LIR への変換(lowering)を行うスレッド(crate keleut-lir)は、この crate に依存しないので役割を設定せず、この既定によって変換スレッドになる。
//! メインスレッドの役割は、[`init`] が [`Role::Other`] に設定する。
//!
//! 確保に失敗したときに予備領域が残っていれば、ラッパは予備領域を OS に解放し、そのスレッドの役割のフラグを立ててから、確保をやり直す。
//! 失敗した確保に対して、ラッパは null を返すだけで、`handle_alloc_error` を呼ばない。
//! 確保の失敗を結果として返さない呼び出し元(`Vec` など)が null を受け取ると、標準ライブラリがプロセスを異常終了させる(SIGABRT、終了コード 134)。
//!
//! フラグは役割ごとに 1 つあり、プロセス全体で共有する。
//! フラグを調べる側は [`take_flag`] でアトミックに下ろすので、1 回のメモリ不足に 1 回だけ応じる。
//!
//! | 役割 | フラグを調べる箇所と、立っていたときの動作 |
//! | --- | --- |
//! | 実行スレッド | `Rt` のヒープ確保の入口と、その中で GC を実行した後。値スタックのチャンクを確保する経路。どちらもメモリ不足(`Stop::OutOfMemory`)として扱う。`keleut repl` は、入力ごとの実行の前後と束縛の表示の前後でも調べ、立っていればその入力のメモリ不足として報告するか、停止した入力とともに捨てる |
//! | 変換スレッド | 型検査の後、KIR の書き出しの後、KIR の読み込み、検証、LIR への変換のそれぞれの後([`conv_checkpoint`])。段階の名前を入れた `処理系の限度` の行を書き、入力の種類で決まる終了コード(下記)で終了する |
//! | コンパイルスレッド | 関数 1 つのコンパイルの後。そのコンパイル結果を捨て、以後のコンパイルをやめ、予備領域を確保し直す |
//! | その他(メインスレッド) | 調べない |
//!
//! コンパイルスレッドでは、予備領域がすでにないとき(実行スレッドが先に使ったとき)、予備領域を確保し直してからそれを解放し、フラグを立てて確保をやり直す。
//! やり直した後は、成功したかどうかによらず、すぐに予備領域を確保し直す。
//! コンパイルスレッドが予備領域を使ってから確保し直すまでの間は、実行スレッドの確保が予備領域なしで失敗することがある。
//! すぐに確保し直すのは、この期間を短くするためである。
//! 予備領域を確保し直せなければ(OS が 8 MiB のマッピングを作れないとき)、やり直さずに、ほかの役割と同じく null を返す。
//!
//! 変換スレッドでは、やり直しも失敗したか、予備領域がすでになければ、ラッパは `処理系の限度` の行([`LIMIT_LINE`])を `write(2)` で標準エラーに書き、`_exit` でプロセスを終了する。
//! 終了コードは、入力が `.kel` なら 1、`.kir` なら 65 である([`set_input`])。
//! 入力の種類を設定しないコマンド(`keleut repl`)では、ラッパはプロセスを終了せず、ほかの役割と同じく null を返す。
//!
//! ## 確保の失敗を呼び出し元が処理する範囲
//!
//! 失敗したときの処理を呼び出し元が持っている確保は、[`handled`] に渡すクロージャの中で行う。
//! `try_reserve` が失敗したら代替の経路に切り替える確保や、失敗をすぐメモリ不足として返す確保がこれに該当する。
//! 以下では、この範囲を自己処理の範囲と呼ぶ。
//! 自己処理の範囲の中で確保が失敗すると、ラッパは予備領域を使わず、フラグも立てずに、すぐ null を返す。
//!
//! ## テスト用の確保失敗
//!
//! 環境変数 `KELEUT_TEST_ALLOC_FAIL` には、`ROLE:SITE:MODE:SKIP` の形式の指定を `,` で区切って 8 個まで並べる。
//! 各指定は、役割 `ROLE`(`exec`、`conv`、`jit`)のスレッドが確保箇所 `SITE`([`Site`] の名前)で行う確保のうち、`SKIP + 1` 回目を失敗させる。
//! 確保箇所は、呼び出し元が [`site`] でスレッドローカルの値に設定し、返されたガードをドロップすると元に戻る。
//! 変換スレッドがスレッドローカルの確保箇所を持たないときは、メインスレッドが [`conv_site`] で設定した、プロセス全体で共有する確保箇所を使う。
//! 共有する確保箇所を使うのは、KIR を処理するスレッドを crate kir と crate keleut-lir が作り、呼び出し元がそのスレッドの確保箇所を設定できないからである。
//!
//! - `soft` は、`SKIP + 1` 回目の 1 回だけを失敗させる。
//!   この失敗は、OS が確保に失敗したときと同じ経路(予備領域を解放してやり直す経路)で処理する。
//! - `hard` は、その確保箇所での `SKIP + 1` 回目以降の確保をすべて失敗させ、予備領域を使わない。
//!   変換スレッドが自己処理の範囲の外で失敗したときは、入力の種類が設定されていれば、ラッパはすぐに [`conv_exit`] でプロセスを終了する。
//!
//! 指定ごとに、最初に確保を失敗させたときに 1 回だけ、標準エラーに `keleut: 試験: SITE の確保を失敗させました` の行を書く。

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, AtomicUsize, Ordering};

/// 予備領域のサイズ(8 MiB)。
pub const RESERVE_BYTES: usize = 8 << 20;

/// 変換スレッドが予備領域を使い切ったときに、標準エラーに書く行。
///
/// メモリを確保せずに書けるように、固定の文にしている。
/// どの段階の確保が失敗したかをラッパは知らないので、段階の名前の代わりに `型検査か実行の前の変換` と書く。
pub const LIMIT_LINE: &str = "keleut: 処理系の限度: 型検査か実行の前の変換でメモリが足りません\n";

/// [`conv_checkpoint`] が書く `処理系の限度` の行のうち、段階の名前より前の部分。
pub const STAGE_LIMIT_PREFIX: &str = "keleut: 処理系の限度: ";

/// [`conv_checkpoint`] が書く `処理系の限度` の行のうち、段階の名前より後の部分。
pub const STAGE_LIMIT_SUFFIX: &str = "でメモリが足りません\n";

/// 変換スレッドのフラグを調べる段階。`処理系の限度` の行には、段階の名前([`Stage::name`])を書く。
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
    /// LIR への変換(行に書く名前は `LIR への下ろし`)。
    Lower,
}

impl Stage {
    /// `処理系の限度` の行に書く、段階の名前を返す。
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

/// スレッドの役割。確保に失敗したときのラッパの動作と、立てるフラグを決める。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// メインスレッド(引数の解析、ファイルの読み込み、スレッドの起動と後始末)。
    Other = 0,
    /// 実行スレッドと、構造的な `Eq` の比較を行うために実行を切り替える先のスレッド。
    Exec = 1,
    /// フロントエンドのスレッドと、KIR を処理するスレッド(読み込み、検証、LIR への変換)。
    Conv = 2,
    /// LLVM バックエンドのコンパイルスレッド。
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

/// テスト用の確保失敗で対象にする確保箇所。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// GC の実行中に行う確保。
    Collect = 1,
    /// レコードのフィールドのコピー(`rec_extend`、`rec_restrict`、`rec_update`)。
    RecFields = 2,
    /// `call_clause` と `perform` の引数の配列。
    CallArgs = 3,
    /// 値スタックのチャンク(予備チャンクを含む)。
    Chunk = 4,
    /// `cont_resume` が継続を値スタックに配置し直すときのチャンク。
    Resume = 5,
    /// `cont_make` が作るコピー。
    Cont = 6,
    /// シェイプ表の作成と、シェイプの登録による拡張。
    Shape = 7,
    /// 型検査。構文解析と KIR の組み立てを含む、フロントエンドのスレッドの処理全体を指す。
    Check = 8,
    /// KIR の書き出し、読み込み、検証と、LIR への変換。
    Kir = 9,
    /// コンパイルスレッドが Rust の側で行う確保(LLVM IR を生成する前後)。
    Jit = 10,
    /// 退避した状態ワードの表を拡張する確保と、空きの項目を確保し直す確保(`Rt::pending_box`)。
    Pending = 11,
}

/// 確保箇所の名前(添字は [`Site`] の値)。
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

/// 入力の種類。変換スレッドが予備領域を使い切ったときに、終了するかどうかと終了コードを決める。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// 終了しない(`keleut repl` と、入力の種類を設定しないコマンド)。
    None = 0,
    /// Keleut のソース(終了コード 1)。
    Kel = 1,
    /// KIR(終了コード 65)。
    Kir = 2,
}

/// 予備領域へのポインタ。予備領域がなければ null である。
static RESERVE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// 役割ごとのフラグ(添字は [`Role`] の値)。
static FLAGS: [AtomicBool; 4] = [const { AtomicBool::new(false) }; 4];

/// 入力の種類([`Input`] の値)。
static INPUT: AtomicU8 = AtomicU8::new(Input::None as u8);

/// テスト用の確保失敗の指定が 1 つ以上あれば真である。
static INJECT_ON: AtomicBool = AtomicBool::new(false);

/// メインスレッドが設定した、変換スレッド用の確保箇所([`conv_site`])。
static CONV_SITE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static ROLE: Cell<u8> = const { Cell::new(Role::Conv as u8) };
    static SITE: Cell<u8> = const { Cell::new(0) };
    static HANDLED: Cell<u32> = const { Cell::new(0) };
}

/// テスト用の確保失敗の指定 1 つ。
///
/// `count` は役割と確保箇所が一致した確保の回数で、`told` は標準エラーに行を書いた後に真になる。
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

/// 指定の個数の上限。
const MAX_ENTRIES: usize = 8;

static ENTRIES: [Entry; MAX_ENTRIES] = [const { Entry::new() }; MAX_ENTRIES];
static N_ENTRIES: AtomicUsize = AtomicUsize::new(0);

/// テスト用に失敗させた確保の回数。
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

/// グローバルアロケータのラッパ。確保に失敗すると、予備領域を解放して確保をやり直す。
pub struct Wrap;

// SAFETY: 確保と解放は System にそのまま任せ、失敗したときだけ予備領域を解放してやり直す。
unsafe impl GlobalAlloc for Wrap {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: 呼び出し元が GlobalAlloc::alloc の事前条件を満たす。
        unsafe { self.with(|| System.alloc(l)) }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: 呼び出し元が GlobalAlloc::alloc_zeroed の事前条件を満たす。
        unsafe { self.with(|| System.alloc_zeroed(l)) }
    }

    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: 呼び出し元が GlobalAlloc::dealloc の事前条件を満たす。
        unsafe { System.dealloc(p, l) }
    }

    #[inline]
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        // SAFETY: 呼び出し元が GlobalAlloc::realloc の事前条件を満たす。失敗した realloc は元の領域を残すので、やり直してよい。
        unsafe { self.with(|| System.realloc(p, l, n)) }
    }
}

impl Wrap {
    /// 確保 `f` を行う。テスト用の確保失敗の指定に該当すれば確保せずに失敗させ、失敗は [`failed`] か [`hard_failure`] で処理する。
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

/// 確保が失敗したときの処理を行う(テスト用の `soft` の失敗を含む)。
///
/// 自己処理の範囲の中なら、すぐ null を返す。
/// それ以外は、予備領域を解放し、役割のフラグを立てて `retry` でやり直す。
/// やり直せないか、やり直しも失敗したときは、変換スレッドで入力の種類が設定されていれば [`conv_exit`] で終了し、それ以外は null を返す。
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

/// テスト用の `hard` の失敗の処理を行う。予備領域は使わない。
#[cold]
#[inline(never)]
fn hard_failure() -> *mut u8 {
    if !in_handled() && role() == Role::Conv && input() != Input::None {
        conv_exit();
    }
    std::ptr::null_mut()
}

/// 予備領域を OS に解放する。予備領域があったときに真を返す。
fn release_reserve() -> bool {
    let p = RESERVE.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if p.is_null() {
        return false;
    }
    // SAFETY: p は reacquire が作った RESERVE_BYTES バイトのマッピングで、swap で取り出したので、ほかのスレッドは持っていない。
    unsafe { munmap(p, RESERVE_BYTES) };
    true
}

/// 予備領域を確保する。すでにあれば何もしない。確保できたか、すでにあれば真を返す。
///
/// 起動時([`init`])のほかに、次のときに呼ぶ。
///
/// - コンパイルスレッドが予備領域を使った後(ラッパの中と、コンパイルスレッドの処理の中)
/// - コンパイルの前に予備領域がないとき
/// - `keleut repl` が次の入力を読む前
///
/// 確保できなければ偽を返し、プロセスは予備領域がないまま動作を続ける。
pub fn reacquire() -> bool {
    if !RESERVE.load(Ordering::Acquire).is_null() {
        return true;
    }
    // SAFETY: 無名マッピングを新しく作るだけである。
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
        // ほかのスレッドが先に予備領域を確保した。
        // SAFETY: p は上で作ったマッピングで、まだどこにも渡していない。
        unsafe { munmap(p, RESERVE_BYTES) };
    }
    true
}

/// テスト用の確保失敗の指定によって失敗させた確保の回数を返す。テストが使う。[`configure`] を呼ぶと 0 に戻る。
pub fn injected_count() -> u64 {
    INJECTED.load(Ordering::Relaxed)
}

/// 予備領域があれば真を返す。コンパイルスレッドがコンパイルの前に調べる。テストも使う。
pub fn has_reserve() -> bool {
    !RESERVE.load(Ordering::Acquire).is_null()
}

/// 起動時の処理を行う。現在のスレッドの役割を [`Role::Other`] にし、予備領域を確保し、`KELEUT_TEST_ALLOC_FAIL` を読む。
///
/// 仮想メモリを制限しているとき(`RLIMIT_AS`)は、先に [`limit_arenas`] で glibc のアリーナの個数を抑える。
/// 環境変数の値を読めなければ、エラーメッセージを返す。
/// 呼び出し元は、これを使い方のエラーとして扱う。
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

/// テスト用の確保失敗の指定を `spec`(`KELEUT_TEST_ALLOC_FAIL` の値の形式)に置き換える。`spec` が空なら指定を消す。
///
/// 回数のカウンタも 0 に戻す。
/// ほかのスレッドが確保している間は呼ばない。
/// 起動時とテストの中で呼ぶ。
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

/// 現在の確保をテスト用に失敗させるなら、指定が `hard` かどうかを返す。失敗させないなら `None` を返す。
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

/// メモリを確保せずに、標準エラーに `s` を書く。
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

/// 変換スレッドが予備領域を使い切ったときに、プロセスを終了する。
///
/// `処理系の限度` の行([`LIMIT_LINE`])を書き、入力が `.kel` なら終了コード 1、`.kir` なら 65 で終了する。
/// メモリを確保せず、`_exit` を使うので終了時の処理も実行しない。
pub fn conv_exit() -> ! {
    write_err(LIMIT_LINE);
    exit_conv()
}

/// 変換スレッドのフラグを調べて下ろす。フラグが立っていて、入力の種類が設定されていれば、`処理系の限度` の行を書いてプロセスを終了する。
///
/// 行は `keleut: 処理系の限度: STAGEでメモリが足りません` で、`STAGE` は `stage` の名前([`Stage::name`])である。
/// 終了コードは、入力が `.kel` なら 1、`.kir` なら 65 である。
/// 行はメモリを確保せずに書き、[`conv_exit`] と同じく終了時の処理を実行しない。
pub fn conv_checkpoint(stage: Stage) {
    if take_flag(Role::Conv) && input() != Input::None {
        write_err(STAGE_LIMIT_PREFIX);
        write_err(stage.name());
        write_err(STAGE_LIMIT_SUFFIX);
        exit_conv();
    }
}

/// 入力が `.kir` なら終了コード 65、それ以外は 1 でプロセスを終了する。メモリを確保せず、終了時の処理も実行しない。
fn exit_conv() -> ! {
    let code = if input() == Input::Kir { 65 } else { 1 };
    // SAFETY: _exit は libc の関数で、どのスレッドから呼んでもよい。
    unsafe { _exit(code) }
}

/// 仮想メモリを制限しているときに許す、glibc のアリーナ(malloc arena)の個数の上限。
pub const ARENA_MAX_UNDER_LIMIT: i32 = 2;

/// 仮想メモリを制限しているとき(`RLIMIT_AS`)に、glibc のアリーナの個数を [`ARENA_MAX_UNDER_LIMIT`] 以下に抑える。
///
/// glibc は、確保を行うスレッドごとにアリーナを作り、アリーナ 1 つにつき 64 MiB の仮想メモリを予約する。
/// アリーナの個数の上限は、既定ではコア数の 8 倍である。
/// 構造的な `Eq` の比較を行うスレッドやコンパイルスレッドがいくつも同時に存在すると、この予約が仮想メモリの上限の多くを占め、スタックとヒープに使える分が足りなくなる。
/// 仮想メモリを制限していなければ何もしない。
/// glibc がアリーナを作るより前、かつ、ほかのスレッドを作るより前に呼ぶ([`init`])。
pub fn limit_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn mallopt(param: i32, value: i32) -> i32;
        }
        /// glibc の `M_ARENA_MAX`。
        const M_ARENA_MAX: i32 = -8;
        if address_space_limit().is_some() {
            // SAFETY: mallopt は glibc の関数で、ほかのスレッドがまだないときに呼ぶ。失敗しても動作は変わらない。
            unsafe {
                mallopt(M_ARENA_MAX, ARENA_MAX_UNDER_LIMIT);
            }
        }
    }
}

/// 仮想メモリのソフトリミット(`RLIMIT_AS`)をバイト数で返す。制限がなければ `None` を返す。
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

/// 現在のスレッドの役割を設定する。
pub fn set_role(r: Role) {
    let _ = ROLE.try_with(|c| c.set(r as u8));
}

/// 現在のスレッドの役割を返す。設定していなければ [`Role::Conv`] を返す。
pub fn role() -> Role {
    Role::from_u8(ROLE.try_with(Cell::get).unwrap_or(Role::Conv as u8))
}

/// 入力の種類を設定する。
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

/// 役割 `r` のフラグが立っていれば、下ろして真を返す。
#[inline]
pub fn take_flag(r: Role) -> bool {
    let f = &FLAGS[r as usize];
    f.load(Ordering::Relaxed) && f.swap(false, Ordering::AcqRel)
}

/// 役割 `r` のフラグのアドレスを返す。フラグは 1 バイトで、0 でなければ立っている。
///
/// JIT コードは、オブジェクトを確保する高速経路(fast path)で、実行スレッドのフラグをこのアドレスから読む。
/// フラグが立っていれば、確保をランタイム関数に任せる。
/// フラグを下ろすのはランタイム関数([`take_flag`])だけである。
pub fn flag_addr(r: Role) -> usize {
    FLAGS[r as usize].as_ptr() as usize
}

/// 役割 `r` のフラグが立っていれば真を返す。フラグは下ろさない。
///
/// コンパイルスレッドが、コンパイルした関数をエントリテーブルに書く前に調べる。テストも使う。
pub fn flagged(r: Role) -> bool {
    FLAGS[r as usize].load(Ordering::Acquire)
}

fn in_handled() -> bool {
    HANDLED.try_with(Cell::get).unwrap_or(0) > 0
}

/// `f` を自己処理の範囲(確保の失敗を呼び出し元が処理する範囲)の中で実行する。
///
/// この範囲の中で確保が失敗すると、ラッパは予備領域を使わず、フラグも立てずに、すぐ null を返す。
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

/// 確保箇所を設定している範囲を表すガード。ドロップすると、設定する前の確保箇所に戻す。
#[must_use]
pub struct SiteGuard {
    prev: u8,
    global: bool,
}

/// 確保箇所を設定しなかったガードが持つ [`SiteGuard::prev`] の値。
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

/// 現在のスレッドの確保箇所を `s` に設定する。テスト用の確保失敗の指定がなければ何もしない。
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

/// 現在のスレッドが確保箇所を持たないときだけ、確保箇所を `s` に設定する。
///
/// 値スタックのチャンクの確保が使う。
/// `cont_resume` のチャンクのように、外側の呼び出し元がすでに確保箇所を設定しているときは、そちらを残す。
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

/// 変換スレッドがスレッドローカルの確保箇所を持たないときに使う、プロセス全体で共有する確保箇所を `s` に設定する。
///
/// メインスレッドが、KIR を処理するスレッドを作る呼び出しの前に設定し、呼び出しの後でガードをドロップして元に戻す。
/// テスト用の確保失敗の指定がなければ何もしない。
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
