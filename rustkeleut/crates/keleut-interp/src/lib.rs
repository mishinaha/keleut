//! LIR のバイトコードの符号化とインタプリタ(計画 261024-1 の 6.7 節、D535)。
//!
//! | 入口 | 働き |
//! | --- | --- |
//! | [`run_program`] | LIR のプログラムを専用のスレッドで実行し、終了状態を返す |
//! | [`bytecode::encode`] | 1 つの関数をバイトコードにする |
//! | [`interp::Interp`] | インタプリタ(フレームは分割値スタックに置く) |
//! | [`run_program_with`] | ネイティブの後段(M91 の JIT。[`native`])を付けたインタプリタで実行する |
//!
//! エフェクト(`handle`、利用者の操作の `perform`、`resume`)は、`doc/lir.md` の 16 節の規約で実行する
//! ([`interp`] の冒頭)。

pub mod bytecode;
pub mod interp;
pub mod native;

use keleut_lir::Program;
use keleut_rt::{Rt, Stop, Word};

pub use interp::Interp;

/// 専用のスレッドのネイティブのスタックの最小の大きさ(バイト)。
///
/// 予備の検査の余裕 [`interp::NATIVE_MARGIN`] の 2 倍で、残りの半分を実行の初めの段に使う。
/// これより小さいと、最初の命令の前に予備の検査が働く。
pub const MIN_NATIVE_STACK: u64 = 2 * interp::NATIVE_MARGIN as u64;

/// 実行の選択肢。
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// 論理的な深さの限度 `L`。
    pub max_depth: u64,
    /// 専用のスレッドのネイティブのスタックの大きさ(バイト)。
    /// [`MIN_NATIVE_STACK`] 以上にする。
    pub native_stack: u64,
    /// `--gc-stress` の規則で回収するか(`keleut_rt::gc::STRESS_RATIO`)。
    pub gc_stress: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            max_depth: keleut_rt::DEFAULT_MAX_DEPTH,
            native_stack: keleut_rt::native_stack_for(keleut_rt::DEFAULT_MAX_DEPTH),
            gc_stress: false,
        }
    }
}

/// インタプリタを作った後、初期化を走らせる前に呼ぶ準備(ネイティブの後段を付ける)。
pub type Setup<'s> = &'s mut (dyn FnMut(&mut Interp<'_>) -> Result<(), Stop> + Send);

fn run_on_thread(prog: &Program, opts: &RunOptions, setup: Option<Setup<'_>>) -> u8 {
    keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Exec);
    let x = 0u8;
    let sp0 = std::hint::black_box(&x) as *const u8 as usize;
    let low = sp0
        .saturating_sub(usize::try_from(opts.native_stack).unwrap_or(usize::MAX))
        .saturating_add(interp::NATIVE_MARGIN);
    if low >= sp0 {
        eprintln!(
            "keleut: 内部エラー: ネイティブのスタック({} バイト)が予備の余裕より小さいです",
            opts.native_stack
        );
        return 70;
    }
    run_mutator(prog, opts, low, interp::NATIVE_MARGIN, setup)
}

fn run_mutator(
    prog: &Program,
    opts: &RunOptions,
    low: usize,
    margin: usize,
    setup: Option<Setup<'_>>,
) -> u8 {
    let stats = std::env::var_os("KELEUT_GC_STATS").is_some();
    let rt = match Rt::new_with(prog, opts.max_depth, opts.gc_stress) {
        Ok(rt) => rt,
        Err(e) => {
            keleut_rt::out::Out::default().eprint(&e.message());
            return e.exit_code();
        }
    };
    let mut it = Interp::new(rt, low);
    it.set_stack_hop(
        usize::try_from(opts.native_stack)
            .unwrap_or(usize::MAX)
            .max(interp::HOP_STACK_MIN),
        margin,
    );
    if let Some(setup) = setup
        && let Err(e) = setup(&mut it)
    {
        it.rt.out.eprint(&e.message());
        return e.exit_code();
    }
    let code = run_init(prog, &mut it);
    if stats {
        let s = it.rt.gc_stats();
        let _ = it.rt.out.eprint_after_flush(s.as_bytes());
    }
    code
}

fn run_init(prog: &Program, it: &mut Interp<'_>) -> u8 {
    let mut r: Result<(), Stop> = Ok(());
    for &f in &prog.init {
        match it.run(f, Word(0), &[], 0) {
            Ok(v) if v == Word::SENTINEL => {
                // 状態語が最上位に届いた。実行時エラーならその文面で止まる。
                r = Err(match std::mem::take(&mut it.rt.pending) {
                    keleut_rt::effect::Pending::Error(s) => s,
                    p => Stop::Internal(format!("最上位に届いた状態語: {p:?}")),
                });
                it.rt.limit = it.rt.max_depth;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                r = Err(e);
                break;
            }
        }
    }
    match r {
        Ok(()) => match it.rt.out.flush() {
            Ok(()) => 0,
            Err(e) => {
                let s = Stop::from(e);
                it.rt.out.eprint(&s.message());
                s.exit_code()
            }
        },
        Err(e) => {
            it.rt.out.eprint(&e.message());
            e.exit_code()
        }
    }
}

/// LIR のプログラム(パス 12 を済ませたもの)を実行し、終了状態を返す。
///
/// ミューテータは、ネイティブのスタックを `opts.native_stack` バイト予約した専用のスレッドで走らせる(計画 6.11 節)。
/// 標準出力は終わる前に flush し、止まった理由の診断を標準エラーに書く。
pub fn run_program(prog: &Program, opts: &RunOptions) -> u8 {
    run_program_inner(prog, opts, None)
}

/// [`run_program`] と同じで、インタプリタを作った後、初期化を走らせる前に `setup` を呼ぶ。
///
/// ネイティブの後段(M91 の JIT)は `setup` で [`Interp::set_native`] を呼んで付ける。
/// `setup` は専用のスレッドで呼ばれる。
pub fn run_program_with(
    prog: &Program,
    opts: &RunOptions,
    setup: &mut (dyn FnMut(&mut Interp<'_>) -> Result<(), Stop> + Send),
) -> u8 {
    run_program_inner(prog, opts, Some(setup))
}

/// 今のスレッドで実行する(試験用)。`margin` は予備の検査の余裕(バイト)。
pub fn run_program_here(
    prog: &Program,
    opts: &RunOptions,
    margin: usize,
    setup: Option<Setup<'_>>,
) -> u8 {
    let x = 0u8;
    let sp0 = std::hint::black_box(&x) as *const u8 as usize;
    let low = sp0
        .saturating_sub(usize::try_from(opts.native_stack).unwrap_or(usize::MAX))
        .saturating_add(margin);
    run_mutator(prog, opts, low, margin, setup)
}

fn run_program_inner(prog: &Program, opts: &RunOptions, mut setup: Option<Setup<'_>>) -> u8 {
    let size = usize::try_from(opts.native_stack).unwrap_or(usize::MAX);
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("keleut-mutator".to_string())
            .stack_size(size)
            .spawn_scoped(s, || run_on_thread(prog, opts, setup.take()));
        match h {
            Ok(h) => match h.join() {
                Ok(code) => code,
                Err(_) => {
                    eprintln!("keleut: 内部エラー: 実行のスレッドが落ちました");
                    70
                }
            },
            Err(e) => {
                eprintln!(
                    "keleut: 内部エラー: 実行のスレッドを作れません({size} バイトのスタック): {e}"
                );
                70
            }
        }
    })
}
