//! 標準出力の規約(計画 261024-1 の 6.5 節、D530)。
//!
//! - `Console.write` の文字列を 64 KiB のバッファに足す。
//! - flush するのは、バッファが満ちたとき、標準エラーへ書く直前、終了の前である。
//! - 書き込みは短い書き込みを繰り返す。
//!   `EINTR` はすぐに、`EAGAIN`(非ブロッキングのパイプ)は少し待ってから再試行する。
//! - 書き込みに失敗したら誤りを返す。
//!   誤りが `EBADF`、`EPIPE`、`ECONNRESET` なら、未出力のバッファを捨てる。
//!   ほかの誤り(`ENOSPC`、`EIO` など)なら、まだ書けていないバイトをバッファに残すので、次の書き出しはそれを改めて書こうとしてまた失敗する。
//!   どちらの場合も、その後の書き込みはバッファに足し、次の書き出しで改めて書き込みを試み、失敗すればまた誤りを返す。
//!   これは Diktor が使う OCaml 5.2 のチャネル(`caml_flush_partial`)と同じ振る舞いである。
//!
//! 書き込みは記述子 1 に直に行い、`std::io::stdout()`(行ごとに flush する)を通さない。
//! 診断を標準エラーに書くのは [`Out::eprint`] で、先に標準出力を flush する。
//!
//! Rust の実行時は、閉じた標準入出力(`>&-`)を起動のときに `/dev/null` で開き直す。
//! そこで、`main` より前に動く `.init_array` の関数 [`probe_closed_stdout`] が `fcntl(1, F_GETFD)` で記述子 1 を調べ、
//! 閉じていたことを [`STDOUT_WAS_CLOSED`] に残す。
//! 印が立っていれば、[`Fd1`] の書き込みは `EBADF` で失敗し、最初の失敗の扱い(診断と 74)に進む。
//! 開き直した `/dev/null` はそのまま残すので、子プロセスのパイプが記述子 1 に入ることはない。

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

/// 起動のとき(Rust の実行時が開き直す前)に記述子 1 が閉じていたか。
pub static STDOUT_WAS_CLOSED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// `fcntl` の `F_GETFD`。
const F_GETFD: i32 = 1;

/// `EBADF`。
const EBADF: i32 = 9;

/// `EPIPE`。
const EPIPE: i32 = 32;

/// `ECONNRESET`。
const ECONNRESET: i32 = 104;

/// 記述子 1 が閉じていれば [`STDOUT_WAS_CLOSED`] を立てる。
///
/// `.init_array` から、Rust の実行時の初期化より前に呼ばれる。
pub extern "C" fn probe_closed_stdout() {
    // SAFETY: fcntl(F_GETFD) は記述子の状態を読むだけである。
    if unsafe { fcntl(1, F_GETFD) } == -1 {
        STDOUT_WAS_CLOSED.store(true, Ordering::Relaxed);
    }
}

#[used]
#[unsafe(link_section = ".init_array")]
static PROBE_CLOSED_STDOUT: extern "C" fn() = probe_closed_stdout;

/// バッファの大きさ(64 KiB)。
pub const BUFFER_BYTES: usize = 64 * 1024;

/// 書き込みの失敗。文面は OS の誤りの文(Rust の `(os error N)` の添え書きを除く)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteError(pub String);

/// 書き込みの途中の失敗。
struct Failed {
    /// 失敗の文。
    error: WriteError,
    /// 失敗までに書けたバイトの数。
    written: usize,
    /// 未出力のバッファを捨てる誤り(`EBADF`、`EPIPE`、`ECONNRESET`)か。
    discard: bool,
}

/// 出力先。
pub trait Sink {
    /// `buf` の先頭から書けるだけ書き、書いたバイトの数を返す。
    fn write_some(&mut self, buf: &[u8]) -> std::io::Result<usize>;
}

/// 記述子 1。
///
/// 起動のときに記述子 1 が閉じていたなら、書き込みはどれも `EBADF` で失敗する。
pub struct Fd1 {
    file: ManuallyDrop<File>,
    closed: bool,
}

impl Default for Fd1 {
    fn default() -> Self {
        // 印を読むことで、.init_array の静的変数を含む目的ファイルがリンクから落ちないようにする。
        std::hint::black_box(&PROBE_CLOSED_STDOUT);
        Fd1 {
            // SAFETY: 記述子 1 は Rust の実行時が開いたままにしており、ManuallyDrop なので閉じない。
            file: ManuallyDrop::new(unsafe { File::from_raw_fd(1) }),
            closed: STDOUT_WAS_CLOSED.load(Ordering::Relaxed),
        }
    }
}

