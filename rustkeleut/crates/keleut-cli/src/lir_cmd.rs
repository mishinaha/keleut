//! サブコマンド `keleut dump lir` である。
//!
//! `keleut dump lir [OPTIONS] FILE` は、KIR を読み込んで検証し、LIR に変換(lowering)して検証し、LIR をテキストで出力する。
//! `FILE` の読み方は `keleut kir` と同じで、拡張子とマジックナンバーで形式を決める。
//! 出力の形式は調査用で、互換性を保証しない。
//!
//! `OPTIONS` は次のとおりである。
//!
//! - `--check`: LIR への変換と検証だけを行い、何も出力しない(コーパスのすべてのプログラムを LIR に変換できることを確かめるときに使う)。
//! - `--unassigned`: パス 12(生存区間とスロット)の前の LIR を出力する。
//! - `--max-nest N`: 入れ子の上限(既定は IRSpec 13.9 節の `N_nest = 100,000`)。
//! - `-o OUT`: 出力先のファイル(省略すると標準出力)。
//!
//! 終了コードは、KIR の読み込みと検証の失敗が 65、LIR への変換と LIR の検証の失敗(処理系のバグ)が 70、使い方のエラーと入力を読めないときが 64、出力先に書けないときが 74 である。
//! 仮想メモリを制限しているとき(`ulimit -v`)に、スタックを小さくしても KIR を処理するスレッドか LIR に変換するスレッドを作れなければ、`処理系の限度` の行を書いて 65 で終了する。

use crate::exit;
use std::io::Write;
use std::path::PathBuf;

/// `keleut dump lir` の使い方の説明(usage)である。
pub const USAGE: &str = "\
usage: keleut dump lir [OPTIONS] FILE

FILE is binary KIR (.kir) or text KIR (.kirt); for other names the magic decides.
options:
  --check          lower and verify only; print nothing
  --unassigned     print LIR before pass 12 (no frame layout)
  --max-nest N     node depth limit (default 100000)
  -o FILE          output file (default standard output)
";

/// コマンド行を解釈した結果である。
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
    // 仮想メモリを制限しているとき(`ulimit -v`)は、スタックを仮想メモリの上限の 4 分の 1 以下にし、そのスタックに収まる深さまで入れ子の上限を下げる。
    // LIR への変換にも同じ上限を使う。
    let limits = kir::Limits::for_nest(o.max_nest).fit_address_space();
    let _site = keleut_rt::alloc::conv_site(keleut_rt::alloc::Site::Kir);
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
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::KirRead);
    // LIR に変換するスレッドを作れなければ、`keleut run` と同じく、スタックと入れ子の上限を下げて検証からやり直す。
    let lowered = crate::run_cmd::verify_then_lower(&prog, &limits, |n| {
        if o.unassigned {
            keleut_lir::lower_unassigned(&prog, n)
        } else {
            keleut_lir::lower_with(&prog, n)
        }
    });
    let lp = match lowered {
        Ok(p) => p,
        Err(crate::run_cmd::VerifyLowerError::Verify(e)) => {
            eprintln!("keleut: KIR の検証に失敗しました: {}", e.message());
            return e.exit_code() as u8;
        }
        Err(crate::run_cmd::VerifyLowerError::Lower(e)) => {
            eprintln!("keleut: {e}");
            return e.exit_code() as u8;
        }
    };
    drop(_site);
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

/// `keleut dump lir …` のエントリポイントである。引数の先頭は `lir` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return 0;
    }
    match parse(rest) {
        Ok(o) => {
            keleut_rt::alloc::set_input(keleut_rt::alloc::Input::Kir);
            run(&o)
        }
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
