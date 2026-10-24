//! パターンの照合(パス 2。IRSpec 8 節)。
//!
//! パターンを、値を調べる命令と、一致しなければ失敗の先へ移る枝の列に下ろす。
//! `match` は節を上から順に試し、節の失敗の先が次の節の照合の始まりになる(後戻りを伴う照合)。
//! ガードは、パターンが一致した節だけで評価する。
//! 節の順、ガードを評価する順と回数は KIR のとおりに保つ。
//!
//! - レコードのパターンの `closed`(タプルのパターン)は、残りのフィールドが無いことを型検査が保証するので、
//!   パターンのラベルの列から形を決め、フィールドを位置で読む(パス 6)。
//! - `open` のパターンは、パターンに書かないフィールドを持つレコードにも一致する。
//!   形はパターンから決まらないので、ラベルごとに外側から何番目かを数え、実行時にラベルでフィールドを読む。
//!   書いたラベルがあることは型検査が保証するので、有無は調べない。
//! - どちらでも、形の分かるレコードなら、フィールドの値のレジスタをそのまま使う。
//! - `rest` のパターンは、ラベルごとに外側から何番目かを数えてフィールドを調べ、
//!   残りのパターンが要るときだけ、取り出したフィールドを除いたレコードを作る。
//! - コンストラクタの番号は、型がコンストラクタを 2 つ以上持つときだけ調べる。

use std::collections::HashMap;

use kir::{GlobalId, Lit, Pat, PatId, RecordTail};

use crate::ir::*;
use crate::lower::{Low, V};

/// 変数のパターンが束縛する先。
pub(crate) enum Target<'a> {
    /// 関数のスロット。
    Slots,
    /// `def_pattern` の大域の列。
    Globals(&'a [GlobalId]),
}

impl Low<'_, '_> {
    /// `v` を `p` と照合する。一致しなければ `fail` へ移る。
    ///
    /// 戻ったとき、いまのブロックは一致した場合の続きである。
    pub(crate) fn pattern(&mut self, p: PatId, v: V, fail: BlockId, t: &Target) {
        let k = self.p.k;
        let Some(x) = k.nodes.pat(p) else {
            self.p.note_bug(format!("範囲の外のパターン {}", p.0));
            return;
        };
        match x {
            Pat::Wild => {}
            Pat::Bind(n) => match t {
                Target::Slots => self.bind_slot(*n, v),
                Target::Globals(gs) => match gs.get(*n as usize) {
                    Some(&global) => {
                        let src = self.boxed(v);
                        self.b.emit(Inst::GlobalSet { global, src });
                    }
                    None => self.p.note_bug(format!("範囲の外の大域の束縛 {n}")),
                },
            },
            Pat::Lit(l) => self.pat_lit(l, v, fail),
            Pat::Record { fields, tail } => self.pat_record(fields, tail, v, fail, t),
            Pat::Ctor { ty, ctor, fields } => {
                let x = self.boxed(v);
                let nctors = k.types.get(ty.index()).map(|t| t.ctors.len()).unwrap_or(0);
                if nctors > 1 {
                    let tag = self.b.reg(Rep::I32);
                    self.b.emit(Inst::CtorTag { dst: tag, v: x });
                    let want = self.b.konst(Const::I32(*ctor as i32));
                    self.branch_eq_i32(tag, want, fail);
                }
                for &(i, sp) in fields {
                    if matches!(k.nodes.pat(sp), Some(Pat::Wild)) {
                        continue;
                    }
                    let dst = self.b.reg(Rep::Any);
                    self.b.emit(Inst::CtorField {
                        dst,
                        v: x,
                        index: i,
                    });
                    self.pattern(sp, V::any(dst), fail, t);
                }
            }
            Pat::Variant { label, pat } => {
                let x = self.boxed(v);
                let tag = self.b.reg(Rep::I32);
                self.b.emit(Inst::VariantTag { dst: tag, v: x });
                let l = self.p.label(label);
                let want = self.b.konst(Const::I32(l.0 as i32));
                self.branch_eq_i32(tag, want, fail);
                if !matches!(k.nodes.pat(*pat), Some(Pat::Wild)) {
                    let dst = self.b.reg(Rep::Any);
                    self.b.emit(Inst::VariantPayload { dst, v: x });
                    self.pattern(*pat, V::any(dst), fail, t);
                }
            }
        }
    }

    /// `a == b`(`i32`)でなければ `fail` へ移る。
    fn branch_eq_i32(&mut self, a: Reg, b: Reg, fail: BlockId) {
        let c = self.b.reg(Rep::Bool);
        self.b.emit(Inst::IntCmp {
            ty: IntTy::I32,
            op: CmpOp::Eq,
            dst: c,
            a,
            b,
        });
        self.branch_true(c, fail);
    }

    /// `c` が真なら続き、偽なら `fail` へ移る。
    fn branch_true(&mut self, c: Reg, fail: BlockId) {
        let ok = self.b.block();
        self.b.terminate(Term::Branch {
            cond: c,
            then: ok,
            else_: fail,
        });
        self.b.switch_to(ok);
    }

