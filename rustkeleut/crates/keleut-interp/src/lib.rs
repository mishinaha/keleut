//! LIR をバイトコードに符号化し、インタプリタで実行する。
//!
//! | 項目 | 役割 |
//! | --- | --- |
//! | [`run_program`] | LIR のプログラムを実行スレッドで実行し、終了コードを返す |
//! | [`bytecode::encode`] | 1 つの関数をバイトコードに符号化する |
//! | [`interp::Interp`] | インタプリタ(フレームは分割値スタックに置く) |
//! | [`run_program_with`] | ネイティブコードのバックエンド(JIT。境界は [`native`])を付けたインタプリタで実行する |
//!
//! 実行スレッドは、プログラムを実行するために作る専用のスレッドである。
//! エフェクト(`handle`、ユーザの操作の `perform`、`resume`)は、`doc/lir.md` の 16 節の規約で実行する。
//! 実装の説明は [`interp`] の冒頭にある。

pub mod bytecode;
pub mod interp;
pub mod native;

use keleut_lir::Program;
use keleut_rt::{Rt, Stop, Word};

pub use interp::Interp;

/// 実行スレッドのネイティブスタックの最小サイズ(バイト)。
///
/// スタック残量の検査の余裕 [`interp::NATIVE_MARGIN`] の 2 倍である。
/// 半分がスタック残量の検査の余裕で、残りの半分が実行に使える領域である。
/// これより小さいと、最初の命令を実行する前にスタック残量の検査で失敗する。
pub const MIN_NATIVE_STACK: u64 = 2 * interp::NATIVE_MARGIN as u64;

/// [`run_program`] などに渡す実行オプション。
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// 論理的な深さの上限 `L`。
    pub max_depth: u64,
    /// 実行スレッドのネイティブスタックのサイズ(バイト)。
    /// [`MIN_NATIVE_STACK`] 以上にする。
    pub native_stack: u64,
    /// 真なら、`--gc-stress` の規則で GC を実行する(規則は `keleut_rt::gc::STRESS_RATIO` の説明にある)。
    pub gc_stress: bool,
    /// 真なら、仮想メモリの上限(`ulimit -v`)があるときに、スタックを最初から上限の 4 分の 1 までに抑える([`exec_stack_sizes`])。
    ///
    /// `--native-stack` を指定しなかったとき(既定のサイズのとき)に真にする。
    pub fit_stack: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            max_depth: keleut_rt::DEFAULT_MAX_DEPTH,
            native_stack: keleut_rt::native_stack_for(keleut_rt::DEFAULT_MAX_DEPTH),
            gc_stress: false,
            fit_stack: true,
        }
    }
}

/// 実行スレッドを作り直すときのスタックサイズの下限(16 MiB)。
///
/// スタックを 512 KiB から 1.1 GB までのどのサイズにしても、深さ 100 万の非末尾の再帰と、深さ 20 万の構造的な `Eq` の比較は同じ結果になる。
/// 呼び出しのフレームは値スタックに置き、ネイティブスタックが足りなければ、JIT の関数はインタプリタでの実行に切り替わり、構造的な `Eq` の比較は新しいスタックを持つスレッドに実行を切り替えるからである。
/// この下限は、KIR を処理するスレッドとフロントエンドのスレッドの下限と同じ値である。
pub const MIN_EXEC_STACK: u64 = 16 << 20;

/// 構造的な `Eq` の比較の入れ子が深くなったときに実行を切り替える先のスレッドの、スタックサイズ(バイト)を返す。
///
/// 実行スレッドのスタックサイズ `native_stack` と [`interp::HOP_STACK_MIN`] の大きいほうにする。
/// 仮想メモリの上限があれば、さらに上限の 16 分の 1(下限は [`interp::HOP_STACK_MIN`])までに抑える。
/// 切り替えた先のスレッドは、スタックの残りが少なくなるまで入れ子を進めてから次のスレッドに切り替えるので、
/// 予約したまま使わない領域は最後のスレッドの残りだけである。
/// スタックが小さいほどこの残りも小さいので、上限があるときは小さくする。
pub fn hop_stack_size(native_stack: u64) -> usize {
    let mut size = usize::try_from(native_stack)
        .unwrap_or(usize::MAX)
        .max(interp::HOP_STACK_MIN);
    if let Some(a) = keleut_rt::alloc::address_space_limit() {
        size = size.min((a / 16).max(interp::HOP_STACK_MIN));
    }
    size
}

