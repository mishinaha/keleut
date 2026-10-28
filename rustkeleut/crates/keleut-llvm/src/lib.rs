//! LLVM の後段(計画 261024-1 の 6.8 節、M91)。
//!
//! feature `llvm`(既定で有効。計画 6.1 節、P241)を有効にしたときだけ中身を持つ。
//! `--no-default-features` で外すと空の crate になり、LLVM 抜きで組める。
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
//!   翻訳を終えたら、翻訳のスレッドが入口の表の項目を原子的に書き換える(計画 6.9 節)。
//!   `jit-only`、`alternating`、`mixed` では、ミューテータは関数を初めて呼ぶときに翻訳を頼み、済むまで待つ。
//!   `threshold=N` では、インタプリタで `N` 回呼んだ関数の翻訳を頼み、待たずにインタプリタで続ける。
//!   LLVM の翻訳がミューテータのネイティブのスタックを使わないので、小さな `--native-stack` でも、深い再帰の途中でも翻訳できる。
//! - 起動時に、実行時の関数を絶対の記号として登録し、スタブ `interp_stub`、`interp_stub_r`、`enter_tailcc` のモジュールを翻訳する。
//!   入口の表のどの項目も、はじめは `interp_stub` を指し、レジスタの入口の表のどの項目も、はじめは `interp_stub_r` を指す。
//!   `threshold=N` では、ミューテータはこの起動を待たずにインタプリタで走り始め、入口の表の項目は起動が済むまで 0 である。
//!   頼んだ関数の項目が 0 でなくなったときに、起動の結果(スタブの番地)を受け取る。
//!   `keleut run` は、この段の設定では実行の後に翻訳のスレッドを待たずにプロセスを終える。
//! - 関数は 1 つずつ別のモジュールにし、`default<O2>` をかけて翻訳する(計画 6.8 節)。
//!   関数どうしの呼び出しは入口の表を通るので、モジュールの間に記号の参照は無い。
//! - `handle`、`resume`、`perform` の下ろしを含む関数も翻訳する(M92。規約は `keleut_interp::native` の冒頭)。
//! - 段の設定([`Tier`])と段の選び方は `keleut_interp::native` の冒頭の「段の切り替え」にある。
//!   既定は [`DEFAULT_TIER`](`threshold=1000`)である。
#[cfg(feature = "llvm")]
pub mod codegen;
#[cfg(feature = "llvm")]
pub mod sys;

#[cfg(feature = "llvm")]
mod run;

#[cfg(feature = "llvm")]
pub use run::{
    DEFAULT_TIER, RunOptions, Tier, dump_ir, run_program, run_program_here, run_program_wait,
};
