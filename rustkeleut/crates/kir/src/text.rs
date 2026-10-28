//! KIR の正準のテキスト(`.kirt`)の印字と読み込み。
//!
//! 規範は IRSpec の 12 節である。
//! 印字は Diktor の第19章(`ir_text.ml`)とバイト単位で同じものを出す。
//!
//! 印字は、各構文を「キーワード、原子の列、子の列」の一様な形(`View`)に写してから、
//! 字下げの規則を当てる。
//! 字下げの規則が見るのは形だけなので、規則を 1 か所に置ける。
//!
//! 読み込みは、正準の形のほかに、字句の間の任意の空白(空白、タブ、改行、復帰)を受け付ける。
//! 文字列の綴りと短い名前の綴りは正準の形だけを受け付ける。
//! そのため、読み込んだテキストを印字し直すと、空白のほかは元と同じになる。

use std::collections::HashMap;
use std::io::Write;

use crate::error::{Error, ErrorKind, READ_LIMIT_MESSAGE, WRITE_LIMIT_MESSAGE};
use crate::model::*;

// ---------------------------------------------------------------------------
// 字句の綴り
// ---------------------------------------------------------------------------

/// 綴りが名前の字句(`[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*`)に一致するか。
pub fn is_name(s: &[u8]) -> bool {
    let head = |c: u8| c.is_ascii_alphabetic() || c == b'_';
    let body = |c: u8| head(c) || c.is_ascii_digit();
    let mut i = 0;
    loop {
        if i >= s.len() || !head(s[i]) {
            return false;
        }
        i += 1;
        while i < s.len() && body(s[i]) {
            i += 1;
        }
        if i == s.len() {
            return true;
        }
        if s[i] != b'.' {
            return false;
        }
        i += 1;
    }
}

fn escape(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    for &c in s {
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x20..=0x7e => out.push(c),
            _ => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.extend_from_slice(b"\\x");
                out.push(HEX[usize::from(c >> 4)]);
                out.push(HEX[usize::from(c & 15)]);
            }
        }
    }
    out.push(b'"');
}

// ---------------------------------------------------------------------------
// 印字
// ---------------------------------------------------------------------------

/// 原子。
enum Atom<'a> {
    Int(i64),
    Bits(u64),
    Str(&'a [u8]),
    Name(&'a str),
}

fn short(s: &str) -> Atom<'_> {
    if is_name(s.as_bytes()) {
        Atom::Name(s)
    } else {
        Atom::Str(s.as_bytes())
    }
}

fn int(n: u32) -> Atom<'static> {
    Atom::Int(i64::from(n))
}

/// 子の項目。式、辞書、パターンは印字のときに [`View`] に写す。
enum Item<'a> {
    Exp(ExpId),
    Dict(DictId),
    Pat(PatId),
    Node(View<'a>),
}

/// 一様な形の 1 つのノード。
struct View<'a> {
    kw: &'static str,
    atoms: Vec<Atom<'a>>,
    kids: Vec<Item<'a>>,
}

fn view<'a>(kw: &'static str, atoms: Vec<Atom<'a>>, kids: Vec<Item<'a>>) -> Item<'a> {
    Item::Node(View { kw, atoms, kids })
}

fn tail_atom(b: bool) -> Atom<'static> {
    Atom::Name(if b { "tail" } else { "ntail" })
}

fn yield_atom(b: bool) -> Atom<'static> {
    Atom::Name(if b { "yield" } else { "noyield" })
}

fn rep_atom(r: Rep) -> Atom<'static> {
    Atom::Name(r.name())
}

/// 印字の途中の出力をためる大きさの目安(バイト)。これを超えたら出力先に書き出す。
const SPILL_BYTES: usize = 1 << 16;

struct Printer<'a, 'w> {
    p: &'a Program,
    /// まだ出力先に書いていない分である。
    out: Vec<u8>,
    sink: &'w mut dyn Write,
    lim: usize,
}

type P<T> = Result<T, Error>;

fn bad<T>(msg: impl Into<String>) -> P<T> {
    Err(Error::new(
        ErrorKind::Internal,
        format!("KIR を印字できません: {}", msg.into()),
    ))
}

