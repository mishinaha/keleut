//! ネイティブの後段(M91 の LLVM の JIT)とインタプリタの境目(計画 261024-1 の 6.8 節、6.9 節)。
//!
//! JIT の関数は `tailcc i64 @f(ptr %ctx)` の 1 つの型を持ち、戻り値は一様な語か番兵(`0x2`)である。
//! `ctx` は [`NativeCtx`] で、呼び出しの相手と、呼ばれた側のフレームの位置を運ぶ。
//! フレームは分割値スタックに置き、形はインタプリタと同じである(`keleut_rt::stack`)。
//!
//! ## 呼び出しの規約
//!
//! 呼ぶ側は、呼ばれた側のフレームを置いてから(JIT は [`krt_prep`]、インタプリタは自分で置く)、
//! [`NativeCtx`] の `callee`、`cur_pos`、`cur_fp`、`env` を書き、入口の表の `entry[callee]` を呼ぶ。
//! 末尾の呼び出しは、自分のフレームの位置に置き直して `musttail` で呼ぶ。
//! フレームの見出しの 3 番の語には、どの境目でも本当の呼び出し元のフレームの位置を書く。
//! そのため、回収と継続は、インタプリタと JIT がネイティブのスタックの上で入れ子になっていても、
//! 値スタックの呼び出し元の鎖だけをたどればよい。
//! 鎖が [`keleut_rt::stack::NATIVE_CALLER`] で終わるのは、Rust から入る [`Interp::run`] の起動(初期化の駆動と、構造的な `Eq` の入れ子)だけである。
//!
//! 入口の表は、JIT の関数を翻訳したものと、インタプリタに入るスタブ `interp_stub` を指す。
//! `interp_stub` は [`krt_interp_run`] を呼び、インタプリタの起動を始める。
//! 起動の最下段のフレームが JIT の関数を末尾で呼ぶと、起動は [`TAILREQ`] を返し、スタブがその関数を `musttail` で呼ぶ。
//! インタプリタが JIT の関数を末尾でなく呼ぶときは、スタブ `enter_tailcc`(ccc)を通して普通に呼ぶ。
//!
//! ## 番兵と致命的な誤り
//!
//! JIT の関数は `handle` と `resume` と `perform` を含まない(それらの関数はインタプリタに残す。[`jit_eligible`])。
//! そのため JIT のフレームは番兵を受け取らず、番兵を受け取ったら呼び出し元の深さに書き戻して自分も番兵で戻る([`krt_sentinel`])。
//! `YIELD` のときは、フレームは値スタックに残り、継続に写される。
//! 写し戻した継続はインタプリタが続ける(脱最適化)。
//! そのため JIT は、末尾でない呼び出しの前に、生きているレジスタをスロットに書き、見出しに再開点を書く。
//!
//! 巻き戻さずに止まる誤り(未実装、内部の誤り、防壁の flush の失敗)は、[`Native::fatal`] に置いて番兵で戻る。
//! インタプリタはネイティブの呼び出しから戻るたびにこれを確かめ、Rust の `Err` にして止まる。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use keleut_lir::{ClassId, Func, FuncId, Inst, LabelId, Program, ShapeId, Term, TypeId};
use keleut_rt::stack::HEADER_WORDS;
use keleut_rt::value::Kind;
use keleut_rt::{CalleeView, DictView, Stop, Word};

use crate::interp::{Interp, PHASE, runtime_error, unimpl_prim};

/// 起動の最下段のフレームが JIT の関数を末尾で呼んだことを表す、[`krt_interp_run`] の戻り値(一様な語にも番兵にもならない語)。
pub const TAILREQ: Word = Word(0x4);

/// 引数の受け渡しの領域の語の数。これより多い引数を渡す関数は JIT にしない。
pub const ARG_WORDS: usize = 1024;

/// JIT のコードと実行時の関数が共有する文脈。JIT は欄を [`off`] の位置で読み書きする。
#[repr(C)]
pub struct NativeCtx {
    /// インタプリタ(`*mut Interp`)。
    pub interp: *mut std::ffi::c_void,
    /// 呼ぶ関数の番号。
    pub callee: u64,
    /// 呼ぶ関数の環境(クロージャ。無ければ 0)。
    pub env: u64,
    /// 呼ばれた側のフレームの論理的な位置。
    pub cur_pos: u64,
    /// 呼ばれた側のフレームの見出しへのポインタ。
    pub cur_fp: *mut u64,
    /// 文脈の深さ `ctx.depth` の場所。
    pub depth: *mut u64,
    /// 今の限度の場所。
    pub limit: *mut u64,
    /// ネイティブのスタックの下限の番地(予備の検査。0 なら検査しない)。
    pub stack_low: u64,
    /// 引数の受け渡しの領域([`ARG_WORDS`] 語)。
    pub args: *mut u64,
    /// 実行時の関数が返す値([`krt_dispatch`] の [`dispatch::VALUE`])。
    pub result: u64,
    /// 大域の値の列。
    pub globals: *mut u64,
    /// 大域の初期化済みの印の列(1 バイトずつ)。
    pub global_init: *mut u8,
    /// 文字列の定数の箱の列。
    pub strings: *const u64,
    /// ネイティブのスタックの、JIT の関数を走らせる下限の番地([`SOFT_MARGIN`]。0 なら検査しない)。
    pub stack_soft: u64,
    /// `interp_stub` の番地。
    pub stub: u64,
    /// 真なら、[`krt_interp_run`] は関数を翻訳せずにインタプリタで走らせる(スタックが下限に近い)。
    pub force_interp: u64,
}

