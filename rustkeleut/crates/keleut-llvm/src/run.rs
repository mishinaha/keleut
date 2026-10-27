//! 翻訳のスレッドと、ネイティブの後段を付けた実行(crate の冒頭)。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use keleut_interp::native::{EnterFn, Native, TierMode, Translator};
use keleut_interp::{Interp, Setup};
use keleut_lir::{FuncId, Program};
use keleut_rt::Stop;

use crate::codegen;
use crate::sys::Jit;

/// 段の設定(計画 6.9 節の `--tier`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// JIT にできる関数をすべて JIT で走らせる(初めて呼ぶときに翻訳する)。
    JitOnly,
    /// どの関数もインタプリタで走らせる。
    InterpOnly,
    /// インタプリタで `N` 回呼んだ関数を、翻訳のスレッドで翻訳する(待たない)。
    Threshold(u64),
    /// JIT の関数の入口ごとに、種から決まる擬似乱数で段を選ぶ。
    Alternating(u64),
    /// 種で決まる半分の関数だけを JIT で走らせる。
    Mixed(u64),
}

impl Tier {
    /// `--tier` の綴り(`interp-only`、`jit-only`、`threshold=N`、`alternating=SEED`、`mixed=SEED`)を読む。
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

/// 段の既定(計画 6.2 節。しきい値の既定は M93 で決める。P249)。
pub const DEFAULT_TIER: Tier = Tier::Threshold(1000);

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
    entry: Arc<[AtomicUsize]>,
    enter: usize,
}

/// 翻訳のスレッドのスタックの大きさ。
///
/// 予約した大きさは仮想メモリの上限(`ulimit -v`)に数えられるので、小さく保つ。
/// 翻訳は関数ごとの小さなモジュールで行い、LLVM の再帰の深さは関数の大きさに縛られるので、16 MiB で足りる。
/// 256 MiB にすると、`conformance/gc/gc.t` の上限 512 MiB の下で JIT の段だけがメモリ不足(3)やスレッドを作れない誤り(70)で終わった。
const JIT_STACK: usize = 16 << 20;

/// 待たない頼みの翻訳の失敗の印と文面。
type Failure = Arc<(AtomicBool, Mutex<Option<String>>)>;

