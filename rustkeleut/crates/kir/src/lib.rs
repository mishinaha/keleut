//! KIR(Keleut Intermediate Representation)の型、バイナリとテキストの読み書き、検証器。
//!
//! 仕様はリポジトリの根の `doc/IRSpec.md`(バージョン 1.0)が定める。
//! バイナリとテキストは、Diktor のライタ(`diktor --emit-ir`、`--emit-ir-text`)とバイト単位で一致するように読み書きする。
//!
//! | 関数 | 動作 | 仕様 |
//! |---|---|---|
//! | [`read_kir`] | バイナリを読む | IRSpec 13.1〜13.7 節、13.9 節 |
//! | [`read_kirt`] | テキストを読む | IRSpec 12 節、13.9 節 |
//! | [`read_any`] | マジックナンバーでバイナリかテキストかを決めて読む | |
//! | [`verify()`] | 検証する | IRSpec 13.8 節、13.9 節 |
//! | [`write_kir`] | 正準な(canonical。一意に決まる形の)バイナリを書く | IRSpec 13 節 |
//! | [`write_kirt`] | 正準なテキストを書く | IRSpec 12 節 |
//! | [`write_kirt_to`] | 正準なテキストを出力先に順に書く | IRSpec 12 節 |
//!
//! 表のどの関数も、どの入力に対しても異常終了せず、失敗を [`Error`] で返す。
//! 読み込み、検証、書き出しの各パスは、ノードの深さに比例して再帰する。
//! 深さは入れ子のカウンタで数え、上限 [`Limits::max_nest`] を超えたらその場で拒否する。
//! 上限の既定値は IRSpec 13.9 節の `N_nest = 100,000` である。
//! 表の関数はこれらのパスを、サイズ [`Limits::stack_bytes`] のスタックを持つ専用のスレッドで実行する。
//! そのため、呼び出し元のスレッドのスタックサイズによらず、深さが上限以下の入力でスタックオーバーフローにならない。
//! 仮想メモリに上限(`ulimit -v`)があれば、スタックを初めからその上限の 4 分の 1 までにし、スレッドを作れなければ半分ずつ小さくして作り直す。
//! 初めに 4 分の 1 までにしたときも、作り直しで小さくしたときも、深さの上限をそのスタックに収まる深さまで下げる(初めの調整は [`Limits::fit_address_space`]、作り直しのときは [`Limits::with_stack`])。
//! 最も小さいスタックでも作れなければ、`処理系の限度` の診断を持つエラーを返す(終了コード 65)。
//!
//! 式、辞書、パターンのノードは [`Nodes`] の配列に置く。
//! そのため、深いプログラムをドロップしてもコピーしても、再帰は起きない。

pub mod binary;
pub mod builtins;
mod error;
mod model;
pub mod text;
pub mod verify;

pub use error::{Error, ErrorKind, READ_LIMIT_MESSAGE, VERIFY_LIMIT_MESSAGE, WRITE_LIMIT_MESSAGE};
pub use model::*;

/// 入れ子の上限 `N_nest`(IRSpec 13.9 節)。
pub const N_NEST: usize = 100_000;

/// 再帰の 1 段(ノードの深さ 1 つ)あたりに見積もるスタックのバイト数。
///
/// 読み込み、検証、バイナリの書き出し、テキストの書き出しのどの再帰の 1 段よりも大きくしてある。
/// 実測では、再帰 1 段の最大はデバッグビルドで約 4.5 KiB(テキストのパターンの読み込み)、
/// リリースビルドで約 0.8 KiB(辞書のテキストの書き出し)である。
/// `tests/depth.rs` に、深さ `N_nest` のプログラムをこの見積もりのスタックで処理するテストがある。
pub const STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) { 8192 } else { 2048 };

/// 読み込み、検証、書き出しに課す上限。ノードの深さの上限と、スタックサイズからなる。
///
/// `stack_bytes` は、`max_nest × STACK_BYTES_PER_LEVEL` に余裕を足した値以上でなければならない。
/// 足りないと、深さが上限以下の入力でスタックオーバーフローになり、プロセスが異常終了する(IRSpec 13.9 節はこれを許さない)。
/// フィールドを直接書き換えず、[`Limits::for_nest`] か [`Limits::with_stack`] で作ること。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// ノードの深さの上限。
    pub max_nest: usize,
    /// 各パスを実行する専用のスレッドのスタックサイズ(バイト)。
    pub stack_bytes: usize,
}

