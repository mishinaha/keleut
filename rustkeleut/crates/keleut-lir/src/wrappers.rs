//! 組み込みの値、`extern`、組み込みのインスタンスのメソッドの本体。
//!
//! これらの値は、どれも LIR の関数のクロージャである。
//! 本体は LIR の命令だけで書いてあるので、値として渡されて `call_value` で呼ばれる場合も、バックエンドは LIR の命令の意味だけを実装すればよく、組み込みの値ごとの動作を知らなくてよい。
//!
//! KIR の `call` が組み込みの値のグローバルを直接呼ぶ箇所は、次のように変換する。
//! [`inline_arity`] が挙げる値を、引数をすべて渡して呼ぶ箇所では、[`builtin_inline`] が命令を呼び出しの位置に展開する。
//! `Array.each` と `pinned` を、引数をすべて渡して呼ぶ箇所では、本体の関数を直接呼び出しで呼ぶ。
//! 引数の個数が合わない箇所では、グローバルの値を読んで `call_value` で呼ぶ。
//!
//! `Array.each` と `pinned` は、ユーザの関数を呼ぶ。
//! この呼び出しは非末尾呼び出しとして数えるので、ユーザの関数は組み込みの値の深さに 1 を足した深さで実行される(IRSpec 付録 A.4)。
//! `Array.each` の繰り返しは、内側のループの関数が自分自身を末尾呼び出しで呼ぶことで行う。

use crate::builder::Builder;
use crate::ir::*;
use crate::numeric::{self, Prim};

/// 呼び出しの位置に命令を展開できる組み込みの値 `name` の、引数の個数を返す。
///
/// それ以外の名前には `None` を返す。
pub(crate) fn inline_arity(name: &str) -> Option<usize> {
    Some(match name {
        "Ref.new" | "Ref.get" | "Array.length" | "MutableArray.length" | "MutableArray.freeze" => 1,
        "Ref.set" | "Array.get" | "MutableArray.new" | "MutableArray.get" => 2,
        "MutableArray.set" => 3,
        _ => return None,
    })
}

/// 組み込みの値 `name` の引数の個数を返す。
///
/// 組み込みの値の名前でなければ `None` を返す。
pub(crate) fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "Array.each" => Some(2),
        "pinned" => Some(1),
        _ => inline_arity(name),
    }
}

/// 一様表現の値 `index` を `unbox` で `i32` にして返す。
///
/// `0 ≦ index < len` でなければ実行時エラー 15(`配列の範囲外です`)になるように、比較と `error` への分岐を生成する。
fn checked_index(b: &mut Builder, index: Reg, len: Reg) -> Reg {
    let i = b.reg(Rep::I32);
    b.emit(Inst::Unbox { dst: i, src: index });
    let zero = b.konst(Const::I32(0));
    let neg = b.reg(Rep::Bool);
    b.emit(Inst::IntCmp {
        ty: IntTy::I32,
        op: CmpOp::Lt,
        dst: neg,
        a: i,
        b: zero,
    });
    b.fail_if(neg, RtError::ArrayIndex, None);
    let over = b.reg(Rep::Bool);
    b.emit(Inst::IntCmp {
        ty: IntTy::I32,
        op: CmpOp::Ge,
        dst: over,
        a: i,
        b: len,
    });
    b.fail_if(over, RtError::ArrayIndex, None);
    i
}

/// 組み込みの値 `name` の命令を、現在の位置に生成する。
///
/// 引数も結果も一様表現の値である。
/// 結果のレジスタを返す。
/// [`inline_arity`] が挙げない名前には `None` を返す。
pub(crate) fn builtin_inline(b: &mut Builder, name: &str, args: &[Reg]) -> Option<Reg> {
    let dst = b.reg(Rep::Any);
    match name {
        "Ref.new" => b.emit(Inst::RefNew {
            dst,
            value: args[0],
        }),
        "Ref.get" => b.emit(Inst::RefGet { dst, r: args[0] }),
        "Ref.set" => {
            b.emit(Inst::RefSet {
                r: args[0],
                value: args[1],
            });
            b.emit(Inst::Const {
                dst,
                value: Const::Unit,
            });
        }
        "Array.length" | "MutableArray.length" => {
            let n = b.reg(Rep::I32);
            b.emit(if name == "Array.length" {
                Inst::ArrayLen { dst: n, a: args[0] }
            } else {
                Inst::MArrayLen { dst: n, a: args[0] }
            });
            b.emit(Inst::Box { dst, src: n });
        }
        "Array.get" | "MutableArray.get" => {
            let n = b.reg(Rep::I32);
            let arr = name == "Array.get";
            b.emit(if arr {
                Inst::ArrayLen { dst: n, a: args[0] }
            } else {
                Inst::MArrayLen { dst: n, a: args[0] }
            });
            let i = checked_index(b, args[1], n);
            b.emit(if arr {
                Inst::ArrayGet {
                    dst,
                    a: args[0],
                    index: i,
                }
            } else {
                Inst::MArrayGet {
                    dst,
                    a: args[0],
                    index: i,
                }
            });
        }
        "MutableArray.set" => {
            let n = b.reg(Rep::I32);
            b.emit(Inst::MArrayLen { dst: n, a: args[0] });
            let i = checked_index(b, args[1], n);
            b.emit(Inst::MArraySet {
                a: args[0],
                index: i,
                value: args[2],
            });
            b.emit(Inst::Const {
                dst,
                value: Const::Unit,
            });
        }
        "MutableArray.new" => {
            let n = b.reg(Rep::I32);
            b.emit(Inst::Unbox {
                dst: n,
                src: args[0],
            });
            let zero = b.konst(Const::I32(0));
            let neg = b.reg(Rep::Bool);
            b.emit(Inst::IntCmp {
                ty: IntTy::I32,
                op: CmpOp::Lt,
                dst: neg,
                a: n,
                b: zero,
            });
            b.fail_if(neg, RtError::MArrayNegLen, None);
            b.emit(Inst::MArrayNew {
                dst,
                len: n,
                init: args[1],
            });
        }
        "MutableArray.freeze" => b.emit(Inst::MArrayFreeze { dst, a: args[0] }),
        _ => return None,
    }
    Some(dst)
}

