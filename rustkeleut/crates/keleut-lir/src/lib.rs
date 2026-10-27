//! 共有の下ろし(KIR → LIR)と LIR の検証器。
//!
//! 計画 261024-1 の 6.4 節(D524)の LIR を作る。
//! LIR は関数ごとの基本ブロックのグラフで、仮想レジスタは Rep を持ち、どの関数の制御も非巡回である。
//! 命令は trap せず、未定義の動作を持たない。
//! 各命令の意味は `rustkeleut/doc/lir.md` に定める。
//!
//! | 入口 | 働き |
//! |---|---|
//! | [`lower`] | 検証済みの KIR を LIR にし(パス 1〜12)、各段の後で LIR を検証する |
//! | [`lower_opts`] | yield の刈り込み(パス 11)を外せる [`lower`] |
//! | [`lower_unassigned`] | パス 11 の前の LIR を作って検証する |
//! | [`assign_frames`] | パス 11(yield の刈り込み)と 12(生存区間、再開点、スロット)を行う |
//! | [`verify()`] | LIR を検証する |
//! | [`Extender`] | 単位を後ろに足していくプログラムを、足した単位だけ下ろして LIR の後ろに足す(`keleut repl`) |
//! | [`print()`] | LIR を印字する |
//!
//! エフェクトの下ろし(パス 8)は [`effect`] にある。
//! `handle`、利用者の操作の `perform`、`resume` を、すべての後段が共有するハンドラのフレームの関数
//! `$rt_handle` とエフェクトの命令に下ろす(`doc/lir.md` の 16 節)。
//! プレリュードの `Console.write`(`runtime` の印の付いた操作)の `perform` は、実行時の直の呼び出し
//! `console_write` に下ろす。

mod bits;
mod builder;
mod depth;
mod dict;
pub mod effect;
mod frame;
mod initck;
pub mod ir;
mod live;
mod lower;
mod numeric;
mod pattern;
mod print;
mod program;
mod record;
pub mod verify;
mod wrappers;

pub use ir::*;
pub use print::print;
pub use record::KNOWN_RECORD_FIELDS;

use std::fmt;

/// 誤りの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// 下ろしの途中で、検証済みの KIR では起きないはずの形に出会った。
    Lower,
    /// 下ろした LIR が検証に通らない(下ろしの不具合)。
    Verify,
    /// 処理のスレッドを作れないか、処理の途中で落ちた。
    Internal,
}

/// この crate の誤り。どれも処理系の内部の誤りで、終了状態は 70 である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Error {
        Error {
            kind,
            message: message.into(),
        }
    }

    /// 種類。
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// 文面。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 終了状態(70)。
    pub fn exit_code(&self) -> i32 {
        70
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            ErrorKind::Lower => "LIR への下ろしに失敗しました",
            ErrorKind::Verify => "LIR の検証に失敗しました",
            ErrorKind::Internal => "LIR の処理の中で内部の誤りが起きました",
        };
        write!(f, "{what}: {}", self.message)
    }
}

impl std::error::Error for Error {}

/// 下ろしの再帰の 1 段(KIR のノードの深さ 1 つ)あたりに見込むスタックのバイト数。
///
/// `tests/deep.rs` が、深さ `N_nest` の鎖をこの見積もりのスタックで下ろせることを確かめる。
pub const STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) { 8192 } else { 2048 };

/// 深さ `max_nest` の KIR を下ろすのに使うスタックの大きさ。
pub fn stack_for(max_nest: usize) -> usize {
    max_nest
        .saturating_mul(STACK_BYTES_PER_LEVEL)
        .saturating_add(16 << 20)
}

/// `f` を大きさ `stack` のスタックを持つ専用のスレッドで走らせる。
fn on_stack<T: Send>(
    stack: usize,
    f: impl FnOnce() -> Result<T, Error> + Send,
) -> Result<T, Error> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("lir".to_string())
            .stack_size(stack)
            .spawn_scoped(s, f)
            .map_err(|e| Error::new(ErrorKind::Internal, format!("スレッドを作れません: {e}")))?;
        match h.join() {
            Ok(r) => r,
            Err(_) => Err(Error::new(ErrorKind::Internal, "下ろしが落ちました")),
        }
    })
}

/// パス 12 の前の LIR を作り、検証する。
///
/// `p` は検証済み(`kir::verify` か `kir::load` を通ったもの)でなければならない。
/// 再帰は KIR のノードの深さに比例するので、深さ `max_nest` に足りるスタックを持つ専用のスレッドで走らせる。
pub fn lower_unassigned(p: &kir::Program, max_nest: usize) -> Result<Program, Error> {
    let lp = on_stack(stack_for(max_nest), || {
        program::lower_program(p).map_err(|m| Error::new(ErrorKind::Lower, m))
    })?;
    verify(&lp)?;
    Ok(lp)
}