/// [`NativeCtx`] の欄のバイトの位置。
#[allow(missing_docs)]
pub mod off {
    pub const CALLEE: u64 = 8;
    pub const ENV: u64 = 16;
    pub const CUR_POS: u64 = 24;
    pub const CUR_FP: u64 = 32;
    pub const DEPTH: u64 = 40;
    pub const LIMIT: u64 = 48;
    pub const STACK_LOW: u64 = 56;
    pub const ARGS: u64 = 64;
    pub const RESULT: u64 = 72;
    pub const GLOBALS: u64 = 80;
    pub const GLOBAL_INIT: u64 = 88;
    pub const STRINGS: u64 = 96;
    pub const STACK_SOFT: u64 = 104;
    pub const STUB: u64 = 112;
    pub const FORCE_INTERP: u64 = 120;
}

/// ネイティブのスタックの残りがこの量(バイト)を予備の検査の余裕に足したものより少なくなったら、
/// 関数を JIT のコードでなくインタプリタで走らせる。
///
/// JIT の関数の末尾でない再帰はネイティブのスタックを使うが、インタプリタの中の呼び出しは使わない。
/// 小さな `--native-stack` でも、論理的な深さの限度まで再帰できるようにする(計画 6.11 節の予備の検査に先に届かない)。
/// JIT の関数の入口がこの下限を下回ったら、置いたフレームのままインタプリタの起動に移る(入口での脱最適化)。
pub const SOFT_MARGIN: u64 = 64 * 1024;

/// [`krt_dispatch`] の操作と結果。
pub mod dispatch {
    /// 値を呼ぶ(`call_value`)。
    pub const OP_VALUE: u64 = 0;
    /// 辞書からメソッドを選んで呼ぶ(`call_method`)。
    pub const OP_METHOD: u64 = 1;
    /// 辞書を渡す(`dict_app`)。
    pub const OP_DICTAPP: u64 = 2;
    /// 末尾の呼び出しの印。
    pub const TAIL: u64 = 8;
    /// 呼ぶ関数のフレームを置いた。`entry[ctx.callee]` を呼ぶ。
    pub const CALL: i64 = 0;
    /// 実装の関数のフレームを置いた(末尾でなく呼ぶ)。戻った値を `OP_VALUE` でもう一度渡す。
    pub const PHASE: i64 = 1;
    /// 値が `ctx.result` にある(構造的な `Eq` の結果、`extern` への `dict_app`)。
    pub const VALUE: i64 = 2;
    /// 状態語を立てた。番兵で戻る。
    pub const ERR: i64 = 3;
}

/// Rust から tailcc の関数を呼ぶスタブ `enter_tailcc` の型。
pub type EnterFn = unsafe extern "C" fn(*mut NativeCtx, usize) -> u64;

/// 関数を翻訳して入口の番地を返す関数。
pub type CompileFn = Box<dyn FnMut(FuncId) -> Result<usize, String>>;

/// 関数の段の状態。
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// まだ決めていない。
    Unknown,
    /// インタプリタで走らせる。
    Interp,
    /// 翻訳した(番地は入口の表にある)。
    Compiled,
}

/// ネイティブの後段の状態(インタプリタが持つ)。
pub struct Native {
    /// 文脈。
    pub ctx: NativeCtx,
    /// 入口の表(関数の番号ごとの番地)。JIT のコードはこの表の番地を定数として持つ。
    pub entry: Box<[AtomicUsize]>,
    enter: EnterFn,
    state: Vec<State>,
    eligible: Vec<bool>,
    /// 段の設定が `jit-only`(使える関数をすべて翻訳する)か。偽なら翻訳しない(`interp-only`)。
    pub jit_only: bool,
    compile: CompileFn,
    frames: Vec<(usize, u64)>,
    /// 巻き戻さずに止まる誤り。
    pub fatal: Option<Stop>,
    /// 引数の受け渡しの領域(`ctx.args` が指す)。
    _args: Box<[u64]>,
    /// 翻訳した関数の数。
    pub compiled: usize,
    /// JIT の関数の入口がスタックの下限に近く、インタプリタに移った回数。
    pub low_stack_entries: u64,
    /// 実行時の関数とネイティブの呼び出しが見たネイティブのスタックの最も深い番地(計測用)。
    pub min_sp: usize,
    /// 同じ時点で見た文脈の深さの最大(計測用)。
    pub max_depth_seen: u64,
    /// 最初のネイティブの呼び出しの直前のスタックの番地(計測用)。
    pub sp0: usize,
}

/// 直近の実行の [`Native::low_stack_entries`](試験用)。
pub static LAST_LOW_STACK_ENTRIES: AtomicU64 = AtomicU64::new(0);
/// 直近の実行の [`Native::compiled`](試験用)。
pub static LAST_COMPILED: AtomicU64 = AtomicU64::new(0);

