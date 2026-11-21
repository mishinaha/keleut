//! コンパイルスレッドの本体と、ネイティブコードのバックエンドを取り付けた実行。
//! 全体の説明は crate の冒頭にある。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use keleut_interp::native::{EnterFn, Native, PendingInit, TierMode, Translator};
use keleut_interp::{Interp, Setup};
use keleut_lir::{FuncId, Program};
use keleut_rt::Stop;

use crate::codegen;
use crate::sys::Jit;

/// ティアの設定(`--tier` の値)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// JIT コンパイルできる関数をすべて JIT のコードで実行する。
    /// 関数は、初めて呼ぶときにコンパイルする。
    JitOnly,
    /// どの関数もインタプリタで実行する。
    InterpOnly,
    /// インタプリタで `N` 回呼んだ関数を、コンパイルスレッドでコンパイルする。
    /// ミューテータは完了を待たない。
    Threshold(u64),
    /// JIT の関数に入るたびに、シードから決まる擬似乱数でティアを選ぶ。
    Alternating(u64),
    /// シードで決まる半分の関数だけを JIT のコードで実行する。
    Mixed(u64),
}

impl Tier {
    /// `--tier` の値(`interp-only`、`jit-only`、`threshold=N`、`alternating=SEED`、`mixed=SEED`)を読む。
    /// どれでもなければ `None` を返す。
    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "interp-only" => return Some(Tier::InterpOnly),
            "jit-only" => return Some(Tier::JitOnly),
            _ => {}
        }
        let (k, v) = s.split_once('=')?;
        let n = v.parse::<u64>().ok()?;
        match k {
            "threshold" => Some(Tier::Threshold(n)),
            "alternating" => Some(Tier::Alternating(n)),
            "mixed" => Some(Tier::Mixed(n)),
            _ => None,
        }
    }

    fn mode(self) -> Option<TierMode> {
        match self {
            Tier::InterpOnly => None,
            Tier::JitOnly => Some(TierMode::JitOnly),
            Tier::Threshold(n) => Some(TierMode::Threshold(n)),
            Tier::Alternating(s) => Some(TierMode::Alternating(s)),
            Tier::Mixed(s) => Some(TierMode::Mixed(s)),
        }
    }
}

/// ティアの既定値(`threshold=1000`)。
/// `keleut run` は、`--tier` を指定しないときにこれを使う。
pub const DEFAULT_TIER: Tier = Tier::Threshold(1000);

/// ネイティブコードのバックエンドを取り付けた実行のオプション。
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// インタプリタのオプション(深さの上限、ネイティブスタックのサイズ、`--gc-stress` など)。
    pub base: keleut_interp::RunOptions,
    /// ティアの設定。
    pub tier: Tier,
    /// LLVM の最適化パイプライン(既定は `default<O2>`)。
    pub passes: String,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            base: keleut_interp::RunOptions::default(),
            tier: Tier::JitOnly,
            passes: "default<O2>".to_string(),
        }
    }
}

/// コンパイルスレッドが起動処理の結果として返す、スタブ `interp_stub` と `enter_tailcc` のアドレス。
struct Init {
    stub: usize,
    enter: usize,
}

/// コンパイルスレッドのスタックサイズ。
///
/// 予約したサイズは仮想メモリの上限(`ulimit -v`)に数えられるので、小さく保つ。
/// コンパイルは関数ごとの小さなモジュールで行い、LLVM の再帰の深さは関数の大きさで抑えられるので、16 MiB で足りる。
/// 256 MiB にすると、仮想メモリを 512 MiB に制限する `conformance/gc/gc.t` で、JIT を使う実行だけがメモリ不足(終了コード 3)か、スレッドを作れない内部エラー(終了コード 70)で終了する。
const JIT_STACK: usize = 16 << 20;

/// 完了を待たない依頼のコンパイルが失敗したことを表すフラグと、そのエラーメッセージ。
///
/// コンパイルスレッドの起動処理が失敗したときも、ここに書く。
type Failure = Arc<(AtomicBool, Mutex<Option<String>>)>;

