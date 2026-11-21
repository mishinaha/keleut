//! プリミティブの変換と、数値の明示的な検査(パス 7)。
//!
//! IRSpec の付録 A.5 のプリミティブと C の関数、付録 A.3 の組み込みのインスタンスのメソッドを、trap しない LIR の命令の並びに変換する。
//! 実行時エラーになることがある演算では、命令の前に明示的な検査と、実行時エラーへの分岐を置く。
//!
//! - 整数の除算と剰余では、除数が 0 なら実行時エラー `ゼロ除算です` にする。
//!   `MIN / -1` と `MIN % -1` は検査しない。
//!   命令 `div` と `rem` が、それぞれの結果を `MIN` と 0 に定めているからである。
//! - `Float64` から整数への変換では、先に NaN でないことと、範囲に入ることを検査する。
//!   範囲は、`Int32` へは `-2^31 ≦ trunc(x) < 2^31`、`Int64` へは `-2^63 ≦ trunc(x) < 2^63` で、どちらも上端は開いている。
//! - 整数の `+ - *` と符号反転は検査しない。
//!   オーバーフローした結果は折り返す(2 の補数で下位のビットだけが残る)。
//! - `__string_sub` では、先に位置と長さが範囲に入ることを検査する。

use crate::builder::Builder;
use crate::ir::*;

/// LIR の命令に変換するプリミティブの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prim {
    /// 整数の二項演算。
    Int(IntTy, IntOp),
    /// 整数の符号反転。
    IntNeg(IntTy),
    /// 整数の比較。
    IntCmp(IntTy, CmpOp),
    /// 浮動小数点数の二項演算。
    Float(FloatOp),
    /// 浮動小数点数の符号反転。
    FloatNeg,
    /// 浮動小数点数の比較。
    FloatCmp(CmpOp),
    /// C の数学関数。
    Math(MathFn),
    /// 文字列の連結。
    StrConcat,
    /// 文字列の比較。
    StrCmp(CmpOp),
    /// 文字列のバイト数を返す `__string_length`。
    StrLen,
    /// 部分文字列を取り出す `__string_sub`。
    StrSub,
    /// 数値変換。
    Conv(ConvOp),
    /// 値の文字列化。
    Show(ShowOp),
    /// `Show[String]`。
    /// 引数の文字列をそのまま結果にする。
    ShowStr,
    /// `Eq[Boolean]`。
    BoolEq,
    /// 実行時エラー `panic: <文字列>` で終了する `__panic`。
    Panic,
}

/// 実装名 `name` に対応するプリミティブを返す。
/// 対応するプリミティブがなければ `None` を返す。
pub(crate) fn by_name(abi: Abi, name: &str) -> Option<Prim> {
    use CmpOp::*;
    use IntTy::*;
    if abi == Abi::C {
        return Some(Prim::Math(match name {
            "sin" => MathFn::Sin,
            "cos" => MathFn::Cos,
            "sqrt" => MathFn::Sqrt,
            "exp" => MathFn::Exp,
            "log" => MathFn::Log,
            _ => return None,
        }));
    }
    let int = |rest: &str, t: IntTy| -> Option<Prim> {
        Some(match rest {
            "add" => Prim::Int(t, IntOp::Add),
            "sub" => Prim::Int(t, IntOp::Sub),
            "mul" => Prim::Int(t, IntOp::Mul),
            "div" => Prim::Int(t, IntOp::Div),
            "rem" => Prim::Int(t, IntOp::Rem),
            "neg" => Prim::IntNeg(t),
            "eq" => Prim::IntCmp(t, Eq),
            "lt" => Prim::IntCmp(t, Lt),
            "le" => Prim::IntCmp(t, Le),
            "gt" => Prim::IntCmp(t, Gt),
            "ge" => Prim::IntCmp(t, Ge),
            _ => return None,
        })
    };
    if let Some(rest) = name.strip_prefix("__int32_") {
        return int(rest, I32);
    }
    if let Some(rest) = name.strip_prefix("__int64_") {
        return int(rest, I64);
    }
    if let Some(rest) = name.strip_prefix("__float64_") {
        return Some(match rest {
            "add" => Prim::Float(FloatOp::Add),
            "sub" => Prim::Float(FloatOp::Sub),
            "mul" => Prim::Float(FloatOp::Mul),
            "div" => Prim::Float(FloatOp::Div),
            "neg" => Prim::FloatNeg,
            "eq" => Prim::FloatCmp(Eq),
            "lt" => Prim::FloatCmp(Lt),
            "le" => Prim::FloatCmp(Le),
            "gt" => Prim::FloatCmp(Gt),
            "ge" => Prim::FloatCmp(Ge),
            _ => return None,
        });
    }
    Some(match name {
        "__string_concat" => Prim::StrConcat,
        "__string_eq" => Prim::StrCmp(Eq),
        "__string_lt" => Prim::StrCmp(Lt),
        "__string_le" => Prim::StrCmp(Le),
        "__string_gt" => Prim::StrCmp(Gt),
        "__string_ge" => Prim::StrCmp(Ge),
        "__string_length" => Prim::StrLen,
        "__string_sub" => Prim::StrSub,
        "__i32_to_i64" => Prim::Conv(ConvOp::I32ToI64),
        "__i32_to_f64" => Prim::Conv(ConvOp::I32ToF64),
        "__i64_to_i32" => Prim::Conv(ConvOp::I64ToI32),
        "__i64_to_f64" => Prim::Conv(ConvOp::I64ToF64),
        "__f64_to_i32" => Prim::Conv(ConvOp::F64ToI32),
        "__f64_to_i64" => Prim::Conv(ConvOp::F64ToI64),
        "__show_int32" => Prim::Show(ShowOp::I32),
        "__panic" => Prim::Panic,
        _ => return None,
    })
}

