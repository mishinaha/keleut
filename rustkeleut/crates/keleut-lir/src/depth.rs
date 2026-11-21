//! 深さの命令(パス 10。IRSpec 11.2 節、11.3 節)。
//!
//! 論理的な深さは、ランタイムが持つコンテキストの深さ `ctx.depth` と、各フレームが持つ自分の深さで数える。
//!
//! - 関数の入口の `depth_enter` が、`ctx.depth` を自分のフレームの深さとして記録する。
//! - 非末尾呼び出しの前の `depth_push` が、呼び出し先の深さ `ctx.depth + 1` を上限と比較する。
//!   超えていれば `depth_limit` の実行時エラーにし、超えていなければ `ctx.depth` を 1 増やす。
//! - 呼び出しの後の `depth_restore` が、`ctx.depth` を自分のフレームの深さに書き戻す。
//! - プリミティブ、組み込みのメソッド、組み込みの値のように、ユーザのコードを呼ばずに終わる非末尾呼び出しでは、
//!   `depth_check` で `ctx.depth + 1` を上限と比較するだけで、`ctx.depth` は変えない。
//! - 末尾呼び出しは深さを変えない。
//!
//! 呼び出し先が正常に戻ったとき、`ctx.depth` は呼び出し先の入口での深さに等しい。
//! 非末尾呼び出しは、`depth_push` と `depth_restore` で囲むか、`ctx.depth` を変えずに呼び出し元の現在の深さで呼び出し先を実行するかの、どちらかだからである。
//! メソッドの値の呼び出し([`crate::dict`])は、この性質を使って、実装の関数の評価と、評価で得た関数の呼び出しを同じ深さで行う。

use crate::builder::Builder;
use crate::ir::Inst;

/// 関数の入口に `depth_enter` を置く。
pub(crate) fn enter(b: &mut Builder) {
    b.emit(Inst::DepthEnter);
}

/// `depth_check` を置く。
/// 呼び出し先の深さを上限と比較するだけで、深さは変えない。
pub(crate) fn check(b: &mut Builder) {
    b.emit(Inst::DepthCheck);
}

/// `depth_push` を置く。
/// 呼び出し先の深さを上限と比較し、`ctx.depth` を呼び出し先の深さにする。
pub(crate) fn push(b: &mut Builder) {
    b.emit(Inst::DepthPush);
}

/// `depth_restore` を置く。
/// `ctx.depth` を自分のフレームの深さに書き戻す。
pub(crate) fn restore(b: &mut Builder) {
    b.emit(Inst::DepthRestore);
}

/// `call` が置く非末尾呼び出しを、`depth_push` と `depth_restore` で囲む。
pub(crate) fn around_call(b: &mut Builder, call: impl FnOnce(&mut Builder)) {
    push(b);
    call(b);
    restore(b);
}