/// コンパイルスレッドの本体。
/// 起動処理の結果を `init` に送り、`req` から関数の番号を受け取るたびに、その関数をコンパイルする。
///
/// `fast_alloc` が真なら、確保の高速経路を生成する(`--gc-stress` では偽。`codegen::function`)。
///
/// プログラムの実行が終わると `stop` が真になり、このスレッドは残りの依頼を捨てて終了する。
/// 依頼を受け取ってからコンパイルを終えるまで、`busy` を真にする。
/// `busy` を真にしてから `stop` を読むので(どちらも SeqCst)、実行が終わった時点で `busy` が偽なら、このスレッドは新しいコンパイルを始めない。
/// LLVM の最適化に入る前にも `stop` を読み直し、真ならコンパイルを捨てる。
///
/// コンパイルを終えたら、エントリテーブルの項目を書き換える(Release)。
/// 完了を待つ依頼(`req` から受け取るタプルの 2 番目の要素が真)には、結果を `rep` に送る。
/// 完了を待たない依頼が失敗したときは `failure` に書く。
/// ミューテータは、依頼した関数のエントリを次に調べるときに `failure` を読み、失敗が書かれていれば内部エラーとして実行を終了する。
#[allow(clippy::too_many_arguments)]
fn jit_thread(
    prog: &Program,
    entry: Arc<[AtomicUsize]>,
    passes: &str,
    fast_alloc: bool,
    init: mpsc::Sender<Result<Init, String>>,
    req: mpsc::Receiver<(FuncId, bool)>,
    rep: mpsc::Sender<Result<usize, String>>,
    failure: Failure,
    stop: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
) {
    keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Jit);
    type Started = (Jit, Init, Box<[AtomicUsize]>, usize, String, String);
    let start = || -> Result<Started, String> {
        let jit = Jit::new()?;
        jit.define_absolute(&keleut_interp::native::helper_symbols())?;
        let n = prog.funcs.len();
        let table = entry.as_ptr() as usize;
        let (triple, layout) = (jit.triple(), jit.data_layout());
        let stubs = format!(
            "target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{}",
            codegen::stubs(table)
        );
        jit.add_ir("keleut-stubs", &stubs, passes)?;
        let stub = jit.lookup("interp_stub")?;
        let stub_r = jit.lookup("interp_stub_r")?;
        let rentry: Box<[AtomicUsize]> = (0..n).map(|_| AtomicUsize::new(stub_r)).collect();
        let enter = jit.lookup("enter_tailcc")?;
        for e in entry.iter() {
            e.store(stub, Ordering::Release);
        }
        Ok((jit, Init { stub, enter }, rentry, table, triple, layout))
    };
    let (jit, entry, rentry, table, triple, layout) = match start() {
        Ok((jit, i, rentry, table, triple, layout)) => {
            if init.send(Ok(i)).is_err() {
                return;
            }
            (jit, entry, rentry, table, triple, layout)
        }
        Err(e) => {
            // 起動処理を待たずに実行を始めたミューテータ(`threshold=N`)は、依頼した関数を調べるときにこのフラグを読む。
            if let Ok(mut m) = failure.1.lock() {
                m.get_or_insert_with(|| format!("LLVM の JIT を始められません: {e}"));
            }
            failure.0.store(true, Ordering::Release);
            let _ = init.send(Err(e));
            return;
        }
    };
    // コンパイルスレッドでメモリの確保が失敗すると、グローバルアロケータは予備領域を解放して、コンパイルスレッドのフラグを立てる(`keleut_rt::alloc`)。
    // このフラグが立ったら、以後のコンパイルをやめる。
    let mut gave_up = false;
    // 仮想メモリの上限は起動時に 1 回だけ読む。
    // 上限がなければ、コンパイルの前に残りを調べない。
    let as_limit = address_space_limit();
    while let Ok((f, wait)) = req.recv() {
        busy.store(true, Ordering::SeqCst);
        if stop.load(Ordering::SeqCst) {
            // 実行が終わった。
            // 残っている依頼(完了を待たない依頼)はコンパイルしない。
            break;
        }
        if gave_up {
            busy.store(false, Ordering::SeqCst);
            if wait && rep.send(Ok(0)).is_err() {
                break;
            }
            continue;
        }
        if !keleut_rt::alloc::has_reserve() && !keleut_rt::alloc::reacquire() {
            // グローバルアロケータの予備領域がなく(実行スレッドが使った)、確保し直すこともできない。
            // 予備領域なしでコンパイルすると、Rust のメモリ確保が失敗したときにプロセスが異常終了する(終了コード 134)。
            // そこで、コンパイルスレッドのフラグが立ったときと同じように、以後のコンパイルをやめてインタプリタで実行を続ける。
            gave_up = true;
            busy.store(false, Ordering::SeqCst);
            if wait && rep.send(Ok(0)).is_err() {
                break;
            }
            continue;
        }
        if let Some(limit) = as_limit
            && !room_for_translation(limit, virtual_size())
        {
            // 仮想メモリの残りが少ない。
            // LLVM の中でのメモリ確保の失敗は捕捉できないので、この関数はコンパイルせずにインタプリタで実行を続ける。
            // 次の依頼でもう一度調べる。
            busy.store(false, Ordering::SeqCst);
            if wait && rep.send(Ok(0)).is_err() {
                break;
            }
            continue;
        }
        let rtable = rentry.as_ptr() as usize;
        let jit_site = keleut_rt::alloc::site(keleut_rt::alloc::Site::Jit);
        let src = codegen::module(prog, f, table, rtable, fast_alloc, &triple, &layout);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let r = src.and_then(|src| {
            let name = codegen::func_name(f);
            jit.add_ir(&name, &src, passes)?;
            let ra = jit.lookup(&format!("{name}.r"))?;
            if keleut_rt::alloc::flagged(keleut_rt::alloc::Role::Jit) {
                // エントリを書く前に、コンパイルスレッドのフラグを調べる。
                // 立っていればコンパイルを捨てる(LLJIT に追加したコードは使わない)。
                return Ok(0);
            }
            // レジスタ渡しのエントリを先に書く。
            // エントリテーブルの項目を読んでコンパイル済みと判断した呼び出しが、どちらのエントリも使えるようにするためである。
            rentry[f.index()].store(ra, Ordering::Release);
            jit.lookup(&name)
        });
        drop(jit_site);
        let r = if keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Jit) {
            // コンパイル中のメモリ確保の失敗を、プログラムのメモリ不足と取り違えないようにする。
            // この関数のコンパイルを捨て、以後のコンパイルをやめてインタプリタで実行を続け、グローバルアロケータの予備領域を確保し直す。
            // 確保できなければ、予備領域なしで続ける。
            gave_up = true;
            keleut_rt::alloc::reacquire();
            Ok(0)
        } else {
            r
        };
        busy.store(false, Ordering::SeqCst);
        match &r {
            Ok(0) => {}
            Ok(addr) => entry[f.index()].store(*addr, Ordering::Release),
            Err(e) if !wait => {
                if let Ok(mut m) = failure.1.lock() {
                    m.get_or_insert_with(|| format!("{}: {e}", prog.funcs[f.index()].sym));
                }
                failure.0.store(true, Ordering::Release);
            }
            Err(_) => {}
        }
        if wait && rep.send(r).is_err() {
            break;
        }
    }
}