/// 再帰の外で使うスタックとして見積もる余裕(バイト)。
///
/// スタックの 4 分の 1 で、32 KiB を下回らず、16 MiB を超えない。
fn stack_margin(stack_bytes: usize) -> usize {
    (stack_bytes / 4).clamp(32 << 10, 16 << 20)
}

impl Limits {
    /// 深さの上限を `max_nest` にし、スタックサイズをその深さに必要な見積もりにした [`Limits`] を作る。
    ///
    /// スタックサイズは `max_nest × STACK_BYTES_PER_LEVEL` に 16 MiB を足した値である。
    pub fn for_nest(max_nest: usize) -> Limits {
        Limits {
            max_nest,
            stack_bytes: max_nest
                .saturating_mul(STACK_BYTES_PER_LEVEL)
                .saturating_add(16 << 20),
        }
    }

    /// スタックサイズ `stack_bytes` を指定して [`Limits`] を作る。
    ///
    /// 深さの上限は、`max_nest` と、スタックに収まる深さのうち小さいほうにする。
    /// こうすると深い入力は、スタックオーバーフローになる前に入れ子のカウンタで拒否される。
    /// 1 段も収まらない小さなスタックでは、深さの上限が 0 になる。
    /// この場合は呼び出し元が拒否する。
    pub fn with_stack(max_nest: usize, stack_bytes: usize) -> Limits {
        let fits = stack_bytes.saturating_sub(stack_margin(stack_bytes)) / STACK_BYTES_PER_LEVEL;
        Limits {
            max_nest: max_nest.min(fits),
            stack_bytes,
        }
    }
}

impl Default for Limits {
    /// 深さの上限を `N_nest` にし、スタックサイズをその深さに必要な見積もりにした [`Limits`] を返す。
    ///
    /// スタックは予約するだけで、実際に使うのは入力の深さの分だけである。
    /// 予約するサイズは、リリースビルドで約 211 MiB、デバッグビルドで約 797 MiB である。
    fn default() -> Limits {
        Limits::for_nest(N_NEST)
    }
}

/// 仮想メモリのソフトリミット(`RLIMIT_AS`。バイト)を返す。上限がなければ `None` を返す。
pub fn address_space_limit() -> Option<usize> {
    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, rlim: *mut RLimit) -> i32;
    }
    /// `getrlimit` に渡す資源の番号。Linux での `RLIMIT_AS` の値である。
    const RLIMIT_AS: i32 = 9;
    let mut r = RLimit { cur: 0, max: 0 };
    // SAFETY: r は呼び出しの間だけ使う有効な領域である。
    if unsafe { getrlimit(RLIMIT_AS, &mut r) } != 0 || r.cur == u64::MAX {
        return None;
    }
    usize::try_from(r.cur).ok()
}

/// 仮想メモリに上限があるときに、KIR を処理するスレッドを作り直す際のスタックの下限(16 MiB)。
pub const MIN_THREAD_STACK: usize = 16 << 20;

/// 最も小さいスタックでも KIR を処理するスレッドを作れないときに書く診断の先頭部分(終了コード 65)。
pub const SPAWN_LIMIT_MESSAGE: &str = "処理系の限度: KIR の処理のスレッドを作れません";

impl Limits {
    /// スタックサイズと深さの上限を、仮想メモリの上限(`RLIMIT_AS`)に合わせて調整した [`Limits`] を返す。
    ///
    /// スタックに許すサイズは、仮想メモリの上限の 4 分の 1 である(ただし [`MIN_THREAD_STACK`] を下回らない)。
    /// スタックがこれより大きければ、スタックをこのサイズにし、
    /// 深さの上限をそのスタックに収まる深さまで下げる([`Limits::with_stack`])。
    /// 仮想メモリに上限がないか、スタックがすでにこのサイズ以下なら、そのまま返す。
    pub fn fit_address_space(self) -> Limits {
        match address_space_limit() {
            Some(a) => {
                let cap = (a / 4).max(MIN_THREAD_STACK);
                if self.stack_bytes > cap {
                    Limits::with_stack(self.max_nest, cap)
                } else {
                    self
                }
            }
            None => self,
        }
    }
}

