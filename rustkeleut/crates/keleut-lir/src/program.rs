//! プログラムの全体の下ろし。
//!
//! KIR の関数を同じ添字の LIR の関数にし、その後ろに次の関数を足す。
//!
//! 1. 大域の順に、組み込みの値の本体(`Array.each` には内側のループも)と、
//!    実装名が付録 A.5 にある `extern` の本体。
//! 2. 付録 A.3 の順に、組み込みのインスタンスのメソッドの本体。
//! 3. 単位の順に、単位の初期化の関数。
//!
//! 単位の初期化の関数は、手順を順に実行する。
//! `def_value` などの式は `thunk` の関数を直に呼んで評価し、深さを増やさない
//! (初期化の手順の深さは 0。IRSpec 11.2 節)。

use kir::{GlobalKind as KG, Step};

use crate::builder::Builder;
use crate::ir::*;
use crate::lower::{Low, Prog, ProgParts, V};
use crate::numeric::{self, Prim};
use crate::pattern::Target;
use crate::wrappers;

/// 後ろに足す関数。
enum Extra {
    Builtin(String),
    EachLoop,
    Extern(Prim),
    BuiltinMethod(Prim),
    Init(usize),
    RtHandle,
}

/// 組み立てを関数にする。
fn func_of(sym: String, kind: FuncKind, yields: bool, ncaps: u32, b: Builder) -> Func {
    let (regs, params, mut blocks) = b.finish();
    crate::lower::drop_unused_env(&mut blocks, regs.len());
    Func {
        sym,
        kind,
        yields,
        ncaps,
        regs,
        params,
        blocks,
        frame: None,
    }
}

/// KIR の関数を下ろす。
fn lower_kir_fn(prog: &mut Prog, i: usize) -> Func {
    let k = prog.k;
    let f = &k.functions[i];
    let mut lw = Low::new(prog, Some((kir::FnId(i as u32), f)));
    lw.entry();
    if !crate::effect::lower_entry(&mut lw, f, i) {
        lw.exp(f.body, true);
        if lw.b.current().is_some() {
            lw.bug(format!("関数 {} の本体がブロックを終えなかった", f.sym));
        }
    }
    let b = std::mem::replace(&mut lw.b, Builder::new());
    func_of(
        f.sym.clone(),
        FuncKind::Kir(f.kind),
        f.yields,
        f.caps.len() as u32,
        b,
    )
}

/// 単位の初期化の関数を組み立てる。
fn lower_init(prog: &mut Prog, u: usize) -> Func {
    let k = prog.k;
    let unit = &k.units[u];
    let mut lw = Low::new(prog, None);
    crate::depth::enter(&mut lw.b);
    let call = |lw: &mut Low, f: kir::FnId| -> Reg {
        let dst = lw.b.reg(Rep::Any);
        let func = lw.p.func(f);
        lw.b.emit(Inst::CallDirect {
            dst,
            func,
            args: Vec::new(),
            yields: k.functions[f.index()].yields,
        });
        dst
    };
    for st in &unit.init {
        match st {
            Step::DefValue { global, func } => {
                let v = call(&mut lw, *func);
                lw.b.emit(Inst::GlobalSet {
                    global: *global,
                    src: v,
                });
                lw.b.emit(Inst::GlobalMarkInit { global: *global });
            }
            Step::DefPattern { func, globals, pat } => {
                let v = call(&mut lw, *func);
                let fail = lw.b.block();
                lw.pattern(*pat, V::any(v), fail, &Target::Globals(globals));
                let cont = lw.b.current().expect("照合がブロックを終えた");
                lw.b.switch_to(fail);
                lw.b.terminate(Term::Error {
                    kind: RtError::PatternFail,
                    operand: Some(v),
                });
                lw.b.switch_to(cont);
                for g in globals {
                    lw.b.emit(Inst::GlobalMarkInit { global: *g });
                }
            }
            Step::DefFns(gs) => {
                for g in gs {
                    lw.b.emit(Inst::GlobalMarkInit { global: *g });
                }
            }
            Step::DefDictabs(g) => lw.b.emit(Inst::GlobalMarkInit { global: *g }),
            Step::DefInstance(i) => {
                for m in &k.instances[i.index()].methods {
                    if k.functions[m.func.index()].dicts == 0 {
                        let v = call(&mut lw, m.func);
                        lw.b.emit(Inst::InstSetValue {
                            inst: *i,
                            method: m.method,
                            src: v,
                        });
                    }
                }
                lw.b.emit(Inst::InstMarkInit { inst: *i });
            }
            Step::RunExp(f) => {
                call(&mut lw, *f);
            }
        }
    }
    let unit_v = lw.b.konst(Const::Unit);
    lw.b.terminate(Term::Return(unit_v));
    let b = std::mem::replace(&mut lw.b, Builder::new());
    func_of(format!("$init:{}", unit.key), FuncKind::Init, true, 0, b)
}

