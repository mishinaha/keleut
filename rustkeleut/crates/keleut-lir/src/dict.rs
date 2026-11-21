//! 辞書と、辞書の定数化(パス 5)。
//!
//! 辞書の式の評価はエフェクトを起こさず、実行時エラーにもならない(IRSpec 5.2 節)。
//! 変換器は、辞書の式を次の 3 つに分ける。
//!
//! - 組み込みのインスタンス `dbuiltin(C, T)`。
//!   メソッドの呼び出しは、IRSpec 付録 A.3 のメソッドに対応するプリミティブの命令になる。
//!   対応するプリミティブがないとき、引数の個数がプリミティブと合わないとき、メソッド自身の辞書があるときは、実行時に辞書からメソッドを選ぶ呼び出し(`call_method` か `tail_call_method`)になる。
//! - ユーザのインスタンス `dinst(I, ps)`。
//!   メソッドの呼び出しは、インスタンスの初期化の検査と、実装の値の呼び出しになる。
//!   実装の関数が辞書を受け取らなければ、実装の値を `inst_get_value` で読む。
//!   辞書を受け取るなら、前提の辞書とメソッド自身の辞書を引数にして実装の関数を直接呼び出し、その結果を実装の値とする。
//! - それ以外(辞書の引数、キャプチャした辞書、構造的な辞書)。
//!   メソッドの呼び出しは、実行時に辞書からメソッドを選ぶ呼び出し(`call_method` か `tail_call_method`)になる。
//!
//! どの場合も、メソッドの選択は引数をすべて評価した後に行う(IRSpec 5.4 節)。
//! 非末尾呼び出しでは、呼び出しの位置の深さを `d` として、呼び出し先の深さを `d + 1` と数える(IRSpec 11.2 節)。

use kir::{ClassId, Dict, DictId, ExpId};

use crate::ir::*;
use crate::lower::{Low, V};
use crate::numeric;

/// 辞書の式を LIR に変換した結果。
#[derive(Clone, Debug)]
pub(crate) enum DictV {
    /// 組み込みのインスタンス。
    Builtin { class: ClassId, ty: TypeId },
    /// ユーザのインスタンスと、前提の辞書(一様表現の値を持つレジスタ)。
    Inst {
        inst: InstanceId,
        premises: Vec<Reg>,
    },
    /// 実行時に決まる辞書(一様表現の値を持つレジスタ)。
    Dyn(Reg),
}

impl Low<'_, '_> {
    /// 辞書の式を LIR に変換する。
    pub(crate) fn dict(&mut self, d: DictId) -> DictV {
        let k = self.p.k;
        let Some(x) = k.nodes.dict(d) else {
            self.p.note_bug(format!("範囲外の辞書 {}", d.0));
            let r = self.b.konst(Const::Unit);
            return DictV::Dyn(r);
        };
        match x {
            Dict::Inst { instance, premises } => {
                let premises = self.dict_regs(premises);
                DictV::Inst {
                    inst: *instance,
                    premises,
                }
            }
            Dict::DParam(i) => match self.dparams.get(*i as usize) {
                Some(&r) => DictV::Dyn(r),
                None => {
                    self.p.note_bug(format!("範囲外の辞書の引数 {i}"));
                    DictV::Dyn(self.b.konst(Const::Unit))
                }
            },
            Dict::Env(i) => DictV::Dyn(self.env_reg(*i)),
            Dict::Record(fs) | Dict::Variant(fs) => {
                let mut fields = Vec::with_capacity(fs.len());
                for (l, d) in fs {
                    let dv = self.dict(*d);
                    let r = self.dict_reg(dv);
                    let l = self.p.label(l);
                    fields.push((l, r));
                }
                let dst = self.b.reg(Rep::Any);
                self.b.emit(if matches!(x, Dict::Record(_)) {
                    Inst::DictRecord { dst, fields }
                } else {
                    Inst::DictVariant { dst, fields }
                });
                DictV::Dyn(dst)
            }
            Dict::Builtin { class, ty } => DictV::Builtin {
                class: *class,
                ty: *ty,
            },
        }
    }

    /// 辞書を一様表現の値にして、その値を持つレジスタを返す。
    pub(crate) fn dict_reg(&mut self, d: DictV) -> Reg {
        match d {
            DictV::Builtin { class, ty } => self.b.konst(Const::DictBuiltin { class, ty }),
            DictV::Inst { inst, premises } if premises.is_empty() => {
                self.b.konst(Const::DictInst0(inst))
            }
            DictV::Inst { inst, premises } => {
                let dst = self.b.reg(Rep::Any);
                self.b.emit(Inst::DictInst {
                    dst,
                    inst,
                    premises,
                });
                dst
            }
            DictV::Dyn(r) => r,
        }
    }