impl<'a> Printer<'a, '_> {
    /// ためた分が目安を超えていれば出力先に書き出す。
    fn spill(&mut self, force: bool) -> P<()> {
        if force || self.out.len() >= SPILL_BYTES {
            self.sink
                .write_all(&self.out)
                .map_err(|e| Error::new(ErrorKind::Output, e.to_string()))?;
            self.out.clear();
        }
        Ok(())
    }

    fn ty(&self, t: TypeId) -> P<&'a Type> {
        match self.p.types.get(t.index()) {
            Some(x) => Ok(x),
            None => bad(format!("型の添字 {} が範囲の外です", t.0)),
        }
    }

    fn ty_sym(&self, t: TypeId) -> P<Atom<'a>> {
        Ok(Atom::Str(self.ty(t)?.sym.as_bytes()))
    }

    fn ctor_name(&self, t: TypeId, c: u32) -> P<Atom<'a>> {
        match self.ty(t)?.ctors.get(c as usize) {
            Some(k) => Ok(short(&k.name)),
            None => bad(format!("コンストラクタの添字 {c} が範囲の外です")),
        }
    }

    fn eff(&self, e: EffectId) -> P<&'a Effect> {
        match self.p.effects.get(e.index()) {
            Some(x) => Ok(x),
            None => bad(format!("エフェクトの添字 {} が範囲の外です", e.0)),
        }
    }

    fn op_name(&self, e: EffectId, o: u32) -> P<Atom<'a>> {
        match self.eff(e)?.ops.get(o as usize) {
            Some(k) => Ok(short(&k.name)),
            None => bad(format!("操作の添字 {o} が範囲の外です")),
        }
    }

    fn cls(&self, c: ClassId) -> P<&'a Class> {
        match self.p.classes.get(c.index()) {
            Some(x) => Ok(x),
            None => bad(format!("クラスの添字 {} が範囲の外です", c.0)),
        }
    }

    fn meth_name(&self, c: ClassId, m: u32) -> P<Atom<'a>> {
        match self.cls(c)?.methods.get(m as usize) {
            Some(k) => Ok(short(&k.name)),
            None => bad(format!("メソッドの添字 {m} が範囲の外です")),
        }
    }

    fn inst_sym(&self, i: InstanceId) -> P<Atom<'a>> {
        match self.p.instances.get(i.index()) {
            Some(x) => Ok(Atom::Str(x.sym.as_bytes())),
            None => bad(format!("インスタンスの添字 {} が範囲の外です", i.0)),
        }
    }

    fn glob_sym(&self, g: GlobalId) -> P<Atom<'a>> {
        match self.p.globals.get(g.index()) {
            Some(x) => Ok(Atom::Str(x.sym.as_bytes())),
            None => bad(format!("大域の添字 {} が範囲の外です", g.0)),
        }
    }

    fn fn_sym(&self, f: FnId) -> P<Atom<'a>> {
        match self.p.functions.get(f.index()) {
            Some(x) => Ok(Atom::Str(x.sym.as_bytes())),
            None => bad(format!("関数の添字 {} が範囲の外です", f.0)),
        }
    }

    fn lit_view(l: &'a Lit) -> View<'a> {
        let atoms = match l {
            Lit::Bool(b) => vec![
                Atom::Name("bool"),
                Atom::Name(if *b { "true" } else { "false" }),
            ],
            Lit::I32(n) => vec![Atom::Name("i32"), Atom::Int(i64::from(*n))],
            Lit::I64(n) => vec![Atom::Name("i64"), Atom::Int(*n)],
            Lit::F64(b) => vec![Atom::Name("f64"), Atom::Bits(*b)],
            Lit::Str(s) => vec![Atom::Name("str"), Atom::Str(s)],
        };
        View {
            kw: "lit",
            atoms,
            kids: Vec::new(),
        }
    }

    fn cap_item(c: Cap) -> Item<'a> {
        match c {
            Cap::Local(i) => view("local", vec![int(i)], vec![]),
            Cap::Env(i) => view("env", vec![int(i)], vec![]),
            Cap::DParam(i) => view("dparam", vec![int(i)], vec![]),
        }
    }

    fn part_item(&self, kw: &'static str, mut atoms: Vec<Atom<'a>>, pt: &'a Part) -> P<Item<'a>> {
        atoms.push(self.fn_sym(pt.func)?);
        Ok(view(
            kw,
            atoms,
            pt.caps.iter().map(|&c| Self::cap_item(c)).collect(),
        ))
    }

    /// 項目を一様な形に写す。
    fn expand(&self, item: Item<'a>) -> P<View<'a>> {
        match item {
            Item::Node(v) => Ok(v),
            Item::Exp(id) => self.exp_view(id),
            Item::Dict(id) => self.dict_view(id),
            Item::Pat(id) => self.pat_view(id),
        }
    }

    fn is_leaf(&self, item: &Item<'a>) -> P<bool> {
        Ok(match item {
            Item::Node(v) => v.kids.is_empty(),
            Item::Exp(id) => self.exp_view(*id)?.kids.is_empty(),
            Item::Dict(id) => self.dict_view(*id)?.kids.is_empty(),
            Item::Pat(id) => self.pat_view(*id)?.kids.is_empty(),
        })
    }

    fn dict_view(&self, id: DictId) -> P<View<'a>> {
        let Some(x) = self.p.nodes.dict(id) else {
            return bad("辞書のノードがありません");
        };
        let fields = |fs: &'a [(String, DictId)]| -> Vec<Item<'a>> {
            fs.iter()
                .map(|(l, d)| view("field", vec![short(l)], vec![Item::Dict(*d)]))
                .collect()
        };
        Ok(match x {
            Dict::Inst { instance, premises } => View {
                kw: "dinst",
                atoms: vec![self.inst_sym(*instance)?],
                kids: premises.iter().map(|&d| Item::Dict(d)).collect(),
            },
            Dict::DParam(i) => View {
                kw: "dparam",
                atoms: vec![int(*i)],
                kids: vec![],
            },
            Dict::Env(i) => View {
                kw: "env",
                atoms: vec![int(*i)],
                kids: vec![],
            },
            Dict::Record(fs) => View {
                kw: "drecord",
                atoms: vec![],
                kids: fields(fs),
            },
            Dict::Variant(fs) => View {
                kw: "dvariant",
                atoms: vec![],
                kids: fields(fs),
            },
            Dict::Builtin { class, ty } => View {
                kw: "dbuiltin",
                atoms: vec![
                    Atom::Str(self.cls(*class)?.sym.as_bytes()),
                    self.ty_sym(*ty)?,
                ],
                kids: vec![],
            },
        })
    }

    fn pat_view(&self, id: PatId) -> P<View<'a>> {
        let Some(x) = self.p.nodes.pat(id) else {
            return bad("パターンのノードがありません");
        };
        Ok(match x {
            Pat::Wild => View {
                kw: "wild",
                atoms: vec![],
                kids: vec![],
            },
            Pat::Bind(i) => View {
                kw: "bind",
                atoms: vec![int(*i)],
                kids: vec![],
            },
            Pat::Lit(l) => Self::lit_view(l),
            Pat::Record { fields, tail } => {
                let mut kids: Vec<Item<'a>> = fields
                    .iter()
                    .map(|(l, q)| view("field", vec![short(l)], vec![Item::Pat(*q)]))
                    .collect();
                let kind = match tail {
                    RecordTail::Closed => "closed",
                    RecordTail::Open => "open",
                    RecordTail::Rest(q) => {
                        kids.push(Item::Pat(*q));
                        "rest"
                    }
                };
                View {
                    kw: "record",
                    atoms: vec![Atom::Name(kind)],
                    kids,
                }
            }
            Pat::Ctor { ty, ctor, fields } => View {
                kw: "ctor",
                atoms: vec![self.ty_sym(*ty)?, self.ctor_name(*ty, *ctor)?],
                kids: fields
                    .iter()
                    .map(|(i, q)| view("field", vec![int(*i)], vec![Item::Pat(*q)]))
                    .collect(),
            },
            Pat::Variant { label, pat } => View {
                kw: "variant",
                atoms: vec![short(label)],
                kids: vec![Item::Pat(*pat)],
            },
        })
    }

    fn exp_view(&self, id: ExpId) -> P<View<'a>> {
        let Some(x) = self.p.nodes.exp(id) else {
            return bad("式のノードがありません");
        };
        let v = |kw, atoms, kids| View { kw, atoms, kids };
        let e = |id: &ExpId| Item::Exp(*id);
        Ok(match x {
            Exp::Lit(l) => Self::lit_view(l),
            Exp::LitError(s) => v(
                "lit_error",
                vec![Atom::Name("lit_range"), Atom::Str(s.as_bytes())],
                vec![],
            ),
            Exp::Local(i) => v("local", vec![int(*i)], vec![]),
            Exp::Env(i) => v("env", vec![int(*i)], vec![]),
            Exp::Global(g) => v("global", vec![self.glob_sym(*g)?], vec![]),
            Exp::Construct { ty, ctor, fields } => v(
                "construct",
                vec![self.ty_sym(*ty)?, self.ctor_name(*ty, *ctor)?],
                fields
                    .iter()
                    .map(|(i, x)| view("field", vec![int(*i)], vec![Item::Exp(*x)]))
                    .collect(),
            ),
            Exp::Variant { label, value } => v("variant", vec![short(label)], vec![e(value)]),
            Exp::Call {
                tail,
                yields,
                func,
                args,
            } => v(
                "call",
                vec![tail_atom(*tail), yield_atom(*yields)],
                std::iter::once(e(func)).chain(args.iter().map(e)).collect(),
            ),
            Exp::Method {
                class,
                method,
                dict,
                own,
            } => v(
                "method",
                vec![
                    Atom::Str(self.cls(*class)?.sym.as_bytes()),
                    self.meth_name(*class, *method)?,
                ],
                std::iter::once(Item::Dict(*dict))
                    .chain(own.iter().map(|d| Item::Dict(*d)))
                    .collect(),
            ),
            Exp::Opcall {
                class,
                method,
                dict,
                args,
            } => v(
                "opcall",
                vec![
                    Atom::Str(self.cls(*class)?.sym.as_bytes()),
                    self.meth_name(*class, *method)?,
                ],
                std::iter::once(Item::Dict(*dict))
                    .chain(args.iter().map(e))
                    .collect(),
            ),
            Exp::Not(a) => v("not", vec![], vec![e(a)]),
            Exp::And(a, b) => v("and", vec![], vec![e(a), e(b)]),
            Exp::Or(a, b) => v("or", vec![], vec![e(a), e(b)]),
            Exp::Closure(pt) | Exp::Dictabs(pt) => {
                let kw = if matches!(x, Exp::Closure(_)) {
                    "closure"
                } else {
                    "dictabs"
                };
                v(
                    kw,
                    vec![self.fn_sym(pt.func)?],
                    pt.caps.iter().map(|&c| Self::cap_item(c)).collect(),
                )
            }
            Exp::Dictapp { func, dicts } => v(
                "dictapp",
                vec![],
                std::iter::once(e(func))
                    .chain(dicts.iter().map(|d| Item::Dict(*d)))
                    .collect(),
            ),
            Exp::Let { slot, value, body } => v("let", vec![int(*slot)], vec![e(value), e(body)]),
            Exp::Letpat { pat, value, body } => {
                v("letpat", vec![], vec![Item::Pat(*pat), e(value), e(body)])
            }
            Exp::Letrec { items, body } => v(
                "letrec",
                vec![],
                items
                    .iter()
                    .map(|(i, x)| view("rec", vec![int(*i)], vec![Item::Exp(*x)]))
                    .chain(std::iter::once(e(body)))
                    .collect(),
            ),
            Exp::Seq(xs) => v("seq", vec![], xs.iter().map(e).collect()),
            Exp::Match { scrutinee, clauses } => v(
                "match",
                vec![],
                std::iter::once(e(scrutinee))
                    .chain(clauses.iter().map(|c| {
                        let mut kids = vec![Item::Pat(c.pat)];
                        if let Some(g) = c.guard {
                            kids.push(view("guard", vec![], vec![Item::Exp(g)]));
                        }
                        kids.push(Item::Exp(c.body));
                        view("clause", vec![], kids)
                    }))
                    .collect(),
            ),
            Exp::RecEmpty => v("rec_empty", vec![], vec![]),
            Exp::RecExtend {
                label,
                value,
                record,
            } => v("rec_extend", vec![short(label)], vec![e(value), e(record)]),
            Exp::RecSelect { label, record } => {
                v("rec_select", vec![short(label)], vec![e(record)])
            }
            Exp::RecRestrict { label, record } => {
                v("rec_restrict", vec![short(label)], vec![e(record)])
            }
            Exp::RecUpdate {
                label,
                record,
                value,
            } => v("rec_update", vec![short(label)], vec![e(record), e(value)]),
            Exp::Perform { effect, op, args } => v(
                "perform",
                vec![
                    Atom::Str(self.eff(*effect)?.sym.as_bytes()),
                    self.op_name(*effect, *op)?,
                ],
                args.iter().map(e).collect(),
            ),
            Exp::Handle(h) => {
                let mut kids = vec![self.part_item("body", vec![], &h.body)?];
                for o in &h.ops {
                    kids.push(self.part_item(
                        "op",
                        vec![self.op_name(h.effect, o.op)?, Atom::Name(o.class.name())],
                        &o.part,
                    )?);
                }
                if let Some(pt) = &h.ret {
                    kids.push(self.part_item("return", vec![], pt)?);
                }
                if let Some(pt) = &h.cancel {
                    kids.push(self.part_item("cancel", vec![], pt)?);
                }
                v(
                    "handle",
                    vec![
                        tail_atom(h.tail),
                        Atom::Str(self.eff(h.effect)?.sym.as_bytes()),
                    ],
                    kids,
                )
            }
            Exp::Resume { tail, value } => v("resume", vec![tail_atom(*tail)], vec![e(value)]),
            Exp::Hole => v("hole", vec![], vec![]),
        })
    }

    fn atom(&mut self, a: &Atom<'_>) {
        match a {
            Atom::Int(n) => self.out.extend_from_slice(n.to_string().as_bytes()),
            Atom::Bits(b) => self.out.extend_from_slice(format!("0x{b:016x}").as_bytes()),
            Atom::Str(s) => escape(&mut self.out, s),
            Atom::Name(s) => self.out.extend_from_slice(s.as_bytes()),
        }
    }

    /// ノードを桁 `col` から印字する。`d` は式、辞書、パターンのノードの深さである。
    fn node(&mut self, item: Item<'a>, col: usize, d: usize) -> P<()> {
        let d = match item {
            Item::Node(_) => d,
            _ => d + 1,
        };
        if d > self.lim {
            return Err(Error::new(ErrorKind::WriteLimit, WRITE_LIMIT_MESSAGE));
        }
        let v = self.expand(item)?;
        self.spill(false)?;
        self.out.push(b'(');
        self.out.extend_from_slice(v.kw.as_bytes());
        for a in &v.atoms {
            self.out.push(b' ');
            self.atom(a);
        }
        let mut all_leaves = true;
        for k in &v.kids {
            if !self.is_leaf(k)? {
                all_leaves = false;
                break;
            }
        }
        for k in v.kids {
            if all_leaves {
                self.out.push(b' ');
                self.node(k, col, d)?;
            } else {
                self.out.push(b'\n');
                self.out.resize(self.out.len() + col + 2, b' ');
                self.node(k, col + 2, d)?;
            }
        }
        self.out.push(b')');
        Ok(())
    }

    fn top(&mut self, item: Item<'a>) -> P<()> {
        self.node(item, 0, 0)?;
        self.out.push(b'\n');
        Ok(())
    }

    fn function(&self, f: &'a Function) -> P<Item<'a>> {
        let reps = |kw, rs: &[Rep]| view(kw, rs.iter().map(|&r| rep_atom(r)).collect(), vec![]);
        let mut kids = vec![
            reps("params", &f.params),
            reps("locals", &f.locals),
            view("dicts", vec![int(f.dicts)], vec![]),
            reps("caps", &f.caps),
            view("result", vec![rep_atom(f.result)], vec![]),
        ];
        if f.kind == FnKind::Opclause {
            kids.push(view(
                "pats",
                vec![],
                f.pats.iter().map(|&q| Item::Pat(q)).collect(),
            ));
        } else if !f.pats.is_empty() {
            return bad(format!("操作節でない関数 {} がパターンを持ちます", f.sym));
        }
        if let Some(g) = f.guard {
            kids.push(view("guard", vec![], vec![Item::Exp(g)]));
        }
        kids.push(Item::Exp(f.body));
        Ok(view(
            "fn",
            vec![
                Atom::Str(f.sym.as_bytes()),
                Atom::Name(f.kind.name()),
                yield_atom(f.yields),
            ],
            kids,
        ))
    }

    fn step(&self, s: &'a Step) -> P<Item<'a>> {
        Ok(match s {
            Step::DefValue { global, func } => view(
                "def_value",
                vec![self.glob_sym(*global)?, self.fn_sym(*func)?],
                vec![],
            ),
            Step::DefPattern { func, globals, pat } => {
                let mut gs = Vec::new();
                for &g in globals {
                    gs.push(self.glob_sym(g)?);
                }
                view(
                    "def_pattern",
                    vec![self.fn_sym(*func)?],
                    vec![view("globals", gs, vec![]), Item::Pat(*pat)],
                )
            }
            Step::DefFns(globals) => {
                let mut gs = Vec::new();
                for &g in globals {
                    gs.push(self.glob_sym(g)?);
                }
                view("def_fns", gs, vec![])
            }
            Step::DefDictabs(g) => view("def_dictabs", vec![self.glob_sym(*g)?], vec![]),
            Step::DefInstance(i) => view("def_instance", vec![self.inst_sym(*i)?], vec![]),
            Step::RunExp(f) => view("run_exp", vec![self.fn_sym(*f)?], vec![]),
        })
    }

    fn program(&mut self) -> P<()> {
        let p = self.p;
        self.top(view(
            "kir",
            vec![
                int(u32::from(p.version.0)),
                int(u32::from(p.version.1)),
                int(0),
            ],
            vec![],
        ))?;
        for u in &p.units {
            self.top(view(
                "unit",
                vec![Atom::Str(u.key.as_bytes())],
                vec![view(
                    "paths",
                    u.paths.iter().map(|s| Atom::Str(s)).collect(),
                    vec![],
                )],
            ))?;
        }
        for t in &p.types {
            self.top(view(
                "type",
                vec![Atom::Str(t.sym.as_bytes())],
                t.ctors
                    .iter()
                    .map(|c| view("ctor", vec![short(&c.name), int(c.arity)], vec![]))
                    .collect(),
            ))?;
        }
        for e in &p.effects {
            self.top(view(
                "effect",
                vec![Atom::Str(e.sym.as_bytes())],
                e.ops
                    .iter()
                    .map(|o| {
                        view(
                            "op",
                            vec![
                                short(&o.name),
                                int(o.arity),
                                Atom::Name(if o.never { "never" } else { "resume" }),
                                Atom::Name(if o.runtime { "runtime" } else { "user" }),
                            ],
                            vec![],
                        )
                    })
                    .collect(),
            ))?;
        }
        for c in &p.classes {
            self.top(view(
                "class",
                vec![Atom::Str(c.sym.as_bytes())],
                c.methods
                    .iter()
                    .map(|m| view("method", vec![short(&m.name), int(m.dicts)], vec![]))
                    .collect(),
            ))?;
        }
        for i in &p.instances {
            let mut kids = vec![view("premises", vec![int(i.premises)], vec![])];
            for m in &i.methods {
                kids.push(view(
                    "method",
                    vec![
                        self.meth_name(i.class, m.method)?,
                        int(m.dicts),
                        self.fn_sym(m.func)?,
                    ],
                    vec![],
                ));
            }
            self.top(view(
                "instance",
                vec![
                    Atom::Str(i.sym.as_bytes()),
                    Atom::Str(self.cls(i.class)?.sym.as_bytes()),
                    self.ty_sym(i.head)?,
                ],
                kids,
            ))?;
        }
        for g in &p.globals {
            let mut atoms = vec![Atom::Str(g.sym.as_bytes())];
            match &g.kind {
                GlobalKind::Builtin => atoms.push(Atom::Name("builtin")),
                GlobalKind::Extern { abi, name, dicts } => {
                    atoms.push(Atom::Name("extern"));
                    atoms.push(Atom::Name(abi.name()));
                    atoms.push(Atom::Str(name.as_bytes()));
                    atoms.push(int(*dicts));
                }
                GlobalKind::Fn(f) => {
                    atoms.push(Atom::Name("fn"));
                    atoms.push(self.fn_sym(*f)?);
                }
                GlobalKind::Dictabs(f) => {
                    atoms.push(Atom::Name("dictabs"));
                    atoms.push(self.fn_sym(*f)?);
                }
                GlobalKind::Value(r) => {
                    atoms.push(Atom::Name("value"));
                    atoms.push(rep_atom(*r));
                }
            }
            self.top(view("global", atoms, vec![]))?;
        }
        for f in &p.functions {
            let item = self.function(f)?;
            self.top(item)?;
        }
        for u in &p.units {
            let mut kids = Vec::new();
            for s in &u.init {
                kids.push(self.step(s)?);
            }
            self.top(view("init", vec![Atom::Str(u.key.as_bytes())], kids))?;
        }
        Ok(())
    }
}

/// プログラムを正準のテキストにして返す。`max_nest` はノードの深さの上限である。
///
/// 深さに比例してスタックを使うので、深いプログラムは大きなスタックの上で呼ぶ
/// ([`crate::write_kirt`] はそうする)。
/// テキストの大きさは入れ子の深さの 2 乗に比例して増えうる(字下げのため)。
/// 深さ `N_nest` 近くでは数十 GB になりうるので、大きなプログラムには [`print_to`] を使う。
pub fn print(p: &Program, max_nest: usize) -> Result<Vec<u8>, Error> {
    let mut v = Vec::new();
    print_to(p, max_nest, &mut v)?;
    Ok(v)
}

/// プログラムを正準のテキストにして `w` に書く。
///
/// 使うメモリは出力の大きさでなく、入れ子の深さに比例する。
/// `w` に書けなければ [`ErrorKind::Output`] を返す。
/// それまでに書いた分は `w` に残る。
/// 深さの限度は何かを書く前に確かめるが、表の範囲の外の添字([`ErrorKind::Internal`])は途中で見つかりうる。
pub fn print_to(p: &Program, max_nest: usize, w: &mut dyn Write) -> Result<(), Error> {
    // 限度を超える深さのプログラムは、何かを印字する前に拒否する(字下げで大きさが深さの 2 乗になるため)
    crate::binary::check_write_depth(p, max_nest)?;
    let mut pr = Printer {
        p,
        out: Vec::with_capacity(SPILL_BYTES * 2),
        sink: w,
        lim: max_nest,
    };
    pr.program()?;
    pr.spill(true)?;
    pr.sink
        .flush()
        .map_err(|e| Error::new(ErrorKind::Output, e.to_string()))
}

// ---------------------------------------------------------------------------
// 読み込み
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Open,
    Close,
    Int(i128),
    Bits(u64),
    Str(Vec<u8>),
    Name(String),
    Eof,
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

struct Fail {
    line: usize,
    msg: String,
    limit: bool,
}

type T<X> = Result<X, Fail>;

impl Lexer<'_> {
    fn fail<X>(&self, msg: impl Into<String>) -> T<X> {
        Err(Fail {
            line: self.line,
            msg: msg.into(),
            limit: false,
        })
    }

    fn skip_ws(&mut self) {
        while self.pos < self.src.len() {
            match self.src[self.pos] {
                b'\n' => self.line += 1,
                b' ' | b'\t' | b'\r' => {}
                _ => return,
            }
            self.pos += 1;
        }
    }

    fn next(&mut self) -> T<Tok> {
        self.skip_ws();
        let s = self.src;
        let Some(&c) = s.get(self.pos) else {
            return Ok(Tok::Eof);
        };
        match c {
            b'(' => {
                self.pos += 1;
                Ok(Tok::Open)
            }
            b')' => {
                self.pos += 1;
                Ok(Tok::Close)
            }
            b'"' => {
                self.pos += 1;
                let mut out = Vec::new();
                loop {
                    let Some(&b) = s.get(self.pos) else {
                        return self.fail("文字列が閉じていません");
                    };
                    self.pos += 1;
                    match b {
                        b'"' => return Ok(Tok::Str(out)),
                        b'\\' => match s.get(self.pos) {
                            Some(b'"') => {
                                out.push(b'"');
                                self.pos += 1;
                            }
                            Some(b'\\') => {
                                out.push(b'\\');
                                self.pos += 1;
                            }
                            Some(b'x') => {
                                let hex = |c: u8| match c {
                                    b'0'..=b'9' => Some(c - b'0'),
                                    b'a'..=b'f' => Some(c - b'a' + 10),
                                    _ => None,
                                };
                                let (Some(h), Some(l)) = (
                                    s.get(self.pos + 1).and_then(|&c| hex(c)),
                                    s.get(self.pos + 2).and_then(|&c| hex(c)),
                                ) else {
                                    return self.fail(
                                        "文字列のエスケープ \\x の後ろに小文字の 16 進の 2 桁がありません",
                                    );
                                };
                                let v = h * 16 + l;
                                if (0x20..=0x7e).contains(&v) {
                                    return self.fail(
                                        "文字列のエスケープが正準の形ではありません(0x20 から 0x7E のバイトはそのまま書きます)",
                                    );
                                }
                                out.push(v);
                                self.pos += 3;
                            }
                            _ => return self.fail("文字列に定義されていないエスケープがあります"),
                        },
                        0x20..=0x7e => out.push(b),
                        _ => {
                            return self.fail(format!(
                                "文字列にエスケープしていないバイト 0x{b:02x} があります"
                            ));
                        }
                    }
                }
            }
            b'0' if s.get(self.pos + 1) == Some(&b'x') => {
                let start = self.pos + 2;
                let mut end = start;
                while end < s.len() && s[end].is_ascii_alphanumeric() {
                    end += 1;
                }
                let digits = &s[start..end];
                if digits.len() != 16
                    || !digits
                        .iter()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
                {
                    return self.fail("ビット列は 0x に続く小文字の 16 進の 16 桁です");
                }
                let mut v = 0u64;
                for &c in digits {
                    let h = if c.is_ascii_digit() {
                        c - b'0'
                    } else {
                        c - b'a' + 10
                    };
                    v = (v << 4) | u64::from(h);
                }
                self.pos = end;
                Ok(Tok::Bits(v))
            }
            b'-' | b'0'..=b'9' => {
                let start = self.pos;
                let mut end = start + usize::from(c == b'-');
                let dstart = end;
                while end < s.len() && s[end].is_ascii_digit() {
                    end += 1;
                }
                let digits = &s[dstart..end];
                if digits.is_empty()
                    || (digits[0] == b'0' && (digits.len() > 1 || c == b'-'))
                    || digits.len() > 30
                    || s.get(end)
                        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
                {
                    return self.fail("整数の綴りが正しくありません");
                }
                let text = std::str::from_utf8(&s[start..end]).unwrap_or("");
                let Ok(v) = text.parse::<i128>() else {
                    return self.fail("整数の綴りが正しくありません");
                };
                self.pos = end;
                Ok(Tok::Int(v))
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.pos;
                let mut end = start;
                while end < s.len()
                    && (s[end].is_ascii_alphanumeric() || s[end] == b'_' || s[end] == b'.')
                {
                    end += 1;
                }
                let word = &s[start..end];
                if !is_name(word) {
                    return self.fail("名前の綴りが正しくありません");
                }
                self.pos = end;
                Ok(Tok::Name(String::from_utf8_lossy(word).into_owned()))
            }
            _ => self.fail(format!("定義されていないバイト 0x{c:02x} があります")),
        }
    }
}