/// パス 11 と 12 を行い、検証する。
pub fn assign_frames(p: &mut Program) -> Result<(), Error> {
    assign_frames_with(p, true)
}

/// パス 11 と 12 を行い、検証する。`yield_prune` が偽なら yield を刈り込まない(`--no-yield-prune`)。
pub fn assign_frames_with(p: &mut Program, yield_prune: bool) -> Result<(), Error> {
    for f in &mut p.funcs {
        frame::assign(f, yield_prune);
    }
    verify(p)
}

/// 検証済みの KIR を LIR にする(パス 1〜12)。下ろしの後とパス 12 の後に検証する。
pub fn lower(p: &kir::Program) -> Result<Program, Error> {
    lower_with(p, kir::N_NEST)
}

/// 深さの上限を与えて [`lower`] を行う。
pub fn lower_with(p: &kir::Program, max_nest: usize) -> Result<Program, Error> {
    lower_opts(p, max_nest, true)
}

/// 深さの上限と yield の刈り込みの有無を与えて [`lower`] を行う。
pub fn lower_opts(p: &kir::Program, max_nest: usize, yield_prune: bool) -> Result<Program, Error> {
    let mut lp = lower_unassigned(p, max_nest)?;
    assign_frames_with(&mut lp, yield_prune)?;
    Ok(lp)
}

/// LIR を検証する。
pub fn verify(p: &Program) -> Result<(), Error> {
    verify::verify(p).map_err(|m| Error::new(ErrorKind::Verify, m))
}

/// 単位を後ろに足していく KIR のプログラムを、足した単位ごとに下ろす(`keleut repl`)。
///
/// 対話的な実行は、受理した入力ごとに KIR の単位を 1 つ後ろに足し、前の単位はそのまま実行時に残す。
/// [`Extender::extend`] は、前に下ろした範囲より後ろの KIR の項目だけを下ろし(パス 1〜12)、
/// LIR のプログラムの各表の後ろに足して、足した部分だけを検証する。
/// 前の関数、大域、形、文字列の添字は変わらないので、生きている実行時の値はそのまま使える。
/// 手間は足した単位の大きさに比例する。
///
/// 1 回で下ろしたプログラム([`lower`])との違いは、関数の並びだけである。
/// 足した単位の KIR の関数は、その前の単位の関数(KIR の関数と次の関数)の後ろに続く。
/// 空の [`Extender`] と [`Program::empty`] から全体を 1 回で足せば、[`lower_opts`] と同じ LIR になる。
#[derive(Default)]
pub struct Extender {
    st: program::State,
    verified: verify::Marks,
}

impl Extender {
    /// 何も下ろしていない状態。[`Program::empty`] と組にして使う。
    pub fn new() -> Extender {
        Extender::default()
    }

    /// KIR のプログラム `k` の、前に下ろした範囲より後ろの項目を下ろし、`lp` の後ろに足す。
    ///
    /// `k` は前に渡したプログラムの表の後ろに項目を足したもので、足した項目まで検証済みでなければならない
    /// (`kir::verify_extension`)。
    /// `lp` は前の呼び出しが作ったプログラムで、`lp.shapes` の後ろには、実行時がその後に作った形を
    /// 実行時の番号の順に足しておく(実行時の形の表と番号を揃えるため。足した形の番号は、その後ろから振る)。
    /// 誤りはどれも処理系の内部の誤りで、誤りの後の `self` と `lp` は使えない。
    pub fn extend(
        &mut self,
        k: &kir::Program,
        lp: &mut Program,
        max_nest: usize,
        yield_prune: bool,
    ) -> Result<(), Error> {
        let st = &mut self.st;
        let lpr = &mut *lp;
        on_stack(stack_for(max_nest), || {
            program::extend_program(k, st, lpr).map_err(|m| Error::new(ErrorKind::Lower, m))
        })?;
        let m = self.verified;
        let check =
            |p: &Program| verify::verify_from(p, &m).map_err(|e| Error::new(ErrorKind::Verify, e));
        check(lp)?;
        for f in &mut lp.funcs[m.funcs..] {
            frame::assign(f, yield_prune);
        }
        check(lp)?;
        self.verified = verify::Marks::of(lp);
        Ok(())
    }
}

/// 関数 `f` について、パス 12 の生存解析([`Frame`] の再開点と安全点を求めるもの)と、
/// 検証器の定義の検査の手間(日誌と集合を読み書きした量)を返す。
///
/// 手間が関数の大きさに比例することを試験で確かめるためのものである。
#[doc(hidden)]
pub fn dataflow_work(f: &Func) -> (u64, u64) {
    (frame::points_counted(f).2, verify::check_defs(f).1)
}
