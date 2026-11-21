//! KIR から LIR への変換(lowering)と、LIR の検証器。
//!
//! LIR は、インタプリタ、LLVM バックエンド、WasmGC バックエンドが共有する中間言語である。
//! LIR は関数ごとの基本ブロックのグラフで、仮想レジスタは Rep を持ち、どの関数の制御フローも非巡回である。
//! 命令は trap せず、未定義動作を持たない。
//! 各命令の意味は `rustkeleut/doc/lir.md`(以下 `doc/lir.md`)が定める。
//! パスの番号は、`doc/lir.md` の 2 節の表の番号である。
//!
//! | 公開の項目 | 動作 |
//! |---|---|
//! | [`lower`] | 検証済みの KIR を LIR に変換する(パス 1〜12)。パス 10 までを終えた後と、パス 12 の後に LIR を検証する |
//! | [`lower_opts`] | [`lower`] と同じ変換を行う。yield の枝刈り(パス 11)を行わない指定ができる |
//! | [`lower_unassigned`] | パス 11 の前の LIR を作って検証する |
//! | [`assign_frames`] | パス 11(yield の枝刈り)とパス 12(生存区間、再開点、スロット)を行う |
//! | [`verify()`] | LIR を検証する |
//! | [`Extender`] | 単位を後ろに追加していくプログラムについて、追加した単位だけを LIR に変換し、LIR の後ろに追加する(`keleut repl`) |
//! | [`print()`] | LIR をテキストにして返す |
//!
//! エフェクトを LIR に変換する処理(パス 8)は [`effect`] にある。
//! `handle`、ユーザの操作の `perform`、`resume` を、すべてのバックエンドが共有する関数 `$rt_handle`(ハンドラのフレームになる)とエフェクトの命令に変換する(`doc/lir.md` の 16 節)。
//! プレリュードの `Console.write`(`runtime` の印の付いた操作)の `perform` は、命令 `console_write` に変換する。

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

/// この crate が返すエラーの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// LIR への変換の途中で、検証済みの KIR には現れないはずの構造を見つけた。
    Lower,
    /// 変換した LIR が検証に通らない。
    /// 変換器のバグである。
    Verify,
    /// 仮想メモリを制限していないのに変換用のスレッドを作れないか、そのスレッドがパニックした。
    Internal,
    /// 仮想メモリを制限しているとき(`ulimit -v`)に、変換用のスレッドを作れない。
    /// 診断の種別は `処理系の限度` で、終了コードは 65 である。
    Limit,
}

/// この crate が返すエラー。
///
/// [`ErrorKind::Limit`] は診断の種別 `処理系の限度` のエラーで、終了コードは 65 である。
/// それ以外はどれも処理系の内部エラーで、終了コードは 70 である。
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

    /// エラーの種類を返す。
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// エラーの詳細を述べるメッセージを返す。
    /// `Display` がメッセージの前に付ける、種類ごとの文言(`LIR の検証に失敗しました` など)は含まない。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 終了コードを返す。
    /// [`ErrorKind::Limit`] は 65、それ以外は 70 である。
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::Limit => 65,
            _ => 70,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            ErrorKind::Lower => "LIR への下ろしに失敗しました",
            ErrorKind::Verify => "LIR の検証に失敗しました",
            ErrorKind::Internal => "LIR の処理の中で内部の誤りが起きました",
            ErrorKind::Limit => "処理系の限度",
        };
        write!(f, "{what}: {}", self.message)
    }
}

impl std::error::Error for Error {}

/// LIR への変換の再帰 1 段(KIR のノードの深さ 1 つ)あたりに見積もる、スタックのバイト数。
///
/// `tests/deep.rs` が、深さが入れ子の上限 `N_nest`(IRSpec 13.9 節。`kir::N_NEST`)の連鎖を、この見積もりのスタックで変換できることを確かめる。
pub const STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) { 8192 } else { 2048 };

/// 深さ `max_nest` の KIR を LIR に変換するのに使うスタックのバイト数を返す。
///
/// 1 段あたり [`STACK_BYTES_PER_LEVEL`] バイトに、16 MiB を加えた値である。
pub fn stack_for(max_nest: usize) -> usize {
    max_nest
        .saturating_mul(STACK_BYTES_PER_LEVEL)
        .saturating_add(16 << 20)
}

/// `stack` バイトのスタックを持つ専用のスレッドで `f` を実行する。
///
/// 仮想メモリを制限しているとき(`ulimit -v`)にスレッドを作れなければ、[`ErrorKind::Limit`] を返す。
/// 制限していないのに作れなければ、内部エラー([`ErrorKind::Internal`])を返す。
/// スレッドを作れなかったときに、スタックを小さくしてスレッドを作り直すことはしない。
/// `stack` は入れ子の上限まで再帰するのに必要なサイズなので、小さくすると変換の再帰の途中でスタックが足りなくなることがある。
/// 呼び出し元は、仮想メモリの制限に合わせて下げた入れ子の上限(`kir::Limits::fit_address_space`)を深さとして渡す。
fn on_stack<T: Send>(
    stack: usize,
    f: impl FnOnce() -> Result<T, Error> + Send,
) -> Result<T, Error> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("lir".to_string())
            .stack_size(stack)
            .spawn_scoped(s, f)
            .map_err(|e| {
                // 仮想メモリを制限しているときに作れなければ `処理系の限度` のエラーにし、
                // 制限していないのに作れなければ内部エラーにする。
                let kind = if kir::address_space_limit().is_some() {
                    ErrorKind::Limit
                } else {
                    ErrorKind::Internal
                };
                Error::new(
                    kind,
                    format!("LIR への下ろしのスレッドを作れません({stack} バイトのスタック): {e}"),
                )
            })?;
        match h.join() {
            Ok(r) => r,
            Err(_) => Err(Error::new(ErrorKind::Internal, "下ろしが落ちました")),
        }
    })
}

