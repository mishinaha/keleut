//! KIR のバイナリ(`.kir`)の読み書き。
//!
//! 規範は IRSpec の 13 節である。
//! Diktor の第20章(`ir_bin.ml`)とバイト単位で同じに読み書きする。
//! 読み込みの検査の順と誤りの文面も、Diktor の読み手に合わせてある。
//!
//! 読み込みは、どの入力に対しても誤りを値で返し、落ちない。
//! 再帰は式、辞書、パターンのノードの深さで数え、深さが限度を超えたらそこで拒否する。
//! 個数と長さは、使う前に節の残りのバイト数と比べる。

use std::collections::{BTreeSet, HashMap};

use crate::error::{Error, ErrorKind, READ_LIMIT_MESSAGE, WRITE_LIMIT_MESSAGE};
use crate::model::*;

/// マジック(`\0KIR`)。
pub const MAGIC: [u8; 4] = [0x00, 0x4B, 0x49, 0x52];

const SEC_STRINGS: u8 = 0x01;
const SEC_BLOBS: u8 = 0x02;
const SEC_UNITS: u8 = 0x03;
const SEC_TYPES: u8 = 0x04;
const SEC_EFFECTS: u8 = 0x05;
const SEC_CLASSES: u8 = 0x06;
const SEC_INSTANCES: u8 = 0x07;
const SEC_PRIMS: u8 = 0x08;
const SEC_GLOBALS: u8 = 0x09;
const SEC_FUNCTIONS: u8 = 0x0A;
const SEC_INIT: u8 = 0x0B;

// ---------------------------------------------------------------------------
// 読み込み
// ---------------------------------------------------------------------------

/// 読み込みの途中の誤り。位置は、誤りに気づいたときのファイルの先頭からのバイト数である。
struct Fail {
    pos: usize,
    msg: String,
    limit: bool,
}

type R<T> = Result<T, Fail>;

struct Rd<'a> {
    src: &'a [u8],
    pos: usize,
    stop: usize,
}

impl Rd<'_> {
    fn fail<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(Fail {
            pos: self.pos,
            msg: msg.into(),
            limit: false,
        })
    }

    fn u8(&mut self) -> R<u8> {
        if self.pos >= self.stop {
            return self.fail("節の終わりを越えて読もうとしました");
        }
        let c = self.src[self.pos];
        self.pos += 1;
        Ok(c)
    }

    /// uLEB128 の `u32`。5 バイト目は第 4〜7 ビットが 0 でなければならない。
    fn uleb(&mut self) -> R<u32> {
        let mut acc: u64 = 0;
        let mut shift = 0u32;
        let mut i = 0;
        loop {
            let byte = self.u8()?;
            if i == 4 && byte & 0xf0 != 0 {
                return self.fail("uLEB128 が u32 の範囲を超えます");
            }
            acc |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(acc as u32);
            }
            shift += 7;
            i += 1;
        }
    }

    /// sLEB128 の `i32`。5 バイト目は続きの印を持たず、第 4〜6 ビットが第 3 ビットと同じである。
    fn sleb32(&mut self) -> R<i32> {
        let mut acc: i64 = 0;
        let mut shift = 0u32;
        let mut i = 0;
        loop {
            let byte = self.u8()?;
            if i == 4 && byte & 0x80 != 0 {
                return self.fail("sLEB128 が長すぎます");
            }
            if i == 4 && byte & 0x70 != (if byte & 0x08 != 0 { 0x70 } else { 0 }) {
                return self.fail("sLEB128 が i32 の範囲を超えます");
            }
            acc |= i64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 != 0 {
                i += 1;
                continue;
            }
            if byte & 0x40 != 0 {
                acc |= -1i64 << shift;
            }
            return Ok(acc as i32);
        }
    }

    /// sLEB128 の `i64`。10 バイト目は 0x00 か 0x7F である。
    fn sleb64(&mut self) -> R<i64> {
        let mut acc: i64 = 0;
        let mut shift = 0u32;
        let mut i = 0;
        loop {
            let byte = self.u8()?;
            if i == 9 && byte != 0 && byte != 0x7f {
                return self.fail("sLEB128 が i64 の範囲を超えます");
            }
            acc |= i64::from(byte & 0x7f).wrapping_shl(shift);
            shift += 7;
            if byte & 0x80 != 0 {
                i += 1;
                continue;
            }
            if byte & 0x40 != 0 && shift < 64 {
                acc |= -1i64 << shift;
            }
            return Ok(acc);
        }
    }

    fn f64(&mut self) -> R<u64> {
        let mut bits = 0u64;
        for i in 0..8 {
            bits |= u64::from(self.u8()?) << (8 * i);
        }
        Ok(bits)
    }

    fn bytes(&mut self) -> R<Vec<u8>> {
        let n = self.uleb()? as usize;
        if n > self.stop - self.pos {
            return self.fail("バイト列の長さが節の残りを超えます");
        }
        let s = self.src[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(s)
    }

    /// 個数。どの項目も 1 バイト以上を占めるので、節の残りのバイト数を超える個数は誤りである。
    fn count(&mut self) -> R<usize> {
        let n = self.uleb()? as usize;
        if n > self.stop - self.pos {
            return self.fail("個数が節の残りのバイト数を超えます");
        }
        Ok(n)
    }

    fn bool(&mut self, what: &str) -> R<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => self.fail(format!("{what}の値が 0 でも 1 でもありません")),
        }
    }

    fn enum_code(&mut self, what: &str, n: usize) -> R<usize> {
        let c = self.u8()?;
        if usize::from(c) >= n {
            return self.fail(format!("{what}の番号 {c} は定義されていません"));
        }
        Ok(usize::from(c))
    }

    fn flags(&mut self, what: &str, mask: u8) -> R<u8> {
        let c = self.u8()?;
        if c & !mask != 0 {
            return self.fail(format!("{what}の印に定義されていないビットがあります"));
        }
        Ok(c)
    }

    fn idx(&self, what: &str, n: usize, i: u32) -> R<u32> {
        if i as usize >= n {
            return self.fail(format!("{what}の添字 {i} が範囲外です(表の大きさは {n})"));
        }
        Ok(i)
    }

    fn rep(&mut self) -> R<Rep> {
        Ok(Rep::ALL[self.enum_code("Rep", 6)?])
    }
}

/// 読み込みの途中の表。参照の範囲の検査と、strings と blobs の使用の印に使う。
#[derive(Default)]
struct Tables {
    strs: Vec<String>,
    str_used: Vec<bool>,
    blobs: Vec<Vec<u8>>,
    blob_used: Vec<bool>,
    ctors: Vec<usize>,
    ops: Vec<usize>,
    methods: Vec<usize>,
    n_instances: usize,
    n_globals: usize,
    n_fns: usize,
}

struct Dec<'a> {
    r: Rd<'a>,
    t: Tables,
    nodes: Nodes,
    lim: usize,
}

