//! 翻訳のスレッドと、ネイティブの後段を付けた実行(crate の冒頭)。

use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;

use keleut_interp::native::{EnterFn, Native};
use keleut_interp::{Interp, Setup};
use keleut_lir::{FuncId, Program};
use keleut_rt::Stop;

use crate::codegen;
use crate::sys::Jit;

/// 段の設定(計画 6.9 節の `--tier`)。M91 は `jit-only` と `interp-only` だけである。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// JIT にできる関数をすべて JIT で走らせる。
    JitOnly,
    /// どの関数もインタプリタで走らせる。
    InterpOnly,
}

/// 実行の選択肢。
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// インタプリタの選択肢(深さの限度、ネイティブのスタック、`--gc-stress`)。
    pub base: keleut_interp::RunOptions,
    /// 段の設定。
    pub tier: Tier,
    /// LLVM の最適化の列(既定は `default<O2>`)。
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

/// 翻訳のスレッドが起動時に返すもの。
struct Init {
    entry: Box<[AtomicUsize]>,
    enter: usize,
}

/// 翻訳のスレッドのスタックの大きさ。
const JIT_STACK: usize = 256 << 20;

/// 翻訳のスレッドの本体。起動の結果を `init` に送り、関数の番号を受け取るたびに翻訳して番地を返す。
fn jit_thread(
    prog: &Program,
    passes: &str,
    init: mpsc::Sender<Result<Init, String>>,
    req: mpsc::Receiver<FuncId>,
    rep: mpsc::Sender<Result<usize, String>>,
) {
    let start = || -> Result<(Jit, Init, usize, String, String), String> {
        let jit = Jit::new()?;
        jit.define_absolute(&keleut_interp::native::helper_symbols())?;
        let n = prog.funcs.len();
        let entry: Box<[AtomicUsize]> = (0..n).map(|_| AtomicUsize::new(0)).collect();
        let table = entry.as_ptr() as usize;
        let (triple, layout) = (jit.triple(), jit.data_layout());
        let stubs = format!(
            "target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{}",
            codegen::stubs(table)
        );
        jit.add_ir("keleut-stubs", &stubs, passes)?;
        let stub = jit.lookup("interp_stub")?;
        let enter = jit.lookup("enter_tailcc")?;
        for e in entry.iter() {
            e.store(stub, std::sync::atomic::Ordering::Relaxed);
        }
        Ok((jit, Init { entry, enter }, table, triple, layout))
    };
    let (jit, table, triple, layout) = match start() {
        Ok((jit, i, table, triple, layout)) => {
            if init.send(Ok(i)).is_err() {
                return;
            }
            (jit, table, triple, layout)
        }
        Err(e) => {
            let _ = init.send(Err(e));
            return;
        }
    };
    while let Ok(f) = req.recv() {
        let r = codegen::module(prog, f, table, &triple, &layout).and_then(|src| {
            let name = codegen::func_name(f);
            jit.add_ir(&name, &src, passes)?;
            jit.lookup(&name)
        });
        if rep.send(r).is_err() {
            break;
        }
    }
}

/// ネイティブの後段を付けてプログラムを実行し、終了状態を返す。
pub fn run_program(prog: &Program, opts: &RunOptions) -> u8 {
    run_with(prog, opts, |prog, opts, setup| {
        keleut_interp::run_program_with(prog, opts, setup)
    })
}

/// [`run_program`] と同じで、今のスレッドで実行する(試験用。`margin` は予備の検査の余裕)。
pub fn run_program_here(prog: &Program, opts: &RunOptions, margin: usize) -> u8 {
    run_with(prog, opts, |prog, opts, setup| {
        keleut_interp::run_program_here(prog, opts, margin, Some(setup))
    })
}

fn run_with(
    prog: &Program,
    opts: &RunOptions,
    go: impl FnOnce(&Program, &keleut_interp::RunOptions, Setup<'_>) -> u8,
) -> u8 {
    if opts.tier == Tier::InterpOnly {
        let mut setup = |_: &mut Interp<'_>| Ok(());
        return go(prog, &opts.base, &mut setup);
    }
    std::thread::scope(|s| {
        let (init_tx, init_rx) = mpsc::channel();
        let (req_tx, req_rx) = mpsc::channel::<FuncId>();
        let (rep_tx, rep_rx) = mpsc::channel::<Result<usize, String>>();
        let passes = opts.passes.as_str();
        let spawned = std::thread::Builder::new()
            .name("keleut-jit".to_string())
            .stack_size(JIT_STACK)
            .spawn_scoped(s, move || jit_thread(prog, passes, init_tx, req_rx, rep_tx));
        if let Err(e) = spawned {
            eprintln!("keleut: 内部エラー: JIT のスレッドを作れません: {e}");
            return 70;
        }
        let init = match init_rx.recv() {
            Ok(Ok(i)) => i,
            Ok(Err(e)) => {
                eprintln!("keleut: 内部エラー: LLVM の JIT を始められません: {e}");
                return 70;
            }
            Err(_) => {
                eprintln!("keleut: 内部エラー: JIT のスレッドが落ちました");
                return 70;
            }
        };
        let mut parts = Some((init, req_tx, rep_rx));
        let mut setup = move |it: &mut Interp<'_>| -> Result<(), Stop> {
            let (init, req_tx, rep_rx) = parts
                .take()
                .ok_or_else(|| Stop::Internal("ネイティブの後段を 2 度付けました".into()))?;
            // SAFETY: enter は翻訳した enter_tailcc の番地で、型は EnterFn と同じである。
            let enter: EnterFn = unsafe { std::mem::transmute::<usize, EnterFn>(init.enter) };
            let compile = Box::new(move |f: FuncId| -> Result<usize, String> {
                req_tx
                    .send(f)
                    .map_err(|_| "JIT のスレッドが終わりました".to_string())?;
                rep_rx
                    .recv()
                    .map_err(|_| "JIT のスレッドが終わりました".to_string())?
            });
            let n = Native::new(it.rt.prog, init.entry, enter, true, compile);
            it.set_native(Box::new(n));
            Ok(())
        };
        go(prog, &opts.base, &mut setup)
    })
}

/// JIT にできる関数の LLVM IR の文面を並べる(`keleut dump llvm`)。
///
/// 入口の表の番地は 0 として書く(調べるための出力で、互換性を約束しない)。
pub fn dump_ir(prog: &Program) -> Result<String, String> {
    let mut s = String::new();
    s.push_str(codegen::DECLS);
    for (i, f) in prog.funcs.iter().enumerate() {
        s.push('\n');
        if keleut_interp::native::jit_eligible(f) {
            s.push_str(&codegen::function(prog, FuncId(i as u32), 0)?);
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
