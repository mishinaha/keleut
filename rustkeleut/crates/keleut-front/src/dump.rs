//! `keleut dump tokens` と `keleut dump ast` の出力を作る(Diktor の driver.ml の `dump_tokens_file` と、dump.ml に対応する)。
//!
//! トークンは、行番号とトークンの表記を `%4d  %s` の形式で 1 行ずつ出力する。
//! AST は S 式に変換し、OCaml の `Format` と同じ hov ボックスで整形して出力する([`crate::oformat`])。
//! 既定値のままの部分(空の型パラメータ、省略した注釈など)は出力しない。

use crate::ast::*;
use crate::escape::ocaml_escaped;
use crate::lexer::Entry;
use crate::oformat::{BoxType, Formatter};

/// トークン列を 1 トークン 1 行の形式で書いたバイト列を返す。
pub fn tokens_text(tokens: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in tokens {
        out.extend_from_slice(format!("{:4}  {}\n", e.sp.line, e.tok.show()).as_bytes());
    }
    out
}

/// S 式である。`A` はアトム、`L` はリストである。
enum Sexp {
    A(String),
    L(Vec<Sexp>),
}

use Sexp::{A, L};

fn a(s: impl Into<String>) -> Sexp {
    A(s.into())
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", ocaml_escaped(s.as_bytes()))
}