/// 組み込みのインスタンスのメソッドに対応するプリミティブを返す(IRSpec の付録 A.3)。
///
/// `class` と `ty` には、シンボルから `builtin/` を取り除いた名前を渡す。
pub(crate) fn builtin_method(class: &str, ty: &str, method: &str) -> Option<Prim> {
    let row = kir::builtins::METHODS
        .iter()
        .find(|&&(c, t, m, _)| c == class && t == ty && m == method)?;
    match row.3 {
        Some(prim) => by_name(Abi::Prim, prim),
        None => Some(match (class, ty) {
            ("Eq", "Boolean") => Prim::BoolEq,
            ("Show", "Int64") => Prim::Show(ShowOp::I64),
            ("Show", "Float64") => Prim::Show(ShowOp::F64),
            ("Show", "String") => Prim::ShowStr,
            ("Show", "Boolean") => Prim::Show(ShowOp::Bool),
            _ => return None,
        }),
    }
}

/// プリミティブ `p` の引数の Rep の並びと、結果の Rep を返す。
pub(crate) fn sig(p: Prim) -> (Vec<Rep>, Rep) {
    use Rep::*;
    match p {
        Prim::Int(t, _) => (vec![t.rep(), t.rep()], t.rep()),
        Prim::IntNeg(t) => (vec![t.rep()], t.rep()),
        Prim::IntCmp(t, _) => (vec![t.rep(), t.rep()], Bool),
        Prim::Float(_) => (vec![F64, F64], F64),
        Prim::FloatNeg | Prim::Math(_) => (vec![F64], F64),
        Prim::FloatCmp(_) => (vec![F64, F64], Bool),
        Prim::StrConcat => (vec![Str, Str], Str),
        Prim::StrCmp(_) => (vec![Str, Str], Bool),
        Prim::StrLen => (vec![Str], I32),
        Prim::StrSub => (vec![Str, I32, I32], Str),
        Prim::Conv(op) => {
            let (a, r) = op.reps();
            (vec![a], r)
        }
        Prim::Show(op) => (vec![op.rep()], Str),
        Prim::ShowStr => (vec![Str], Str),
        Prim::BoolEq => (vec![Bool, Bool], Bool),
        Prim::Panic => (vec![Str], Any),
    }
}

/// 2^31 を表す `f64` のビット列。
/// `TWO_63`、`MINUS_TWO_31`、`MINUS_TWO_63` も同じように、それぞれ 2^63、-2^31、-2^63 を表す `f64` のビット列である。
const TWO_31: u64 = 0x41e0_0000_0000_0000;
const TWO_63: u64 = 0x43e0_0000_0000_0000;
const MINUS_TWO_31: u64 = 0xc1e0_0000_0000_0000;
const MINUS_TWO_63: u64 = 0xc3e0_0000_0000_0000;