/// `f` を、`lim` が指定するサイズのスタックを持つ専用のスレッドで実行する。`f` は深さの上限を受け取る。
///
/// 仮想メモリに上限があれば、初めに [`Limits::fit_address_space`] で `lim` を調整する。
/// そのうえでスレッドを作れなければ、スタックを半分ずつ小さくし、深さの上限をそれに収まる深さまで下げて作り直す。
/// 最後は [`MIN_THREAD_STACK`] で作り直し、それでも作れなければ、`処理系の限度` の診断を持つ [`ErrorKind::Limit`] のエラー(終了コード 65)を返す。
/// 初めのスタックがすでに [`MIN_THREAD_STACK`] 以下なら、作り直さない。
/// 仮想メモリに上限がないときにスレッドを作れなければ、作り直さずに [`ErrorKind::Internal`] を返す。
/// `f` がパニックしたとき(この crate のバグ)は [`ErrorKind::Internal`] を返す。
fn on_limits<T: Send>(
    lim: &Limits,
    f: impl FnOnce(usize) -> Result<T, Error> + Send,
) -> Result<T, Error> {
    on_limits_nest(lim, f).map(|(t, _)| t)
}

/// [`on_limits`] と同じように `f` を実行し、結果と、`f` に渡した深さの上限を返す。
fn on_limits_nest<T: Send>(
    lim: &Limits,
    f: impl FnOnce(usize) -> Result<T, Error> + Send,
) -> Result<(T, usize), Error> {
    let mut lim = lim.fit_address_space();
    let job = std::sync::Mutex::new(Some(f));
    loop {
        let job = &job;
        let n = lim.max_nest;
        let r = std::thread::scope(|s| {
            let h = std::thread::Builder::new()
                .name("kir".to_string())
                .stack_size(lim.stack_bytes)
                .spawn_scoped(s, move || {
                    let f = job.lock().ok().and_then(|mut g| g.take());
                    f.map(|f| f(n))
                });
            match h {
                Ok(h) => Ok(match h.join() {
                    Ok(Some(r)) => r,
                    _ => Err(Error::new(
                        ErrorKind::Internal,
                        "KIR の処理の中で内部の誤りが起きました",
                    )),
                }),
                Err(e) => Err(e),
            }
        });
        let limited = address_space_limit().is_some();
        match r {
            Ok(r) => return r.map(|t| (t, n)),
            Err(e) if !limited => {
                return Err(Error::new(
                    ErrorKind::Internal,
                    format!("KIR の処理のスレッドを作れません: {e}"),
                ));
            }
            Err(_) if lim.stack_bytes > MIN_THREAD_STACK => {
                let next = (lim.stack_bytes / 2).max(MIN_THREAD_STACK);
                lim = Limits::with_stack(lim.max_nest, next);
            }
            Err(e) => {
                return Err(Error::new(
                    ErrorKind::Limit,
                    format!(
                        "{SPAWN_LIMIT_MESSAGE}({} バイトのスタック): {e}",
                        lim.stack_bytes
                    ),
                ));
            }
        }
    }
}

/// `f` を、サイズ `stack_bytes` のスタックを持つ専用のスレッドで実行する。
///
/// スレッドを作れないときと、`f` がパニックしたとき(この crate のバグ)は [`ErrorKind::Internal`] を返す。
/// 仮想メモリの上限に合わせた調整も、スレッドの作り直しもしない。
/// 読み込み、検証、書き出しの関数は、これと違って [`Limits`] を調整し、スレッドを作り直す。
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

/// 上限を指定してバイナリを読む。
pub fn read_kir_with(bytes: &[u8], lim: &Limits) -> Result<Program, Error> {
    on_limits(lim, |n| binary::decode(bytes, n))
}

/// テキスト(`.kirt`)を読む。
pub fn read_kirt(text: &[u8]) -> Result<Program, Error> {
    read_kirt_with(text, &Limits::default())
}

