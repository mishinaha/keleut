//! サブコマンド `keleut repl` を実装する。
//!
//! | 書式 | 動作 |
//! |---|---|
//! | `keleut repl [OPTIONS]` | 標準入力から入力を読み、入力ごとに型検査して実行し、結果を表示する(`diktor --repl` と同じ出力) |
//!
//! 入力の区切り方、コマンド(`:quit`、`:q`、`:reset`、`:type 式`)、結果の表示、診断メッセージは、
//! Diktor の `driver.ml` の §16.6b と同じである。
//! 結果、診断、プログラムの出力は、すべて標準出力に書く。
//! プロンプトは、標準入力が端末のときだけ表示する。
//! 入力の終わりか `:quit` で、終了コード 0 で終了する。
//! 型エラーや実行時エラーでは、セッションを終了しない。
//!
//! 型検査は keleut-front の [`Session`] が行い、型検査に通った入力ごとに KIR の単位を 1 つ、
//! セッションのプログラムの末尾に追加する。
//! 実行にはインタプリタを使い、ランタイムの状態(ヒープ、グローバル、インスタンス、シェイプ表)はセッションの間ずっと保持する。
//! 入力ごとに、次の順に処理する。
//!
//! 1. 追加した単位だけを検証する(`kir::verify_extension`)。
//! 2. その単位を LIR に変換(lowering)し、LIR のプログラムの末尾に追加する(`keleut_lir::Extender`)。
//! 3. ランタイムの表を拡張する([`Rt::grow`])。
//! 4. その単位の初期化コードだけを実行する。
//!
//! 追加した単位は、前の入力のグローバルを同じ添字で読む。
//! 入力 1 つの処理時間は、その入力の大きさにほぼ比例し、それまでの入力の個数にはほとんどよらない。
//! ただし、宣言したクラスの数と、module の中の値の数に比例する処理が残る(Diktor も同じである)。
//! 実行は専用のスレッド(`keleut-mutator`)で行い、型検査のスレッドとは要求(`Job`)と応答(`Reply`)をやりとりする。
//!
//! セッションの開始時と `:reset` の後に、プレリュードの初期化コードを 1 回実行する。
//! この初期化がエラーで止まれば、Diktor と同じく診断を標準エラーに書き、そのエラーの終了コード(実行時エラーなら 3)で終了する。
//!
//! 入力が実行時エラーで止まったときは、止まるまでに書いた出力を残す(LangSpec §6.6、§18.5)。
//! その入力は受理せず、その入力が束縛した名前は後の入力から見えない。
//! その入力の単位はプログラムに残すが、単位が追加したグローバルとインスタンスの値は捨てて、GC ルートから外す。
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--no-prelude`、`--prelude PATH`:`keleut check` の同名のオプションと同じである。
//! - `--max-nest N`、`--native-stack BYTES`:構文解析の深さの上限と、フロントエンドのスレッドのスタックサイズを指定する(`keleut check` と同じ)。
//! - `--max-depth N`、`--gc-stress`、`--no-yield-prune`:`keleut run` の同名のオプションと同じである。
//!   インタプリタのスタックサイズは `--max-depth` から決める。
//! - `--backend B`、`--tier MODE`、`--frontend F`、`--strict-exhaustive`:kconform のペアのオプションと `diktor --repl` のオプションを受け付けるためのもので、
//!   指定しても動作は変わらない(入力は、バックエンドの指定によらずインタプリタで実行する)。
//!
//! 標準入力が閉じていれば、診断を標準エラーに書いて終了コード 74 で終了する。
//! 閉じているかどうかは、Rust のランタイムが fd 0 を `/dev/null` で開き直す前に調べておく。

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

/// `keleut repl` の使い方の説明(usage)。
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

/// プロセスの開始時に標準入力(fd 0)が閉じていたかどうかを表す。
///
/// Rust の標準ライブラリは `main` の前に、閉じている fd 0〜2 を `/dev/null` で開き直すので、
/// `main` の中から読むと、閉じていた標準入力は入力の終わりに見える。
/// Diktor(OCaml)は、同じ状況で `Bad file descriptor` を報告して終了コード 74 で終了する。
/// 同じ動作にするために、標準ライブラリの初期化より前に実行される `.init_array` の関数で fd 0 を調べておく
/// (`front_cmd` が標準出力を調べるのと同じ方法である)。
static STDIN_CLOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
mod stdin_probe {
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    /// `fcntl` のコマンド `F_GETFD`(ファイル記述子フラグを読む)の値。
    const F_GETFD: i32 = 1;

    extern "C" fn probe() {
        // SAFETY: fd 0 のファイル記述子フラグを読むだけで、何も変えない。
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
            // kconform のペアのオプションと `diktor --repl` のオプションは、受け付けて無視する。
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

/// 標準出力に書く内容をためるバッファ。
///
/// `flush` で標準出力に書き、書けなければ終了コード 74 を返す(Diktor の `flush_stdout_or_die` に相当する)。
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

    /// ためた出力を標準出力に書いて flush する。
    /// 書けなければ、診断を標準エラーに書いて終了コード 74 を返す。
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

/// 入力を 1 つ実行した結果を表す。
enum Exec {
    /// 最後まで実行し、結果を表示した。
    Done,
    /// 実行時エラーで止まった(診断は標準出力に書いた)。
    Failed,
    /// 標準出力に書けなかった(診断は標準エラーに書いた)。
    Io,
    /// セッションを続けられないので、この終了コードで終了する(診断は標準エラーに書いた)。
    ///
    /// 次のどれかが起きたときである。
    ///
    /// - プレリュードの初期化がエラーで止まった。
    /// - KIR の検証、LIR への変換、ランタイムの表の準備のどれかが、内部エラーか資源不足で失敗した。
    /// - 実行スレッドを作れなかったか、実行スレッドが異常終了した。
    Exit(u8),
}

/// 実行スレッドに依頼する実行の種類(プレリュードの初期化か、入力の実行か)を表す。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// セッションの開始時に行う、プレリュードの初期化。
    /// エラーで止まればセッションを終了する。
    Start,
    /// 入力の実行。
    /// 追加した単位(この入力)の初期化コードを実行し、結果を表示する。
    Input,
}

/// `keleut repl …` のエントリポイントである。
/// 引数の先頭は `repl` である。
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
    // 入力の種類は設定しない。
    // 変換スレッドがメモリの確保に失敗しても、アロケータのラッパが REPL のプロセスを終了しないようにするためである(`keleut_rt::alloc`)。
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

/// コマンドの行から、行末の空白、タブ、CR を取り除く。
fn trim_command(line: &[u8]) -> &[u8] {
    let mut n = line.len();
    while n > 0 && matches!(line[n - 1], b' ' | b'\t' | b'\r') {
        n -= 1;
    }
    &line[..n]
}

/// REPL の本体である(Diktor の `repl` の `loop` に相当する)。
/// 型検査のスレッドの中で呼ぶ。
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
        // 前の入力(実行時エラーで止まった入力など)がグローバルアロケータの予備領域を使っていれば、次の入力の前に確保し直す。
        // 確保できなければ、予備領域なしで続ける(`keleut_rt::alloc`)。
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
                // それまでのランタイムの状態を捨ててから、プレリュードだけのセッションを始め直す。
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

/// フロントエンドがセッションを始められなかったときに、診断を標準エラーに書いて終了コードを返す。
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

/// 実行スレッドへの要求。
struct Job {
    /// セッションのプログラム。
    /// 末尾の単位(セッションの開始時は、プレリュードまでの単位)が、この要求で追加した分である。
    program: Box<kir::Program>,
    shown: Vec<Shown>,
    layouts: Layouts,
    mode: Mode,
    /// 追加した単位の検証に使った、入れ子の深さの上限(`kir::verify_extension` が返した値)。
    /// LIR への変換に使うスタックは、この深さに必要な大きさにする。
    max_nest: usize,
}

/// 実行スレッドからの応答。
struct Reply {
    /// 要求で渡されたプログラム。
    /// 型検査の側に返す。
    program: Box<kir::Program>,
    exec: Exec,
    /// LIR に変換して追加した関数の数と、LIR の関数の総数(`KELEUT_REPL_STATS` の出力に使う)。
    lir: (usize, usize),
}

/// 実行スレッドを操作するハンドルである。
/// 実行スレッドは、セッションのランタイムの状態を保持している。
///
/// ドロップすると、要求の送信側を閉じてスレッドの終了を待つ。
/// スレッドは、終了するときにヒープと表を解放する。
struct Live {
    jobs: Option<mpsc::Sender<Job>>,
    replies: mpsc::Receiver<Reply>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// これまでに追加した単位の、KIR の検証の状態。
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

/// 実行の要求の結果を表す。
enum Ran {
    /// 実行した。
    /// セッションを終了するエラーで止まった場合も含む。
    Ran(Exec),
    /// 処理系の限度のために実行しなかった。
    /// 値は 1 行の診断である。
    ///
    /// 入れ子が KIR の上限 `N_nest` を超えたときと、KIR を処理するスレッドを作れなかったときである。
    Limit(String),
}

impl Live {
    /// 実行スレッドを作る。
    ///
    /// `keleut run` と同じく、仮想メモリを制限しているとき(`ulimit -v`)は、スタックを初めから仮想メモリの上限の 4 分の 1 以下にし、
    /// 作れなければ半分ずつ小さくして作り直す(`keleut_interp::exec_stack_sizes`)。
    /// 最も小さいスタックでも作れなければ、`実行時エラー: メモリ不足です` を標準エラーに書いて 3 を返す
    /// (プレリュードの初期化の開始時に起きた実行時エラーと同じ扱いである)。
    /// 仮想メモリを制限していないのに作れなければ、内部エラーとして 70 を返す。
    fn spawn(o: &Options) -> Result<Live, u8> {
        let mut last = None;
        for native in
            keleut_interp::exec_stack_sizes(keleut_rt::native_stack_for(o.max_depth), true)
        {
            let size = usize::try_from(native).unwrap_or(usize::MAX);
            let (jtx, jrx) = mpsc::channel::<Job>();
            let (rtx, rrx) = mpsc::channel::<Reply>();
            let oc = o.clone();
            let h = std::thread::Builder::new()
                .name("keleut-mutator".to_string())
                .stack_size(size)
                .spawn(move || mutator(&oc, native, jrx, rtx));
            match h {
                Ok(t) => {
                    return Ok(Live {
                        jobs: Some(jtx),
                        replies: rrx,
                        thread: Some(t),
                        verified: kir::verify::Extension::new(),
                    });
                }
                Err(e) => last = Some((size, e)),
            }
        }
        if let Some((size, e)) = last
            && keleut_rt::alloc::address_space_limit().is_none()
        {
            // 仮想メモリを制限していないのに作れないのは、処理系の内部エラーである(`keleut run` と同じ)。
            eprintln!("keleut: 内部エラー: 実行スレッドを作れません({size} バイトのスタック): {e}");
            return Err(70);
        }
        let stop = keleut_rt::Stop::OutOfMemory;
        keleut_rt::out::Out::default().eprint(&stop.message());
        Err(stop.exit_code())
    }

    /// プログラムに追加した単位を検証し、実行スレッドで実行する。
    ///
    /// 次の 2 つの場合は、処理系の限度として単位を実行しない([`Ran::Limit`])。
    ///
    /// - 単位の入れ子が KIR の上限 `N_nest` を超えた。`keleut run --frontend rust` が KIR の書き出しで止まるのと同じ上限である。
    /// - 仮想メモリを制限しているとき(`ulimit -v`)に、KIR を処理するスレッドを作れなかった。
    ///
    /// それ以外の検証の失敗は内部エラーなので、診断を標準エラーに書いて [`Exec::Exit`] の 70 を返す。
    /// プログラムは、どの場合も呼び出し元に返す。
    fn run(
        &mut self,
        program: Box<kir::Program>,
        shown: Vec<Shown>,
        layouts: Layouts,
        mode: Mode,
    ) -> (Box<kir::Program>, Ran) {
        let limits = kir::Limits::default();
        let before = self.verified.done();
        let max_nest = match kir::verify_extension(&mut self.verified, &program, &limits) {
            Ok(n) => n,
            Err(e) if e.kind() == kir::ErrorKind::Limit => {
                // ライタの診断を優先する(`keleut run` の `program_for_run` と同じメッセージにする)。
                let msg = match kir::write_kir_with(&program, &limits) {
                    Err(w) if w.kind() == kir::ErrorKind::WriteLimit => w.message().to_string(),
                    _ => e.message().to_string(),
                };
                return (program, Ran::Limit(format!("<stdin>: {msg}")));
            }
            Err(e) => {
                eprintln!(
                    "keleut: 内部エラー: KIR の検証に失敗しました: {}",
                    e.message()
                );
                return (program, Ran::Ran(Exec::Exit(70)));
            }
        };
        let Some(jobs) = &self.jobs else {
            return (program, Ran::Ran(Exec::Exit(70)));
        };
        let job = Job {
            program,
            shown,
            layouts,
            mode,
            max_nest,
        };
        if let Err(mpsc::SendError(job)) = jobs.send(job) {
            eprintln!("keleut: 内部エラー: 実行スレッドが異常終了しました");
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
                // スレッドが異常終了して、プログラムも失われた。セッションを終了するので、空のプログラムを返す。
                eprintln!("keleut: 内部エラー: 実行スレッドが異常終了しました");
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

/// 要求を処理していない間、インタプリタに持たせておく空のプログラム([`mutator`])。
static PARKED: keleut_lir::Program = keleut_lir::Program::empty();

/// 実行スレッドの本体。
///
/// 要求ごとに、追加した単位を LIR に変換して LIR のプログラムの末尾に追加し、インタプリタが指すプログラムをそれに差し替え、
/// ランタイムの表を拡張して、追加した単位の初期化コードを実行する。
/// 要求を処理していない間は、インタプリタが指すプログラムを空のプログラム([`PARKED`])に差し替えておく。
/// インタプリタが LIR のプログラムを借用したままでは、そこに単位を追加できないからである。
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

/// 要求を 1 つ処理する([`mutator`] から呼ぶ)。
fn run_job(
    o: &Options,
    low: usize,
    job: &Job,
    lp: &mut keleut_lir::Program,
    ext: &mut keleut_lir::Extender,
    parked: &mut Option<Interp<'static>>,
) -> Exec {
    // ランタイムがこれまでに作ったシェイプを、LIR のシェイプ表の末尾に同じ番号で追加する。
    if let Some(it) = parked.as_ref() {
        let n = it.rt.shapes.count() as usize;
        for i in lp.shapes.len()..n {
            let labels = it.rt.shapes.labels(keleut_rtcore::ShapeId(i as u32));
            lp.shapes
                .push(labels.iter().map(|&l| keleut_lir::LabelId(l)).collect());
        }
    }
    let init0 = lp.init.len();
    // LIR への変換に使うスタックは、検証が使った深さの上限に必要な大きさにする(`Job::max_nest`)。
    // 仮想メモリを制限しているとき(`ulimit -v`)にそのスレッドを作れなければ(`keleut_lir::ErrorKind::Limit`)、
    // 検証を済ませた状態を元に戻せないので、`処理系の限度` の行を書いてセッションを終了コード 1 で終了する。
    // LIR への変換の再帰は単位の深さだけ進むので、検証の深さより小さいスタックで作り直すことはしない。
    if let Err(e) = ext.extend(&job.program, lp, job.max_nest, o.yield_prune) {
        eprintln!("keleut: {e}");
        *parked = None;
        return Exec::Exit(if e.kind() == keleut_lir::ErrorKind::Limit {
            1
        } else {
            70
        });
    }
    let lp: &keleut_lir::Program = lp;
    // 前の入力の後や、この単位を LIR に変換する途中で、ほかの確保がグローバルアロケータの予備領域を使っていれば、実行スレッドのフラグが立っている。
    // 表を拡張する前に、ここでフラグを下ろす。
    // フラグが残っていると、表を拡張する確保が失敗して、セッションが終了するからである。
    // 下ろしたフラグは、この入力のメモリ不足として報告する。
    let early_oom = keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Exec);
    // この単位が追加するグローバルとインスタンスの最初の添字。実行時エラーで止まったときに、ここから後の値を捨てる。
    let base = parked
        .as_ref()
        .map(|it| (it.rt.globals.len(), it.rt.inst_values.len()));
    // この単位のシェイプを、メモリ不足でランタイムのシェイプ表に登録できなかったときに真にする。
    let mut shape_oom = false;
    let mut it = match parked.take() {
        Some(it) => {
            let mut it = it.rebind(lp);
            // シェイプ表を拡張できないときは、ほかの表をまだ変えていないので、この入力のメモリ不足として扱い、セッションを続ける。
            // 登録できたシェイプは表に残り、次の入力の `grow_shapes` が続きから登録する。
            match it.rt.grow_shapes() {
                Ok(()) => {}
                Err(Stop::OutOfMemory) => shape_oom = true,
                Err(e) => {
                    eprint!("{}", String::from_utf8_lossy(&e.message()));
                    return Exec::Exit(70);
                }
            }
            if !shape_oom && let Err(e) = it.rt.grow() {
                // 表を拡張する途中の内部エラーか、メモリ不足である。
                // グローバルの表を拡張した後なので、この入力のエラーとして続けることはせず、セッションを終了する。
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
                // 起動時のシェイプの登録がメモリ不足で失敗すれば、初期化の開始時の実行時エラーとして終了コード 3 で終了する。
                // 内部エラー(シェイプの番号の食い違い)なら 70 である。
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
        // 止まった入力の実行中に立ったフラグは、この入力の停止とともに下ろす。
        // メモリ不足 1 回につき、フラグによる報告は 1 度だけにする。
        keleut_rt::alloc::take_flag(keleut_rt::alloc::Role::Exec);
    }
    let exec = match r {
        Ok(()) => match it.rt.out.flush() {
            Ok(()) => Exec::Done,
            Err(e) => io_failed(&mut it, Stop::from(e)),
        },
        Err(e) if job.mode == Mode::Start => {
            // プレリュードの初期化がエラーで止まった。Diktor と同じく、診断を標準エラーに書いて終了する。
            it.rt.out.eprint(&e.message());
            Exec::Exit(e.exit_code())
        }
        Err(e @ (Stop::Io(_) | Stop::IoAbort(_))) => io_failed(&mut it, e),
        Err(e) => {
            // 止まるまでに書いた出力を残し、その後に診断を書く(診断は Diktor の `runtime_message` に相当する)。
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

/// 実行時エラーで止まった入力の単位が追加した、グローバルとインスタンスの値を捨てる。
///
/// 型検査の側がその入力の名前とインスタンスを捨てるので、後の入力はこれらの値を参照できない。
/// 参照セルはトップレベルに置けない(`Heap[h]` を導入するのは `run` だけである)ので、
/// 前の入力の値からこれらの値に到達することもない。
/// 値を Unit にして初期化前の状態に戻し、GC ルートから外す。
/// `g0` と `i0` は、[`Rt::grow`] で拡張する前の表の長さである。
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

/// プレリュードの初期化を 1 回実行する(Diktor の `repl` の `start` が呼ぶ `Interp.start_session` に相当する)。
///
/// 実行スレッドを作り、組み込みの単位とプレリュードの単位からなるプログラムを LIR に変換して、初期化コードを実行する。
/// プレリュードの出力は標準出力に書く。
/// エラーで止まれば、診断を標準エラーに書き、終了コードを `Err` で返す。
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
        // 仮想メモリを制限しているとき(`ulimit -v`)に KIR を処理するスレッドを作れないのは処理系の限度なので、終了コード 1 で終了する。
        // 入力を LIR に変換するスレッドを作れないとき([`run_job`])と同じである。
        Ran::Limit(msg) if msg.contains(kir::SPAWN_LIMIT_MESSAGE) => {
            eprintln!("keleut: プレリュードを初期化できません: {msg}");
            Err(1)
        }
        Ran::Limit(msg) => {
            eprintln!("keleut: 内部エラー: プレリュードを初期化できません: {msg}");
            Err(70)
        }
    }
}

/// 入力を 1 つ型検査して実行する(Diktor の `repl` の `process` に相当する)。
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

/// 関数 `f` を呼ぶ。
///
/// 状態ワードが設定されたまま最上位に戻ったときは、それが表すエラーを返す(`keleut_interp` の `run_init` と同じ)。
/// エラー以外の状態ワードなら、内部エラーを返す。
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
                p => Stop::Internal(format!("最上位に届いた状態ワード: {p:?}")),
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

/// エラーで止まった呼び出しの後で、ランタイムを次の呼び出しができる状態に戻す。
fn reset(it: &mut Interp<'_>) {
    it.rt.pending = Pending::None;
    it.rt.limit = it.rt.max_depth;
    it.rt.depth = 0;
    it.rt.chain = Word::UNIT;
}

/// 追加した単位の初期化関数 `inits` を順に実行し、束縛の値を表示する。
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
    // 初期化の途中で、`Rt` のヒープ確保の関数を通らない確保(引数の退避など)がグローバルアロケータの予備領域を使っていれば、
    // 実行スレッドのフラグが立ったままになっている。
    // その入力のメモリ不足として、束縛を表示する前に報告する。
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

/// KIR のグローバル `g` が辞書を受け取る関数なら、受け取る辞書の個数を返す。
/// それ以外なら 0 を返す。
fn dict_count(kp: &kir::Program, g: usize) -> usize {
    match kp.globals.get(g).map(|x| &x.kind) {
        Some(kir::GlobalKind::Dictabs(f)) => {
            kp.functions.get(f.index()).map_or(0, |f| f.dicts as usize)
        }
        _ => 0,
    }
}

/// 束縛の値を、表示するバイト列にする(Diktor の `show_value` に相当する)。
///
/// 辞書を受け取る値は、辞書の代わりに Unit を渡して評価してから表示する。
/// 初期化式は構文上の値なので、辞書を使わず、停止し、エフェクトを起こさない。
/// 評価がエラーで止まったときは `<unevaluated>` と表示する。
/// ただし、巻き戻さずに終了するエラー(`Stop::is_fatal`)と出力エラーは、そのまま返す。
/// 型の先頭の型構成子が宣言表 `datas` にあり(`newtype` で宣言した型と、組み込みのデータ型)、その型が `Show` を満たす値は、フロントエンドがその値のために作った内部用のグローバル `$show` の関数を呼んで文字列にする。
/// その関数がエラーで止まったら、入力を受理しない。
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

/// `$show` の関数を、ルートスタックの `mark` 番の値に適用する。
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
        _ => {
            return Err(Stop::Internal(
                "表示の関数のグローバルの種類が違います".into(),
            ));
        }
    };
    Ok(it.rt.str_bytes(s).to_vec())
}

/// 値を、その構造に沿って文字列にする(Diktor の `Value.show ~fn_name:false` に相当する)。
///
/// [`Rt::show`] と同じ形式で、関数の値(組み込みの値、`extern`、メソッドの値を含む)をすべて `<fn>` と書く。
/// レコードのフィールドは、型のレイアウト([`Layout`])が分かれば、型が持つレコードの行(row)の順に書き、値の表示の順序を型の表示の順序にそろえる。
/// レイアウトが分からないとき(型の大きさやレイアウトの表が上限を超えたときを含む)と、レイアウトのラベルの集合が値と合わないときは、
/// ラベル名のバイト列の順に書く(安定ソートなので、同じラベルは正準なレイアウトのとおり外側から並ぶ)。
/// 正準なレイアウトの順(ラベルの番号の順)は、受理しなかった入力を含むセッションの履歴で決まるので、表示には使わない。
/// 深い値でもネイティブスタックを使わないように、再帰せずにワークリストでたどる。
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
        // SAFETY: `w` は生きているオブジェクトを指す。
        let (kind, n) = unsafe { (value::kind_of(w), value::count_of(w) as usize) };
        // SAFETY: 各分岐は、オブジェクトの種類ごとのレイアウトのとおりに読む。
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
                // 型が持つレコードの行の順に並べ替える。レイアウトがないか、ラベルの集合が型と違えば、ラベルの表示名の順にし、表示名が同じなら内部名の順にする。
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
        // バックエンドのペアのオプションと `--strict-exhaustive` は、受け付けて無視する(入力はインタプリタで実行する)。
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