/// ネイティブコードのバックエンドを取り付けてプログラムを実行し、終了コードを返す。
///
/// `threshold=N` では、実行が終わると、コンパイルスレッドを待たずに、その終了コードで必ずプロセスを終了する。
/// このとき、この関数は戻らない。
/// コンパイルスレッドが、コンパイルの途中か、LLJIT とスタブを作る起動処理の途中であることがあるからである。
/// ほかのティアの設定では、実行が終わった時点でコンパイルスレッドがコンパイルの途中であるときだけ、同じように待たずに終了する。
/// 完了を待たない依頼(`threshold=N`)のコンパイルは、大きな関数では数分かかることがある。
/// 結果に影響しないコンパイルのために終了を遅らせないよう、待たずに終了する。
/// プロセスを終了するときは、標準出力と標準エラーを flush してから `_exit` を呼ぶ。
/// `exit` を使わないのは、コンパイルスレッドが LLVM の中を実行している間に、LLVM の静的オブジェクトのデストラクタを実行しないためである。
pub fn run_program(prog: &Program, opts: &RunOptions) -> u8 {
    run_with(prog, opts, true, |prog, opts, setup| {
        keleut_interp::run_program_with(prog, opts, setup)
    })
}

/// [`run_program`] と同じように実行するが、プロセスを終了しない。
/// テスト用である。
///
/// 実行の後、コンパイルスレッドが終了するのを待ってから終了コードを返す。
/// ミューテータのスレッドは、[`run_program`] と同じく `native_stack` のサイズで作る。
pub fn run_program_wait(prog: &Program, opts: &RunOptions) -> u8 {
    run_with(prog, opts, false, |prog, opts, setup| {
        keleut_interp::run_program_with(prog, opts, setup)
    })
}

