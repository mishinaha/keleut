//! サブコマンド `keleut dump` を、下位のサブコマンドに振り分ける。
//!
//! `keleut dump tokens|ast|lir|bytecode|llvm FILE` の `dump` の次の引数で、実行する関数を選ぶ。
//! `tokens`、`ast`、`lir`、`bytecode`、`llvm` 以外の名前は、使い方のエラー(終了コード 64)になる。
//! feature `llvm`(既定で有効)を `--no-default-features` で外してビルドした `keleut` では、`llvm` は未実装(終了コード 4)になる。

use crate::exit;

/// `keleut dump` の使い方の説明(usage)である。
pub const USAGE: &str = "\
usage: keleut dump tokens|ast|lir|bytecode|llvm [OPTIONS] FILE
";

/// `dump` のサブコマンドを実装する関数の型である。引数の並び(先頭はサブコマンドの名前)を受け取り、終了コードを返す。
type Handler = fn(&[String]) -> u8;

/// `dump` のサブコマンドの名前と、それを実装する関数の表である。
const TOOLS: &[(&str, Handler)] = &[
    ("tokens", crate::front_cmd::tokens),
    ("ast", crate::front_cmd::ast),
    ("lir", crate::lir_cmd::main),
    ("bytecode", crate::run_cmd::dump_bytecode),
    ("llvm", crate::run_cmd::dump_llvm),
];

/// `keleut dump …` のエントリポイントである。引数の先頭は `dump` である。
pub fn main(args: &[String]) -> u8 {
    let Some(name) = args.get(1) else {
        eprint!("keleut: missing dump tool\n{USAGE}");
        return exit::USAGE;
    };
    match TOOLS.iter().find(|(n, _)| n == name) {
        Some((_, f)) => f(&args[1..]),
        None => {
            eprint!("keleut: unknown dump tool: {name}\n{USAGE}");
            exit::USAGE
        }
    }
}
