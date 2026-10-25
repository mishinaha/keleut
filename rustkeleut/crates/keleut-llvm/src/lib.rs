//! LLVM の後段(計画 261024-1 の 6.8 節、M91)。
//!
//! feature `llvm` を有効にしたときだけ中身を持つ。
//! 無効なら空の crate で、`cargo build` は LLVM 抜きで組める(計画 6.1 節)。
//!
//! | モジュール | 中身 |
//! | --- | --- |
//! | `sys` | llvm-sys(`=221.1.1`、LLVM 22 の共有ライブラリ)の薄い包み。LLJIT、絶対の記号、IR の文面のモジュール |
//! | `codegen` | LIR の関数から LLVM IR の文面を作る |
//!
//! [`run_program`] は、インタプリタ(`keleut-interp`)にネイティブの後段を付けて実行する。
//! 関数と JIT の規約は `keleut_interp::native` の冒頭にある。
//!
//! - LLVM の翻訳は専用のスレッド(`keleut-jit`)で行う。
//!   ミューテータのスレッドは、関数を初めて呼ぶときに翻訳を頼み、番地が返るまで待つ。
//!   LLVM の翻訳がミューテータのネイティブのスタックを使わないので、小さな `--native-stack` でも、深い再帰の途中でも翻訳できる。
//! - 起動時に、実行時の関数を絶対の記号として登録し、スタブ `interp_stub` と `enter_tailcc` のモジュールを翻訳する。
//!   入口の表のどの項目も、はじめは `interp_stub` を指す。
//! - 関数は 1 つずつ別のモジュールにし、`default<O2>` をかけて翻訳する(計画 6.8 節)。
//!   関数どうしの呼び出しは入口の表を通るので、モジュールの間に記号の参照は無い。
//! - 段の設定は `jit-only`(JIT にできる関数をすべて JIT で走らせる)と `interp-only` である。
//!   `jit-only` では、各関数を最初に呼ぶときに翻訳する(実行するどの関数も JIT のコードで走る)。

#[cfg(feature = "llvm")]
pub mod codegen;
#[cfg(feature = "llvm")]
pub mod sys;

#[cfg(feature = "llvm")]
mod run;

#[cfg(feature = "llvm")]
pub use run::{RunOptions, Tier, dump_ir, run_program, run_program_here};