/// 表の名前から添字を引く索引。同じ名前が 2 つあれば、名前から項目を決められないので誤りにする。
struct Index {
    map: HashMap<String, u32>,
}

impl Index {
    fn build<'x>(what: &str, names: impl Iterator<Item = &'x str>) -> Result<Index, String> {
        let mut map = HashMap::new();
        for (i, n) in names.enumerate() {
            if map.insert(n.to_string(), i as u32).is_some() {
                return Err(format!("{what}の名前が重なります: {n}"));
            }
        }
        Ok(Index { map })
    }

    fn get(&self, n: &str) -> Option<u32> {
        self.map.get(n).copied()
    }
}

struct Parser<'a> {
    lx: Lexer<'a>,
    peeked: Vec<Tok>,
    lim: usize,
    nodes: Nodes,
    types: Vec<Type>,
    effects: Vec<Effect>,
    classes: Vec<Class>,
    instances: Vec<Instance>,
    globals: Vec<Global>,
    type_ix: Index,
    ctor_ix: Vec<Index>,
    eff_ix: Index,
    op_ix: Vec<Index>,
    cls_ix: Index,
    meth_ix: Vec<Index>,
    inst_ix: Index,
    glob_ix: Index,
    fn_ix: Index,
}

fn empty_index() -> Index {
    Index {
        map: HashMap::new(),
    }
}

