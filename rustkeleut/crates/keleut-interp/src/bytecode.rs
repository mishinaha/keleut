//! バイトコード(計画 261024-1 の 6.7 節、D535)。
//!
//! バイトコードは LIR の関数を 1 対 1 に符号化したもので、関数を最初に呼ぶときに作る。
//! ファイルには書かない。
//!
//! 命令は 32 ビットの語の列である。
//! 先頭の語は、下位 8 ビットが命令の番号、その上の 24 ビットが命令の細目(演算の種類など)である。
//! 被演算子は続く語に 1 つずつ置く。
//! スロットの被演算子は、`any` の区画のスロット `n` を `n`、生の区画のスロット `n` を `any の数 + n` と数えた通しの番号である。
//! 64 ビットの定数は下位、上位の 2 語に分ける。
//!
//! 計画 6.7 節は、スロットが 256 を超える関数に wide 形を使うとしている。
//! 本実装は被演算子を最初から 32 ビットの語にするので、wide 形を別に持たない。
//!
//! 末尾でない呼び出しの命令は、書き先のスロットを持たない。
//! 書き先と、呼ばれた側が戻った後に続ける位置は、再開点の番号から引く表 [`Code::resume`] にある。
//! この表があるので、別の後段が作ったフレームも再開点から続けられる。

use keleut_lir::{
    BlockId, CmpOp, Const, ConvOp, FloatOp, Func, FuncId, Inst, IntOp, IntTy, MathFn, Program, Reg,
    Rep, RtError, ShowOp, Slot, Term,
};
use keleut_rt::Rt;
use keleut_rt::stack::HEADER_WORDS;
use std::collections::HashMap;

/// 命令の番号。
#[allow(missing_docs)]
pub mod op {
    pub const CONST: u8 = 1;
    pub const MOVE: u8 = 2;
    pub const BOX_BOOL: u8 = 3;
    pub const BOX_I32: u8 = 4;
    pub const BOX_I64: u8 = 5;
    pub const BOX_F64: u8 = 6;
    pub const UNBOX_BOOL: u8 = 7;
    pub const UNBOX_I32: u8 = 8;
    pub const UNBOX_I64: u8 = 9;
    pub const UNBOX_F64: u8 = 10;
    pub const ENV_GET: u8 = 11;
    pub const DEPTH_ENTER: u8 = 12;
    pub const DEPTH_CHECK: u8 = 13;
    pub const DEPTH_PUSH: u8 = 14;
    pub const DEPTH_RESTORE: u8 = 15;
    pub const I32_ARITH: u8 = 16;
    pub const I64_ARITH: u8 = 17;
    pub const I32_NEG: u8 = 18;
    pub const I64_NEG: u8 = 19;
    pub const I32_CMP: u8 = 20;
    pub const I64_CMP: u8 = 21;
    pub const F64_ARITH: u8 = 22;
    pub const F64_NEG: u8 = 23;
    pub const F64_CMP: u8 = 24;
    pub const F64_MATH: u8 = 25;
    pub const F64_TRUNC: u8 = 26;
    pub const F64_ISNAN: u8 = 27;
    pub const CONV: u8 = 28;
    pub const BOOL_NOT: u8 = 29;
    pub const BOOL_EQ: u8 = 30;
    pub const STR_CONCAT: u8 = 31;
    pub const STR_CMP: u8 = 32;
    pub const STR_LEN: u8 = 33;
    pub const STR_SUB: u8 = 34;
    pub const SHOW: u8 = 35;
    pub const CTOR_NEW: u8 = 36;
    pub const CTOR_TAG: u8 = 37;
    pub const CTOR_FIELD: u8 = 38;
    pub const VARIANT_NEW: u8 = 39;
    pub const VARIANT_TAG: u8 = 40;
    pub const VARIANT_PAYLOAD: u8 = 41;
    pub const REC_NEW: u8 = 42;
    pub const REC_GET: u8 = 43;
    pub const REC_EXTEND: u8 = 44;
    pub const REC_FIELD: u8 = 45;
    pub const REC_HAS: u8 = 46;
    pub const REC_RESTRICT: u8 = 47;
    pub const REC_UPDATE: u8 = 48;
    pub const CLOSURE_NEW: u8 = 49;
    pub const CLOSURE_SET_CAP: u8 = 50;
    pub const METHOD_NEW: u8 = 51;
    pub const DICT_INST: u8 = 52;
    pub const DICT_RECORD: u8 = 53;
    pub const DICT_VARIANT: u8 = 54;
    pub const GLOBAL_GET: u8 = 55;
    pub const GLOBAL_IS_INIT: u8 = 56;
    pub const GLOBAL_SET: u8 = 57;
    pub const GLOBAL_MARK_INIT: u8 = 58;
    pub const INST_IS_INIT: u8 = 59;
    pub const INST_MARK_INIT: u8 = 60;
    pub const INST_SET_VALUE: u8 = 61;
    pub const INST_GET_VALUE: u8 = 62;
    pub const REF_NEW: u8 = 63;
    pub const REF_GET: u8 = 64;
    pub const REF_SET: u8 = 65;
    pub const ARRAY_LEN: u8 = 66;
    pub const ARRAY_GET: u8 = 67;
    pub const MARRAY_NEW: u8 = 68;
    pub const MARRAY_SET: u8 = 69;
    pub const MARRAY_FREEZE: u8 = 70;
    pub const CALL_DIRECT: u8 = 71;
    pub const CALL_VALUE: u8 = 72;
    pub const CALL_METHOD: u8 = 73;
    pub const DICT_APP: u8 = 74;
    pub const CONSOLE_WRITE: u8 = 75;
    pub const UNIMPL_VALUE: u8 = 76;
    pub const JUMP: u8 = 77;
    pub const BRANCH: u8 = 78;
    pub const RETURN: u8 = 79;
    pub const TAIL_DIRECT: u8 = 80;
    pub const TAIL_VALUE: u8 = 81;
    pub const TAIL_METHOD: u8 = 82;
    pub const ERROR: u8 = 83;
    pub const UNIMPLEMENTED: u8 = 84;
    pub const FX: u8 = 85;
    pub const FX_TERM: u8 = 86;
}

