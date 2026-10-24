//! サブコマンド `keleut dump lir` である(計画 261024-1 の 6.2 節、6.4 節)。
//!
//! `keleut dump lir [OPTIONS] FILE` は、KIR を読んで検証し、LIR に下ろして検証し、LIR を印字する。
//! `FILE` の読み方(拡張子とマジック)は `keleut kir` と同じである。
//! 印字の形は調べるためのもので、互換性を約束しない。
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--check`: 下ろしと検証だけを行い、何も印字しない(コーパスの全件の確認に使う)。
//! - `--unassigned`: パス 12(生存区間とスロット)の前の LIR を印字する。
//! - `--max-nest N`: 入れ子の限度(既定は IRSpec 13.9 節の `N_nest = 100,000`)。
//! - `-o OUT`: 印字の出力先(省けば標準出力)。
//!
//! 終了状態は、KIR の読み込みと検証の失敗が 65、下ろしと LIR の検証の失敗(処理系の不具合)が 70、
//! 使い方の誤りと入力を読めないときが 64、出力に書けないときが 74 である。

use crate::exit;
use std::io::Write;
use std::path::PathBuf;

/// `keleut dump lir` の使い方である。
pub const USAGE: &str = "\
usage: keleut dump lir [OPTIONS] FILE

FILE is binary KIR (.kir) or text KIR (.kirt); for other names the magic decides.
options:
  --check          lower and verify only; print nothing
  --unassigned     print LIR before pass 12 (no frame layout)
  --max-nest N     node depth limit (default 100000)
  -o FILE          output file (default standard output)
";

/// 解釈したコマンド行である。
#[derive(Debug, PartialEq, Eq)]
struct Options {
    input: PathBuf,
    output: Option<PathBuf>,
    check: bool,
    unassigned: bool,
    max_nest: usize,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        input: PathBuf::new(),
        output: None,
        check: false,
        unassigned: false,
        max_nest: kir::N_NEST,
    };
    let mut input = None;
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
            "--check" => o.check = true,
            "--unassigned" => o.unassigned = true,
            "-o" => o.output = Some(PathBuf::from(value("-o")?)),
            "--max-nest" => {
                let v = value("--max-nest")?;
                o.max_nest = match v.parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => return Err(format!("bad --max-nest value: {v}")),
                };
            }
            _ if a.starts_with('-') && a != "-" => return Err(format!("unknown option: {a}")),
            _ => {
                if input.is_some() {
                    return Err("only one input file is allowed".to_string());
                }
                input = Some(PathBuf::from(a));
            }
        }
        i += 1;
    }
    o.input = input.ok_or("missing input file")?;
    if o.check && o.output.is_some() {
        return Err("--check takes no -o".to_string());
    }
    Ok(o)
}

fn run(o: &Options) -> u8 {
    let data = match std::fs::read(&o.input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("keleut: {}: {e}", o.input.display());
            return exit::USAGE;
        }
    };
    let limits = kir::Limits::for_nest(o.max_nest);
    let read = match o.input.extension().and_then(|e| e.to_str()) {
        Some("kir") => kir::read_kir_with,
        Some("kirt") => kir::read_kirt_with,
        _ => kir::read_any_with,
    };
    let prog = match read(&data, &limits) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("keleut: KIR を読み込めません: {}", e.message());
            return e.exit_code() as u8;
        }
    };
    if let Err(e) = kir::verify_with(&prog, &limits) {
        eprintln!("keleut: KIR の検証に失敗しました: {}", e.message());
        return e.exit_code() as u8;
    }
    let lowered = if o.unassigned {
        keleut_lir::lower_unassigned(&prog, o.max_nest)
    } else {
        keleut_lir::lower_with(&prog, o.max_nest)
    };
    let lp = match lowered {
        Ok(p) => p,
        Err(e) => {
            eprintln!("keleut: {e}");
            return e.exit_code() as u8;
        }
    };
    if o.check {
        return 0;
    }
    let text = keleut_lir::print(&lp);
    let r = match &o.output {
        Some(p) => std::fs::write(p, text.as_bytes()).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(text.as_bytes())
                .and_then(|()| out.flush())
                .map_err(|e| e.to_string())
        }
    };
    match r {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("keleut: LIR を書き出せません: {msg}");
            exit::IO
        }
    }
}

/// `keleut dump lir …` の入口である。引数の先頭は `lir` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return 0;
    }
    match parse(rest) {
        Ok(o) => run(&o),
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            exit::USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn コマンド行を解釈する() {
        let o = parse(&argv(&["--check", "--max-nest", "10", "a.kir"])).unwrap();
        assert!(o.check);
        assert_eq!(o.max_nest, 10);
        assert_eq!(o.input, PathBuf::from("a.kir"));
        for a in [
            &[][..],
            &["a", "b"],
            &["--check", "-o", "x", "a"],
            &["--max-nest", "0", "a"],
            &["--bogus", "a"],
        ] {
            assert!(parse(&argv(a)).is_err(), "{a:?}");
        }
    }
}