/// 実行スレッドのスタックサイズの候補を、試す順に並べて返す。
///
/// 仮想メモリの上限がなければ、候補は `native_stack` だけである。
/// 上限があれば、最初の候補は `native_stack` で、`fit` が真なら上限の 4 分の 1(下限は [`MIN_EXEC_STACK`])までに抑える。
/// その後に、半分ずつ小さくしたサイズを並べ、最後を [`MIN_EXEC_STACK`] にする。
/// 最初の候補が [`MIN_EXEC_STACK`] 以下なら、候補はそれだけである。
pub fn exec_stack_sizes(native_stack: u64, fit: bool) -> Vec<u64> {
    let mut size = native_stack;
    let Some(a) = keleut_rt::alloc::address_space_limit() else {
        return vec![size];
    };
    if fit {
        size = size.min((a as u64 / 4).max(MIN_EXEC_STACK));
    }
    let mut v = vec![size];
    while size > MIN_EXEC_STACK {
        size = (size / 2).max(MIN_EXEC_STACK);
        v.push(size);
    }
    v
}

/// インタプリタを作った後、トップレベルの初期化コードを実行する前に呼ぶ準備の関数。
///
/// ネイティブコードのバックエンドをインタプリタに付けるために使う。
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
    it.set_stack_hop(hop_stack_size(opts.native_stack), margin);
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
                // 状態ワードが最上位まで伝播した。実行時エラーなら、そのメッセージを書いて終了する。
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

/// LIR のプログラム(パス 12 を済ませたもの)を実行し、終了コードを返す。
///
/// プログラムは、ネイティブスタックを `opts.native_stack` バイト予約した実行スレッドで実行する。
/// 仮想メモリの上限(`ulimit -v`)があり `opts.fit_stack` が真なら、スタックを最初から上限の 4 分の 1 までに抑える。
/// 仮想メモリの上限があるときにスレッドを作れなければ、スタックを半分ずつ小さくして作り直す([`exec_stack_sizes`])。
/// スタックが小さくなっても、論理的な深さの上限は変わらない(理由は [`MIN_EXEC_STACK`] の説明にある)。
/// 最も小さいスタックでも作れなければ、`実行時エラー: メモリ不足です` を標準エラーに書いて 3 を返す。
/// 仮想メモリの上限がないのにスレッドを作れなければ、内部エラーとして 70 を返す。
/// 標準出力は終了する前に flush し、終了した理由の診断を標準エラーに書く。
pub fn run_program(prog: &Program, opts: &RunOptions) -> u8 {
    run_program_inner(prog, opts, None)
}

/// [`run_program`] と同じように実行するが、インタプリタを作った後、トップレベルの初期化コードを実行する前に `setup` を呼ぶ。
///
/// ネイティブコードのバックエンド(JIT)は、`setup` の中で [`Interp::set_native`] を呼んで付ける。
/// `setup` は実行スレッドで呼ばれる。
/// `setup` がエラーを返したら、その診断を標準エラーに書き、初期化コードを実行せずにその終了コードを返す。
pub fn run_program_with(
    prog: &Program,
    opts: &RunOptions,
    setup: &mut (dyn FnMut(&mut Interp<'_>) -> Result<(), Stop> + Send),
) -> u8 {
    run_program_inner(prog, opts, Some(setup))
}

/// 新しいスレッドを作らず、現在のスレッドで実行する(テスト用)。
///
/// `margin` はスタック残量の検査の余裕(バイト)である。
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
    let mut last = None;
    for native_stack in exec_stack_sizes(opts.native_stack, opts.fit_stack) {
        let size = usize::try_from(native_stack).unwrap_or(usize::MAX);
        let o = RunOptions {
            native_stack,
            ..opts.clone()
        };
        let setup = &mut setup;
        let r = std::thread::scope(|s| {
            let h = std::thread::Builder::new()
                .name("keleut-mutator".to_string())
                .stack_size(size)
                .spawn_scoped(s, move || run_on_thread(prog, &o, setup.take()));
            match h {
                Ok(h) => Ok(match h.join() {
                    Ok(code) => code,
                    Err(_) => {
                        eprintln!("keleut: 内部エラー: 実行のスレッドが落ちました");
                        70
                    }
                }),
                Err(e) => Err(e),
            }
        });
        match r {
            Ok(code) => return code,
            Err(e) => last = Some((size, e)),
        }
    }
    if let Some((size, e)) = last
        && keleut_rt::alloc::address_space_limit().is_none()
    {
        // 仮想メモリの上限がないのに作れないときは、処理系の内部エラーとして扱う(`--max-depth` が大きすぎてスタックを予約できない場合など)。
        eprintln!("keleut: 内部エラー: 実行のスレッドを作れません({size} バイトのスタック): {e}");
        return 70;
    }
    // 仮想メモリを制限しているときに、最も小さいスタックでもスレッドを作れなかった。
    // 評価のための領域を用意できないので、初期化を始めるときに起きたメモリ不足の実行時エラーとして終了する(LangSpec §15.5 の表の実行時エラーの行)。
    let stop = Stop::OutOfMemory;
    keleut_rt::out::Out::default().eprint(&stop.message());
    stop.exit_code()
}
