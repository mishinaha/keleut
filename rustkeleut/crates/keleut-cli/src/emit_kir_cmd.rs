//! サブコマンド `keleut emit-kir` である。
//!
//! | 書式 | 動作 |
//! |---|---|
//! | `keleut emit-kir [OPTIONS] FILE...` | 型検査し、KIR のバイナリ(`--text` ならテキスト)を書く(`diktor --emit-ir`、`--emit-ir-text` と同じバイト列) |
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--text`: 正準なテキスト(`.kirt`)を書く。指定しなければバイナリ(`.kir`)を書く。
//! - `-o FILE`: 出力先のファイル。指定しなければ標準出力に書く。
//! - `--no-prelude`、`--prelude PATH`、`--import-path DIR`、`--strict-exhaustive`、`--max-nest N`、`--native-stack BYTES`: `keleut check` の同名のオプションと同じ。
//!
//! 手順と終了コードは Diktor の `--emit-ir` と同じである。
//!
//! 1. 型検査する。
//!    警告とエラーは標準エラーに書く(Diktor が実行の前に行う型検査と同じ)。
//!    型エラーと import エラーは 1、字句エラーと構文エラーは 2、内部エラーは 3、未実装は 4、入力を開けないときは 64 で終了する。
//!    `--strict-exhaustive` を指定して警告が残れば、1 で終了する。
//! 2. 単位(コンパイル単位)のキーを決める。2 つのファイルのキーが同じになれば、64 で終了する。
//! 3. KIR を作り、先にバイナリに符号化する。
//!    ノードの深さが入れ子の上限を超えるプログラムは、何も書かずに 1 で終了する。
//! 4. 書き出す。書き出せなければ、74 で終了する。
//!
//! 出力先のファイルは、手順 3 までがすべて済んでから開く。
//! 書き出す前に終了したときは、ファイルを作らない。
//! 開いた後に書けなくなったときは、Diktor と同じく書きかけのファイルが残る。
//! テキストは、全体をメモリに置かずに出力先へ順に書く。
//! 標準出力に書くときに、標準出力がプロセスの開始時から閉じていれば、Diktor と同じく 74 で終了する。

use crate::exit;
use crate::front_cmd::{MAX_STACK_BYTES, io_msg, parse_bytes, stdout_closed};
use keleut_front::Limits;
use keleut_front::types::check::{
    CHECK_STACK_BYTES_PER_LEVEL, CheckOptions, MIN_CHECK_NEST, Stop, check_limits, check_program,
    on_check_stack, render_line,
};
use keleut_front::types::emit::program_of_checked;
use keleut_front::types::load::{StdFs, UnitKeyError};
use keleut_front::types::{Fail, OutLine};
use std::io::Write;

/// `keleut emit-kir` の使い方の説明(usage)である。
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
    /// `--native-stack` を指定していれば真である。
    /// 指定したときは、[`build_on_front_stack`] がスタックを小さくしない。
    /// 仮想メモリの上限の 4 分の 1 への切り下げも、スレッドを作れないときに半分ずつ小さくする作り直しも行わない。
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

/// 標準エラーに 1 行を書き、すぐに flush する。
fn eprintln_now(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = writeln!(e, "{s}");
    let _ = e.flush();
}

/// KIR を書き出す段階で処理系の限度を超えたときの診断を書き、終了コード 1 を返す(Diktor の `encode_ir` の `Limit_error` に対応する)。
fn write_limit(msg: &str) -> u8 {
    eprintln_now(&format!("keleut: KIR を書き出せません: {msg}"));
    1
}

/// 書き出しのエラーが処理系の限度なら真を返す。
///
/// 処理系の限度になるのは、入れ子の上限を超えた場合と、最も小さいスタックでも KIR を処理するスレッドを作れない場合である。
/// どちらも、`.kel` の入力を実行の前に変換する段階で起きる処理系の限度なので、終了コード 1 で終了する(LangSpec §15.5)。
fn is_write_limit(e: &kir::Error) -> bool {
    matches!(e.kind(), kir::ErrorKind::WriteLimit | kir::ErrorKind::Limit)
}

/// フロントエンドのスレッドが失敗したときの診断を書き、終了コードを返す。
///
/// 仮想メモリを制限しているとき(`ulimit -v`)にスレッドを作れないのは、型検査に必要なメモリが処理系の限度を超えた場合である。
/// このときは `処理系の限度` の行を書き、終了コード 1 を返す(LangSpec §15.5)。
/// 制限していないのに作れないときと、それ以外のエラー(スレッドのパニック)は内部エラーで、70 を返す。
fn front_failure(msg: &str) -> u8 {
    if msg.starts_with(SPAWN_FAILED) && kir::address_space_limit().is_some() {
        eprintln_now(&format!("keleut: 処理系の限度: {msg}"));
        1
    } else {
        eprintln_now(&format!("keleut: {msg}"));
        70
    }
}

/// 型検査して KIR を作る。途中で終了するときは、終了コードを `Err` で返す。
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
    // 型検査の間に確保に失敗してグローバルアロケータが予備領域を使っていたら、`処理系の限度` の行を書いて終了する(`keleut_rt::alloc::conv_checkpoint`)。
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::Check);
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
    // 型検査を済ませたプログラム(型検査の文脈と木)は、KIR を作り終えたらこのスレッドで解放する。
    // `keleut check` と違い、この後に KIR の処理と実行が続くので、残すと実行の間ずっとメモリを占め、仮想メモリを制限しているとき(`ulimit -v`)に実行に使えるメモリが減る。
    // 木を解放する再帰は、構文解析の上限に合わせたこのスレッドのスタックに収まる。
    drop(c);
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

/// 出力先を開く。
///
/// 開けないときのメッセージは、Diktor と同じく `パス: 理由` である。
/// 標準出力がプロセスの開始時から閉じていたときは、Diktor と同じく書き込みエラーにする([`stdout_closed`])。
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

/// 書き出せないことを報告して、終了コード 74 を返す。
fn output_error(what: &str, msg: &str) -> u8 {
    eprintln_now(&format!("keleut: KIR の{what}を書き出せません: {msg}"));
    exit::IO
}

/// OCaml の `Sys_error` のメッセージに合わせて、末尾の `(os error N)` を取り除く。
fn strip_os_error(s: &str) -> &str {
    s.rfind(" (os error ").map_or(s, |i| &s[..i])
}

/// バイナリを書き出す。
///
/// 診断のメッセージは Diktor と同じく、開けないときは `パス: 理由`、開いた後に書けないときは理由だけである。
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

/// 正準なテキストを、全体をメモリに置かずに出力先へ順に書く(Diktor の `Ir_text.output` と同じ)。
///
/// 入れ子の深さが上限に近いプログラムのテキストは数十 GB になることがあるので、全体を作ってから書くことはしない。
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
        Err(e) if is_write_limit(&e) => write_limit(e.message()),
        Err(e) => {
            eprintln_now(&format!("keleut: KIR を符号化できません: {}", e.message()));
            70
        }
    }
}

/// `keleut emit-kir …` のエントリポイントである。引数の先頭は `emit-kir` である。
pub fn main(args: &[String]) -> u8 {
    let o = match parse_options(&args[1..]) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    keleut_rt::alloc::set_input(keleut_rt::alloc::Input::Kel);
    let mut o = o;
    let (strict, limits, shrink) = (o.strict, o.limits, !o.fixed_stack);
    let prog = match build_on_front_stack(&mut o.check, strict, limits, shrink) {
        Ok(Ok(p)) => p,
        Ok(Err(code)) => return code,
        Err(msg) => return front_failure(&msg),
    };
    // テキストだけを書くときも、先にバイナリに符号化して深さを検査する(Diktor と同じ順)。
    let bin = match encode(&prog) {
        Ok(b) => b,
        Err(e) if is_write_limit(&e) => return write_limit(e.message()),
        Err(e) => {
            eprintln_now(&format!("keleut: KIR を符号化できません: {}", e.message()));
            return 70;
        }
    };
    if o.text {
        // 深さの検査は済んだので、バイナリを解放してからテキストを書く。
        drop(bin);
        write_text(&o, &prog)
    } else {
        write_binary(&o, &bin)
    }
}

/// フロントエンドの専用のスレッドで型検査して、KIR を作る。
///
/// `shrink` が真なら、仮想メモリの制限(`ulimit -v`)に合わせてスタックを小さくする。
/// 既定のスタック(8 GiB)は、仮想メモリを制限していると予約できない。
/// [`on_check_stack`] はそのとき、深さの上限に必要なスタック(リリースビルドで約 455 MiB)で作り直す。
/// feature `llvm` を有効にしてビルドした `keleut` では `libLLVM.so` が仮想メモリの大部分を使うので、仮想メモリの上限が 512 MiB のときはその作り直しも失敗する。
/// 予約できたときも、ヒープに使えるメモリが足りなくなることがある。
/// そこで、仮想メモリを制限しているときは、初めからスタックを仮想メモリの上限の 4 分の 1 以下にする(16 MiB より小さくはしない)。
/// それでもスレッドを作れなければ、スタックを半分ずつ小さくして作り直す(下限は [`MIN_RUN_FRONT_STACK`])。
/// どちらの場合も、`--native-stack` と同じ規則で、そのスタックに収まる深さまで `check.max_nest` を下げる。
/// 構文解析の上限は処理系の限度なので、下げても言語仕様に反しない(LangSpec §15.5)。
fn build_on_front_stack(
    check: &mut CheckOptions,
    strict: bool,
    limits: Limits,
    shrink: bool,
) -> Result<Result<kir::Program, u8>, String> {
    let max_nest = check.max_nest;
    let mut limits = limits;
    if shrink
        && let Some(cap) = kir::address_space_limit().map(|b| b / 4)
        && cap < limits.stack_bytes
        && let Ok(l) = check_limits(max_nest, Some(cap.max(MIN_RUN_FRONT_STACK)))
    {
        limits = l;
        check.max_nest = l.max_nest;
    }
    let mut r = on_check_stack(limits, || front_thread(check, strict));
    let mut stack = limits.stack_bytes;
    while shrink
        && let Err(msg) = &r
        && msg.starts_with(SPAWN_FAILED)
        && stack > MIN_RUN_FRONT_STACK
    {
        stack = (stack / 2).max(MIN_RUN_FRONT_STACK);
        let Ok(l) = check_limits(max_nest, Some(stack)) else {
            break;
        };
        check.max_nest = l.max_nest;
        r = on_check_stack(l, || front_thread(check, strict));
    }
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::Check);
    r
}

/// フロントエンドのスレッドの本体である。
///
/// スレッドの役割を変換スレッド(`Role::Conv`)に、確保箇所を `check` に設定してから、[`build`] を呼ぶ(`keleut_rt::alloc`)。
fn front_thread(check: &CheckOptions, strict: bool) -> Result<kir::Program, u8> {
    keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Conv);
    let _site = keleut_rt::alloc::site(keleut_rt::alloc::Site::Check);
    build(check, strict)
}

/// KIR をバイナリに符号化する。
///
/// 符号化は KIR を処理するスレッドで行うので、プロセス全体で共有する確保箇所を `kir` に設定しておく(`keleut_rt::alloc::conv_site`)。
/// 符号化の後に、変換スレッドのフラグを調べる(`keleut_rt::alloc::conv_checkpoint`)。
fn encode(prog: &kir::Program) -> Result<Vec<u8>, kir::Error> {
    let r = {
        let _site = keleut_rt::alloc::conv_site(keleut_rt::alloc::Site::Kir);
        kir::write_kir(prog)
    };
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::KirWrite);
    r
}

/// フロントエンドのスレッドを作れなかったときに [`on_check_stack`] が返す、エラーメッセージの先頭の部分である。
const SPAWN_FAILED: &str = "フロントエンドのスレッドを作れません";

/// フロントエンドのスレッドを作り直すときの、スタックサイズの下限(16 MiB)である。
const MIN_RUN_FRONT_STACK: usize = 16 << 20;

/// フロントエンドのスレッドに使える、最も小さいスタックサイズ(バイト)を返す。
///
/// 構文解析の上限を、その最小値である [`MIN_CHECK_NEST`] 段まで下げたときに必要なサイズである。
/// リリースビルドで約 384 KiB、デバッグビルドで約 2.3 MiB である。
fn min_front_stack() -> usize {
    let mut s = MIN_CHECK_NEST * CHECK_STACK_BYTES_PER_LEVEL;
    while check_limits(MIN_CHECK_NEST, Some(s)).is_err() {
        s += 64 << 10;
    }
    s
}

/// `keleut run` の `--native-stack` から、フロントエンドのスレッドのスタックサイズを決める。
///
/// 指定された値を、[`min_front_stack`] 以上、[`MAX_STACK_BYTES`] 以下の範囲に収める。
/// 小さい値を使い方のエラーにしないのは、同じ値がインタプリタと JIT のスタックサイズでもあるからである。
/// たとえば、実行のために指定した `--native-stack 512k` は、デバッグビルドではフロントエンドに足りない。
pub(crate) fn front_stack_for_run(native_stack: u64) -> usize {
    usize::try_from(native_stack)
        .unwrap_or(usize::MAX)
        .clamp(min_front_stack(), MAX_STACK_BYTES)
}

/// `keleut run --frontend rust` のフロントエンドである。
///
/// `front_args` は、`keleut run` が受け取ったフロントエンドのオプション(`--no-prelude`、`--prelude PATH`、`--import-path DIR`、`--strict-exhaustive`)である。
/// 手順、診断、終了コードは `diktor --emit-ir` と同じで、KIR をファイルに書く代わりに返す。
/// 入れ子の深さは、バイナリに符号化して検査する。
///
/// `front_stack` は、`keleut run` の `--native-stack` から決めたフロントエンドのスタックサイズである([`front_stack_for_run`])。
/// 値があれば、そのスタックで実行し、`keleut check --native-stack` と同じく、収まる深さまで構文解析の上限を下げる。
/// `None` なら、既定のサイズのスタックで実行する。
/// このとき仮想メモリを制限していれば(`ulimit -v`)、初めからスタックを仮想メモリの上限の 4 分の 1 以下にし、収まる深さまで構文解析の上限を下げる。
/// スレッドを作れなければ、スタックを半分ずつ小さくして作り直す(下限は 16 MiB。[`build_on_front_stack`])。
pub(crate) fn program_for_run(
    files: &[std::path::PathBuf],
    front_args: &[String],
    front_stack: Option<usize>,
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
    let limits = match check_limits(check.max_nest, front_stack) {
        Ok(l) => l,
        Err(msg) => {
            eprintln_now(&format!("keleut: {msg}"));
            return Err(exit::USAGE);
        }
    };
    check.max_nest = limits.max_nest;
    let r = build_on_front_stack(&mut check, strict, limits, front_stack.is_none());
    let prog = match r {
        Ok(r) => r?,
        Err(msg) => return Err(front_failure(&msg)),
    };
    match encode(&prog) {
        Ok(_) => Ok(prog),
        Err(e) if is_write_limit(&e) => Err(write_limit(e.message())),
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

    #[test]
    fn 実行の_native_stack_からフロントエンドのスタックを決める() {
        let min = min_front_stack();
        assert!(check_limits(MIN_CHECK_NEST, Some(min)).is_ok());
        assert!(check_limits(MIN_CHECK_NEST, Some(min - (64 << 10))).is_err());
        assert_eq!(front_stack_for_run(512 << 10), (512 << 10).max(min));
        assert_eq!(front_stack_for_run(64 << 20), 64 << 20);
        assert_eq!(front_stack_for_run(1 << 40), MAX_STACK_BYTES);
    }
}
