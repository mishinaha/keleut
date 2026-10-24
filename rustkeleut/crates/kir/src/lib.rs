//! KIR(Keleut Intermediate Representation)の型、バイナリとテキストの読み書き、検証器。
//!
//! 規範は親リポジトリの `doc/IRSpec.md`(版 0.3)である。
//! バイナリとテキストは、Diktor の書き手(`diktor --emit-ir`、`--emit-ir-text`)とバイト単位で同じに読み書きする。
//!
//! | 入口 | 働き | 規範 |
//! |---|---|---|
//! | [`read_kir`] | バイナリを読む | IRSpec 13.1〜13.7 節、13.9 節 |
//! | [`read_kirt`] | テキストを読む | IRSpec 12 節、13.9 節 |
//! | [`read_any`] | マジックでバイナリかテキストかを決めて読む | |
//! | [`verify()`] | 検証する | IRSpec 13.8 節、13.9 節 |
//! | [`write_kir`] | 正準のバイナリを書く | IRSpec 13 節 |
//! | [`write_kirt`] | 正準のテキストを書く | IRSpec 12 節 |
//! | [`write_kirt_to`] | 正準のテキストを書き手に流す | IRSpec 12 節 |
//!
//! どの入口も、どの入力に対しても落ちずに [`Error`] を返す。
//! 読み込みと検証と書き出しの各パスは、ノードの深さに比例して再帰する。
//! 深さは入れ子の計数器で数え、限度 [`Limits::max_nest`](既定は IRSpec 13.9 節の `N_nest = 100,000`)を超えたら
//! その場で拒否する。
//! 入口はこれらのパスを、大きさ [`Limits::stack_bytes`] のスタックを持つ専用のスレッドで走らせる。
//! そのため、呼び出し側のスレッドのスタックの大きさに依らず、限度の内側でスタックを使い尽くさない。
//!
//! 式、辞書、パターンのノードは [`Nodes`] の配列に置く。
//! 深いプログラムを捨てても写しても、再帰は起きない。

pub mod binary;
pub mod builtins;
mod error;
mod model;
pub mod text;
pub mod verify;

pub use error::{Error, ErrorKind, READ_LIMIT_MESSAGE, VERIFY_LIMIT_MESSAGE, WRITE_LIMIT_MESSAGE};
pub use model::*;

/// 入れ子の限度 `N_nest`(IRSpec 13.9 節)。
pub const N_NEST: usize = 100_000;

/// 再帰の 1 段(ノードの深さ 1 つ)あたりに見込むスタックのバイト数。
///
/// 読み込み、検証、書き出し、印字のどの再帰の 1 段よりも大きく取ってある。
/// 2026-10-24 に測った 1 段の最大は、最適化なしで約 4.5 KiB(テキストのパターンの読み込み)、
/// 最適化ありで約 0.8 KiB(辞書の印字)である。
/// 深さ `N_nest` を、この見積もりのスタックで走らせる試験を `tests/depth.rs` に置く。
pub const STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) { 8192 } else { 2048 };

/// 処理の限度。
///
/// `stack_bytes` は、`max_nest × STACK_BYTES_PER_LEVEL` に余裕を足した大きさ以上でなければならない。
/// 足りないと、限度の内側の入力でスタックがあふれ、プロセスが落ちる(IRSpec 13.9 節はこれを許さない)。
/// 欄を直に書き換えず、[`Limits::for_nest`] か [`Limits::with_stack`] で作ること。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// ノードの深さの上限。
    pub max_nest: usize,
    /// 各パスを走らせる専用のスレッドのスタックの大きさ(バイト)。
    pub stack_bytes: usize,
}

/// 再帰の外で使うスタックに見込む余裕(バイト)。
///
/// スタックの 4 分の 1 で、32 KiB を下回らず、16 MiB を超えない。
fn stack_margin(stack_bytes: usize) -> usize {
    (stack_bytes / 4).clamp(32 << 10, 16 << 20)
}

impl Limits {
    /// 深さの上限 `max_nest` に足りるスタックの大きさを見積もった限度。
    pub fn for_nest(max_nest: usize) -> Limits {
        Limits {
            max_nest,
            stack_bytes: max_nest
                .saturating_mul(STACK_BYTES_PER_LEVEL)
                .saturating_add(16 << 20),
        }
    }

    /// スタックの大きさ `stack_bytes` を決めた限度。
    ///
    /// 深さの上限は、`max_nest` と、スタックに収まる深さの小さい方にする。
    /// こうすると深い入力は、スタックがあふれる前に入れ子の計数器で拒否される。
    /// 1 段も収まらない小さなスタックでは、深さの上限が 0 になる(呼び出し側が拒否する)。
    pub fn with_stack(max_nest: usize, stack_bytes: usize) -> Limits {
        let fits = stack_bytes.saturating_sub(stack_margin(stack_bytes)) / STACK_BYTES_PER_LEVEL;
        Limits {
            max_nest: max_nest.min(fits),
            stack_bytes,
        }
    }
}