/// [`run_program`] と同じように実行するが、ミューテータ用のスレッドを作らず、プログラムを現在のスレッドで実行する。
/// テスト用である。
///
/// `margin` はスタック残量の検査の余裕(バイト)である。
/// プロセスを終了せず、コンパイルスレッドが終了するのを待ってから戻る。
pub fn run_program_here(prog: &Program, opts: &RunOptions, margin: usize) -> u8 {
    run_with(prog, opts, false, |prog, opts, setup| {
        keleut_interp::run_program_here(prog, opts, margin, Some(setup))
    })
}

unsafe extern "C" {
    fn _exit(status: i32) -> !;
}

/// コンパイルスレッドの起動処理の結果を受け取るまで、`enter_tailcc` の代わりに置く関数。
///
/// 起動処理の結果を受け取るまではコンパイル済みの関数がないので、呼ばれることはない。
/// 呼ばれたら abort する。
unsafe extern "C" fn no_enter(_: *mut keleut_interp::native::NativeCtx, _: usize) -> u64 {
    std::process::abort()
}

/// 標準出力と標準エラーを flush し、終了処理を実行せずにプロセスを終了する。
fn exit_now(code: u8) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: _exit は libc の関数で、引数は終了コードである。
    // どのスレッドから呼んでもよい。
    unsafe { _exit(i32::from(code)) }
}

