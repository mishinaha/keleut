//! RustKeleut のコマンド行の入口 `keleut` である。
//!
//! サブコマンドの一覧と終了状態は、計画 261024-1 の 6.2 節に従う。
//! 本ファイルは振り分けだけを持ち、各サブコマンドの中身は後のマイルストーンで足す。
//! まだ中身の無いサブコマンドは、「未実装」として終了状態 4 で終わる。

use std::process::ExitCode;

mod check_cmd;
mod dump_cmd;
mod front_cmd;
mod kir_cmd;
mod lir_cmd;

/// 終了状態の定数である(計画 6.2 節の表)。
mod exit {
    /// 未実装の機能を使った。
    pub const UNIMPLEMENTED: u8 = 4;
    /// 使い方の誤り。
    pub const USAGE: u8 = 64;
    /// 出力に書けない。
    pub const IO: u8 = 74;
}

/// 使い方の説明である。
/// 使い方の誤りのときに、標準エラーへ印字する。
const USAGE: &str = "\
usage: keleut COMMAND [ARGS...]

commands:
  run [OPTIONS] FILE.kel... | FILE.kir   run a program
  check FILE.kel...                      type-check (same output as diktor --type-check)
  emit-kir [--text] -o OUT FILE.kel...   write KIR
  kir verify|dis|asm|tables FILE         KIR tools
  build --target wasm -o DIR FILE        build a WasmGC program
  dump tokens|ast|lir|bytecode|llvm FILE debugging output
  repl                                   interactive session
  compat-diktor ARGS...                  interpret Diktor's flags
";

/// サブコマンドを受け持つ関数である。
/// 引数の列(先頭はサブコマンドの名前)を受け取り、終了状態を返す。
type Handler = fn(&[String]) -> u8;

/// サブコマンドの名前と、それを受け持つ関数の表である。
///
/// 表に無い名前は使い方の誤り(64)にする。
const COMMANDS: &[(&str, Handler)] = &[
    ("run", unimplemented_command),
    ("check", check_cmd::main),
    ("emit-kir", unimplemented_command),
    ("kir", kir_cmd::main),
    ("build", unimplemented_command),
    ("dump", dump_cmd::main),
    ("repl", unimplemented_command),
    ("compat-diktor", unimplemented_command),
];

/// まだ中身の無いサブコマンドである。
///
/// 引数の先頭はサブコマンドの名前である。
fn unimplemented_command(args: &[String]) -> u8 {
    eprintln!("keleut: 未実装: サブコマンド {}", args[0]);
    exit::UNIMPLEMENTED
}

/// 引数の列(プログラム名を除く)を振り分け、終了状態を返す。
fn dispatch(args: &[String]) -> u8 {
    let Some(name) = args.first() else {
        eprint!("keleut: missing command\n{USAGE}");
        return exit::USAGE;
    };
    if name == "--help" || name == "-h" || name == "help" {
        print!("{USAGE}");
        return 0;
    }
    if name == "--version" {
        println!("keleut {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    match COMMANDS.iter().find(|(n, _)| n == name) {
        Some((_, f)) => f(args),
        None => {
            eprint!("keleut: unknown command: {name}\n{USAGE}");
            exit::USAGE
        }
    }
}

fn main() -> ExitCode {
    // `std::env::args` は UTF-8 でない引数で落ちるので、`args_os` で受けて使い方の誤りにする。
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(a) => {
                eprintln!(
                    "keleut: argument is not valid UTF-8: {}",
                    a.to_string_lossy()
                );
                return ExitCode::from(exit::USAGE);
            }
        }
    }
    ExitCode::from(dispatch(&args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn 名前の無い呼び出しと知らない名前は使い方の誤りになる() {
        assert_eq!(dispatch(&argv(&[])), exit::USAGE);
        assert_eq!(dispatch(&argv(&["nosuch"])), exit::USAGE);
    }

    #[test]
    fn 中身の無いサブコマンドは未実装になる() {
        for (name, _) in COMMANDS {
            if *name != "kir" && *name != "dump" && *name != "check" {
                assert_eq!(dispatch(&argv(&[name])), exit::UNIMPLEMENTED);
            }
        }
    }
}