    fn pat_lit(&mut self, l: &Lit, v: V, fail: BlockId) {
        let (rep, c) = match l {
            Lit::Bool(b) => (Rep::Bool, Const::Bool(*b)),
            Lit::I32(n) => (Rep::I32, Const::I32(*n)),
            Lit::I64(n) => (Rep::I64, Const::I64(*n)),
            Lit::F64(bits) => (Rep::F64, Const::F64(*bits)),
            Lit::Str(s) => (Rep::Str, Const::Str(self.p.string(s))),
        };
        let x = self.coerce(v, rep);
        let y = self.b.konst(c);
        let dst = self.b.reg(Rep::Bool);
        self.b.emit(match rep {
            Rep::Bool => Inst::BoolEq { dst, a: x, b: y },
            Rep::I32 => Inst::IntCmp {
                ty: IntTy::I32,
                op: CmpOp::Eq,
                dst,
                a: x,
                b: y,
            },
            Rep::I64 => Inst::IntCmp {
                ty: IntTy::I64,
                op: CmpOp::Eq,
                dst,
                a: x,
                b: y,
            },
            // IEEE 754 の `==`(0.0 は -0.0 にも一致し、NaN にはどれも一致しない)。
            Rep::F64 => Inst::FloatCmp {
                op: CmpOp::Eq,
                dst,
                a: x,
                b: y,
            },
            _ => Inst::StrCmp {
                op: CmpOp::Eq,
                dst,
                a: x,
                b: y,
            },
        });
        self.branch_true(dst, fail);
    }

    fn pat_record(
        &mut self,
        fields: &[(String, PatId)],
        tail: &RecordTail,
        v: V,
        fail: BlockId,
        t: &Target,
    ) {
        let k = self.p.k;
        let x = self.boxed(v);
        let labels: Vec<LabelId> = fields.iter().map(|(l, _)| self.p.label(l)).collect();
        match tail {
            RecordTail::Closed => {
                // 形はパターンのラベルの列で決まる。同名のラベルの j 番目は、外側から j 番目のフィールドである。
                let layout = crate::record::canonical_positions(&labels);
                let known = self.known.get(&x).cloned();
                for (i, (_, sp)) in fields.iter().enumerate() {
                    if matches!(k.nodes.pat(*sp), Some(Pat::Wild)) {
                        continue;
                    }
                    let pos = layout[i];
                    let fv = match known.as_ref().and_then(|l| l.get(pos as usize)) {
                        Some(&(lab, r)) if lab == labels[i] => r,
                        _ => {
                            let dst = self.b.reg(Rep::Any);
                            self.b.emit(Inst::RecGet {
                                dst,
                                rec: x,
                                index: pos,
                            });
                            dst
                        }
                    };
                    self.pattern(*sp, V::any(fv), fail, t);
                }
            }
            RecordTail::Open => {
                let known = self.known.get(&x).cloned();
                let mut seen: HashMap<LabelId, u32> = HashMap::new();
                for (i, (_, sp)) in fields.iter().enumerate() {
                    let l = labels[i];
                    let nth = *seen.get(&l).unwrap_or(&0);
                    seen.insert(l, nth + 1);
                    if matches!(k.nodes.pat(*sp), Some(Pat::Wild)) {
                        continue;
                    }
                    // 形の分かるレコードでは、同名のラベルの nth 番目は配置の中で連続する位置にある。
                    let at = known
                        .as_ref()
                        .map(|lay| lay.partition_point(|&(y, _)| y < l) + nth as usize);
                    let fv = match (known.as_ref(), at) {
                        (Some(lay), Some(at)) if lay.get(at).map(|&(y, _)| y) == Some(l) => {
                            lay[at].1
                        }
                        _ => {
                            let dst = self.b.reg(Rep::Any);
                            self.b.emit(Inst::RecField {
                                dst,
                                rec: x,
                                label: l,
                                nth,
                            });
                            dst
                        }
                    };
                    self.pattern(*sp, V::any(fv), fail, t);
                }
            }
            RecordTail::Rest(p0) => {
                let mut seen: HashMap<LabelId, u32> = HashMap::new();
                for (i, (_, sp)) in fields.iter().enumerate() {
                    let l = labels[i];
                    let nth = *seen.get(&l).unwrap_or(&0);
                    seen.insert(l, nth + 1);
                    let has = self.b.reg(Rep::Bool);
                    self.b.emit(Inst::RecHas {
                        dst: has,
                        rec: x,
                        label: l,
                        nth,
                    });
                    self.branch_true(has, fail);
                    if matches!(k.nodes.pat(*sp), Some(Pat::Wild)) {
                        continue;
                    }
                    let dst = self.b.reg(Rep::Any);
                    self.b.emit(Inst::RecField {
                        dst,
                        rec: x,
                        label: l,
                        nth,
                    });
                    self.pattern(*sp, V::any(dst), fail, t);
                }
                if !matches!(k.nodes.pat(*p0), Some(Pat::Wild)) {
                    let mut rest = x;
                    for &l in &labels {
                        let dst = self.b.reg(Rep::Any);
                        self.b.emit(Inst::RecRestrict {
                            dst,
                            rec: rest,
                            label: l,
                        });
                        rest = dst;
                    }
                    self.pattern(*p0, V::any(rest), fail, t);
                }
            }
        }
    }
}