impl Dec<'_> {
    fn str(&mut self) -> R<String> {
        let i = self.r.uleb()?;
        let i = self.r.idx("strings", self.t.strs.len(), i)? as usize;
        self.t.str_used[i] = true;
        Ok(self.t.strs[i].clone())
    }

    fn blob(&mut self) -> R<Vec<u8>> {
        let i = self.r.uleb()?;
        let i = self.r.idx("blobs", self.t.blobs.len(), i)? as usize;
        self.t.blob_used[i] = true;
        Ok(self.t.blobs[i].clone())
    }

    fn reference(&mut self, what: &str, n: usize) -> R<u32> {
        let i = self.r.uleb()?;
        self.r.idx(what, n, i)
    }

    fn ty(&mut self) -> R<TypeId> {
        Ok(TypeId(self.reference("型", self.t.ctors.len())?))
    }

    fn eff(&mut self) -> R<EffectId> {
        Ok(EffectId(self.reference("エフェクト", self.t.ops.len())?))
    }

    fn cls(&mut self) -> R<ClassId> {
        Ok(ClassId(self.reference("クラス", self.t.methods.len())?))
    }

    fn ctor(&mut self, ty: TypeId) -> R<u32> {
        self.reference("コンストラクタ", self.t.ctors[ty.index()])
    }

    fn op(&mut self, e: EffectId) -> R<u32> {
        self.reference("操作", self.t.ops[e.index()])
    }

    fn meth(&mut self, c: ClassId) -> R<u32> {
        self.reference("メソッド", self.t.methods[c.index()])
    }

    fn func(&mut self) -> R<FnId> {
        Ok(FnId(self.reference("関数", self.t.n_fns)?))
    }

    fn glob(&mut self) -> R<GlobalId> {
        Ok(GlobalId(self.reference("大域", self.t.n_globals)?))
    }

    fn inst(&mut self) -> R<InstanceId> {
        Ok(InstanceId(
            self.reference("インスタンス", self.t.n_instances)?,
        ))
    }

    fn nest(&self, d: usize) -> R<()> {
        if d > self.lim {
            return Err(Fail {
                pos: self.r.pos,
                msg: READ_LIMIT_MESSAGE.to_string(),
                limit: true,
            });
        }
        Ok(())
    }

    fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let n = self.r.count()?;
        let mut v = Vec::new();
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }

    fn times<T>(&mut self, n: usize, mut f: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let mut v = Vec::new();
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }

    fn lit(&mut self) -> R<Lit> {
        match self.r.u8()? {
            0 => Ok(Lit::Bool(false)),
            1 => Ok(Lit::Bool(true)),
            2 => Ok(Lit::I32(self.r.sleb32()?)),
            3 => Ok(Lit::I64(self.r.sleb64()?)),
            4 => Ok(Lit::F64(self.r.f64()?)),
            5 => Ok(Lit::Str(self.blob()?)),
            c => self
                .r
                .fail(format!("リテラルの種類 {c} は定義されていません")),
        }
    }

    fn cap(&mut self) -> R<Cap> {
        match self.r.u8()? {
            0 => Ok(Cap::Local(self.r.uleb()?)),
            1 => Ok(Cap::Env(self.r.uleb()?)),
            2 => Ok(Cap::DParam(self.r.uleb()?)),
            c => self.r.fail(format!("捕獲の種類 {c} は定義されていません")),
        }
    }

    fn part(&mut self) -> R<Part> {
        let func = self.func()?;
        let caps = self.list(|s| s.cap())?;
        Ok(Part { func, caps })
    }

    fn dict(&mut self, d: usize) -> R<DictId> {
        self.nest(d)?;
        let x = match self.r.u8()? {
            0 => {
                let instance = self.inst()?;
                let premises = self.list(|s| s.dict(d + 1))?;
                Dict::Inst { instance, premises }
            }
            1 => Dict::DParam(self.r.uleb()?),
            2 => Dict::Env(self.r.uleb()?),
            c @ (3 | 4) => {
                let fs = self.list(|s| {
                    let l = s.str()?;
                    Ok((l, s.dict(d + 1)?))
                })?;
                if c == 3 {
                    Dict::Record(fs)
                } else {
                    Dict::Variant(fs)
                }
            }
            5 => {
                let class = self.cls()?;
                let ty = self.ty()?;
                Dict::Builtin { class, ty }
            }
            c => {
                return self
                    .r
                    .fail(format!("辞書の式の番号 {c} は定義されていません"));
            }
        };
        Ok(self.nodes.push_dict(x))
    }

    fn pat(&mut self, d: usize) -> R<PatId> {
        self.nest(d)?;
        let x = match self.r.u8()? {
            0 => Pat::Wild,
            1 => Pat::Bind(self.r.uleb()?),
            2 => Pat::Lit(self.lit()?),
            3 => {
                let tail = self.r.u8()?;
                if tail > 2 {
                    return self.r.fail(format!(
                        "レコードのパターンの尾部の種類 {tail} は定義されていません"
                    ));
                }
                let fields = self.list(|s| {
                    let l = s.str()?;
                    Ok((l, s.pat(d + 1)?))
                })?;
                let tail = match tail {
                    0 => RecordTail::Closed,
                    1 => RecordTail::Open,
                    _ => RecordTail::Rest(self.pat(d + 1)?),
                };
                Pat::Record { fields, tail }
            }
            4 => {
                let ty = self.ty()?;
                let ctor = self.ctor(ty)?;
                let fields = self.list(|s| {
                    let i = s.r.uleb()?;
                    Ok((i, s.pat(d + 1)?))
                })?;
                Pat::Ctor { ty, ctor, fields }
            }
            5 => {
                let label = self.str()?;
                let pat = self.pat(d + 1)?;
                Pat::Variant { label, pat }
            }
            c => {
                return self
                    .r
                    .fail(format!("パターンの番号 {c} は定義されていません"));
            }
        };
        Ok(self.nodes.push_pat(x))
    }

    /// 式のノードを読む。
    ///
    /// 再帰の 1 段のスタックを小さく保つため、構文ごとの読み方を別の関数に分け、
    /// ここでは番号から関数を選んで呼ぶだけにする(最適化なしで組んだときの大きな 1 枚のフレームを避ける)。
    fn exp(&mut self, d: usize) -> R<ExpId> {
        self.nest(d)?;
        let tag = self.r.u8()?;
        let arm: fn(&mut Self, usize, u8) -> R<Exp> = match tag {
            0x00..=0x04 | 0x15 | 0x1D => Self::exp_leaf,
            0x05 => Self::exp_construct,
            0x06 | 0x0A | 0x17 | 0x18 | 0x1C => Self::exp_unary,
            0x07 => Self::exp_call,
            0x08 | 0x09 => Self::exp_method,
            0x0B | 0x0C | 0x10 | 0x16 | 0x19 => Self::exp_binary,
            0x0D | 0x0E => Self::exp_part,
            0x0F => Self::exp_dictapp,
            0x11 => Self::exp_letpat,
            0x12 => Self::exp_letrec,
            0x13 => Self::exp_seq,
            0x14 => Self::exp_match,
            0x1A => Self::exp_perform,
            0x1B => Self::exp_handle,
            c => {
                return self
                    .r
                    .fail(format!("式の番号 0x{c:02x} は定義されていません"));
            }
        };
        let x = arm(self, d + 1, tag)?;
        Ok(self.nodes.push_exp(x))
    }

    fn exp_leaf(&mut self, _n: usize, tag: u8) -> R<Exp> {
        Ok(match tag {
            0x00 => Exp::Lit(self.lit()?),
            0x01 => {
                if self.r.u8()? != 0 {
                    return self.r.fail("範囲外のリテラルの種類が定義されていません");
                }
                Exp::LitError(self.str()?)
            }
            0x02 => Exp::Local(self.r.uleb()?),
            0x03 => Exp::Env(self.r.uleb()?),
            0x04 => Exp::Global(self.glob()?),
            0x15 => Exp::RecEmpty,
            _ => Exp::Hole,
        })
    }

    fn exp_construct(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let ty = self.ty()?;
        let ctor = self.ctor(ty)?;
        let count = self.r.count()?;
        let mut fields = Vec::new();
        for _ in 0..count {
            let i = self.r.uleb()?;
            fields.push((i, self.exp(n)?));
        }
        Ok(Exp::Construct { ty, ctor, fields })
    }

    fn exp_unary(&mut self, n: usize, tag: u8) -> R<Exp> {
        Ok(match tag {
            0x06 => {
                let label = self.str()?;
                Exp::Variant {
                    label,
                    value: self.exp(n)?,
                }
            }
            0x0A => Exp::Not(self.exp(n)?),
            0x17 => {
                let label = self.str()?;
                Exp::RecSelect {
                    label,
                    record: self.exp(n)?,
                }
            }
            0x18 => {
                let label = self.str()?;
                Exp::RecRestrict {
                    label,
                    record: self.exp(n)?,
                }
            }
            _ => {
                let tail = self.r.bool("resume の末尾の印")?;
                Exp::Resume {
                    tail,
                    value: self.exp(n)?,
                }
            }
        })
    }

    fn exp_call(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let flags = self.r.flags("呼び出し", 3)?;
        let count = self.r.count()?;
        let func = self.exp(n)?;
        let mut args = Vec::new();
        for _ in 0..count {
            args.push(self.exp(n)?);
        }
        Ok(Exp::Call {
            tail: flags & 1 != 0,
            yields: flags & 2 != 0,
            func,
            args,
        })
    }

    fn exp_method(&mut self, n: usize, tag: u8) -> R<Exp> {
        let class = self.cls()?;
        let method = self.meth(class)?;
        let count = self.r.count()?;
        let dict = self.dict(n)?;
        if tag == 0x08 {
            let mut own = Vec::new();
            for _ in 0..count {
                own.push(self.dict(n)?);
            }
            Ok(Exp::Method {
                class,
                method,
                dict,
                own,
            })
        } else {
            let mut args = Vec::new();
            for _ in 0..count {
                args.push(self.exp(n)?);
            }
            Ok(Exp::Opcall {
                class,
                method,
                dict,
                args,
            })
        }
    }

    fn exp_binary(&mut self, n: usize, tag: u8) -> R<Exp> {
        let label = if tag == 0x16 || tag == 0x19 {
            self.str()?
        } else {
            String::new()
        };
        let slot = if tag == 0x10 { self.r.uleb()? } else { 0 };
        let a = self.exp(n)?;
        let b = self.exp(n)?;
        Ok(match tag {
            0x0B => Exp::And(a, b),
            0x0C => Exp::Or(a, b),
            0x10 => Exp::Let {
                slot,
                value: a,
                body: b,
            },
            0x16 => Exp::RecExtend {
                label,
                value: a,
                record: b,
            },
            _ => Exp::RecUpdate {
                label,
                record: a,
                value: b,
            },
        })
    }

    fn exp_part(&mut self, _n: usize, tag: u8) -> R<Exp> {
        let pt = self.part()?;
        Ok(if tag == 0x0D {
            Exp::Closure(pt)
        } else {
            Exp::Dictabs(pt)
        })
    }

    fn exp_dictapp(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let count = self.r.count()?;
        let func = self.exp(n)?;
        let mut dicts = Vec::new();
        for _ in 0..count {
            dicts.push(self.dict(n)?);
        }
        Ok(Exp::Dictapp { func, dicts })
    }

    fn exp_letpat(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let pat = self.pat(n)?;
        let value = self.exp(n)?;
        let body = self.exp(n)?;
        Ok(Exp::Letpat { pat, value, body })
    }

    fn exp_letrec(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let count = self.r.count()?;
        let mut items = Vec::new();
        for _ in 0..count {
            let i = self.r.uleb()?;
            items.push((i, self.exp(n)?));
        }
        if items.is_empty() {
            return self.r.fail("letrec の項目がありません");
        }
        let body = self.exp(n)?;
        Ok(Exp::Letrec { items, body })
    }

    fn exp_seq(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let count = self.r.count()?;
        let mut xs = Vec::new();
        for _ in 0..count {
            xs.push(self.exp(n)?);
        }
        if xs.len() < 2 {
            return self.r.fail("seq の要素が 2 個より少なくなっています");
        }
        Ok(Exp::Seq(xs))
    }

    fn exp_match(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let count = self.r.count()?;
        let scrutinee = self.exp(n)?;
        let mut clauses = Vec::new();
        for _ in 0..count {
            let g = self.r.bool("ガードの有無")?;
            let pat = self.pat(n)?;
            let guard = if g { Some(self.exp(n)?) } else { None };
            let body = self.exp(n)?;
            clauses.push(Clause { pat, guard, body });
        }
        Ok(Exp::Match { scrutinee, clauses })
    }

    fn exp_perform(&mut self, n: usize, _tag: u8) -> R<Exp> {
        let effect = self.eff()?;
        let op = self.op(effect)?;
        let count = self.r.count()?;
        let mut args = Vec::new();
        for _ in 0..count {
            args.push(self.exp(n)?);
        }
        Ok(Exp::Perform { effect, op, args })
    }

    fn exp_handle(&mut self, _n: usize, _tag: u8) -> R<Exp> {
        let flags = self.r.flags("ハンドラ式", 7)?;
        let effect = self.eff()?;
        let n = self.r.count()?;
        if n == 0 {
            return self.r.fail("ハンドラ式の操作節がありません");
        }
        let body = self.part()?;
        let ops = self.times(n, |s| {
            let op = s.op(effect)?;
            let class = ClauseClass::ALL[s.r.enum_code("操作節の分類", 3)?];
            let part = s.part()?;
            Ok(HandlerOp { op, class, part })
        })?;
        let ret = if flags & 2 != 0 {
            Some(self.part()?)
        } else {
            None
        };
        let cancel = if flags & 4 != 0 {
            Some(self.part()?)
        } else {
            None
        };
        Ok(Exp::Handle(Box::new(Handle {
            tail: flags & 1 != 0,
            effect,
            body,
            ops,
            ret,
            cancel,
        })))
    }

    fn fn_entry(&mut self, sym: String) -> R<Function> {
        let kind = FnKind::ALL[self.r.enum_code("関数の種類", 7)?];
        let yields = self.r.bool("yield の印")?;
        let params = self.list(|s| s.r.rep())?;
        let locals = self.list(|s| s.r.rep())?;
        let dicts = self.r.uleb()?;
        let caps = self.list(|s| s.r.rep())?;
        let result = self.r.rep()?;
        let (pats, guard) = if kind == FnKind::Opclause {
            let pats = self.list(|s| s.pat(1))?;
            let g = self.r.bool("ガードの有無")?;
            (pats, if g { Some(self.exp(1)?) } else { None })
        } else {
            (Vec::new(), None)
        };
        let body = self.exp(1)?;
        Ok(Function {
            sym,
            kind,
            yields,
            params,
            locals,
            dicts,
            caps,
            result,
            pats,
            guard,
            body,
        })
    }

    fn step(&mut self) -> R<Step> {
        match self.r.u8()? {
            0 => {
                let global = self.glob()?;
                let func = self.func()?;
                Ok(Step::DefValue { global, func })
            }
            1 => {
                let func = self.func()?;
                let globals = self.list(|s| s.glob())?;
                let pat = self.pat(1)?;
                Ok(Step::DefPattern { func, globals, pat })
            }
            2 => {
                let gs = self.list(|s| s.glob())?;
                if gs.is_empty() {
                    return self.r.fail("def_fns の大域がありません");
                }
                Ok(Step::DefFns(gs))
            }
            3 => Ok(Step::DefDictabs(self.glob()?)),
            4 => Ok(Step::DefInstance(self.inst()?)),
            5 => Ok(Step::RunExp(self.func()?)),
            c => self
                .r
                .fail(format!("初期化の手順の番号 {c} は定義されていません")),
        }
    }
}

