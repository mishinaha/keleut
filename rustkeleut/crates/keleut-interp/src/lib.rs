//! LIR のバイトコードの符号化とインタプリタ(計画 261024-1 の 6.7 節、D535)。
//!
//! | 入口 | 働き |
//! | --- | --- |
//! | [`run_program`] | LIR のプログラムを専用のスレッドで実行し、終了状態を返す |
//! | [`bytecode::encode`] | 1 つの関数をバイトコードにする |
//! | [`interp::Interp`] | インタプリタ(フレームは分割値スタックに置く) |
//!
//! エフェクト(`handle`、利用者の操作の `perform`、`resume`)は M89 で足す。
//! それまでは、これらに下ろした `unimplemented_value` に届くと終了状態 4 で終わる。

pub mod bytecode;
pub mod interp;

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
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            max_depth: keleut_rt::DEFAULT_MAX_DEPTH,
            native_stack: keleut_rt::native_stack_for(keleut_rt::DEFAULT_MAX_DEPTH),
        }
    }
}

fn run_on_thread(prog: &Program, opts: &RunOptions) -> u8 {
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
    let rt = match Rt::new(prog, opts.max_depth) {
        Ok(rt) => rt,
        Err(e) => {
            keleut_rt::out::Out::default().eprint(&e.message());
            return e.exit_code();
        }
    };
    let mut it = Interp::new(rt, low);
    let mut r: Result<(), Stop> = Ok(());
    for &f in &prog.init {
        if let Err(e) = it.run(f, Word(0), &[], 0) {
            r = Err(e);
            break;
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
    let size = usize::try_from(opts.native_stack).unwrap_or(usize::MAX);
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("keleut-mutator".to_string())
            .stack_size(size)
            .spawn_scoped(s, || run_on_thread(prog, opts));
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
