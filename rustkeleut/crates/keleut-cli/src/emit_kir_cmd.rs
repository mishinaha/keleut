//! `keleut emit-kir` である(計画 261024-1 の 6.2 節。M99 で KIR の書き手を移した)。
//!
//! | 形 | 働き |
//! |---|---|
//! | `keleut emit-kir [OPTIONS] FILE...` | 型検査し、KIR のバイナリ(`--text` ならテキスト)を書く(`diktor --emit-ir`、`--emit-ir-text` と同じバイト列) |
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--text`:正準のテキスト(`.kirt`)を書く。無ければバイナリ(`.kir`)を書く。
//! - `-o FILE`:書き出す先。無ければ標準出力に書く。
//! - `--no-prelude`、`--prelude PATH`、`--import-path DIR`、`--strict-exhaustive`、`--max-nest N`、`--native-stack BYTES`:
//!   `keleut check` の同名の旗と同じ。
//!
//! 手順と終了状態は Diktor の `--emit-ir` と同じである。
//!
//! 1. 型検査する。
//!    警告と誤りは標準エラーに書き(Diktor が実行の前段で行う型検査と同じ)、
//!    型エラーと import エラーは 1、字句と構文の誤りは 2、内部の誤りは 3、未実装は 4、入力を開けないときは 64 で終わる。
//!    `--strict-exhaustive` で警告が残れば 1 で終わる。
//! 2. 単位の鍵を決める。別のファイルが同じ鍵になれば 64 で終わる。
//! 3. KIR を作り、先にバイナリに符号化する。
//!    ノードの深さが入れ子の限度を超えるプログラムは、何も書かずに 1 で終わる。
//! 4. 書き出す。書き出せなければ 74 で終わる。
//!
//! 出力先のファイルは、すべてが済んでから開く。
//! 書き出す前に終わったときは、ファイルを作らない。
//! 開いた後に書けなくなったときは、Diktor と同じく書きかけのファイルが残る。
//! テキストは全体をメモリに置かずに流しながら書く。
//! 標準出力に書くとき、標準出力がプロセスの始めから閉じていれば、Diktor と同じく 74 で終わる。

use crate::exit;
use crate::front_cmd::{MAX_STACK_BYTES, io_msg, parse_bytes, stdout_closed};
use keleut_front::Limits;
use keleut_front::types::check::{
    CheckOptions, Stop, check_limits, check_program, on_check_stack, render_line,
};
use keleut_front::types::emit::program_of_checked;
use keleut_front::types::load::{StdFs, UnitKeyError};
use keleut_front::types::{Fail, OutLine};
use std::io::Write;

/// 使い方である。
pub const USAGE: &str = "\
usage: keleut emit-kir [OPTIONS] FILE...
options:
  --text                 write the canonical text (.kirt) instead of the binary (.kir)
  -o FILE                write to FILE (default: standard output)
  --no-prelude           do not load the prelude
  --prelude PATH         use PATH as the prelude
  --import-path DIR      add DIR to the import search path (repeatable)
  --strict-exhaustive    treat exhaustiveness warnings as errors
  --max-nest N           parser tree depth limit (default 200000)
  --native-stack BYTES   stack size of the front-end thread (suffix k, m or g; default 8g;
                         lowers --max-nest to what fits)
";

struct Options {
    check: CheckOptions,
    strict: bool,
    text: bool,
    out: Option<String>,
    limits: Limits,
    /// `--native-stack` を与えたか(与えたときは、スタックを小さくして作り直さない)。
    fixed_stack: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut files = Vec::new();
    let mut max_nest = keleut_front::DEFAULT_MAX_NEST;
    let mut stack = None;
    let mut no_prelude = false;
    let mut prelude = None;
    let mut strict = false;
    let mut text = false;
    let mut out = None;
    let mut import_path = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--text" => text = true,
            "--no-prelude" => no_prelude = true,
            "--strict-exhaustive" => strict = true,
            "-o" => {
                i += 1;
                out = Some(args.get(i).ok_or("-o requires a path")?.clone());
            }
            "--prelude" => {
                i += 1;
                prelude = Some(args.get(i).ok_or("--prelude requires a path")?.clone());
            }
            "--max-nest" => {
                i += 1;
                let v = args.get(i).ok_or("--max-nest needs a value")?;
                max_nest = match v.parse::<usize>() {
                    Ok(n) if n >= 1 && n <= u32::MAX as usize => n,
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
            "--import-path" => {
                i += 1;
                import_path.push(
                    args.get(i)
                        .ok_or("--import-path requires a directory")?
                        .clone(),
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
    let limits = check_limits(max_nest, stack)?;
    Ok(Options {
        check: CheckOptions {
            no_prelude,
            prelude,
            files,
            import_path,
            max_nest: limits.max_nest,
        },
        strict,
        text,
        out,
        limits,
        fixed_stack: stack.is_some(),
    })
}

/// 標準エラーに 1 行を書き出す。
fn eprintln_now(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = writeln!(e, "{s}");
    let _ = e.flush();
}

/// 書き出しの段の入れ子の限度の診断(Diktor の `encode_ir` の `Limit_error`)。
fn write_limit(msg: &str) -> u8 {
    eprintln_now(&format!("keleut: KIR を書き出せません: {msg}"));
    1
}

/// 型検査して KIR を作る。終わるときは終了状態を `Err` で返す。
///
/// 型検査のスレッドの中で呼ぶ。
fn build(check: &CheckOptions, strict: bool) -> Result<kir::Program, u8> {
    let mut fs = StdFs;
    let mut c = match check_program(check, &mut fs) {
        Ok(c) => c,
        Err(Stop::Input(m)) => {
            eprintln_now(&format!("keleut: ファイルを開けません: {m}"));
            return Err(exit::USAGE);
        }
        Err(Stop::Lex(m)) | Err(Stop::Syntax(m)) => {
            eprintln_now(&m);
            return Err(2);
        }
        Err(Stop::Internal(m)) => {
            eprintln_now(&format!("内部エラー: 予期しない例外です: {m}"));
            return Err(3);
        }
    };
    let out = std::mem::take(&mut c.output);
    if let Some(p) = &out.panic {
        eprintln_now(p);
        return Err(3);
    }
    for l in &out.lines {
        if let OutLine::Warning(_) = l {
            eprintln_now(&render_line(l));
        }
    }
    if let Some(e) = &out.error {
        eprintln_now(&e.render());
        return Err(e.exit);
    }
    if strict && out.lines.iter().any(|l| matches!(l, OutLine::Warning(_))) {
        return Err(1);
    }
    let keys = match c.unit_keys(&mut fs, check) {
        Ok(k) => k,
        Err(UnitKeyError::Clash(m)) => {
            eprintln_now(&format!("keleut: {m}"));
            return Err(exit::USAGE);
        }
        Err(UnitKeyError::Input(m)) => {
            eprintln_now(&format!("keleut: ファイルを開けません: {m}"));
            return Err(exit::USAGE);
        }
        Err(UnitKeyError::Bug(m)) => {
            eprintln_now(&m);
            return Err(3);
        }
    };
    let r = program_of_checked(&mut c, &keys, check);
    // 型検査の文脈と木は捨てずにプロセスの終わりまで残す(`keleut check` と同じ理由)。
    std::mem::forget(c);
    match r {
        Ok(p) => Ok(p),
        Err(Fail::Limit) => Err(write_limit(kir::WRITE_LIMIT_MESSAGE)),
        Err(Fail::Bug(m)) => {
            eprintln_now(&m);
            Err(3)
        }
        Err(f) => {
            eprintln_now(&format!("[BUG] KIR の書き出し: {f:?}"));
            Err(3)
        }
    }
}

/// 書き出す先を開く。
///
/// 開けないときの文面は Diktor と同じく `パス: 理由` である。
/// 標準出力がプロセスの始めから閉じていたときは、Diktor と同じく書けない誤りにする([`stdout_closed`])。
fn open_out(o: &Options) -> Result<Box<dyn Write + Send>, String> {
    match &o.out {
        Some(path) => match std::fs::File::create(path) {
            Ok(f) => Ok(Box::new(f)),
            Err(e) => Err(format!("{path}: {}", io_msg(&e))),
        },
        None => match stdout_closed() {
            Some(e) => Err(io_msg(&e)),
            None => Ok(Box::new(std::io::stdout())),
        },
    }
}

/// 書き出せない誤りを報告して 74 を返す。
fn output_error(what: &str, msg: &str) -> u8 {
    eprintln_now(&format!("keleut: KIR の{what}を書き出せません: {msg}"));
    exit::IO
}

/// OCaml の `Sys_error` の文面に合わせて、末尾の `(os error N)` を除く。
fn strip_os_error(s: &str) -> &str {
    s.rfind(" (os error ").map_or(s, |i| &s[..i])
}

/// バイナリを書き出す。
///
/// 診断の文面は Diktor と同じく、開けないときは `パス: 理由`、開いた後に書けないときは理由だけである。
fn write_binary(o: &Options, data: &[u8]) -> u8 {
    const WHAT: &str = "バイナリ";
    let mut w = match open_out(o) {
        Ok(w) => w,
        Err(m) => return output_error(WHAT, &m),
    };
    match w.write_all(data).and_then(|_| w.flush()) {
        Ok(()) => 0,
        Err(e) => output_error(WHAT, &io_msg(&e)),
    }
}

/// 正準のテキストを、全体をメモリに置かずに流しながら書き出す(Diktor の `Ir_text.output` と同じ)。
///
/// 入れ子の深さが限度に近いプログラムのテキストは数十 GB になりうるので、組み上げてから書くことはしない。
/// 開いた後に書けなくなったときは、Diktor と同じく書きかけのファイルを残す。
fn write_text(o: &Options, prog: &kir::Program) -> u8 {
    const WHAT: &str = "テキスト";
    let mut w = match open_out(o) {
        Ok(w) => w,
        Err(m) => return output_error(WHAT, &m),
    };
    match kir::write_kirt_to(prog, &kir::Limits::default(), &mut *w) {
        Ok(()) => 0,
        Err(e) if e.kind() == kir::ErrorKind::Output => {
            output_error(WHAT, strip_os_error(e.message()))
        }
        Err(e) if e.kind() == kir::ErrorKind::WriteLimit => write_limit(e.message()),
        Err(e) => {
            eprintln_now(&format!("keleut: KIR を符号化できません: {}", e.message()));
            70
        }
    }
}

/// `keleut emit-kir …`(引数の先頭は `emit-kir`)。
pub fn main(args: &[String]) -> u8 {
    let o = match parse_options(&args[1..]) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    let mut o = o;
    let (strict, limits, shrink) = (o.strict, o.limits, !o.fixed_stack);
    let prog = match build_on_front_stack(&mut o.check, strict, limits, shrink) {
        Ok(Ok(p)) => p,
        Ok(Err(code)) => return code,
        Err(msg) => {
            eprintln_now(&format!("keleut: {msg}"));
            return 70;
        }
    };
    // テキストだけを書くときも、先にバイナリに符号化して深さを確かめる(Diktor と同じ順)。
    let bin = match kir::write_kir(&prog) {
        Ok(b) => b,
        Err(e) if e.kind() == kir::ErrorKind::WriteLimit => return write_limit(e.message()),
        Err(e) => {
            eprintln_now(&format!("keleut: KIR を符号化できません: {}", e.message()));
            return 70;
        }
    };
    if o.text {
        // 深さは確かめ終えたので、バイナリは捨ててからテキストを流す。
        drop(bin);
        write_text(&o, &prog)
    } else {
        write_binary(&o, &bin)
    }
}

/// フロントエンドの専用のスレッドで型検査して KIR を組み立てる。
///
/// `shrink` が真なら、仮想メモリの上限(`ulimit -v`)に合わせてスタックを小さくする。
/// 既定のスタック(8 GiB)は上限の下では予約できず、[`on_check_stack`] はそのとき深さの限度に見込むスタック
/// (リリースの構築で約 455 MiB)で作り直す。
/// feature `llvm` で組んだ `keleut` では `libLLVM.so` が仮想メモリの大部分を使うので、上限 512 MiB の下ではその作り直しも失敗する。
/// 予約できたときも、ヒープの残りが足りなくなることがある。
/// 上限があれば、初めからスタックを上限の 4 分の 1 までにする。
/// それでもスレッドを作れなければ、スタックを半分ずつ小さくして作り直す(下限は [`MIN_RUN_FRONT_STACK`])。
/// どちらも、`--native-stack` と同じ規則で、そのスタックに収まる深さまで `check.max_nest` を下げる(限度は I)。
fn build_on_front_stack(
    check: &mut CheckOptions,
    strict: bool,
    limits: Limits,
    shrink: bool,
) -> Result<Result<kir::Program, u8>, String> {
    let max_nest = check.max_nest;
    let mut limits = limits;
    if shrink
        && let Some(cap) = address_space_limit().map(|b| b / 4)
        && cap < limits.stack_bytes
        && let Ok(l) = check_limits(max_nest, Some(cap.max(MIN_RUN_FRONT_STACK)))
    {
        limits = l;
        check.max_nest = l.max_nest;
    }
    let mut r = on_check_stack(limits, || build(check, strict));
    let mut stack = limits.stack_bytes;
    while shrink
        && let Err(msg) = &r
        && msg.starts_with(SPAWN_FAILED)
        && stack / 2 >= MIN_RUN_FRONT_STACK
    {
        stack /= 2;
        let Ok(l) = check_limits(max_nest, Some(stack)) else {
            break;
        };
        check.max_nest = l.max_nest;
        r = on_check_stack(l, || build(check, strict));
    }
    r
}

/// 仮想メモリの軟らかい上限(`RLIMIT_AS`。バイト)を返す。上限が無ければ `None` である。
fn address_space_limit() -> Option<usize> {
    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, rlim: *mut RLimit) -> i32;
    }
    /// Linux の `RLIMIT_AS`。
    const RLIMIT_AS: i32 = 9;
    let mut r = RLimit { cur: 0, max: 0 };
    // SAFETY: r は呼び出しの間だけ使う有効な領域である。
    if unsafe { getrlimit(RLIMIT_AS, &mut r) } != 0 || r.cur == u64::MAX {
        return None;
    }
    usize::try_from(r.cur).ok()
}

/// フロントエンドのスレッドを作れなかったときの、[`on_check_stack`] の誤りの文面の始まりである。
const SPAWN_FAILED: &str = "フロントエンドのスレッドを作れません";

/// フロントエンドのスレッドを作り直すときのスタックの下限(16 MiB)である。
const MIN_RUN_FRONT_STACK: usize = 16 << 20;

/// `keleut run --frontend rust` のフロントエンドである。
///
/// `front_args` は `keleut run` が受けたフロントエンドの旗(`--no-prelude`、`--prelude PATH`、`--import-path DIR`、
/// `--strict-exhaustive`)である。
/// 手順、診断、終了状態は `diktor --emit-ir` と同じで、KIR をファイルに書く代わりに返す
/// (入れ子の深さは、バイナリに符号化して確かめる)。
/// フロントエンドのスレッドは既定の大きさのスタックで走らせる(`keleut run` の `--native-stack` はインタプリタのスタックである)。
/// 仮想メモリの上限(`ulimit -v`)があれば、スタックを初めから上限の 4 分の 1 までにし、収まる深さまで入れ子の限度を下げる。
/// スレッドを作れなければ、スタックを半分ずつ小さくして作り直す(下限は 16 MiB。[`build_on_front_stack`])。
pub(crate) fn program_for_run(
    files: &[std::path::PathBuf],
    front_args: &[String],
) -> Result<kir::Program, u8> {
    let mut check = CheckOptions {
        no_prelude: false,
        prelude: None,
        files: files.iter().map(|f| f.display().to_string()).collect(),
        import_path: Vec::new(),
        max_nest: keleut_front::DEFAULT_MAX_NEST,
    };
    let mut strict = false;
    let mut i = 0;
    while i < front_args.len() {
        match front_args[i].as_str() {
            "--no-prelude" => check.no_prelude = true,
            "--strict-exhaustive" => strict = true,
            "--prelude" => {
                i += 1;
                check.prelude = front_args.get(i).cloned();
            }
            "--import-path" => {
                i += 1;
                check.import_path.extend(front_args.get(i).cloned());
            }
            _ => {}
        }
        i += 1;
    }
    let limits = match check_limits(check.max_nest, None) {
        Ok(l) => l,
        Err(msg) => {
            eprintln_now(&format!("keleut: {msg}"));
            return Err(exit::USAGE);
        }
    };
    let r = build_on_front_stack(&mut check, strict, limits, true);
    let prog = match r {
        Ok(r) => r?,
        Err(msg) => {
            eprintln_now(&format!("keleut: {msg}"));
            return Err(70);
        }
    };
    match kir::write_kir(&prog) {
        Ok(_) => Ok(prog),
        Err(e) if e.kind() == kir::ErrorKind::WriteLimit => Err(write_limit(e.message())),
        Err(e) => {
            eprintln_now(&format!("keleut: KIR を符号化できません: {}", e.message()));
            Err(70)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(xs: &[&str]) -> Result<Options, String> {
        parse_options(&xs.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn 選択肢を読む() {
        let o = opts(&["--text", "-o", "x.kirt", "--no-prelude", "a.kel"]).unwrap();
        assert!(o.text && o.check.no_prelude && !o.strict);
        assert_eq!(o.out.as_deref(), Some("x.kirt"));
        assert_eq!(o.check.files, vec!["a.kel"]);
        let o = opts(&["a.kel", "b.kel"]).unwrap();
        assert!(!o.text && o.out.is_none());
        assert!(opts(&["-o"]).is_err());
        assert!(opts(&["--typo", "a.kel"]).is_err());
        assert!(opts(&[]).is_err());
        assert!(opts(&["--native-stack", "256k", "a.kel"]).is_err());
    }
}