fn check_sorted<T: Ord>(r: &Rd<'_>, what: &str, items: &[T]) -> R<()> {
    for w in items.windows(2) {
        if w[0] >= w[1] {
            return r.fail(format!("{what}の表が厳密な昇順に並んでいません"));
        }
    }
    Ok(())
}

/// バイナリを読む。`max_nest` はノードの深さの上限(IRSpec 13.9 節の `N_nest`)である。
///
/// 深さに比例してスタックを使うので、深い入力を読むときは大きなスタックの上で呼ぶ
/// ([`crate::read_kir`] はそうする)。
pub fn decode(src: &[u8], max_nest: usize) -> Result<Program, Error> {
    decode_inner(src, max_nest).map_err(|f| {
        Error::new(
            if f.limit {
                ErrorKind::Limit
            } else {
                ErrorKind::Malformed
            },
            format!("位置 {}: {}", f.pos, f.msg),
        )
    })
}

fn decode_inner(src: &[u8], lim: usize) -> R<Program> {
    let len = src.len();
    let mut r = Rd {
        src,
        pos: 0,
        stop: len,
    };
    if len < 16 {
        return r.fail("見出しの 16 バイトがありません");
    }
    if src[0..4] != MAGIC {
        return r.fail("マジックが KIR のものではありません");
    }
    r.pos = 4;
    let u16le = |r: &mut Rd<'_>| -> R<u32> {
        let a = u32::from(r.u8()?);
        Ok(a | (u32::from(r.u8()?) << 8))
    };
    let major = u16le(&mut r)?;
    let minor = u16le(&mut r)?;
    if major != u32::from(VERSION.0) {
        return r.fail(format!(
            "版 {major}.{minor} は読めません(読めるのは主版 {} の版だけです)",
            VERSION.0
        ));
    }
    let features = {
        let a = u16le(&mut r)?;
        a | (u16le(&mut r)? << 16)
    };
    if features != 0 {
        return r.fail(format!(
            "必要な機能のビット 0x{features:08x} を処理できません"
        ));
    }
    let nsec = {
        let a = u16le(&mut r)?;
        a | (u16le(&mut r)? << 16)
    };
    let mut secs: [Option<(usize, usize)>; 12] = [None; 12];
    let mut seen: u64 = 0;
    let mut last = 0u8;
    while r.pos < len {
        let id = r.u8()?;
        if id == 0 || (id > SEC_INIT && id < 0x80) {
            return r.fail(format!("節 0x{id:02x} は定義されていません"));
        }
        if id <= last {
            return r.fail(format!("節 0x{id:02x} が id の昇順に並んでいません"));
        }
        last = id;
        let size = r.uleb()? as usize;
        if size > len - r.pos {
            return r.fail(format!("節 0x{id:02x} の大きさがファイルの残りを超えます"));
        }
        if id <= SEC_INIT {
            secs[usize::from(id)] = Some((r.pos, r.pos + size));
        }
        r.pos += size;
        seen += 1;
    }
    if seen != u64::from(nsec) {
        return r.fail(format!("節の数が見出し({nsec})と違います({seen})"));
    }
    let mut bounds = [(0usize, 0usize); 12];
    for id in SEC_STRINGS..=SEC_INIT {
        match secs[usize::from(id)] {
            Some(b) => bounds[usize::from(id)] = b,
            None => return r.fail(format!("必須の節 0x{id:02x} がありません")),
        }
    }
    let mut dec = Dec {
        r,
        t: Tables::default(),
        nodes: Nodes::default(),
        lim,
    };
    let enter = |dec: &mut Dec<'_>, id: u8| {
        let (a, b) = bounds[usize::from(id)];
        dec.r.pos = a;
        dec.r.stop = b;
    };
    let leave = |dec: &Dec<'_>, id: u8| -> R<()> {
        if dec.r.pos != dec.r.stop {
            return dec
                .r
                .fail(format!("節 0x{id:02x} の中身の後ろにバイトが残っています"));
        }
        Ok(())
    };
    // strings と blobs。Diktor と同じく、並びの検査を残りのバイトの検査より先に行う
    enter(&mut dec, SEC_STRINGS);
    let strs = dec.list(|s| s.r.bytes())?;
    check_sorted(&dec.r, "strings", &strs)?;
    leave(&dec, SEC_STRINGS)?;
    let mut names = Vec::with_capacity(strs.len());
    for s in strs {
        match String::from_utf8(s) {
            Ok(s) => names.push(s),
            Err(_) => return dec.r.fail("strings の表に UTF-8 でない名前があります"),
        }
    }
    enter(&mut dec, SEC_BLOBS);
    let blobs = dec.list(|s| s.r.bytes())?;
    check_sorted(&dec.r, "blobs", &blobs)?;
    leave(&dec, SEC_BLOBS)?;
    dec.t.str_used = vec![false; names.len()];
    dec.t.strs = names;
    dec.t.blob_used = vec![false; blobs.len()];
    dec.t.blobs = blobs;
    // 関数の記号の列。項目は後で同じ節の続きから読む
    enter(&mut dec, SEC_FUNCTIONS);
    let nfn = dec.r.count()?;
    let fn_syms = dec.times(nfn, |s| s.str())?;
    dec.t.n_fns = fn_syms.len();
    let fn_items_pos = dec.r.pos;
    enter(&mut dec, SEC_UNITS);
    let units = dec.list(|s| {
        let k = s.str()?;
        Ok((k, s.list(|s| s.blob())?))
    })?;
    leave(&dec, SEC_UNITS)?;
    enter(&mut dec, SEC_TYPES);
    let types = dec.list(|s| {
        let sym = s.str()?;
        let ctors = s.list(|s| {
            let name = s.str()?;
            Ok(Ctor {
                name,
                arity: s.r.uleb()?,
            })
        })?;
        Ok(Type { sym, ctors })
    })?;
    leave(&dec, SEC_TYPES)?;
    dec.t.ctors = types.iter().map(|t| t.ctors.len()).collect();
    enter(&mut dec, SEC_EFFECTS);
    let effects = dec.list(|s| {
        let sym = s.str()?;
        let ops = s.list(|s| {
            let name = s.str()?;
            let arity = s.r.uleb()?;
            let fl = s.r.flags("操作", 3)?;
            Ok(Op {
                name,
                arity,
                never: fl & 1 != 0,
                runtime: fl & 2 != 0,
            })
        })?;
        Ok(Effect { sym, ops })
    })?;
    leave(&dec, SEC_EFFECTS)?;
    dec.t.ops = effects.iter().map(|e| e.ops.len()).collect();
    enter(&mut dec, SEC_CLASSES);
    let classes = dec.list(|s| {
        let sym = s.str()?;
        let methods = s.list(|s| {
            let name = s.str()?;
            Ok(ClassMethod {
                name,
                dicts: s.r.uleb()?,
            })
        })?;
        Ok(Class { sym, methods })
    })?;
    leave(&dec, SEC_CLASSES)?;
    dec.t.methods = classes.iter().map(|c| c.methods.len()).collect();
    enter(&mut dec, SEC_INSTANCES);
    let instances = dec.list(|s| {
        let sym = s.str()?;
        let class = s.cls()?;
        let head = s.ty()?;
        let premises = s.r.uleb()?;
        let methods = s.list(|s| {
            let method = s.meth(class)?;
            let dicts = s.r.uleb()?;
            let func = s.func()?;
            Ok(Impl {
                method,
                dicts,
                func,
            })
        })?;
        Ok(Instance {
            sym,
            class,
            head,
            premises,
            methods,
        })
    })?;
    leave(&dec, SEC_INSTANCES)?;
    dec.t.n_instances = instances.len();
    enter(&mut dec, SEC_PRIMS);
    let prims = dec.list(|s| {
        let a = Abi::ALL[s.r.enum_code("ABI", 2)?];
        Ok((a, s.str()?))
    })?;
    leave(&dec, SEC_PRIMS)?;
    check_sorted(&dec.r, "prims", &prims)?;
    let mut prim_used = vec![false; prims.len()];
    enter(&mut dec, SEC_GLOBALS);
    let globals = dec.list(|s| {
        let sym = s.str()?;
        let kind = match s.r.u8()? {
            0 => GlobalKind::Builtin,
            1 => {
                let i = s.r.uleb()?;
                let i = s.r.idx("prims", prims.len(), i)? as usize;
                prim_used[i] = true;
                let (abi, name) = prims[i].clone();
                GlobalKind::Extern {
                    abi,
                    name,
                    dicts: s.r.uleb()?,
                }
            }
            2 => GlobalKind::Fn(s.func()?),
            3 => GlobalKind::Dictabs(s.func()?),
            4 => GlobalKind::Value(s.r.rep()?),
            c => return s.r.fail(format!("大域の種類 {c} は定義されていません")),
        };
        Ok(Global { sym, kind })
    })?;
    leave(&dec, SEC_GLOBALS)?;
    if let Some(i) = prim_used.iter().position(|u| !u) {
        return dec.r.fail(format!(
            "prims の項目 {i} を参照する extern の大域がありません"
        ));
    }
    dec.t.n_globals = globals.len();
    enter(&mut dec, SEC_FUNCTIONS);
    dec.r.pos = fn_items_pos;
    let mut functions = Vec::with_capacity(fn_syms.len());
    for sym in fn_syms {
        functions.push(dec.fn_entry(sym)?);
    }
    leave(&dec, SEC_FUNCTIONS)?;
    enter(&mut dec, SEC_INIT);
    let inits = dec.list(|s| s.list(|s| s.step()))?;
    leave(&dec, SEC_INIT)?;
    if inits.len() != units.len() {
        return dec.r.fail("init の節の単位の数が units の節と違います");
    }
    if let Some(i) = dec.t.str_used.iter().position(|u| !u) {
        return dec
            .r
            .fail(format!("strings の項目 {i} がどこからも使われていません"));
    }
    if let Some(i) = dec.t.blob_used.iter().position(|u| !u) {
        return dec
            .r
            .fail(format!("blobs の項目 {i} がどこからも使われていません"));
    }
    let units = units
        .into_iter()
        .zip(inits)
        .map(|((key, paths), init)| Unit { key, paths, init })
        .collect();
    Ok(Program {
        version: (major as u16, minor as u16),
        units,
        types,
        effects,
        classes,
        instances,
        globals,
        functions,
        nodes: dec.nodes,
    })
}