/// 関数を JIT にできるか。
///
/// エフェクトの命令とブロックの終わり(`handle`、`resume`、`perform` の下ろし)を含む関数はインタプリタに残す(計画 9 節の M91)。
/// 引数の受け渡しの領域に入らない呼び出しを含む関数も残す。
pub fn jit_eligible(f: &Func) -> bool {
    if f.frame.is_none() || f.params.len() > ARG_WORDS {
        return false;
    }
    for b in &f.blocks {
        for i in &b.insts {
            match i {
                Inst::Fx { .. } => return false,
                Inst::CallMethod { own, args, .. } if own.len() + args.len() > ARG_WORDS => {
                    return false;
                }
                Inst::CtorNew { fields, .. } | Inst::RecNew { fields, .. }
                    if fields.len() > ARG_WORDS =>
                {
                    return false;
                }
                Inst::ClosureNew { caps: v, .. }
                | Inst::MethodNew { own: v, .. }
                | Inst::DictInst { premises: v, .. }
                | Inst::CallValue { args: v, .. }
                | Inst::CallDirect { args: v, .. }
                | Inst::DictApp { dicts: v, .. }
                    if v.len() > ARG_WORDS =>
                {
                    return false;
                }
                Inst::DictRecord { fields, .. } | Inst::DictVariant { fields, .. }
                    if 2 * fields.len() > ARG_WORDS =>
                {
                    return false;
                }
                _ => {}
            }
        }
        match &b.term {
            Term::Fx { .. } => return false,
            Term::TailCallMethod { own, args, .. } if own.len() + args.len() > ARG_WORDS => {
                return false;
            }
            Term::TailCallDirect { args, .. } | Term::TailCallValue { args, .. }
                if args.len() > ARG_WORDS =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

impl Drop for Native {
    /// 環境変数 `KELEUT_JIT_STATS` があれば、翻訳した関数の数を標準エラーに書く。
    fn drop(&mut self) {
        LAST_LOW_STACK_ENTRIES.store(self.low_stack_entries, Ordering::Relaxed);
        LAST_COMPILED.store(self.compiled as u64, Ordering::Relaxed);
        if std::env::var_os("KELEUT_JIT_STATS").is_some() {
            let interp = self.state.iter().filter(|s| **s == State::Interp).count();
            let used = self.sp0.saturating_sub(self.min_sp);
            eprintln!(
                "keleut: jit: 翻訳した関数 {}、インタプリタで走らせた関数 {interp}、スタックの下限で移った入口 {}、\
                 ネイティブのスタック {used} バイト(文脈の深さ {})",
                self.compiled, self.low_stack_entries, self.max_depth_seen
            );
        }
    }
}

impl Native {
    /// プログラム `prog` のネイティブの後段の状態を作る。
    ///
    /// `entry` は関数の数の長さの入口の表で、どの項目もはじめは `stub`(`interp_stub` の番地)を指す。
    pub fn new(
        prog: &Program,
        entry: Box<[AtomicUsize]>,
        enter: EnterFn,
        jit_only: bool,
        compile: CompileFn,
    ) -> Native {
        let frames = prog
            .funcs
            .iter()
            .map(|f| match &f.frame {
                Some(fr) => (
                    HEADER_WORDS + (fr.any_slots + fr.raw_slots) as usize,
                    u64::from(fr.any_slots) | (u64::from(fr.raw_slots) << 32),
                ),
                None => (HEADER_WORDS, 0),
            })
            .collect();
        let mut args = vec![0u64; ARG_WORDS].into_boxed_slice();
        Native {
            ctx: NativeCtx {
                interp: std::ptr::null_mut(),
                callee: 0,
                env: 0,
                cur_pos: 0,
                cur_fp: std::ptr::null_mut(),
                depth: std::ptr::null_mut(),
                limit: std::ptr::null_mut(),
                stack_low: 0,
                args: args.as_mut_ptr(),
                result: 0,
                globals: std::ptr::null_mut(),
                global_init: std::ptr::null_mut(),
                strings: std::ptr::null(),
                stack_soft: 0,
                stub: entry
                    .first()
                    .map_or(0, |e| e.load(Ordering::Relaxed) as u64),
                force_interp: 0,
            },
            entry,
            enter,
            state: vec![State::Unknown; prog.funcs.len()],
            eligible: prog.funcs.iter().map(jit_eligible).collect(),
            jit_only,
            compile,
            frames,
            fatal: None,
            _args: args,
            compiled: 0,
            low_stack_entries: 0,
            min_sp: usize::MAX,
            max_depth_seen: 0,
            sp0: 0,
        }
    }
}

impl<'p> Interp<'p> {
    /// ネイティブの後段を付ける。
    pub fn set_native(&mut self, n: Box<Native>) {
        self.native = Some(n);
    }

    /// 関数 `f` を JIT で走らせるなら、入口の番地を返す(`jit-only` では初めて呼ぶときに翻訳する)。
    pub(crate) fn native_target(&mut self, f: FuncId) -> Result<Option<usize>, Stop> {
        let Some(n) = self.native.as_mut() else {
            return Ok(None);
        };
        if self.stack_low != 0 && crate::interp::approx_sp() < self.stack_low + SOFT_MARGIN as usize
        {
            // ネイティブのスタックが下限に近い。JIT の関数もインタプリタで走らせる([`SOFT_MARGIN`])。
            return Ok(None);
        }
        match n.state[f.index()] {
            State::Compiled => return Ok(Some(n.entry[f.index()].load(Ordering::Relaxed))),
            State::Interp => return Ok(None),
            State::Unknown => {}
        }
        if !n.jit_only || !n.eligible[f.index()] {
            n.state[f.index()] = State::Interp;
            return Ok(None);
        }
        let addr = (n.compile)(f).map_err(|e| {
            Stop::Internal(format!(
                "関数 {} を翻訳できません: {e}",
                self.prog.funcs[f.index()].sym
            ))
        })?;
        n.entry[f.index()].store(addr, Ordering::Release);
        n.state[f.index()] = State::Compiled;
        n.compiled += 1;
        Ok(Some(addr))
    }

    /// 文脈のポインタの欄を今の `self` から書き直す。
    ///
    /// ネイティブの呼び出しの直前に呼ぶ。
    /// JIT と実行時の関数は、この呼び出しの間、ここで書いたポインタを通してインタプリタと実行時を読み書きする。
    fn native_ctx(&mut self) -> *mut NativeCtx {
        let me = self as *mut Interp<'p> as *mut std::ffi::c_void;
        let low = self.stack_low as u64;
        let soft = if low == 0 { 0 } else { low + SOFT_MARGIN };
        let rt = &mut self.rt;
        let n = self.native.as_mut().expect("ネイティブの後段がある");
        if n.sp0 == 0 {
            n.sp0 = crate::interp::approx_sp();
        }
        n.ctx.interp = me;
        n.ctx.depth = &mut rt.depth;
        n.ctx.limit = &mut rt.limit;
        n.ctx.stack_low = low;
        n.ctx.stack_soft = soft;
        n.ctx.globals = rt.globals.as_mut_ptr().cast::<u64>();
        n.ctx.global_init = rt.global_init.as_mut_ptr().cast::<u8>();
        n.ctx.strings = rt.strings.as_ptr().cast::<u64>();
        &mut n.ctx
    }

    /// 置いたフレーム(位置 `pos`、見出し `fp`)の関数 `func` を、入口 `addr` からネイティブに呼ぶ。
    pub(crate) fn call_native(
        &mut self,
        addr: usize,
        func: FuncId,
        env: Word,
        pos: u64,
        fp: *mut u64,
    ) -> Result<Word, Stop> {
        sample(self);
        let ctx = self.native_ctx();
        // SAFETY: ctx は self.native の中の文脈で、ここで書いた欄は呼び出しの間生きている。
        let r = unsafe {
            (*ctx).callee = u64::from(func.0);
            (*ctx).cur_pos = pos;
            (*ctx).cur_fp = fp;
            (*ctx).env = env.0;
            let enter = self.native.as_ref().expect("ネイティブの後段がある").enter;
            enter(ctx, addr)
        };
        let n = self.native.as_mut().expect("ネイティブの後段がある");
        if let Some(f) = n.fatal.take() {
            return Err(f);
        }
        Ok(Word(r))
    }

    /// 起動の最下段から JIT の関数 `func`(フレームは位置 `pos` に置いた)を末尾で呼ぶ依頼を文脈に書く。
    pub(crate) fn native_request(&mut self, func: FuncId, env: Word, pos: u64, fp: *mut u64) {
        let n = self.native.as_mut().expect("ネイティブの後段がある");
        n.ctx.callee = u64::from(func.0);
        n.ctx.cur_pos = pos;
        n.ctx.cur_fp = fp;
        n.ctx.env = env.0;
    }

    /// [`Interp::run`] の起動の最下段が残した末尾呼び出しの依頼を、ネイティブに呼んで終える。
    pub(crate) fn call_native_request(&mut self) -> Result<Word, Stop> {
        let n = self.native.as_ref().expect("ネイティブの後段がある");
        let func = FuncId(n.ctx.callee as u32);
        let (env, pos, fp) = (Word(n.ctx.env), n.ctx.cur_pos, n.ctx.cur_fp);
        let addr = n.entry[func.index()].load(Ordering::Acquire);
        self.call_native(addr, func, env, pos, fp)
    }
}

// ---- 実行時の関数(JIT のコードが呼ぶ) ----

/// 実行時の関数の本体 `f` を走らせ、panic を JIT のフレームの手前で止める。
///
/// JIT のフレームは巻き戻しの表を持たず、`extern "C"` の関数から panic が出ると Rust はプロセスを abort する。
/// そこで panic を捕まえ、標準出力のバッファを吐き出し、インタプリタの後段で実行のスレッドが落ちたときと同じ診断を出して 70 で終わる。
/// panic の後のインタプリタの状態は信用できないので、JIT のコードには戻らない。
fn guarded<R>(ctx: *mut NativeCtx, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // SAFETY: ctx は呼び出しの最中の文脈で、この後 JIT のコードには戻らない。
                let it = unsafe { interp(ctx) };
                let _ = it.rt.out.flush();
            }));
            eprintln!("keleut: 内部エラー: 実行のスレッドが落ちました");
            std::process::exit(70)
        }
    }
}