impl Default for Limits {
    /// `N_nest` と、それに足りるスタック。
    ///
    /// スタックは予約するだけで、実際に使うのは入力の深さの分だけである。
    /// 予約の大きさは、最適化ありで約 211 MiB、最適化なしで約 797 MiB である。
    fn default() -> Limits {
        Limits::for_nest(N_NEST)
    }
}

/// `f` を大きさ `stack_bytes` のスタックを持つ専用のスレッドで走らせる。
///
/// スレッドを作れないときと、`f` が落ちたとき(この crate の不具合)は [`ErrorKind::Internal`] を返す。
pub fn on_stack<T: Send>(
    stack_bytes: usize,
    f: impl FnOnce() -> Result<T, Error> + Send,
) -> Result<T, Error> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("kir".to_string())
            .stack_size(stack_bytes)
            .spawn_scoped(s, f)
            .map_err(|e| {
                Error::new(
                    ErrorKind::Internal,
                    format!("KIR の処理のスレッドを作れません: {e}"),
                )
            })?;
        match h.join() {
            Ok(r) => r,
            Err(_) => Err(Error::new(
                ErrorKind::Internal,
                "KIR の処理の中で内部の誤りが起きました",
            )),
        }
    })
}

/// バイナリ(`.kir`)を読む。
pub fn read_kir(bytes: &[u8]) -> Result<Program, Error> {
    read_kir_with(bytes, &Limits::default())
}

/// 限度を与えてバイナリを読む。
pub fn read_kir_with(bytes: &[u8], lim: &Limits) -> Result<Program, Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || binary::decode(bytes, n))
}

/// テキスト(`.kirt`)を読む。
pub fn read_kirt(text: &[u8]) -> Result<Program, Error> {
    read_kirt_with(text, &Limits::default())
}

/// 限度を与えてテキストを読む。
pub fn read_kirt_with(text: &[u8], lim: &Limits) -> Result<Program, Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || text::parse(text, n))
}

/// 先頭の 4 バイトがマジック `00 4B 49 52` ならバイナリとして、ほかはテキストとして読む。
pub fn read_any(bytes: &[u8]) -> Result<Program, Error> {
    read_any_with(bytes, &Limits::default())
}

/// 限度を与えて [`read_any`] を行う。
pub fn read_any_with(bytes: &[u8], lim: &Limits) -> Result<Program, Error> {
    if is_binary(bytes) {
        read_kir_with(bytes, lim)
    } else {
        read_kirt_with(bytes, lim)
    }
}

/// バイト列がバイナリのマジックで始まるか。
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(&binary::MAGIC)
}

/// 検証する(IRSpec 13.8 節、13.9 節)。
pub fn verify(p: &Program) -> Result<(), Error> {
    verify_with(p, &Limits::default())
}

/// 限度を与えて検証する。
pub fn verify_with(p: &Program, lim: &Limits) -> Result<(), Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || verify::check(p, n))
}

/// 正準のバイナリを書く。
pub fn write_kir(p: &Program) -> Result<Vec<u8>, Error> {
    write_kir_with(p, &Limits::default())
}

/// 限度を与えて正準のバイナリを書く。
pub fn write_kir_with(p: &Program, lim: &Limits) -> Result<Vec<u8>, Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || binary::encode(p, n))
}

/// 正準のテキストを書く。
pub fn write_kirt(p: &Program) -> Result<Vec<u8>, Error> {
    write_kirt_with(p, &Limits::default())
}

/// 限度を与えて正準のテキストを書く。
pub fn write_kirt_with(p: &Program, lim: &Limits) -> Result<Vec<u8>, Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || text::print(p, n))
}

/// 限度を与えて、正準のテキストを `w` に書く。
///
/// [`write_kirt_with`] と違い、テキストの全体をメモリに置かない([`text::print_to`])。
pub fn write_kirt_to(
    p: &Program,
    lim: &Limits,
    w: &mut (dyn std::io::Write + Send),
) -> Result<(), Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || text::print_to(p, n, w))
}

/// 読んで検証する。KIR を実行する処理系の読み込みの入口である(失敗は終了状態 65)。
pub fn load(bytes: &[u8]) -> Result<Program, Error> {
    load_with(bytes, &Limits::default())
}

/// 限度を与えて [`load`] を行う。読み込みと検証を 1 つのスレッドで行う。
pub fn load_with(bytes: &[u8], lim: &Limits) -> Result<Program, Error> {
    let n = lim.max_nest;
    on_stack(lim.stack_bytes, || {
        let p = if is_binary(bytes) {
            binary::decode(bytes, n)?
        } else {
            text::parse(bytes, n)?
        };
        verify::check(&p, n)?;
        Ok(p)
    })
}
