//! 組み込みの値、`extern`、組み込みのインスタンスのメソッドの本体。
//!
//! これらの値は、どれも LIR の関数のクロージャである。
//! 値として渡されて `call_value` で呼ばれても、本体は LIR の命令だけで書いてあるので、
//! 後段は LIR の命令の意味のほかに組み込みの振る舞いを知らなくてよい。
//! 直に呼ぶ箇所(`call` の呼び出し先が組み込みの値の大域であるもの)は、[`builtin_inline`] で命令をその場に置く。
//!
//! 利用者の関数を呼ぶ `Array.each` と `pinned` は、呼び出しを末尾でない呼び出しとして数える
//! (IRSpec 付録 A.4。組み込みの深さ + 1)。
//! `Array.each` の繰り返しは、内側のループの関数の自己の末尾呼び出しで書く。

use crate::builder::Builder;
use crate::ir::*;
use crate::numeric::{self, Prim};

/// その場に置ける組み込みの値の引数の数。
pub(crate) fn inline_arity(name: &str) -> Option<usize> {
    Some(match name {
        "Ref.new" | "Ref.get" | "Array.length" | "MutableArray.length" | "MutableArray.freeze" => 1,
        "Ref.set" | "Array.get" | "MutableArray.new" | "MutableArray.get" => 2,
        "MutableArray.set" => 3,
        _ => return None,
    })
}

/// 組み込みの値の引数の数。
pub(crate) fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "Array.each" => Some(2),
        "pinned" => Some(1),
        _ => inline_arity(name),
    }
}

/// 一様な語の `index` を `i32` にし、`0 ≦ index < len` でなければ「配列の範囲外です」にする。
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

/// 組み込みの値をその場に置く。引数も結果も一様な語である。
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

/// 引数をすべて受け取ったプリミティブの本体を組み立てる。
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

/// 組み込みの値の本体を組み立てる。`each_loop` は `Array.each` の内側のループの関数である。
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

/// `Array.each` の内側のループ `(配列, 関数, 位置)` を組み立てる。`me` はこの関数自身である。
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
