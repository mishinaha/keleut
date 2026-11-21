//! 標準出力の規約。
//!
//! - `Console.write` の文字列を 64 KiB のバッファに追記する。
//! - flush するのは、バッファがいっぱいになったとき、標準エラーへ書く直前、終了の前である。
//! - 1 回の書き込みで一部しか書けなかったとき(短い書き込み)は、残りを書き終えるまで書き込みを繰り返す。
//!   `EINTR` ならすぐに、`EAGAIN`(非ブロッキングのパイプ)なら少し待ってから再試行する。
//! - 書き込みに失敗したら出力エラーを返す。
//!   エラーが `EBADF`、`EPIPE`、`ECONNRESET` なら、未出力のバッファを捨てる。
//!   それ以外のエラー(`ENOSPC`、`EIO` など)なら、まだ書けていないバイトをバッファに残す。
//!   次の flush は残したバイトをもう一度書こうとするので、エラーの原因が続いていればまた失敗する。
//!   どちらの場合も、その後に書き込まれた文字列はバッファに追記する。
//!   次の flush はその書き込みを試み、失敗すればまたエラーを返す。
//!   これは Diktor が使う OCaml 5.2 のチャネル(`caml_flush_partial`)と同じ動作である。
//!
//! 書き込みはファイル記述子 1 に直接行い、`std::io::stdout()`(行ごとに flush する)を通さない。
//! 診断を標準エラーに書くときは [`Out::eprint`] を使う。
//! [`Out::eprint`] は、先に標準出力を flush する。
//!
//! Rust のランタイムは、閉じた標準入出力(`>&-`)を起動時に `/dev/null` で開き直す。
//! そこで、`main` より前に動く `.init_array` の関数 [`probe_closed_stdout`] が `fcntl(1, F_GETFD)` でファイル記述子 1 を調べ、
//! 閉じていたことを [`STDOUT_WAS_CLOSED`] に記録する。
//! このフラグが立っていれば、[`Fd1`] の書き込みは `EBADF` で失敗し、最初の出力エラーの扱い(診断を書いて終了コード 74 で終了する)に進む。
//! 開き直した `/dev/null` はそのまま残すので、子プロセスのパイプがファイル記述子 1 に入ることはない。

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

/// 起動時(Rust のランタイムが開き直す前)にファイル記述子 1 が閉じていたら真。
pub static STDOUT_WAS_CLOSED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// `fcntl` のコマンド `F_GETFD` の値。
const F_GETFD: i32 = 1;

/// `errno` の `EBADF` の値。
const EBADF: i32 = 9;

/// `errno` の `EPIPE` の値。
const EPIPE: i32 = 32;

/// `errno` の `ECONNRESET` の値。
const ECONNRESET: i32 = 104;

/// ファイル記述子 1 が閉じていれば [`STDOUT_WAS_CLOSED`] を立てる。
///
/// `.init_array` から、Rust のランタイムの初期化より前に呼ばれる。
pub extern "C" fn probe_closed_stdout() {
    // SAFETY: fcntl(F_GETFD) はファイル記述子の状態を読むだけである。
    if unsafe { fcntl(1, F_GETFD) } == -1 {
        STDOUT_WAS_CLOSED.store(true, Ordering::Relaxed);
    }
}

#[used]
#[unsafe(link_section = ".init_array")]
static PROBE_CLOSED_STDOUT: extern "C" fn() = probe_closed_stdout;

/// バッファのバイト数(64 KiB)。
pub const BUFFER_BYTES: usize = 64 * 1024;

/// 標準出力への書き込みに失敗したことを表す出力エラー。
///
/// メッセージは OS のエラーメッセージで、Rust が付ける ` (os error N)` を取り除いてある。
/// 書き込みが 0 バイトを返したときのメッセージは `write returned 0` である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteError(pub String);

/// 書き込みの途中で起きた出力エラーの情報。
struct Failed {
    /// 呼び出し元に返す出力エラー。
    error: WriteError,
    /// 失敗するまでに書けたバイト数。
    written: usize,
    /// 未出力のバッファを捨てるエラー(`EBADF`、`EPIPE`、`ECONNRESET`)なら真。
    discard: bool,
}

/// バイト列の出力先。[`Out`] はこれを通して書き込む。
pub trait Sink {
    /// `buf` の先頭から書けるだけ書き、書いたバイト数を返す。
    fn write_some(&mut self, buf: &[u8]) -> std::io::Result<usize>;
}

/// ファイル記述子 1 に書く出力先。
///
/// 起動時にファイル記述子 1 が閉じていたときは、どの書き込みも `EBADF` で失敗する。
pub struct Fd1 {
    file: ManuallyDrop<File>,
    closed: bool,
}