/// 引数をすべて受け取ってプリミティブ `p` を実行する関数の本体を組み立てる。
pub(crate) fn prim_body(p: Prim) -> Builder {
    let mut b = Builder::new();
    crate::depth::enter(&mut b);
    let (reps, res) = numeric::sig(p);
    let mut regs = Vec::with_capacity(reps.len());
    for &rep in &reps {
        let a = b.param();
        if rep == Rep::Any {
            regs.push(a);
        } else {
            let r = b.reg(rep);
            b.emit(Inst::Unbox { dst: r, src: a });
            regs.push(r);
        }
    }
    if let Some(r) = numeric::emit(&mut b, p, &regs) {
        let out = if res == Rep::Any {
            r
        } else {
            let o = b.reg(Rep::Any);
            b.emit(Inst::Box { dst: o, src: r });
            o
        };
        b.terminate(Term::Return(out));
    }
    b
}

/// 組み込みの値 `name` の本体を組み立てる。
///
/// `each_loop` は `Array.each` の内側のループの関数である。
pub(crate) fn builtin_body(name: &str, each_loop: FuncId) -> Option<Builder> {
    let mut b = Builder::new();
    crate::depth::enter(&mut b);
    let n = builtin_arity(name)?;
    let args: Vec<Reg> = (0..n).map(|_| b.param()).collect();
    match name {
        "Array.each" => {
            let zero = b.konst(Const::I32(0));
            let i = b.reg(Rep::Any);
            b.emit(Inst::Box { dst: i, src: zero });
            b.terminate(Term::TailCallDirect {
                func: each_loop,
                args: vec![args[0], args[1], i],
                yields: true,
            });
        }
        "pinned" => {
            let dst = b.reg(Rep::Any);
            crate::depth::around_call(&mut b, |b| {
                b.emit(Inst::CallValue {
                    dst,
                    callee: args[0],
                    args: Vec::new(),
                    yields: true,
                })
            });
            b.terminate(Term::Return(dst));
        }
        _ => {
            if let Some(r) = builtin_inline(&mut b, name, &args) {
                b.terminate(Term::Return(r));
            }
        }
    }
    Some(b)
}

/// `Array.each` の内側のループの関数を組み立てる。
///
/// 引数は `(配列, 関数, 添字)` である。
/// `me` はこの関数自身で、次の要素に進むときに末尾呼び出しで呼ぶ。
pub(crate) fn each_loop_body(me: FuncId) -> Builder {
    let mut b = Builder::new();
    crate::depth::enter(&mut b);
    let arr = b.param();
    let f = b.param();
    let iv = b.param();
    let i = b.reg(Rep::I32);
    b.emit(Inst::Unbox { dst: i, src: iv });
    let n = b.reg(Rep::I32);
    b.emit(Inst::ArrayLen { dst: n, a: arr });
    let done = b.reg(Rep::Bool);
    b.emit(Inst::IntCmp {
        ty: IntTy::I32,
        op: CmpOp::Ge,
        dst: done,
        a: i,
        b: n,
    });
    let fin = b.block();
    let body = b.block();
    b.terminate(Term::Branch {
        cond: done,
        then: fin,
        else_: body,
    });
    b.switch_to(fin);
    let unit = b.konst(Const::Unit);
    b.terminate(Term::Return(unit));
    b.switch_to(body);
    let x = b.reg(Rep::Any);
    b.emit(Inst::ArrayGet {
        dst: x,
        a: arr,
        index: i,
    });
    let ignored = b.reg(Rep::Any);
    crate::depth::around_call(&mut b, |b| {
        b.emit(Inst::CallValue {
            dst: ignored,
            callee: f,
            args: vec![x],
            yields: true,
        })
    });
    let one = b.konst(Const::I32(1));
    let j = b.reg(Rep::I32);
    b.emit(Inst::IntArith {
        ty: IntTy::I32,
        op: IntOp::Add,
        dst: j,
        a: i,
        b: one,
    });
    let jv = b.reg(Rep::Any);
    b.emit(Inst::Box { dst: jv, src: j });
    b.terminate(Term::TailCallDirect {
        func: me,
        args: vec![arr, f, jv],
        yields: true,
    });
    b
}