/// 文脈からインタプリタを取り出す。
///
/// # Safety
///
/// `ctx` は [`Interp::native_ctx`] が書いた文脈で、ネイティブの呼び出しの最中である。
unsafe fn interp<'a>(ctx: *mut NativeCtx) -> &'a mut Interp<'static> {
    // SAFETY: 呼び出し側の約束による。インタプリタは呼び出しの間動かない。
    unsafe { &mut *((*ctx).interp as *mut Interp<'static>) }
}

fn native<'a>(it: &'a mut Interp<'_>) -> &'a mut Native {
    it.native.as_mut().expect("ネイティブの後段がある")
}

/// ネイティブのスタックの番地と文脈の深さを記録する(計測用。`KELEUT_JIT_STATS`)。
#[inline]
fn sample(it: &mut Interp<'_>) {
    let sp = crate::interp::approx_sp();
    let depth = it.rt.depth;
    let n = native(it);
    n.min_sp = n.min_sp.min(sp);
    n.max_depth_seen = n.max_depth_seen.max(depth);
}

/// 致命的な誤りを置く(最初のものを残す)。
fn set_fatal(it: &mut Interp<'_>, s: Stop) {
    let n = native(it);
    if n.fatal.is_none() {
        n.fatal = Some(s);
    }
}

/// 誤りを状態語にする。巻き戻さずに止まる誤りなら致命的な誤りにする。
fn fail(it: &mut Interp<'_>, s: Stop) {
    if let Err(f) = it.rt.raise(s) {
        set_fatal(it, f);
    }
}

fn bug(m: &str) -> Stop {
    Stop::Internal(m.to_string())
}

/// フレームを位置 `at` に置き、見出しを書く。呼び出し元の今のフレームは位置 `me` にある。
fn place(
    it: &mut Interp<'_>,
    ctx: *mut NativeCtx,
    fid: u64,
    me: u64,
    at: u64,
    caller: u64,
    env: u64,
) -> Option<*mut u64> {
    it.rt.stack.ptr_at(me);
    let (fw, sizes) = native(it).frames[fid as usize];
    match it.rt.stack.place(at, fw) {
        Ok(pos) => {
            let p = it.rt.stack.ptr_at(pos);
            // SAFETY: p からフレームの語の数だけ塊の中にあり、ctx は呼び出しの最中の文脈である。
            unsafe {
                *p = fid;
                *p.add(1) = 0;
                *p.add(2) = sizes;
                *p.add(3) = caller;
                (*ctx).callee = fid;
                (*ctx).cur_pos = pos;
                (*ctx).cur_fp = p;
                (*ctx).env = env;
            }
            Some(p)
        }
        Err(_) => {
            fail(it, Stop::OutOfMemory);
            None
        }
    }
}

/// JIT の関数が関数 `fid` を呼ぶ前に、そのフレームを位置 `at` に置く(見出しの 3 番の語は `caller`)。
///
/// 呼ぶ側の今のフレームは位置 `me` にある。
/// 置いたフレームの見出しへのポインタを返し、文脈の `callee`、`cur_pos`、`cur_fp`、`env` を書く。
/// 値スタックを広げられなければメモリの不足を状態語にして空のポインタを返す(呼ぶ側の命令の失敗)。
///
/// # Safety
///
/// JIT のコードから、ネイティブの呼び出しの最中に呼ぶ。
pub unsafe extern "C" fn krt_prep(
    ctx: *mut NativeCtx,
    fid: u64,
    me: u64,
    at: u64,
    caller: u64,
    env: u64,
) -> *mut u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        sample(it);
        place(it, ctx, fid, me, at, caller, env).unwrap_or(std::ptr::null_mut())
    })
}