// ---------------------------------------------------------------------------
// 書き出し
// ---------------------------------------------------------------------------

type W<T> = Result<T, Error>;

fn internal<T>(msg: impl Into<String>) -> W<T> {
    Err(Error::new(
        ErrorKind::Internal,
        format!("KIR を符号化できません: {}", msg.into()),
    ))
}

/// strings と blobs の表を集める。深さが限度を超えたら書き出しの限度の誤りにする。
struct Collect<'a> {
    p: &'a Program,
    strs: BTreeSet<&'a str>,
    blobs: BTreeSet<&'a [u8]>,
    lim: usize,
}

impl<'a> Collect<'a> {
    fn nest(&self, d: usize) -> W<()> {
        if d > self.lim {
            return Err(Error::new(ErrorKind::WriteLimit, WRITE_LIMIT_MESSAGE));
        }
        Ok(())
    }

    fn lit(&mut self, l: &'a Lit) {
        if let Lit::Str(s) = l {
            self.blobs.insert(s);
        }
    }

    fn dict(&mut self, id: DictId, d: usize) -> W<()> {
        self.nest(d)?;
        let Some(x) = self.p.nodes.dict(id) else {
            return internal(format!("辞書のノード {} がありません", id.0));
        };
        match x {
            Dict::Inst { premises, .. } => {
                for &q in premises {
                    self.dict(q, d + 1)?;
                }
            }
            Dict::Record(fs) | Dict::Variant(fs) => {
                for (l, q) in fs {
                    self.strs.insert(l);
                    self.dict(*q, d + 1)?;
                }
            }
            Dict::DParam(_) | Dict::Env(_) | Dict::Builtin { .. } => {}
        }
        Ok(())
    }