/// 再開点の表の行の種類。
pub mod rk {
    /// ふつうの末尾でない呼び出し。番兵で戻ったら呼び出し元も番兵で戻る(伝播)。
    pub const ORDINARY: u8 = 0;
    /// 番兵を受け取る呼び出し(`call_body`、`cont_resume`、`call_clause`、`call_cancel`)。番兵を結果に書いて続ける。
    pub const CATCH: u8 = 1;
    /// `perform`。
    pub const PERFORM: u8 = 2;
    /// メソッドの値を末尾で呼ぶ命令の行(実装の関数を評価する間だけ使う)。
    pub const TAIL: u8 = 3;
}

/// 再開点の表の 1 行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumeEntry {
    /// 呼び出しの命令の位置。
    pub call_pc: u32,
    /// 呼ばれた側が戻った後に続ける位置。
    pub pc_after: u32,
    /// 戻り値を書くスロット(末尾の呼び出しの行では使わない)。
    pub dst: u32,
    /// 行の種類([`rk`])。
    pub kind: u8,
    /// yield を受け取りうるか(パス 11。`doc/lir.md` の 17 節)。
    pub yields: bool,
}

/// 1 つの関数のバイトコード。
#[derive(Debug)]
pub struct Code {
    /// 関数。
    pub func: FuncId,
    /// 命令の語の列。
    pub words: Vec<u32>,
    /// 再開点の番号から引く表。
    ///
    /// 先頭の行は LIR の再開点(番号は LIR の [`keleut_lir::Frame::resume_points`] の位置)である。
    /// その後ろに、メソッドの値を末尾で呼ぶ命令(`tail_call_method` と `tail_call_value`)ごとの行を置く。
    /// 実装の関数を先に評価するとき、呼び出し元のフレームの再開点にこの行の番号を書く。
    pub resume: Vec<ResumeEntry>,
    /// `any` の区画のスロットの数。
    pub any_slots: u32,
    /// 生の区画のスロットの数。
    pub raw_slots: u32,
    /// フレームの語の数(見出しを含む)。
    pub frame_words: usize,
    /// `error` の種類。
    pub errors: Vec<RtError>,
    /// `unimplemented` の文面。
    pub texts: Vec<String>,
    /// エフェクトの命令の種類(`FX` の細目が添字)。
    pub fx: Vec<keleut_lir::FxOp>,
    /// エフェクトのブロックの終わりの種類(`FX_TERM` の細目が添字)。
    pub fx_terms: Vec<keleut_lir::FxTermOp>,
}

struct Enc<'a> {
    f: &'a Func,
    words: Vec<u32>,
    any: u32,
    slot: Vec<u32>,
    resume: Vec<ResumeEntry>,
    errors: Vec<RtError>,
    texts: Vec<String>,
    fx: Vec<keleut_lir::FxOp>,
    fx_terms: Vec<keleut_lir::FxTermOp>,
    block_pc: Vec<u32>,
    fixups: Vec<(usize, BlockId)>,
}