/// JIT のフレーム(見出しは `fp`)の末尾でない呼び出しが番兵で戻った。
///
/// 深さをこのフレームの深さに書き戻し、状態語がこのフレームに届いてよいものかを確かめる
/// (`yields` が 0 の再開点に `YIELD` が届いたら内部の誤り。`doc/lir.md` の 16.1 節、17 節)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_sentinel(ctx: *mut NativeCtx, fp: *mut u64, yields: u64) {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        if native(it).fatal.is_some() {
            return;
        }
        // SAFETY: fp はこのフレームの見出しを指す。
        it.rt.depth = unsafe { *fp.add(1) };
        let e = match &it.rt.pending {
            keleut_rt::effect::Pending::Yield { .. } if yields == 0 => Some(bug(
                "yield が noyield の再開点に届きました(may_yield の印の誤り)",
            )),
            keleut_rt::effect::Pending::Matched(_)
            | keleut_rt::effect::Pending::ClauseValue(_)
            | keleut_rt::effect::Pending::TailResume(_) => {
                Some(bug("節の印がふつうの呼び出しに届きました"))
            }
            _ => None,
        };
        if let Some(e) = e {
            set_fatal(it, e);
        }
    })
}

/// `interp_stub` から入るインタプリタの起動。
///
/// フレームは呼び出し側が置き、文脈の `callee`、`cur_pos`、`cur_fp`、`env` に書いてある。
/// 関数が JIT にできるものなら(`jit-only`)ここで翻訳し、[`TAILREQ`] を返してスタブに呼ばせる。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_interp_run(ctx: *mut NativeCtx) -> u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        // SAFETY: ctx は生きている文脈である。
        let (func, env, pos, fp) = unsafe {
            (
                FuncId((*ctx).callee as u32),
                Word((*ctx).env),
                (*ctx).cur_pos,
                (*ctx).cur_fp,
            )
        };
        // SAFETY: ctx は生きている文脈である。
        let forced = unsafe { std::mem::replace(&mut (*ctx).force_interp, 0) } != 0;
        if forced {
            native(it).low_stack_entries += 1;
        } else {
            match it.native_target(func) {
                Ok(Some(_)) => return TAILREQ.0,
                Ok(None) => {}
                Err(e) => {
                    set_fatal(it, e);
                    return Word::SENTINEL.0;
                }
            }
        }
        let r = it.run_activation(func, env, pos, fp);
        // 起動の中のネイティブの呼び出しが文脈のポインタを書き直したので、戻る前にこの起動のものに戻す必要は無い
        // (どの欄も同じ Interp と Rt を指す)。
        match r {
            Ok(v) => v.0,
            Err(e) => {
                set_fatal(it, e);
                Word::SENTINEL.0
            }
        }
    })
}

/// 値を呼ぶ相手を決める([`krt_dispatch`] の中)。
struct Disp<'a, 'p> {
    it: &'a mut Interp<'p>,
    ctx: *mut NativeCtx,
    me: u64,
    row: u32,
    tail: bool,
    args: Vec<Word>,
}

impl Disp<'_, '_> {
    /// 自分のフレームの見出し、関数、フレームの語の数、呼び出し元。
    fn frame(&mut self) -> (*mut u64, u64, u64) {
        let fp = self.it.rt.stack.ptr_at(self.me);
        // SAFETY: fp は今のフレームの見出しを指す。
        let (fid, w3) = unsafe { (*fp & 0xffff_ffff, *fp.add(3)) };
        let fw = native(self.it).frames[fid as usize].0 as u64;
        (fp, self.me + fw, w3)
    }

    fn set_row(&mut self, row: u32) {
        let (fp, _, _) = self.frame();
        // SAFETY: fp は今のフレームの見出しを指す。
        unsafe { *fp = (*fp & 0xffff_ffff) | (u64::from(row) << 32) };
    }

    /// 関数 `func` を環境 `env` と引数で呼ぶフレームを置く。
    fn call(&mut self, func: FuncId, env: Word) -> i64 {
        let (_, top, w3) = self.frame();
        let (at, caller) = if self.tail {
            (self.me, w3)
        } else {
            (top, self.me)
        };
        let Some(p) = place(
            self.it,
            self.ctx,
            u64::from(func.0),
            self.me,
            at,
            caller,
            env.0,
        ) else {
            return dispatch::ERR;
        };
        for (i, a) in self.args.iter().enumerate() {
            // SAFETY: 置いたフレームは関数の引数の数以上のスロットを持つ。
            unsafe { *p.add(HEADER_WORDS + i) = a.0 };
        }
        dispatch::CALL
    }

    fn err(&mut self, s: Stop) -> i64 {
        fail(self.it, s);
        dispatch::ERR
    }

    fn value(&mut self, w: Word) -> i64 {
        // SAFETY: ctx は生きている文脈である。
        unsafe { (*self.ctx).result = w.0 };
        dispatch::VALUE
    }

