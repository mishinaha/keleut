//! サブコマンド `keleut dump` の振り分けである(計画 261024-1 の 6.2 節)。
//!
//! `keleut dump tokens|ast|lir|bytecode|llvm FILE` の 2 語目で、受け持つ関数を選ぶ。
//! 表に無い名前は使い方の誤り(64)である。
//! `llvm` は feature `llvm` を付けずに組むと未実装(4)である。

use crate::exit;

/// `keleut dump` の使い方である。
pub const USAGE: &str = "\
usage: keleut dump tokens|ast|lir|bytecode|llvm [OPTIONS] FILE
";

/// 道具を受け持つ関数である。引数の列(先頭は道具の名前)を受け取り、終了状態を返す。
type Handler = fn(&[String]) -> u8;

/// 道具の名前と、それを受け持つ関数の表である。
const TOOLS: &[(&str, Handler)] = &[
    ("tokens", crate::front_cmd::tokens),
    ("ast", crate::front_cmd::ast),
    ("lir", crate::lir_cmd::main),
    ("bytecode", crate::run_cmd::dump_bytecode),
    ("llvm", crate::run_cmd::dump_llvm),
];

/// `keleut dump …` の入口である。引数の先頭は `dump` である。
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
