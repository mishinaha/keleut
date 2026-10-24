//! 標準出力の規約(計画 261024-1 の 6.5 節、D530)。
//!
//! - `Console.write` の文字列を 64 KiB のバッファに足す。
//! - flush するのは、バッファが満ちたとき、標準エラーへ書く直前、終了の前である。
//! - 書き込みは短い書き込みを繰り返す。
//!   `EINTR` はすぐに、`EAGAIN`(非ブロッキングのパイプ)は少し待ってから再試行する。
//! - 最初の書き込みの失敗で、未出力のバッファを捨て、失敗の印を付ける。
//!   その後の書き込みと flush は何もせず、誤りも起こさない。
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
    failed: bool,
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
            failed: false,
        }
    }

    /// 失敗の印が付いているか。
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// 出力先(試験用)。
    pub fn sink(&self) -> &S {
        &self.sink
    }

    fn write_all(&mut self, mut data: &[u8]) -> Result<(), WriteError> {
        let mut waits = 0u32;
        while !data.is_empty() {
            match self.sink.write_some(data) {
                Ok(0) => {
                    return Err(WriteError("write returned 0".to_string()));
                }
                Ok(n) => {
                    data = &data[n..];
                    waits = 0;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // 読み手が追いつくのを待つ。待ち時間は 1 ms から 50 ms まで延ばす。
                    waits = waits.saturating_add(1);
                    let ms = u64::from(waits.min(50));
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                Err(e) => return Err(WriteError(os_message(&e))),
            }
        }
        Ok(())
    }

    fn fail(&mut self, e: WriteError) -> Result<(), WriteError> {
        self.failed = true;
        self.buf = Vec::new();
        Err(e)
    }

    /// バッファを書き出す。
    pub fn flush(&mut self) -> Result<(), WriteError> {
        if self.failed || self.buf.is_empty() {
            return Ok(());
        }
        let buf = std::mem::take(&mut self.buf);
        let r = self.write_all(&buf);
        self.buf = buf;
        self.buf.clear();
        match r {
            Ok(()) => Ok(()),
            Err(e) => self.fail(e),
        }
    }

    /// `Console.write` の文字列を足す。満ちたら書き出す。
    pub fn write(&mut self, data: &[u8]) -> Result<(), WriteError> {
        if self.failed {
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
            && let Err(e) = self.write_all(&rest[..big])
        {
            return self.fail(e);
        }
        self.buf.extend_from_slice(&rest[big..]);
        if self.buf.len() >= BUFFER_BYTES {
            self.flush()?;
        }
        Ok(())
    }

    /// 標準出力を flush してから(失敗は印を付けるだけで捨てる)、`msg` を標準エラーに書く。
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
        block_every: usize,
        calls: usize,
    }

    impl Sink for Rec {
        fn write_some(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if let Some(n) = self.fail_after
                && self.writes.len() >= n
            {
                return Err(std::io::Error::from_raw_os_error(28));
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
    fn 最初の失敗の後は何もしない() {
        let mut o = Out::new(Rec {
            fail_after: Some(0),
            ..Rec::default()
        });
        o.write(b"hello").unwrap();
        let e = o.flush().unwrap_err();
        assert_eq!(e.0, "No space left on device");
        assert!(o.failed());
        o.write(&vec![0u8; BUFFER_BYTES * 3]).unwrap();
        o.flush().unwrap();
        assert!(o.sink().data.is_empty());
    }
}