impl Enc<'_> {
    fn s(&self, r: Reg) -> u32 {
        self.slot[r.index()]
    }

    fn op(&mut self, op: u8, sub: u32) {
        self.words.push(u32::from(op) | (sub << 8));
    }

    fn w(&mut self, x: u32) {
        self.words.push(x);
    }

    fn r(&mut self, r: Reg) {
        let s = self.s(r);
        self.words.push(s);
    }

    fn regs(&mut self, rs: &[Reg]) {
        self.w(rs.len() as u32);
        for &r in rs {
            self.r(r);
        }
    }

    fn w64(&mut self, x: u64) {
        self.w(x as u32);
        self.w((x >> 32) as u32);
    }

    fn rep(&self, r: Reg) -> Rep {
        self.f.regs[r.index()]
    }

    fn target(&mut self, b: BlockId) {
        self.fixups.push((self.words.len(), b));
        self.w(0);
    }
}

fn int_sub(op: IntOp) -> u32 {
    match op {
        IntOp::Add => 0,
        IntOp::Sub => 1,
        IntOp::Mul => 2,
        IntOp::Div => 3,
        IntOp::Rem => 4,
    }
}

/// 比べ方の細目。
pub fn cmp_sub(op: CmpOp) -> u32 {
    match op {
        CmpOp::Eq => 0,
        CmpOp::Lt => 1,
        CmpOp::Le => 2,
        CmpOp::Gt => 3,
        CmpOp::Ge => 4,
    }
}

fn float_sub(op: FloatOp) -> u32 {
    match op {
        FloatOp::Add => 0,
        FloatOp::Sub => 1,
        FloatOp::Mul => 2,
        FloatOp::Div => 3,
    }
}

fn math_sub(f: MathFn) -> u32 {
    match f {
        MathFn::Sin => 0,
        MathFn::Cos => 1,
        MathFn::Sqrt => 2,
        MathFn::Exp => 3,
        MathFn::Log => 4,
    }
}

/// 変換の細目。
pub fn conv_sub(op: ConvOp) -> u32 {
    match op {
        ConvOp::I32ToI64 => 0,
        ConvOp::I32ToF64 => 1,
        ConvOp::I64ToI32 => 2,
        ConvOp::I64ToF64 => 3,
        ConvOp::F64ToI32 => 4,
        ConvOp::F64ToI64 => 5,
    }
}

fn show_sub(op: ShowOp) -> u32 {
    match op {
        ShowOp::I32 => 0,
        ShowOp::I64 => 1,
        ShowOp::F64 => 2,
        ShowOp::Bool => 3,
    }
}

/// 定数の 64 ビットの値(生の Rep なら生の値、`any` と `str` なら一様な語)。
fn const_word(rt: &Rt<'_>, c: &Const) -> u64 {
    use keleut_rt::Word;
    match c {
        Const::Unit => Word::UNIT.0,
        Const::Bool(b) => u64::from(*b),
        Const::I32(n) => *n as i64 as u64,
        Const::I64(n) => *n as u64,
        Const::F64(bits) => *bits,
        Const::Str(s) => rt.strings[s.index()].0,
        Const::Ctor0 { ty, ctor } => Word::ctor0(ty.0, *ctor).0,
        Const::DictBuiltin { class, ty } => Word::dict_builtin(class.0, ty.0).0,
        Const::DictInst0(i) => Word::dict_inst0(i.0).0,
    }
}