    fn callee_value(&mut self, f: Word) -> i64 {
        match self.it.rt.callee_view(f) {
            CalleeView::Closure(func) => self.call(func, f),
            CalleeView::ExternUnimpl(g) => {
                let s = unimpl_prim(self.it.prog, g);
                self.err(s)
            }
            CalleeView::Method => {
                let (c, m, d, own) = self.it.rt.method_parts(f);
                self.select(c, m, d, &own)
            }
            CalleeView::Other => {
                let s = Stop::Internal(format!(
                    "呼べない値を呼びました: {}",
                    String::from_utf8_lossy(&self.it.rt.show(f))
                ));
                set_fatal(self.it, s);
                dispatch::ERR
            }
        }
    }

    fn select(&mut self, class: ClassId, m: u32, d: Word, own: &[Word]) -> i64 {
        match self.it.rt.dict_view(d) {
            DictView::Builtin(c, t) => match self.it.builtin_method(c, t, m) {
                Ok(body) => self.call(body, Word(0)),
                Err(s) => {
                    set_fatal(self.it, s);
                    dispatch::ERR
                }
            },
            DictView::Inst(i, premises) => {
                if let Err(s) = self.it.check_inst(i) {
                    return self.err(s);
                }
                let prog = self.it.prog;
                let im = &prog.instances[i.index()].methods[m as usize];
                if im.dicts == 0 {
                    let v = self.it.rt.inst_values[i.index()][m as usize];
                    return self.callee_value(v);
                }
                let func = im.func;
                let mut dicts = premises;
                dicts.extend_from_slice(own);
                // 実装の関数を末尾でなく評価する。その間、見出しの再開点に印 PHASE を付け、
                // 呼び出しの命令の被演算子(JIT がスロットに書いた)を根にする(`doc/lir.md` の 1.8 節)。
                self.set_row(self.row | PHASE);
                let (_, top, _) = self.frame();
                let Some(p) = place(
                    self.it,
                    self.ctx,
                    u64::from(func.0),
                    self.me,
                    top,
                    self.me,
                    0,
                ) else {
                    return dispatch::ERR;
                };
                for (i, a) in dicts.iter().enumerate() {
                    // SAFETY: 置いたフレームは関数の引数の数以上のスロットを持つ。
                    unsafe { *p.add(HEADER_WORDS + i) = a.0 };
                }
                dispatch::PHASE
            }
            DictView::Record(_) | DictView::Variant(_) => {
                if self.args.len() != 2 {
                    set_fatal(self.it, bug("構造的な Eq の引数の数が 2 でありません"));
                    return dispatch::ERR;
                }
                let (a, b) = (self.args[0], self.args[1]);
                // 比べ方は確保し、利用者のコードを呼ぶ。見出しの再開点は呼び出しのもの(set_row 済み)。
                self.it.rt.gc.at_frame(self.me);
                let (_, top, _) = self.frame();
                match self.it.eq_struct(class, d, a, b, top) {
                    Ok(r) => self.value(Word::bool(r)),
                    Err(s) => self.err(s),
                }
            }
            DictView::Other => {
                let s = Stop::Internal(format!(
                    "辞書でない値からメソッドを選びました: {}",
                    String::from_utf8_lossy(&self.it.rt.show(d))
                ));
                set_fatal(self.it, s);
                dispatch::ERR
            }
        }
    }
}

/// JIT の関数の、相手を実行時に決める呼び出し(`call_value`、`call_method`、`dict_app` と末尾の形)。
///
/// `op` は [`dispatch`] の操作(末尾なら [`dispatch::TAIL`] を足す)、`me` は今のフレームの位置、
/// `row` は見出しに書く再開点(末尾の形では末尾の形の行)、`x` は呼ぶ値か辞書、
/// `cm` は `クラス << 32 | メソッド`、引数 `nargs` 個とメソッド自身の辞書 `nown` 個は文脈の引数の領域に並べてある。
/// 結果は [`dispatch`] の `CALL`、`PHASE`、`VALUE`、`ERR` である。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn krt_dispatch(
    ctx: *mut NativeCtx,
    op: u64,
    me: u64,
    row: u64,
    x: u64,
    cm: u64,
    nown: u64,
    nargs: u64,
) -> i64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        sample(it);
        let (nargs, nown) = (nargs as usize, nown as usize);
        // SAFETY: 引数の領域は ARG_WORDS 語で、JIT は nargs + nown 語を書いた。
        let (args, own): (Vec<Word>, Vec<Word>) = unsafe {
            let p = (*ctx).args;
            (
                (0..nargs).map(|i| Word(*p.add(i))).collect(),
                (0..nown).map(|i| Word(*p.add(nargs + i))).collect(),
            )
        };
        let mut d = Disp {
            it,
            ctx,
            me,
            row: row as u32,
            tail: op & dispatch::TAIL != 0,
            args,
        };
        d.set_row(row as u32);
        let x = Word(x);
        match op & 7 {
            dispatch::OP_VALUE => d.callee_value(x),
            dispatch::OP_METHOD => d.select(ClassId((cm >> 32) as u32), cm as u32, x, &own),
            _ => {
                if d.it.rt.is_extern_value(x) {
                    return d.value(x);
                }
                match d.it.rt.callee_view(x) {
                    CalleeView::Closure(func) => d.call(func, x),
                    _ => {
                        set_fatal(d.it, bug("辞書を受け取らない値に辞書を渡しました"));
                        dispatch::ERR
                    }
                }
            }
        }
    })
}

/// 深さの限度を超えた(`depth_check`、`depth_push`)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_depth_error(ctx: *mut NativeCtx) {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let s = it.rt.depth_error();
        fail(it, s);
    })
}

/// 関数 `fid` のブロック `blk` の終わり `error` の実行時エラーを起こす。`opnd` は被演算子の語。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_error(ctx: *mut NativeCtx, fid: u64, blk: u64, opnd: u64) {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let prog = it.prog;
        let s = match &prog.funcs[fid as usize].blocks[blk as usize].term {
            Term::Error { kind, .. } => runtime_error(prog, &it.rt, kind, opnd),
            _ => bug("error でないブロックの終わり"),
        };
        fail(it, s);
    })
}

