//! サブコマンド `keleut dump tokens` と `keleut dump ast` である。
//!
//! | 書式 | 動作 |
//! |---|---|
//! | `keleut dump tokens [OPTIONS] FILE...` | ASI を適用した後のトークン列を出力する(`diktor --dump-tokens` と同じ形式) |
//! | `keleut dump ast [OPTIONS] FILE...` | 脱糖した AST を S 式で出力する(`diktor --dump-ast` と同じ形式) |
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--max-nest N`: 構文解析で作る木の深さの上限(既定は 200,000)。
//!   その上限に必要なスタックが [`MAX_STACK_BYTES`] を超える値は、使い方のエラーになる。
//! - `--native-stack BYTES`: フロントエンドを実行する専用のスレッドのスタックサイズ。
//!   `k`、`m`、`g` の接尾辞を使える。
//!   指定したスタックに `--max-nest` の深さが収まらなければ、収まる深さまで上限を下げる。
//!   [`MAX_STACK_BYTES`] を超える値、64 KiB より小さい値、1 段も収まらない値は、使い方のエラーになる。
//!
//! ファイルは並べた順に処理し、エラーのあったファイルで処理をやめる。
//! 終了コードは Diktor と同じで、字句エラー、構文エラー、構文解析の上限を超えたときは 2、入力を開けないときと使い方のエラーは 64、標準出力に書けないときは 74、内部エラーは 3 である。
//! 専用のスレッドを作れないときと、そのスレッドがパニックしたときは 70 である。
//!
//! 標準出力と標準エラーに書くタイミングも、Diktor(OCaml のチャネル)に合わせる。
//! cram は標準出力と標準エラーを 1 つのストリームにまとめて読むので、書くタイミングが変わると行の前後が入れ替わるからである。
//!
//! - 標準出力は 65,536 バイトのバッファを持ち、次の 3 つの時点で flush する。
//!   バッファがいっぱいになったとき、AST の各宣言を出力した後(`Format` の `@.`)、終了するときである。
//! - 字句エラー、入力を開けないとき、内部エラーの診断は標準エラーのバッファにため、終了するときに、標準出力の後で flush する。
//! - 構文エラーの診断は、書いてすぐに flush する(OCaml の `prerr_endline`)。

use crate::exit;
use keleut_front::{Files, Limits, ParseError, dump};
use std::io::Write;

/// `keleut dump tokens` と `keleut dump ast` の使い方の説明(usage)である。
pub const USAGE: &str = "\
usage: keleut dump tokens [OPTIONS] FILE...
       keleut dump ast [OPTIONS] FILE...
options:
  --max-nest N           parser tree depth limit (default 200000)
  --native-stack BYTES   stack size of the frontend thread (suffix k, m or g)
";

/// 専用のスレッドのスタックサイズの上限(8 GiB)である。
///
/// これより大きいスタックを指定すると、多くの環境でスレッドを作れない。
pub const MAX_STACK_BYTES: usize = 8 << 30;

/// OCaml のチャネルが持つバッファのサイズ(65,536 バイト)である。
const CHANNEL_BUFFER: usize = 65536;

/// OCaml の出力チャネルと同じタイミングで書く、バッファ付きの出力先である。
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

    /// バッファに追記する(OCaml の `caml_putblock` と同じ)。バッファがいっぱいになったら flush する。
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

/// 標準出力と標準エラーのチャネルの組である。
struct Out {
    stdout: Chan,
    stderr: Chan,
}

/// 処理を途中でやめる理由である。
enum Stop {
    /// 診断は [`Out`] に書いてある。値は終了コードである。
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

    /// 診断を標準エラーのバッファにためる(OCaml の `eprintf` と同じ)。
    fn eprint(&mut self, s: &str) {
        let _ = self.stderr.put(s.as_bytes());
    }

    /// 診断を 1 行書いて、すぐに flush する(OCaml の `prerr_endline` と同じ)。
    fn eprintln_flush(&mut self, s: &str) {
        let _ = self.stderr.put(s.as_bytes());
        let _ = self.stderr.put(b"\n");
        let _ = self.stderr.flush();
    }

    /// 標準出力、標準エラーの順に flush し、`code` をそのまま返す(Diktor の `safe_exit` と同じ順)。
    fn exit(mut self, code: u8) -> u8 {
        let _ = self.stdout.flush();
        let _ = self.stderr.flush();
        code
    }
}

/// I/O エラーのメッセージを返す。OCaml の `Sys_error` のメッセージに合わせて、末尾の `(os error N)` を取り除く。
pub(crate) fn io_msg(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// プロセスの開始時に標準出力(fd 1)が閉じていたら真である。
///
/// Rust の標準ライブラリは、`main` の前に、閉じている fd 0〜2 を `/dev/null` で開き直す。
/// そのため、`main` の中から書いてもエラーにならず、出力は何も報告されずに捨てられる。
/// Diktor(OCaml)は、同じ状況で `Bad file descriptor` のエラーになり、終了コード 74 で終了する。
/// これに合わせるために、標準ライブラリの初期化より前に実行される `.init_array` の関数で fd 1 を調べておく。
static STDOUT_CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
mod stdout_probe {
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    /// `fcntl` のコマンド `F_GETFD`(ファイル記述子フラグを読む)の値である。
    const F_GETFD: i32 = 1;

    extern "C" fn probe() {
        // SAFETY: fd 1 のファイル記述子フラグを読むだけで、何も変更しない。
        if unsafe { fcntl(1, F_GETFD) } == -1 {
            super::STDOUT_CLOSED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[used]
    #[unsafe(link_section = ".init_array")]
    static PROBE: extern "C" fn() = probe;
}

/// プロセスの開始時に標準出力が閉じていたら、書き込みエラー(`EBADF`)を返す。
pub(crate) fn stdout_closed() -> Option<std::io::Error> {
    STDOUT_CLOSED
        .load(std::sync::atomic::Ordering::Relaxed)
        .then(|| std::io::Error::from_raw_os_error(9))
}

/// コマンド行を解釈した結果である。
struct Options {
    files: Vec<String>,
    limits: Limits,
}

/// バイト数を読む。`k`、`m`、`g` の接尾辞を使える。数として読めない値と、64 KiB より小さい値は `None` になる。
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

/// 深さの上限 `n` に必要なスタック([`Limits::for_nest`] と同じ見積もり)が [`MAX_STACK_BYTES`] 以下なら真を返す。
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

/// 入力ファイルを読む。読めなければ、診断をバッファにためて、終了コード 64 の [`Stop::Exit`] を返す。
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

/// `keleut dump tokens …` のエントリポイントである。引数の先頭は `tokens` である。
pub fn tokens(args: &[String]) -> u8 {
    run(args, tokens_files)
}

/// `keleut dump ast …` のエントリポイントである。引数の先頭は `ast` である。
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