/// 関数 `fid` をバイトコードにする。関数はパス 12 を済ませていなければならない。
pub fn encode(prog: &Program, rt: &Rt<'_>, fid: FuncId) -> Result<Code, String> {
    let f = &prog.funcs[fid.index()];
    let frame = f
        .frame
        .as_ref()
        .ok_or_else(|| format!("関数 {} にフレームがありません", f.sym))?;
    let any = frame.any_slots;
    let slot: Vec<u32> = frame
        .slot
        .iter()
        .map(|s| match *s {
            Slot::Any(n) => n,
            Slot::Raw(n) => any + n,
        })
        .collect();
    let mut rp_of: HashMap<(u32, u32), u32> = HashMap::new();
    for (i, p) in frame.resume_points.points().iter().enumerate() {
        rp_of.insert((p.block.0, p.index), i as u32);
    }
    let nrp = frame.resume_points.len();
    let mut e = Enc {
        f,
        words: Vec::new(),
        any,
        slot,
        resume: vec![
            ResumeEntry {
                call_pc: u32::MAX,
                pc_after: u32::MAX,
                dst: u32::MAX,
                kind: rk::ORDINARY,
                yields: true,
            };
            nrp
        ],
        errors: Vec::new(),
        texts: Vec::new(),
        fx: Vec::new(),
        fx_terms: Vec::new(),
        block_pc: vec![0; f.blocks.len()],
        fixups: Vec::new(),
    };
    for (bi, b) in f.blocks.iter().enumerate() {
        e.block_pc[bi] = e.words.len() as u32;
        for (ii, inst) in b.insts.iter().enumerate() {
            let start = e.words.len() as u32;
            let rp = rp_of.get(&(bi as u32, ii as u32)).copied();
            encode_inst(&mut e, rt, inst, rp)?;
            if inst.is_call() {
                let rp = rp.ok_or_else(|| {
                    format!("{}: b{bi} の {ii} 番の呼び出しに再開点がありません", f.sym)
                })?;
                let kind = match inst {
                    Inst::Fx {
                        op: keleut_lir::FxOp::Perform { .. },
                        ..
                    } => rk::PERFORM,
                    Inst::Fx { op, .. } if op.catches() => rk::CATCH,
                    _ => rk::ORDINARY,
                };
                e.resume[rp as usize] = ResumeEntry {
                    call_pc: start,
                    pc_after: e.words.len() as u32,
                    dst: e.s(inst.def().expect("呼び出しは書き先を持つ")),
                    kind,
                    yields: frame.yield_points.get(rp as usize).copied().unwrap_or(true),
                };
            }
        }
        encode_term(&mut e, &b.term)?;
    }
    for (at, b) in std::mem::take(&mut e.fixups) {
        e.words[at] = e.block_pc[b.index()];
    }
    if e.resume.iter().any(|r| r.call_pc == u32::MAX) {
        return Err(format!("{}: 命令に対応しない再開点があります", f.sym));
    }
    let _ = e.any;
    Ok(Code {
        func: fid,
        words: e.words,
        resume: e.resume,
        any_slots: any,
        raw_slots: frame.raw_slots,
        frame_words: HEADER_WORDS + (any + frame.raw_slots) as usize,
        errors: e.errors,
        texts: e.texts,
        fx: e.fx,
        fx_terms: e.fx_terms,
    })
}