impl Default for Fd1 {
    fn default() -> Self {
        // `.init_array` に置いた静的変数を参照して、それを含むオブジェクトファイルがリンクから外れないようにする。
        std::hint::black_box(&PROBE_CLOSED_STDOUT);
        Fd1 {
            // SAFETY: ファイル記述子 1 は Rust のランタイムが開いたままにしており、ManuallyDrop なので閉じない。
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

/// OS のエラーメッセージから、Rust が付ける ` (os error N)` を取り除く。
pub fn os_message(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) if s.ends_with(')') => s[..i].to_string(),
        _ => s,
    }
}

/// バッファ付きの標準出力。出力先 `S` へ、このモジュールの規約に従って書く。
pub struct Out<S: Sink = Fd1> {
    sink: S,
    buf: Vec<u8>,
    /// 真なら、[`Out::write`] に渡された文字列を捨てる([`Out::set_discard`])。
    discard: bool,
    /// 真なら、[`Out::eprint_after_flush`] が抑制の記録を標準出力に書く([`Out::set_log_to_stdout`])。
    log_to_stdout: bool,
}

impl Default for Out<Fd1> {
    fn default() -> Self {
        Out::new(Fd1::default())
    }
}

impl<S: Sink> Out<S> {
    /// `sink` へ書く標準出力を作る。
    pub fn new(sink: S) -> Out<S> {
        Out {
            sink,
            buf: Vec::with_capacity(BUFFER_BYTES),
            discard: false,
            log_to_stdout: false,
        }
    }

    /// `on` が真なら、それ以後 [`Out::write`] に渡す文字列を捨てる。
    ///
    /// すでにバッファにある出力は捨てない。
    pub fn set_discard(&mut self, on: bool) {
        self.discard = on;
    }

    /// `on` が真なら、[`Out::eprint_after_flush`] が抑制の記録を標準エラーではなく標準出力のバッファに追記する。
    ///
    /// 対話的な実行(REPL)で使う。
    /// REPL は、結果、診断、プログラムの出力をすべて標準出力に出すからである(Diktor の `--repl` の `cancel_log` と同じ)。
    pub fn set_log_to_stdout(&mut self, on: bool) {
        self.log_to_stdout = on;
    }

    /// 出力先を返す(テスト用)。
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
                    // 読み取る側のプロセスが追いつくのを待つ。待ち時間は 1 ms から始めて、続けて待つたびに 1 ms ずつ、50 ms まで延ばす。
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

    /// バッファを flush する。
    ///
    /// 失敗したときは、バッファを捨てるエラーならバッファを空にし、それ以外のエラーなら書けなかったバイトを残す。
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

    /// `Console.write` の文字列をバッファに追記する。バッファがいっぱいになったら flush する。
    pub fn write(&mut self, data: &[u8]) -> Result<(), WriteError> {
        if self.discard {
            return Ok(());
        }
        if self.buf.len() + data.len() < BUFFER_BYTES {
            self.buf.extend_from_slice(data);
            return Ok(());
        }
        // バッファがいっぱいになるまで追記して flush する。
        // 残りがバッファ 1 つ分以上あれば、そのうちバッファのサイズの整数倍に当たる先頭部分を、バッファを通さずに直接書く。
        let room = BUFFER_BYTES - self.buf.len();
        self.buf.extend_from_slice(&data[..room]);
        self.flush()?;
        let rest = &data[room..];
        let big = rest.len() - rest.len() % BUFFER_BYTES;
        if big > 0
            && let Err(f) = self.write_all(&rest[..big])
        {
            // バッファを捨てないエラーでは、書けなかったバイトを最大でバッファ 1 つ分だけ残す。
            // OCaml のチャネルがバッファに入れて残す量に合わせている。
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

    /// 標準出力を flush してから `msg` を標準エラーに書く。flush に失敗したら、`msg` を書かずにそのエラーを返す。
    ///
    /// 防壁(`cancel` 節の実行中に、伝播中の状態を退避して守る手順)が抑制した例外の記録に使う(IRSpec 9.6 節)。
    /// 前の失敗がバッファを捨てるエラーで、その後に何も追記していなければ、バッファは空なので flush は成功し、`msg` を書く。
    /// [`Out::set_log_to_stdout`] に真を渡してあるときは、先に flush することはせず、`msg` を [`Out::write`] で標準出力のバッファに追記する(バッファがいっぱいになれば [`Out::write`] が flush する)。
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

    /// 標準出力を flush してから `msg` を標準エラーに書く。flush の失敗は無視する。
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

    /// 書き込みの呼び出しを記録し、決めた回数だけ書き込みに成功した後は失敗する出力先。
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
        // バッファを捨てたので、何も追記しなければ flush は成功する。
        o.flush().unwrap();
        // 追記したものは次の flush で書き込みを試み、また失敗する。
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
        // 書けなかったバイトを残したので、何も追記しなくても次の flush はまた失敗する。
        assert!(o.flush().is_err());
        // バッファを通さずに直接書く大きな書き込みが失敗したときも、バッファ 1 つ分までを残す。
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
