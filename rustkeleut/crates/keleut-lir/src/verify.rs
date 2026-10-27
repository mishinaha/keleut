//! LIR の検証器。
//!
//! 下ろしの後とパス 12 の後に、次のことを確かめる(計画 261024-1 の 6.4 節)。
//!
//! | 性質 | 確かめ方 |
//! |---|---|
//! | 添字 | レジスタ、ブロック、関数、大域、インスタンス、型、クラス、ラベル、形、文字列の番号が範囲にある |
//! | Rep | 各命令の被演算子と結果の Rep が命令の型に合う。引数と戻り値は `any` である |
//! | 非巡回 | どの枝も番号の大きいブロックへ向かう(ブロックは位相の順に並ぶ)。入口のほかのブロックは前任を持つ |
//! | 定義 | どのレジスタも、読むどの経路でも先に書かれている |
//! | 呼び出し | 直の呼び出しの先は捕獲を持たない初期化でない関数で、引数の数が合う。クロージャの捕獲の数が合う。`closure_set_cap` の対象は同じブロックで直前に作ったクロージャで、添字が捕獲の数より小さい |
//! | 環境 | `env_get` は入口のブロックの先頭の `depth_enter` に続く位置にだけある |
//! | 深さ | `depth_enter` は入口のブロックの先頭にちょうど 1 つある |
//! | 辞書 | `dict_builtin` の組は組み込みのメソッドの表にある。`dict_inst` の前提は 1 つ以上ある。`inst_get_value` と `inst_set_value` のメソッドは辞書を受け取らない |
//! | 表 | 大域の関数は捕獲を持たず、種類が大域の種類に合う。インスタンスはクラスのメソッドの数だけ実装を持ち、実装の関数の引数の数が辞書の数に等しい。初期化の関数は引数を持たない |
//! | 実行時エラー | `error` の被演算子が番号に合う。19 番(メモリの不足)は `error` に使わない |
//! | エフェクト | エフェクトの命令とブロックの終わりの被演算子の数と Rep が合い、エフェクトと操作の番号が範囲にある。`$rt_handle` は引数を 3 つ取る |
//! | フレーム | スロットの区画が Rep に合い、区間の重なるレジスタが同じスロットを使わない。再開点と安全点の位置と集合が、パス 12 の生存解析([`frame::points`])で計算し直したものと一致する。yield を受け取らないとした再開点は `noyield` の呼び出しである |
//!
//! フレームの最後の検査は、パス 12 と同じ生存解析で計算し直して比べる。
//! そのため、割り当ての後でフレームを書き換えたことは見つけるが、生存解析そのものの誤りは見つけない。

use std::collections::HashSet;

use crate::bits::{Bits, Timeline};
use crate::frame;
use crate::ir::*;

/// 検証の失敗(文面)。
pub type VResult = Result<(), String>;

struct Cx<'a> {
    p: &'a Program,
    f: &'a Func,
    /// 組み込みのメソッドの表にある `(クラス, 型)` の組。
    builtin_pairs: &'a HashSet<(ClassId, TypeId)>,
}

macro_rules! ensure {
    ($c:expr, $($arg:tt)*) => {
        if !$c {
            return Err(format!($($arg)*));
        }
    };
}