    fn pat(&mut self, id: PatId, d: usize) -> W<()> {
        self.nest(d)?;
        let Some(x) = self.p.nodes.pat(id) else {
            return internal(format!("パターンのノード {} がありません", id.0));
        };
        match x {
            Pat::Wild | Pat::Bind(_) => {}
            Pat::Lit(l) => self.lit(l),
            Pat::Record { fields, tail } => {
                for (l, q) in fields {
                    self.strs.insert(l);
                    self.pat(*q, d + 1)?;
                }
                if let RecordTail::Rest(q) = tail {
                    self.pat(*q, d + 1)?;
                }
            }
            Pat::Ctor { fields, .. } => {
                for (_, q) in fields {
                    self.pat(*q, d + 1)?;
                }
            }
            Pat::Variant { label, pat } => {
                self.strs.insert(label);
                self.pat(*pat, d + 1)?;
            }
        }
        Ok(())
    }

    /// 式の表の項目を集める。再帰の 1 段のスタックを小さく保つため、構文を 2 つの群に分けて別の関数で集める。
    fn exp(&mut self, id: ExpId, d: usize) -> W<()> {
        self.nest(d)?;
        let Some(x) = self.p.nodes.exp(id) else {
            return internal(format!("式のノード {} がありません", id.0));
        };
        let arm: fn(&mut Self, &'a Exp, usize) -> W<()> = match x {
            Exp::Lit(_)
            | Exp::LitError(_)
            | Exp::Local(_)
            | Exp::Env(_)
            | Exp::Global(_)
            | Exp::Closure(_)
            | Exp::Dictabs(_)
            | Exp::RecEmpty
            | Exp::Handle(_)
            | Exp::Hole
            | Exp::Construct { .. }
            | Exp::Variant { .. }
            | Exp::RecSelect { .. }
            | Exp::RecRestrict { .. }
            | Exp::Call { .. }
            | Exp::Method { .. }
            | Exp::Opcall { .. } => Self::exp_g0,
            _ => Self::exp_g1,
        };
        arm(self, x, d + 1)
    }

