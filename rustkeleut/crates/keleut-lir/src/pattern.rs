//! パターンの照合(パス 2。IRSpec 8 節)。
//!
//! パターンを、値を調べる命令と、一致しなかったときの分岐の並びに変換する。
//! 一致しなかったときの分岐先を、以下では失敗時の分岐先と呼ぶ。
//! `match` は節を上から順に試す。
//! ある節の失敗時の分岐先は、次の節の照合の先頭である(後戻りを伴う照合)。
//! ガードは、パターンが一致した節でだけ評価する。
//! 節の順序と、ガードを評価する順序と回数は、KIR のとおりに保つ。
//!
//! - `closed` のレコードのパターン(タプルのパターン)では、パターンに書いたフィールドのほかにフィールドがないことを型検査が保証する。
//!   そこで、パターンのラベルの並びからシェイプを決め、フィールドを位置で読む(パス 6)。
//! - `open` のレコードのパターンは、パターンに書かないフィールドを持つレコードにも一致する。
//!   シェイプはパターンから決まらないので、同じラベルの中で外側から何番目かを数え、実行時にラベルでフィールドを読む。
//!   パターンに書いたラベルのフィールドがあることは型検査が保証するので、フィールドの有無は調べない。
//! - `closed` でも `open` でも、照合対象がシェイプの分かるレコードなら、フィールドの値のレジスタをそのまま使う。
//! - `rest` のレコードのパターンでは、同じラベルの中で外側から何番目かを数え、フィールドがあることを実行時に調べてから読む。
//!   残りに対するパターンがワイルドカードでないときだけ、パターンに書いたフィールドを取り除いたレコードを作る。
//! - コンストラクタの番号は、型がコンストラクタを 2 つ以上持つときだけ調べる。

use std::collections::HashMap;

use kir::{GlobalId, Lit, Pat, PatId, RecordTail};

use crate::ir::*;
use crate::lower::{Low, V};

/// 変数のパターンが値を束縛する先。
pub(crate) enum Target<'a> {
    /// 関数のスロット。
    Slots,
    /// `def_pattern` のグローバルの並び。
    Globals(&'a [GlobalId]),
}

impl Low<'_, '_> {
    /// 値 `v` をパターン `p` と照合する命令を生成する。
    /// 一致しなければ `fail` へ分岐する。
    ///
    /// この関数から戻ったとき、現在のブロックは、一致した場合に続きを実行するブロックである。
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

    /// `i32` の `a` と `b` が等しくなければ `fail` へ分岐する。
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

    /// `c` が真なら続きを実行し、偽なら `fail` へ分岐する。
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
            // IEEE 754 の `==` で比較する。
            // 0.0 は -0.0 にも一致し、NaN はどの値とも一致しない。
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
                // シェイプは、パターンのラベルの並びで決まる。
                // 同じラベルのうち j 番目に書いたものは、外側から j 番目のフィールドに対応する。
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
                    // シェイプの分かるレコードでは、同じラベルのフィールドがレイアウトの中で連続して並ぶ。
                    // 外側から nth 番目のフィールドは、そのラベルの最初の位置から nth 個後ろにある。
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
