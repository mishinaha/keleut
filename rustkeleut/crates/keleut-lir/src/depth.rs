//! 深さの命令(パス 10。IRSpec 11.2 節、11.3 節)。
//!
//! 論理的な深さは、実行時の文脈の深さ `ctx.depth` と、フレームごとの自分の深さで数える。
//!
//! - 関数の入口の `depth_enter` が、`ctx.depth` を自分のフレームの深さとして書く。
//! - 末尾でない呼び出しの前の `depth_push` が、呼ばれた側の深さ `ctx.depth + 1` を限度と比べ、
//!   超えれば `depth_limit` にし、超えなければ `ctx.depth` を 1 増やす。
//! - 呼び出しの後の `depth_restore` が、`ctx.depth` を自分のフレームの深さに書き戻す。
//! - プリミティブ、組み込みのメソッド、組み込みの値のように、利用者のコードを呼ばずに終わる
//!   末尾でない呼び出しは、`depth_check` で `ctx.depth + 1` を限度と比べるだけにする。
//! - 末尾の呼び出しは深さを変えない。
//!
//! 呼ばれた側が正常に戻ったとき、`ctx.depth` は呼ばれた側の入口の深さに等しい。
//! 各フレームは、末尾でない呼び出しの後で必ず書き戻すからである。
//! メソッドの値の呼び出しで、実装の関数の評価と得た関数の呼び出しを同じ深さで行う箇所
//! ([`crate::dict`])は、この性質を使う。

use crate::builder::Builder;
use crate::ir::Inst;

/// 関数の入口の命令。
pub(crate) fn enter(b: &mut Builder) {
    b.emit(Inst::DepthEnter);
}

/// 呼ばれた側の深さを限度と比べる。
pub(crate) fn check(b: &mut Builder) {
    b.emit(Inst::DepthCheck);
}

/// 呼ばれた側の深さに入る。
pub(crate) fn push(b: &mut Builder) {
    b.emit(Inst::DepthPush);
}

/// 自分の深さに戻る。
pub(crate) fn restore(b: &mut Builder) {
    b.emit(Inst::DepthRestore);
}

/// 末尾でない呼び出しを `depth_push` と `depth_restore` で囲む。
pub(crate) fn around_call(b: &mut Builder, call: impl FnOnce(&mut Builder)) {
    push(b);
    call(b);
    restore(b);
}
