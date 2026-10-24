//! 一時のディレクトリ、並行の実行、時間切れのある子プロセスである。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// 消し忘れない一時のディレクトリである。
/// 落とすと中身ごと消す(`keep` が立っていれば残す)。
pub struct TempDir {
    path: PathBuf,
    keep: bool,
}

static SEQ: AtomicUsize = AtomicUsize::new(0);

impl TempDir {
    /// `TMPDIR`(無ければ `/tmp`)の下に新しいディレクトリを作る。
    pub fn new(prefix: &str, keep: bool) -> std::io::Result<TempDir> {
        let base = std::env::temp_dir();
        loop {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{prefix}-{}-{n}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(TempDir { path, keep }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if self.keep {
            eprintln!(
                "kconform: 一時のディレクトリを残しました: {}",
                self.path.display()
            );
        } else {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// `items` の各要素に `f` を `jobs` 本のスレッドで当て、元の順の結果を返す。
pub fn par_map<T: Sync, R: Send>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1).min(items.len().max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= items.len() {
                        break;
                    }
                    let r = f(&items[i]);
                    results.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.expect("すべての要素を処理した"))
        .collect()
}

/// 子プロセスを、時間切れのある形で待つ。
///
/// 子プロセスは自分のプロセスグループで起動しておくこと(`process_group(0)`)。
/// 時間切れになったら、そのグループのすべてに SIGKILL を送り、`None` を返す。
pub fn wait_timeout(child: &mut Child, limit: Duration) -> std::io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    let mut sleep = Duration::from_millis(1);
    loop {
        if let Some(st) = child.try_wait()? {
            return Ok(Some(st));
        }
        if start.elapsed() >= limit {
            let pgid = child.id();
            let _ = Command::new("kill")
                .arg("-KILL")
                .arg("--")
                .arg(format!("-{pgid}"))
                .status();
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(sleep);
        sleep = (sleep * 2).min(Duration::from_millis(50));
    }
}

/// バイト列の中の `from` をすべて `to` に置き換える。
pub fn replace_all(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return hay.to_vec();
    }
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// 端末の色の指定を除く(dune 3.21.1 の `Ansi_color.strip` と同じ)。
///
/// ESC から、その後に最初に現れる `m` までを除く。
/// `m` が現れなければ、ESC から終わりまでを除く。
pub fn strip_ansi(hay: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i] == 0x1b {
            match hay[i + 1..].iter().position(|&b| b == b'm') {
                Some(k) => i += k + 2,
                None => break,
            }
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// シェルの誤りの文面の頭に付く、スクリプトの位置を除く(dune 3.21.1 の cram と同じ)。
///
/// 行の頭にある次の 2 つの形を、行ごとに 1 度だけ除く。
/// `N` は 0 で始まらない 10 進の数である。
///
/// - `<parent>: N: <command>: `(dash の形)
/// - `<command>: line N: `(bash の形)
pub fn strip_script_prefixes(hay: &[u8], parent: &[u8], command: &[u8]) -> Vec<u8> {
    fn line_number(s: &[u8]) -> Option<&[u8]> {
        let n = s.iter().take_while(|b| b.is_ascii_digit()).count();
        (n > 0 && s[0] != b'0').then(|| &s[n..])
    }
    fn dash(l: &[u8], parent: &[u8], command: &[u8]) -> Option<usize> {
        let r = l.strip_prefix(parent)?.strip_prefix(b": ")?;
        let r = line_number(r)?.strip_prefix(b": ")?;
        let r = r.strip_prefix(command)?.strip_prefix(b": ")?;
        Some(l.len() - r.len())
    }
    fn bash(l: &[u8], command: &[u8]) -> Option<usize> {
        let r = l.strip_prefix(command)?.strip_prefix(b": line ")?;
        let r = line_number(r)?.strip_prefix(b": ")?;
        Some(l.len() - r.len())
    }
    let mut out = Vec::with_capacity(hay.len());
    for l in hay.split_inclusive(|&b| b == b'\n') {
        let cut = dash(l, parent, command)
            .or_else(|| bash(l, command))
            .unwrap_or(0);
        out.extend_from_slice(&l[cut..]);
    }
    out
}

/// シェルの引数として安全な形にする(単一引用符で囲む)。
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 置き換え() {
        assert_eq!(replace_all(b"a/x/b/x", b"/x", b"$R"), b"a$R/b$R");
    }

    #[test]
    fn 並行の結果は元の順になる() {
        let xs: Vec<usize> = (0..100).collect();
        assert_eq!(
            par_map(&xs, 8, |x| x * 2),
            xs.iter().map(|x| x * 2).collect::<Vec<_>>()
        );
    }

    #[test]
    fn 色の指定を除く() {
        assert_eq!(
            strip_ansi(b"\x1b[31mred\x1b[0m \x1b[1;32mb\x1b[0m\n"),
            b"red b\n"
        );
        assert_eq!(strip_ansi(b"a\x1b[31"), b"a");
    }

    #[test]
    fn スクリプトの位置を除く() {
        let f = |s: &[u8]| strip_script_prefixes(s, b"/m/main.sh", b"/m/0.sh");
        assert_eq!(
            f(b"/m/0.sh: line 1: x: command not found\n"),
            b"x: command not found\n"
        );
        assert_eq!(
            f(b"/m/main.sh: 12: /m/0.sh: x: not found\n"),
            b"x: not found\n"
        );
        assert_eq!(f(b"ok\n/m/0.sh: line 2: y\n"), b"ok\ny\n");
        assert_eq!(f(b"/m/0.sh: line 0: y\n"), b"/m/0.sh: line 0: y\n");
        assert_eq!(f(b" /m/0.sh: line 1: y"), b" /m/0.sh: line 1: y");
        assert_eq!(
            f(b"/m/0.sh: line 1: /m/0.sh: line 1: y"),
            b"/m/0.sh: line 1: y"
        );
    }

    #[test]
    fn 引用() {
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }
}