/// プリミティブ `p` の命令を生成し、結果のレジスタを返す。
///
/// `args` は、[`sig`] が返す Rep を持つレジスタでなければならない。
/// 必ず実行時エラーになるプリミティブ(`__panic`)では、ブロックを終えて `None` を返す。
pub(crate) fn emit(b: &mut Builder, p: Prim, args: &[Reg]) -> Option<Reg> {
    let (_, res) = sig(p);
    let a = args[0];
    let dst = b.reg(res);
    let inst = match p {
        Prim::Int(ty, op) => {
            let d = args[1];
            if matches!(op, IntOp::Div | IntOp::Rem) {
                let zero = b.konst(match ty {
                    IntTy::I32 => Const::I32(0),
                    IntTy::I64 => Const::I64(0),
                });
                let bad = b.reg(Rep::Bool);
                b.emit(Inst::IntCmp {
                    ty,
                    op: CmpOp::Eq,
                    dst: bad,
                    a: d,
                    b: zero,
                });
                b.fail_if(bad, RtError::DivZero, None);
            }
            Inst::IntArith {
                ty,
                op,
                dst,
                a,
                b: d,
            }
        }
        Prim::IntNeg(ty) => Inst::IntNeg { ty, dst, a },
        Prim::IntCmp(ty, op) => Inst::IntCmp {
            ty,
            op,
            dst,
            a,
            b: args[1],
        },
        Prim::Float(op) => Inst::FloatArith {
            op,
            dst,
            a,
            b: args[1],
        },
        Prim::FloatNeg => Inst::FloatNeg { dst, a },
        Prim::FloatCmp(op) => Inst::FloatCmp {
            op,
            dst,
            a,
            b: args[1],
        },
        Prim::Math(f) => Inst::FloatMath { f, dst, a },
        Prim::StrConcat => Inst::StrConcat { dst, a, b: args[1] },
        Prim::StrCmp(op) => Inst::StrCmp {
            op,
            dst,
            a,
            b: args[1],
        },
        Prim::StrLen => Inst::StrLen { dst, a },
        Prim::StrSub => {
            let (pos, len) = (args[1], args[2]);
            let zero = b.konst(Const::I32(0));
            for x in [pos, len] {
                let bad = b.reg(Rep::Bool);
                b.emit(Inst::IntCmp {
                    ty: IntTy::I32,
                    op: CmpOp::Lt,
                    dst: bad,
                    a: x,
                    b: zero,
                });
                b.fail_if(bad, RtError::StrSubRange, None);
            }
            // 0 ≦ len < 2^31 で、文字列のバイト数 n も 0 ≦ n < 2^31 なので、n - len はオーバーフローしない。
            // len が n を超えていれば差は負になり、0 以上の pos との比較で範囲外と判定される。
            let n = b.reg(Rep::I32);
            b.emit(Inst::StrLen { dst: n, a });
            let room = b.reg(Rep::I32);
            b.emit(Inst::IntArith {
                ty: IntTy::I32,
                op: IntOp::Sub,
                dst: room,
                a: n,
                b: len,
            });
            let bad = b.reg(Rep::Bool);
            b.emit(Inst::IntCmp {
                ty: IntTy::I32,
                op: CmpOp::Gt,
                dst: bad,
                a: pos,
                b: room,
            });
            b.fail_if(bad, RtError::StrSubRange, None);
            Inst::StrSub {
                dst,
                s: a,
                pos,
                len,
            }
        }
        Prim::Conv(op) => {
            if matches!(op, ConvOp::F64ToI32 | ConvOp::F64ToI64) {
                f64_to_int_checks(b, op, a);
            }
            Inst::Conv { op, dst, a }
        }
        Prim::Show(op) => Inst::Show { op, dst, a },
        Prim::ShowStr => Inst::Move { dst, src: a },
        Prim::BoolEq => Inst::BoolEq { dst, a, b: args[1] },
        Prim::Panic => {
            b.terminate(Term::Error {
                kind: RtError::Panic,
                operand: Some(a),
            });
            return None;
        }
    };
    b.emit(inst);
    Some(dst)
}

/// `__f64_to_i32` と `__f64_to_i64` の変換の前に置く、NaN と範囲の検査を生成する。
fn f64_to_int_checks(b: &mut Builder, op: ConvOp, x: Reg) {
    let nan = b.reg(Rep::Bool);
    b.emit(Inst::FloatIsNan { dst: nan, a: x });
    b.fail_if(nan, RtError::F64ToIntNan(op), None);
    let (lo, hi) = match op {
        ConvOp::F64ToI32 => (MINUS_TWO_31, TWO_31),
        _ => (MINUS_TWO_63, TWO_63),
    };
    let t = b.reg(Rep::F64);
    b.emit(Inst::FloatTrunc { dst: t, a: x });
    let lo = b.konst(Const::F64(lo));
    let hi = b.konst(Const::F64(hi));
    let ge = b.reg(Rep::Bool);
    b.emit(Inst::FloatCmp {
        op: CmpOp::Ge,
        dst: ge,
        a: t,
        b: lo,
    });
    let lt = b.reg(Rep::Bool);
    b.emit(Inst::FloatCmp {
        op: CmpOp::Lt,
        dst: lt,
        a: t,
        b: hi,
    });
    let both = b.reg(Rep::Bool);
    b.emit(Inst::BoolEq {
        dst: both,
        a: ge,
        b: lt,
    });
    // 範囲に入るのは、ge と lt がともに真のときだけである。
    // trunc(x) が下限(-2^31 か -2^63)より小さければ ge は偽で lt は真になり、上限(2^31 か 2^63)以上なら ge は真で lt は偽になる。
    // NaN は先の検査でエラーにしたので、ge と lt がともに偽になることはない。
    // したがって、ge と lt が等しくなければ範囲外である。
    let bad = b.reg(Rep::Bool);
    b.emit(Inst::BoolNot { dst: bad, a: both });
    b.fail_if(bad, RtError::F64ToIntRange(op), Some(x));
}
