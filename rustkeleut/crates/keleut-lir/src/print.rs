//! LIR のテキスト出力(`keleut dump lir`)。
//!
//! 出力の形式は LIR を調べるためのもので、互換性を保証しない。
//! 命令の名前は、`doc/lir.md` の節の見出しと同じである。

use std::fmt::Write;

use crate::ir::*;

fn regs(rs: &[Reg]) -> String {
    rs.iter()
        .map(|r| format!("r{}", r.0))
        .collect::<Vec<_>>()
        .join(" ")
}

fn bytes(s: &[u8]) -> String {
    let mut o = String::from("\"");
    for &c in s {
        match c {
            b'"' => o.push_str("\\\""),
            b'\\' => o.push_str("\\\\"),
            0x20..=0x7e => o.push(c as char),
            _ => {
                let _ = write!(o, "\\x{c:02x}");
            }
        }
    }
    o.push('"');
    o
}

struct P<'a> {
    p: &'a Program,
}

impl P<'_> {
    fn func(&self, f: FuncId) -> String {
        match self.p.funcs.get(f.index()) {
            Some(x) => format!("#{} {}", f.0, bytes(x.sym.as_bytes())),
            None => format!("#{}", f.0),
        }
    }

    fn global(&self, g: GlobalId) -> String {
        match self.p.globals.get(g.index()) {
            Some(x) => bytes(x.sym.as_bytes()),
            None => format!("?{}", g.0),
        }
    }

    fn label(&self, l: LabelId) -> String {
        self.p
            .labels
            .get(l.index())
            .cloned()
            .unwrap_or_else(|| format!("?{}", l.0))
    }

    fn class(&self, c: ClassId, m: u32) -> String {
        match self.p.classes.get(c.index()) {
            Some(x) => format!(
                "{} {}",
                bytes(x.sym.as_bytes()),
                x.methods.get(m as usize).cloned().unwrap_or_default()
            ),
            None => format!("?{} {m}", c.0),
        }
    }

    fn inst_name(&self, i: InstanceId) -> String {
        match self.p.instances.get(i.index()) {
            Some(x) => bytes(x.sym.as_bytes()),
            None => format!("?{}", i.0),
        }
    }

    fn ty(&self, t: TypeId) -> String {
        match self.p.types.get(t.index()) {
            Some(x) => bytes(x.sym.as_bytes()),
            None => format!("?{}", t.0),
        }
    }

    fn konst(&self, c: &Const) -> String {
        match c {
            Const::Unit => "unit".into(),
            Const::Bool(b) => format!("bool {b}"),
            Const::I32(n) => format!("i32 {n}"),
            Const::I64(n) => format!("i64 {n}"),
            Const::F64(b) => format!("f64 0x{b:016x}"),
            Const::Str(s) => format!(
                "str {}",
                self.p
                    .strings
                    .get(s.index())
                    .map(|x| bytes(x))
                    .unwrap_or_default()
            ),
            Const::Ctor0 { ty, ctor } => format!("ctor0 {} {ctor}", self.ty(*ty)),
            Const::DictBuiltin { class, ty } => format!(
                "dict_builtin {} {}",
                self.p
                    .classes
                    .get(class.index())
                    .map(|c| bytes(c.sym.as_bytes()))
                    .unwrap_or_default(),
                self.ty(*ty)
            ),
            Const::DictInst0(i) => format!("dict_inst0 {}", self.inst_name(*i)),
        }
    }

    fn inst(&self, f: &Func, i: &Inst) -> String {
        use Inst::*;
        let d = |r: &Reg| format!("r{}:{} = ", r.0, f.regs[r.index()].name());
        let r = |r: &Reg| format!("r{}", r.0);
        let y = |b: &bool| if *b { "yield" } else { "noyield" };
        match i {
            Const { dst, value } => format!("{}const {}", d(dst), self.konst(value)),
            Move { dst, src } => format!("{}move {}", d(dst), r(src)),
            Box { dst, src } => format!("{}box {}", d(dst), r(src)),
            Unbox { dst, src } => format!("{}unbox {}", d(dst), r(src)),
            EnvGet { dst, index } => format!("{}env_get {index}", d(dst)),
            DepthEnter => "depth_enter".into(),
            DepthCheck => "depth_check".into(),
            DepthPush => "depth_push".into(),
            DepthRestore => "depth_restore".into(),
            IntArith { ty, op, dst, a, b } => {
                format!("{}{}.{} {} {}", d(dst), ty.name(), op.name(), r(a), r(b))
            }
            IntNeg { ty, dst, a } => format!("{}{}.neg {}", d(dst), ty.name(), r(a)),
            IntCmp { ty, op, dst, a, b } => {
                format!("{}{}.{} {} {}", d(dst), ty.name(), op.name(), r(a), r(b))
            }
            FloatArith { op, dst, a, b } => {
                format!("{}f64.{} {} {}", d(dst), op.name(), r(a), r(b))
            }
            FloatNeg { dst, a } => format!("{}f64.neg {}", d(dst), r(a)),
            FloatCmp { op, dst, a, b } => format!("{}f64.{} {} {}", d(dst), op.name(), r(a), r(b)),
            FloatMath { f: m, dst, a } => format!("{}f64.{} {}", d(dst), m.name(), r(a)),
            FloatTrunc { dst, a } => format!("{}f64.trunc {}", d(dst), r(a)),
            FloatIsNan { dst, a } => format!("{}f64.is_nan {}", d(dst), r(a)),
            Conv { op, dst, a } => format!("{}conv.{} {}", d(dst), op.name(), r(a)),
            BoolNot { dst, a } => format!("{}bool.not {}", d(dst), r(a)),
            BoolEq { dst, a, b } => format!("{}bool.eq {} {}", d(dst), r(a), r(b)),
            StrConcat { dst, a, b } => format!("{}str.concat {} {}", d(dst), r(a), r(b)),
            StrCmp { op, dst, a, b } => format!("{}str.{} {} {}", d(dst), op.name(), r(a), r(b)),
            StrLen { dst, a } => format!("{}str.len {}", d(dst), r(a)),
            StrSub { dst, s, pos, len } => {
                format!("{}str.sub {} {} {}", d(dst), r(s), r(pos), r(len))
            }
            Show { op, dst, a } => format!("{}show.{} {}", d(dst), op.name(), r(a)),
            CtorNew {
                dst,
                ty,
                ctor,
                fields,
            } => format!(
                "{}ctor_new {} {ctor} ({})",
                d(dst),
                self.ty(*ty),
                regs(fields)
            ),
            CtorTag { dst, v } => format!("{}ctor_tag {}", d(dst), r(v)),
            CtorField { dst, v, index } => format!("{}ctor_field {} {index}", d(dst), r(v)),
            VariantNew {
                dst,
                label,
                payload,
            } => format!(
                "{}variant_new {} {}",
                d(dst),
                self.label(*label),
                r(payload)
            ),
            VariantTag { dst, v } => format!("{}variant_tag {}", d(dst), r(v)),
            VariantPayload { dst, v } => format!("{}variant_payload {}", d(dst), r(v)),
            RecNew { dst, shape, fields } => {
                format!("{}rec_new shape{} ({})", d(dst), shape.0, regs(fields))
            }
            RecGet { dst, rec, index } => format!("{}rec_get {} {index}", d(dst), r(rec)),
            RecExtend {
                dst,
                rec,
                label,
                value,
            } => format!(
                "{}rec_extend {} {} {}",
                d(dst),
                r(rec),
                self.label(*label),
                r(value)
            ),
            RecField {
                dst,
                rec,
                label,
                nth,
            } => format!(
                "{}rec_field {} {} {nth}",
                d(dst),
                r(rec),
                self.label(*label)
            ),
            RecHas {
                dst,
                rec,
                label,
                nth,
            } => format!("{}rec_has {} {} {nth}", d(dst), r(rec), self.label(*label)),
            RecRestrict { dst, rec, label } => {
                format!("{}rec_restrict {} {}", d(dst), r(rec), self.label(*label))
            }
            RecUpdate {
                dst,
                rec,
                label,
                value,
            } => format!(
                "{}rec_update {} {} {}",
                d(dst),
                r(rec),
                self.label(*label),
                r(value)
            ),
            ClosureNew { dst, func, caps } => {
                format!(
                    "{}closure_new {} ({})",
                    d(dst),
                    self.func(*func),
                    regs(caps)
                )
            }
            ClosureSetCap { clo, index, value } => {
                format!("closure_set_cap {} {index} {}", r(clo), r(value))
            }
            MethodNew {
                dst,
                class,
                method,
                dict,
                own,
            } => format!(
                "{}method_new {} {} ({})",
                d(dst),
                self.class(*class, *method),
                r(dict),
                regs(own)
            ),
            DictInst {
                dst,
                inst,
                premises,
            } => format!(
                "{}dict_inst {} ({})",
                d(dst),
                self.inst_name(*inst),
                regs(premises)
            ),
            DictRecord { dst, fields } | DictVariant { dst, fields } => {
                let name = if matches!(i, DictRecord { .. }) {
                    "dict_record"
                } else {
                    "dict_variant"
                };
                let fs = fields
                    .iter()
                    .map(|(l, x)| format!("{} {}", self.label(*l), r(x)))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{}{name} ({fs})", d(dst))
            }
            GlobalGet { dst, global } => format!("{}global_get {}", d(dst), self.global(*global)),
            GlobalIsInit { dst, global } => {
                format!("{}global_is_init {}", d(dst), self.global(*global))
            }
            GlobalSet { global, src } => format!("global_set {} {}", self.global(*global), r(src)),
            GlobalMarkInit { global } => format!("global_mark_init {}", self.global(*global)),
            InstIsInit { dst, inst } => {
                format!("{}inst_is_init {}", d(dst), self.inst_name(*inst))
            }
            InstMarkInit { inst } => format!("inst_mark_init {}", self.inst_name(*inst)),
            InstSetValue { inst, method, src } => {
                format!(
                    "inst_set_value {} {method} {}",
                    self.inst_name(*inst),
                    r(src)
                )
            }
            InstGetValue { dst, inst, method } => {
                format!(
                    "{}inst_get_value {} {method}",
                    d(dst),
                    self.inst_name(*inst)
                )
            }
            RefNew { dst, value } => format!("{}ref_new {}", d(dst), r(value)),
            RefGet { dst, r: x } => format!("{}ref_get {}", d(dst), r(x)),
            RefSet { r: x, value } => format!("ref_set {} {}", r(x), r(value)),
            ArrayLen { dst, a } => format!("{}array_len {}", d(dst), r(a)),
            ArrayGet { dst, a, index } => format!("{}array_get {} {}", d(dst), r(a), r(index)),
            MArrayNew { dst, len, init } => {
                format!("{}marray_new {} {}", d(dst), r(len), r(init))
            }
            MArrayLen { dst, a } => format!("{}marray_len {}", d(dst), r(a)),
            MArrayGet { dst, a, index } => format!("{}marray_get {} {}", d(dst), r(a), r(index)),
            MArraySet { a, index, value } => {
                format!("marray_set {} {} {}", r(a), r(index), r(value))
            }
            MArrayFreeze { dst, a } => format!("{}marray_freeze {}", d(dst), r(a)),
            CallDirect {
                dst,
                func,
                args,
                yields,
            } => format!(
                "{}call_direct {} {} ({})",
                d(dst),
                y(yields),
                self.func(*func),
                regs(args)
            ),
            CallValue {
                dst,
                callee,
                args,
                yields,
            } => format!(
                "{}call_value {} {} ({})",
                d(dst),
                y(yields),
                r(callee),
                regs(args)
            ),
            CallMethod {
                dst,
                class,
                method,
                dict,
                own,
                args,
                yields,
            } => format!(
                "{}call_method {} {} {} ({}) ({})",
                d(dst),
                y(yields),
                self.class(*class, *method),
                r(dict),
                regs(own),
                regs(args)
            ),
            DictApp { dst, f: g, dicts } => {
                format!("{}dict_app {} ({})", d(dst), r(g), regs(dicts))
            }
            ConsoleWrite { dst, s } => format!("{}console_write {}", d(dst), r(s)),
            UnimplementedValue { dst, what } => {
                format!("{}unimplemented_value {}", d(dst), bytes(what.as_bytes()))
            }
            Fx { op, dst, args } => {
                let head = dst.as_ref().map(d).unwrap_or_default();
                format!("{head}{}{} ({})", op.name(), self.fx_detail(op), regs(args))
            }
        }
    }

    fn effect(&self, e: u32) -> String {
        match self.p.effects.get(e as usize) {
            Some(x) => bytes(x.sym.as_bytes()),
            None => format!("?{e}"),
        }
    }

    fn effect_op(&self, e: u32, op: u32) -> String {
        let name = self
            .p
            .effects
            .get(e as usize)
            .and_then(|x| x.ops.get(op as usize))
            .map(|o| o.0.clone())
            .unwrap_or_else(|| format!("?{op}"));
        format!("{} {name}", self.effect(e))
    }

    fn fx_detail(&self, op: &FxOp) -> String {
        match op {
            FxOp::HandlerNew {
                effect,
                ops,
                has_ret,
                has_cancel,
            } => {
                let os: Vec<String> = ops
                    .iter()
                    .map(|(o, c)| format!("({} {})", self.effect_op(*effect, *o), c.name()))
                    .collect();
                format!(
                    " {} [{}]{}{}",
                    self.effect(*effect),
                    os.join(" "),
                    if *has_ret { " ret" } else { "" },
                    if *has_cancel { " cancel" } else { "" }
                )
            }
            FxOp::HandlerFind { effect, op } => format!(" {}", self.effect_op(*effect, *op)),
            FxOp::Perform { effect, op } => format!(" {}", self.effect_op(*effect, *op)),
            FxOp::ContCheck { nontail } => {
                if *nontail {
                    " ntail".into()
                } else {
                    " tail".into()
                }
            }
            FxOp::HandlerHas(part) | FxOp::HandlerGet(part) => format!(" {}", part.name()),
            _ => String::new(),
        }
    }

    fn term(&self, t: &Term) -> String {
        let r = |r: &Reg| format!("r{}", r.0);
        let y = |b: &bool| if *b { "yield" } else { "noyield" };
        match t {
            Term::Jump(b) => format!("jump b{}", b.0),
            Term::Branch { cond, then, else_ } => {
                format!("branch {} b{} b{}", r(cond), then.0, else_.0)
            }
            Term::Return(x) => format!("return {}", r(x)),
            Term::TailCallDirect { func, args, yields } => format!(
                "tail_call_direct {} {} ({})",
                y(yields),
                self.func(*func),
                regs(args)
            ),
            Term::TailCallValue {
                callee,
                args,
                yields,
            } => format!(
                "tail_call_value {} {} ({})",
                y(yields),
                r(callee),
                regs(args)
            ),
            Term::TailCallMethod {
                class,
                method,
                dict,
                own,
                args,
                yields,
            } => format!(
                "tail_call_method {} {} {} ({}) ({})",
                y(yields),
                self.class(*class, *method),
                r(dict),
                regs(own),
                regs(args)
            ),
            Term::Error { kind, operand } => {
                let detail = match kind {
                    RtError::Unbound(g) | RtError::UnimplPrim(g) => {
                        format!(" {}", self.global(*g))
                    }
                    RtError::LitRange(s) => format!(" {}", bytes(s.as_bytes())),
                    RtError::InstanceNotReady(i) => format!(" {}", self.inst_name(*i)),
                    RtError::F64ToIntNan(op) | RtError::F64ToIntRange(op) => {
                        format!(" {}", op.prim_name())
                    }
                    _ => String::new(),
                };
                let op = operand.map(|x| format!(" {}", r(&x))).unwrap_or_default();
                format!("error {}{detail}{op}", self.p.error_number(kind))
            }
            Term::Unimplemented { what } => format!("unimplemented {}", bytes(what.as_bytes())),
            Term::Fx { op, args } => {
                let detail = match op {
                    FxTermOp::TailCallClause { op } => format!(" {op}"),
                    _ => String::new(),
                };
                format!("{}{detail} ({})", op.name(), regs(args))
            }
        }
    }

    fn function(&self, out: &mut String, id: usize, f: &Func) {
        let _ = writeln!(
            out,
            "(fn #{id} {} {} {} (params {}) (caps {})",
            bytes(f.sym.as_bytes()),
            f.kind.name(),
            if f.yields { "yield" } else { "noyield" },
            regs(&f.params),
            f.ncaps
        );
        if let Some(fr) = &f.frame {
            let slots = f
                .regs
                .iter()
                .enumerate()
                .map(|(i, rep)| {
                    let s = match fr.slot[i] {
                        Slot::Any(x) => format!("a{x}"),
                        Slot::Raw(x) => format!("w{x}"),
                    };
                    format!("r{i}:{}@{s}", rep.name())
                })
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(
                out,
                "  (frame (any {}) (raw {}) (slots {slots}))",
                fr.any_slots, fr.raw_slots
            );
            for (n, (pt, live)) in fr.resume_points.iter().enumerate() {
                let y = if fr.yield_points.get(n).copied().unwrap_or(true) {
                    "yield"
                } else {
                    "noyield"
                };
                let _ = writeln!(
                    out,
                    "  (resume {n} b{} {} {y} (live {}))",
                    pt.block.0,
                    pt.index,
                    regs(&live)
                );
            }
            for (pt, live) in fr.safepoints.iter() {
                let _ = writeln!(
                    out,
                    "  (safepoint b{} {} (live {}))",
                    pt.block.0,
                    pt.index,
                    regs(&live)
                );
            }
        }
        for (bi, b) in f.blocks.iter().enumerate() {
            let _ = writeln!(out, "  b{bi}:");
            for i in &b.insts {
                let _ = writeln!(out, "    {}", self.inst(f, i));
            }
            let _ = writeln!(out, "    {}", self.term(&b.term));
        }
        out.push_str(")\n");
    }
}

