//! LLVM の後段(計画 261024-1 の 6.8 節、M91)。
//!
//! feature `llvm` を有効にしたときだけ中身を持つ。
//! 無効なら空の crate で、`cargo build` は LLVM 抜きで組める(計画 6.1 節)。
//!
//! | モジュール | 中身 |
//! | --- | --- |
//! | `sys` | llvm-sys(`=221.1.1`、LLVM 22 の共有ライブラリ)の薄い包み。LLJIT、絶対の記号、IR の文面のモジュール |

#[cfg(feature = "llvm")]
pub mod sys;