impl Sink for Fd1 {
    fn write_some(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.closed {
            return Err(std::io::Error::from_raw_os_error(EBADF));
        }
        self.file.write(buf)
    }
}

/// OS の誤りの文から、Rust が足す ` (os error N)` を除く。
pub fn os_message(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) if s.ends_with(')') => s[..i].to_string(),
        _ => s,
    }
}

/// 標準出力。
pub struct Out<S: Sink = Fd1> {
    sink: S,
    buf: Vec<u8>,
    /// 書く文字列を捨てるか([`Out::set_discard`])。
    discard: bool,
    /// [`Out::eprint_after_flush`] の記録を標準出力に書くか([`Out::set_log_to_stdout`])。
    log_to_stdout: bool,
}

impl Default for Out<Fd1> {
    fn default() -> Self {
        Out::new(Fd1::default())
    }
}

impl<S: Sink> Out<S> {
    /// `sink` へ書く標準出力。
    pub fn new(sink: S) -> Out<S> {
        Out {
            sink,
            buf: Vec::with_capacity(BUFFER_BYTES),
            discard: false,
            log_to_stdout: false,
        }
    }

    /// 真なら、これから [`Out::write`] に渡す文字列を捨てる。
    ///
    /// 対話的な実行は、前の入力の初期化をやり直すときに、その出力を捨てる。
    pub fn set_discard(&mut self, on: bool) {
        self.discard = on;
    }

    /// 真なら、[`Out::eprint_after_flush`] の記録を標準エラーでなく標準出力に足す。
    ///
    /// 対話的な実行は、結果、診断、プログラムの出力をすべて標準出力に出す(Diktor の `--repl` の `cancel_log`)。
    pub fn set_log_to_stdout(&mut self, on: bool) {
        self.log_to_stdout = on;
    }

    /// 出力先(試験用)。
    pub fn sink(&self) -> &S {
        &self.sink
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), Failed> {
        let mut written = 0;
        let mut waits = 0u32;
        while written < data.len() {
            match self.sink.write_some(&data[written..]) {
                Ok(0) => {
                    return Err(Failed {
                        error: WriteError("write returned 0".to_string()),
                        written,
                        discard: false,
                    });
                }
                Ok(n) => {
                    written += n;
                    waits = 0;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // 読み手が追いつくのを待つ。待ち時間は 1 ms から 50 ms まで延ばす。
                    waits = waits.saturating_add(1);
                    let ms = u64::from(waits.min(50));
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                Err(e) => {
                    let discard = matches!(e.raw_os_error(), Some(EBADF | EPIPE | ECONNRESET));
                    return Err(Failed {
                        error: WriteError(os_message(&e)),
                        written,
                        discard,
                    });
                }
            }
        }
        Ok(())
    }

    /// バッファを書き出す。
    ///
    /// 失敗したとき、捨てる誤りならバッファを空にし、そうでなければ書けなかったバイトを残す。
    pub fn flush(&mut self) -> Result<(), WriteError> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let buf = std::mem::take(&mut self.buf);
        let r = self.write_all(&buf);
        self.buf = buf;
        match r {
            Ok(()) => {
                self.buf.clear();
                Ok(())
            }
            Err(f) => {
                if f.discard {
                    self.buf.clear();
                } else {
                    self.buf.drain(..f.written);
                }
                Err(f.error)
            }
        }
    }

    /// `Console.write` の文字列を足す。満ちたら書き出す。
    pub fn write(&mut self, data: &[u8]) -> Result<(), WriteError> {
        if self.discard {
            return Ok(());
        }
        if self.buf.len() + data.len() < BUFFER_BYTES {
            self.buf.extend_from_slice(data);
            return Ok(());
        }
        // 満ちるまで足して書き出し、残りが 1 つのバッファより大きければ直に書く。
        let room = BUFFER_BYTES - self.buf.len();
        self.buf.extend_from_slice(&data[..room]);
        self.flush()?;
        let rest = &data[room..];
        let big = rest.len() - rest.len() % BUFFER_BYTES;
        if big > 0
            && let Err(f) = self.write_all(&rest[..big])
        {
            // 捨てない誤りでは、OCaml のチャネルがバッファに入れて残す分(1 つのバッファ)だけ、書けなかったバイトを残す。
            if !f.discard {
                let left = &rest[f.written..big];
                self.buf
                    .extend_from_slice(&left[..left.len().min(BUFFER_BYTES)]);
            }
            return Err(f.error);
        }
        self.buf.extend_from_slice(&rest[big..]);
        if self.buf.len() >= BUFFER_BYTES {
            self.flush()?;
        }
        Ok(())
    }