/// 未実装の機能に届いた(`unimplemented`、`unimplemented_value`)。`inst` が `u64::MAX` ならブロックの終わり。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_unimpl(ctx: *mut NativeCtx, fid: u64, blk: u64, inst: u64) {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let b = &it.prog.funcs[fid as usize].blocks[blk as usize];
        let what = if inst == u64::MAX {
            match &b.term {
                Term::Unimplemented { what } => what.clone(),
                _ => String::new(),
            }
        } else {
            match &b.insts[inst as usize] {
                Inst::UnimplementedValue { what, .. } => what.clone(),
                _ => String::new(),
            }
        };
        set_fatal(it, Stop::Unimplemented(what));
    })
}

/// 確保する実行時の関数の共通の形。安全点を置いてから `f` を呼び、失敗なら状態語にして 0 を返す。
fn alloc_with(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    f: impl FnOnce(&mut Interp<'static>, &[u64]) -> Result<Word, Stop>,
) -> u64 {
    guarded(ctx, || {
        // SAFETY: JIT のコードから、ネイティブの呼び出しの最中に呼ばれる。
        let it = unsafe { interp(ctx) };
        it.rt.gc.at_safepoint(me, sp as u32);
        // SAFETY: 引数の領域は ARG_WORDS 語ある。
        let args = unsafe { std::slice::from_raw_parts((*ctx).args, ARG_WORDS) };
        match f(it, args) {
            Ok(w) => w.0,
            Err(s) => {
                fail(it, s);
                0
            }
        }
    })
}

/// `box`(`i64` は `kind` 0、`f64` は 1)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_box(ctx: *mut NativeCtx, me: u64, sp: u64, kind: u64, v: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        if kind == 0 {
            it.rt.box_i64(v as i64)
        } else {
            it.rt.box_f64(v)
        }
    })
}

/// `str.concat`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_str_concat(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    a: u64,
    b: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| it.rt.str_concat(Word(a), Word(b)))
}

/// `str.sub`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_str_sub(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    s: u64,
    pos: u64,
    len: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        it.rt.str_sub(Word(s), pos as i32, len as i32)
    })
}

/// `show.i32`(0)、`show.i64`(1)、`show.f64`(2)、`show.bool`(3)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_show(ctx: *mut NativeCtx, me: u64, sp: u64, op: u64, x: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        let s = match op {
            0 => (x as i32).to_string(),
            1 => (x as i64).to_string(),
            2 => keleut_rtcore::show_f64(f64::from_bits(x)),
            _ => (if x != 0 { "true" } else { "false" }).to_string(),
        };
        it.rt.new_str(s.as_bytes())
    })
}

fn words(a: &[u64], n: u64) -> Vec<Word> {
    a[..n as usize].iter().map(|&w| Word(w)).collect()
}

/// `ctor_new`(フィールドは引数の領域に `n` 個)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_ctor_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    ty: u64,
    ctor: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.ctor_new(TypeId(ty as u32), ctor as u32, &words(a, n))
    })
}

/// `variant_new`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_variant_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    label: u64,
    p: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        it.rt.variant_new(LabelId(label as u32), Word(p))
    })
}

/// `rec_new`(フィールドは引数の領域に `n` 個)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_rec_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    shape: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.rec_new(ShapeId(shape as u32), &words(a, n))
    })
}

/// `rec_extend`(0)、`rec_restrict`(1)、`rec_update`(2)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn krt_rec_op(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    op: u64,
    v: u64,
    label: u64,
    x: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        let l = LabelId(label as u32);
        match op {
            0 => it.rt.rec_extend(Word(v), l, Word(x)),
            1 => it.rt.rec_restrict(Word(v), l),
            _ => it.rt.rec_update(Word(v), l, Word(x)),
        }
    })
}

/// `closure_new`(捕獲は引数の領域に `n` 個)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_closure_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    func: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.closure_new(FuncId(func as u32), &words(a, n))
    })
}

/// `method_new`(メソッド自身の辞書は引数の領域に `n` 個)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn krt_method_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    class: u64,
    method: u64,
    dict: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.method_new(
            ClassId(class as u32),
            method as u32,
            Word(dict),
            &words(a, n),
        )
    })
}

/// `dict_inst`(前提の辞書は引数の領域に `n` 個)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_dict_inst(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    inst: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt
            .dict_inst(keleut_lir::InstanceId(inst as u32), &words(a, n))
    })
}

/// `dict_record`(`kind` 0)と `dict_variant`(1)。項目は引数の領域に(ラベル、辞書)の順で `n` 組。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_dict_struct(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    kind: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        let fs: Vec<(LabelId, Word)> = (0..n as usize)
            .map(|i| (LabelId(a[2 * i] as u32), Word(a[2 * i + 1])))
            .collect();
        let k = if kind == 0 {
            Kind::DictRecord
        } else {
            Kind::DictVariant
        };
        it.rt.dict_struct(k, &fs)
    })
}

/// `ref_new`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_ref_new(ctx: *mut NativeCtx, me: u64, sp: u64, x: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| it.rt.ref_new(Word(x)))
}

/// `marray_new`(`n` は `i32`)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_marray_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    n: u64,
    x: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        let n = (n as i32).max(0) as usize;
        it.rt.array_new(Kind::MArray, n, Word(x))
    })
}

/// `marray_freeze`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_marray_freeze(ctx: *mut NativeCtx, me: u64, sp: u64, a: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| it.rt.array_freeze(Word(a)))
}

/// `str.eq` などの比べ方(`op` は [`crate::bytecode::cmp_sub`] の細目)。真なら 1。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_str_cmp(ctx: *mut NativeCtx, op: u64, a: u64, b: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let o = keleut_rtcore::compare_bytes(it.rt.str_bytes(Word(a)), it.rt.str_bytes(Word(b)));
        use std::cmp::Ordering::*;
        u64::from(match op {
            0 => o == Equal,
            1 => o == Less,
            2 => o != Greater,
            3 => o == Greater,
            _ => o != Less,
        })
    })
}