/// パス 1〜10 を行って、パス 11 の前の LIR を作り、検証する。
///
/// `p` は検証済み(`kir::verify` か `kir::load` を通ったもの)でなければならない。
/// 再帰の深さは KIR のノードの深さに比例するので、深さ `max_nest` に足りるスタックを持つ専用のスレッドで実行する。
pub fn lower_unassigned(p: &kir::Program, max_nest: usize) -> Result<Program, Error> {
    let lp = on_stack(stack_for(max_nest), || {
        program::lower_program(p).map_err(|m| Error::new(ErrorKind::Lower, m))
    })?;
    verify(&lp)?;
    Ok(lp)
}

/// パス 11 とパス 12 を行い、LIR を検証する。
pub fn assign_frames(p: &mut Program) -> Result<(), Error> {
    assign_frames_with(p, true)
}

/// パス 11 とパス 12 を行い、LIR を検証する。
///
/// `yield_prune` が偽なら yield の枝刈りを行わない(`--no-yield-prune`)。
pub fn assign_frames_with(p: &mut Program, yield_prune: bool) -> Result<(), Error> {
    for f in &mut p.funcs {
        frame::assign(f, yield_prune);
    }
    verify(p)
}

/// 検証済みの KIR を LIR に変換する(パス 1〜12)。
///
/// パス 10 までを終えた後と、パス 12 の後に LIR を検証する。
/// KIR の入れ子の上限は `kir::N_NEST` とする。
pub fn lower(p: &kir::Program) -> Result<Program, Error> {
    lower_with(p, kir::N_NEST)
}

/// KIR の入れ子の上限 `max_nest` を指定して、[`lower`] と同じ変換を行う。
pub fn lower_with(p: &kir::Program, max_nest: usize) -> Result<Program, Error> {
    lower_opts(p, max_nest, true)
}

/// KIR の入れ子の上限 `max_nest` と、yield の枝刈りを行うかどうか(`yield_prune`)を指定して、[`lower`] と同じ変換を行う。
pub fn lower_opts(p: &kir::Program, max_nest: usize, yield_prune: bool) -> Result<Program, Error> {
    let mut lp = lower_unassigned(p, max_nest)?;
    assign_frames_with(&mut lp, yield_prune)?;
    Ok(lp)
}

/// LIR を検証する。
pub fn verify(p: &Program) -> Result<(), Error> {
    verify::verify(p).map_err(|m| Error::new(ErrorKind::Verify, m))
}

/// 単位を後ろに追加していく KIR のプログラムを、追加した単位ごとに LIR に変換する(`keleut repl`)。
///
/// 対話的な実行(REPL)は、受理した入力ごとに KIR の単位を 1 つ後ろに追加し、それまでの単位はランタイムにそのまま残す。
/// [`Extender::extend`] は、前回までに変換した範囲より後ろの KIR の項目だけを LIR に変換し(パス 1〜12)、LIR のプログラムの各表の後ろに追加して、追加した部分だけを検証する。
/// それまでの関数、グローバル、シェイプ、文字列の添字は変わらないので、ランタイムが持っている値はそのまま使える。
/// 処理時間は、追加した単位のサイズに比例する。
///
/// 1 回で変換したプログラム([`lower`])との違いは、関数の順序だけである。
/// 追加した単位の KIR の関数は、それまでの単位の関数(KIR の関数と、変換器がその後ろに追加した関数)の後ろに続く。
/// 空の [`Extender`] と [`Program::empty`] から全体を 1 回で追加すれば、[`lower_opts`] と同じ LIR になる。
#[derive(Default)]
pub struct Extender {
    st: program::State,
    verified: verify::Marks,
}

impl Extender {
    /// まだ何も変換していない状態の [`Extender`] を作る。
    /// [`Program::empty`] と対にして使う。
    pub fn new() -> Extender {
        Extender::default()
    }

    /// KIR のプログラム `k` のうち、前回までに変換した範囲より後ろの項目を LIR に変換し、`lp` の後ろに追加する。
    ///
    /// `k` は、前回渡したプログラムの各表の後ろに項目を追加したもので、追加した項目まで検証済みでなければならない(`kir::verify_extension`)。
    /// `lp` は前回の呼び出しが作ったプログラムである。
    /// 前回の呼び出しの後にランタイムがシェイプを作っていれば、呼び出し元がそれを、ランタイムの番号の順に `lp.shapes` の後ろへ追加しておく。
    /// ランタイムのシェイプ表と番号を一致させるためで、今回追加するシェイプの番号はその後ろから振る。
    /// 返すエラーは、仮想メモリを制限しているときに変換用のスレッドを作れない場合の [`ErrorKind::Limit`] と、処理系の内部エラーである。
    /// どちらのエラーの後も、`self` と `lp` は使えない。
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

/// 関数 `f` について、パス 12 の生存解析([`Frame`] の再開点と安全点を求める解析)の処理量と、検証器が行う定義の検査の処理量を返す。
///
/// 処理量は、変更ログと集合を読み書きした量である。
/// 処理量が関数のサイズに比例することを、テストで確かめるために使う。
#[doc(hidden)]
pub fn dataflow_work(f: &Func) -> (u64, u64) {
    (frame::points_counted(f).2, verify::check_defs(f).1)
}