/// 付録 B の文面に書く名前(単位の鍵と末尾の `#n` を除いた修飾名)。
pub(crate) fn display_name(sym: &str) -> String {
    // 単位の鍵は最後の `/` の前までである(IRSpec 3.1 節。名前の部分は `/` を含まない)。
    let rest = match sym.rfind('/') {
        Some(i) => &sym[i + 1..],
        None => sym,
    };
    match rest.rfind('#') {
        Some(i) if i + 1 < rest.len() && rest[i + 1..].bytes().all(|c| c.is_ascii_digit()) => {
            rest[..i].to_string()
        }
        _ => rest.to_string(),
    }
}

/// 単位を足していく下ろしで、前の下ろしから持ち越す状態。
#[derive(Default)]
pub(crate) struct State {
    /// ラベル、形、文字列の索引と、パス 8、9 の解析の結果と、関数の添字の対応。
    pub parts: ProgParts,
    /// 下ろし終えた KIR の表の大きさ。
    pub k: kir::verify::Marks,
}

/// パス 1〜7、9、10 を行う(パス 12 の前の LIR)。
pub(crate) fn lower_program(k: &kir::Program) -> Result<Program, String> {
    let mut lp = Program::empty();
    extend_program(k, &mut State::default(), &mut lp)?;
    Ok(lp)
}