/// `rec_field`(`has` が 0)と `rec_has`(1)。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_rec_field(
    ctx: *mut NativeCtx,
    has: u64,
    v: u64,
    label: u64,
    nth: u64,
) -> u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let l = LabelId(label as u32);
        if has == 0 {
            it.rt.rec_field(Word(v), l, nth as u32).0
        } else {
            u64::from(it.rt.rec_has(Word(v), l, nth as u32))
        }
    })
}

/// インスタンスの命令。`op` は 0 が `inst_is_init`、1 が `inst_mark_init`、2 が `inst_set_value`、3 が `inst_get_value`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_inst(ctx: *mut NativeCtx, op: u64, inst: u64, m: u64, x: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let (i, m) = (inst as usize, m as usize);
        match op {
            0 => u64::from(it.rt.inst_init[i]),
            1 => {
                it.rt.inst_init[i] = true;
                0
            }
            2 => {
                it.rt.inst_values[i][m] = Word(x);
                0
            }
            _ => it.rt.inst_values[i][m].0,
        }
    })
}

/// 配列の要素の読み書き。`op` は 0 が `array_get`(`marray_get`)、1 が `marray_set`。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_array(ctx: *mut NativeCtx, op: u64, a: u64, i: u64, x: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        if op == 0 {
            it.rt.array_get(Word(a), i as i32).0
        } else {
            it.rt.array_set(Word(a), i as i32, Word(x));
            0
        }
    })
}

/// `console_write`。書けたら 0、失敗なら状態語を立てて 1 を返す。
///
/// # Safety
///
/// [`krt_prep`] と同じ。
pub unsafe extern "C" fn krt_console_write(ctx: *mut NativeCtx, s: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 呼び出し側の約束による。
        let it = unsafe { interp(ctx) };
        let bytes = it.rt.str_bytes(Word(s));
        // SAFETY: 文字列の箱は動かず、書く間に回収は起きない。
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
        match it.rt.out.write(bytes) {
            Ok(()) => 0,
            Err(e) => {
                fail(it, Stop::from(e));
                1
            }
        }
    })
}

/// JIT のコードが呼ぶ実行時の関数の名前と番地(LLJIT の絶対の記号として登録する)。
pub fn helper_symbols() -> Vec<(&'static str, usize)> {
    vec![
        ("krt_prep", krt_prep as *const () as usize),
        ("krt_sentinel", krt_sentinel as *const () as usize),
        ("krt_interp_run", krt_interp_run as *const () as usize),
        ("krt_dispatch", krt_dispatch as *const () as usize),
        ("krt_depth_error", krt_depth_error as *const () as usize),
        ("krt_error", krt_error as *const () as usize),
        ("krt_unimpl", krt_unimpl as *const () as usize),
        ("krt_box", krt_box as *const () as usize),
        ("krt_str_concat", krt_str_concat as *const () as usize),
        ("krt_str_sub", krt_str_sub as *const () as usize),
        ("krt_show", krt_show as *const () as usize),
        ("krt_ctor_new", krt_ctor_new as *const () as usize),
        ("krt_variant_new", krt_variant_new as *const () as usize),
        ("krt_rec_new", krt_rec_new as *const () as usize),
        ("krt_rec_op", krt_rec_op as *const () as usize),
        ("krt_closure_new", krt_closure_new as *const () as usize),
        ("krt_method_new", krt_method_new as *const () as usize),
        ("krt_dict_inst", krt_dict_inst as *const () as usize),
        ("krt_dict_struct", krt_dict_struct as *const () as usize),
        ("krt_ref_new", krt_ref_new as *const () as usize),
        ("krt_marray_new", krt_marray_new as *const () as usize),
        ("krt_marray_freeze", krt_marray_freeze as *const () as usize),
        ("krt_str_cmp", krt_str_cmp as *const () as usize),
        ("krt_rec_field", krt_rec_field as *const () as usize),
        ("krt_inst", krt_inst as *const () as usize),
        ("krt_array", krt_array as *const () as usize),
        ("krt_console_write", krt_console_write as *const () as usize),
        ("krt_sin", keleut_rtcore::sys::krt_sin as *const () as usize),
        ("krt_cos", keleut_rtcore::sys::krt_cos as *const () as usize),
        ("krt_exp", keleut_rtcore::sys::krt_exp as *const () as usize),
        ("krt_log", keleut_rtcore::sys::krt_log as *const () as usize),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn 文脈の欄の位置は_jit_のものと同じ() {
        assert_eq!(offset_of!(NativeCtx, callee) as u64, off::CALLEE);
        assert_eq!(offset_of!(NativeCtx, env) as u64, off::ENV);
        assert_eq!(offset_of!(NativeCtx, cur_pos) as u64, off::CUR_POS);
        assert_eq!(offset_of!(NativeCtx, cur_fp) as u64, off::CUR_FP);
        assert_eq!(offset_of!(NativeCtx, depth) as u64, off::DEPTH);
        assert_eq!(offset_of!(NativeCtx, limit) as u64, off::LIMIT);
        assert_eq!(offset_of!(NativeCtx, stack_low) as u64, off::STACK_LOW);
        assert_eq!(offset_of!(NativeCtx, args) as u64, off::ARGS);
        assert_eq!(offset_of!(NativeCtx, result) as u64, off::RESULT);
        assert_eq!(offset_of!(NativeCtx, globals) as u64, off::GLOBALS);
        assert_eq!(offset_of!(NativeCtx, global_init) as u64, off::GLOBAL_INIT);
        assert_eq!(offset_of!(NativeCtx, strings) as u64, off::STRINGS);
        assert_eq!(offset_of!(NativeCtx, stack_soft) as u64, off::STACK_SOFT);
        assert_eq!(offset_of!(NativeCtx, stub) as u64, off::STUB);
        assert_eq!(
            offset_of!(NativeCtx, force_interp) as u64,
            off::FORCE_INTERP
        );
        assert_eq!(TAILREQ.0 & 7, 4);
    }
}
