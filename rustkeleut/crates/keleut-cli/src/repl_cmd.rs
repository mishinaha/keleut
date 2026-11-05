//! `keleut repl` である(計画 261024-1 の 6.2 節。M100)。
//!
//! | 形 | 働き |
//! |---|---|
//! | `keleut repl [OPTIONS]` | 標準入力から入力を読み、入力ごとに型検査して実行し、結果を表示する(`diktor --repl` と同じ出力) |
//!
//! 入力の区切り、コマンド(`:quit`、`:q`、`:reset`、`:type 式`)、結果の表示、診断の文面は、
//! Diktor の driver.ml の §16.6b と同じである。
//! 結果、診断、プログラムの出力は、すべて標準出力に出す。
//! プロンプトは、標準入力が端末のときだけ出す。
//! 入力の終わりか `:quit` で 0 で終わり、型エラーや実行時エラーではセッションを終えない。
//!
//! 型検査は keleut-front の [`Session`] が行い、型検査に通った入力ごとに KIR の単位を 1 つ、
//! セッションのプログラムの後ろに足す。
//! 実行はインタプリタで行い、実行時の状態(ヒープ、大域、インスタンス、形の表)はセッションの間ずっと持つ。
//! 入力ごとに、足した単位だけを検証し(`kir::verify_extension`)、LIR に下ろして LIR のプログラムの後ろに足し
//! (`keleut_lir::Extender`)、実行時の表を伸ばして([`Rt::grow`])、その単位の初期化だけを実行する。
//! 前の入力の大域は、足した単位から同じ添字で読む。
//! 1 つの入力の手間は、その入力の大きさにほぼ比例し、前の入力の数にはほぼ依らない
//! (宣言したクラスの数と、module の中の値の数に比例する部分が残る。Diktor も同じである)。
//! 実行は専用のスレッド(`keleut-mutator`)で行い、型検査のスレッドとは依頼と返事でやりとりする。
//! 始めるとき(と `:reset` の後)に、プレリュードの初期化を 1 回実行する。
//! 止まれば、Diktor と同じく診断を標準エラーに書いて、その誤りの終了状態(実行時エラーなら 3)で終わる。
//! 実行時エラーで止まった入力が止まるまでに書いた出力は残す(LangSpec §6.6、§18.5)。
//! その入力は捨てる(後の入力から名前が見えない)。
//! その入力の単位はプログラムに残すが、単位が足した大域とインスタンスの値は捨て、回収の根から外す。
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--no-prelude`、`--prelude PATH`:`keleut check` の同名の旗と同じ。
//! - `--max-nest N`、`--native-stack BYTES`:構文解析の深さの限度と、フロントエンドのスレッドのスタック(`keleut check` と同じ)。
//! - `--max-depth N`、`--gc-stress`、`--no-yield-prune`:`keleut run` の同名の旗と同じ(インタプリタのスタックは `--max-depth` から決める)。
//! - `--backend B`、`--tier MODE`、`--frontend F`、`--strict-exhaustive`:kconform の組の選択肢と `diktor --repl` の旗を受けるためのもので、効果は無い
//!   (入力は後段に関係なくインタプリタで実行する)。
//!
//! 標準入力が閉じていれば(Rust の実行時が `/dev/null` で開き直す前に調べる)、診断を標準エラーに書いて 74 で終わる。

use crate::exit;
use crate::front_cmd::{MAX_STACK_BYTES, parse_bytes};
use keleut_front::types::check::{Stop as FrontStop, check_limits, on_check_stack};
use keleut_front::types::session::{
    Finish, Layout, Layouts, Outcome, Parsed, Session, SessionOptions, Shown, StartError,
};
use keleut_interp::Interp;
use keleut_rt::ctx::display_name;
use keleut_rt::effect::Pending;
use keleut_rt::value::{self, ImmKind, Kind};
use keleut_rt::{Rt, Stop, Word};
use std::io::{BufRead, IsTerminal, Write};
use std::sync::mpsc;

/// 使い方である。
pub const USAGE: &str = "\
usage: keleut repl [OPTIONS]
options:
  --no-prelude           do not load the prelude
  --prelude PATH         use PATH as the prelude
  --max-nest N           parser tree depth limit (default 200000)
  --native-stack BYTES   stack size of the front-end thread (suffix k, m or g; default 8g;
                         lowers --max-nest to what fits)
  --max-depth N          logical depth limit L of the interpreter (default 1048576)
  --gc-stress            collect garbage at short intervals (for testing)
  --no-yield-prune       treat every resume point as able to receive a yield
  --backend B, --tier MODE, --frontend F, --strict-exhaustive
                         accepted for kconform and diktor --repl; no effect
                         (inputs always run on the interpreter)
";

/// プロセスが始まったときに標準入力(fd 0)が閉じていたかどうかである。
///
/// Rust の標準ライブラリは `main` の前に、閉じている fd 0〜2 を `/dev/null` で開き直すので、
/// `main` の中から読むと入力の終わりになる。
/// Diktor(OCaml)は同じとき `Bad file descriptor` で 74 で終わるので、
/// 標準ライブラリの初期化より前に走る `.init_array` の関数で fd 0 を調べておく
/// (`front_cmd` の標準出力の調べと同じ)。
static STDIN_CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
mod stdin_probe {
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    /// `F_GETFD` である。
    const F_GETFD: i32 = 1;

    extern "C" fn probe() {
        // SAFETY: fd 0 の記述子の旗を読むだけで、何も変えない。
        if unsafe { fcntl(0, F_GETFD) } == -1 {
            super::STDIN_CLOSED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[used]
    #[unsafe(link_section = ".init_array")]
    static PROBE: extern "C" fn() = probe;
}

#[derive(Clone)]
struct Options {
    session: SessionOptions,
    front_stack: Option<usize>,
    max_depth: u64,
    gc_stress: bool,
    yield_prune: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut max_nest = keleut_front::DEFAULT_MAX_NEST;
    let mut front_stack = None;
    let mut no_prelude = false;
    let mut prelude = None;
    let mut max_depth = keleut_rt::DEFAULT_MAX_DEPTH;
    let mut gc_stress = false;
    let mut yield_prune = true;
    let mut files = 0usize;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "--no-prelude" => no_prelude = true,
            "--prelude" => prelude = Some(value(a)?),
            "--max-nest" => {
                let v = value(a)?;
                max_nest = match v.parse::<usize>() {
                    Ok(n) if n >= 1 && n <= u32::MAX as usize => n,
                    _ => return Err(format!("bad --max-nest value: {v}")),
                };
            }
            "--native-stack" => {
                let v = value(a)?;
                front_stack = Some(
                    parse_bytes(&v)
                        .filter(|&b| b <= MAX_STACK_BYTES)
                        .ok_or(format!("bad --native-stack value: {v}"))?,
                );
            }
            "--max-depth" => {
                let v = value(a)?;
                max_depth = match v.parse::<u64>() {
                    Ok(n) if n >= 1 => n,
                    _ => return Err(format!("bad --max-depth value: {v}")),
                };
            }
            "--gc-stress" => gc_stress = true,
            "--no-yield-prune" => yield_prune = false,
            // kconform の組の選択肢と `diktor --repl` の旗。受けて捨てる。
            "--backend" | "--frontend" | "--tier" => {
                value(a)?;
            }
            "--strict-exhaustive" => {}
            _ if a.starts_with('-') && a.len() > 1 => return Err(format!("unknown option: {a}")),
            _ => files += 1,
        }
        i += 1;
    }
    if files > 0 {
        return Err("repl takes no input files".to_string());
    }
    let limits = check_limits(max_nest, front_stack)?;
    Ok(Options {
        session: SessionOptions {
            no_prelude,
            prelude,
            max_nest: limits.max_nest,
        },
        front_stack,
        max_depth,
        gc_stress,
        yield_prune,
    })
}

/// 標準出力。書けなくなったら、その誤りを持つ(Diktor の `flush_stdout_or_die`)。
struct Output {
    buf: Vec<u8>,
}

impl Output {
    fn line(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(b'\n');
    }

    fn bytes(&mut self, s: &[u8]) {
        self.buf.extend_from_slice(s);
    }

    /// 溜めた出力を書き出す。書けなければ診断を標準エラーに書いて 74 を返す。
    fn flush(&mut self) -> Result<(), u8> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let mut out = std::io::stdout().lock();
        let r = out.write_all(&self.buf).and_then(|()| out.flush());
        self.buf.clear();
        r.map_err(|e| {
            eprintln!(
                "keleut: 標準出力に書き出せません: {}",
                crate::front_cmd::io_msg(&e)
            );
            exit::IO
        })
    }
}

/// 1 つの入力を実行した結果である。
enum Exec {
    /// 最後まで実行し、結果を表示した。
    Done,
    /// 実行時エラーで止まった(診断は標準出力に書いた)。
    Failed,
    /// 標準出力に書けない(診断は標準エラーに書いた)。
    Io,
    /// プレリュードの初期化が止まったか、
    /// KIR の検証、LIR への下ろし、実行時の表の準備が内部の誤りか資源の不足で失敗したか、
    /// 実行のスレッドを作れないか落ちた。
    /// 診断は標準エラーに書いた。セッションをこの終了状態で終える。
    Exit(u8),
}

/// 実行の種類である。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 始めるときのプレリュードの初期化。止まればセッションを終える。
    Start,
    /// 入力の実行。足した単位(この入力)の初期化を実行し、結果を表示する。
    Input,
}

/// `keleut repl …`(引数の先頭は `repl`)。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return 0;
    }
    let o = match parse_options(rest) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    let limits = match check_limits(o.session.max_nest, o.front_stack) {
        Ok(l) => l,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    // 入力の種類は書かない。変換の役割の確保の失敗で REPL を終えない(`keleut_rt::alloc`)。
    match on_check_stack(limits, || {
        keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Conv);
        session_loop(&o)
    }) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("keleut: {msg}");
            70
        }
    }
}

/// 行末の空白と CR を除く(コマンドの行)。
fn trim_command(line: &[u8]) -> &[u8] {
    let mut n = line.len();
    while n > 0 && matches!(line[n - 1], b' ' | b'\t' | b'\r') {
        n -= 1;
    }
    &line[..n]
}

/// 対話的な実行の本体(Diktor の `repl` の `loop`)。型検査のスレッドの中で呼ぶ。
fn session_loop(o: &Options) -> u8 {
    let mut sess = match Session::new(o.session.clone()) {
        Ok(s) => s,
        Err(e) => return front_stop(e),
    };
    let mut out = Output { buf: Vec::new() };
    let mut live = match start_run(o, &mut sess, &mut out) {
        Ok(l) => l,
        Err(c) => return c,
    };
    if STDIN_CLOSED.load(std::sync::atomic::Ordering::Relaxed) {
        let e = std::io::Error::from_raw_os_error(9);
        eprintln!(
            "keleut: 標準入力を読めません: {}",
            crate::front_cmd::io_msg(&e)
        );
        return exit::IO;
    }
    let interactive = std::io::stdin().is_terminal();
    let mut input = std::io::stdin().lock();
    let mut buf: Vec<u8> = Vec::new();
    let mut line_no: u32 = 0;
    let mut start_line: u32 = 1;
    let mut line: Vec<u8> = Vec::new();
    loop {
        // 前の入力が大域の確保器の予備を使っていれば(実行時エラーで止まった入力など)、次の入力の前に取り直す。
        // 取れなければ予備なしで続ける(`keleut_rt::alloc`)。
        keleut_rt::alloc::reacquire();
        if interactive {
            out.bytes(if buf.is_empty() { b"> " } else { b". " });
            if let Err(c) = out.flush() {
                return c;
            }
        }
        line.clear();
        let n = match input.read_until(b'\n', &mut line) {
            Ok(n) => n,
            Err(e) => {
                let _ = out.flush();
                eprintln!(
                    "keleut: 標準入力を読めません: {}",
                    crate::front_cmd::io_msg(&e)
                );
                return exit::IO;
            }
        };
        if n == 0 {
            if !buf.is_empty() {
                if interactive {
                    out.bytes(b"\n");
                }
                match sess.parse(&buf, start_line, 0) {
                    Parsed::Failed(m) => out.line(&m),
                    _ => out.line("<stdin>: 入力が途中で終わっています"),
                }
            } else if interactive {
                out.bytes(b"\n");
            }
            return match out.flush() {
                Ok(()) => 0,
                Err(c) => c,
            };
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        line_no += 1;
        if buf.is_empty() && line.first() == Some(&b':') {
            let cmd = trim_command(&line);
            if cmd == b":quit" || cmd == b":q" {
                return match out.flush() {
                    Ok(()) => 0,
                    Err(c) => c,
                };
            } else if cmd == b":reset" {
                // 前の実行時の状態を捨ててから、プレリュードだけのセッションを始め直す。
                drop(live);
                if let Err(e) = sess.reset() {
                    let _ = out.flush();
                    return front_stop(e);
                }
                live = match start_run(o, &mut sess, &mut out) {
                    Ok(l) => l,
                    Err(c) => return c,
                };
            } else if cmd.len() > 6 && &cmd[..6] == b":type " {
                let s = sess.type_of(&cmd[6..], line_no, 6);
                out.line(&s);
            } else {
                out.bytes("未知のコマンド: ".as_bytes());
                out.bytes(cmd);
                out.bytes(b"\n");
            }
        } else {
            if buf.is_empty() {
                start_line = line_no;
            }
            buf.extend_from_slice(&line);
            buf.push(b'\n');
            match sess.parse(&buf, start_line, 0) {
                Parsed::Incomplete => {}
                Parsed::Failed(m) => {
                    buf.clear();
                    out.line(&m);
                }
                Parsed::Complete(ds) => {
                    buf.clear();
                    match process(&mut live, &mut sess, ds, &mut out) {
                        Ok(()) => {}
                        Err(c) => return c,
                    }
                }
            }
        }
        if let Err(c) = out.flush() {
            return c;
        }
    }
}

/// フロントエンドがセッションを始められなかったときの診断と終了状態。
fn front_stop(e: StartError) -> u8 {
    match e {
        StartError::Stop(FrontStop::Input(m)) => {
            eprintln!("keleut: ファイルを開けません: {m}");
            exit::USAGE
        }
        StartError::Stop(FrontStop::Lex(m) | FrontStop::Syntax(m)) => {
            eprintln!("{m}");
            2
        }
        StartError::Type(m) => {
            eprintln!("{m}");
            1
        }
        StartError::Stop(FrontStop::Internal(m)) => {
            eprintln!("{m}");
            3
        }
        StartError::Emit(m) => {
            eprintln!("keleut: 内部エラー: プレリュードを初期化できません: {m}");
            70
        }
    }
}

/// 実行のスレッドへの依頼。
struct Job {
    /// セッションのプログラム。最後の単位(始めるときはプレリュードまでの単位)を足したもの。
    program: Box<kir::Program>,
    shown: Vec<Shown>,
    layouts: Layouts,
    mode: Mode,
}

/// 実行のスレッドからの返事。
struct Reply {
    /// 依頼で渡したプログラム(型検査の側に返す)。
    program: Box<kir::Program>,
    exec: Exec,
    /// LIR に下ろして足した関数の数と、LIR の関数の総数(`KELEUT_REPL_STATS`)。
    lir: (usize, usize),
}

/// 生きている実行時の状態を持つ、実行のスレッドとの窓口である。
///
/// 捨てると依頼の口を閉じ、スレッドの終わりを待つ(ヒープと表を手放す)。
struct Live {
    jobs: Option<mpsc::Sender<Job>>,
    replies: mpsc::Receiver<Reply>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// 足した単位の KIR の検証の状態。
    verified: kir::verify::Extension,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// 実行の依頼の結果。
enum Ran {
    /// 実行した(またはセッションを終える誤りで止まった)。
    Ran(Exec),
    /// 入れ子が KIR の限度 `N_nest` を超えたので、実行しなかった。標準出力に書く 1 行の診断。
    Limit(String),
}

impl Live {
    /// 実行のスレッドを作る。作れなければ、診断を標準エラーに書いて 70 を返す(`keleut run` と同じ)。
    fn spawn(o: &Options) -> Result<Live, u8> {
        let native = keleut_rt::native_stack_for(o.max_depth);
        let size = usize::try_from(native).unwrap_or(usize::MAX);
        let (jtx, jrx) = mpsc::channel::<Job>();
        let (rtx, rrx) = mpsc::channel::<Reply>();
        let oc = o.clone();
        let h = std::thread::Builder::new()
            .name("keleut-mutator".to_string())
            .stack_size(size)
            .spawn(move || mutator(&oc, native, jrx, rtx));
        match h {
            Ok(t) => Ok(Live {
                jobs: Some(jtx),
                replies: rrx,
                thread: Some(t),
                verified: kir::verify::Extension::new(),
            }),
            Err(e) => {
                eprintln!(
                    "keleut: 内部エラー: 実行のスレッドを作れません({size} バイトのスタック): {e}"
                );
                Err(70)
            }
        }
    }

    /// プログラムに足した単位を検証し、実行のスレッドで実行する。
    ///
    /// 入れ子が KIR の限度 `N_nest` を超えた単位は、処理系の限度として実行しない([`Ran::Limit`])。
    /// `keleut run --frontend rust` が KIR の書き出しで止まるのと同じ限度である。
    /// それ以外の検証の失敗は内部の誤りなので、診断を標準エラーに書いて [`Exec::Exit`] の 70 を返す。
    /// プログラムは、どの場合も返す。
    fn run(
        &mut self,
        program: Box<kir::Program>,
        shown: Vec<Shown>,
        layouts: Layouts,
        mode: Mode,
    ) -> (Box<kir::Program>, Ran) {
        let limits = kir::Limits::default();
        let before = self.verified.done();
        if let Err(e) = kir::verify_extension(&mut self.verified, &program, &limits) {
            if e.kind() == kir::ErrorKind::Limit {
                // 書き手の診断を優先する(`keleut run` の `program_for_run` と同じ文面にする)。
                let msg = match kir::write_kir_with(&program, &limits) {
                    Err(w) if w.kind() == kir::ErrorKind::WriteLimit => w.message().to_string(),
                    _ => e.message().to_string(),
                };
                return (program, Ran::Limit(format!("<stdin>: {msg}")));
            }
            eprintln!(
                "keleut: 内部エラー: KIR の検証に失敗しました: {}",
                e.message()
            );
            return (program, Ran::Ran(Exec::Exit(70)));
        }
        let Some(jobs) = &self.jobs else {
            return (program, Ran::Ran(Exec::Exit(70)));
        };
        let job = Job {
            program,
            shown,
            layouts,
            mode,
        };
        if let Err(mpsc::SendError(job)) = jobs.send(job) {
            eprintln!("keleut: 内部エラー: 実行のスレッドが落ちました");
            return (job.program, Ran::Ran(Exec::Exit(70)));
        }
        match self.replies.recv() {
            Ok(r) => {
                if std::env::var_os("KELEUT_REPL_STATS").is_some() {
                    let after = self.verified.done();
                    eprintln!(
                        "keleut-repl-stats: kir_functions=+{}/{} kir_globals=+{}/{} lir_functions=+{}/{}",
                        after.functions - before.functions,
                        after.functions,
                        after.globals - before.globals,
                        after.globals,
                        r.lir.0,
                        r.lir.1
                    );
                }
                (r.program, Ran::Ran(r.exec))
            }
            Err(_) => {
                // スレッドが落ちて、プログラムも失われた。セッションを終えるので、空のプログラムを返す。
                eprintln!("keleut: 内部エラー: 実行のスレッドが落ちました");
                self.jobs = None;
                let empty = kir::Program {
                    version: kir::VERSION,
                    units: Vec::new(),
                    types: Vec::new(),
                    effects: Vec::new(),
                    classes: Vec::new(),
                    instances: Vec::new(),
                    globals: Vec::new(),
                    functions: Vec::new(),
                    nodes: kir::Nodes::default(),
                };
                (Box::new(empty), Ran::Ran(Exec::Exit(70)))
            }
        }
    }
}

/// 実行時の文脈を使わない間に付けておく、空のプログラム。
static PARKED: keleut_lir::Program = keleut_lir::Program::empty();

/// 実行のスレッドの本体。
///
/// 依頼ごとに、足した単位を LIR に下ろして LIR のプログラムの後ろに足し、インタプリタをそのプログラムに付け替え、
/// 実行時の表を伸ばして、足した単位の初期化を実行する。
/// 依頼の間は、インタプリタを空のプログラムに付け替えておく(LIR のプログラムに単位を足せるようにする)。
fn mutator(o: &Options, native: u64, jobs: mpsc::Receiver<Job>, replies: mpsc::Sender<Reply>) {
    keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Exec);
    let x = 0u8;
    let sp0 = std::hint::black_box(&x) as *const u8 as usize;
    let low = sp0
        .saturating_sub(usize::try_from(native).unwrap_or(usize::MAX))
        .saturating_add(keleut_interp::interp::NATIVE_MARGIN);
    let mut lp = keleut_lir::Program::empty();
    let mut ext = keleut_lir::Extender::new();
    let mut parked: Option<Interp<'static>> = None;
    while let Ok(job) = jobs.recv() {
        let f0 = lp.funcs.len();
        let exec = run_job(o, low, &job, &mut lp, &mut ext, &mut parked);
        let end = matches!(exec, Exec::Exit(_) | Exec::Io);
        let n = lp.funcs.len();
        let reply = Reply {
            program: job.program,
            exec,
            lir: (n.saturating_sub(f0), n),
        };
        if replies.send(reply).is_err() || end {
            break;
        }
    }
}

/// 1 つの依頼を実行する([`mutator`])。
fn run_job(
    o: &Options,
    low: usize,
    job: &Job,
    lp: &mut keleut_lir::Program,
    ext: &mut keleut_lir::Extender,
    parked: &mut Option<Interp<'static>>,
) -> Exec {
    // 実行時がこれまでに作った形を、LIR の形の表の後ろに同じ番号で足す。
    if let Some(it) = parked.as_ref() {
        let n = it.rt.shapes.count() as usize;
        for i in lp.shapes.len()..n {
            let labels = it.rt.shapes.labels(keleut_rtcore::ShapeId(i as u32));
            lp.shapes
                .push(labels.iter().map(|&l| keleut_lir::LabelId(l)).collect());
        }
    }
    let init0 = lp.init.len();
    if let Err(e) = ext.extend(&job.program, lp, kir::N_NEST, o.yield_prune) {
        eprintln!("keleut: {e}");
        *parked = None;
        return Exec::Exit(70);
    }
    let lp: &keleut_lir::Program = lp;
    // 前の入力の後や、この単位を下ろす途中で、ほかの確保が大域の確保器の予備を使っていれば、実行の役割の印が立っている。
    // 印をここで下ろしてから表を伸ばす(残った印で表を伸ばす確保が止まり、セッションが終わらないように)。
    // 下ろした印は、この入力のメモリの不足として報せる。
    let early_oom = keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Exec);
    // この単位が足す大域とインスタンスの最初の添字(実行時エラーで止まったときに値を捨てる)。
    let base = parked
        .as_ref()
        .map(|it| (it.rt.globals.len(), it.rt.inst_values.len()));
    // この単位の形を実行時の形の表に入れられなかった(メモリの不足)。
    let mut shape_oom = false;
    let mut it = match parked.take() {
        Some(it) => {
            let mut it = it.rebind(lp);
            // 形の表を広げられないときは、ほかの表にまだ触れていないので、この入力のメモリの不足として続ける。
            // 入れられた形は表に残り、次の入力の `grow_shapes` が続きから入れる。
            match it.rt.grow_shapes() {
                Ok(()) => {}
                Err(Stop::OutOfMemory) => shape_oom = true,
                Err(e) => {
                    eprint!("{}", String::from_utf8_lossy(&e.message()));
                    return Exec::Exit(70);
                }
            }
            if !shape_oom && let Err(e) = it.rt.grow() {
                // 表を伸ばす途中の内部の誤りか、メモリの不足である。
                // 大域の表を伸ばした後なので、入力の誤りとして続けず、セッションを終える。
                eprint!("{}", String::from_utf8_lossy(&e.message()));
                return Exec::Exit(70);
            }
            it
        }
        None => match Rt::new_with(lp, o.max_depth, o.gc_stress) {
            Ok(rt) => {
                let mut it = Interp::new(rt, low);
                it.rt.out.set_log_to_stdout(true);
                it
            }
            Err(e) => {
                // 起動のときの形の登録がメモリの不足になれば、初期化を始めるときの実行時エラー(3)で終える。
                // 内部の誤り(形の番号の食い違い)は 70 である。
                eprint!("{}", String::from_utf8_lossy(&e.message()));
                return Exec::Exit(e.exit_code());
            }
        },
    };
    let r = if early_oom || shape_oom {
        Err(Stop::OutOfMemory)
    } else {
        exec(
            &mut it,
            lp,
            &job.program,
            &lp.init[init0..],
            &job.shown,
            &job.layouts,
        )
    };
    if r.is_err() {
        // 止まった入力の間に立った印は、この入力の停止とともに捨てる(印はメモリの不足 1 回に 1 度だけ答える)。
        keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Exec);
    }
    let exec = match r {
        Ok(()) => match it.rt.out.flush() {
            Ok(()) => Exec::Done,
            Err(e) => io_failed(&mut it, Stop::from(e)),
        },
        Err(e) if job.mode == Mode::Start => {
            // プレリュードの初期化が止まった(Diktor は標準エラーに書いて終わる)。
            it.rt.out.eprint(&e.message());
            Exec::Exit(e.exit_code())
        }
        Err(e @ (Stop::Io(_) | Stop::IoAbort(_))) => io_failed(&mut it, e),
        Err(e) => {
            // 止まるまでに書いた出力を残し、その後ろに診断を書く(Diktor の `runtime_message`)。
            match it.rt.out.flush() {
                Err(w) => io_failed(&mut it, Stop::from(w)),
                Ok(()) => {
                    let mut w = std::io::stdout().lock();
                    match w.write_all(&e.message()).and_then(|()| w.flush()) {
                        Ok(()) => Exec::Failed,
                        Err(err) => {
                            eprintln!(
                                "keleut: 標準出力に書き出せません: {}",
                                crate::front_cmd::io_msg(&err)
                            );
                            Exec::Io
                        }
                    }
                }
            }
        }
    };
    if let (Exec::Failed, Mode::Input, Some((g0, i0))) = (&exec, job.mode, base) {
        drop_unit_values(&mut it, g0, i0);
    }
    *parked = Some(it.rebind(&PARKED));
    exec
}

/// 実行時エラーで止まった入力の単位の、大域とインスタンスの値を捨てる。
///
/// 型検査の側がその入力の名前とインスタンスを捨てるので、後の入力はこれらの値を指せない。
/// 参照セルはトップレベルに置けない(`Heap[h]` を導入するのは `run` だけである)ので、
/// 前の入力の値からこれらの値に届くことも無い。
/// 値を Unit にして初期化前に戻し、回収の根から外す(`g0` と `i0` は [`Rt::grow`] で伸ばす前の表の長さ)。
fn drop_unit_values(it: &mut Interp<'_>, g0: usize, i0: usize) {
    let rt = &mut it.rt;
    for g in g0..rt.globals.len() {
        rt.globals[g] = Word::UNIT;
        rt.global_init[g] = false;
    }
    for i in i0..rt.inst_values.len() {
        rt.inst_values[i].fill(Word::UNIT);
        rt.inst_init[i] = false;
    }
}

/// プレリュードの初期化を 1 回実行する(Diktor の `repl` の `start` の `Interp.start_session`)。
///
/// 実行のスレッドを作り、組み込みの単位とプレリュードの単位からなるプログラムを下ろして初期化する。
/// プレリュードの出力は標準出力に出す。
/// 止まれば、診断を標準エラーに書いた後の終了状態を `Err` で返す。
fn start_run(o: &Options, sess: &mut Session, out: &mut Output) -> Result<Live, u8> {
    out.flush()?;
    let mut live = Live::spawn(o)?;
    let Some(program) = sess.take_program() else {
        eprintln!("keleut: 内部エラー: プレリュードを初期化できません");
        return Err(70);
    };
    let (program, r) = live.run(program, Vec::new(), Layouts::default(), Mode::Start);
    sess.put_program(program);
    match r {
        Ran::Ran(Exec::Done) => Ok(live),
        Ran::Ran(Exec::Io) => Err(exit::IO),
        Ran::Ran(Exec::Exit(c)) => Err(c),
        Ran::Ran(Exec::Failed) => Err(70),
        Ran::Limit(msg) => {
            eprintln!("keleut: 内部エラー: プレリュードを初期化できません: {msg}");
            Err(70)
        }
    }
}

/// 1 つの入力を型検査して実行する(Diktor の `repl` の `process`)。
fn process(
    live: &mut Live,
    sess: &mut Session,
    ds: Vec<keleut_front::ast::Decl>,
    out: &mut Output,
) -> Result<(), u8> {
    let (program, shown, layouts) = match sess.check(ds) {
        Outcome::Rejected(lines) => {
            for l in lines {
                out.line(&l);
            }
            return Ok(());
        }
        Outcome::Run {
            lines,
            program,
            shown,
            layouts,
        } => {
            for l in lines {
                out.line(&l);
            }
            (program, shown, layouts)
        }
    };
    if let Err(c) = out.flush() {
        sess.finish(program, Finish::Unrun);
        return Err(c);
    }
    let (program, r) = live.run(program, shown, layouts, Mode::Input);
    let (finish, end) = match r {
        Ran::Ran(Exec::Done) => (Finish::Accepted, None),
        Ran::Ran(Exec::Failed) => (Finish::Failed, None),
        Ran::Ran(Exec::Io) => (Finish::Failed, Some(exit::IO)),
        Ran::Ran(Exec::Exit(c)) => (Finish::Failed, Some(c)),
        Ran::Limit(msg) => {
            out.line(&msg);
            (Finish::Unrun, None)
        }
    };
    sess.finish(program, finish);
    match end {
        Some(c) => Err(c),
        None => Ok(()),
    }
}

fn io_failed(it: &mut Interp<'_>, e: Stop) -> Exec {
    it.rt.out.eprint(&e.message());
    Exec::Io
}

/// 関数 `f` を呼ぶ。最上位に届いた状態語は、その誤りにする(`keleut_interp` の `run_init` と同じ)。
fn call(
    it: &mut Interp<'_>,
    f: keleut_lir::FuncId,
    env: Word,
    args: &[Word],
) -> Result<Word, Stop> {
    match it.run(f, env, args, 0) {
        Ok(v) if v == Word::SENTINEL => {
            let e = match std::mem::take(&mut it.rt.pending) {
                Pending::Error(s) => s,
                p => Stop::Internal(format!("最上位に届いた状態語: {p:?}")),
            };
            reset(it);
            Err(e)
        }
        Ok(v) => Ok(v),
        Err(e) => {
            reset(it);
            Err(e)
        }
    }
}

/// 止まった呼び出しの後で、次の呼び出しができる状態に戻す。
fn reset(it: &mut Interp<'_>) {
    it.rt.pending = Pending::None;
    it.rt.limit = it.rt.max_depth;
    it.rt.depth = 0;
    it.rt.chain = Word::UNIT;
}

/// 足した単位の初期化 `inits` を順に実行し、結果を表示する。
fn exec(
    it: &mut Interp<'_>,
    lp: &keleut_lir::Program,
    kp: &kir::Program,
    inits: &[keleut_lir::FuncId],
    shown: &[Shown],
    layouts: &Layouts,
) -> Result<(), Stop> {
    for &f in inits {
        call(it, f, Word(0), &[])?;
    }
    // 初期化の途中で、ヒープの確保の入口を通らない確保(引数の退避など)が大域の確保器の予備を使っていれば、
    // 印が残っている。
    // その入力のメモリの不足として、束縛を表示する前に報せる。
    if keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Exec) {
        return Err(Stop::OutOfMemory);
    }
    for s in shown {
        if s.unit_exp {
            continue;
        }
        let text = display(it, lp, kp, s, layouts)?;
        let mut line = Vec::with_capacity(s.label.len() + text.len() + 4);
        line.extend_from_slice(s.label.as_bytes());
        line.extend_from_slice(b" = ");
        line.extend_from_slice(&text);
        line.push(b'\n');
        it.rt.out.write(&line).map_err(Stop::from)?;
    }
    if keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Exec) {
        return Err(Stop::OutOfMemory);
    }
    Ok(())
}

/// KIR の大域 `g` が辞書を受け取る関数なら、その辞書の数。
fn dict_count(kp: &kir::Program, g: usize) -> usize {
    match kp.globals.get(g).map(|x| &x.kind) {
        Some(kir::GlobalKind::Dictabs(f)) => {
            kp.functions.get(f.index()).map_or(0, |f| f.dicts as usize)
        }
        _ => 0,
    }
}

/// 束縛の値の表示(Diktor の `show_value`)。
///
/// 辞書を受け取る値は、辞書の代わりに Unit を渡して評価してから表示する
/// (初期化式は構文上の値なので、辞書を使わず、停止し、エフェクトを起こさない)。
/// 評価が止まったときは `<unevaluated>` にする。
/// 型の頭が newtype で `Show` を満たす値は、`$show` の関数で文字列にする。
/// その関数が止まったら、入力を受理しない。
fn display(
    it: &mut Interp<'_>,
    lp: &keleut_lir::Program,
    kp: &kir::Program,
    s: &Shown,
    layouts: &Layouts,
) -> Result<Vec<u8>, Stop> {
    let g = s.global.index();
    let v = it.rt.globals[g];
    let forced = match &lp.globals[g].kind {
        keleut_lir::GlobalKind::Dictabs(f) => {
            let n = dict_count(kp, g);
            match call(it, *f, v, &vec![Word::UNIT; n]) {
                Ok(w) => Some(w),
                Err(e) if e.is_fatal() || matches!(e, Stop::Io(_)) => return Err(e),
                Err(_) => None,
            }
        }
        _ => Some(v),
    };
    let Some(v) = forced else {
        return Ok(b"<unevaluated>".to_vec());
    };
    let Some(sg) = s.show else {
        return Ok(show_value(&it.rt, v, layouts, s.layout));
    };
    let mark = it.rt.roots.len();
    it.rt.roots.push(v.0);
    let r = show_call(it, lp, kp, sg.index(), mark);
    it.rt.roots.truncate(mark);
    r
}

/// `$show` の関数を、根の枠の `mark` 番の値に適用する。
fn show_call(
    it: &mut Interp<'_>,
    lp: &keleut_lir::Program,
    kp: &kir::Program,
    sg: usize,
    mark: usize,
) -> Result<Vec<u8>, Stop> {
    let f = it.rt.globals[sg];
    let s = match &lp.globals[sg].kind {
        keleut_lir::GlobalKind::Fn(func) => {
            let v = Word(it.rt.roots.get(mark));
            call(it, *func, f, &[v])?
        }
        keleut_lir::GlobalKind::Dictabs(func) => {
            let n = dict_count(kp, sg);
            let c = call(it, *func, f, &vec![Word::UNIT; n])?;
            let v = Word(it.rt.roots.get(mark));
            let cf = it.rt.closure_func(c);
            call(it, cf, c, &[v])?
        }
        _ => return Err(Stop::Internal("表示の関数の大域の種類が違います".into())),
    };
    Ok(it.rt.str_bytes(s).to_vec())
}

/// 値の構造に沿った表示(Diktor の `Value.show ~fn_name:false`)。
///
/// [`Rt::show`] と同じ形で、関数の値(組み込みの値、`extern`、メソッドの値を含む)をすべて `<fn>` と書く。
/// レコードのフィールドは、型の配置([`Layout`])が分かれば型の行の順に書き、値の表示の順を型の表示の順にそろえる。
/// 配置が分からないとき(型の大きさや配置の表の上限を超えたときを含む)と、配置のラベルの集まりが値と合わないときは、
/// ラベルの綴りのバイト列の順に書く(安定な整列で、同じラベルは正準の配置のとおり外側から)。
/// 正準の配置の順(ラベルの番号の順)は受理しなかった入力を含むセッションの履歴で決まるので、表示に使わない(V231)。
/// 深い値でもネイティブのスタックを使わないよう、明示の作業の列でたどる。
fn show_value(rt: &Rt<'_>, w: Word, ls: &Layouts, root: usize) -> Vec<u8> {
    enum Item {
        V(Word, usize),
        T(&'static str),
        S(String),
    }
    fn push_seq(work: &mut Vec<Item>, open: Item, xs: Vec<(Word, usize)>, close: &'static str) {
        work.push(Item::T(close));
        for (i, (x, l)) in xs.into_iter().enumerate().rev() {
            work.push(Item::V(x, l));
            if i > 0 {
                work.push(Item::T(", "));
            }
        }
        work.push(open);
    }
    let p = rt.prog;
    let ctor_name = |t: u32, c: u32| display_name(&p.types[t as usize].ctors[c as usize].name);
    let layout = |l: usize| ls.nodes.get(l).unwrap_or(&Layout::Any);
    let mut out = Vec::new();
    let mut work = vec![Item::V(w, root)];
    while let Some(it) = work.pop() {
        let (w, lid) = match it {
            Item::T(s) => {
                out.extend_from_slice(s.as_bytes());
                continue;
            }
            Item::S(s) => {
                out.extend_from_slice(s.as_bytes());
                continue;
            }
            Item::V(w, l) => (w, l),
        };
        if w.is_i32() {
            out.extend_from_slice(w.as_i32().to_string().as_bytes());
            continue;
        }
        if let Some(k) = w.imm_kind() {
            match k {
                ImmKind::Unit => out.extend_from_slice(b"()"),
                ImmKind::Bool => {
                    out.extend_from_slice(if w.as_bool() { b"true" } else { b"false" })
                }
                ImmKind::Ctor0 => {
                    let (t, c) = w.payload_pair();
                    out.extend_from_slice(ctor_name(t, c).as_bytes());
                }
                ImmKind::DictBuiltin | ImmKind::DictInst0 => out.extend_from_slice(b"<dict>"),
            }
            continue;
        }
        if !w.is_ptr() {
            out.extend_from_slice(b"<?>");
            continue;
        }
        // SAFETY: w は生きている箱を指す。
        let (kind, n) = unsafe { (value::kind_of(w), value::count_of(w) as usize) };
        // SAFETY: 各分岐は箱の種類の配置のとおりに読む。
        let at = |i: usize| Word(unsafe { value::get(w, i) });
        match kind {
            Some(Kind::I64) => {
                out.extend_from_slice((rt.unbox_word(w) as i64).to_string().as_bytes())
            }
            Some(Kind::F64) => out.extend_from_slice(
                keleut_rtcore::show_f64(f64::from_bits(rt.unbox_word(w))).as_bytes(),
            ),
            Some(Kind::Str) => {
                out.push(b'"');
                keleut_rt::ctx::escape_into(&mut out, rt.str_bytes(w));
                out.push(b'"');
            }
            Some(Kind::Record) => {
                let shape = at(1).0 as u32;
                let labels = rt.shapes.labels(keleut_rtcore::ShapeId(shape));
                let names: Vec<&str> = labels
                    .iter()
                    .map(|&l| p.labels[l as usize].as_str())
                    .collect();
                // 型の行の順に並べ替える。配置が無いか、ラベルの集まりが型と違えば、ラベルの綴りの順にする。
                let mut order: Vec<(usize, usize)> = (0..n).map(|i| (i, 0)).collect();
                order.sort_by(|&(a, _), &(b, _)| {
                    display_name(names[a])
                        .cmp(display_name(names[b]))
                        .then_with(|| names[a].cmp(names[b]))
                });
                if let Layout::Record(fs) = layout(lid)
                    && fs.len() == n
                {
                    let mut used = vec![false; n];
                    let mut v = Vec::with_capacity(n);
                    for (l, c) in fs {
                        match (0..n).find(|&i| !used[i] && names[i] == l.as_str()) {
                            Some(i) => {
                                used[i] = true;
                                v.push((i, *c));
                            }
                            None => break,
                        }
                    }
                    if v.len() == n {
                        order = v;
                    }
                }
                let tuple = names.iter().all(|&l| l == "_item");
                let fields: Vec<(Word, usize)> =
                    order.iter().map(|&(i, c)| (at(2 + i), c)).collect();
                if tuple {
                    let close = if n == 1 { ",)" } else { ")" };
                    push_seq(&mut work, Item::T("("), fields, close);
                } else {
                    work.push(Item::T("}"));
                    for (k, &(i, _)) in order.iter().enumerate().rev() {
                        work.push(Item::V(fields[k].0, fields[k].1));
                        let l = display_name(names[i]);
                        let sep = if k > 0 { ", " } else { "" };
                        work.push(Item::S(format!("{sep}{l} = ")));
                    }
                    work.push(Item::T("{"));
                }
            }
            Some(Kind::Variant) => {
                let raw = p.labels[at(1).0 as usize].as_str();
                let l = display_name(raw);
                let payload = at(2);
                let c = match layout(lid) {
                    Layout::Variant(fs) => fs.iter().find(|(x, _)| x == raw).map_or(0, |(_, c)| *c),
                    _ => 0,
                };
                if payload == Word::UNIT {
                    out.extend_from_slice(format!("#{l}").as_bytes());
                } else {
                    push_seq(
                        &mut work,
                        Item::S(format!("#{l}(")),
                        vec![(payload, c)],
                        ")",
                    );
                }
            }
            Some(Kind::Data) => {
                let tc = at(1).0;
                let name = ctor_name((tc >> 32) as u32, tc as u32).to_string();
                let cs: Option<&Vec<usize>> = match layout(lid) {
                    Layout::Data(cs) => cs.iter().find(|(c, _)| *c == name).map(|(_, f)| f),
                    _ => None,
                };
                let fields = (0..n)
                    .map(|i| (at(2 + i), cs.and_then(|f| f.get(i).copied()).unwrap_or(0)))
                    .collect();
                push_seq(&mut work, Item::S(format!("{name}(")), fields, ")");
            }
            Some(Kind::Closure | Kind::Method | Kind::ExternUnimpl) => {
                out.extend_from_slice(b"<fn>")
            }
            Some(Kind::Ref) => out.extend_from_slice(b"<ref>"),
            Some(k @ (Kind::Array | Kind::MArray)) => {
                let c = match layout(lid) {
                    Layout::Array(c) => *c,
                    _ => 0,
                };
                let xs = (0..n).map(|i| (at(1 + i), c)).collect();
                if k == Kind::Array {
                    push_seq(&mut work, Item::T("["), xs, "]");
                } else {
                    push_seq(&mut work, Item::T("<mutable ["), xs, "]>");
                }
            }
            Some(Kind::DictInst | Kind::DictRecord | Kind::DictVariant) => {
                out.extend_from_slice(b"<dict>")
            }
            Some(Kind::Handler | Kind::Cont | Kind::Pending) | None => {
                out.extend_from_slice(b"<?>")
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(xs: &[&str]) -> Result<Options, String> {
        parse_options(&xs.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn 選択肢を読む() {
        let o = opts(&["--no-prelude", "--max-depth", "10", "--gc-stress"]).unwrap();
        assert!(o.session.no_prelude && o.gc_stress && o.yield_prune);
        assert_eq!(o.max_depth, 10);
        assert!(
            opts(&[
                "--backend",
                "interp",
                "--tier",
                "interp-only",
                "--frontend",
                "rust"
            ])
            .is_ok()
        );
        assert!(opts(&["x.kel"]).is_err());
        // 後段の組の選択肢と --strict-exhaustive は受けて捨てる(入力はインタプリタで実行する)。
        assert!(
            opts(&[
                "--backend",
                "wasm",
                "--tier",
                "jit-only",
                "--strict-exhaustive"
            ])
            .is_ok()
        );
        assert!(opts(&["--backend"]).is_err());
        assert!(opts(&["--typo"]).is_err());
        assert!(opts(&["--native-stack", "256k"]).is_err());
    }

    #[test]
    fn コマンドの行末の空白を除く() {
        assert_eq!(trim_command(b":q \t\r"), b":q");
        assert_eq!(trim_command(b""), b"");
    }
}