/// プログラム `p` の LIR をテキストにして返す。
pub fn print(p: &Program) -> String {
    let pr = P { p };
    let mut out = String::new();
    for (i, l) in p.labels.iter().enumerate() {
        let _ = writeln!(out, "(label {i} {})", bytes(l.as_bytes()));
    }
    for (i, s) in p.shapes.iter().enumerate() {
        let ls = s.iter().map(|l| pr.label(*l)).collect::<Vec<_>>().join(" ");
        let _ = writeln!(out, "(shape {i} ({ls}))");
    }
    for (i, g) in p.globals.iter().enumerate() {
        let kind = match &g.kind {
            GlobalKind::Builtin { func } => format!("builtin {}", pr.func(*func)),
            GlobalKind::Extern {
                abi,
                name,
                dicts,
                func,
            } => format!(
                "extern {} {} {dicts} {}",
                abi.name(),
                bytes(name.as_bytes()),
                func.map(|f| pr.func(f)).unwrap_or_else(|| "none".into())
            ),
            GlobalKind::Fn(f) => format!("fn {}", pr.func(*f)),
            GlobalKind::Dictabs(f) => format!("dictabs {}", pr.func(*f)),
            GlobalKind::Value(r) => format!("value {}", r.name()),
        };
        let _ = writeln!(
            out,
            "(global {i} {} {} {kind})",
            bytes(g.sym.as_bytes()),
            bytes(g.display.as_bytes())
        );
    }
    for (i, x) in p.instances.iter().enumerate() {
        let ms = x
            .methods
            .iter()
            .map(|m| format!("({} {})", m.dicts, pr.func(m.func)))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            out,
            "(instance {i} {} {} (premises {}) {ms})",
            bytes(x.sym.as_bytes()),
            bytes(x.display.as_bytes()),
            x.premises
        );
    }
    for m in &p.builtin_methods {
        let _ = writeln!(
            out,
            "(builtin_method {} {} {})",
            pr.class(m.class, m.method),
            pr.ty(m.ty),
            pr.func(m.func)
        );
    }
    let inits = p
        .init
        .iter()
        .map(|f| pr.func(*f))
        .collect::<Vec<_>>()
        .join(" ");
    let _ = writeln!(out, "(init {inits})");
    for (i, f) in p.funcs.iter().enumerate() {
        pr.function(&mut out, i, f);
    }
    out
}