    /// 式 `method` を変換する。
    /// メソッドの値を `method_new` で作るだけで、呼び出しは行わない。
    pub(crate) fn method_value(
        &mut self,
        class: ClassId,
        method: u32,
        dict: DictId,
        own: &[DictId],
    ) -> V {
        let d = self.dict(dict);
        let dict = self.dict_reg(d);
        let own = self.dict_regs(own);
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::MethodNew {
            dst,
            class,
            method,
            dict,
            own,
        });
        V::any(dst)
    }

    /// 式 `opcall` を変換する。
    /// 末尾呼び出しにはならない。
    pub(crate) fn opcall(
        &mut self,
        class: ClassId,
        method: u32,
        dict: DictId,
        args: &[ExpId],
    ) -> Option<V> {
        let d = self.dict(dict);
        let args = self.args(args)?;
        // `opcall` は KIR に `yield` の印を持たないので、ユーザの実装の呼び出しは yield することがあるものとして扱う。
        self.method_call(class, method, d, Vec::new(), args, false, true)
    }

    /// メソッドを選んで呼ぶ命令を置く。
    /// 引数は評価済みである。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn method_call(
        &mut self,
        class: ClassId,
        method: u32,
        d: DictV,
        own: Vec<Reg>,
        args: Vec<V>,
        tail: bool,
        yields: bool,
    ) -> Option<V> {
        let k = self.p.k;
        match d {
            DictV::Builtin { class: dc, ty } => {
                let cname = k.classes[dc.index()].sym.as_str();
                let tname = k.types[ty.index()].sym.as_str();
                let mname = k.classes[class.index()]
                    .methods
                    .get(method as usize)
                    .map(|m| m.name.as_str())
                    .unwrap_or("");
                let prim = numeric::builtin_method(
                    cname.strip_prefix("builtin/").unwrap_or(cname),
                    tname.strip_prefix("builtin/").unwrap_or(tname),
                    mname,
                );
                match prim {
                    Some(p) if numeric::sig(p).0.len() == args.len() && own.is_empty() => {
                        self.prim_call(p, &args, tail)
                    }
                    _ => {
                        let dict = self.dict_reg(DictV::Builtin { class: dc, ty });
                        self.dyn_method(class, method, dict, own, args, tail, yields)
                    }
                }
            }
            DictV::Inst { inst, premises } => {
                let Some(im) = k
                    .instances
                    .get(inst.index())
                    .and_then(|i| i.methods.iter().find(|m| m.method == method))
                else {
                    return self.bug("インスタンスにメソッドの実装がない");
                };
                let ndicts = k.instances[inst.index()].premises + im.dicts;
                let func = self.p.func(im.func);
                let args: Vec<Reg> = args.into_iter().map(|v| self.boxed(v)).collect();
                if !tail {
                    crate::depth::push(&mut self.b);
                }
                let ok = self.b.reg(Rep::Bool);
                self.b.emit(Inst::InstIsInit { dst: ok, inst });
                let bad = self.b.reg(Rep::Bool);
                self.b.emit(Inst::BoolNot { dst: bad, a: ok });
                self.b.fail_if(bad, RtError::InstanceNotReady(inst), None);
                let f = self.b.reg(Rep::Any);
                if ndicts == 0 {
                    self.b.emit(Inst::InstGetValue {
                        dst: f,
                        inst,
                        method,
                    });
                } else {
                    // 実装の関数は呼び出し先の深さで評価するので、ここでは深さを増やさない。
                    let mut ds = premises;
                    ds.extend(own);
                    self.b.emit(Inst::CallDirect {
                        dst: f,
                        func,
                        args: ds,
                        yields: false,
                    });
                }
                if tail {
                    self.b.terminate(Term::TailCallValue {
                        callee: f,
                        args,
                        yields,
                    });
                    return None;
                }
                let dst = self.b.reg(Rep::Any);
                self.b.emit(Inst::CallValue {
                    dst,
                    callee: f,
                    args,
                    yields,
                });
                crate::depth::restore(&mut self.b);
                Some(V::any(dst))
            }
            DictV::Dyn(dict) => self.dyn_method(class, method, dict, own, args, tail, yields),
        }
    }

    /// 実行時に辞書からメソッドを選ぶ呼び出し(`call_method` か `tail_call_method`)を置く。
    #[allow(clippy::too_many_arguments)]
    fn dyn_method(
        &mut self,
        class: ClassId,
        method: u32,
        dict: Reg,
        own: Vec<Reg>,
        args: Vec<V>,
        tail: bool,
        yields: bool,
    ) -> Option<V> {
        let args: Vec<Reg> = args.into_iter().map(|v| self.boxed(v)).collect();
        if tail {
            self.b.terminate(Term::TailCallMethod {
                class,
                method,
                dict,
                own,
                args,
                yields,
            });
            return None;
        }
        let dst = self.b.reg(Rep::Any);
        crate::depth::around_call(&mut self.b, |b| {
            b.emit(Inst::CallMethod {
                dst,
                class,
                method,
                dict,
                own,
                args,
                yields,
            })
        });
        Some(V::any(dst))
    }
}