impl Cx<'_> {
    fn rep(&self, r: Reg) -> Result<Rep, String> {
        self.f
            .regs
            .get(r.index())
            .copied()
            .ok_or_else(|| format!("範囲の外のレジスタ r{}", r.0))
    }

    fn is(&self, r: Reg, want: Rep) -> VResult {
        let got = self.rep(r)?;
        ensure!(
            got == want,
            "r{} の Rep は {} だが {} が要る",
            r.0,
            got.name(),
            want.name()
        );
        Ok(())
    }

    fn all(&self, rs: &[Reg], want: Rep) -> VResult {
        rs.iter().try_for_each(|&r| self.is(r, want))
    }

    fn func(&self, f: FuncId) -> Result<&Func, String> {
        self.p
            .funcs
            .get(f.index())
            .ok_or_else(|| format!("範囲の外の関数 {}", f.0))
    }

    fn global(&self, g: GlobalId) -> Result<&Global, String> {
        self.p
            .globals
            .get(g.index())
            .ok_or_else(|| format!("範囲の外の大域 {}", g.0))
    }

    fn inst(&self, i: InstanceId) -> Result<&Instance, String> {
        self.p
            .instances
            .get(i.index())
            .ok_or_else(|| format!("範囲の外のインスタンス {}", i.0))
    }

    fn label(&self, l: LabelId) -> VResult {
        ensure!(l.index() < self.p.labels.len(), "範囲の外のラベル {}", l.0);
        Ok(())
    }

    fn class_method(&self, c: ClassId, m: u32) -> VResult {
        let cl = self
            .p
            .classes
            .get(c.index())
            .ok_or_else(|| format!("範囲の外のクラス {}", c.0))?;
        ensure!(
            (m as usize) < cl.methods.len(),
            "クラス {} に範囲の外のメソッド {}",
            cl.sym,
            m
        );
        Ok(())
    }

    fn direct(&self, func: FuncId, args: &[Reg]) -> VResult {
        let t = self.func(func)?;
        ensure!(t.ncaps == 0, "捕獲を持つ関数 {} を直に呼んだ", t.sym);
        ensure!(
            t.kind != FuncKind::Init,
            "初期化の関数 {} を直に呼んだ",
            t.sym
        );
        ensure!(
            t.params.len() == args.len(),
            "関数 {} の引数は {} 個だが {} 個を渡した",
            t.sym,
            t.params.len(),
            args.len()
        );
        self.all(args, Rep::Any)
    }

    fn konst(&self, dst: Reg, c: &Const) -> VResult {
        self.is(dst, c.rep())?;
        match c {
            Const::Str(s) => ensure!(s.index() < self.p.strings.len(), "範囲の外の文字列 {}", s.0),
            Const::Ctor0 { ty, ctor } => {
                let t = self
                    .p
                    .types
                    .get(ty.index())
                    .ok_or_else(|| format!("範囲の外の型 {}", ty.0))?;
                let c = t
                    .ctors
                    .get(*ctor as usize)
                    .ok_or_else(|| format!("範囲の外のコンストラクタ {ctor}"))?;
                ensure!(c.arity == 0, "引数を持つコンストラクタ {} の ctor0", c.name);
            }
            Const::DictBuiltin { class, ty } => {
                ensure!(class.index() < self.p.classes.len(), "範囲の外のクラス");
                ensure!(ty.index() < self.p.types.len(), "範囲の外の型");
                ensure!(
                    self.builtin_pairs.contains(&(*class, *ty)),
                    "組み込みのメソッドの表に無い組 {}[{}] の辞書",
                    self.p.classes[class.index()].sym,
                    self.p.types[ty.index()].sym
                );
            }
            Const::DictInst0(i) => {
                let x = self.inst(*i)?;
                ensure!(
                    x.premises == 0,
                    "前提を持つインスタンス {} の定数の辞書",
                    x.sym
                );
            }
            _ => {}
        }
        Ok(())
    }

    /// 命令を確かめる。`before` は同じブロックの中でこの命令より前の命令、`entry` はブロックが入口か。
    fn check_inst(&self, i: &Inst, before: &[Inst], entry: bool) -> VResult {
        use Inst::*;
        use Rep::*;
        let at_entry_start = entry && before.is_empty();
        match i {
            Const { dst, value } => self.konst(*dst, value)?,
            Move { dst, src } => {
                let r = self.rep(*src)?;
                self.is(*dst, r)?;
            }
            Box { dst, src } => {
                self.is(*dst, Any)?;
                ensure!(self.rep(*src)? != Any, "box の元が any である");
            }
            Unbox { dst, src } => {
                self.is(*src, Any)?;
                ensure!(self.rep(*dst)? != Any, "unbox の先が any である");
            }
            EnvGet { dst, index } => {
                self.is(*dst, Any)?;
                ensure!(*index < self.f.ncaps, "範囲の外の捕獲 {index}");
                ensure!(
                    entry
                        && !before.is_empty()
                        && before
                            .iter()
                            .all(|x| matches!(x, DepthEnter | EnvGet { .. })),
                    "env_get が入口の先頭の depth_enter に続く位置にない"
                );
            }
            DepthEnter => ensure!(at_entry_start, "depth_enter が入口の先頭にない"),
            DepthCheck | DepthPush | DepthRestore => {}
            IntArith { ty, dst, a, b, .. } => self.all(&[*dst, *a, *b], ty.rep())?,
            IntNeg { ty, dst, a } => self.all(&[*dst, *a], ty.rep())?,
            IntCmp { ty, dst, a, b, .. } => {
                self.all(&[*a, *b], ty.rep())?;
                self.is(*dst, Bool)?;
            }
            FloatArith { dst, a, b, .. } => self.all(&[*dst, *a, *b], F64)?,
            FloatNeg { dst, a } | FloatMath { dst, a, .. } | FloatTrunc { dst, a } => {
                self.all(&[*dst, *a], F64)?
            }
            FloatCmp { dst, a, b, .. } => {
                self.all(&[*a, *b], F64)?;
                self.is(*dst, Bool)?;
            }
            FloatIsNan { dst, a } => {
                self.is(*a, F64)?;
                self.is(*dst, Bool)?;
            }
            Conv { op, dst, a } => {
                let (x, y) = op.reps();
                self.is(*a, x)?;
                self.is(*dst, y)?;
            }
            BoolNot { dst, a } => self.all(&[*dst, *a], Bool)?,
            BoolEq { dst, a, b } => self.all(&[*dst, *a, *b], Bool)?,
            StrConcat { dst, a, b } => self.all(&[*dst, *a, *b], Str)?,
            StrCmp { dst, a, b, .. } => {
                self.all(&[*a, *b], Str)?;
                self.is(*dst, Bool)?;
            }
            StrLen { dst, a } => {
                self.is(*a, Str)?;
                self.is(*dst, I32)?;
            }
            StrSub { dst, s, pos, len } => {
                self.all(&[*dst, *s], Str)?;
                self.all(&[*pos, *len], I32)?;
            }
            Show { op, dst, a } => {
                self.is(*a, op.rep())?;
                self.is(*dst, Str)?;
            }
            CtorNew {
                dst,
                ty,
                ctor,
                fields,
            } => {
                self.is(*dst, Any)?;
                self.all(fields, Any)?;
                let t = self
                    .p
                    .types
                    .get(ty.index())
                    .ok_or_else(|| format!("範囲の外の型 {}", ty.0))?;
                let c = t
                    .ctors
                    .get(*ctor as usize)
                    .ok_or_else(|| format!("範囲の外のコンストラクタ {ctor}"))?;
                ensure!(
                    c.arity as usize == fields.len() && !fields.is_empty(),
                    "コンストラクタ {} のフィールドの数が合わない",
                    c.name
                );
            }
            CtorTag { dst, v } | VariantTag { dst, v } => {
                self.is(*v, Any)?;
                self.is(*dst, I32)?;
            }
            CtorField { dst, v, .. } | VariantPayload { dst, v } => self.all(&[*dst, *v], Any)?,
            VariantNew {
                dst,
                label,
                payload,
            } => {
                self.all(&[*dst, *payload], Any)?;
                self.label(*label)?;
            }
            RecNew { dst, shape, fields } => {
                self.is(*dst, Any)?;
                self.all(fields, Any)?;
                let s = self
                    .p
                    .shapes
                    .get(shape.index())
                    .ok_or_else(|| format!("範囲の外の形 {}", shape.0))?;
                ensure!(
                    s.len() == fields.len() && !s.is_empty(),
                    "形 {} のフィールドの数が合わない",
                    shape.0
                );
            }
            RecGet { dst, rec, .. } => self.all(&[*dst, *rec], Any)?,
            RecExtend {
                dst,
                rec,
                label,
                value,
            }
            | RecUpdate {
                dst,
                rec,
                label,
                value,
            } => {
                self.all(&[*dst, *rec, *value], Any)?;
                self.label(*label)?;
            }
            RecField {
                dst, rec, label, ..
            }
            | RecRestrict { dst, rec, label } => {
                self.all(&[*dst, *rec], Any)?;
                self.label(*label)?;
            }
            RecHas {
                dst, rec, label, ..
            } => {
                self.is(*rec, Any)?;
                self.is(*dst, Bool)?;
                self.label(*label)?;
            }
            ClosureNew { dst, func, caps } => {
                self.is(*dst, Any)?;
                self.all(caps, Any)?;
                let t = self.func(*func)?;
                ensure!(
                    matches!(
                        t.kind,
                        FuncKind::Kir(
                            FnKind::Lambda
                                | FnKind::Dictabs
                                | FnKind::Opclause
                                | FnKind::Retclause
                                | FnKind::Cancelclause
                                | FnKind::Handlebody
                        )
                    ),
                    "クロージャにできない種類の関数 {}",
                    t.sym
                );
                ensure!(
                    t.ncaps as usize == caps.len(),
                    "関数 {} の捕獲の数が合わない",
                    t.sym
                );
            }
            ClosureSetCap { clo, index, value } => {
                self.all(&[*clo, *value], Any)?;
                // 対象を最後に書いた命令は、同じブロックの closure_new でなければならない。
                let maker = before.iter().rev().find(|x| x.def() == Some(*clo));
                let Some(ClosureNew { func, .. }) = maker else {
                    return Err(format!(
                        "closure_set_cap の r{} を同じブロックで最後に書いたのが closure_new でない",
                        clo.0
                    ));
                };
                let t = self.func(*func)?;
                ensure!(
                    *index < t.ncaps,
                    "関数 {} のクロージャに範囲の外の捕獲 {index} を書く",
                    t.sym
                );
            }
            MethodNew {
                dst,
                class,
                method,
                dict,
                own,
            } => {
                self.all(&[*dst, *dict], Any)?;
                self.all(own, Any)?;
                self.class_method(*class, *method)?;
            }
            DictInst {
                dst,
                inst,
                premises,
            } => {
                self.is(*dst, Any)?;
                self.all(premises, Any)?;
                let x = self.inst(*inst)?;
                ensure!(
                    x.premises as usize == premises.len(),
                    "インスタンス {} の前提の数が合わない",
                    x.sym
                );
                ensure!(
                    !premises.is_empty(),
                    "前提を持たないインスタンス {} の dict_inst",
                    x.sym
                );
            }
            DictRecord { dst, fields } | DictVariant { dst, fields } => {
                self.is(*dst, Any)?;
                for (l, r) in fields {
                    self.label(*l)?;
                    self.is(*r, Any)?;
                }
            }
            GlobalGet { dst, global } => {
                self.is(*dst, Any)?;
                self.global(*global)?;
            }
            GlobalIsInit { dst, global } => {
                self.is(*dst, Bool)?;
                self.global(*global)?;
            }
            GlobalSet { global, src } => {
                self.is(*src, Any)?;
                self.global(*global)?;
            }
            GlobalMarkInit { global } => {
                self.global(*global)?;
            }
            InstIsInit { dst, inst } => {
                self.is(*dst, Bool)?;
                self.inst(*inst)?;
            }
            InstMarkInit { inst } => {
                self.inst(*inst)?;
            }
            InstSetValue { inst, method, src } => {
                self.is(*src, Any)?;
                self.inst_value(*inst, *method)?;
            }
            InstGetValue { dst, inst, method } => {
                self.is(*dst, Any)?;
                self.inst_value(*inst, *method)?;
            }
            RefNew { dst, value } => self.all(&[*dst, *value], Any)?,
            RefGet { dst, r } => self.all(&[*dst, *r], Any)?,
            RefSet { r, value } => self.all(&[*r, *value], Any)?,
            ArrayLen { dst, a } | MArrayLen { dst, a } => {
                self.is(*a, Any)?;
                self.is(*dst, I32)?;
            }
            ArrayGet { dst, a, index } | MArrayGet { dst, a, index } => {
                self.all(&[*dst, *a], Any)?;
                self.is(*index, I32)?;
            }
            MArrayNew { dst, len, init } => {
                self.all(&[*dst, *init], Any)?;
                self.is(*len, I32)?;
            }
            MArraySet { a, index, value } => {
                self.all(&[*a, *value], Any)?;
                self.is(*index, I32)?;
            }
            MArrayFreeze { dst, a } => self.all(&[*dst, *a], Any)?,
            CallDirect {
                dst, func, args, ..
            } => {
                self.is(*dst, Any)?;
                self.direct(*func, args)?;
            }
            CallValue {
                dst, callee, args, ..
            } => {
                self.all(&[*dst, *callee], Any)?;
                self.all(args, Any)?;
            }
            CallMethod {
                dst,
                class,
                method,
                dict,
                own,
                args,
                ..
            } => {
                self.all(&[*dst, *dict], Any)?;
                self.all(own, Any)?;
                self.all(args, Any)?;
                self.class_method(*class, *method)?;
            }
            DictApp { dst, f, dicts } => {
                self.all(&[*dst, *f], Any)?;
                self.all(dicts, Any)?;
            }
            ConsoleWrite { dst, s } => {
                self.is(*dst, Any)?;
                self.is(*s, Str)?;
            }
            UnimplementedValue { dst, .. } => self.is(*dst, Any)?,
            Fx { op, dst, args } => self.fx(op, *dst, args)?,
        }
        Ok(())
    }

    fn effect_op(&self, e: u32, op: Option<u32>) -> VResult {
        let x = self
            .p
            .effects
            .get(e as usize)
            .ok_or_else(|| format!("範囲の外のエフェクト {e}"))?;
        if let Some(op) = op {
            ensure!(
                (op as usize) < x.ops.len(),
                "エフェクト {} に範囲の外の操作 {op}",
                x.sym
            );
        }
        Ok(())
    }

    /// エフェクトの命令を確かめる。
    fn fx(&self, op: &FxOp, dst: Option<Reg>, args: &[Reg]) -> VResult {
        let (want, res) = op.sig();
        match want {
            FxArgs::Fixed(reps) => {
                ensure!(
                    reps.len() == args.len(),
                    "{} の被演算子は {} 個だが {} 個ある",
                    op.name(),
                    reps.len(),
                    args.len()
                );
                for (&r, &w) in args.iter().zip(reps) {
                    self.is(r, w)?;
                }
            }
            FxArgs::AtLeast(n) => {
                ensure!(
                    args.len() >= n,
                    "{} の被演算子が {} 個より少ない",
                    op.name(),
                    n
                );
                self.all(args, Rep::Any)?;
            }
        }
        match (res, dst) {
            (None, None) => {}
            (Some(rep), Some(d)) => self.is(d, rep)?,
            _ => return Err(format!("{} の結果の有無が合わない", op.name())),
        }
        match op {
            FxOp::HandlerNew {
                effect,
                ops,
                has_ret,
                has_cancel,
            } => {
                self.effect_op(*effect, None)?;
                ensure!(!ops.is_empty(), "操作節を持たない handler_new");
                for (o, _) in ops {
                    self.effect_op(*effect, Some(*o))?;
                }
                ensure!(
                    args.len() == 2 + ops.len() + usize::from(*has_ret) + usize::from(*has_cancel),
                    "handler_new の被演算子の数が節の数と合わない"
                );
            }
            FxOp::HandlerFind { effect, op } => self.effect_op(*effect, Some(*op))?,
            FxOp::Perform { effect, op } => {
                self.effect_op(*effect, Some(*op))?;
                let arity = self.p.effects[*effect as usize].ops[*op as usize].1 as usize;
                ensure!(
                    args.len() == 2 + arity,
                    "perform の引数の数が操作の引数の数と合わない"
                );
            }
            _ => {}
        }
        Ok(())
    }

    /// `inst_get_value` と `inst_set_value` の相手。メソッドの実装の関数は辞書を受け取らない。
    fn inst_value(&self, inst: InstanceId, method: u32) -> VResult {
        let x = self.inst(inst)?;
        let m = x
            .methods
            .get(method as usize)
            .ok_or_else(|| format!("インスタンス {} に範囲の外のメソッド {method}", x.sym))?;
        ensure!(
            m.dicts == 0,
            "インスタンス {} のメソッド {method} は辞書を受け取るので値を持たない",
            x.sym
        );
        Ok(())
    }

    fn check_term(&self, bi: usize, t: &Term) -> VResult {
        let nb = self.f.blocks.len();
        for s in t.succs() {
            ensure!(
                s.index() > bi && s.index() < nb,
                "b{bi} から前向きでない枝 b{}",
                s.0
            );
        }
        match t {
            Term::Jump(_) | Term::Unimplemented { .. } => {}
            Term::Branch { cond, .. } => self.is(*cond, Rep::Bool)?,
            Term::Return(r) => self.is(*r, Rep::Any)?,
            Term::TailCallDirect { func, args, .. } => self.direct(*func, args)?,
            Term::TailCallValue { callee, args, .. } => {
                self.is(*callee, Rep::Any)?;
                self.all(args, Rep::Any)?;
            }
            Term::TailCallMethod {
                class,
                method,
                dict,
                own,
                args,
                ..
            } => {
                self.is(*dict, Rep::Any)?;
                self.all(own, Rep::Any)?;
                self.all(args, Rep::Any)?;
                self.class_method(*class, *method)?;
            }
            Term::Fx { op, args } => {
                let (n, fixed) = op.arity();
                ensure!(
                    if fixed {
                        args.len() == n
                    } else {
                        args.len() >= n
                    },
                    "{} の被演算子の数が合わない",
                    op.name()
                );
                self.all(args, Rep::Any)?;
            }
            Term::Error { kind, operand } => {
                match (kind.operand_rep(), operand) {
                    (None, None) => {}
                    (Some(want), Some(r)) => self.is(*r, want)?,
                    _ => {
                        return Err(format!(
                            "実行時エラー {} の被演算子が合わない",
                            self.p.error_number(kind)
                        ));
                    }
                }
                match kind {
                    RtError::OutOfMemory => {
                        return Err(
                            "メモリの不足(19 番)は確保する命令が起こすもので、error には使わない"
                                .to_string(),
                        );
                    }
                    RtError::Unbound(g) => {
                        self.global(*g)?;
                    }
                    RtError::UnimplPrim(g) => {
                        let gl = self.global(*g)?;
                        ensure!(
                            matches!(gl.kind, GlobalKind::Extern { .. }),
                            "extern でない大域 {} の未実装のプリミティブ",
                            gl.sym
                        );
                    }
                    RtError::InstanceNotReady(i) => {
                        self.inst(*i)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// 定義の検査([`check_defs`])。
    fn check_defs(&self) -> VResult {
        check_defs(self.f).0
    }

    fn check_frame(&self, fr: &Frame) -> VResult {
        let f = self.f;
        ensure!(
            fr.slot.len() == f.regs.len(),
            "スロットの表の長さが合わない"
        );
        for (i, &p) in f.params.iter().enumerate() {
            ensure!(
                fr.slot[p.index()] == Slot::Any(i as u32),
                "引数 {i} のスロットが any の {i} 番でない"
            );
        }
        let iv = frame::intervals(f);
        let mut by_slot: std::collections::HashMap<Slot, Vec<(u32, u32)>> =
            std::collections::HashMap::new();
        for (r, &rep) in f.regs.iter().enumerate() {
            let s = fr.slot[r];
            match s {
                Slot::Any(x) => {
                    ensure!(frame::is_any_class(rep), "生の r{r} が any の区画にある");
                    ensure!(x < fr.any_slots, "範囲の外の any のスロット {x}");
                }
                Slot::Raw(x) => {
                    ensure!(!frame::is_any_class(rep), "r{r} が生の区画にある");
                    ensure!(x < fr.raw_slots, "範囲の外の生のスロット {x}");
                }
            }
            if let Some(v) = iv[r] {
                by_slot.entry(s).or_default().push(v);
            }
        }
        for (s, mut v) in by_slot {
            v.sort();
            for w in v.windows(2) {
                ensure!(
                    w[0].1 < w[1].0,
                    "区間の重なるレジスタがスロット {s:?} を共有する"
                );
            }
        }
        let (rp, sp) = frame::points(f);
        ensure!(
            rp == fr.resume_points,
            "再開点が、パス 12 の生存解析で計算し直したものと一致しない"
        );
        ensure!(
            sp == fr.safepoints,
            "安全点が、パス 12 の生存解析で計算し直したものと一致しない"
        );
        ensure!(
            fr.yield_points.len() == rp.len(),
            "yield の印の数が再開点の数と合わない"
        );
        let honest = frame::yield_points(f, &rp, true);
        for (i, (&y, &h)) in fr.yield_points.iter().zip(&honest).enumerate() {
            ensure!(
                y || !h,
                "再開点 {i} は yield の印の付いた呼び出しなのに yield を受け取らないとされている"
            );
        }
        Ok(())
    }
}

/// 組み込みのメソッドの表にある `(クラス, 型)` の組。
fn builtin_pairs(p: &Program) -> HashSet<(ClassId, TypeId)> {
    p.builtin_methods.iter().map(|m| (m.class, m.ty)).collect()
}

/// 1 つの関数を検証する。
pub fn verify_func(p: &Program, f: &Func) -> VResult {
    verify_func_with(p, f, &builtin_pairs(p))
}

/// 定義の検査(前向きのデータフロー)。
///
/// ブロックを位相の順にたどり、書いたレジスタの集合を 1 つの [`Timeline`] で書き換えていく。
/// ブロックを処理し終えたら、その出口の集合を各後任の行に積む(積集合)。
/// 行が空なら日誌の長さとして取っておくので、写しは作らない。
/// 後任の入口の集合はその行の集合であり、後任を処理する前に今の集合をそれに戻す。
/// 直前のブロックが前任なら、行は今の集合から除くレジスタの列になるので、戻す手間はその数に比例する。
/// 検査の結果と、その手間(日誌と集合を読み書きした量)を返す。
pub(crate) fn check_defs(f: &Func) -> (VResult, u64) {
    let mut d = Timeline::new(f.regs.len(), f.blocks.len());
    let r = defs_in(f, &mut d);
    (r, d.work())
}

fn defs_in(f: &Func, d: &mut Timeline) -> VResult {
    for p in &f.params {
        d.set(p.0, true);
    }
    for (bi, b) in f.blocks.iter().enumerate() {
        if bi > 0 {
            ensure!(d.has(bi), "b{bi} は入口からたどれない");
            d.restore(bi);
            d.free(bi);
        }
        for (ii, i) in b.insts.iter().enumerate() {
            for u in i.uses() {
                ensure!(
                    d.contains(u.0),
                    "b{bi} の {ii} 番目の命令が、書かれていないかもしれない r{} を読む",
                    u.0
                );
            }
            if let Some(x) = i.def() {
                d.set(x.0, true);
            }
        }
        for u in b.term.uses() {
            ensure!(
                d.contains(u.0),
                "b{bi} の終わりが、書かれていないかもしれない r{} を読む",
                u.0
            );
        }
        let mut ss: Vec<usize> = b.term.succs().iter().map(|s| s.index()).collect();
        ss.dedup();
        for s in ss {
            d.meet_into(s);
        }
        d.tick();
    }
    Ok(())
}

fn verify_func_with(p: &Program, f: &Func, builtin_pairs: &HashSet<(ClassId, TypeId)>) -> VResult {
    let cx = Cx {
        p,
        f,
        builtin_pairs,
    };
    ensure!(!f.blocks.is_empty(), "ブロックが無い");
    let mut seen = Bits::new(f.regs.len());
    for &r in &f.params {
        cx.is(r, Rep::Any)?;
        ensure!(
            !seen.contains(r.index()),
            "引数のレジスタ r{} が重なる",
            r.0
        );
        seen.insert(r.index());
    }
    let entry_ok = matches!(f.blocks[0].insts.first(), Some(Inst::DepthEnter));
    ensure!(entry_ok, "入口の先頭に depth_enter が無い");
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            cx.check_inst(i, &b.insts[..ii], bi == 0)
                .map_err(|m| format!("b{bi} の {ii} 番目の命令: {m}"))?;
        }
        cx.check_term(bi, &b.term)
            .map_err(|m| format!("b{bi} の終わり: {m}"))?;
    }
    cx.check_defs()?;
    if let Some(fr) = &f.frame {
        cx.check_frame(fr)?;
    }
    Ok(())
}

/// プログラムを検証する。
pub fn verify(p: &Program) -> VResult {
    verify_from(p, &Marks::default())
}

/// 表ごとの項目の数。単位を足したプログラムで、検証を済ませた範囲の境目に使う。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// 関数。
    pub funcs: usize,
    /// 大域。
    pub globals: usize,
    /// インスタンス。
    pub instances: usize,
    /// 組み込みのインスタンスのメソッド。
    pub builtin_methods: usize,
    /// 初期化の関数。
    pub init: usize,
    /// 形。
    pub shapes: usize,
}

impl Marks {
    /// プログラム `p` の表の大きさ。
    pub fn of(p: &Program) -> Marks {
        Marks {
            funcs: p.funcs.len(),
            globals: p.globals.len(),
            instances: p.instances.len(),
            builtin_methods: p.builtin_methods.len(),
            init: p.init.len(),
            shapes: p.shapes.len(),
        }
    }
}

/// `m` より後ろの項目を検証する(単位を足したプログラム。前の項目は検証してあるものとする)。
///
/// [`verify`] と同じ規則で、関数と表の項目を `m` の位置から確かめる。
/// 前の項目は後ろの項目を指さないので、手間は足した項目の大きさに比例する。
pub fn verify_from(p: &Program, m: &Marks) -> VResult {
    ensure!(
        p.shapes.first().map(|s| s.is_empty()) == Some(true),
        "形の表の先頭が空の形でない"
    );
    for (i, s) in p.shapes.iter().enumerate().skip(m.shapes) {
        ensure!(
            s.windows(2).all(|w| w[0] <= w[1]),
            "形 {i} がラベルの番号の順に並んでいない"
        );
        for l in s {
            ensure!(l.index() < p.labels.len(), "形 {i} の範囲の外のラベル");
        }
    }
    let nf = p.funcs.len();
    let fid_ok = |f: FuncId| f.index() < nf;
    // 大域の値になる関数は、捕獲を持たず、大域の種類に合う種類を持つ(1.6 節)。
    let global_fn = |g: &Global, f: FuncId, want: FuncKind| -> VResult {
        ensure!(fid_ok(f), "大域 {} の関数が範囲の外", g.sym);
        let t = &p.funcs[f.index()];
        ensure!(
            t.ncaps == 0 && t.kind == want,
            "大域 {} の関数 {} は捕獲を持たない種類 {} の関数でない",
            g.sym,
            t.sym,
            want.name()
        );
        Ok(())
    };
    for g in p.globals.iter().skip(m.globals) {
        match &g.kind {
            GlobalKind::Builtin { func } => global_fn(g, *func, FuncKind::Builtin)?,
            GlobalKind::Fn(func) => global_fn(g, *func, FuncKind::Kir(FnKind::Lambda))?,
            GlobalKind::Dictabs(func) => global_fn(g, *func, FuncKind::Kir(FnKind::Dictabs))?,
            GlobalKind::Extern { func, .. } => {
                if let Some(func) = func {
                    global_fn(g, *func, FuncKind::Extern)?;
                }
            }
            GlobalKind::Value(_) => {}
        }
    }
    for x in p.instances.iter().skip(m.instances) {
        let cl = p
            .classes
            .get(x.class.index())
            .ok_or_else(|| format!("インスタンス {} のクラスが範囲の外", x.sym))?;
        ensure!(
            x.methods.len() == cl.methods.len(),
            "インスタンス {} の実装の数がクラス {} のメソッドの数と合わない",
            x.sym,
            cl.sym
        );
        for m in &x.methods {
            ensure!(fid_ok(m.func), "インスタンス {} の関数が範囲の外", x.sym);
            let t = &p.funcs[m.func.index()];
            ensure!(
                matches!(t.kind, FuncKind::Kir(_))
                    && t.ncaps == 0
                    && t.params.len() == m.dicts as usize,
                "インスタンス {} の実装の関数 {} は、辞書を {} 個受け取る捕獲の無い KIR の関数でない",
                x.sym,
                t.sym,
                m.dicts
            );
        }
    }
    for b in p.builtin_methods.iter().skip(m.builtin_methods) {
        ensure!(fid_ok(b.func), "組み込みのメソッドの関数が範囲の外");
        let t = &p.funcs[b.func.index()];
        ensure!(
            t.kind == FuncKind::BuiltinMethod && t.ncaps == 0,
            "組み込みのメソッドの関数 {} の種類が合わない",
            t.sym
        );
    }
    for &f in p.init.iter().skip(m.init) {
        ensure!(
            fid_ok(f) && p.funcs[f.index()].kind == FuncKind::Init,
            "初期化の関数 {} が不正",
            f.0
        );
    }
    if let Some(h) = p.rt_handle {
        ensure!(fid_ok(h), "$rt_handle の関数が範囲の外");
        let t = &p.funcs[h.index()];
        ensure!(
            t.kind == FuncKind::Handler && t.ncaps == 0 && t.params.len() == 3,
            "$rt_handle の関数 {} は引数を 3 つ取る捕獲の無い種類 handler の関数でない",
            t.sym
        );
    }
    for f in p.funcs.iter().skip(m.funcs) {
        if f.kind == FuncKind::Init {
            ensure!(
                f.params.is_empty() && f.ncaps == 0,
                "初期化の関数 {} が引数か捕獲を持つ",
                f.sym
            );
        }
    }
    let pairs = builtin_pairs(p);
    for (i, f) in p.funcs.iter().enumerate().skip(m.funcs) {
        verify_func_with(p, f, &pairs).map_err(|m| format!("関数 {i}({}): {m}", f.sym))?;
    }
    Ok(())
}