    fn exp_g0(&mut self, x: &'a Exp, n: usize) -> W<()> {
        match x {
            Exp::Lit(l) => self.lit(l),
            Exp::LitError(s) => {
                self.strs.insert(s);
            }
            Exp::Local(_)
            | Exp::Env(_)
            | Exp::Global(_)
            | Exp::Closure(_)
            | Exp::Dictabs(_)
            | Exp::RecEmpty
            | Exp::Handle(_)
            | Exp::Hole => {}
            Exp::Construct { fields, .. } => {
                for (_, e) in fields {
                    self.exp(*e, n)?;
                }
            }
            Exp::Variant { label, value: e }
            | Exp::RecSelect { label, record: e }
            | Exp::RecRestrict { label, record: e } => {
                self.strs.insert(label);
                self.exp(*e, n)?;
            }
            Exp::Call { func, args, .. } => {
                self.exp(*func, n)?;
                for a in args {
                    self.exp(*a, n)?;
                }
            }
            Exp::Method { dict, own, .. } => {
                self.dict(*dict, n)?;
                for o in own {
                    self.dict(*o, n)?;
                }
            }
            Exp::Opcall { dict, args, .. } => {
                self.dict(*dict, n)?;
                for a in args {
                    self.exp(*a, n)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn exp_g1(&mut self, x: &'a Exp, n: usize) -> W<()> {
        match x {
            Exp::Not(a) | Exp::Resume { value: a, .. } => self.exp(*a, n)?,
            Exp::And(a, b) | Exp::Or(a, b) => {
                self.exp(*a, n)?;
                self.exp(*b, n)?;
            }
            Exp::Let { value, body, .. } => {
                self.exp(*value, n)?;
                self.exp(*body, n)?;
            }
            Exp::Dictapp { func, dicts } => {
                self.exp(*func, n)?;
                for q in dicts {
                    self.dict(*q, n)?;
                }
            }
            Exp::Letpat { pat, value, body } => {
                self.pat(*pat, n)?;
                self.exp(*value, n)?;
                self.exp(*body, n)?;
            }
            Exp::Letrec { items, body } => {
                for (_, e) in items {
                    self.exp(*e, n)?;
                }
                self.exp(*body, n)?;
            }
            Exp::Seq(xs) | Exp::Perform { args: xs, .. } => {
                for e in xs {
                    self.exp(*e, n)?;
                }
            }
            Exp::Match { scrutinee, clauses } => {
                self.exp(*scrutinee, n)?;
                for c in clauses {
                    self.pat(c.pat, n)?;
                    if let Some(g) = c.guard {
                        self.exp(g, n)?;
                    }
                    self.exp(c.body, n)?;
                }
            }
            Exp::RecExtend {
                label,
                value,
                record,
            } => {
                self.strs.insert(label);
                self.exp(*value, n)?;
                self.exp(*record, n)?;
            }
            Exp::RecUpdate {
                label,
                record,
                value,
            } => {
                self.strs.insert(label);
                self.exp(*record, n)?;
                self.exp(*value, n)?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct Enc<'a> {
    p: &'a Program,
    b: Vec<u8>,
    str: HashMap<&'a str, u32>,
    blob: HashMap<&'a [u8], u32>,
    prim: HashMap<(u8, &'a str), u32>,
}

fn put_uleb(b: &mut Vec<u8>, mut n: u64) {
    loop {
        if n < 0x80 {
            b.push(n as u8);
            return;
        }
        b.push((n & 0x7f) as u8 | 0x80);
        n >>= 7;
    }
}

fn put_sleb(b: &mut Vec<u8>, mut n: i64) {
    loop {
        let byte = (n & 0x7f) as u8;
        let rest = n >> 7;
        if (rest == 0 && byte & 0x40 == 0) || (rest == -1 && byte & 0x40 != 0) {
            b.push(byte);
            return;
        }
        b.push(byte | 0x80);
        n = rest;
    }
}

fn put_bytes(b: &mut Vec<u8>, s: &[u8]) {
    put_uleb(b, s.len() as u64);
    b.extend_from_slice(s);
}

fn check_ref(what: &str, i: u32, n: usize) -> W<u32> {
    if i as usize >= n {
        return internal(format!("{what}の添字 {i} が表の範囲の外です"));
    }
    Ok(i)
}

impl<'a> Enc<'a> {
    fn u8(&mut self, n: u8) {
        self.b.push(n);
    }

    fn uleb(&mut self, n: u32) {
        put_uleb(&mut self.b, u64::from(n));
    }

    fn len(&mut self, n: usize) -> W<()> {
        if n > u32::MAX as usize {
            return internal("列が u32 の範囲を超えます");
        }
        put_uleb(&mut self.b, n as u64);
        Ok(())
    }

    fn str(&mut self, s: &str) -> W<()> {
        match self.str.get(s) {
            Some(&i) => {
                self.uleb(i);
                Ok(())
            }
            None => internal(format!("名前が表にありません: {s}")),
        }
    }

    fn blob(&mut self, s: &[u8]) -> W<()> {
        match self.blob.get(s) {
            Some(&i) => {
                self.uleb(i);
                Ok(())
            }
            None => internal("文字列が表にありません"),
        }
    }

    fn ty(&mut self, t: TypeId) -> W<()> {
        let i = check_ref("型", t.0, self.p.types.len())?;
        self.uleb(i);
        Ok(())
    }

    fn ctor(&mut self, t: TypeId, c: u32) -> W<()> {
        let n = self.p.types.get(t.index()).map_or(0, |t| t.ctors.len());
        let i = check_ref("コンストラクタ", c, n)?;
        self.uleb(i);
        Ok(())
    }

    fn eff(&mut self, e: EffectId) -> W<()> {
        let i = check_ref("エフェクト", e.0, self.p.effects.len())?;
        self.uleb(i);
        Ok(())
    }

    fn op(&mut self, e: EffectId, o: u32) -> W<()> {
        let n = self.p.effects.get(e.index()).map_or(0, |e| e.ops.len());
        let i = check_ref("操作", o, n)?;
        self.uleb(i);
        Ok(())
    }

    fn cls(&mut self, c: ClassId) -> W<()> {
        let i = check_ref("クラス", c.0, self.p.classes.len())?;
        self.uleb(i);
        Ok(())
    }

    fn meth(&mut self, c: ClassId, m: u32) -> W<()> {
        let n = self.p.classes.get(c.index()).map_or(0, |c| c.methods.len());
        let i = check_ref("メソッド", m, n)?;
        self.uleb(i);
        Ok(())
    }

    fn inst(&mut self, x: InstanceId) -> W<()> {
        let i = check_ref("インスタンス", x.0, self.p.instances.len())?;
        self.uleb(i);
        Ok(())
    }

    fn glob(&mut self, g: GlobalId) -> W<()> {
        let i = check_ref("大域", g.0, self.p.globals.len())?;
        self.uleb(i);
        Ok(())
    }

    fn func(&mut self, f: FnId) -> W<()> {
        let i = check_ref("関数", f.0, self.p.functions.len())?;
        self.uleb(i);
        Ok(())
    }

    fn lit(&mut self, l: &Lit) -> W<()> {
        match l {
            Lit::Bool(false) => self.u8(0),
            Lit::Bool(true) => self.u8(1),
            Lit::I32(n) => {
                self.u8(2);
                put_sleb(&mut self.b, i64::from(*n));
            }
            Lit::I64(n) => {
                self.u8(3);
                put_sleb(&mut self.b, *n);
            }
            Lit::F64(bits) => {
                self.u8(4);
                self.b.extend_from_slice(&bits.to_le_bytes());
            }
            Lit::Str(s) => {
                self.u8(5);
                self.blob(s)?;
            }
        }
        Ok(())
    }

    fn cap(&mut self, c: Cap) {
        let (k, i) = match c {
            Cap::Local(i) => (0, i),
            Cap::Env(i) => (1, i),
            Cap::DParam(i) => (2, i),
        };
        self.u8(k);
        self.uleb(i);
    }

    fn part(&mut self, p: &Part) -> W<()> {
        self.func(p.func)?;
        self.len(p.caps.len())?;
        for &c in &p.caps {
            self.cap(c);
        }
        Ok(())
    }

    fn dict(&mut self, id: DictId) -> W<()> {
        let p = self.p;
        let Some(x) = p.nodes.dict(id) else {
            return internal("辞書のノードがありません");
        };
        match x {
            Dict::Inst { instance, premises } => {
                self.u8(0);
                self.inst(*instance)?;
                self.len(premises.len())?;
                for &q in premises {
                    self.dict(q)?;
                }
            }
            Dict::DParam(i) => {
                self.u8(1);
                self.uleb(*i);
            }
            Dict::Env(i) => {
                self.u8(2);
                self.uleb(*i);
            }
            Dict::Record(fs) | Dict::Variant(fs) => {
                self.u8(if matches!(x, Dict::Record(_)) { 3 } else { 4 });
                self.len(fs.len())?;
                for (l, q) in fs {
                    self.str(l)?;
                    self.dict(*q)?;
                }
            }
            Dict::Builtin { class, ty } => {
                self.u8(5);
                self.cls(*class)?;
                self.ty(*ty)?;
            }
        }
        Ok(())
    }

    fn pat(&mut self, id: PatId) -> W<()> {
        let p = self.p;
        let Some(x) = p.nodes.pat(id) else {
            return internal("パターンのノードがありません");
        };
        match x {
            Pat::Wild => self.u8(0),
            Pat::Bind(i) => {
                self.u8(1);
                self.uleb(*i);
            }
            Pat::Lit(l) => {
                self.u8(2);
                self.lit(l)?;
            }
            Pat::Record { fields, tail } => {
                self.u8(3);
                self.u8(match tail {
                    RecordTail::Closed => 0,
                    RecordTail::Open => 1,
                    RecordTail::Rest(_) => 2,
                });
                self.len(fields.len())?;
                for (l, q) in fields {
                    self.str(l)?;
                    self.pat(*q)?;
                }
                if let RecordTail::Rest(q) = tail {
                    self.pat(*q)?;
                }
            }
            Pat::Ctor { ty, ctor, fields } => {
                self.u8(4);
                self.ty(*ty)?;
                self.ctor(*ty, *ctor)?;
                self.len(fields.len())?;
                for (i, q) in fields {
                    self.uleb(*i);
                    self.pat(*q)?;
                }
            }
            Pat::Variant { label, pat } => {
                self.u8(5);
                self.str(label)?;
                self.pat(*pat)?;
            }
        }
        Ok(())
    }

    /// 式のノードを書く。再帰の 1 段のスタックを小さく保つため、構文を群に分けて別の関数で書く。
    fn exp(&mut self, id: ExpId) -> W<()> {
        let p = self.p;
        let Some(x) = p.nodes.exp(id) else {
            return internal("式のノードがありません");
        };
        let arm: fn(&mut Self, &'a Exp) -> W<()> = match x {
            Exp::Lit(_)
            | Exp::LitError(_)
            | Exp::Local(_)
            | Exp::Env(_)
            | Exp::Global(_)
            | Exp::Closure(_)
            | Exp::Dictabs(_)
            | Exp::RecEmpty
            | Exp::Handle(_)
            | Exp::Hole => Self::exp_g0,
            Exp::Construct { .. }
            | Exp::Variant { .. }
            | Exp::Not(_)
            | Exp::RecSelect { .. }
            | Exp::RecRestrict { .. }
            | Exp::Resume { .. } => Self::exp_g1,
            Exp::Call { .. } | Exp::Method { .. } | Exp::Opcall { .. } | Exp::Dictapp { .. } => {
                Self::exp_g2
            }
            Exp::And(..)
            | Exp::Or(..)
            | Exp::Let { .. }
            | Exp::Letpat { .. }
            | Exp::RecExtend { .. }
            | Exp::RecUpdate { .. } => Self::exp_g3,
            Exp::Letrec { .. } | Exp::Seq(_) | Exp::Match { .. } | Exp::Perform { .. } => {
                Self::exp_g4
            }
        };
        arm(self, x)
    }

    fn exp_g0(&mut self, x: &'a Exp) -> W<()> {
        match x {
            Exp::Lit(l) => {
                self.u8(0x00);
                self.lit(l)?;
            }
            Exp::LitError(s) => {
                self.u8(0x01);
                self.u8(0);
                self.str(s)?;
            }
            Exp::Local(i) => {
                self.u8(0x02);
                self.uleb(*i);
            }
            Exp::Env(i) => {
                self.u8(0x03);
                self.uleb(*i);
            }
            Exp::Global(g) => {
                self.u8(0x04);
                self.glob(*g)?;
            }
            Exp::Closure(pt) => {
                self.u8(0x0D);
                self.part(pt)?;
            }
            Exp::Dictabs(pt) => {
                self.u8(0x0E);
                self.part(pt)?;
            }
            Exp::RecEmpty => self.u8(0x15),
            Exp::Handle(h) => {
                self.u8(0x1B);
                self.u8(u8::from(h.tail)
                    | (u8::from(h.ret.is_some()) << 1)
                    | (u8::from(h.cancel.is_some()) << 2));
                self.eff(h.effect)?;
                self.len(h.ops.len())?;
                self.part(&h.body)?;
                for o in &h.ops {
                    self.op(h.effect, o.op)?;
                    self.u8(o.class.code());
                    self.part(&o.part)?;
                }
                if let Some(pt) = &h.ret {
                    self.part(pt)?;
                }
                if let Some(pt) = &h.cancel {
                    self.part(pt)?;
                }
            }
            Exp::Hole => self.u8(0x1D),
            _ => {}
        }
        Ok(())
    }

    fn exp_g1(&mut self, x: &'a Exp) -> W<()> {
        match x {
            Exp::Construct { ty, ctor, fields } => {
                self.u8(0x05);
                self.ty(*ty)?;
                self.ctor(*ty, *ctor)?;
                self.len(fields.len())?;
                for (i, e) in fields {
                    self.uleb(*i);
                    self.exp(*e)?;
                }
            }
            Exp::Variant { label, value } => {
                self.u8(0x06);
                self.str(label)?;
                self.exp(*value)?;
            }
            Exp::Not(a) => {
                self.u8(0x0A);
                self.exp(*a)?;
            }
            Exp::RecSelect { label, record } => {
                self.u8(0x17);
                self.str(label)?;
                self.exp(*record)?;
            }
            Exp::RecRestrict { label, record } => {
                self.u8(0x18);
                self.str(label)?;
                self.exp(*record)?;
            }
            Exp::Resume { tail, value } => {
                self.u8(0x1C);
                self.u8(u8::from(*tail));
                self.exp(*value)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn exp_g2(&mut self, x: &'a Exp) -> W<()> {
        match x {
            Exp::Call {
                tail,
                yields,
                func,
                args,
            } => {
                self.u8(0x07);
                self.u8(u8::from(*tail) | (u8::from(*yields) << 1));
                self.len(args.len())?;
                self.exp(*func)?;
                for a in args {
                    self.exp(*a)?;
                }
            }
            Exp::Method {
                class,
                method,
                dict,
                own,
            } => {
                self.u8(0x08);
                self.cls(*class)?;
                self.meth(*class, *method)?;
                self.len(own.len())?;
                self.dict(*dict)?;
                for o in own {
                    self.dict(*o)?;
                }
            }
            Exp::Opcall {
                class,
                method,
                dict,
                args,
            } => {
                self.u8(0x09);
                self.cls(*class)?;
                self.meth(*class, *method)?;
                self.len(args.len())?;
                self.dict(*dict)?;
                for a in args {
                    self.exp(*a)?;
                }
            }
            Exp::Dictapp { func, dicts } => {
                self.u8(0x0F);
                self.len(dicts.len())?;
                self.exp(*func)?;
                for q in dicts {
                    self.dict(*q)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn exp_g3(&mut self, x: &'a Exp) -> W<()> {
        match x {
            Exp::And(a, b) => {
                self.u8(0x0B);
                self.exp(*a)?;
                self.exp(*b)?;
            }
            Exp::Or(a, b) => {
                self.u8(0x0C);
                self.exp(*a)?;
                self.exp(*b)?;
            }
            Exp::Let { slot, value, body } => {
                self.u8(0x10);
                self.uleb(*slot);
                self.exp(*value)?;
                self.exp(*body)?;
            }
            Exp::Letpat { pat, value, body } => {
                self.u8(0x11);
                self.pat(*pat)?;
                self.exp(*value)?;
                self.exp(*body)?;
            }
            Exp::RecExtend {
                label,
                value,
                record,
            } => {
                self.u8(0x16);
                self.str(label)?;
                self.exp(*value)?;
                self.exp(*record)?;
            }
            Exp::RecUpdate {
                label,
                record,
                value,
            } => {
                self.u8(0x19);
                self.str(label)?;
                self.exp(*record)?;
                self.exp(*value)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn exp_g4(&mut self, x: &'a Exp) -> W<()> {
        match x {
            Exp::Letrec { items, body } => {
                self.u8(0x12);
                self.len(items.len())?;
                for (i, e) in items {
                    self.uleb(*i);
                    self.exp(*e)?;
                }
                self.exp(*body)?;
            }
            Exp::Seq(xs) => {
                self.u8(0x13);
                self.len(xs.len())?;
                for e in xs {
                    self.exp(*e)?;
                }
            }
            Exp::Match { scrutinee, clauses } => {
                self.u8(0x14);
                self.len(clauses.len())?;
                self.exp(*scrutinee)?;
                for c in clauses {
                    self.u8(u8::from(c.guard.is_some()));
                    self.pat(c.pat)?;
                    if let Some(g) = c.guard {
                        self.exp(g)?;
                    }
                    self.exp(c.body)?;
                }
            }
            Exp::Perform { effect, op, args } => {
                self.u8(0x1A);
                self.eff(*effect)?;
                self.op(*effect, *op)?;
                self.len(args.len())?;
                for a in args {
                    self.exp(*a)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn reps(&mut self, rs: &[Rep]) -> W<()> {
        self.len(rs.len())?;
        for r in rs {
            self.u8(r.code());
        }
        Ok(())
    }

    fn function(&mut self, f: &Function) -> W<()> {
        self.u8(f.kind.code());
        self.u8(u8::from(f.yields));
        self.reps(&f.params)?;
        self.reps(&f.locals)?;
        self.uleb(f.dicts);
        self.reps(&f.caps)?;
        self.u8(f.result.code());
        if f.kind == FnKind::Opclause {
            self.len(f.pats.len())?;
            for &q in &f.pats {
                self.pat(q)?;
            }
            match f.guard {
                None => self.u8(0),
                Some(g) => {
                    self.u8(1);
                    self.exp(g)?;
                }
            }
        } else if !f.pats.is_empty() || f.guard.is_some() {
            return internal(format!(
                "操作節でない関数がパターンかガードを持ちます: {}",
                f.sym
            ));
        }
        self.exp(f.body)
    }

    fn step(&mut self, s: &Step) -> W<()> {
        match s {
            Step::DefValue { global, func } => {
                self.u8(0);
                self.glob(*global)?;
                self.func(*func)?;
            }
            Step::DefPattern { func, globals, pat } => {
                self.u8(1);
                self.func(*func)?;
                self.len(globals.len())?;
                for &g in globals {
                    self.glob(g)?;
                }
                self.pat(*pat)?;
            }
            Step::DefFns(gs) => {
                self.u8(2);
                self.len(gs.len())?;
                for &g in gs {
                    self.glob(g)?;
                }
            }
            Step::DefDictabs(g) => {
                self.u8(3);
                self.glob(*g)?;
            }
            Step::DefInstance(i) => {
                self.u8(4);
                self.inst(*i)?;
            }
            Step::RunExp(f) => {
                self.u8(5);
                self.func(*f)?;
            }
        }
        Ok(())
    }

    /// 1 つの節の中身を `f` で作り、`self.b` の代わりに返す。
    fn section(&mut self, f: impl FnOnce(&mut Self) -> W<()>) -> W<Vec<u8>> {
        let saved = std::mem::take(&mut self.b);
        let r = f(self);
        let out = std::mem::replace(&mut self.b, saved);
        r.map(|()| out)
    }
}

/// 関数の本体、ガード、パターンと `def_pattern` のパターンをたどり、ノードの中の名前と文字列を集める。
/// ノードの深さが `lim` を超えたら、何も書かないうちに [`ErrorKind::WriteLimit`] を返す。
fn collect_nodes(p: &Program, lim: usize) -> W<Collect<'_>> {
    let mut c = Collect {
        p,
        strs: BTreeSet::new(),
        blobs: BTreeSet::new(),
        lim,
    };
    for f in &p.functions {
        for &q in &f.pats {
            c.pat(q, 1)?;
        }
        if let Some(g) = f.guard {
            c.exp(g, 1)?;
        }
        c.exp(f.body, 1)?;
    }
    for u in &p.units {
        for s in &u.init {
            if let Step::DefPattern { pat, .. } = s {
                c.pat(*pat, 1)?;
            }
        }
    }
    Ok(c)
}

/// 書き出しの前に、ノードの深さが `lim` 以下で、どのノードも置き場にあることを確かめる。
pub(crate) fn check_write_depth(p: &Program, lim: usize) -> Result<(), Error> {
    collect_nodes(p, lim).map(|_| ())
}

/// プログラムを正準のバイナリにする。`max_nest` はノードの深さの上限である。
///
/// 深さが上限を超えるプログラムは書かずに [`ErrorKind::WriteLimit`] を返す(IRSpec 13.9 節)。
/// 表の範囲の外の添字のような不整合は [`ErrorKind::Internal`] を返す。
pub fn encode(p: &Program, max_nest: usize) -> Result<Vec<u8>, Error> {
    let mut c = collect_nodes(p, max_nest)?;
    for u in &p.units {
        c.strs.insert(&u.key);
        for s in &u.paths {
            c.blobs.insert(s);
        }
    }
    for t in &p.types {
        c.strs.insert(&t.sym);
        for k in &t.ctors {
            c.strs.insert(&k.name);
        }
    }
    for e in &p.effects {
        c.strs.insert(&e.sym);
        for o in &e.ops {
            c.strs.insert(&o.name);
        }
    }
    for k in &p.classes {
        c.strs.insert(&k.sym);
        for m in &k.methods {
            c.strs.insert(&m.name);
        }
    }
    for i in &p.instances {
        c.strs.insert(&i.sym);
    }
    let mut prims: BTreeSet<(u8, &str)> = BTreeSet::new();
    for g in &p.globals {
        c.strs.insert(&g.sym);
        if let GlobalKind::Extern { abi, name, .. } = &g.kind {
            c.strs.insert(name);
            prims.insert((abi.code(), name));
        }
    }
    for f in &p.functions {
        c.strs.insert(&f.sym);
    }
    let strs: Vec<&str> = c.strs.into_iter().collect();
    let blobs: Vec<&[u8]> = c.blobs.into_iter().collect();
    let prims: Vec<(u8, &str)> = prims.into_iter().collect();
    let mut e = Enc {
        p,
        b: Vec::new(),
        str: strs
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, i as u32))
            .collect(),
        blob: blobs
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, i as u32))
            .collect(),
        prim: prims
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, i as u32))
            .collect(),
    };
    let mut sections: Vec<(u8, Vec<u8>)> = Vec::new();
    sections.push((
        SEC_STRINGS,
        e.section(|e| {
            e.len(strs.len())?;
            for s in &strs {
                put_bytes(&mut e.b, s.as_bytes());
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_BLOBS,
        e.section(|e| {
            e.len(blobs.len())?;
            for s in &blobs {
                put_bytes(&mut e.b, s);
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_UNITS,
        e.section(|e| {
            e.len(p.units.len())?;
            for u in &p.units {
                e.str(&u.key)?;
                e.len(u.paths.len())?;
                for s in &u.paths {
                    e.blob(s)?;
                }
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_TYPES,
        e.section(|e| {
            e.len(p.types.len())?;
            for t in &p.types {
                e.str(&t.sym)?;
                e.len(t.ctors.len())?;
                for k in &t.ctors {
                    e.str(&k.name)?;
                    e.uleb(k.arity);
                }
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_EFFECTS,
        e.section(|e| {
            e.len(p.effects.len())?;
            for x in &p.effects {
                e.str(&x.sym)?;
                e.len(x.ops.len())?;
                for o in &x.ops {
                    e.str(&o.name)?;
                    e.uleb(o.arity);
                    e.u8(u8::from(o.never) | (u8::from(o.runtime) << 1));
                }
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_CLASSES,
        e.section(|e| {
            e.len(p.classes.len())?;
            for k in &p.classes {
                e.str(&k.sym)?;
                e.len(k.methods.len())?;
                for m in &k.methods {
                    e.str(&m.name)?;
                    e.uleb(m.dicts);
                }
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_INSTANCES,
        e.section(|e| {
            e.len(p.instances.len())?;
            for i in &p.instances {
                e.str(&i.sym)?;
                e.cls(i.class)?;
                e.ty(i.head)?;
                e.uleb(i.premises);
                e.len(i.methods.len())?;
                for m in &i.methods {
                    e.meth(i.class, m.method)?;
                    e.uleb(m.dicts);
                    e.func(m.func)?;
                }
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_PRIMS,
        e.section(|e| {
            e.len(prims.len())?;
            for (abi, name) in &prims {
                e.u8(*abi);
                e.str(name)?;
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_GLOBALS,
        e.section(|e| {
            e.len(p.globals.len())?;
            for g in &p.globals {
                e.str(&g.sym)?;
                match &g.kind {
                    GlobalKind::Builtin => e.u8(0),
                    GlobalKind::Extern { abi, name, dicts } => {
                        e.u8(1);
                        let i = e.prim[&(abi.code(), name.as_str())];
                        e.uleb(i);
                        e.uleb(*dicts);
                    }
                    GlobalKind::Fn(f) => {
                        e.u8(2);
                        e.func(*f)?;
                    }
                    GlobalKind::Dictabs(f) => {
                        e.u8(3);
                        e.func(*f)?;
                    }
                    GlobalKind::Value(r) => {
                        e.u8(4);
                        e.u8(r.code());
                    }
                }
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_FUNCTIONS,
        e.section(|e| {
            e.len(p.functions.len())?;
            for f in &p.functions {
                e.str(&f.sym)?;
            }
            for f in &p.functions {
                e.function(f)?;
            }
            Ok(())
        })?,
    ));
    sections.push((
        SEC_INIT,
        e.section(|e| {
            e.len(p.units.len())?;
            for u in &p.units {
                e.len(u.init.len())?;
                for s in &u.init {
                    e.step(s)?;
                }
            }
            Ok(())
        })?,
    ));
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&p.version.0.to_le_bytes());
    out.extend_from_slice(&p.version.1.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    for (id, payload) in sections {
        if payload.len() > u32::MAX as usize {
            return internal("節が u32 の大きさを超えます");
        }
        out.push(id);
        put_bytes(&mut out, &payload);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rd(b: &[u8]) -> Rd<'_> {
        Rd {
            src: b,
            pos: 0,
            stop: b.len(),
        }
    }

    #[test]
    fn uleb_rules() {
        assert_eq!(rd(&[0x00]).uleb().ok(), Some(0));
        assert_eq!(rd(&[0xE5, 0x8E, 0x26]).uleb().ok(), Some(624_485));
        // 冗長な符号は受け付ける
        assert_eq!(rd(&[0x80, 0x80, 0x80, 0x80, 0x00]).uleb().ok(), Some(0));
        assert_eq!(rd(&[0x81, 0x80, 0x00]).uleb().ok(), Some(1));
        assert_eq!(
            rd(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]).uleb().ok(),
            Some(u32::MAX)
        );
        // 5 バイト目の第 4〜7 ビット
        assert!(rd(&[0xFF, 0xFF, 0xFF, 0xFF, 0x1F]).uleb().is_err());
        assert!(rd(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]).uleb().is_err());
        // 途中で終わる
        assert!(rd(&[0x80]).uleb().is_err());
    }

    #[test]
    fn sleb32_rules() {
        assert_eq!(rd(&[0x7F]).sleb32().ok(), Some(-1));
        assert_eq!(rd(&[0x3F]).sleb32().ok(), Some(63));
        assert_eq!(rd(&[0xC0, 0x00]).sleb32().ok(), Some(64));
        assert_eq!(rd(&[0xFF, 0x7F]).sleb32().ok(), Some(-1));
        assert_eq!(
            rd(&[0x80, 0x80, 0x80, 0x80, 0x78]).sleb32().ok(),
            Some(i32::MIN)
        );
        assert_eq!(
            rd(&[0xFF, 0xFF, 0xFF, 0xFF, 0x07]).sleb32().ok(),
            Some(i32::MAX)
        );
        // 5 バイト目の第 4〜6 ビットが第 3 ビットと違う
        assert!(rd(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]).sleb32().is_err());
        assert!(rd(&[0x80, 0x80, 0x80, 0x80, 0x70]).sleb32().is_err());
        // 5 バイト目が続きの印を持つ
        assert!(rd(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]).sleb32().is_err());
    }

    #[test]
    fn sleb64_rules() {
        let mut max = vec![0xFF; 8];
        max.push(0xFF);
        max.push(0x00);
        assert_eq!(rd(&max).sleb64().ok(), Some(i64::MAX));
        let mut min = vec![0x80; 9];
        min.push(0x7F);
        assert_eq!(rd(&min).sleb64().ok(), Some(i64::MIN));
        let mut bad = vec![0x80; 9];
        bad.push(0x01);
        assert!(rd(&bad).sleb64().is_err());
        let mut long = vec![0x80; 10];
        long.push(0x00);
        assert!(rd(&long).sleb64().is_err());
        assert_eq!(rd(&[0xFF, 0x7F]).sleb64().ok(), Some(-1));
    }

    #[test]
    fn writer_is_minimal() {
        for n in [0u64, 1, 127, 128, 16383, 16384, u64::from(u32::MAX)] {
            let mut b = Vec::new();
            put_uleb(&mut b, n);
            let want = if n == 0 {
                1
            } else {
                (64 - n.leading_zeros()).div_ceil(7) as usize
            };
            assert_eq!(b.len(), want, "{n}");
            assert_eq!(u64::from(rd(&b).uleb().ok().unwrap()), n);
        }
        for n in [
            0i64,
            -1,
            63,
            64,
            -64,
            -65,
            i64::from(i32::MIN),
            i64::MAX,
            i64::MIN,
        ] {
            let mut b = Vec::new();
            put_sleb(&mut b, n);
            assert_eq!(rd(&b).sleb64().ok(), Some(n), "{n}");
            if let Ok(m) = i32::try_from(n) {
                assert_eq!(rd(&b).sleb32().ok(), Some(m), "{n}");
            }
        }
    }
}