/// 上限を指定してテキストを読む。
pub fn read_kirt_with(text: &[u8], lim: &Limits) -> Result<Program, Error> {
    on_limits(lim, |n| text::parse(text, n))
}

/// 先頭の 4 バイトがマジックナンバー `00 4B 49 52` ならバイナリとして、それ以外はテキストとして読む。
pub fn read_any(bytes: &[u8]) -> Result<Program, Error> {
    read_any_with(bytes, &Limits::default())
}

/// 上限を指定して、[`read_any`] と同じように読む。
pub fn read_any_with(bytes: &[u8], lim: &Limits) -> Result<Program, Error> {
    if is_binary(bytes) {
        read_kir_with(bytes, lim)
    } else {
        read_kirt_with(bytes, lim)
    }
}

/// バイト列がバイナリのマジックナンバーで始まれば真を返す。
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(&binary::MAGIC)
}

/// プログラムを検証する(IRSpec 13.8 節、13.9 節)。
pub fn verify(p: &Program) -> Result<(), Error> {
    verify_with(p, &Limits::default())
}

/// 上限を指定してプログラムを検証する。
pub fn verify_with(p: &Program, lim: &Limits) -> Result<(), Error> {
    on_limits(lim, |n| verify::check(p, n))
}

/// 単位を末尾に追加していくプログラムのうち、検証済みの範囲より後ろに追加された項目だけを、上限を指定して検証する([`verify::Extension`])。
///
/// `keleut repl` が、受理した入力ごとに、その入力で追加した項目(単位を含む)を検証するのに使う。
/// 検証に失敗したときは `ext` を変えない。
/// 検証に通ったときは、検証に使った深さの上限を返す。
/// 仮想メモリに上限があるときは、スタックをその上限に合わせて小さくするか、スレッドを作り直すことで、深さの上限を `lim` より下げていることがある。
/// 呼び出し元は、LIR への変換(lowering)を、返された深さに必要なスタックで行う。
pub fn verify_extension(
    ext: &mut verify::Extension,
    p: &Program,
    lim: &Limits,
) -> Result<usize, Error> {
    on_limits_nest(lim, |n| ext.check(p, n)).map(|((), n)| n)
}

/// 正準なバイナリを書く。
pub fn write_kir(p: &Program) -> Result<Vec<u8>, Error> {
    write_kir_with(p, &Limits::default())
}

/// 上限を指定して正準なバイナリを書く。
pub fn write_kir_with(p: &Program, lim: &Limits) -> Result<Vec<u8>, Error> {
    on_limits(lim, |n| binary::encode(p, n))
}

/// 正準なテキストを書く。
pub fn write_kirt(p: &Program) -> Result<Vec<u8>, Error> {
    write_kirt_with(p, &Limits::default())
}

/// 上限を指定して正準なテキストを書く。
pub fn write_kirt_with(p: &Program, lim: &Limits) -> Result<Vec<u8>, Error> {
    on_limits(lim, |n| text::print(p, n))
}

/// 上限を指定して、正準なテキストを `w` に書く。
///
/// [`write_kirt_with`] と違い、テキスト全体をメモリに置かない([`text::print_to`])。
pub fn write_kirt_to(
    p: &Program,
    lim: &Limits,
    w: &mut (dyn std::io::Write + Send),
) -> Result<(), Error> {
    on_limits(lim, |n| text::print_to(p, n, w))
}

/// バイナリかテキストを読み込んで検証する。
///
/// KIR を実行する処理系が、プログラムの読み込みに使う関数である。
/// 読み込みか検証に失敗したときの終了コードは 65 である。
pub fn load(bytes: &[u8]) -> Result<Program, Error> {
    load_with(bytes, &Limits::default())
}

/// 上限を指定して、[`load`] と同じように読み込んで検証する。読み込みと検証を 1 つのスレッドで行う。
pub fn load_with(bytes: &[u8], lim: &Limits) -> Result<Program, Error> {
    on_limits(lim, |n| {
        let p = if is_binary(bytes) {
            binary::decode(bytes, n)?
        } else {
            text::parse(bytes, n)?
        };
        verify::check(&p, n)?;
        Ok(p)
    })
}
