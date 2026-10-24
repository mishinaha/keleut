//! `keleut dump tokens` と `keleut dump ast` である(計画 261024-1 の 6.2 節、M96)。
//!
//! | 形 | 働き |
//! |---|---|
//! | `keleut dump tokens [OPTIONS] FILE...` | ASI を適用した後のトークン列を印字する(`diktor --dump-tokens` と同じ形) |
//! | `keleut dump ast [OPTIONS] FILE...` | 脱糖した AST を S 式で印字する(`diktor --dump-ast` と同じ形) |
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--max-nest N`: 構文解析で作る木の深さの限度(既定は 200,000)。
//!   限度に見合うスタックが [`MAX_STACK_BYTES`] を超える値は、使い方の誤りにする。
//! - `--native-stack BYTES`: フロントエンドを走らせる専用のスレッドのスタックの大きさ。
//!   `k`、`m`、`g` の接尾辞を使える。与えたスタックに `--max-nest` が収まらなければ、限度を下げる。
//!   [`MAX_STACK_BYTES`] を超える値は、使い方の誤りにする。
//!
//! ファイルは並べた順に処理し、誤りのあったファイルで止まる。
//! 終了状態は Diktor と同じで、字句と構文の誤りと構文解析の限度は 2、
//! 入力を開けないときと使い方の誤りは 64、標準出力に書けないときは 74、内部の誤りは 3 である。
//! 専用のスレッドを作れないときと、その中で落ちたときは 70 である。
//!
//! 標準出力と標準エラーへ書く時機も Diktor(OCaml のチャネル)に合わせる。
//! cram は 2 つを 1 本にして読むので、書く時機が変わると行の前後が入れ替わる。
//!
//! - 標準出力は 65,536 バイトの緩衝で、満ちたときと、AST の各宣言を印字した後
//!   (`Format` の `@.`)と、終わるときに書き出す。
//! - 字句エラーと入力の誤りの診断は緩衝に溜め、終わるときに標準出力の後に書き出す。
//! - 構文エラーの診断は、その場で書き出す(OCaml の `prerr_endline`)。

use crate::exit;
use keleut_front::{Files, Limits, ParseError, dump};
use std::io::Write;

/// 使い方である。
pub const USAGE: &str = "\
usage: keleut dump tokens [OPTIONS] FILE...
       keleut dump ast [OPTIONS] FILE...
options:
  --max-nest N           parser tree depth limit (default 200000)
  --native-stack BYTES   stack size of the frontend thread (suffix k, m or g)
";

/// 専用のスレッドのスタックの大きさの上限(8 GiB)である。
///
/// これより大きいスタックは、多くの環境でスレッドを作れない。
pub const MAX_STACK_BYTES: usize = 8 << 30;

/// OCaml のチャネルの緩衝の大きさである。
const CHANNEL_BUFFER: usize = 65536;

/// OCaml の出力チャネルの写しである。
struct Chan {
    fd: i32,
    buf: Vec<u8>,
}

impl Chan {
    fn new(fd: i32) -> Chan {
        Chan {
            fd,
            buf: Vec::with_capacity(CHANNEL_BUFFER),
        }
    }

    fn raw_write(&self, data: &[u8]) -> std::io::Result<()> {
        if self.fd == 1 {
            let mut o = std::io::stdout().lock();
            o.write_all(data)?;
            o.flush()
        } else {
            let mut e = std::io::stderr().lock();
            e.write_all(data)?;
            e.flush()
        }
    }

    /// 緩衝に書く(`caml_putblock`)。満ちたら書き出す。
    fn put(&mut self, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            let free = CHANNEL_BUFFER - self.buf.len();
            if data.len() < free {
                self.buf.extend_from_slice(data);
                return Ok(());
            }
            self.buf.extend_from_slice(&data[..free]);
            data = &data[free..];
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let r = self.raw_write(&self.buf);
        self.buf.clear();
        r
    }
}

/// 標準出力と標準エラーの組である。
struct Out {
    stdout: Chan,
    stderr: Chan,
}

/// 処理を止める理由である。
enum Stop {
    /// 診断を書き終えた。終了状態を持つ。
    Exit(u8),
    /// 標準出力に書けない。
    Output(std::io::Error),
}

impl Out {
    fn new() -> Out {
        Out {
            stdout: Chan::new(1),
            stderr: Chan::new(2),
        }
    }

    fn out(&mut self, data: &[u8]) -> Result<(), Stop> {
        self.stdout.put(data).map_err(Stop::Output)
    }

    fn flush_out(&mut self) -> Result<(), Stop> {
        self.stdout.flush().map_err(Stop::Output)
    }

    /// 緩衝に溜める診断(OCaml の `eprintf`)。
    fn eprint(&mut self, s: &str) {
        let _ = self.stderr.put(s.as_bytes());
    }

    /// その場で書き出す診断(OCaml の `prerr_endline`)。
    fn eprintln_flush(&mut self, s: &str) {
        let _ = self.stderr.put(s.as_bytes());
        let _ = self.stderr.put(b"\n");
        let _ = self.stderr.flush();
    }

    /// 終わる(Diktor の `safe_exit`)。標準出力、標準エラーの順に書き出す。
    fn exit(mut self, code: u8) -> u8 {
        let _ = self.stdout.flush();
        let _ = self.stderr.flush();
        code
    }
}

/// I/O の誤りの文面(OCaml の `Sys_error` の文面と同じく、末尾の `(os error N)` を除く)。
pub(crate) fn io_msg(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// プロセスが始まったときに標準出力(fd 1)が閉じていたかどうかである。
///
/// Rust の標準ライブラリは `main` の前に、閉じている fd 0〜2 を `/dev/null` で開き直す。
/// そのため `main` の中から書いても誤りにならず、出力が黙って捨てられる。
/// Diktor(OCaml)は同じとき `Bad file descriptor` で 74 で終わるので、
/// 標準ライブラリの初期化より前に走る `.init_array` の関数で fd 1 を調べておく。
static STDOUT_CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
mod stdout_probe {
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    /// `F_GETFD` である。
    const F_GETFD: i32 = 1;

    extern "C" fn probe() {
        // SAFETY: fd 1 の記述子の旗を読むだけで、何も変えない。
        if unsafe { fcntl(1, F_GETFD) } == -1 {
            super::STDOUT_CLOSED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[used]
    #[unsafe(link_section = ".init_array")]
    static PROBE: extern "C" fn() = probe;
}

/// プロセスが始まったときに標準出力が閉じていたなら、書けない誤り(`EBADF`)を返す。
pub(crate) fn stdout_closed() -> Option<std::io::Error> {
    STDOUT_CLOSED
        .load(std::sync::atomic::Ordering::Relaxed)
        .then(|| std::io::Error::from_raw_os_error(9))
}

/// 解釈したコマンド行である。
struct Options {
    files: Vec<String>,
    limits: Limits,
}

/// `k`、`m`、`g` の接尾辞を使えるバイト数を読む。64 KiB より小さい値は受けない。
pub(crate) fn parse_bytes(s: &str) -> Option<usize> {
    let (num, mul) = match s.as_bytes().last()? {
        b'k' | b'K' => (&s[..s.len() - 1], 1usize << 10),
        b'm' | b'M' => (&s[..s.len() - 1], 1 << 20),
        b'g' | b'G' => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    let v = num.parse::<usize>().ok()?.checked_mul(mul)?;
    (v >= 64 << 10).then_some(v)
}

/// 深さの限度 `n` に見合うスタック([`Limits::for_nest`] と同じ見積もり)が上限に収まるか。
fn nest_fits(n: usize) -> bool {
    n.checked_mul(keleut_front::STACK_BYTES_PER_LEVEL)
        .and_then(|b| b.checked_add(32 << 20))
        .is_some_and(|b| b <= MAX_STACK_BYTES)
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut files = Vec::new();
    let mut max_nest = keleut_front::DEFAULT_MAX_NEST;
    let mut stack = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--max-nest" => {
                i += 1;
                let v = args.get(i).ok_or("--max-nest needs a value")?;
                max_nest = match v.parse::<usize>() {
                    Ok(n) if n >= 1 && nest_fits(n) => n,
                    _ => return Err(format!("bad --max-nest value: {v}")),
                };
            }
            "--native-stack" => {
                i += 1;
                let v = args.get(i).ok_or("--native-stack needs a value")?;
                stack = Some(
                    parse_bytes(v)
                        .filter(|&b| b <= MAX_STACK_BYTES)
                        .ok_or(format!("bad --native-stack value: {v}"))?,
                );
            }
            _ if a.starts_with('-') && a.len() > 1 => return Err(format!("unknown option: {a}")),
            _ => files.push(a.to_string()),
        }
        i += 1;
    }
    if files.is_empty() {
        return Err("no input files".to_string());
    }
    let limits = match stack {
        Some(s) => Limits::with_stack(max_nest, s),
        None => Limits::for_nest(max_nest),
    };
    if limits.max_nest == 0 {
        return Err("--native-stack is too small".to_string());
    }
    Ok(Options { files, limits })
}

/// 入力のファイルを読む。読めなければ診断を溜めて 64 で止まる。
fn read_input(out: &mut Out, path: &str) -> Result<Vec<u8>, Stop> {
    std::fs::read(path).map_err(|e| {
        out.eprint(&format!(
            "keleut: ファイルを開けません: {path}: {}\n",
            io_msg(&e)
        ));
        Stop::Exit(exit::USAGE)
    })
}

fn tokens_files(out: &mut Out, o: &Options) -> Result<(), Stop> {
    let mut files = Files::new();
    for path in &o.files {
        let src = read_input(out, path)?;
        let id = files.add(path);
        match keleut_front::tokens(&src, id) {
            Ok(toks) => out.out(&dump::tokens_text(&toks))?,
            Err(e) => {
                out.eprint(&format!("{}: 字句エラー: {}\n", files.show(e.pos), e.msg));
                return Err(Stop::Exit(2));
            }
        }
    }
    out.flush_out()
}

fn ast_files(out: &mut Out, o: &Options) -> Result<(), Stop> {
    let mut files = Files::new();
    for path in &o.files {
        let src = read_input(out, path)?;
        let id = files.add(path);
        match keleut_front::parse(&src, id, &files, o.limits.max_nest) {
            Ok(prog) => {
                out.out(&dump::import_lines(&prog.imports))?;
                let mut f = keleut_front::oformat::Formatter::new();
                for d in &prog.decls {
                    let text = dump::decl_text(&mut f, d);
                    out.out(&text)?;
                    out.flush_out()?;
                }
                out.flush_out()?;
            }
            Err(ParseError::Lex(e)) => {
                out.eprint(&format!("{}: 字句エラー: {}\n", files.show(e.pos), e.msg));
                return Err(Stop::Exit(2));
            }
            Err(ParseError::Syntax(msg)) => {
                out.eprintln_flush(&msg);
                return Err(Stop::Exit(2));
            }
            Err(ParseError::Internal(msg)) => {
                out.eprint(&format!("内部エラー: 予期しない例外です: {msg}\n"));
                return Err(Stop::Exit(3));
            }
        }
    }
    out.flush_out()
}

fn run(args: &[String], body: fn(&mut Out, &Options) -> Result<(), Stop>) -> u8 {
    let o = match parse_options(&args[1..]) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    let stack = o.limits.stack_bytes;
    let r = keleut_front::on_stack(stack, move || {
        let mut out = Out::new();
        match body(&mut out, &o) {
            Ok(()) => out.exit(0),
            Err(Stop::Exit(c)) => out.exit(c),
            Err(Stop::Output(e)) => {
                out.stdout.buf.clear();
                out.eprint(&format!(
                    "keleut: 標準出力に書き出せません: {}\n",
                    io_msg(&e)
                ));
                out.exit(exit::IO)
            }
        }
    });
    match r {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("keleut: {msg}");
            70
        }
    }
}

/// `keleut dump tokens …`(引数の先頭は `tokens`)。
pub fn tokens(args: &[String]) -> u8 {
    run(args, tokens_files)
}

/// `keleut dump ast …`(引数の先頭は `ast`)。
pub fn ast(args: &[String]) -> u8 {
    run(args, ast_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(xs: &[&str]) -> Result<Options, String> {
        parse_options(&xs.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn スタックが上限を超える限度は使い方の誤りになる() {
        assert!(opts(&["--max-nest", "1000000000000", "a.kel"]).is_err());
        assert!(opts(&["--max-nest", "18446744073709551615", "a.kel"]).is_err());
        assert!(opts(&["--native-stack", "9g", "a.kel"]).is_err());
        let o = opts(&["--max-nest", "1000", "a.kel"]).unwrap();
        assert_eq!(o.limits.max_nest, 1000);
        assert!(opts(&["--native-stack", "8g", "a.kel"]).is_ok());
    }
}
