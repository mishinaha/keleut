//! LLVM バックエンド。
//!
//! feature `llvm` が有効なときだけ内容を持つ。
//! この feature は既定で有効である。
//! `--no-default-features` で外すと空の crate になり、LLVM なしでビルドできる。
//!
//! | モジュール | 内容 |
//! | --- | --- |
//! | `sys` | llvm-sys(`=221.1.1`、LLVM 22 の共有ライブラリ)の薄いラッパ。LLJIT を作り、絶対シンボルを登録し、LLVM IR のテキストで書いたモジュールを追加する |
//! | `codegen` | LIR の関数から LLVM IR のテキストを作る |
//!
//! [`run_program`] は、インタプリタ(`keleut-interp`)にネイティブコードのバックエンドを取り付けて実行する。
//! JIT の関数のエントリ、呼び出し規約、インタプリタとの境界の扱いは、`keleut_interp::native` の冒頭で説明している。
//! LIR の各命令の意味は `rustkeleut/doc/lir.md`(以下 `doc/lir.md`)が定める。
//!
//! - LLVM によるコンパイルは専用のスレッド(`keleut-jit`。以下「コンパイルスレッド」)で行う。
//!   コンパイルが済むと、コンパイルスレッドがエントリテーブルの項目をアトミックに書き換える。
//!   `jit-only`、`alternating`、`mixed` では、ミューテータは関数を初めて呼ぶときにコンパイルを依頼し、完了を待つ。
//!   `threshold=N` では、インタプリタで `N` 回呼んだ関数のコンパイルを依頼し、完了を待たずにインタプリタで実行を続ける。
//!   LLVM はミューテータのネイティブスタックを使わないので、`--native-stack` が小さくても、深い再帰の途中でもコンパイルできる。
//! - コンパイルスレッドは起動時に、ランタイム関数を絶対シンボルとして登録し、スタブ `interp_stub`、`interp_stub_r`、`enter_tailcc` のモジュールをコンパイルする。
//!   エントリテーブル(値スタック渡しのエントリの表)の項目はどれも、はじめは `interp_stub` を指す。
//!   レジスタ渡しのエントリテーブルの項目はどれも、はじめは `interp_stub_r` を指す。
//!   `threshold=N` では、ミューテータはこの起動処理を待たずにインタプリタで実行を始め、エントリテーブルの項目は起動処理が済むまで 0 である。
//!   ミューテータは、依頼した関数の項目が 0 でなくなったときに、起動処理の結果(スタブのアドレス)を受け取る。
//!   `keleut run` は、このティアの設定では、プログラムの実行が終わるとコンパイルスレッドを待たずにプロセスを終了する。
//! - 関数は 1 つずつ別のモジュールにし、`default<O2>` で最適化してコンパイルする。
//!   関数どうしの呼び出しはエントリテーブルを通るので、モジュールの間にシンボルの参照はない。
//! - `handle`、`resume`、`perform` を LIR に変換した命令を含む関数もコンパイルする(規約は `keleut_interp::native` の冒頭)。
//! - ティアの設定([`Tier`])と、設定ごとのティアの選び方は、`keleut_interp::native` の冒頭で説明している。
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
