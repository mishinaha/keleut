//! RustKeleut のコマンド `keleut` のエントリポイントである。
//!
//! 本ファイルはサブコマンドへの振り分けだけを行い、各サブコマンドの実装はそれぞれのモジュールにある。
//! サブコマンドの一覧は `COMMANDS` に、複数のサブコマンドが使う終了コードはモジュール `exit` にある。

use std::process::ExitCode;

/// グローバルアロケータである。
/// システムのアロケータのラッパで、確保に失敗すると予備領域を解放してやり直し、スレッドの役割のフラグを立てる(`keleut_rt::alloc`)。
#[global_allocator]
static ALLOC: keleut_rt::alloc::Wrap = keleut_rt::alloc::Wrap;

mod build_cmd;
mod check_cmd;
mod compat_cmd;
mod dump_cmd;
mod emit_kir_cmd;
mod front_cmd;
mod kir_cmd;
mod lir_cmd;
mod repl_cmd;
mod run_cmd;

/// 複数のサブコマンドが使う終了コードの定数である。
mod exit {
    /// 未実装の機能を使った(feature `llvm` を外してビルドしたときのバックエンド `jit` など)。
    #[cfg_attr(feature = "llvm", allow(dead_code))]
    pub const UNIMPLEMENTED: u8 = 4;
    /// 使い方のエラー。
    pub const USAGE: u8 = 64;
    /// 出力エラー(出力先に書けない)。
    pub const IO: u8 = 74;
}

/// `keleut` の使い方の説明(usage)である。
/// 使い方のエラーのときは標準エラーに、`--help` のときは標準出力に書く。
const USAGE: &str = "\
usage: keleut COMMAND [ARGS...]

commands:
  run [OPTIONS] FILE.kel... | FILE.kir   run a program
  check FILE.kel...                      type-check (same output as diktor --type-check)
  emit-kir [--text] [-o OUT] FILE.kel... write KIR
  kir verify|dis|asm|tables FILE         KIR tools
  build --target wasm -o DIR FILE        build a WasmGC program
  dump tokens|ast|lir|bytecode|llvm FILE debugging output
  repl                                   interactive session
  compat-diktor ARGS...                  interpret Diktor's flags
";

/// このビルドで使えるバックエンドの名前である。`keleut --version` の 2 行目に出力する。
///
/// `jit` は、feature `llvm`(既定で有効)を外さずにビルドしたときだけ入る。
/// kconform はこの行を読み、LLVM ペアを実行できるかどうかを決める。
const BACKENDS: &str = if cfg!(feature = "llvm") {
    "interp jit wasm"
} else {
    "interp wasm"
};

/// サブコマンドを実装する関数の型である。
/// 引数の並び(先頭はサブコマンドの名前)を受け取り、終了コードを返す。
type Handler = fn(&[String]) -> u8;

/// サブコマンドの名前と、それを実装する関数の表である。
///
/// 表にない名前は使い方のエラー(終了コード 64)になる。
const COMMANDS: &[(&str, Handler)] = &[
    ("run", run_cmd::main),
    ("check", check_cmd::main),
    ("emit-kir", emit_kir_cmd::main),
    ("kir", kir_cmd::main),
    ("build", build_cmd::main),
    ("dump", dump_cmd::main),
    ("repl", repl_cmd::main),
    ("compat-diktor", compat_cmd::main),
];

/// 引数の並び(プログラム名を除く)をサブコマンドに振り分け、終了コードを返す。
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
        println!("backends: {BACKENDS}");
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
    // 予備領域を確保し、メインスレッドの役割を `Role::Other` に設定し、テスト用の確保失敗の指定(`KELEUT_TEST_ALLOC_FAIL`)を読む。
    // 指定を読めなければ、使い方のエラーとして終了する。
    if let Err(msg) = keleut_rt::alloc::init() {
        eprintln!("keleut: {msg}");
        return ExitCode::from(exit::USAGE);
    }
    // `std::env::args` は UTF-8 でない引数があるとパニックするので、`args_os` で受け取る。
    // UTF-8 でない引数があれば、使い方のエラーとして終了する。
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
    fn 後段の一覧は_feature_llvm_に従う() {
        assert_eq!(
            BACKENDS.split(' ').any(|b| b == "jit"),
            cfg!(feature = "llvm")
        );
        assert!(BACKENDS.split(' ').any(|b| b == "interp"));
        assert!(BACKENDS.split(' ').any(|b| b == "wasm"));
    }
}
