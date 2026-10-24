//! サブコマンド `keleut kir` である(計画 261024-1 の 6.2 節、6.3 節、6.11 節)。
//!
//! | 形 | 働き |
//! |---|---|
//! | `keleut kir verify [OPTIONS] FILE` | 読んで検証する。通れば何も印字せずに 0 で終わる |
//! | `keleut kir dis [OPTIONS] [-o OUT] FILE` | 読んで検証し、正準のテキストを `OUT`(省けば標準出力)に書く |
//! | `keleut kir asm [OPTIONS] -o OUT FILE` | 読んで検証し、正準のバイナリを `OUT` に書く |
//! | `keleut kir tables` | 組み込みの表(IRSpec 付録 A)を `diktor --dump-ir-tables` と同じ形で印字する |
//!
//! `FILE` はバイナリ(`.kir`)でもテキスト(`.kirt`)でもよい。
//! 拡張子が `.kir` ならバイナリとして、`.kirt` ならテキストとして読む。
//! ほかの名前では、先頭の 4 バイトがマジックならバイナリとして読む。
//! 拡張子で決めるのは、マジックの壊れた `.kir` を、テキストの字句の誤りでなく
//! マジックの誤り(Diktor の `--run-ir` と同じ診断)として報告するためである。
//! `OPTIONS` は次のとおりである。
//!
//! - `--no-verify`: 検証を行わない(`dis` と `asm` だけ)。壊れた KIR を調べるときに使う。
//! - `--max-nest N`: 入れ子の限度(既定は IRSpec 13.9 節の `N_nest = 100,000`)。
//! - `--native-stack BYTES`: 読み込み、検証、書き出しを走らせる専用のスレッドのスタックの大きさ。
//!   `k`、`m`、`g` の接尾辞を使える。省けば `--max-nest` から見積もる。
//!   与えたスタックに `--max-nest` の深さが収まらないときは、収まる深さまで限度を下げる
//!   ([`Limits::with_stack`])。深い入力はスタックあふれで落ちずに、限度の診断で 65 になる。
//!   1 段も収まらない大きさは使い方の誤りである。
//!
//! 終了状態は計画 6.2 節の表に従う(読み込みと検証の失敗は 65、使い方の誤りは 64、内部の誤りは 70、
//! 標準出力に書けないときは 74)。
//! 表に無い場合は Diktor に合わせる。
//! 書き出しの入れ子の限度は 1(Diktor の `--emit-ir` と同じ)、入力のファイルを読めないときは 64、
//! 出力のファイルに書けないときは 74 である。
//! `dis` はテキストを出力先へ流しながら書くので、使うメモリはテキストの大きさでなく入れ子の深さに比例する。
//! 途中で書けなくなったとき(ディスクが尽きたときを含む)も 74 である。
//! 診断の文面は、Diktor の `--run-ir` の文面(`KIR を読み込めません: …`、`KIR の検証に失敗しました: …`)に合わせる。

use crate::exit;
use kir::{Error, ErrorKind, Limits};
use std::io::Write;
use std::path::PathBuf;

/// `keleut kir` の使い方である。
pub const USAGE: &str = "\
usage: keleut kir verify [OPTIONS] FILE
       keleut kir dis [OPTIONS] [-o OUT.kirt] FILE
       keleut kir asm [OPTIONS] -o OUT.kir FILE
       keleut kir tables

FILE is binary KIR (.kir) or text KIR (.kirt); for other names the magic decides.
options:
  -o FILE                output file (dis: default standard output)
  --no-verify            skip verification (dis, asm)
  --max-nest N           node depth limit (default 100000)
  --native-stack BYTES   stack size of the KIR thread (suffix k, m or g)
";

/// 道具の種類である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Verify,
    Dis,
    Asm,
}

/// 解釈したコマンド行である。
#[derive(Debug, PartialEq, Eq)]
struct Options {
    tool: Tool,
    input: PathBuf,
    output: Option<PathBuf>,
    verify: bool,
    limits: Limits,
}

/// 使い方の誤りを報告して 64 を返す。
fn usage_error(msg: &str) -> u8 {
    eprint!("keleut: {msg}\n{USAGE}");
    exit::USAGE
}

