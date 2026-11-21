//! RustKeleut のランタイム。
//!
//! | モジュール | 内容 |
//! | --- | --- |
//! | [`alloc`] | グローバルアロケータのラッパ、予備領域、スレッドの役割ごとのフラグ、確保の失敗を呼び出し元が処理する範囲、テスト用の確保失敗 |
//! | [`value`] | 一様表現の値(即値とポインタ)と、ヒープのオブジェクトのレイアウト |
//! | [`heap`] | サイズクラスごとのページからの確保とスイープ、Rust で書いたランタイム関数のルートスタック [`heap::Roots`] |
//! | [`gc`] | オブジェクトを移動しないマークスイープの GC(ルートの走査、反復によるマーク、表の項目の解放) |
//! | [`stack`] | 分割値スタックとフレームのレイアウト |
//! | [`out`] | 標準出力の規約(64 KiB のバッファ、flush のタイミング、短い書き込み、`EAGAIN`、最初の出力エラーの扱い) |
//! | [`error`] | 評価が終了する理由、メッセージ、終了コード |
//! | [`ctx`] | ランタイムコンテキストと、値を作る操作と読む操作、診断の表示 |
//! | [`effect`] | エフェクトのデータ(状態ワード、ハンドラ記録、継続) |
//! | [`extend`] | 単位を追加したプログラムへのコンテキストの差し替え(`keleut repl`) |
//!
//! バックエンド(インタプリタと LLVM の JIT)は、LIR の命令の動作のうちランタイムに任せる部分を [`ctx::Rt`] の関数で実行する。
//! バックエンドが GC のために守る規則は、[`gc`] のモジュールの説明と `rustkeleut/doc/lir.md`(以下 `doc/lir.md`)の 1.8 節にある。

pub mod alloc;
pub mod ctx;
pub mod effect;
pub mod error;
pub mod extend;
pub mod gc;
pub mod heap;
pub mod out;
pub mod stack;
pub mod value;

pub use ctx::{CalleeView, DEFAULT_MAX_DEPTH, DictView, Rt};
pub use error::Stop;
pub use value::Word;

/// 深さの上限 `L` が `limit` のときの、予備の深さ `R = max(1024, L / 64)` を返す(IRSpec 11.5 節)。
pub fn reserve(limit: u64) -> u64 {
    (limit / 64).max(1024)
}

/// 論理的な深さ 1 段あたりに見積もる、ネイティブスタックのバイト数 `B_native`。
pub const B_NATIVE: u64 = 1024;

/// 深さの上限 `L` が `limit` のときに、実行スレッドに予約するネイティブスタックのサイズ `S_native = (L + R) × B_native + 64 MiB` を返す。
///
/// `keleut run` は、`--native-stack` を指定しないときの既定のサイズとして使う。
/// `keleut repl` は、`--native-stack` の指定によらず、実行スレッドのスタックのサイズとして使う。
/// 仮想メモリの上限があるときは、どちらのコマンドも、実行スレッドを作るときにこのサイズを上限の 4 分の 1 までに抑える。
/// ただし、上限の 4 分の 1 が 16 MiB より小さければ、16 MiB までに抑える。
pub fn native_stack_for(limit: u64) -> u64 {
    (limit.saturating_add(reserve(limit)))
        .saturating_mul(B_NATIVE)
        .saturating_add(64 << 20)
}
