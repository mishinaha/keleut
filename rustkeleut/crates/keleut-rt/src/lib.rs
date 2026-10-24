//! RustKeleut の実行時(計画 261024-1 の 6.5 節、D525〜D530)。
//!
//! | モジュール | 中身 |
//! | --- | --- |
//! | [`value`] | 一様な語(即値とポインタ)と、ヒープの箱の配置 |
//! | [`heap`] | 確保(M87 は回収をしない)と、Rust の実行時の関数の根の枠 `Root` |
//! | [`stack`] | 分割値スタックとフレームの形 |
//! | [`out`] | 標準出力の規約(64 KiB のバッファ、flush の時点、短い書き込み、`EAGAIN`、最初の失敗の扱い) |
//! | [`error`] | 評価が止まる理由、文面、終了状態 |
//! | [`ctx`] | 実行時の文脈と、値を作り読む操作、診断の印字 |
//!
//! 後段(M87 のインタプリタ、M91 の JIT)は、LIR の命令の意味のうち実行時に任せるものを [`ctx::Rt`] の関数で行う。
//! エフェクト(状態語、ハンドラ記録、継続)は M89 で、GC は M90 で足す。

pub mod ctx;
pub mod error;
pub mod heap;
pub mod out;
pub mod stack;
pub mod value;

pub use ctx::{CalleeView, DEFAULT_MAX_DEPTH, DictView, Rt};
pub use error::Stop;
pub use value::Word;

/// 予備の大きさ `R = max(1024, L / 64)`(IRSpec 11.5 節)。
pub fn reserve(limit: u64) -> u64 {
    (limit / 64).max(1024)
}

/// ネイティブの 1 段あたりに見込むバイト数 `B_native` の初期値(計画 6.11 節。M91 で測って決める)。
pub const B_NATIVE: u64 = 1024;

/// 専用のスレッドに予約するネイティブのスタックの大きさ `S_native = (L + R) × B_native + 64 MiB`(計画 6.11 節)。
pub fn native_stack_for(limit: u64) -> u64 {
    (limit.saturating_add(reserve(limit)))
        .saturating_mul(B_NATIVE)
        .saturating_add(64 << 20)
}