/// `--native-stack` の値を読む。
fn parse_bytes(s: &str) -> Option<usize> {
    let (num, mul) = match s.as_bytes().last()? {
        b'k' | b'K' => (&s[..s.len() - 1], 1usize << 10),
        b'm' | b'M' => (&s[..s.len() - 1], 1 << 20),
        b'g' | b'G' => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    let n: usize = num.parse().ok()?;
    let v = n.checked_mul(mul)?;
    // スレッドを作れる最小の大きさより小さい値は使い方の誤りにする。
    (v >= 64 << 10).then_some(v)
}

/// 引数の列(先頭は `verify` などの道具の名前)を解釈する。
fn parse(args: &[String]) -> Result<Options, String> {
    let tool = match args.first().map(String::as_str) {
        Some("verify") => Tool::Verify,
        Some("dis") => Tool::Dis,
        Some("asm") => Tool::Asm,
        Some(t) => return Err(format!("unknown kir tool: {t}")),
        None => return Err("missing kir tool".to_string()),
    };
    let mut input = None;
    let mut output = None;
    let mut verify = true;
    let mut max_nest = kir::N_NEST;
    let mut stack = None;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "-o" => output = Some(PathBuf::from(value("-o")?)),
            "--no-verify" if tool != Tool::Verify => verify = false,
            "--max-nest" => {
                let v = value("--max-nest")?;
                max_nest = match v.parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => return Err(format!("bad --max-nest value: {v}")),
                };
            }
            "--native-stack" => {
                let v = value("--native-stack")?;
                stack = Some(parse_bytes(&v).ok_or(format!("bad --native-stack value: {v}"))?);
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
    let Some(input) = input else {
        return Err("missing input file".to_string());
    };
    match tool {
        Tool::Verify if output.is_some() => return Err("verify takes no -o".to_string()),
        Tool::Asm if output.is_none() => return Err("asm needs -o OUT.kir".to_string()),
        _ => {}
    }
    let limits = match stack {
        None => Limits::for_nest(max_nest),
        Some(s) => {
            let l = Limits::with_stack(max_nest, s);
            if l.max_nest < 1 {
                return Err(format!("--native-stack is too small: {s}"));
            }
            l
        }
    };
    Ok(Options {
        tool,
        input,
        output,
        verify,
        limits,
    })
}

/// kir の誤りを、何をしていたか(`what`)とともに報告し、終了状態を返す。
fn report(what: &str, e: &Error) -> u8 {
    let what = match e.kind() {
        ErrorKind::Internal => "内部エラー",
        _ => what,
    };
    eprintln!("keleut: {what}: {}", e.message());
    // 終了状態はどれも 0〜255 に収まる。
    e.exit_code() as u8
}

/// 出力に書けないことを報告して 74 を返す。
fn output_error(what: &str, msg: &str) -> u8 {
    eprintln!("keleut: KIR の{what}を書き出せません: {msg}");
    exit::IO
}

/// 正準のテキストを出力先へ流しながら書く。
///
/// 出力のファイルは、失敗したときに消す(書きかけのテキストを残さない)。
fn dis_out(o: &Options, prog: &kir::Program) -> u8 {
    const WHAT: &str = "テキスト";
    let r = match &o.output {
        Some(p) => {
            let f = match std::fs::File::create(p) {
                Ok(f) => f,
                Err(e) => return output_error(WHAT, &format!("{}: {e}", p.display())),
            };
            let mut w = std::io::BufWriter::with_capacity(1 << 16, f);
            let r = kir::write_kirt_to(prog, &o.limits, &mut w).and_then(|()| {
                w.into_inner()
                    .map(drop)
                    .map_err(|e| Error::new(ErrorKind::Output, e.into_error().to_string()))
            });
            if r.is_err() {
                let _ = std::fs::remove_file(p);
            }
            r.map_err(|e| match e.kind() {
                ErrorKind::Output => Error::new(
                    ErrorKind::Output,
                    format!("{}: {}", p.display(), e.message()),
                ),
                _ => e,
            })
        }
        None => {
            let mut w = std::io::BufWriter::with_capacity(1 << 16, std::io::stdout());
            kir::write_kirt_to(prog, &o.limits, &mut w).and_then(|()| {
                w.flush()
                    .map_err(|e| Error::new(ErrorKind::Output, e.to_string()))
            })
        }
    };
    match r {
        Ok(()) => 0,
        Err(e) if e.kind() == ErrorKind::Output => output_error(WHAT, e.message()),
        Err(e) => report("KIR を書き出せません", &e),
    }
}

/// 書き出しの結果を出力先に書く。書けなければ 74 を返す。
fn write_out(output: Option<&PathBuf>, what: &str, data: &[u8]) -> u8 {
    let r = match output {
        Some(p) => std::fs::write(p, data).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            let mut o = std::io::stdout().lock();
            o.write_all(data)
                .and_then(|()| o.flush())
                .map_err(|e| e.to_string())
        }
    };
    match r {
        Ok(()) => 0,
        Err(msg) => output_error(what, &msg),
    }
}

