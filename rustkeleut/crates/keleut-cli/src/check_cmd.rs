//! `keleut check` である(計画 261024-1 の 6.2 節。M97 で型検査の核を移した)。
//!
//! | 形 | 働き |
//! |---|---|
//! | `keleut check [OPTIONS] FILE...` | 型検査し、トップレベルの束縛の型を `名前 : 型` の行で印字する(`diktor --type-check` と同じ形) |
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--no-prelude`、`--prelude PATH`、`--strict-exhaustive`:Diktor の同名の旗と同じ。
//! - `--quiet`:Diktor の実行の前段の型検査と同じく、型の行を出さず、警告と誤りを標準エラーに出す。
//! - `--max-nest N`:構文解析で作る木の深さの限度(既定は 200,000)。
//!   1 段あたり [`CHECK_STACK_BYTES_PER_LEVEL`](keleut_front::types::check::CHECK_STACK_BYTES_PER_LEVEL) と 64 MiB の見込みが 8 GiB を超える値は、使い方の誤りにする。
//! - `--native-stack BYTES`:型検査を走らせる専用のスレッドのスタックの大きさ(既定は 8 GiB)。
//!   `k`、`m`、`g` の接尾辞を使える。
//!   与えたスタックで型検査を終えられる深さまで `--max-nest` を下げ、
//!   その深さが [`MIN_CHECK_NEST`](keleut_front::types::check::MIN_CHECK_NEST) に満たない小さい値と、8 GiB を超える値は使い方の誤りにする。
//!   計算の規則は [`check_limits`] にある。
//! - `--import-path DIR`:import の検索パスに `DIR` を足す。繰り返して書け、書いた順に探す(Diktor の同名の旗と同じ)。
//!
//! 終了状態は Diktor と同じで、型エラーと import エラーは 1、字句と構文の誤りは 2、内部の誤りは 3、未実装は 4、
//! 入力や import 先を開けないときと使い方の誤りは 64、標準出力に書けないときは 74 である。

use crate::exit;
use crate::front_cmd::{MAX_STACK_BYTES, io_msg, parse_bytes, stdout_closed};
use keleut_front::Limits;
use keleut_front::types::OutLine;
use keleut_front::types::check::{
    CheckOptions, Stop, check_limits, check_program, on_check_stack, render_line,
};
use keleut_front::types::load::StdFs;
use std::io::Write;

/// 使い方である。
pub const USAGE: &str = "\
usage: keleut check [OPTIONS] FILE...
options:
  --no-prelude           do not load the prelude
  --prelude PATH         use PATH as the prelude
  --import-path DIR      add DIR to the import search path (repeatable)
  --strict-exhaustive    treat exhaustiveness warnings as errors
  --quiet                print only warnings and errors, to stderr (like diktor before running)
  --max-nest N           parser tree depth limit (default 200000)
  --native-stack BYTES   stack size of the checker thread (suffix k, m or g; default 8g;
                         lowers --max-nest to what fits)
";

struct Options {
    check: CheckOptions,
    strict: bool,
    quiet: bool,
    limits: Limits,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut files = Vec::new();
    let mut max_nest = keleut_front::DEFAULT_MAX_NEST;
    let mut stack = None;
    let mut no_prelude = false;
    let mut prelude = None;
    let mut strict = false;
    let mut quiet = false;
    let mut import_path = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--no-prelude" => no_prelude = true,
            "--strict-exhaustive" => strict = true,
            "--quiet" => quiet = true,
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
        quiet,
        limits,
    })
}

/// 標準エラーに 1 行を書き出す(OCaml の `prerr_endline`)。
fn eprintln_now(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = writeln!(e, "{s}");
    let _ = e.flush();
}

/// 型検査の本体である。終了状態を返す。
fn run_check(o: &Options) -> u8 {
    // 型検査の文脈と木は捨てずに、プロセスの終わりまで残す。
    // 大きい入力では、何十万もの節点と表を解放する時間が型検査の時間の 1 割近くになるからである。
    let r = check_program(&o.check, &mut StdFs).map(|mut c| {
        let out = std::mem::take(&mut c.output);
        std::mem::forget(c);
        out
    });
    // 型検査の間に確保が大域の確保器の予備を使ったら、型の行を書く前に限度の行で終える(`keleut_rt::alloc`)。
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::Check);
    let out = match r {
        Ok(out) => out,
        Err(Stop::Input(m)) => {
            eprintln_now(&format!("keleut: ファイルを開けません: {m}"));
            return exit::USAGE;
        }
        Err(Stop::Lex(m)) => {
            eprintln_now(&m);
            return 2;
        }
        Err(Stop::Syntax(m)) => {
            eprintln_now(&m);
            return 2;
        }
        Err(Stop::Internal(m)) => {
            eprintln_now(&format!("内部エラー: 予期しない例外です: {m}"));
            return 3;
        }
    };
    if let Some(p) = &out.panic {
        eprintln_now(p);
        return 3;
    }
    let mut stdout = Vec::new();
    for l in &out.lines {
        if o.quiet {
            if let OutLine::Warning(_) = l {
                eprintln_now(&render_line(l));
            }
        } else {
            stdout.extend_from_slice(render_line(l).as_bytes());
            stdout.push(b'\n');
        }
    }
    let code = match &out.error {
        None => {
            if o.strict && out.lines.iter().any(|l| matches!(l, OutLine::Warning(_))) {
                1
            } else {
                0
            }
        }
        Some(e) => {
            if o.quiet {
                eprintln_now(&e.render());
            } else {
                stdout.extend_from_slice(e.render().as_bytes());
                stdout.push(b'\n');
            }
            e.exit
        }
    };
    if !stdout.is_empty() {
        let mut so = std::io::stdout().lock();
        let r = match stdout_closed() {
            Some(e) => Err(e),
            None => so.write_all(&stdout).and_then(|_| so.flush()),
        };
        if let Err(e) = r {
            drop(so);
            eprintln_now(&format!("keleut: 標準出力に書き出せません: {}", io_msg(&e)));
            return exit::IO;
        }
    }
    code
}

/// `keleut check …`(引数の先頭は `check`)。
pub fn main(args: &[String]) -> u8 {
    let o = match parse_options(&args[1..]) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    let limits = o.limits;
    keleut_rt::alloc::set_input(keleut_rt::alloc::Input::Kel);
    let r = on_check_stack(limits, move || {
        keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Conv);
        let _site = keleut_rt::alloc::site(keleut_rt::alloc::Site::Check);
        run_check(&o)
    });
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::Check);
    match r {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("keleut: {msg}");
            70
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
        let o = opts(&["--no-prelude", "--strict-exhaustive", "a.kel", "b.kel"]).unwrap();
        assert!(o.check.no_prelude && o.strict && !o.quiet);
        assert_eq!(o.check.files, vec!["a.kel", "b.kel"]);
        assert!(opts(&["--prelude"]).is_err());
        assert!(opts(&["--typo", "a.kel"]).is_err());
        assert!(opts(&[]).is_err());
        assert_eq!(
            opts(&["--import-path", "x", "--import-path", "y", "a.kel"])
                .unwrap()
                .check
                .import_path,
            vec!["x", "y"]
        );
        assert!(opts(&["a.kel", "--import-path"]).is_err());
        assert!(opts(&["--native-stack", "9g", "a.kel"]).is_err());
    }

    #[test]
    fn スタックの大きさから深さの限度を決める() {
        let o = opts(&["a.kel"]).unwrap();
        assert_eq!(o.limits.max_nest, keleut_front::DEFAULT_MAX_NEST);
        assert_eq!(o.limits.stack_bytes, 8 << 30);
        assert_eq!(o.check.max_nest, o.limits.max_nest);
        // 小さいスタックでは、収まる深さまで限度を下げる。
        let o = opts(&["--native-stack", "4m", "a.kel"]).unwrap();
        assert!(o.limits.max_nest >= 64 && o.limits.max_nest < 2000);
        assert_eq!(o.check.max_nest, o.limits.max_nest);
        // 型検査の余白とプレリュードの深さに満たないスタックと、8 GiB に収まらない深さは使い方の誤りである。
        assert!(opts(&["--native-stack", "256k", "a.kel"]).is_err());
        assert!(opts(&["--native-stack", "300k", "a.kel"]).is_err());
        assert!(opts(&["--max-nest", "4000000000", "a.kel"]).is_err());
    }
}