/// 翻訳のスレッドの本体。起動の結果を `init` に送り、関数の番号を受け取るたびに翻訳する。
///
/// 実行が終わったら(`stop`)、残りの頼みを捨てて終わる。
/// 頼みを受け取ってから翻訳を終えるまで `busy` を真にする。
/// `busy` を真にしてから `stop` を読むので(どちらも SeqCst)、実行の終わりに `busy` が偽なら、このスレッドは新しい翻訳を始めない。
/// LLVM の最適化の列に入る前にも `stop` を読み直し、立っていれば翻訳を捨てる。
///
/// 翻訳を終えたら入口の表の項目を書き換える(Release)。
/// 待つ頼み(組の 2 番目が真)には、結果を `rep` に送る。
/// 待たない頼みの失敗は `failure` に書き、ミューテータが次に入口を調べるときに内部の誤りにする。
#[allow(clippy::too_many_arguments)]
fn jit_thread(
    prog: &Program,
    passes: &str,
    init: mpsc::Sender<Result<Init, String>>,
    req: mpsc::Receiver<(FuncId, bool)>,
    rep: mpsc::Sender<Result<usize, String>>,
    failure: Failure,
    stop: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
) {
    let start = || -> Result<(Jit, Init, usize, String, String), String> {
        let jit = Jit::new()?;
        jit.define_absolute(&keleut_interp::native::helper_symbols())?;
        let n = prog.funcs.len();
        let entry: Arc<[AtomicUsize]> = (0..n).map(|_| AtomicUsize::new(0)).collect();
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
            e.store(stub, Ordering::Relaxed);
        }
        Ok((jit, Init { entry, enter }, table, triple, layout))
    };
    let (jit, entry, table, triple, layout) = match start() {
        Ok((jit, i, table, triple, layout)) => {
            let entry = i.entry.clone();
            if init.send(Ok(i)).is_err() {
                return;
            }
            (jit, entry, table, triple, layout)
        }
        Err(e) => {
            let _ = init.send(Err(e));
            return;
        }
    };
    while let Ok((f, wait)) = req.recv() {
        busy.store(true, Ordering::SeqCst);
        if stop.load(Ordering::SeqCst) {
            // 実行が終わった。残りの待たない頼みは翻訳しない。
            break;
        }
        let src = codegen::module(prog, f, table, &triple, &layout);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let r = src.and_then(|src| {
            let name = codegen::func_name(f);
            jit.add_ir(&name, &src, passes)?;
            jit.lookup(&name)
        });
        busy.store(false, Ordering::SeqCst);
        match &r {
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

/// ネイティブの後段を付けてプログラムを実行し、終了状態を返す。
///
/// 実行が終わったとき翻訳のスレッドがまだ翻訳していれば、その翻訳を待たずに、終了状態 `code` でプロセスを終える(戻らない)。
/// 待たない頼み(`threshold=N`)の翻訳は大きな関数では数分かかることがあり、結果を変えない翻訳のために終了を遅らせないためである。
/// このとき標準出力と標準エラーを flush してから `_exit` を呼ぶ。
/// `exit` を使わないのは、翻訳のスレッドが LLVM の中にいる間に LLVM の静的な破棄子を走らせないためである。
pub fn run_program(prog: &Program, opts: &RunOptions) -> u8 {
    run_with(prog, opts, true, |prog, opts, setup| {
        keleut_interp::run_program_with(prog, opts, setup)
    })
}

/// [`run_program`] と同じで、プロセスを終えない(試験用)。
///
/// 実行の後、翻訳のスレッドが終わるのを待ってから終了状態を返す。
/// ミューテータのスレッドは [`run_program`] と同じく `native_stack` の大きさで作る。
pub fn run_program_wait(prog: &Program, opts: &RunOptions) -> u8 {
    run_with(prog, opts, false, |prog, opts, setup| {
        keleut_interp::run_program_with(prog, opts, setup)
    })
}

/// [`run_program`] と同じで、今のスレッドで実行する(試験用。`margin` は予備の検査の余裕)。
///
/// こちらはプロセスを終えず、翻訳のスレッドが終わるのを待ってから戻る。
pub fn run_program_here(prog: &Program, opts: &RunOptions, margin: usize) -> u8 {
    run_with(prog, opts, false, |prog, opts, setup| {
        keleut_interp::run_program_here(prog, opts, margin, Some(setup))
    })
}

unsafe extern "C" {
    fn _exit(status: i32) -> !;
}

/// 標準出力と標準エラーを flush し、終了の処理を走らせずにプロセスを終える。
fn exit_now(code: u8) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: _exit は libc の関数で、引数は終了状態である。どのスレッドから呼んでもよい。
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
        let fail2 = failure.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let busy = Arc::new(AtomicBool::new(false));
        let busy2 = busy.clone();
        let spawned = std::thread::Builder::new()
            .name("keleut-jit".to_string())
            .stack_size(JIT_STACK)
            .spawn_scoped(s, move || {
                jit_thread(prog, passes, init_tx, req_rx, rep_tx, fail2, stop2, busy2)
            });
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
        let mut parts = Some((init, req_tx, rep_rx, failure));
        let mut setup = move |it: &mut Interp<'_>| -> Result<(), Stop> {
            let (init, req_tx, rep_rx, failure) = parts
                .take()
                .ok_or_else(|| Stop::Internal("ネイティブの後段を 2 度付けました".into()))?;
            // SAFETY: enter は翻訳した enter_tailcc の番地で、型は EnterFn と同じである。
            let enter: EnterFn = unsafe { std::mem::transmute::<usize, EnterFn>(init.enter) };
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
            let n = Native::new(it.rt.prog, init.entry, enter, mode, translator);
            it.set_native(Box::new(n));
            Ok(())
        };
        let code = go(prog, &opts.base, &mut setup);
        stop.store(true, Ordering::SeqCst);
        if exit_busy && busy.load(Ordering::SeqCst) {
            // 翻訳の途中である。結果は出し終えたので、翻訳を待たずに終える。
            exit_now(code);
        }
        code
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