/// 解釈したコマンド行を実行する。
fn run(o: &Options) -> u8 {
    let data = match std::fs::read(&o.input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("keleut: {}: {e}", o.input.display());
            return exit::USAGE;
        }
    };
    let read = match o.input.extension().and_then(|e| e.to_str()) {
        Some("kir") => kir::read_kir_with,
        Some("kirt") => kir::read_kirt_with,
        _ => kir::read_any_with,
    };
    let prog = match read(&data, &o.limits) {
        Ok(p) => p,
        Err(e) => return report("KIR を読み込めません", &e),
    };
    if o.verify
        && let Err(e) = kir::verify_with(&prog, &o.limits)
    {
        return report("KIR の検証に失敗しました", &e);
    }
    match o.tool {
        Tool::Verify => 0,
        Tool::Dis => dis_out(o, &prog),
        Tool::Asm => match kir::write_kir_with(&prog, &o.limits) {
            Ok(b) => write_out(o.output.as_ref(), "バイナリ", &b),
            Err(e) => report("KIR を書き出せません", &e),
        },
    }
}

/// 組み込みの表を `diktor --dump-ir-tables` と同じ形で作る。
///
/// インスタンスのメソッドは、クラスの順、型の順、メソッドの順に並べ、
/// プリミティブで書いていない実装の欄は `-` にする。
pub fn tables() -> String {
    use kir::builtins::{C_FUNCTIONS, CLASSES, METHODS, PRIMS, TYPES, VALUES};
    let mut s = String::new();
    for t in TYPES {
        s += &format!("type builtin/{t}\n");
    }
    for (c, ms) in CLASSES {
        s += &format!("class builtin/{c} {}\n", ms.join(" "));
    }
    for (c, ms) in CLASSES {
        for t in TYPES {
            for m in ms {
                let hit = METHODS
                    .iter()
                    .find(|(c2, t2, m2, _)| *c2 == c && *t2 == t && m2 == m);
                if let Some((_, _, _, p)) = hit {
                    s += &format!("method {c} {t} {m} {}\n", p.unwrap_or("-"));
                }
            }
        }
    }
    for v in VALUES {
        s += &format!("value builtin/{v}\n");
    }
    for p in PRIMS {
        s += &format!("prim {p}\n");
    }
    for f in C_FUNCTIONS {
        s += &format!("c {f}\n");
    }
    s
}

/// `keleut kir …` の入口である。引数の先頭は `kir` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    match rest.first().map(String::as_str) {
        Some("--help" | "-h" | "help") => {
            print!("{USAGE}");
            return 0;
        }
        Some("tables") => {
            if rest.len() > 1 {
                return usage_error("tables takes no arguments");
            }
            return write_out(None, "表", tables().as_bytes());
        }
        _ => {}
    }
    match parse(rest) {
        Ok(o) => run(&o),
        Err(msg) => usage_error(&msg),
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
        let o = parse(&argv(&["asm", "a.kirt", "-o", "a.kir", "--no-verify"])).unwrap();
        assert_eq!(o.tool, Tool::Asm);
        assert_eq!(o.input, PathBuf::from("a.kirt"));
        assert_eq!(o.output, Some(PathBuf::from("a.kir")));
        assert!(!o.verify);
        assert_eq!(o.limits, Limits::default());
        let o = parse(&argv(&[
            "verify",
            "--max-nest",
            "10",
            "--native-stack",
            "8m",
            "x",
        ]))
        .unwrap();
        assert_eq!(o.limits.max_nest, 10);
        assert_eq!(o.limits.stack_bytes, 8 << 20);
        // スタックに収まらない深さの限度は、収まる深さまで下げる。
        let o = parse(&argv(&["verify", "--native-stack", "1m", "x"])).unwrap();
        assert_eq!(o.limits, Limits::with_stack(kir::N_NEST, 1 << 20));
        assert!(o.limits.max_nest < kir::N_NEST);
    }

    #[test]
    fn 使い方の誤り() {
        for a in [
            &["asm", "a.kirt"][..],
            &["verify", "a", "-o", "b"],
            &["verify", "--no-verify", "a"],
            &["verify"],
            &["verify", "a", "b"],
            &["dis", "--max-nest", "0", "a"],
            &["dis", "--native-stack", "1k", "a"],
            &["dis", "--bogus", "a"],
            &["nosuch", "a"],
        ] {
            assert!(parse(&argv(a)).is_err(), "{a:?}");
        }
        assert_eq!(main(&argv(&["kir"])), exit::USAGE);
        assert_eq!(main(&argv(&["kir", "tables", "x"])), exit::USAGE);
    }

    #[test]
    fn 表の行の数() {
        let t = tables();
        let n = |p: &str| t.lines().filter(|l| l.starts_with(p)).count();
        assert_eq!(n("type "), 9);
        assert_eq!(n("class "), 9);
        assert_eq!(n("method "), 44);
        assert_eq!(n("value "), 12);
        assert_eq!(n("prim "), 48);
        assert_eq!(n("c "), 5);
        // 型の順に並べるので、Eq の Boolean は Int32 より前に来る。
        let eq: Vec<&str> = t.lines().filter(|l| l.starts_with("method Eq ")).collect();
        assert_eq!(eq[0], "method Eq Boolean eq -");
    }
}