fn run_with(
    prog: &Program,
    opts: &RunOptions,
    exit_busy: bool,
    go: impl FnOnce(&Program, &keleut_interp::RunOptions, Setup<'_>) -> u8,
) -> u8 {
    let Some(mode) = opts.tier.mode() else {
        let mut setup = |_: &mut Interp<'_>| Ok(());
        return go(prog, &opts.base, &mut setup);
    };
    std::thread::scope(|s| {
        let (init_tx, init_rx) = mpsc::channel();
        let (req_tx, req_rx) = mpsc::channel::<(FuncId, bool)>();
        let (rep_tx, rep_rx) = mpsc::channel::<Result<usize, String>>();
        let failure: Failure = Arc::new((AtomicBool::new(false), Mutex::new(None)));
        let passes = opts.passes.as_str();
        let fast_alloc = !opts.base.gc_stress;
        let fail2 = failure.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let busy = Arc::new(AtomicBool::new(false));
        let busy2 = busy.clone();
        // エントリテーブル。
        // コンパイルスレッドが起動処理を終えるまで、どの項目も 0 である。
        let entry: Arc<[AtomicUsize]> =
            (0..prog.funcs.len()).map(|_| AtomicUsize::new(0)).collect();
        let entry2 = entry.clone();
        let spawned = std::thread::Builder::new()
            .name("keleut-jit".to_string())
            .stack_size(JIT_STACK)
            .spawn_scoped(s, move || {
                jit_thread(
                    prog, entry2, passes, fast_alloc, init_tx, req_rx, rep_tx, fail2, stop2, busy2,
                )
            });
        if spawned.is_err() {
            // コンパイルスレッドを作れない(仮想メモリを制限しているときなど)。
            // これをプログラムの失敗にはせず、コンパイルなしでインタプリタで実行する(コンパイル中にメモリの確保が失敗したときと同じ)。
            let mut setup = |_: &mut Interp<'_>| Ok(());
            return go(prog, &opts.base, &mut setup);
        }
        // `threshold=N` では、LLJIT の作成とスタブのコンパイル(数ミリ秒)を待たずに、インタプリタで実行を始める。
        // ミューテータは、コンパイルを依頼した関数のエントリテーブルの項目が 0 でなくなったときに、起動処理の結果を受け取る。
        // 項目が 0 でなくなるのは、起動処理が済んで、項目が `interp_stub` かコンパイル済みの関数を指すようになったときである。
        // ほかのティアの設定では、最初の呼び出しでコンパイルを待つので、ここで起動処理を待つ。
        let lazy = matches!(mode, TierMode::Threshold(_));
        let recv_init = move || -> Result<(u64, EnterFn), String> {
            let init = match init_rx.recv() {
                Ok(Ok(i)) => i,
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("JIT のスレッドが異常終了しました".to_string()),
            };
            // SAFETY: enter はコンパイルした enter_tailcc のアドレスで、その関数の型は EnterFn と同じである。
            let enter: EnterFn = unsafe { std::mem::transmute::<usize, EnterFn>(init.enter) };
            Ok((init.stub as u64, enter))
        };
        let (eager, pending): (Option<EnterFn>, Option<PendingInit>) = if lazy {
            (None, Some(Box::new(recv_init)))
        } else {
            match recv_init() {
                Ok((_, enter)) => (Some(enter), None),
                Err(e) => {
                    eprintln!("keleut: 内部エラー: LLVM の JIT を始められません: {e}");
                    return 70;
                }
            }
        };
        let mut parts = Some((eager, pending, entry, req_tx, rep_rx, failure));
        let mut setup = move |it: &mut Interp<'_>| -> Result<(), Stop> {
            let (eager, pending, entry, req_tx, rep_rx, failure) =
                parts.take().ok_or_else(|| {
                    Stop::Internal("ネイティブのバックエンドを 2 度付けました".into())
                })?;
            let enter = eager.unwrap_or(no_enter);
            let request = Box::new(
                move |f: FuncId, wait: bool| -> Result<Option<usize>, String> {
                    req_tx
                        .send((f, wait))
                        .map_err(|_| "JIT のスレッドが終わりました".to_string())?;
                    if !wait {
                        return Ok(None);
                    }
                    rep_rx
                        .recv()
                        .map_err(|_| "JIT のスレッドが終わりました".to_string())?
                        .map(Some)
                },
            );
            let translator = Translator { request, failure };
            let mut n = Native::new(it.rt.prog, entry, enter, mode, translator);
            if let Some(p) = pending {
                n.set_pending_init(p);
            }
            it.set_native(Box::new(n));
            Ok(())
        };
        let code = go(prog, &opts.base, &mut setup);
        stop.store(true, Ordering::SeqCst);
        if exit_busy && (lazy || busy.load(Ordering::SeqCst)) {
            // コンパイルスレッドが、コンパイルの途中か起動処理の途中であることがある。
            // プログラムの実行は終わっているので、待たずに終了する。
            exit_now(code);
        }
        code
    })
}

/// JIT コンパイルできる関数の LLVM IR のテキストを並べて返す(`keleut dump llvm`)。
///
/// JIT コンパイルできない関数については、LLVM IR の代わりに、`インタプリタに残す` と書いたコメント行(`;` で始まる行)を 1 行出力する。
/// エントリテーブルのアドレスは 0 として書く。
/// 調査用の出力で、形式の互換性は保証しない。
pub fn dump_ir(prog: &Program) -> Result<String, String> {
    let mut s = String::new();
    s.push_str(codegen::DECLS);
    for (i, f) in prog.funcs.iter().enumerate() {
        s.push('\n');
        if keleut_interp::native::jit_eligible(f) {
            s.push_str(&codegen::function(prog, FuncId(i as u32), 0, 0, true)?);
        } else {
            s.push_str(&format!(
                "; {} ({}): インタプリタに残す\n",
                f.sym,
                f.kind.name()
            ));
        }
    }
    Ok(s)
}