fn encode_inst(e: &mut Enc<'_>, rt: &Rt<'_>, inst: &Inst, rp: Option<u32>) -> Result<(), String> {
    use op::*;
    let rp = || rp.ok_or_else(|| "呼び出しに再開点がありません".to_string());
    match inst {
        Inst::Const { dst, value } => {
            e.op(CONST, 0);
            e.r(*dst);
            e.w64(const_word(rt, value));
        }
        Inst::Move { dst, src } => {
            e.op(MOVE, 0);
            e.r(*dst);
            e.r(*src);
        }
        Inst::Box { dst, src } => {
            let o = match e.rep(*src) {
                Rep::Bool => BOX_BOOL,
                Rep::I32 => BOX_I32,
                Rep::I64 => BOX_I64,
                Rep::F64 => BOX_F64,
                Rep::Str | Rep::Any => MOVE,
            };
            e.op(o, 0);
            e.r(*dst);
            e.r(*src);
        }
        Inst::Unbox { dst, src } => {
            let o = match e.rep(*dst) {
                Rep::Bool => UNBOX_BOOL,
                Rep::I32 => UNBOX_I32,
                Rep::I64 => UNBOX_I64,
                Rep::F64 => UNBOX_F64,
                Rep::Str | Rep::Any => MOVE,
            };
            e.op(o, 0);
            e.r(*dst);
            e.r(*src);
        }
        Inst::EnvGet { dst, index } => {
            e.op(ENV_GET, 0);
            e.r(*dst);
            e.w(*index);
        }
        Inst::DepthEnter => e.op(DEPTH_ENTER, 0),
        Inst::DepthCheck => e.op(DEPTH_CHECK, 0),
        Inst::DepthPush => e.op(DEPTH_PUSH, 0),
        Inst::DepthRestore => e.op(DEPTH_RESTORE, 0),
        Inst::IntArith { ty, op, dst, a, b } => {
            e.op(
                if *ty == IntTy::I32 {
                    I32_ARITH
                } else {
                    I64_ARITH
                },
                int_sub(*op),
            );
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::IntNeg { ty, dst, a } => {
            e.op(if *ty == IntTy::I32 { I32_NEG } else { I64_NEG }, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::IntCmp { ty, op, dst, a, b } => {
            e.op(
                if *ty == IntTy::I32 { I32_CMP } else { I64_CMP },
                cmp_sub(*op),
            );
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::FloatArith { op, dst, a, b } => {
            e.op(F64_ARITH, float_sub(*op));
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::FloatNeg { dst, a } => {
            e.op(F64_NEG, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::FloatCmp { op, dst, a, b } => {
            e.op(F64_CMP, cmp_sub(*op));
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::FloatMath { f, dst, a } => {
            e.op(F64_MATH, math_sub(*f));
            e.r(*dst);
            e.r(*a);
        }
        Inst::FloatTrunc { dst, a } => {
            e.op(F64_TRUNC, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::FloatIsNan { dst, a } => {
            e.op(F64_ISNAN, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::Conv { op, dst, a } => {
            e.op(CONV, conv_sub(*op));
            e.r(*dst);
            e.r(*a);
        }
        Inst::BoolNot { dst, a } => {
            e.op(BOOL_NOT, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::BoolEq { dst, a, b } => {
            e.op(BOOL_EQ, 0);
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::StrConcat { dst, a, b } => {
            e.op(STR_CONCAT, 0);
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::StrCmp { op, dst, a, b } => {
            e.op(STR_CMP, cmp_sub(*op));
            e.r(*dst);
            e.r(*a);
            e.r(*b);
        }
        Inst::StrLen { dst, a } => {
            e.op(STR_LEN, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::StrSub { dst, s, pos, len } => {
            e.op(STR_SUB, 0);
            e.r(*dst);
            e.r(*s);
            e.r(*pos);
            e.r(*len);
        }
        Inst::Show { op, dst, a } => {
            e.op(SHOW, show_sub(*op));
            e.r(*dst);
            e.r(*a);
        }
        Inst::CtorNew {
            dst,
            ty,
            ctor,
            fields,
        } => {
            e.op(CTOR_NEW, 0);
            e.r(*dst);
            e.w(ty.0);
            e.w(*ctor);
            e.regs(fields);
        }
        Inst::CtorTag { dst, v } => {
            e.op(CTOR_TAG, 0);
            e.r(*dst);
            e.r(*v);
        }
        Inst::CtorField { dst, v, index } => {
            e.op(CTOR_FIELD, 0);
            e.r(*dst);
            e.r(*v);
            e.w(*index);
        }
        Inst::VariantNew {
            dst,
            label,
            payload,
        } => {
            e.op(VARIANT_NEW, 0);
            e.r(*dst);
            e.w(label.0);
            e.r(*payload);
        }
        Inst::VariantTag { dst, v } => {
            e.op(VARIANT_TAG, 0);
            e.r(*dst);
            e.r(*v);
        }
        Inst::VariantPayload { dst, v } => {
            e.op(VARIANT_PAYLOAD, 0);
            e.r(*dst);
            e.r(*v);
        }
        Inst::RecNew { dst, shape, fields } => {
            e.op(REC_NEW, 0);
            e.r(*dst);
            e.w(shape.0);
            e.regs(fields);
        }
        Inst::RecGet { dst, rec, index } => {
            e.op(REC_GET, 0);
            e.r(*dst);
            e.r(*rec);
            e.w(*index);
        }
        Inst::RecExtend {
            dst,
            rec,
            label,
            value,
        } => {
            e.op(REC_EXTEND, 0);
            e.r(*dst);
            e.r(*rec);
            e.w(label.0);
            e.r(*value);
        }
        Inst::RecField {
            dst,
            rec,
            label,
            nth,
        } => {
            e.op(REC_FIELD, 0);
            e.r(*dst);
            e.r(*rec);
            e.w(label.0);
            e.w(*nth);
        }
        Inst::RecHas {
            dst,
            rec,
            label,
            nth,
        } => {
            e.op(REC_HAS, 0);
            e.r(*dst);
            e.r(*rec);
            e.w(label.0);
            e.w(*nth);
        }
        Inst::RecRestrict { dst, rec, label } => {
            e.op(REC_RESTRICT, 0);
            e.r(*dst);
            e.r(*rec);
            e.w(label.0);
        }
        Inst::RecUpdate {
            dst,
            rec,
            label,
            value,
        } => {
            e.op(REC_UPDATE, 0);
            e.r(*dst);
            e.r(*rec);
            e.w(label.0);
            e.r(*value);
        }
        Inst::ClosureNew { dst, func, caps } => {
            e.op(CLOSURE_NEW, 0);
            e.r(*dst);
            e.w(func.0);
            e.regs(caps);
        }
        Inst::ClosureSetCap { clo, index, value } => {
            e.op(CLOSURE_SET_CAP, 0);
            e.r(*clo);
            e.w(*index);
            e.r(*value);
        }
        Inst::MethodNew {
            dst,
            class,
            method,
            dict,
            own,
        } => {
            e.op(METHOD_NEW, 0);
            e.r(*dst);
            e.w(class.0);
            e.w(*method);
            e.r(*dict);
            e.regs(own);
        }
        Inst::DictInst {
            dst,
            inst,
            premises,
        } => {
            e.op(DICT_INST, 0);
            e.r(*dst);
            e.w(inst.0);
            e.regs(premises);
        }
        Inst::DictRecord { dst, fields } | Inst::DictVariant { dst, fields } => {
            e.op(
                if matches!(inst, Inst::DictRecord { .. }) {
                    DICT_RECORD
                } else {
                    DICT_VARIANT
                },
                0,
            );
            e.r(*dst);
            e.w(fields.len() as u32);
            for (l, r) in fields {
                e.w(l.0);
                e.r(*r);
            }
        }
        Inst::GlobalGet { dst, global } => {
            e.op(GLOBAL_GET, 0);
            e.r(*dst);
            e.w(global.0);
        }
        Inst::GlobalIsInit { dst, global } => {
            e.op(GLOBAL_IS_INIT, 0);
            e.r(*dst);
            e.w(global.0);
        }
        Inst::GlobalSet { global, src } => {
            e.op(GLOBAL_SET, 0);
            e.w(global.0);
            e.r(*src);
        }
        Inst::GlobalMarkInit { global } => {
            e.op(GLOBAL_MARK_INIT, 0);
            e.w(global.0);
        }
        Inst::InstIsInit { dst, inst } => {
            e.op(INST_IS_INIT, 0);
            e.r(*dst);
            e.w(inst.0);
        }
        Inst::InstMarkInit { inst } => {
            e.op(INST_MARK_INIT, 0);
            e.w(inst.0);
        }
        Inst::InstSetValue { inst, method, src } => {
            e.op(INST_SET_VALUE, 0);
            e.w(inst.0);
            e.w(*method);
            e.r(*src);
        }
        Inst::InstGetValue { dst, inst, method } => {
            e.op(INST_GET_VALUE, 0);
            e.r(*dst);
            e.w(inst.0);
            e.w(*method);
        }
        Inst::RefNew { dst, value } => {
            e.op(REF_NEW, 0);
            e.r(*dst);
            e.r(*value);
        }
        Inst::RefGet { dst, r } => {
            e.op(REF_GET, 0);
            e.r(*dst);
            e.r(*r);
        }
        Inst::RefSet { r, value } => {
            e.op(REF_SET, 0);
            e.r(*r);
            e.r(*value);
        }
        Inst::ArrayLen { dst, a } | Inst::MArrayLen { dst, a } => {
            e.op(ARRAY_LEN, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::ArrayGet { dst, a, index } | Inst::MArrayGet { dst, a, index } => {
            e.op(ARRAY_GET, 0);
            e.r(*dst);
            e.r(*a);
            e.r(*index);
        }
        Inst::MArrayNew { dst, len, init } => {
            e.op(MARRAY_NEW, 0);
            e.r(*dst);
            e.r(*len);
            e.r(*init);
        }
        Inst::MArraySet { a, index, value } => {
            e.op(MARRAY_SET, 0);
            e.r(*a);
            e.r(*index);
            e.r(*value);
        }
        Inst::MArrayFreeze { dst, a } => {
            e.op(MARRAY_FREEZE, 0);
            e.r(*dst);
            e.r(*a);
        }
        Inst::CallDirect { func, args, .. } => {
            e.op(CALL_DIRECT, 0);
            e.w(rp()?);
            e.w(func.0);
            e.regs(args);
        }
        Inst::CallValue { callee, args, .. } => {
            e.op(CALL_VALUE, 0);
            e.w(rp()?);
            e.r(*callee);
            e.regs(args);
        }
        Inst::CallMethod {
            class,
            method,
            dict,
            own,
            args,
            ..
        } => {
            e.op(CALL_METHOD, 0);
            e.w(rp()?);
            e.w(class.0);
            e.w(*method);
            e.r(*dict);
            e.regs(own);
            e.regs(args);
        }
        Inst::DictApp { f, dicts, .. } => {
            e.op(DICT_APP, 0);
            e.w(rp()?);
            e.r(*f);
            e.regs(dicts);
        }
        Inst::ConsoleWrite { dst, s } => {
            e.op(CONSOLE_WRITE, 0);
            e.r(*dst);
            e.r(*s);
        }
        Inst::UnimplementedValue { what, .. } => {
            e.texts.push(what.clone());
            e.op(UNIMPL_VALUE, (e.texts.len() - 1) as u32);
        }
        Inst::Fx { op, dst, args } => {
            e.fx.push(op.clone());
            e.op(FX, (e.fx.len() - 1) as u32);
            e.w(if op.is_call() { rp()? } else { u32::MAX });
            match dst {
                Some(d) => e.r(*d),
                None => e.w(u32::MAX),
            }
            e.regs(args);
        }
    }
    Ok(())
}

fn tail_entry(e: &mut Enc<'_>, start: usize) -> u32 {
    e.resume.push(ResumeEntry {
        call_pc: start as u32,
        pc_after: u32::MAX,
        dst: u32::MAX,
        kind: rk::TAIL,
        yields: true,
    });
    (e.resume.len() - 1) as u32
}

fn encode_term(e: &mut Enc<'_>, t: &Term) -> Result<(), String> {
    use op::*;
    let start = e.words.len();
    match t {
        Term::Jump(b) => {
            e.op(JUMP, 0);
            e.target(*b);
        }
        Term::Branch { cond, then, else_ } => {
            e.op(BRANCH, 0);
            e.r(*cond);
            e.target(*then);
            e.target(*else_);
        }
        Term::Return(r) => {
            e.op(RETURN, 0);
            e.r(*r);
        }
        Term::TailCallDirect { func, args, .. } => {
            e.op(TAIL_DIRECT, 0);
            e.w(func.0);
            e.regs(args);
        }
        Term::TailCallValue { callee, args, .. } => {
            let rp = tail_entry(e, start);
            e.op(TAIL_VALUE, 0);
            e.w(rp);
            e.r(*callee);
            e.regs(args);
        }
        Term::TailCallMethod {
            class,
            method,
            dict,
            own,
            args,
            ..
        } => {
            let rp = tail_entry(e, start);
            e.op(TAIL_METHOD, 0);
            e.w(rp);
            e.w(class.0);
            e.w(*method);
            e.r(*dict);
            e.regs(own);
            e.regs(args);
        }
        Term::Error { kind, operand } => {
            e.errors.push(kind.clone());
            e.op(ERROR, (e.errors.len() - 1) as u32);
            match operand {
                Some(r) => e.r(*r),
                None => e.w(u32::MAX),
            }
        }
        Term::Unimplemented { what } => {
            e.texts.push(what.clone());
            e.op(UNIMPLEMENTED, (e.texts.len() - 1) as u32);
        }
        Term::Fx { op, args } => {
            e.fx_terms.push(op.clone());
            e.op(FX_TERM, (e.fx_terms.len() - 1) as u32);
            e.regs(args);
        }
    }
    Ok(())
}

/// 命令の語の列を読みやすく書く(`keleut dump bytecode` 用)。
pub fn disasm(code: &Code) -> String {
    let mut s = format!(
        "(code #{} (any {}) (raw {}) (words {}) (resume {}))\n",
        code.func.0,
        code.any_slots,
        code.raw_slots,
        code.words.len(),
        code.resume.len()
    );
    for (i, r) in code.resume.iter().enumerate() {
        s.push_str(&format!(
            "  resume {i}: call {} after {} dst {} kind {}{}\n",
            r.call_pc,
            r.pc_after as i64,
            r.dst as i64,
            r.kind,
            if r.yields { "" } else { " noyield" }
        ));
    }
    let mut pc = 0;
    let w = &code.words;
    while pc < w.len() {
        let op = (w[pc] & 0xff) as u8;
        let len = inst_len(w, pc);
        let sub = (w[pc] >> 8) as usize;
        let name = match op {
            op::FX => code.fx.get(sub).map_or("fx", |x| x.name()),
            op::FX_TERM => code.fx_terms.get(sub).map_or("fx_term", |x| x.name()),
            _ => op_name(op),
        };
        s.push_str(&format!(
            "  {pc:5}: {} sub={} {:?}\n",
            name,
            w[pc] >> 8,
            &w[pc + 1..pc + len]
        ));
        pc += len;
    }
    s
}

/// 位置 `pc` の命令の語の数。
pub fn inst_len(w: &[u32], pc: usize) -> usize {
    use op::*;
    let op = (w[pc] & 0xff) as u8;
    let at = |k: usize| w[pc + k] as usize;
    match op {
        DEPTH_ENTER | DEPTH_CHECK | DEPTH_PUSH | DEPTH_RESTORE | UNIMPL_VALUE | UNIMPLEMENTED => 1,
        GLOBAL_MARK_INIT | INST_MARK_INIT | JUMP | RETURN | ERROR => 2,
        CONST => 4,
        MOVE | BOX_BOOL | BOX_I32 | BOX_I64 | BOX_F64 | UNBOX_BOOL | UNBOX_I32 | UNBOX_I64
        | UNBOX_F64 | ENV_GET | I32_NEG | I64_NEG | F64_NEG | F64_MATH | F64_TRUNC | F64_ISNAN
        | CONV | BOOL_NOT | STR_LEN | SHOW | CTOR_TAG | VARIANT_TAG | VARIANT_PAYLOAD
        | GLOBAL_GET | GLOBAL_IS_INIT | GLOBAL_SET | INST_IS_INIT | REF_NEW | REF_GET | REF_SET
        | ARRAY_LEN | MARRAY_FREEZE | CONSOLE_WRITE => 3,
        I32_ARITH | I64_ARITH | I32_CMP | I64_CMP | F64_ARITH | F64_CMP | BOOL_EQ | STR_CONCAT
        | STR_CMP | CTOR_FIELD | VARIANT_NEW | REC_GET | REC_RESTRICT | CLOSURE_SET_CAP
        | INST_SET_VALUE | INST_GET_VALUE | ARRAY_GET | MARRAY_NEW | MARRAY_SET | BRANCH => 4,
        STR_SUB | REC_EXTEND | REC_FIELD | REC_HAS | REC_UPDATE => 5,
        CTOR_NEW => 5 + at(4),
        REC_NEW | CLOSURE_NEW | DICT_INST => 4 + at(3),
        DICT_RECORD | DICT_VARIANT => 3 + 2 * at(2),
        METHOD_NEW => 6 + at(5),
        CALL_DIRECT | CALL_VALUE | DICT_APP => 4 + at(3),
        CALL_METHOD | TAIL_METHOD => {
            let no = at(5);
            7 + no + at(6 + no)
        }
        TAIL_DIRECT => 3 + at(2),
        TAIL_VALUE => 4 + at(3),
        FX => 4 + at(3),
        FX_TERM => 2 + at(1),
        _ => 1,
    }
}

fn op_name(op: u8) -> &'static str {
    use op::*;
    match op {
        CONST => "const",
        MOVE => "move",
        BOX_BOOL => "box.bool",
        BOX_I32 => "box.i32",
        BOX_I64 => "box.i64",
        BOX_F64 => "box.f64",
        UNBOX_BOOL => "unbox.bool",
        UNBOX_I32 => "unbox.i32",
        UNBOX_I64 => "unbox.i64",
        UNBOX_F64 => "unbox.f64",
        ENV_GET => "env_get",
        DEPTH_ENTER => "depth_enter",
        DEPTH_CHECK => "depth_check",
        DEPTH_PUSH => "depth_push",
        DEPTH_RESTORE => "depth_restore",
        I32_ARITH => "i32.arith",
        I64_ARITH => "i64.arith",
        I32_NEG => "i32.neg",
        I64_NEG => "i64.neg",
        I32_CMP => "i32.cmp",
        I64_CMP => "i64.cmp",
        F64_ARITH => "f64.arith",
        F64_NEG => "f64.neg",
        F64_CMP => "f64.cmp",
        F64_MATH => "f64.math",
        F64_TRUNC => "f64.trunc",
        F64_ISNAN => "f64.is_nan",
        CONV => "conv",
        BOOL_NOT => "bool.not",
        BOOL_EQ => "bool.eq",
        STR_CONCAT => "str.concat",
        STR_CMP => "str.cmp",
        STR_LEN => "str.len",
        STR_SUB => "str.sub",
        SHOW => "show",
        CTOR_NEW => "ctor_new",
        CTOR_TAG => "ctor_tag",
        CTOR_FIELD => "ctor_field",
        VARIANT_NEW => "variant_new",
        VARIANT_TAG => "variant_tag",
        VARIANT_PAYLOAD => "variant_payload",
        REC_NEW => "rec_new",
        REC_GET => "rec_get",
        REC_EXTEND => "rec_extend",
        REC_FIELD => "rec_field",
        REC_HAS => "rec_has",
        REC_RESTRICT => "rec_restrict",
        REC_UPDATE => "rec_update",
        CLOSURE_NEW => "closure_new",
        CLOSURE_SET_CAP => "closure_set_cap",
        METHOD_NEW => "method_new",
        DICT_INST => "dict_inst",
        DICT_RECORD => "dict_record",
        DICT_VARIANT => "dict_variant",
        GLOBAL_GET => "global_get",
        GLOBAL_IS_INIT => "global_is_init",
        GLOBAL_SET => "global_set",
        GLOBAL_MARK_INIT => "global_mark_init",
        INST_IS_INIT => "inst_is_init",
        INST_MARK_INIT => "inst_mark_init",
        INST_SET_VALUE => "inst_set_value",
        INST_GET_VALUE => "inst_get_value",
        REF_NEW => "ref_new",
        REF_GET => "ref_get",
        REF_SET => "ref_set",
        ARRAY_LEN => "array_len",
        ARRAY_GET => "array_get",
        MARRAY_NEW => "marray_new",
        MARRAY_SET => "marray_set",
        MARRAY_FREEZE => "marray_freeze",
        CALL_DIRECT => "call_direct",
        CALL_VALUE => "call_value",
        CALL_METHOD => "call_method",
        DICT_APP => "dict_app",
        CONSOLE_WRITE => "console_write",
        UNIMPL_VALUE => "unimplemented_value",
        JUMP => "jump",
        BRANCH => "branch",
        RETURN => "return",
        TAIL_DIRECT => "tail_call_direct",
        TAIL_VALUE => "tail_call_value",
        TAIL_METHOD => "tail_call_method",
        ERROR => "error",
        UNIMPLEMENTED => "unimplemented",
        _ => "?",
    }
}