impl Parser<'_> {
    fn fail<X>(&self, msg: impl Into<String>) -> T<X> {
        self.lx.fail(msg)
    }

    fn peek(&mut self, k: usize) -> T<&Tok> {
        while self.peeked.len() <= k {
            let t = self.lx.next()?;
            self.peeked.push(t);
        }
        Ok(&self.peeked[k])
    }

    fn next(&mut self) -> T<Tok> {
        if self.peeked.is_empty() {
            self.lx.next()
        } else {
            Ok(self.peeked.remove(0))
        }
    }

    /// 次が `(kw` なら真。
    fn at(&mut self, kw: &str) -> T<bool> {
        if *self.peek(0)? != Tok::Open {
            return Ok(false);
        }
        Ok(matches!(self.peek(1)?, Tok::Name(n) if n == kw))
    }

    /// `(` とキーワードを読み、キーワードを返す。
    fn open(&mut self) -> T<String> {
        match self.next()? {
            Tok::Open => {}
            _ => return self.fail("( がありません"),
        }
        match self.next()? {
            Tok::Name(n) => Ok(n),
            _ => self.fail("( の後ろにキーワードがありません"),
        }
    }

    fn open_kw(&mut self, kw: &str) -> T<()> {
        let k = self.open()?;
        if k != kw {
            return self.fail(format!("{kw} を期待しましたが {k} があります"));
        }
        Ok(())
    }

    fn close(&mut self) -> T<()> {
        match self.next()? {
            Tok::Close => Ok(()),
            _ => self.fail(") がありません"),
        }
    }

    fn is_close(&mut self) -> T<bool> {
        Ok(*self.peek(0)? == Tok::Close)
    }

    fn word(&mut self) -> T<String> {
        match self.next()? {
            Tok::Name(n) => Ok(n),
            _ => self.fail("キーワードがありません"),
        }
    }

    fn word_of(&mut self, what: &str, words: &[&str]) -> T<usize> {
        let w = self.word()?;
        match words.iter().position(|x| *x == w) {
            Some(i) => Ok(i),
            None => self.fail(format!("{what}の綴り {w} は定義されていません")),
        }
    }

    fn uint(&mut self) -> T<u32> {
        match self.next()? {
            Tok::Int(n) if (0..=i128::from(u32::MAX)).contains(&n) => Ok(n as u32),
            Tok::Int(_) => self.fail("整数が u32 の範囲にありません"),
            _ => self.fail("整数がありません"),
        }
    }

    fn bytes(&mut self) -> T<Vec<u8>> {
        match self.next()? {
            Tok::Str(s) => Ok(s),
            _ => self.fail("文字列がありません"),
        }
    }

    /// strings の表に入る名前の文字列(記号、単位の鍵、実装名、字面)。
    fn sym(&mut self) -> T<String> {
        let b = self.bytes()?;
        match String::from_utf8(b) {
            Ok(s) => Ok(s),
            Err(_) => self.fail("名前が UTF-8 ではありません"),
        }
    }

    /// 短い名前(名前の字句に合えば裸の名前、合わなければ文字列)。
    fn short(&mut self) -> T<String> {
        match self.next()? {
            Tok::Name(n) => Ok(n),
            Tok::Str(s) => {
                if is_name(&s) {
                    return self.fail("名前の字句に合う短い名前を文字列で書いています");
                }
                match String::from_utf8(s) {
                    Ok(s) => Ok(s),
                    Err(_) => self.fail("名前が UTF-8 ではありません"),
                }
            }
            _ => self.fail("名前がありません"),
        }
    }

    fn rep(&mut self) -> T<Rep> {
        let i = self.word_of("Rep", &["bool", "i32", "i64", "f64", "str", "any"])?;
        Ok(Rep::ALL[i])
    }

    fn bool_word(&mut self, yes: &str, no: &str) -> T<bool> {
        Ok(self.word_of("印", &[no, yes])? == 1)
    }

    fn lookup(&self, what: &str, ix: &Index, n: &str) -> T<u32> {
        match ix.get(n) {
            Some(i) => Ok(i),
            None => self.fail(format!("{what}が表にありません: {n}")),
        }
    }

    fn ty_ref(&mut self) -> T<TypeId> {
        let s = self.sym()?;
        Ok(TypeId(self.lookup("型", &self.type_ix, &s)?))
    }

    fn eff_ref(&mut self) -> T<EffectId> {
        let s = self.sym()?;
        Ok(EffectId(self.lookup("エフェクト", &self.eff_ix, &s)?))
    }

    fn cls_ref(&mut self) -> T<ClassId> {
        let s = self.sym()?;
        Ok(ClassId(self.lookup("クラス", &self.cls_ix, &s)?))
    }

    fn inst_ref(&mut self) -> T<InstanceId> {
        let s = self.sym()?;
        Ok(InstanceId(self.lookup(
            "インスタンス",
            &self.inst_ix,
            &s,
        )?))
    }

    fn glob_ref(&mut self) -> T<GlobalId> {
        let s = self.sym()?;
        Ok(GlobalId(self.lookup("大域", &self.glob_ix, &s)?))
    }

    fn fn_ref(&mut self) -> T<FnId> {
        let s = self.sym()?;
        Ok(FnId(self.lookup("関数", &self.fn_ix, &s)?))
    }

    fn ctor_ref(&mut self, t: TypeId) -> T<u32> {
        let n = self.short()?;
        self.lookup("コンストラクタ", &self.ctor_ix[t.index()], &n)
    }

    fn op_ref(&mut self, e: EffectId) -> T<u32> {
        let n = self.short()?;
        self.lookup("操作", &self.op_ix[e.index()], &n)
    }

    fn meth_ref(&mut self, c: ClassId) -> T<u32> {
        let n = self.short()?;
        self.lookup("メソッド", &self.meth_ix[c.index()], &n)
    }

    fn nest(&self, d: usize) -> T<()> {
        if d > self.lim {
            return Err(Fail {
                line: self.lx.line,
                msg: READ_LIMIT_MESSAGE.to_string(),
                limit: true,
            });
        }
        Ok(())
    }

    /// `(lit …)` の `lit` を読んだ後の続き。
    fn lit_rest(&mut self) -> T<Lit> {
        let k = self.word_of("リテラルの種類", &["bool", "i32", "i64", "f64", "str"])?;
        let l = match k {
            0 => Lit::Bool(self.bool_word("true", "false")?),
            1 => match self.next()? {
                Tok::Int(n) if (i128::from(i32::MIN)..=i128::from(i32::MAX)).contains(&n) => {
                    Lit::I32(n as i32)
                }
                _ => return self.fail("i32 のリテラルの値がありません"),
            },
            2 => match self.next()? {
                Tok::Int(n) if (i128::from(i64::MIN)..=i128::from(i64::MAX)).contains(&n) => {
                    Lit::I64(n as i64)
                }
                _ => return self.fail("i64 のリテラルの値がありません"),
            },
            3 => match self.next()? {
                Tok::Bits(b) => Lit::F64(b),
                _ => return self.fail("f64 のリテラルのビット列がありません"),
            },
            _ => Lit::Str(self.bytes()?),
        };
        self.close()?;
        Ok(l)
    }

    fn cap(&mut self) -> T<Cap> {
        let k = self.open()?;
        let i = self.uint()?;
        self.close()?;
        match k.as_str() {
            "local" => Ok(Cap::Local(i)),
            "env" => Ok(Cap::Env(i)),
            "dparam" => Ok(Cap::DParam(i)),
            _ => self.fail(format!("捕獲のキーワード {k} は定義されていません")),
        }
    }

    fn caps(&mut self) -> T<Vec<Cap>> {
        let mut v = Vec::new();
        while !self.is_close()? {
            v.push(self.cap()?);
        }
        self.close()?;
        Ok(v)
    }

    /// `(kw SYM cap*)` のうち、キーワードの後ろから読む。
    fn part_rest(&mut self) -> T<Part> {
        let func = self.fn_ref()?;
        let caps = self.caps()?;
        Ok(Part { func, caps })
    }

    fn dict(&mut self, d: usize) -> T<DictId> {
        self.nest(d)?;
        let k = self.open()?;
        let x = match k.as_str() {
            "dinst" => {
                let instance = self.inst_ref()?;
                let mut premises = Vec::new();
                while !self.is_close()? {
                    premises.push(self.dict(d + 1)?);
                }
                Dict::Inst { instance, premises }
            }
            "dparam" => Dict::DParam(self.uint()?),
            "env" => Dict::Env(self.uint()?),
            "drecord" | "dvariant" => {
                let mut fs = Vec::new();
                while !self.is_close()? {
                    self.open_kw("field")?;
                    let l = self.short()?;
                    let x = self.dict(d + 1)?;
                    self.close()?;
                    fs.push((l, x));
                }
                if k == "drecord" {
                    Dict::Record(fs)
                } else {
                    Dict::Variant(fs)
                }
            }
            "dbuiltin" => {
                let class = self.cls_ref()?;
                let ty = self.ty_ref()?;
                Dict::Builtin { class, ty }
            }
            _ => return self.fail(format!("辞書の式のキーワード {k} は定義されていません")),
        };
        self.close()?;
        Ok(self.nodes.push_dict(x))
    }

    fn pat(&mut self, d: usize) -> T<PatId> {
        self.nest(d)?;
        let k = self.open()?;
        let x = match k.as_str() {
            "wild" => Pat::Wild,
            "bind" => Pat::Bind(self.uint()?),
            "lit" => {
                let l = self.lit_rest()?;
                return Ok(self.nodes.push_pat(Pat::Lit(l)));
            }
            "record" => {
                let tk = self.word_of("レコードのパターンの尾部", &["closed", "open", "rest"])?;
                let mut fields = Vec::new();
                while self.at("field")? {
                    self.open_kw("field")?;
                    let l = self.short()?;
                    let q = self.pat(d + 1)?;
                    self.close()?;
                    fields.push((l, q));
                }
                let tail = match tk {
                    0 => RecordTail::Closed,
                    1 => RecordTail::Open,
                    _ => RecordTail::Rest(self.pat(d + 1)?),
                };
                Pat::Record { fields, tail }
            }
            "ctor" => {
                let ty = self.ty_ref()?;
                let ctor = self.ctor_ref(ty)?;
                let mut fields = Vec::new();
                while !self.is_close()? {
                    self.open_kw("field")?;
                    let i = self.uint()?;
                    let q = self.pat(d + 1)?;
                    self.close()?;
                    fields.push((i, q));
                }
                Pat::Ctor { ty, ctor, fields }
            }
            "variant" => {
                let label = self.short()?;
                let pat = self.pat(d + 1)?;
                Pat::Variant { label, pat }
            }
            _ => return self.fail(format!("パターンのキーワード {k} は定義されていません")),
        };
        self.close()?;
        Ok(self.nodes.push_pat(x))
    }

    fn exps_until_close(&mut self, d: usize) -> T<Vec<ExpId>> {
        let mut v = Vec::new();
        while !self.is_close()? {
            v.push(self.exp(d)?);
        }
        Ok(v)
    }

    /// 式のノードを読む。
    ///
    /// 再帰の 1 段のスタックを小さく保つため、構文ごとの読み方を別の関数に分け、
    /// ここではキーワードから関数を選んで呼ぶだけにする。
    /// 各関数は閉じ括弧まで読む。
    fn exp(&mut self, d: usize) -> T<ExpId> {
        self.nest(d)?;
        let k = self.open()?;
        let arm: fn(&mut Self, usize, &str) -> T<Exp> = match k.as_str() {
            "lit" => Self::px_lit,
            "lit_error" | "local" | "env" | "global" | "rec_empty" | "hole" => Self::px_leaf,
            "construct" => Self::px_construct,
            "variant" | "not" | "rec_select" | "rec_restrict" | "resume" => Self::px_unary,
            "call" => Self::px_call,
            "method" | "opcall" => Self::px_method,
            "and" | "or" | "let" | "rec_extend" | "rec_update" => Self::px_binary,
            "closure" | "dictabs" => Self::px_part,
            "dictapp" => Self::px_dictapp,
            "letpat" => Self::px_letpat,
            "letrec" => Self::px_letrec,
            "seq" => Self::px_seq,
            "match" => Self::px_match,
            "perform" => Self::px_perform,
            "handle" => Self::px_handle,
            _ => return self.fail(format!("式のキーワード {k} は定義されていません")),
        };
        let x = arm(self, d + 1, &k)?;
        Ok(self.nodes.push_exp(x))
    }

    fn px_lit(&mut self, _n: usize, _k: &str) -> T<Exp> {
        Ok(Exp::Lit(self.lit_rest()?))
    }

    fn px_leaf(&mut self, _n: usize, k: &str) -> T<Exp> {
        let x = match k {
            "lit_error" => {
                self.word_of("範囲外のリテラルの種類", &["lit_range"])?;
                Exp::LitError(self.sym()?)
            }
            "local" => Exp::Local(self.uint()?),
            "env" => Exp::Env(self.uint()?),
            "global" => Exp::Global(self.glob_ref()?),
            "rec_empty" => Exp::RecEmpty,
            _ => Exp::Hole,
        };
        self.close()?;
        Ok(x)
    }

    fn px_construct(&mut self, n: usize, _k: &str) -> T<Exp> {
        let ty = self.ty_ref()?;
        let ctor = self.ctor_ref(ty)?;
        let mut fields = Vec::new();
        while !self.is_close()? {
            self.open_kw("field")?;
            let i = self.uint()?;
            let x = self.exp(n)?;
            self.close()?;
            fields.push((i, x));
        }
        self.close()?;
        Ok(Exp::Construct { ty, ctor, fields })
    }

    fn px_unary(&mut self, n: usize, k: &str) -> T<Exp> {
        let x = match k {
            "variant" => {
                let label = self.short()?;
                Exp::Variant {
                    label,
                    value: self.exp(n)?,
                }
            }
            "not" => Exp::Not(self.exp(n)?),
            "rec_select" => {
                let label = self.short()?;
                Exp::RecSelect {
                    label,
                    record: self.exp(n)?,
                }
            }
            "rec_restrict" => {
                let label = self.short()?;
                Exp::RecRestrict {
                    label,
                    record: self.exp(n)?,
                }
            }
            _ => {
                let tail = self.bool_word("tail", "ntail")?;
                Exp::Resume {
                    tail,
                    value: self.exp(n)?,
                }
            }
        };
        self.close()?;
        Ok(x)
    }

    fn px_call(&mut self, n: usize, _k: &str) -> T<Exp> {
        let tail = self.bool_word("tail", "ntail")?;
        let yields = self.bool_word("yield", "noyield")?;
        let func = self.exp(n)?;
        let args = self.exps_until_close(n)?;
        self.close()?;
        Ok(Exp::Call {
            tail,
            yields,
            func,
            args,
        })
    }

    fn px_method(&mut self, n: usize, k: &str) -> T<Exp> {
        let class = self.cls_ref()?;
        let method = self.meth_ref(class)?;
        let dict = self.dict(n)?;
        let x = if k == "method" {
            let mut own = Vec::new();
            while !self.is_close()? {
                own.push(self.dict(n)?);
            }
            Exp::Method {
                class,
                method,
                dict,
                own,
            }
        } else {
            let args = self.exps_until_close(n)?;
            Exp::Opcall {
                class,
                method,
                dict,
                args,
            }
        };
        self.close()?;
        Ok(x)
    }

    fn px_binary(&mut self, n: usize, k: &str) -> T<Exp> {
        let label = if k == "rec_extend" || k == "rec_update" {
            self.short()?
        } else {
            String::new()
        };
        let slot = if k == "let" { self.uint()? } else { 0 };
        let a = self.exp(n)?;
        let b = self.exp(n)?;
        self.close()?;
        Ok(match k {
            "and" => Exp::And(a, b),
            "or" => Exp::Or(a, b),
            "let" => Exp::Let {
                slot,
                value: a,
                body: b,
            },
            "rec_extend" => Exp::RecExtend {
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

    fn px_part(&mut self, _n: usize, k: &str) -> T<Exp> {
        let pt = self.part_rest()?;
        Ok(if k == "closure" {
            Exp::Closure(pt)
        } else {
            Exp::Dictabs(pt)
        })
    }

    fn px_dictapp(&mut self, n: usize, _k: &str) -> T<Exp> {
        let func = self.exp(n)?;
        let mut dicts = Vec::new();
        while !self.is_close()? {
            dicts.push(self.dict(n)?);
        }
        self.close()?;
        Ok(Exp::Dictapp { func, dicts })
    }

    fn px_letpat(&mut self, n: usize, _k: &str) -> T<Exp> {
        let pat = self.pat(n)?;
        let value = self.exp(n)?;
        let body = self.exp(n)?;
        self.close()?;
        Ok(Exp::Letpat { pat, value, body })
    }

    fn px_letrec(&mut self, n: usize, _k: &str) -> T<Exp> {
        let mut items = Vec::new();
        while self.at("rec")? {
            self.open_kw("rec")?;
            let i = self.uint()?;
            let x = self.exp(n)?;
            self.close()?;
            items.push((i, x));
        }
        if items.is_empty() {
            return self.fail("letrec の項目がありません");
        }
        let body = self.exp(n)?;
        self.close()?;
        Ok(Exp::Letrec { items, body })
    }

    fn px_seq(&mut self, n: usize, _k: &str) -> T<Exp> {
        let xs = self.exps_until_close(n)?;
        if xs.len() < 2 {
            return self.fail("seq の要素が 2 個より少なくなっています");
        }
        self.close()?;
        Ok(Exp::Seq(xs))
    }

    fn px_match(&mut self, n: usize, _k: &str) -> T<Exp> {
        let scrutinee = self.exp(n)?;
        let mut clauses = Vec::new();
        while !self.is_close()? {
            self.open_kw("clause")?;
            let pat = self.pat(n)?;
            let guard = if self.at("guard")? {
                self.open_kw("guard")?;
                let g = self.exp(n)?;
                self.close()?;
                Some(g)
            } else {
                None
            };
            let body = self.exp(n)?;
            self.close()?;
            clauses.push(Clause { pat, guard, body });
        }
        self.close()?;
        Ok(Exp::Match { scrutinee, clauses })
    }

    fn px_perform(&mut self, n: usize, _k: &str) -> T<Exp> {
        let effect = self.eff_ref()?;
        let op = self.op_ref(effect)?;
        let args = self.exps_until_close(n)?;
        self.close()?;
        Ok(Exp::Perform { effect, op, args })
    }

    fn px_handle(&mut self, _n: usize, _k: &str) -> T<Exp> {
        let tail = self.bool_word("tail", "ntail")?;
        let effect = self.eff_ref()?;
        self.open_kw("body")?;
        let body = self.part_rest()?;
        let mut ops = Vec::new();
        while self.at("op")? {
            self.open_kw("op")?;
            let op = self.op_ref(effect)?;
            let class = ClauseClass::ALL[self.word_of("操作節の分類", &["N", "T", "G"])?];
            let part = self.part_rest()?;
            ops.push(HandlerOp { op, class, part });
        }
        if ops.is_empty() {
            return self.fail("ハンドラ式の操作節がありません");
        }
        let ret = if self.at("return")? {
            self.open_kw("return")?;
            Some(self.part_rest()?)
        } else {
            None
        };
        let cancel = if self.at("cancel")? {
            self.open_kw("cancel")?;
            Some(self.part_rest()?)
        } else {
            None
        };
        self.close()?;
        Ok(Exp::Handle(Box::new(Handle {
            tail,
            effect,
            body,
            ops,
            ret,
            cancel,
        })))
    }

    fn reps(&mut self, kw: &str) -> T<Vec<Rep>> {
        self.open_kw(kw)?;
        let mut v = Vec::new();
        while !self.is_close()? {
            v.push(self.rep()?);
        }
        self.close()?;
        Ok(v)
    }

    fn function(&mut self) -> T<Function> {
        self.open_kw("fn")?;
        let sym = self.sym()?;
        let kind = FnKind::ALL[self.word_of(
            "関数の種類",
            &[
                "lambda",
                "opclause",
                "retclause",
                "cancelclause",
                "handlebody",
                "thunk",
                "dictabs",
            ],
        )?];
        let yields = self.bool_word("yield", "noyield")?;
        let params = self.reps("params")?;
        let locals = self.reps("locals")?;
        self.open_kw("dicts")?;
        let dicts = self.uint()?;
        self.close()?;
        let caps = self.reps("caps")?;
        self.open_kw("result")?;
        let result = self.rep()?;
        self.close()?;
        let mut pats = Vec::new();
        if kind == FnKind::Opclause {
            self.open_kw("pats")?;
            while !self.is_close()? {
                pats.push(self.pat(1)?);
            }
            self.close()?;
        }
        let guard = if kind == FnKind::Opclause && self.at("guard")? {
            self.open_kw("guard")?;
            let g = self.exp(1)?;
            self.close()?;
            Some(g)
        } else {
            None
        };
        let body = self.exp(1)?;
        self.close()?;
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

    fn globs_until_close(&mut self) -> T<Vec<GlobalId>> {
        let mut v = Vec::new();
        while !self.is_close()? {
            v.push(self.glob_ref()?);
        }
        Ok(v)
    }

    fn step(&mut self) -> T<Step> {
        let k = self.open()?;
        let s = match k.as_str() {
            "def_value" => {
                let global = self.glob_ref()?;
                let func = self.fn_ref()?;
                Step::DefValue { global, func }
            }
            "def_pattern" => {
                let func = self.fn_ref()?;
                self.open_kw("globals")?;
                let globals = self.globs_until_close()?;
                self.close()?;
                let pat = self.pat(1)?;
                Step::DefPattern { func, globals, pat }
            }
            "def_fns" => {
                let gs = self.globs_until_close()?;
                if gs.is_empty() {
                    return self.fail("def_fns の大域がありません");
                }
                Step::DefFns(gs)
            }
            "def_dictabs" => Step::DefDictabs(self.glob_ref()?),
            "def_instance" => Step::DefInstance(self.inst_ref()?),
            "run_exp" => Step::RunExp(self.fn_ref()?),
            _ => return self.fail(format!("初期化の手順のキーワード {k} は定義されていません")),
        };
        self.close()?;
        Ok(s)
    }

    fn index<'x>(&self, what: &str, names: impl Iterator<Item = &'x str>) -> T<Index> {
        match Index::build(what, names) {
            Ok(ix) => Ok(ix),
            Err(m) => self.fail(m),
        }
    }

    fn program(&mut self, fn_syms: Vec<String>) -> T<Program> {
        self.open_kw("kir")?;
        let major = self.uint()?;
        let minor = self.uint()?;
        let features = self.uint()?;
        self.close()?;
        if major != u32::from(VERSION.0) {
            return self.fail(format!(
                "版 {major}.{minor} は読めません(読めるのは主版 {} の版だけです)",
                VERSION.0
            ));
        }
        // 副版はどの値も受け付けるが、バイナリの見出しの u16 に収まらなければ書けない。
        if minor > u32::from(u16::MAX) {
            return self.fail(format!("副版 {minor} が u16 の範囲にありません"));
        }
        if features != 0 {
            return self.fail(format!(
                "必要な機能のビット 0x{features:08x} を処理できません"
            ));
        }
        self.fn_ix = self.index("関数", fn_syms.iter().map(|s| s.as_str()))?;
        let mut units: Vec<Unit> = Vec::new();
        while self.at("unit")? {
            self.open_kw("unit")?;
            let key = self.sym()?;
            self.open_kw("paths")?;
            let mut paths = Vec::new();
            while !self.is_close()? {
                paths.push(self.bytes()?);
            }
            self.close()?;
            self.close()?;
            units.push(Unit {
                key,
                paths,
                init: Vec::new(),
            });
        }
        while self.at("type")? {
            self.open_kw("type")?;
            let sym = self.sym()?;
            let mut ctors = Vec::new();
            while !self.is_close()? {
                self.open_kw("ctor")?;
                let name = self.short()?;
                let arity = self.uint()?;
                self.close()?;
                ctors.push(Ctor { name, arity });
            }
            self.close()?;
            self.types.push(Type { sym, ctors });
        }
        self.type_ix = self.index("型", self.types.iter().map(|t| t.sym.as_str()))?;
        let mut ctor_ix = Vec::new();
        for t in &self.types {
            ctor_ix.push(self.index("コンストラクタ", t.ctors.iter().map(|c| c.name.as_str()))?);
        }
        self.ctor_ix = ctor_ix;
        while self.at("effect")? {
            self.open_kw("effect")?;
            let sym = self.sym()?;
            let mut ops = Vec::new();
            while !self.is_close()? {
                self.open_kw("op")?;
                let name = self.short()?;
                let arity = self.uint()?;
                let never = self.bool_word("never", "resume")?;
                let runtime = self.bool_word("runtime", "user")?;
                self.close()?;
                ops.push(Op {
                    name,
                    arity,
                    never,
                    runtime,
                });
            }
            self.close()?;
            self.effects.push(Effect { sym, ops });
        }
        self.eff_ix = self.index("エフェクト", self.effects.iter().map(|e| e.sym.as_str()))?;
        let mut op_ix = Vec::new();
        for e in &self.effects {
            op_ix.push(self.index("操作", e.ops.iter().map(|o| o.name.as_str()))?);
        }
        self.op_ix = op_ix;
        while self.at("class")? {
            self.open_kw("class")?;
            let sym = self.sym()?;
            let mut methods = Vec::new();
            while !self.is_close()? {
                self.open_kw("method")?;
                let name = self.short()?;
                let dicts = self.uint()?;
                self.close()?;
                methods.push(ClassMethod { name, dicts });
            }
            self.close()?;
            self.classes.push(Class { sym, methods });
        }
        self.cls_ix = self.index("クラス", self.classes.iter().map(|c| c.sym.as_str()))?;
        let mut meth_ix = Vec::new();
        for c in &self.classes {
            meth_ix.push(self.index("メソッド", c.methods.iter().map(|m| m.name.as_str()))?);
        }
        self.meth_ix = meth_ix;
        while self.at("instance")? {
            self.open_kw("instance")?;
            let sym = self.sym()?;
            let class = self.cls_ref()?;
            let head = self.ty_ref()?;
            self.open_kw("premises")?;
            let premises = self.uint()?;
            self.close()?;
            let mut methods = Vec::new();
            while !self.is_close()? {
                self.open_kw("method")?;
                let method = self.meth_ref(class)?;
                let dicts = self.uint()?;
                let func = self.fn_ref()?;
                self.close()?;
                methods.push(Impl {
                    method,
                    dicts,
                    func,
                });
            }
            self.close()?;
            self.instances.push(Instance {
                sym,
                class,
                head,
                premises,
                methods,
            });
        }
        self.inst_ix = self.index(
            "インスタンス",
            self.instances.iter().map(|i| i.sym.as_str()),
        )?;
        while self.at("global")? {
            self.open_kw("global")?;
            let sym = self.sym()?;
            let k = self.word_of(
                "大域の種類",
                &["builtin", "extern", "fn", "dictabs", "value"],
            )?;
            let kind = match k {
                0 => GlobalKind::Builtin,
                1 => {
                    let abi = Abi::ALL[self.word_of("ABI", &["prim", "C"])?];
                    let name = self.sym()?;
                    let dicts = self.uint()?;
                    GlobalKind::Extern { abi, name, dicts }
                }
                2 => GlobalKind::Fn(self.fn_ref()?),
                3 => GlobalKind::Dictabs(self.fn_ref()?),
                _ => GlobalKind::Value(self.rep()?),
            };
            self.close()?;
            self.globals.push(Global { sym, kind });
        }
        self.glob_ix = self.index("大域", self.globals.iter().map(|g| g.sym.as_str()))?;
        let mut functions = Vec::new();
        while self.at("fn")? {
            let f = self.function()?;
            if functions.len() >= fn_syms.len() || fn_syms[functions.len()] != f.sym {
                return self.fail("関数の記号の下読みと本読みが合いません");
            }
            functions.push(f);
        }
        if functions.len() != fn_syms.len() {
            return self.fail("関数の記号の下読みと本読みが合いません");
        }
        let mut i = 0;
        while self.at("init")? {
            self.open_kw("init")?;
            let key = self.sym()?;
            if i >= units.len() || units[i].key != key {
                return self.fail(format!(
                    "init {key} が units の {} 番目の単位の初期化ではありません",
                    i + 1
                ));
            }
            let mut steps = Vec::new();
            while !self.is_close()? {
                steps.push(self.step()?);
            }
            self.close()?;
            units[i].init = steps;
            i += 1;
        }
        if i != units.len() {
            return self.fail("init の数が単位の数と違います");
        }
        if *self.peek(0)? != Tok::Eof {
            return self.fail("ファイルの終わりの前に、置けない最上位のノードがあります");
        }
        Ok(Program {
            version: (major as u16, minor as u16),
            units,
            types: std::mem::take(&mut self.types),
            effects: std::mem::take(&mut self.effects),
            classes: std::mem::take(&mut self.classes),
            instances: std::mem::take(&mut self.instances),
            globals: std::mem::take(&mut self.globals),
            functions,
            nodes: std::mem::take(&mut self.nodes),
        })
    }
}

/// 最上位の `(fn "記号"` を下読みして、関数の記号の列を返す。
/// 大域とインスタンスと関数の本体は、後に現れる関数を記号で指すからである。
fn prescan_fns(src: &[u8]) -> T<Vec<String>> {
    let mut lx = Lexer {
        src,
        pos: 0,
        line: 1,
    };
    let mut depth = 0usize;
    let mut out = Vec::new();
    loop {
        match lx.next()? {
            Tok::Eof => return Ok(out),
            Tok::Open => {
                depth += 1;
                if depth == 1 {
                    let t = lx.next()?;
                    if t == Tok::Name("fn".to_string()) {
                        match lx.next()? {
                            Tok::Str(s) => match String::from_utf8(s) {
                                Ok(s) => out.push(s),
                                Err(_) => return lx.fail("名前が UTF-8 ではありません"),
                            },
                            Tok::Open => depth += 1,
                            Tok::Close => depth -= 1,
                            _ => {}
                        }
                    } else if t == Tok::Open {
                        depth += 1;
                    } else if t == Tok::Close {
                        depth -= 1;
                    }
                }
            }
            Tok::Close => {
                if depth == 0 {
                    return lx.fail("対応しない ) があります");
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

/// テキストを読む。`max_nest` はノードの深さの上限(IRSpec 13.9 節の `N_nest`)である。
///
/// 深さに比例してスタックを使うので、深い入力を読むときは大きなスタックの上で呼ぶ
/// ([`crate::read_kirt`] はそうする)。
pub fn parse(src: &[u8], max_nest: usize) -> Result<Program, Error> {
    let r = prescan_fns(src).and_then(|syms| {
        let mut p = Parser {
            lx: Lexer {
                src,
                pos: 0,
                line: 1,
            },
            peeked: Vec::new(),
            lim: max_nest,
            nodes: Nodes::default(),
            types: Vec::new(),
            effects: Vec::new(),
            classes: Vec::new(),
            instances: Vec::new(),
            globals: Vec::new(),
            type_ix: empty_index(),
            ctor_ix: Vec::new(),
            eff_ix: empty_index(),
            op_ix: Vec::new(),
            cls_ix: empty_index(),
            meth_ix: Vec::new(),
            inst_ix: empty_index(),
            glob_ix: empty_index(),
            fn_ix: empty_index(),
        };
        p.program(syms)
    });
    r.map_err(|f| {
        Error::new(
            if f.limit {
                ErrorKind::Limit
            } else {
                ErrorKind::Syntax
            },
            format!("{} 行目: {}", f.line, f.msg),
        )
    })
}