/// S 式を整形して出力する(Diktor の `pp` に対応する)。
/// 入れ子は明示的なスタックでたどり、再帰しない。
fn pp(f: &mut Formatter, s: &Sexp) {
    enum W<'a> {
        Node(&'a Sexp),
        /// リストのまだ出力していない要素と、その先頭がリストの最初の要素なら真になるフラグ。
        Rest(&'a [Sexp], bool),
    }
    let mut stack = vec![W::Node(s)];
    while let Some(w) = stack.pop() {
        match w {
            W::Node(A(s)) => f.string(s),
            W::Node(L(xs)) => {
                f.open_box(1, BoxType::Hov);
                f.string("(");
                stack.push(W::Rest(xs, true));
            }
            W::Rest(xs, first) => match xs.split_first() {
                None => {
                    f.string(")");
                    f.close_box();
                }
                Some((x, rest)) => {
                    if !first {
                        f.brk(1, 0);
                    }
                    stack.push(W::Rest(rest, false));
                    stack.push(W::Node(x));
                }
            },
        }
    }
}

fn tparam(tp: &TypeParam) -> Sexp {
    let name = if tp.arity == 0 {
        a(&tp.name)
    } else {
        L(vec![a(&tp.name), a(format!("arity={}", tp.arity))])
    };
    if tp.classes.is_empty() {
        return name;
    }
    let mut v = vec![a(":"), name];
    v.extend(tp.classes.iter().map(|c| a(c.show())));
    L(v)
}

fn ty(t: &TypeExp) -> Sexp {
    match &t.kind {
        TypeExpKind::Ident(li) => a(li.show()),
        TypeExpKind::Apply(f, args) => {
            let mut v = vec![a("tapp"), ty(f)];
            v.extend(args.iter().map(ty));
            L(v)
        }
        TypeExpKind::Arrow(params, ret, eff) => {
            let mut v = vec![a("=>"), L(params.iter().map(ty).collect()), ty(ret)];
            if let Some(e) = eff {
                v.push(a("@"));
                v.push(ty(e));
            }
            L(v)
        }
        TypeExpKind::BraceRow(elems, ext) => {
            let mut v = vec![a("row")];
            for e in elems {
                v.push(match e {
                    BraceElem::Field(l, t) => L(vec![a(format!("{l}:")), ty(t)]),
                    BraceElem::Label(li, args) if args.is_empty() => a(li.show()),
                    BraceElem::Label(li, args) => {
                        let mut w = vec![a(li.show())];
                        w.extend(args.iter().map(ty));
                        L(w)
                    }
                });
            }
            if let Some(t) = ext {
                v.push(a("extends"));
                v.push(ty(t));
            }
            L(v)
        }
        TypeExpKind::VariantCase(s, None) => a(format!("#{s}")),
        TypeExpKind::VariantCase(s, Some(t)) => L(vec![a(format!("#{s}")), ty(t)]),
        TypeExpKind::Union(ts) => {
            let mut v = vec![a("union")];
            v.extend(ts.iter().map(ty));
            L(v)
        }
        TypeExpKind::Hole => a("_"),
    }
}

fn pat(p: &Pat) -> Sexp {
    match &p.kind {
        PatKind::Wildcard => a("_"),
        PatKind::Var(x) => a(x),
        PatKind::Bool(b) => a(if *b { "true" } else { "false" }),
        PatKind::Number(n) => a(n.show()),
        PatKind::Text(s) => a(quoted(s)),
        PatKind::Record(fields, rest) => {
            let mut v = vec![a("precord")];
            v.extend(
                fields
                    .iter()
                    .map(|(l, p)| L(vec![a(format!("{l}=")), pat(p)])),
            );
            if let Some(r) = rest {
                v.push(a("..."));
                v.push(pat(r));
            }
            L(v)
        }
        PatKind::Ctor(li, args) => {
            let mut v = vec![a("pctor"), a(li.show())];
            v.extend(args.iter().map(|c| match &c.label {
                None => pat(&c.pat),
                Some(l) => L(vec![a(format!("{l}=")), pat(&c.pat)]),
            }));
            L(v)
        }
        PatKind::Variant(s, p) => L(vec![a(format!("#{s}")), pat(p)]),
        PatKind::Annot(p, t) => L(vec![a("pannot"), pat(p), ty(t)]),
    }
}

fn exp(e: &Exp) -> Sexp {
    use ExpKind::*;
    match &e.kind {
        Bool(b) => a(if *b { "true" } else { "false" }),
        Number(n) => a(n.show()),
        Text(s) => a(quoted(s)),
        Ident(li) => a(li.show()),
        Hole => a("???"),
        Apply(f, x) => L(vec![a("apply"), exp(f), exp(x)]),
        Construct(li, args) => {
            let mut v = vec![a("construct"), a(li.show())];
            v.extend(args.iter().map(|c| match &c.label {
                None => exp(&c.exp),
                Some(l) => L(vec![a(format!("{l}=")), exp(&c.exp)]),
            }));
            L(v)
        }
        Variant(s, x) => L(vec![a(format!("#{s}")), exp(x)]),
        BinOp(l, op, r) => L(vec![a(op.show()), exp(l), exp(r)]),
        Not(x) => L(vec![a("!"), exp(x)]),
        Neg(x) => L(vec![a("-"), exp(x)]),
        Lambda(l) => L(vec![
            a("fn"),
            L(l.params.iter().map(pat).collect()),
            exp(&l.body),
        ]),
        Let(b, x) => L(vec![a("let"), binding(b), exp(x)]),
        LetRec(bs, x) => L(vec![
            a("letrec"),
            L(bs.iter().map(binding).collect()),
            exp(x),
        ]),
        Seq(es) => {
            let mut v = vec![a("seq")];
            v.extend(es.iter().map(exp));
            L(v)
        }
        Match(x, cs) => {
            let mut v = vec![a("match"), exp(x)];
            v.extend(cs.iter().map(clause));
            L(v)
        }
        RecordEmpty => a("{}"),
        RecordExtend(rest, l, v) => L(vec![a("extend"), a(l), exp(v), exp(rest)]),
        RecordUpdate(r, l, v) => L(vec![a("update"), a(l), exp(v), exp(r)]),
        RecordRestriction(r, l) => L(vec![a("restrict"), a(l), exp(r)]),
        RecordSelection(r, l) => L(vec![a("select"), a(l), exp(r)]),
        Perform(li, x) => L(vec![a("perform"), a(li.show()), exp(x)]),
        Handle(x, cs) => {
            let mut v = vec![a("handle"), exp(x)];
            v.extend(cs.iter().map(clause));
            L(v)
        }
        Resume(None) => L(vec![a("resume")]),
        Resume(Some(x)) => L(vec![a("resume"), exp(x)]),
        Run(h, x) => L(vec![a("run"), a(h), exp(x)]),
    }
}

fn clause(c: &Clause) -> Sexp {
    let mut v = vec![a("case"), pat(&c.pat)];
    if let Some(g) = &c.guard {
        v.push(a("if"));
        v.push(exp(g));
    }
    v.push(a("=>"));
    v.push(exp(&c.body));
    L(v)
}

fn tparams_list(head: Option<&str>, ts: &[TypeParam]) -> Sexp {
    let mut v: Vec<Sexp> = head.map(a).into_iter().collect();
    v.extend(ts.iter().map(tparam));
    L(v)
}

fn binding(b: &LetBinding) -> Sexp {
    let mut v = vec![
        a(if b.is_pub { "binding-pub" } else { "binding" }),
        pat(&b.name),
    ];
    if !b.tparams.is_empty() {
        v.push(tparams_list(Some("tparams"), &b.tparams));
    }
    if let Some(ps) = &b.params {
        let mut w = vec![a("params")];
        w.extend(ps.iter().map(pat));
        v.push(L(w));
    }
    if let Some(t) = &b.ret {
        v.push(a(":"));
        v.push(ty(t));
    }
    if let Some(t) = &b.eff {
        v.push(a("@"));
        v.push(ty(t));
    }
    v.push(a("="));
    v.push(exp(&b.body));
    L(v)
}

fn ctor_decl(c: &CtorDecl) -> Sexp {
    let mut v = vec![a(&c.name)];
    v.extend(c.fields.iter().map(|f| match &f.label {
        None => ty(&f.ty),
        Some(l) => L(vec![a(format!("{l}:")), ty(&f.ty)]),
    }));
    L(v)
}

fn decl(d: &Decl) -> Sexp {
    match &d.kind {
        DeclKind::Type(t) => {
            let mut v = vec![a(if t.is_pub { "type-pub" } else { "type" }), a(&t.name)];
            if !t.params.is_empty() {
                v.push(tparams_list(None, &t.params));
            }
            if let Some(k) = &t.kind {
                v.push(a(format!(": {k}")));
            }
            v.push(a("="));
            v.push(ty(&t.body));
            L(v)
        }
        DeclKind::Newtype(n) => {
            let mut v = vec![
                a(if n.is_pub { "newtype-pub" } else { "newtype" }),
                a(&n.name),
            ];
            if !n.params.is_empty() {
                v.push(tparams_list(None, &n.params));
            }
            v.extend(n.ctors.iter().map(ctor_decl));
            L(v)
        }
        DeclKind::Effect(e) => {
            let mut v = vec![
                a(if e.is_pub { "effect-pub" } else { "effect" }),
                a(&e.name),
            ];
            if !e.params.is_empty() {
                v.push(tparams_list(None, &e.params));
            }
            v.extend(
                e.ops
                    .iter()
                    .map(|(op, t)| L(vec![a(format!("{op}:")), ty(t)])),
            );
            L(v)
        }
        DeclKind::Class(c) => {
            let mut v = vec![a(if c.is_pub { "class-pub" } else { "class" }), a(&c.name)];
            v.push(tparams_list(None, &c.params));
            for cv in &c.vals {
                let mut w = vec![a("val"), a(&cv.name)];
                if !cv.tparams.is_empty() {
                    w.push(tparams_list(None, &cv.tparams));
                }
                w.push(a(":"));
                w.push(ty(&cv.ty));
                v.push(L(w));
            }
            L(v)
        }
        DeclKind::Instance(i) => {
            let mut v = vec![a("instance")];
            if !i.tparams.is_empty() {
                v.push(tparams_list(Some("tparams"), &i.tparams));
            }
            v.push(a(&i.class));
            v.push(L(i.args.iter().map(ty).collect()));
            v.extend(i.body.iter().map(decl));
            L(v)
        }
        DeclKind::Let(b) => L(vec![a("dlet"), binding(b)]),
        DeclKind::LetRec(bs) => {
            let mut v = vec![a("dletrec")];
            v.extend(bs.iter().map(binding));
            L(v)
        }
        DeclKind::Module(is_pub, name, ds) => {
            let mut v = vec![a(if *is_pub { "module-pub" } else { "module" }), a(name)];
            v.extend(ds.iter().map(decl));
            L(v)
        }
        DeclKind::Extern(e) => {
            let mut v = vec![
                a(if e.is_pub { "extern-pub" } else { "extern" }),
                a(quoted(&e.abi)),
                a(&e.name),
            ];
            if !e.tparams.is_empty() {
                v.push(tparams_list(Some("tparams"), &e.tparams));
            }
            let mut w = vec![a("params")];
            w.extend(e.params.iter().map(pat));
            v.push(L(w));
            if let Some(t) = &e.ret {
                v.push(a(":"));
                v.push(ty(t));
            }
            if let Some(t) = &e.eff {
                v.push(a("@"));
                v.push(ty(t));
            }
            L(v)
        }
        DeclKind::Exp(e) => L(vec![a("exp"), exp(e)]),
    }
}

/// import 文を 1 文 1 行の形式で書いたバイト列を返す(Diktor の `dump_imports` に対応する)。
/// Diktor が `Printf` で書く行なので、`Format` の整形を通さない。
pub fn import_lines(imports: &[ImportDecl]) -> Vec<u8> {
    let mut out = Vec::new();
    for im in imports {
        let names: Vec<String> = im
            .names
            .iter()
            .map(|n| format!("({})", n.path.join(" ")))
            .collect();
        out.extend_from_slice(
            format!("(import \"{}\" {})\n", im.source, names.join(" ")).as_bytes(),
        );
    }
    out
}

/// 宣言 1 つを S 式に変換して整形し、末尾の改行まで含めた出力を返す(Diktor の `dump_decls` の 1 回分に対応する)。
pub fn decl_text(f: &mut Formatter, d: &Decl) -> Vec<u8> {
    let s = decl(d);
    pp(f, &s);
    f.newline();
    f.take()
}