/// コンパイルの前に残しておく仮想メモリの余裕(64 MiB)。
///
/// `RLIMIT_AS` の上限と現在の仮想メモリのサイズの差がこれを下回れば、コンパイルしない。
const JIT_AS_ROOM: u64 = 64 << 20;

/// 仮想メモリのソフトリミット(`RLIMIT_AS`)をバイト数で返す。
/// 上限がなければ `None` を返す。
fn address_space_limit() -> Option<u64> {
    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, rlim: *mut RLimit) -> i32;
    }
    /// Linux での `RLIMIT_AS` の値。
    const RLIMIT_AS: i32 = 9;
    let mut r = RLimit { cur: 0, max: 0 };
    // SAFETY: r は、この呼び出しの間だけ使う有効な書き込み先である。
    if unsafe { getrlimit(RLIMIT_AS, &mut r) } != 0 || r.cur == u64::MAX {
        return None;
    }
    Some(r.cur)
}

/// `/proc/self/statm` の最初のフィールド(仮想メモリのサイズ。単位はページ)をバイト数にして返す。
/// 読めなければ `None` を返す。
///
/// 仮想メモリの残りが少ないときに呼ぶので、ヒープに確保せず、スタック上のバッファに読み込む。
fn virtual_size() -> Option<u64> {
    use std::io::Read;
    unsafe extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    /// Linux での `_SC_PAGESIZE` の値。
    const SC_PAGESIZE: i32 = 30;
    let mut buf = [0u8; 128];
    let n = std::fs::File::open("/proc/self/statm")
        .and_then(|mut f| f.read(&mut buf))
        .ok()?;
    let pages = parse_statm_size(&buf[..n])?;
    // SAFETY: sysconf は libc の関数で、引数は定数である。
    let page = unsafe { sysconf(SC_PAGESIZE) };
    pages.checked_mul(u64::try_from(page).ok().filter(|&p| p > 0)?)
}

/// `/proc/self/statm` の内容から、最初のフィールド(ページ数)を読む。
fn parse_statm_size(s: &[u8]) -> Option<u64> {
    let end = s.iter().position(|&b| b == b' ').unwrap_or(s.len());
    std::str::from_utf8(&s[..end]).ok()?.trim().parse().ok()
}

/// 上限 `limit` と仮想メモリのサイズ `size`(どちらもバイト数)から、コンパイルしてよいかを決める。
///
/// 差が [`JIT_AS_ROOM`] 以上なら真を返す。
/// サイズを読めなかったとき(`None`)も真を返し、残りを調べずにコンパイルする。
fn room_for_translation(limit: u64, size: Option<u64>) -> bool {
    match size {
        Some(size) => limit.saturating_sub(size) >= JIT_AS_ROOM,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statm_first_field() {
        assert_eq!(parse_statm_size(b"12345 678 9 1 0 2 0\n"), Some(12345));
        assert_eq!(parse_statm_size(b"42\n"), Some(42));
        assert_eq!(parse_statm_size(b""), None);
        assert_eq!(parse_statm_size(b"x 1"), None);
    }

    #[test]
    fn room_threshold() {
        let limit = 512 << 20;
        assert!(room_for_translation(limit, Some(limit - JIT_AS_ROOM)));
        assert!(!room_for_translation(limit, Some(limit - JIT_AS_ROOM + 1)));
        assert!(!room_for_translation(limit, Some(limit + 1)));
        assert!(room_for_translation(limit, None));
    }

    #[test]
    fn reads_own_virtual_size() {
        // テストのプロセスの仮想メモリのサイズは 0 より大きく、4096 バイトの倍数である。
        let size = virtual_size().expect("/proc/self/statm を読めない");
        assert!(size > 0);
        assert_eq!(size % 4096, 0);
    }
}