/// KIR のプログラム `k` の、`st` が下ろし終えた範囲より後ろの項目を下ろし、`lp` の後ろに足す
/// (パス 1〜7、9、10)。
///
/// `lp` は、同じ `st` で前に下ろした LIR(初めは [`Program::empty`])である。
/// 足した単位の KIR の関数は `lp` の関数の後ろに同じ順で並べ、その後ろに足した単位の次の関数を置く。
///
/// 1. 足した大域の順に、組み込みの値と `extern` の本体(組み込みの値はプログラムの先頭の単位だけにある)。
/// 2. プログラムの先頭を下ろすときだけ、組み込みのインスタンスのメソッドの本体。
/// 3. `$rt_handle` が無く、足した関数に操作節があれば `$rt_handle`。
/// 4. 足した単位の順に、単位の初期化の関数。
///
/// 1 回で下ろすプログラム([`lower_program`])は、空の `st` と `lp` から全体を下ろしたものである。
/// 誤りを返した後の `st` と `lp` は使えない。
pub(crate) fn extend_program(
    k: &kir::Program,
    st: &mut State,
    lp: &mut Program,
) -> Result<(), String> {
    let m = st.k;
    let mut parts = std::mem::take(&mut st.parts);
    parts.labels = std::mem::take(&mut lp.labels);
    parts.shapes = std::mem::take(&mut lp.shapes);
    parts.strings = std::mem::take(&mut lp.strings);
    crate::initck::extend(k, &mut parts.init, m.functions, m.units);
    crate::effect::clause_classes(k, &mut parts.clause_class, m.functions);
    parts.global_func.resize(k.globals.len(), None);
    let nf = k.functions.len();
    let lf0 = lp.funcs.len();
    let nnew = nf - m.functions;
    parts.fn_map.truncate(m.functions);
    parts
        .fn_map
        .extend((0..nnew).map(|i| FuncId((lf0 + i) as u32)));
    let mut prog = Prog::resume(k, parts);
    let mut extras: Vec<(String, FuncKind, bool, Extra)> = Vec::new();
    let mut next = (lf0 + nnew) as u32;
    let mut alloc =
        |extras: &mut Vec<(String, FuncKind, bool, Extra)>, sym: String, kind, yields, e| {
            extras.push((sym, kind, yields, e));
            next += 1;
            FuncId(next - 1)
        };
    let mut globals = Vec::with_capacity(k.globals.len() - m.globals);
    for (gi, g) in k.globals.iter().enumerate().skip(m.globals) {
        let display = display_name(&g.sym);
        let kind = match &g.kind {
            KG::Builtin => {
                let name = g.sym.strip_prefix("builtin/").unwrap_or(&g.sym).to_string();
                let yields = matches!(name.as_str(), "Array.each" | "pinned");
                let f = alloc(
                    &mut extras,
                    format!("$builtin:{name}"),
                    FuncKind::Builtin,
                    yields,
                    Extra::Builtin(name.clone()),
                );
                if name == "Array.each" && prog.each_loop.is_none() {
                    let l = alloc(
                        &mut extras,
                        "$builtin:Array.each$loop".to_string(),
                        FuncKind::Builtin,
                        true,
                        Extra::EachLoop,
                    );
                    prog.each_loop = Some(l);
                }
                prog.global_func[gi] = Some(f);
                GlobalKind::Builtin { func: f }
            }
            KG::Extern { abi, name, dicts } => {
                let func = numeric::by_name(*abi, name).map(|p| {
                    alloc(
                        &mut extras,
                        format!("$extern:{}", g.sym),
                        FuncKind::Extern,
                        false,
                        Extra::Extern(p),
                    )
                });
                prog.global_func[gi] = func;
                GlobalKind::Extern {
                    abi: *abi,
                    name: name.clone(),
                    dicts: *dicts,
                    func,
                }
            }
            KG::Fn(f) => GlobalKind::Fn(prog.func(*f)),
            KG::Dictabs(f) => GlobalKind::Dictabs(prog.func(*f)),
            KG::Value(r) => GlobalKind::Value(*r),
        };
        globals.push(Global {
            sym: g.sym.clone(),
            display,
            kind,
        });
    }
    let find_class = |name: &str| k.classes.iter().position(|c| c.sym == name);
    let find_type = |name: &str| k.types.iter().position(|t| t.sym == name);
    let mut builtin_methods = Vec::new();
    // 組み込みのクラスと型はプログラムの先頭の単位だけにある。
    let first = m.classes == 0;
    for &(c, t, m, _) in kir::builtins::METHODS.iter().filter(|_| first) {
        let (Some(ci), Some(ti)) = (
            find_class(&format!("builtin/{c}")),
            find_type(&format!("builtin/{t}")),
        ) else {
            continue;
        };
        let Some(mi) = k.classes[ci].methods.iter().position(|x| x.name == m) else {
            continue;
        };
        let Some(p) = numeric::builtin_method(c, t, m) else {
            continue;
        };
        let f = alloc(
            &mut extras,
            format!("$builtin_method:{c}[{t}].{m}"),
            FuncKind::BuiltinMethod,
            false,
            Extra::BuiltinMethod(p),
        );
        builtin_methods.push(BuiltinMethod {
            class: ClassId(ci as u32),
            ty: TypeId(ti as u32),
            method: mi as u32,
            func: f,
        });
    }
    if prog.rt_handle.is_none()
        && prog
            .clause_class
            .iter()
            .skip(m.functions)
            .any(|c| c.is_some())
    {
        let f = alloc(
            &mut extras,
            "$rt_handle".to_string(),
            FuncKind::Handler,
            true,
            Extra::RtHandle,
        );
        prog.rt_handle = Some(f);
    }
    let mut init = Vec::new();
    for (ui, u) in k.units.iter().enumerate().skip(m.units) {
        let f = alloc(
            &mut extras,
            format!("$init:{}", u.key),
            FuncKind::Init,
            true,
            Extra::Init(ui),
        );
        init.push(f);
    }
    let mut funcs = Vec::with_capacity(next as usize - lf0);
    for i in m.functions..nf {
        funcs.push(lower_kir_fn(&mut prog, i));
    }
    for (idx, (sym, kind, yields, e)) in extras.into_iter().enumerate() {
        let me = FuncId((lf0 + nnew + idx) as u32);
        let f = match e {
            Extra::Builtin(name) => {
                let each = prog.each_loop.unwrap_or(me);
                match wrappers::builtin_body(&name, each) {
                    Some(b) => func_of(sym, kind, yields, 0, b),
                    None => {
                        let mut b = Builder::new();
                        crate::depth::enter(&mut b);
                        b.terminate(Term::Unimplemented {
                            what: format!("組み込みの値 {name}"),
                        });
                        func_of(sym, kind, yields, 0, b)
                    }
                }
            }
            Extra::EachLoop => func_of(sym, kind, yields, 0, wrappers::each_loop_body(me)),
            Extra::Extern(p) | Extra::BuiltinMethod(p) => {
                func_of(sym, kind, yields, 0, wrappers::prim_body(p))
            }
            Extra::Init(u) => lower_init(&mut prog, u),
            Extra::RtHandle => func_of(sym, kind, yields, 0, crate::effect::rt_handle_body(me)),
        };
        funcs.push(f);
    }
    let instances: Vec<Instance> = k
        .instances
        .iter()
        .skip(m.instances)
        .map(|x| {
            let class = &k.classes[x.class.index()];
            let mut methods: Vec<InstMethod> = Vec::with_capacity(class.methods.len());
            for (mi, _) in class.methods.iter().enumerate() {
                let im = x.methods.iter().find(|m| m.method as usize == mi);
                methods.push(match im {
                    Some(m) => InstMethod {
                        dicts: x.premises + m.dicts,
                        func: prog.func(m.func),
                    },
                    None => {
                        // 検証済みの KIR では、インスタンスはクラスのメソッドをすべて実装する。
                        prog.note_bug(format!(
                            "インスタンス {} にメソッド {mi} の実装が無い",
                            x.sym
                        ));
                        InstMethod {
                            dicts: 0,
                            func: FuncId(0),
                        }
                    }
                });
            }
            Instance {
                sym: x.sym.clone(),
                class: x.class,
                head: x.head,
                display: format!(
                    "{}[{}]",
                    display_name(&class.sym),
                    display_name(&k.types[x.head.index()].sym)
                ),
                premises: x.premises,
                methods,
            }
        })
        .collect();
    if let Some(b) = prog.bug.take() {
        return Err(b);
    }
    let types: Vec<TypeInfo> = k
        .types
        .iter()
        .skip(m.types)
        .map(|t| TypeInfo {
            sym: t.sym.clone(),
            ctors: t
                .ctors
                .iter()
                .map(|c| CtorInfo {
                    name: c.name.clone(),
                    arity: c.arity,
                })
                .collect(),
        })
        .collect();
    let classes: Vec<ClassInfo> = k
        .classes
        .iter()
        .skip(m.classes)
        .map(|c| ClassInfo {
            sym: c.sym.clone(),
            methods: c.methods.iter().map(|m| m.name.clone()).collect(),
        })
        .collect();
    let effects = k.effects.iter().skip(m.effects).map(|e| EffectInfo {
        sym: e.sym.clone(),
        display: display_name(&e.sym),
        ops: e.ops.iter().map(|o| (o.name.clone(), o.arity)).collect(),
    });
    if lf0 == 0 && m.functions == 0 {
        lp.kir_funcs = nf as u32;
    }
    lp.funcs.extend(funcs);
    lp.globals.extend(globals);
    lp.instances.extend(instances);
    lp.types.extend(types);
    lp.classes.extend(classes);
    lp.builtin_methods.extend(builtin_methods);
    lp.init.extend(init);
    lp.effects.extend(effects);
    lp.rt_handle = prog.rt_handle;
    let mut parts = prog.into_parts();
    lp.labels = std::mem::take(&mut parts.labels);
    lp.shapes = std::mem::take(&mut parts.shapes);
    lp.strings = std::mem::take(&mut parts.strings);
    st.parts = parts;
    st.k = kir::verify::Marks::of(k);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::display_name;

    #[test]
    fn 表示名は最後の区切りの後で再束縛の印を除く() {
        // 単位の鍵が `/` を含む(取り込んだ単位)ときも、Diktor の display_of_symbol と同じ名前になる。
        assert_eq!(display_name("lib/x.kel/__no_such"), "__no_such");
        assert_eq!(display_name("lib/x.kel/M.f#1"), "M.f");
        assert_eq!(display_name("main/M.__no_such"), "M.__no_such");
        assert_eq!(display_name("main/x#12"), "x");
        assert_eq!(display_name("main/x#"), "x#");
        assert_eq!(display_name("builtin/Show"), "Show");
    }
}