    /// 標準出力を flush してから `msg` を標準エラーに書く。flush の失敗は書かずに返す。
    ///
    /// 防壁が抑制した例外の記録に使う(IRSpec 9.6 節)。
    /// 前の失敗がバッファを捨てる誤りで、その後に書き足したものが無ければ、バッファは空なので flush は成功し、`msg` を書く。
    pub fn eprint_after_flush(&mut self, msg: &[u8]) -> Result<(), WriteError> {
        if self.log_to_stdout {
            return self.write(msg);
        }
        self.flush()?;
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(msg);
        let _ = e.flush();
        Ok(())
    }

    /// 標準出力を flush してから(失敗は無視する)、`msg` を標準エラーに書く。
    ///
    /// 標準エラーに書けないときは何もしない(Diktor の `safe_exit` と同じ)。
    pub fn eprint(&mut self, msg: &[u8]) {
        let _ = self.flush();
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(msg);
        let _ = e.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 書いた呼び出しを記録し、決めた回数の後で失敗する出力先。
    #[derive(Default)]
    struct Rec {
        writes: Vec<usize>,
        data: Vec<u8>,
        max: usize,
        fail_after: Option<usize>,
        errno: i32,
        block_every: usize,
        calls: usize,
    }

    impl Sink for Rec {
        fn write_some(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if let Some(n) = self.fail_after
                && self.writes.len() >= n
            {
                return Err(std::io::Error::from_raw_os_error(self.errno));
            }
            if self.block_every > 0 && self.calls.is_multiple_of(self.block_every) {
                return Err(ErrorKind::WouldBlock.into());
            }
            let n = buf
                .len()
                .min(if self.max == 0 { usize::MAX } else { self.max });
            self.writes.push(n);
            self.data.extend_from_slice(&buf[..n]);
            Ok(n)
        }
    }

    #[test]
    fn 満ちるまで書かず短い書き込みと待ちを繰り返す() {
        let mut o = Out::new(Rec {
            max: 1000,
            block_every: 7,
            ..Rec::default()
        });
        o.write(b"abc").unwrap();
        assert!(o.sink().data.is_empty());
        let big = vec![b'x'; BUFFER_BYTES * 2 + 10];
        o.write(&big).unwrap();
        assert_eq!(o.sink().data.len(), BUFFER_BYTES * 2);
        o.flush().unwrap();
        assert_eq!(o.sink().data.len(), big.len() + 3);
        assert!(o.sink().writes.iter().all(|&n| n <= 1000));
    }

    #[test]
    fn 捨てる誤りではバッファを捨て後の書き込みは改めて試す() {
        let mut o = Out::new(Rec {
            fail_after: Some(0),
            errno: EBADF,
            ..Rec::default()
        });
        o.write(b"hello").unwrap();
        let e = o.flush().unwrap_err();
        assert_eq!(e.0, "Bad file descriptor");
        // 捨てたので、書き足さなければ flush は成功する。
        o.flush().unwrap();
        // 書き足したものは改めて書き出しを試み、また失敗する。
        o.write(b"again").unwrap();
        assert!(o.flush().is_err());
        assert!(o.write(&vec![0u8; BUFFER_BYTES * 3]).is_err());
        o.flush().unwrap();
        assert!(o.sink().data.is_empty());
    }

    #[test]
    fn ほかの誤りでは書けなかったバイトを残す() {
        let mut o = Out::new(Rec {
            fail_after: Some(1),
            errno: 28,
            max: 2,
            ..Rec::default()
        });
        o.write(b"hello").unwrap();
        let e = o.flush().unwrap_err();
        assert_eq!(e.0, "No space left on device");
        assert_eq!(o.sink().data, b"he");
        // 残したので、書き足さなくても次の flush はまた失敗する。
        assert!(o.flush().is_err());
        // 直に書く大きな書き込みの失敗でも、1 つのバッファまでを残す。
        let mut o = Out::new(Rec {
            fail_after: Some(1),
            errno: 28,
            ..Rec::default()
        });
        assert!(o.write(&vec![0u8; BUFFER_BYTES * 3]).is_err());
        assert_eq!(o.sink().data.len(), BUFFER_BYTES);
        assert!(o.flush().is_err());
    }
}
