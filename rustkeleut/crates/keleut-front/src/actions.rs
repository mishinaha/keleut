//! 規則の意味の動作と脱糖である(Diktor の parser.mly の §3.3〜§3.10 の補助関数と、各規則の動作)。
//!
//! 動作は規則の綴り(`tables.rs` の `Prod::key`)で引く。
//! menhir の `%inline` は規則を展開するので、同じ動作を持つ綴りが複数ある。
//!
//! 意味の値の列(`cons` で前に足していく列)は、逆順の `Vec` で持つ。
//! `x :: l` は末尾への `push` で、使うときに逆にする。
//! こうすると、右再帰で長い列を作っても仕事が列の長さに比例する。
//!
//! 意味の動作が 2 つ以上の誤りを投げうるときは、Diktor(OCaml)の評価の順に合わせる。
//! OCaml は関数の引数を右から評価するので、`f a (g b)` では `g b` の誤りが先に出る。

use crate::ast::*;
use crate::pos::{Pos, Span};
use crate::tables::PRODS;
use crate::token::{Tok, ocaml_int_of_string_opt};
use std::sync::OnceLock;

/// 意味の動作の失敗である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActErr {
    /// 位置を付けた構文エラー(Diktor の `Syntax_error_at`)。
    Syntax(Pos, String),
    /// 内部エラー(Diktor が受け皿の外の例外で終わる場合)。
    Internal(String),
    /// 構文解析の限度を超えた。
    Limit,
}

type Res = Result<Sv, ActErr>;

fn syntax_error<T>(pos: Pos, msg: impl Into<String>) -> Result<T, ActErr> {
    Err(ActErr::Syntax(pos, msg.into()))
}

/// 構文解析の文脈である(ノードの番号を振り、木の深さを数える)。
///
/// ノードの番号は文脈ごとに 0 から振る。
/// 複数のファイルや REPL の入力を 1 つの文脈で続けて解析すると、番号はそれらの全体で重ならない。
/// 限度を超えたという印は解析ごとに消すので、限度で失敗した後の文脈も次の解析に使える。
#[derive(Debug)]
pub struct Ctx {
    next_id: NodeId,
    /// 構文解析の限度(診断に出す値)。
    max_nest: usize,
    /// 木の深さの限度(`max_nest` を `u32` に収めた値)。
    limit: u32,
    /// 限度を超える深さのノードを作ったか。
    pub(crate) over: bool,
}

impl Ctx {
    /// 番号を 0 から振る文脈。`limit` は構文解析の限度(作る木の深さの限度)である。
    pub fn new(limit: usize) -> Ctx {
        Ctx {
            next_id: 0,
            max_nest: limit,
            limit: limit.min(u32::MAX as usize - 1) as u32,
            over: false,
        }
    }

    /// 構文解析の限度である。
    pub fn limit(&self) -> usize {
        self.max_nest
    }

    /// 次に振るノードの番号である。
    pub fn next_id(&self) -> NodeId {
        self.next_id
    }

    fn meta_with(&mut self, id: NodeId, span: Span, child: u32) -> Meta {
        let depth = child.saturating_add(1);
        if depth > self.limit {
            self.over = true;
        }
        Meta { id, span, depth }
    }

    fn meta(&mut self, (start, finish): (Pos, Pos), child: u32) -> Meta {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.meta_with(id, Span { start, finish }, child)
    }

    fn exp(&mut self, sloc: (Pos, Pos), kind: ExpKind) -> Exp {
        let meta = self.meta(sloc, kind.child_depth());
        Exp { meta, kind }
    }

    /// 既にある注釈を借りて式を作る(深さは作り直す)。
    fn reuse_exp(&mut self, m: Meta, kind: ExpKind) -> Exp {
        let meta = self.meta_with(m.id, m.span, kind.child_depth());
        Exp { meta, kind }
    }

    fn pat(&mut self, sloc: (Pos, Pos), kind: PatKind) -> Pat {
        let meta = self.meta(sloc, kind.child_depth());
        Pat { meta, kind }
    }

    fn ty(&mut self, sloc: (Pos, Pos), kind: TypeExpKind) -> TypeExp {
        let meta = self.meta(sloc, kind.child_depth());
        TypeExp { meta, kind }
    }

    fn decl(&mut self, sloc: (Pos, Pos), kind: DeclKind) -> Decl {
        let meta = self.meta(sloc, kind.child_depth());
        Decl { meta, kind }
    }

    fn clause(&mut self, sloc: (Pos, Pos), pat: Pat, guard: Option<Exp>, body: Exp) -> Clause {
        let mut c = Clause {
            meta: Meta {
                id: 0,
                span: Span::default(),
                depth: 0,
            },
            pat,
            guard,
            body,
        };
        c.meta = self.meta(sloc, c.child_depth());
        c
    }

    #[allow(clippy::too_many_arguments)]
    fn binding(
        &mut self,
        sloc: (Pos, Pos),
        name: Pat,
        tparams: Vec<TypeParam>,
        params: Option<Vec<Pat>>,
        ret: Option<TypeExp>,
        eff: Option<TypeExp>,
        body: Exp,
    ) -> LetBinding {
        let mut b = LetBinding {
            meta: Meta {
                id: 0,
                span: Span::default(),
                depth: 0,
            },
            is_pub: false,
            name,
            tparams,
            params,
            ret,
            eff,
            body,
        };
        b.meta = self.meta(sloc, b.child_depth());
        b
    }
}

/// `newtype` の右辺の生の形である。
#[derive(Debug)]
pub enum RhsRaw {
    None,
    Short(Vec<FieldDecl>),
    Ctors(Vec<CtorDecl>),
}

/// 末尾ブロックの位置に来た、末尾ブロックにならない波括弧の形である。
#[derive(Debug)]
pub enum TrailingOther {
    Record,
    Params(Pos),
    Annot(Pos),
}

/// `extern` の頭(名前、型パラメータ、引数、返り値の型とエフェクト)である。
type ExternSigT = (
    String,
    Vec<TypeParam>,
    Vec<Pat>,
    (Option<TypeExp>, Option<TypeExp>),
);

/// レコードのパターンの欄と残りである。
type RpItemsT = (Vec<(String, Pat)>, Option<Pat>);

/// 意味の値である。列は逆順に持つ(モジュールの説明)。
#[derive(Debug)]
pub enum Sv {
    None,
    Tok(Tok),
    Str(String),
    OptStr(Option<String>),
    /// 正順の列(`upper_path`)。
    Strs(Vec<String>),
    LongId(LongId),
    Int(u32),
    Exp(Box<Exp>),
    Exps(Vec<Exp>),
    OptExp(Box<Option<Exp>>),
    Pat(Box<Pat>),
    Pats(Vec<Pat>),
    Ty(Box<TypeExp>),
    Tys(Vec<TypeExp>),
    OptTy(Box<Option<TypeExp>>),
    TyPair(Box<(Option<TypeExp>, Option<TypeExp>)>),
    ArrowRet(Box<(TypeExp, Option<TypeExp>)>),
    TyItems(Box<(Vec<TypeExp>, bool)>),
    BraceElem(Box<BraceElem>),
    BraceElems(Vec<BraceElem>),
    EffName(Box<(LongId, Vec<TypeExp>)>),
    Decl(Box<Decl>),
    Decls(Vec<Decl>),
    DeclKind(Box<DeclKind>),
    Binding(Box<LetBinding>),
    Bindings(Vec<LetBinding>),
    Header(Box<(Vec<ImportDecl>, Vec<Decl>)>),
    Import(Box<ImportDecl>),
    ImportName(Box<ImportName>),
    ImportNames(Vec<ImportName>),
    TParam(Box<TypeParam>),
    TParams(Vec<TypeParam>),
    Classes(Vec<LongId>),
    ClassVal(Box<ClassVal>),
    ClassVals(Vec<ClassVal>),
    Rhs(Box<RhsRaw>),
    Ctor(Box<CtorDecl>),
    Ctors(Vec<CtorDecl>),
    Field(Box<FieldDecl>),
    Fields(Vec<FieldDecl>),
    EffOp(Box<(String, TypeExp)>),
    EffOps(Vec<(String, TypeExp)>),
    ExternSig(Box<ExternSigT>),
    Arg(Box<(Option<String>, Exp)>),
    Args(Vec<(Option<String>, Exp)>),
    RField(Box<(String, Exp)>),
    RFields(Vec<(String, Exp)>),
    Clause(Box<Clause>),
    Clauses(Vec<Clause>),
    CapPat(Box<CtorArgPat>),
    CapPats(Vec<CtorArgPat>),
    PField(Box<(String, Pat)>),
    RpItems(Box<RpItemsT>),
    PpItems(Box<(Vec<Pat>, Option<Pat>, bool)>),
    TrailOther(TrailingOther),
    Program(Box<Program>),
}

impl Sv {
    /// 意味の値を、木の深さに比例するスタックを使わずに捨てる([`dismantle`])。
    ///
    /// 構文解析が限度で止まったときの、最後の還元の値に使う。
    pub(crate) fn dismantle(self) {
        let work: Vec<Node> = match self {
            Sv::Exp(e) => vec![Node::Exp(*e)],
            Sv::Exps(es) => es.into_iter().map(Node::Exp).collect(),
            Sv::OptExp(e) => (*e).into_iter().map(Node::Exp).collect(),
            Sv::Pat(p) => vec![Node::Pat(*p)],
            Sv::Pats(ps) => ps.into_iter().map(Node::Pat).collect(),
            Sv::Ty(t) => vec![Node::Ty(*t)],
            Sv::Tys(ts) => ts.into_iter().map(Node::Ty).collect(),
            Sv::OptTy(t) => (*t).into_iter().map(Node::Ty).collect(),
            Sv::Decl(d) => vec![Node::Decl(*d)],
            Sv::Decls(ds) => ds.into_iter().map(Node::Decl).collect(),
            Sv::DeclKind(k) => vec![Node::Decl(Decl {
                meta: Meta {
                    id: 0,
                    span: Span::default(),
                    depth: k.child_depth().saturating_add(1),
                },
                kind: *k,
            })],
            Sv::Binding(b) => vec![Node::Bind(*b)],
            Sv::Bindings(bs) => bs.into_iter().map(Node::Bind).collect(),
            Sv::Header(h) => h.1.into_iter().map(Node::Decl).collect(),
            Sv::Program(p) => p.decls.into_iter().map(Node::Decl).collect(),
            Sv::Arg(a) => vec![Node::Exp(a.1)],
            Sv::Args(xs) => xs.into_iter().map(|(_, e)| Node::Exp(e)).collect(),
            Sv::RField(f) => vec![Node::Exp(f.1)],
            Sv::RFields(xs) => xs.into_iter().map(|(_, e)| Node::Exp(e)).collect(),
            Sv::Clause(c) => {
                let c = *c;
                let mut v = vec![Node::Pat(c.pat), Node::Exp(c.body)];
                v.extend(c.guard.map(Node::Exp));
                v
            }
            Sv::Clauses(cs) => cs
                .into_iter()
                .flat_map(|c| {
                    let mut v = vec![Node::Pat(c.pat), Node::Exp(c.body)];
                    v.extend(c.guard.map(Node::Exp));
                    v
                })
                .collect(),
            // ほかの値は、限度の内側の深さの子しか持たない。
            _ => Vec::new(),
        };
        dismantle(work);
    }
}

#[cold]
fn bad(want: &str, got: &Sv) -> ! {
    panic!("keleut-front: 意味の値の型が合いません({want} を期待したが {got:?})")
}

/// 値を取り出す関数を作る。
/// 1 つ目の形は変種の中身をそのまま返し、`box` を前に付けた 2 つ目の形は箱から出して返す。
macro_rules! accessors {
    ($($name:ident : $var:ident => $t:ty),* $(,)?) => {
        impl Sv {
            $(
                fn $name(self) -> $t {
                    match self {
                        Sv::$var(x) => x,
                        other => bad(stringify!($var), &other),
                    }
                }
            )*
        }
    };
    (box $($name:ident : $var:ident => $t:ty),* $(,)?) => {
        impl Sv {
            $(
                fn $name(self) -> $t {
                    match self {
                        Sv::$var(x) => *x,
                        other => bad(stringify!($var), &other),
                    }
                }
            )*
        }
    };
}

accessors! {
    tok: Tok => Tok,
    s: Str => String,
    ostr: OptStr => Option<String>,
    strs: Strs => Vec<String>,
    lid: LongId => LongId,
    int: Int => u32,
    exps: Exps => Vec<Exp>,
    pats: Pats => Vec<Pat>,
    tys: Tys => Vec<TypeExp>,
    belems: BraceElems => Vec<BraceElem>,
    decls: Decls => Vec<Decl>,
    bindings: Bindings => Vec<LetBinding>,
    inames: ImportNames => Vec<ImportName>,
    tparams: TParams => Vec<TypeParam>,
    classes: Classes => Vec<LongId>,
    classvals: ClassVals => Vec<ClassVal>,
    ctors: Ctors => Vec<CtorDecl>,
    fields: Fields => Vec<FieldDecl>,
    effops: EffOps => Vec<(String, TypeExp)>,
    args: Args => Vec<(Option<String>, Exp)>,
    rfields: RFields => Vec<(String, Exp)>,
    clauses: Clauses => Vec<Clause>,
    cappats: CapPats => Vec<CtorArgPat>,
    trailother: TrailOther => TrailingOther,
}

accessors! {
    box
    exp: Exp => Exp,
    oexp: OptExp => Option<Exp>,
    pat: Pat => Pat,
    ty: Ty => TypeExp,
    oty: OptTy => Option<TypeExp>,
    typair: TyPair => (Option<TypeExp>, Option<TypeExp>),
    arrowret: ArrowRet => (TypeExp, Option<TypeExp>),
    tyitems: TyItems => (Vec<TypeExp>, bool),
    belem: BraceElem => BraceElem,
    effname: EffName => (LongId, Vec<TypeExp>),
    decl: Decl => Decl,
    dkind: DeclKind => DeclKind,
    binding: Binding => LetBinding,
    header: Header => (Vec<ImportDecl>, Vec<Decl>),
    import: Import => ImportDecl,
    iname: ImportName => ImportName,
    tparam: TParam => TypeParam,
    classval: ClassVal => ClassVal,
    rhs: Rhs => RhsRaw,
    ctor: Ctor => CtorDecl,
    field: Field => FieldDecl,
    effop: EffOp => (String, TypeExp),
    arg: Arg => (Option<String>, Exp),
    rfield: RField => (String, Exp),
    clause: Clause => Clause,
    cappat: CapPat => CtorArgPat,
    pfield: PField => (String, Pat),
    rpitems: RpItems => (Vec<(String, Pat)>, Option<Pat>),
    ppitems: PpItems => (Vec<Pat>, Option<Pat>, bool),
}

impl Sv {
    /// 受理した値を構文解析の結果にする。
    pub fn into_program(self) -> Program {
        match self {
            Sv::Program(p) => *p,
            other => bad("Program", &other),
        }
    }

    fn externsig(self) -> ExternSigT {
        match self {
            Sv::ExternSig(b) => *b,
            other => bad("ExternSig", &other),
        }
    }

    /// 識別子、文字列、`#` の名前のトークンの中身。
    fn text(self) -> String {
        match self.tok() {
            Tok::LOWER_IDENTIFIER(s)
            | Tok::UPPER_IDENTIFIER(s)
            | Tok::TEXT(s)
            | Tok::HASH_IDENT(s) => s,
            t => panic!("keleut-front: 文字列を持たないトークン {t:?}"),
        }
    }

    fn number(self) -> Number {
        match self.tok() {
            Tok::NUMBER(n) => n,
            t => panic!("keleut-front: 数でないトークン {t:?}"),
        }
    }

    fn boolean(self) -> bool {
        match self.tok() {
            Tok::BOOL(b) => b,
            t => panic!("keleut-front: 真偽値でないトークン {t:?}"),
        }
    }
}

/// 逆順の列を正順にする。
fn fwd<T>(mut v: Vec<T>) -> Vec<T> {
    v.reverse();
    v
}

/// 規則の右辺の値と位置である。添字は 1 から数える(`$1`、`$2`、…)。
pub struct Args {
    v: Vec<(Sv, Pos, Pos)>,
    sloc: (Pos, Pos),
}

impl Args {
    /// 右辺の値と位置の列と、規則全体の範囲から作る。
    pub fn new(v: Vec<(Sv, Pos, Pos)>, sloc: (Pos, Pos)) -> Args {
        Args { v, sloc }
    }

    /// 右辺の列を返す(呼び出し側が割り付けを使い回す)。
    pub fn into_vec(self) -> Vec<(Sv, Pos, Pos)> {
        self.v
    }

    fn t(&mut self, i: usize) -> Sv {
        std::mem::replace(&mut self.v[i - 1].0, Sv::None)
    }

    fn sp(&self, i: usize) -> Pos {
        self.v[i - 1].1
    }

    fn ep(&self, i: usize) -> Pos {
        self.v[i - 1].2
    }

    fn loc(&self, i: usize) -> (Pos, Pos) {
        (self.v[i - 1].1, self.v[i - 1].2)
    }

    fn start(&self) -> Pos {
        self.sloc.0
    }
}

// ---- 脱糖の補助関数(Diktor の parser.mly の §3.4〜§3.10) ----

fn is_upper(s: &str) -> bool {
    s.as_bytes().first().is_some_and(|c| c.is_ascii_uppercase())
}

fn is_index_label(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0] == b'_' && b[1..].iter().all(|c| c.is_ascii_digit())
}

fn dot_select(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    e: Exp,
    id_pos: Pos,
    id: String,
) -> Result<Exp, ActErr> {
    if let ExpKind::Ident(LongId(comps)) = &e.kind
        && comps.iter().all(|c| is_upper(c))
    {
        let mut comps = comps.clone();
        comps.push(id);
        return Ok(cx.exp(sloc, ExpKind::Ident(LongId(comps))));
    }
    if is_upper(&id) {
        return syntax_error(
            id_pos,
            format!("レコードのラベルに大文字識別子は使えません: {id}"),
        );
    }
    if is_index_label(&id) {
        let Some(n) = ocaml_int_of_string_opt(&id[1..]) else {
            return Err(ActErr::Internal("Failure(\"int_of_string\")".to_string()));
        };
        if n as u64 > cx.limit as u64 {
            return Err(ActErr::Limit);
        }
        let mut e = e;
        for _ in 0..n {
            e = cx.exp(
                sloc,
                ExpKind::RecordRestriction(Box::new(e), "_item".to_string()),
            );
        }
        return Ok(cx.exp(
            sloc,
            ExpKind::RecordSelection(Box::new(e), "_item".to_string()),
        ));
    }
    Ok(cx.exp(sloc, ExpKind::RecordSelection(Box::new(e), id)))
}

fn record_of_args(cx: &mut Ctx, sloc: (Pos, Pos), args: Vec<(Option<String>, Exp)>) -> Exp {
    let mut row = cx.exp(sloc, ExpKind::RecordEmpty);
    for (label, v) in args.into_iter().rev() {
        let l = label.unwrap_or_else(|| "_item".to_string());
        row = cx.exp(sloc, ExpKind::RecordExtend(Box::new(row), l, Box::new(v)));
    }
    row
}

fn tuple_exp(cx: &mut Ctx, sloc: (Pos, Pos), exps: Vec<Exp>) -> Exp {
    record_of_args(cx, sloc, exps.into_iter().map(|e| (None, e)).collect())
}

fn variant_payload(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    args: Vec<(Option<String>, Exp)>,
) -> Result<Exp, ActErr> {
    if args.len() == 1 && args[0].0.is_none() {
        return Ok(args.into_iter().next().map(|(_, e)| e).expect("1 個"));
    }
    if args.iter().any(|(l, _)| l.is_some()) {
        return syntax_error(sloc.0, "#Foo(...) にラベル付き引数は使えません");
    }
    Ok(tuple_exp(
        cx,
        sloc,
        args.into_iter().map(|(_, e)| e).collect(),
    ))
}

fn call(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    callee: Exp,
    args: Vec<(Option<String>, Exp)>,
) -> Result<Exp, ActErr> {
    match callee.kind {
        ExpKind::Variant(s, payload) if payload.kind == ExpKind::RecordEmpty => {
            let p = variant_payload(cx, sloc, args)?;
            Ok(cx.exp(sloc, ExpKind::Variant(s, Box::new(p))))
        }
        ExpKind::Ident(LongId(comps)) if comps.last().is_some_and(|c| is_upper(c)) => {
            let args = args
                .into_iter()
                .map(|(label, exp)| CtorArg { label, exp })
                .collect();
            Ok(cx.exp(sloc, ExpKind::Construct(LongId(comps), args)))
        }
        kind => {
            let callee = Exp {
                meta: callee.meta,
                kind,
            };
            let row = record_of_args(cx, sloc, args);
            Ok(cx.exp(sloc, ExpKind::Apply(Box::new(callee), Box::new(row))))
        }
    }
}

/// ブロックの宣言の列を式にする(Diktor の `block_of_items`)。
fn block_of_items(cx: &mut Ctx, sloc: (Pos, Pos), items: Vec<Decl>) -> Result<Exp, ActErr> {
    // 左から見て最初の誤りを報告する(Diktor の再帰は左から順に誤りを調べる)。
    for d in &items {
        match &d.kind {
            DeclKind::Exp(_) => {}
            DeclKind::Let(b) => {
                if b.is_pub {
                    return syntax_error(
                        d.meta.span.start,
                        "ブロックの中の let には pub を付けられません(with より後ろの宣言は公開できません)",
                    );
                }
            }
            DeclKind::LetRec(bs) => {
                if bs.iter().any(|b| b.is_pub) {
                    return syntax_error(
                        d.meta.span.start,
                        "ブロックの中の let rec には pub を付けられません(with より後ろの宣言は公開できません)",
                    );
                }
            }
            _ => {
                return syntax_error(
                    d.meta.span.start,
                    "この宣言はブロック内では使えません(let / let rec / 式のみ)",
                );
            }
        }
    }
    // 後ろから畳む。`Seq` を前に伸ばす間は、要素を逆順の列に溜めておき、
    // 使うときに 1 つのノードにする(Diktor は伸ばすたびに新しいノードを作るが、形は同じである)。
    enum Acc {
        Empty,
        One(Exp),
        SeqRev(Vec<Exp>),
    }
    let close = |cx: &mut Ctx, acc: Acc| -> Exp {
        match acc {
            Acc::Empty => cx.exp(sloc, ExpKind::RecordEmpty),
            Acc::One(e) => e,
            Acc::SeqRev(es) => cx.exp(sloc, ExpKind::Seq(fwd(es))),
        }
    };
    let mut acc = Acc::Empty;
    for d in items.into_iter().rev() {
        acc = match d.kind {
            DeclKind::Exp(e) => match acc {
                Acc::Empty => Acc::One(e),
                Acc::One(r) => match r.kind {
                    ExpKind::Seq(es) => {
                        let mut v = fwd(es);
                        v.push(e);
                        Acc::SeqRev(v)
                    }
                    kind => Acc::SeqRev(vec![Exp { meta: r.meta, kind }, e]),
                },
                Acc::SeqRev(mut v) => {
                    v.push(e);
                    Acc::SeqRev(v)
                }
            },
            DeclKind::Let(b) => {
                let rest = close(cx, acc);
                Acc::One(cx.reuse_exp(d.meta, ExpKind::Let(Box::new(b), Box::new(rest))))
            }
            DeclKind::LetRec(bs) => {
                let rest = close(cx, acc);
                Acc::One(cx.reuse_exp(d.meta, ExpKind::LetRec(bs, Box::new(rest))))
            }
            _ => unreachable!("先に調べてある"),
        };
    }
    Ok(close(cx, acc))
}

/// 引数の行の最も内側に `k` を足す(Diktor の `append_last_arg`)。
///
/// 行の鎖をほどいてから内側から組み直し、各ノードの深さを作り直す。
fn append_last_arg(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    meta: Meta,
    f: Exp,
    args: Exp,
    k: Exp,
) -> Result<Exp, ActErr> {
    let mut chain: Vec<(Meta, String, Exp)> = Vec::new();
    let mut cur = args;
    let inner_meta = loop {
        match cur.kind {
            ExpKind::RecordExtend(rest, l, v) => {
                chain.push((cur.meta, l, *v));
                cur = *rest;
            }
            ExpKind::RecordEmpty => break cur.meta,
            _ => return syntax_error(sloc.0, "with: 呼び出しの引数形が不正です"),
        }
    };
    let empty = cx.exp(sloc, ExpKind::RecordEmpty);
    let mut row = cx.reuse_exp(
        inner_meta,
        ExpKind::RecordExtend(Box::new(empty), "_item".to_string(), Box::new(k)),
    );
    while let Some((m, l, v)) = chain.pop() {
        row = cx.reuse_exp(m, ExpKind::RecordExtend(Box::new(row), l, Box::new(v)));
    }
    Ok(cx.reuse_exp(meta, ExpKind::Apply(Box::new(f), Box::new(row))))
}

fn with_splice(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    pat: Pat,
    call: Exp,
    k_body: Exp,
) -> Result<Exp, ActErr> {
    let params = if pat.kind == PatKind::Wildcard {
        vec![]
    } else {
        vec![pat]
    };
    let k = cx.exp(
        sloc,
        ExpKind::Lambda(Lambda {
            params,
            body: Box::new(k_body),
        }),
    );
    match call.kind {
        ExpKind::Apply(f, args) => append_last_arg(cx, sloc, call.meta, *f, *args, k),
        _ => syntax_error(
            call.meta.span.start,
            "with の右辺は関数呼び出しでなければなりません",
        ),
    }
}

fn trailing_record<T>(pos: Pos) -> Result<T, ActErr> {
    syntax_error(
        pos,
        "末尾ブロックの中身にレコードの形は書けません(空の {} もレコードです。何もしない関数は { () } と書きます)",
    )
}

fn trailing_other_err<T>(pos: Pos, t: TrailingOther) -> Result<T, ActErr> {
    match t {
        TrailingOther::Record => trailing_record(pos),
        TrailingOther::Params(p) => syntax_error(
            p,
            "末尾ブロックの引数は小文字の識別子 1 個です(2 個以上の引数を取る関数は fn(x, y) => 式 と書いて括弧の中に渡します)",
        ),
        TrailingOther::Annot(p) => syntax_error(
            p,
            "末尾ブロックの引数に型注釈は書けません(型注釈の要る関数は fn(x: T) => 式 と書いて括弧の中に渡します)",
        ),
    }
}

fn trailing_misplaced<T>(pos: Pos) -> Result<T, ActErr> {
    syntax_error(
        pos,
        "コンストラクタと構造的ヴァリアントには末尾ブロックを付けられません",
    )
}

fn trailing_call(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    callee: Exp,
    args: Vec<(Option<String>, Exp)>,
    lbrace_pos: Pos,
    prev_end: Pos,
    k: Exp,
) -> Result<Exp, ActErr> {
    if lbrace_pos.line != prev_end.line {
        return syntax_error(
            lbrace_pos,
            "末尾ブロックの { は、呼び出しと同じ行に書きます",
        );
    }
    let c = call(cx, sloc, callee, args)?;
    match c.kind {
        ExpKind::Apply(f, a) => append_last_arg(cx, sloc, c.meta, *f, *a, k),
        _ => trailing_misplaced(lbrace_pos),
    }
}

fn pat_tuple(cx: &mut Ctx, sloc: (Pos, Pos), pats: Vec<Pat>, rest: Option<Pat>) -> Pat {
    let fields = pats.into_iter().map(|p| ("_item".to_string(), p)).collect();
    cx.pat(sloc, PatKind::Record(fields, rest.map(Box::new)))
}

fn variant_pat_payload(
    cx: &mut Ctx,
    sloc: (Pos, Pos),
    args: Vec<CtorArgPat>,
) -> Result<Pat, ActErr> {
    if args.len() == 1 && args[0].label.is_none() {
        return Ok(args.into_iter().next().expect("1 個").pat);
    }
    if args.iter().any(|a| a.label.is_some()) {
        return syntax_error(sloc.0, "#Foo(...) パターンにラベル付き引数は使えません");
    }
    Ok(pat_tuple(
        cx,
        sloc,
        args.into_iter().map(|a| a.pat).collect(),
        None,
    ))
}

fn tuple_ty(cx: &mut Ctx, sloc: (Pos, Pos), tys: Vec<TypeExp>) -> TypeExp {
    let elems = tys
        .into_iter()
        .map(|t| BraceElem::Field("_item".to_string(), t))
        .collect();
    cx.ty(sloc, TypeExpKind::BraceRow(elems, None))
}

fn negate(cx: &mut Ctx, sloc: (Pos, Pos), sp: Pos, ep: Pos, e: Exp) -> Exp {
    if let ExpKind::Number(n) = &e.kind
        && !n.text.starts_with('-')
        && (ep.cnum as i64 - sp.cnum as i64) == (n.text.len() + n.suffix_text.len()) as i64
    {
        let mut n = n.clone();
        n.text = format!("-{}", n.text);
        return cx.exp(sloc, ExpKind::Number(n));
    }
    cx.exp(sloc, ExpKind::Neg(Box::new(e)))
}

fn set_pub(pos: Pos, d: DeclKind) -> Result<DeclKind, ActErr> {
    Ok(match d {
        DeclKind::Type(mut t) => {
            t.is_pub = true;
            DeclKind::Type(t)
        }
        DeclKind::Newtype(mut n) => {
            n.is_pub = true;
            DeclKind::Newtype(n)
        }
        DeclKind::Effect(mut e) => {
            e.is_pub = true;
            DeclKind::Effect(e)
        }
        DeclKind::Class(mut c) => {
            c.is_pub = true;
            DeclKind::Class(c)
        }
        DeclKind::Let(mut b) => {
            b.is_pub = true;
            DeclKind::Let(b)
        }
        DeclKind::LetRec(mut bs) => {
            for b in &mut bs {
                b.is_pub = true;
            }
            DeclKind::LetRec(bs)
        }
        DeclKind::Module(_, n, ds) => DeclKind::Module(true, n, ds),
        DeclKind::Extern(mut e) => {
            e.is_pub = true;
            DeclKind::Extern(e)
        }
        DeclKind::Instance(_) => return syntax_error(pos, "type instance に pub は付けられません"),
        DeclKind::Exp(_) => return syntax_error(pos, "式文に pub は付けられません"),
    })
}

fn check_instance_body(pos: Pos, items: Vec<Decl>) -> Result<Vec<Decl>, ActErr> {
    let name_of = |b: &LetBinding| match &b.name.kind {
        PatKind::Var(x) => x.clone(),
        _ => "_".to_string(),
    };
    let found = items.iter().find_map(|d| match &d.kind {
        DeclKind::Let(b) if b.is_pub => Some(name_of(b)),
        DeclKind::LetRec(bs) => bs.iter().find(|b| b.is_pub).map(name_of),
        _ => None,
    });
    match found {
        Some(x) => syntax_error(
            pos,
            format!(
                "type instance の本体の let には pub を付けられません(メソッドはクラスを import して使います): {x}"
            ),
        ),
        None => Ok(items),
    }
}

// ---- 規則の動作 ----

type ActFn = fn(&mut Ctx, &mut Args) -> Res;

/// 規則の番号の動作を実行する。
pub fn run(p: usize, cx: &mut Ctx, a: &mut Args) -> Res {
    static TABLE: OnceLock<Vec<ActFn>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        PRODS
            .iter()
            .map(|p| lookup(p.key).unwrap_or(if p.start { start_action } else { missing }))
            .collect()
    });
    table[p](cx, a)
}

fn start_action(_: &mut Ctx, a: &mut Args) -> Res {
    Ok(a.t(1))
}

fn missing(_: &mut Ctx, _: &mut Args) -> Res {
    panic!("keleut-front: 動作の無い規則を還元しました(tables.rs と actions.rs が合っていません)")
}

/// 動作の無い規則の綴りを返す(試験用)。
pub fn unmapped_keys() -> Vec<&'static str> {
    PRODS
        .iter()
        .filter(|p| !p.start && lookup(p.key).is_none())
        .map(|p| p.key)
        .collect()
}

macro_rules! f {
    (|$c:ident, $a:ident| $body:expr) => {
        Some(
            (|$c: &mut Ctx, $a: &mut Args| -> Res {
                #[allow(unused_variables)]
                let ($c, $a) = ($c, $a);
                $body
            }) as ActFn,
        )
    };
}

fn binop(cx: &mut Ctx, a: &mut Args, op: BinOp) -> Res {
    let r = a.t(3).exp();
    let l = a.t(1).exp();
    Ok(Sv::Exp(Box::new(cx.exp(
        a.sloc,
        ExpKind::BinOp(Box::new(l), op, Box::new(r)),
    ))))
}

fn lbrace_body<T>(a: &mut Args, wrap: fn(T) -> Sv, get: fn(Sv) -> T) -> Res {
    Ok(wrap(get(a.t(2))))
}

fn import_decl(a: &mut Args, names: Vec<ImportName>) -> Res {
    let source = a.t(2).text();
    Ok(Sv::Import(Box::new(ImportDecl {
        source,
        names,
        span: Span {
            start: a.sloc.0,
            finish: a.sloc.1,
        },
    })))
}

fn items_with(cx: &mut Ctx, a: &mut Args, rest: Option<Vec<Decl>>) -> Res {
    let sloc = a.sloc;
    let k_body = match rest {
        Some(items) => block_of_items(cx, sloc, fwd(items))?,
        None => cx.exp(sloc, ExpKind::RecordEmpty),
    };
    let call = a.t(4).exp();
    let pat = a.t(2).pat();
    let e = with_splice(cx, sloc, pat, call, k_body)?;
    Ok(Sv::Decls(vec![cx.decl(sloc, DeclKind::Exp(e))]))
}

fn lookup(key: &str) -> Option<ActFn> {
    match key {
        // ---- 名前 ----
        "lower_id: LOWER_IDENTIFIER" | "upper_id: UPPER_IDENTIFIER" => {
            f!(|c, a| Ok(Sv::Str(a.t(1).text())))
        }
        "upper_path: upper_id" => f!(|c, a| Ok(Sv::Strs(vec![a.t(1).s()]))),
        "upper_path: upper_path DOT upper_id" => f!(|c, a| {
            let x = a.t(3).s();
            let mut p = a.t(1).strs();
            p.push(x);
            Ok(Sv::Strs(p))
        }),
        "long_id: lower_id" => f!(|c, a| Ok(Sv::LongId(LongId(vec![a.t(1).s()])))),
        "long_id: upper_path" => f!(|c, a| Ok(Sv::LongId(LongId(a.t(1).strs())))),
        "long_id: upper_path DOT lower_id" => f!(|c, a| {
            let x = a.t(3).s();
            let mut p = a.t(1).strs();
            p.push(x);
            Ok(Sv::LongId(LongId(p)))
        }),

        // ---- プログラムと宣言 ----
        "program: header EOF" => f!(|c, a| {
            let (is, ds) = a.t(1).header();
            Ok(Sv::Program(Box::new(Program {
                imports: fwd(is),
                decls: fwd(ds),
            })))
        }),
        "header: items" | "header_after: items_head" => {
            f!(|c, a| Ok(Sv::Header(Box::new((vec![], a.t(1).decls())))))
        }
        "header: import_decl" | "header_after: import_decl" => {
            f!(|c, a| Ok(Sv::Header(Box::new((vec![a.t(1).import()], vec![])))))
        }
        "header: import_decl semis header_after"
        | "header_after: import_decl semis header_after" => f!(|c, a| {
            let (mut is, ds) = a.t(3).header();
            is.push(a.t(1).import());
            Ok(Sv::Header(Box::new((is, ds))))
        }),
        "semis: NL" | "semis: SEMI" | "semis: NL semis" | "semis: SEMI semis" => {
            f!(|c, a| Ok(Sv::None))
        }
        "items_head:" | "items:" => f!(|c, a| Ok(Sv::Decls(vec![]))),
        "items_head: item" | "items: item" => f!(|c, a| Ok(Sv::Decls(vec![a.t(1).decl()]))),
        "items_head: item NL items"
        | "items_head: item SEMI items"
        | "items: item NL items"
        | "items: item SEMI items" => {
            f!(|c, a| {
                let mut l = a.t(3).decls();
                l.push(a.t(1).decl());
                Ok(Sv::Decls(l))
            })
        }
        "items: NL items" | "items: SEMI items" => f!(|c, a| Ok(Sv::Decls(a.t(2).decls()))),
        "items_head: WITH pat EQ exp" | "items: WITH pat EQ exp" => {
            f!(|c, a| items_with(c, a, None))
        }
        "items_head: WITH pat EQ exp NL items"
        | "items_head: WITH pat EQ exp SEMI items"
        | "items: WITH pat EQ exp NL items"
        | "items: WITH pat EQ exp SEMI items" => f!(|c, a| {
            let rest = a.t(6).decls();
            items_with(c, a, Some(rest))
        }),
        "import_decl: FROM TEXT IMPORT import_name" => f!(|c, a| {
            let n = a.t(4).iname();
            import_decl(a, vec![n])
        }),
        "import_decl: FROM TEXT IMPORT LBRACE_RECORD import_name_list RBRACE" => f!(|c, a| {
            let ns = fwd(a.t(5).inames());
            import_decl(a, ns)
        }),
        "import_name_list: import_name" | "import_name_list: import_name COMMA" => {
            f!(|c, a| Ok(Sv::ImportNames(vec![a.t(1).iname()])))
        }
        "import_name_list: import_name COMMA import_name_list" => f!(|c, a| {
            let mut l = a.t(3).inames();
            l.push(a.t(1).iname());
            Ok(Sv::ImportNames(l))
        }),
        "import_name: import_head" => f!(|c, a| {
            let span = Span {
                start: a.sloc.0,
                finish: a.sloc.1,
            };
            Ok(Sv::ImportName(Box::new(ImportName {
                path: vec![a.t(1).s()],
                span,
            })))
        }),
        "import_name: upper_id DOT import_head" => f!(|c, a| {
            let span = Span {
                start: a.sloc.0,
                finish: a.sloc.1,
            };
            let h = a.t(3).s();
            Ok(Sv::ImportName(Box::new(ImportName {
                path: vec![a.t(1).s(), h],
                span,
            })))
        }),
        "import_head: upper_id" | "import_head: lower_id" => f!(|c, a| Ok(Sv::Str(a.t(1).s()))),
        "item: decl" => f!(|c, a| Ok(Sv::Decl(Box::new(a.t(1).decl())))),
        "item: exp" => f!(|c, a| {
            let e = a.t(1).exp();
            Ok(Sv::Decl(Box::new(c.decl(a.sloc, DeclKind::Exp(e)))))
        }),
        "decl: PUB decl_body" => f!(|c, a| {
            let d = set_pub(a.start(), a.t(2).dkind())?;
            Ok(Sv::Decl(Box::new(c.decl(a.sloc, d))))
        }),
        "decl: decl_body" => f!(|c, a| {
            let d = a.t(1).dkind();
            Ok(Sv::Decl(Box::new(c.decl(a.sloc, d))))
        }),
        "decl_body: LET binding" => {
            f!(|c, a| Ok(Sv::DeclKind(Box::new(DeclKind::Let(a.t(2).binding())))))
        }
        "decl_body: LET REC binding and_bindings" => f!(|c, a| {
            let mut bs = a.t(4).bindings();
            bs.push(a.t(3).binding());
            Ok(Sv::DeclKind(Box::new(DeclKind::LetRec(fwd(bs)))))
        }),
        "decl_body: TYPE upper_id typarams_opt kind_annot_opt EQ ty" => f!(|c, a| {
            let body = a.t(6).ty();
            let kind = a.t(4).ostr();
            let params = fwd(a.t(3).tparams());
            let name = a.t(2).s();
            Ok(Sv::DeclKind(Box::new(DeclKind::Type(TypeAlias {
                is_pub: false,
                name,
                params,
                kind,
                body,
            }))))
        }),
        "decl_body: TYPE CLASS upper_id typarams class_body" => f!(|c, a| {
            let vals = fwd(a.t(5).classvals());
            let params = fwd(a.t(4).tparams());
            let name = a.t(3).s();
            Ok(Sv::DeclKind(Box::new(DeclKind::Class(ClassDecl {
                is_pub: false,
                name,
                params,
                vals,
            }))))
        }),
        "decl_body: TYPE INSTANCE typarams_opt upper_id LBRACKET ty_args RBRACKET instance_body" => {
            f!(|c, a| {
                let body = check_instance_body(a.start(), fwd(a.t(8).decls()))?;
                let args = fwd(a.t(6).tys());
                let class = a.t(4).s();
                let tparams = fwd(a.t(3).tparams());
                Ok(Sv::DeclKind(Box::new(DeclKind::Instance(InstanceDecl {
                    tparams,
                    class,
                    args,
                    body,
                }))))
            })
        }
        "decl_body: NEWTYPE upper_id typarams_opt newtype_rhs" => f!(|c, a| {
            let rhs = a.t(4).rhs();
            let params = fwd(a.t(3).tparams());
            let name = a.t(2).s();
            let ctors = match rhs {
                RhsRaw::None => vec![],
                RhsRaw::Ctors(cs) => cs,
                RhsRaw::Short(fields) => vec![CtorDecl {
                    name: name.clone(),
                    fields,
                }],
            };
            Ok(Sv::DeclKind(Box::new(DeclKind::Newtype(Newtype {
                is_pub: false,
                name,
                params,
                ctors,
            }))))
        }),
        "decl_body: EFFECT upper_id typarams_opt EQ eff_decl_body" => f!(|c, a| {
            let ops = fwd(a.t(5).effops());
            let params = fwd(a.t(3).tparams());
            let name = a.t(2).s();
            Ok(Sv::DeclKind(Box::new(DeclKind::Effect(Effect {
                is_pub: false,
                name,
                params,
                ops,
            }))))
        }),
        "decl_body: MODULE upper_id module_body" => f!(|c, a| {
            let body = fwd(a.t(3).decls());
            let name = a.t(2).s();
            Ok(Sv::DeclKind(Box::new(DeclKind::Module(false, name, body))))
        }),
        "decl_body: EXTERN TEXT LET extern_sig" => f!(|c, a| {
            let (name, tparams, params, (ret, eff)) = a.t(4).externsig();
            let abi = a.t(2).text();
            Ok(Sv::DeclKind(Box::new(DeclKind::Extern(ExternDecl {
                is_pub: false,
                abi,
                prim: name.clone(),
                name,
                tparams,
                params,
                ret,
                eff,
            }))))
        }),
        "and_bindings:" => f!(|c, a| Ok(Sv::Bindings(vec![]))),
        "and_bindings: AND binding and_bindings" => f!(|c, a| {
            let mut l = a.t(3).bindings();
            l.push(a.t(2).binding());
            Ok(Sv::Bindings(l))
        }),
        "binding: lower_id typarams_opt LPAREN params RPAREN sig_tail EQ exp" => f!(|c, a| {
            let body = a.t(8).exp();
            let (ret, eff) = a.t(6).typair();
            let params = fwd(a.t(4).pats());
            let tparams = fwd(a.t(2).tparams());
            let x = a.t(1).s();
            let name = c.pat(a.sloc, PatKind::Var(x));
            Ok(Sv::Binding(Box::new(c.binding(
                a.sloc,
                name,
                tparams,
                Some(params),
                ret,
                eff,
                body,
            ))))
        }),
        "binding: bind_pat annot_opt EQ exp" => f!(|c, a| {
            let body = a.t(4).exp();
            let ret = a.t(2).oty();
            let name = a.t(1).pat();
            Ok(Sv::Binding(Box::new(c.binding(
                a.sloc,
                name,
                vec![],
                None,
                ret,
                None,
                body,
            ))))
        }),
        "sig_tail:" => f!(|c, a| Ok(Sv::TyPair(Box::new((None, None))))),
        "sig_tail: AT eff" => f!(|c, a| Ok(Sv::TyPair(Box::new((None, Some(a.t(2).ty())))))),
        "sig_tail: COLON ty_ret" => f!(|c, a| Ok(Sv::TyPair(Box::new(a.t(2).typair())))),
        "ty_ret: union_ty" | "ty_ret: arrow_ty" => {
            f!(|c, a| Ok(Sv::TyPair(Box::new((Some(a.t(1).ty()), None)))))
        }
        "ty_ret: union_ty AT eff" => f!(|c, a| {
            let e = a.t(3).ty();
            Ok(Sv::TyPair(Box::new((Some(a.t(1).ty()), Some(e)))))
        }),
        "annot_opt:" => f!(|c, a| Ok(Sv::OptTy(Box::new(None)))),
        "annot_opt: COLON ty" => f!(|c, a| Ok(Sv::OptTy(Box::new(Some(a.t(2).ty()))))),
        "extern_sig: lower_id typarams_opt LPAREN params RPAREN sig_tail" => f!(|c, a| {
            let tail = a.t(6).typair();
            let params = fwd(a.t(4).pats());
            let tparams = fwd(a.t(2).tparams());
            let name = a.t(1).s();
            Ok(Sv::ExternSig(Box::new((name, tparams, params, tail))))
        }),
        "module_body: LBRACE_BLOCK items RBRACE"
        | "module_body: LBRACE_RECORD items RBRACE"
        | "module_body: LBRACE_TYPE items RBRACE"
        | "instance_body: LBRACE_BLOCK items RBRACE"
        | "instance_body: LBRACE_RECORD items RBRACE"
        | "instance_body: LBRACE_TYPE items RBRACE" => {
            f!(|c, a| lbrace_body(a, Sv::Decls, Sv::decls))
        }
        "class_body: LBRACE_BLOCK class_items RBRACE"
        | "class_body: LBRACE_RECORD class_items RBRACE"
        | "class_body: LBRACE_TYPE class_items RBRACE" => {
            f!(|c, a| lbrace_body(a, Sv::ClassVals, Sv::classvals))
        }
        "class_items:" => f!(|c, a| Ok(Sv::ClassVals(vec![]))),
        "class_items: NL class_items" | "class_items: SEMI class_items" => {
            f!(|c, a| Ok(Sv::ClassVals(a.t(2).classvals())))
        }
        "class_items: class_item" => f!(|c, a| Ok(Sv::ClassVals(vec![a.t(1).classval()]))),
        "class_items: class_item NL class_items" | "class_items: class_item SEMI class_items" => {
            f!(|c, a| {
                let mut l = a.t(3).classvals();
                l.push(a.t(1).classval());
                Ok(Sv::ClassVals(l))
            })
        }
        "class_item: VAL lower_id typarams_opt COLON ty" => f!(|c, a| {
            let ty = a.t(5).ty();
            let tparams = fwd(a.t(3).tparams());
            let name = a.t(2).s();
            Ok(Sv::ClassVal(Box::new(ClassVal { name, tparams, ty })))
        }),
        "newtype_rhs:" => f!(|c, a| Ok(Sv::Rhs(Box::new(RhsRaw::None)))),
        "newtype_rhs: LPAREN ctor_fields RPAREN" => {
            f!(|c, a| Ok(Sv::Rhs(Box::new(RhsRaw::Short(fwd(a.t(2).fields()))))))
        }
        "newtype_rhs: EQ ctors" => {
            f!(|c, a| Ok(Sv::Rhs(Box::new(RhsRaw::Ctors(fwd(a.t(2).ctors()))))))
        }
        "ctors: ctor" => f!(|c, a| Ok(Sv::Ctors(vec![a.t(1).ctor()]))),
        "ctors: ctor VERTICAL ctors" => f!(|c, a| {
            let mut l = a.t(3).ctors();
            l.push(a.t(1).ctor());
            Ok(Sv::Ctors(l))
        }),
        "ctor: upper_id" => f!(|c, a| Ok(Sv::Ctor(Box::new(CtorDecl {
            name: a.t(1).s(),
            fields: vec![]
        })))),
        "ctor: upper_id LPAREN ctor_fields RPAREN" => f!(|c, a| {
            let fields = fwd(a.t(3).fields());
            Ok(Sv::Ctor(Box::new(CtorDecl {
                name: a.t(1).s(),
                fields,
            })))
        }),
        "ctor_fields:" => f!(|c, a| Ok(Sv::Fields(vec![]))),
        "ctor_fields: ctor_field_list" => f!(|c, a| Ok(Sv::Fields(a.t(1).fields()))),
        "ctor_field_list: ctor_field" | "ctor_field_list: ctor_field COMMA" => {
            f!(|c, a| Ok(Sv::Fields(vec![a.t(1).field()])))
        }
        "ctor_field_list: ctor_field COMMA ctor_field_list" => f!(|c, a| {
            let mut l = a.t(3).fields();
            l.push(a.t(1).field());
            Ok(Sv::Fields(l))
        }),
        "ctor_field: ty" => f!(|c, a| Ok(Sv::Field(Box::new(FieldDecl {
            label: None,
            ty: a.t(1).ty()
        })))),
        "ctor_field: lower_id COLON ty" => f!(|c, a| {
            let ty = a.t(3).ty();
            Ok(Sv::Field(Box::new(FieldDecl {
                label: Some(a.t(1).s()),
                ty,
            })))
        }),
        "eff_decl_body: LBRACE_BLOCK eff_ops RBRACE"
        | "eff_decl_body: LBRACE_RECORD eff_ops RBRACE"
        | "eff_decl_body: LBRACE_TYPE eff_ops RBRACE" => {
            f!(|c, a| lbrace_body(a, Sv::EffOps, Sv::effops))
        }
        "eff_ops:" => f!(|c, a| Ok(Sv::EffOps(vec![]))),
        "eff_ops: eff_op_list" => f!(|c, a| Ok(Sv::EffOps(a.t(1).effops()))),
        "eff_op_list: eff_op" | "eff_op_list: eff_op COMMA" => {
            f!(|c, a| Ok(Sv::EffOps(vec![a.t(1).effop()])))
        }
        "eff_op_list: eff_op COMMA eff_op_list" => f!(|c, a| {
            let mut l = a.t(3).effops();
            l.push(a.t(1).effop());
            Ok(Sv::EffOps(l))
        }),
        "eff_op: lower_id COLON ty" => f!(|c, a| {
            let ty = a.t(3).ty();
            Ok(Sv::EffOp(Box::new((a.t(1).s(), ty))))
        }),
        "typarams_opt:" => f!(|c, a| Ok(Sv::TParams(vec![]))),
        "typarams_opt: typarams" => f!(|c, a| Ok(Sv::TParams(a.t(1).tparams()))),
        "typarams: LBRACKET typaram_list RBRACKET" => f!(|c, a| Ok(Sv::TParams(a.t(2).tparams()))),
        "typaram_list: typaram" | "typaram_list: typaram COMMA" => {
            f!(|c, a| Ok(Sv::TParams(vec![a.t(1).tparam()])))
        }
        "typaram_list: typaram COMMA typaram_list" => f!(|c, a| {
            let mut l = a.t(3).tparams();
            l.push(a.t(1).tparam());
            Ok(Sv::TParams(l))
        }),
        "typaram: ty_ident hkt_opt cls_opt" => f!(|c, a| {
            let classes = fwd(a.t(3).classes());
            let arity = a.t(2).int();
            Ok(Sv::TParam(Box::new(TypeParam {
                name: a.t(1).s(),
                arity,
                classes,
            })))
        }),
        "ty_ident: lower_id" | "ty_ident: upper_id" => f!(|c, a| Ok(Sv::Str(a.t(1).s()))),
        "hkt_opt:" => f!(|c, a| Ok(Sv::Int(0))),
        "hkt_opt: LBRACKET lowline_list RBRACKET" => f!(|c, a| Ok(Sv::Int(a.t(2).int()))),
        "lowline_list: LOWLINE" | "lowline_list: LOWLINE COMMA" => f!(|c, a| Ok(Sv::Int(1))),
        "lowline_list: LOWLINE COMMA lowline_list" => {
            f!(|c, a| Ok(Sv::Int(a.t(3).int().saturating_add(1))))
        }
        "cls_opt:" => f!(|c, a| Ok(Sv::Classes(vec![]))),
        "cls_opt: COLON cls_list" => f!(|c, a| Ok(Sv::Classes(a.t(2).classes()))),
        "cls_list: upper_id" => f!(|c, a| Ok(Sv::Classes(vec![LongId(vec![a.t(1).s()])]))),
        "cls_list: upper_id PLUS cls_list" => f!(|c, a| {
            let mut l = a.t(3).classes();
            l.push(LongId(vec![a.t(1).s()]));
            Ok(Sv::Classes(l))
        }),
        "kind_annot_opt:" => f!(|c, a| Ok(Sv::OptStr(None))),
        "kind_annot_opt: COLON upper_id" => f!(|c, a| Ok(Sv::OptStr(Some(a.t(2).s())))),
        "params:" => f!(|c, a| Ok(Sv::Pats(vec![]))),
        "params: param_list" => f!(|c, a| Ok(Sv::Pats(a.t(1).pats()))),
        "param_list: param" | "param_list: param COMMA" => {
            f!(|c, a| Ok(Sv::Pats(vec![a.t(1).pat()])))
        }
        "param_list: param COMMA param_list" => f!(|c, a| {
            let mut l = a.t(3).pats();
            l.push(a.t(1).pat());
            Ok(Sv::Pats(l))
        }),
        "param: pat annot_opt" => f!(|c, a| {
            let t = a.t(2).oty();
            let p = a.t(1).pat();
            Ok(Sv::Pat(Box::new(match t {
                None => p,
                Some(t) => c.pat(a.sloc, PatKind::Annot(Box::new(p), t)),
            })))
        }),

        // ---- 式 ----
        "exp: FN LPAREN params RPAREN EQ_GREATER exp" => f!(|c, a| {
            let body = a.t(6).exp();
            let params = fwd(a.t(3).pats());
            Ok(Sv::Exp(Box::new(c.exp(
                a.sloc,
                ExpKind::Lambda(Lambda {
                    params,
                    body: Box::new(body),
                }),
            ))))
        }),
        "exp: IF exp THEN exp ELSE exp" => f!(|c, a| {
            let cond_loc = a.loc(2);
            let (t_sp, t_ep) = (a.sp(3), a.ep(4));
            let (e_sp, e_ep) = (a.sp(5), a.ep(6));
            let e3 = a.t(6).exp();
            let e2 = a.t(4).exp();
            let e1 = a.t(2).exp();
            let p_false = c.pat(cond_loc, PatKind::Bool(false));
            let arm_false = c.clause((e_sp, e_ep), p_false, None, e3);
            let p_true = c.pat(cond_loc, PatKind::Bool(true));
            let arm_true = c.clause((t_sp, t_ep), p_true, None, e2);
            Ok(Sv::Exp(Box::new(c.exp(
                a.sloc,
                ExpKind::Match(Box::new(e1), vec![arm_true, arm_false]),
            ))))
        }),
        "exp: postfixed"
        | "postfixed: or_exp"
        | "or_exp: and_exp"
        | "and_exp: cmp_exp"
        | "cmp_exp: add_exp"
        | "add_exp: mul_exp"
        | "mul_exp: prefix_exp"
        | "prefix_exp: postfix_exp"
        | "postfix_exp: tb_call"
        | "postfix_exp: atom"
        | "atom: block"
        | "atom: record_exp"
        | "atom: paren_exp"
        | "trailing_block: lambda_block" => f!(|c, a| Ok(Sv::Exp(Box::new(a.t(1).exp())))),
        "postfixed: postfixed MATCH clause_body" => f!(|c, a| {
            let cs = fwd(a.t(3).clauses());
            let e = a.t(1).exp();
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::Match(Box::new(e), cs)),
            )))
        }),
        "postfixed: postfixed HANDLE clause_body" => f!(|c, a| {
            let cs = fwd(a.t(3).clauses());
            let e = a.t(1).exp();
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::Handle(Box::new(e), cs)),
            )))
        }),
        "or_exp: or_exp BIG_VERTICAL and_exp" => f!(|c, a| binop(c, a, BinOp::Or)),
        "and_exp: and_exp BIG_AMPERSAND cmp_exp" => f!(|c, a| binop(c, a, BinOp::And)),
        "cmp_exp: add_exp BIG_EQ add_exp" => f!(|c, a| binop(c, a, BinOp::Eq)),
        "cmp_exp: add_exp EXCLAMATION_EQ add_exp" => f!(|c, a| binop(c, a, BinOp::Ne)),
        "cmp_exp: add_exp LESS add_exp" => f!(|c, a| binop(c, a, BinOp::Lt)),
        "cmp_exp: add_exp LESS_EQ add_exp" => f!(|c, a| binop(c, a, BinOp::Le)),
        "cmp_exp: add_exp GREATER add_exp" => f!(|c, a| binop(c, a, BinOp::Gt)),
        "cmp_exp: add_exp GREATER_EQ add_exp" => f!(|c, a| binop(c, a, BinOp::Ge)),
        "add_exp: add_exp PLUS mul_exp" => f!(|c, a| binop(c, a, BinOp::Add)),
        "add_exp: add_exp HYPHEN mul_exp" => f!(|c, a| binop(c, a, BinOp::Sub)),
        "mul_exp: mul_exp ASTERISK prefix_exp" => f!(|c, a| binop(c, a, BinOp::Mul)),
        "mul_exp: mul_exp SOLIDUS prefix_exp" => f!(|c, a| binop(c, a, BinOp::Div)),
        "mul_exp: mul_exp PERCENT prefix_exp" => f!(|c, a| binop(c, a, BinOp::Rem)),
        "prefix_exp: EXCLAMATION prefix_exp" => f!(|c, a| {
            let e = a.t(2).exp();
            Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::Not(Box::new(e))))))
        }),
        "prefix_exp: HYPHEN prefix_exp" => f!(|c, a| {
            let (sp, ep) = a.loc(2);
            let e = a.t(2).exp();
            Ok(Sv::Exp(Box::new(negate(c, a.sloc, sp, ep, e))))
        }),
        "postfix_exp: postfix_exp DOT lower_id" | "postfix_exp: postfix_exp DOT upper_id" => {
            f!(|c, a| {
                let id_pos = a.sp(3);
                let id = a.t(3).s();
                let e = a.t(1).exp();
                Ok(Sv::Exp(Box::new(dot_select(c, a.sloc, e, id_pos, id)?)))
            })
        }
        "postfix_exp: postfix_exp BACKSLASH lower_id" => f!(|c, a| {
            let l = a.t(3).s();
            let e = a.t(1).exp();
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::RecordRestriction(Box::new(e), l)),
            )))
        }),
        "postfix_exp: postfix_exp LPAREN call_args RPAREN" => f!(|c, a| {
            let args = fwd(a.t(3).args());
            let e = a.t(1).exp();
            Ok(Sv::Exp(Box::new(call(c, a.sloc, e, args)?)))
        }),
        "tb_call: postfix_exp LPAREN call_args RPAREN trailing_block" => f!(|c, a| {
            let (lb, pe) = (a.sp(5), a.ep(4));
            let k = a.t(5).exp();
            let args = fwd(a.t(3).args());
            let e = a.t(1).exp();
            Ok(Sv::Exp(Box::new(trailing_call(
                c, a.sloc, e, args, lb, pe, k,
            )?)))
        }),
        "tb_call: postfix_exp DOT lower_id trailing_block" => f!(|c, a| {
            let (lb, pe) = (a.sp(4), a.ep(3));
            let k = a.t(4).exp();
            let id_pos = a.sp(3);
            let id = a.t(3).s();
            let e = a.t(1).exp();
            let callee = dot_select(c, a.sloc, e, id_pos, id)?;
            Ok(Sv::Exp(Box::new(trailing_call(
                c,
                a.sloc,
                callee,
                vec![],
                lb,
                pe,
                k,
            )?)))
        }),
        "tb_call: lower_id trailing_block" => f!(|c, a| {
            let (lb, pe) = (a.sp(2), a.ep(1));
            let k = a.t(2).exp();
            let l1 = a.loc(1);
            let x = a.t(1).s();
            let callee = c.exp(l1, ExpKind::Ident(LongId(vec![x])));
            Ok(Sv::Exp(Box::new(trailing_call(
                c,
                a.sloc,
                callee,
                vec![],
                lb,
                pe,
                k,
            )?)))
        }),
        "tb_call: postfix_exp LPAREN call_args RPAREN trailing_other" => {
            f!(|c, a| trailing_other_err(a.sp(5), a.t(5).trailother()))
        }
        "tb_call: postfix_exp DOT lower_id trailing_other" => {
            f!(|c, a| trailing_other_err(a.sp(4), a.t(4).trailother()))
        }
        "tb_call: lower_id trailing_other" => {
            f!(|c, a| trailing_other_err(a.sp(2), a.t(2).trailother()))
        }
        "tb_call: tb_call trailing_block" | "tb_call: tb_call trailing_other" => {
            f!(|c, a| syntax_error(a.sp(2), "末尾ブロックは 1 つの呼び出しに 1 個までです"))
        }
        "tb_call: upper_id tb_brace" | "tb_call: HASH_IDENT tb_brace" => {
            f!(|c, a| trailing_misplaced(a.sp(2)))
        }
        "tb_call: postfix_exp DOT upper_id tb_brace" => f!(|c, a| trailing_misplaced(a.sp(4))),
        "tb_call: paren_exp tb_brace" => f!(|c, a| syntax_error(
            a.sp(2),
            "括弧で囲んだ式には末尾ブロックを付けられません(関数を括弧の中の引数として渡します)"
        )),
        "trailing_other: record_exp" => f!(|c, a| Ok(Sv::TrailOther(TrailingOther::Record))),
        "trailing_other: LBRACE_RECORD field_list EQ_GREATER" => f!(|c, a| {
            let p2 = a.sp(2);
            let fs = a.t(2).rfields();
            let punned = |(l, e): &(String, Exp)| matches!(&e.kind, ExpKind::Ident(LongId(v)) if v.len() == 1 && &v[0] == l);
            Ok(Sv::TrailOther(if fs.len() >= 2 && fs.iter().all(punned) {
                TrailingOther::Params(p2)
            } else {
                TrailingOther::Record
            }))
        }),
        "trailing_other: LBRACE_TYPE lower_id" | "trailing_other: LBRACE_TYPE upper_id" => {
            f!(|c, a| Ok(Sv::TrailOther(TrailingOther::Annot(a.sp(2)))))
        }
        "tb_brace: LBRACE_BLOCK" | "tb_brace: LBRACE_RECORD" | "tb_brace: LBRACE_TYPE" => {
            f!(|c, a| Ok(Sv::None))
        }
        "trailing_block: block" => f!(|c, a| {
            let b = a.t(1).exp();
            Ok(Sv::Exp(Box::new(c.exp(
                a.sloc,
                ExpKind::Lambda(Lambda {
                    params: vec![],
                    body: Box::new(b),
                }),
            ))))
        }),
        "lambda_block: LBRACE_BLOCK lower_id EQ_GREATER items RBRACE" => f!(|c, a| {
            let l4 = a.loc(4);
            let items = fwd(a.t(4).decls());
            let body = block_of_items(c, l4, items)?;
            let l2 = a.loc(2);
            let x = a.t(2).s();
            let p = c.pat(l2, PatKind::Var(x));
            Ok(Sv::Exp(Box::new(c.exp(
                a.sloc,
                ExpKind::Lambda(Lambda {
                    params: vec![p],
                    body: Box::new(body),
                }),
            ))))
        }),
        "lambda_block: LBRACE_BLOCK LOWLINE EQ_GREATER items RBRACE" => f!(|c, a| syntax_error(
            a.sp(2),
            "末尾ブロックの引数に _ は書けません(引数なしは { 式 }、引数を捨てる 1 引数は { case _ => 式 })"
        )),
        "lambda_block: LBRACE_BLOCK paren_exp EQ_GREATER" => f!(|c, a| syntax_error(
            a.sp(2),
            "末尾ブロックの引数は小文字の識別子 1 個です(引数のない関数は { 式 }、タプルを分解する関数は { case (a, b) => 式 } と書きます)"
        )),
        "lambda_block: LBRACE_BLOCK clause clauses RBRACE" => f!(|c, a| {
            let mut cs = a.t(3).clauses();
            cs.push(a.t(2).clause());
            let cs = fwd(cs);
            let v = c.exp(a.sloc, ExpKind::Ident(LongId(vec!["%arg".to_string()])));
            let body = c.exp(a.sloc, ExpKind::Match(Box::new(v), cs));
            let p = c.pat(a.sloc, PatKind::Var("%arg".to_string()));
            Ok(Sv::Exp(Box::new(c.exp(
                a.sloc,
                ExpKind::Lambda(Lambda {
                    params: vec![p],
                    body: Box::new(body),
                }),
            ))))
        }),
        "atom: NUMBER" => f!(|c, a| {
            let n = a.t(1).number();
            Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::Number(n)))))
        }),
        "atom: TEXT" => f!(|c, a| {
            let s = a.t(1).text();
            Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::Text(s)))))
        }),
        "atom: BOOL" => f!(|c, a| {
            let b = a.t(1).boolean();
            Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::Bool(b)))))
        }),
        "atom: HOLE" => f!(|c, a| Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::Hole))))),
        "atom: lower_id" | "atom: upper_id" => f!(|c, a| {
            let x = a.t(1).s();
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::Ident(LongId(vec![x]))),
            )))
        }),
        "atom: HASH_IDENT" => f!(|c, a| {
            let s = a.t(1).text();
            let empty = c.exp(a.sloc, ExpKind::RecordEmpty);
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::Variant(s, Box::new(empty))),
            )))
        }),
        "atom: PERFORM perform_op LPAREN call_args RPAREN" => f!(|c, a| {
            let args = fwd(a.t(4).args());
            let op = a.t(2).lid();
            let row = record_of_args(c, a.sloc, args);
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::Perform(op, Box::new(row))),
            )))
        }),
        "atom: RESUME LPAREN call_args RPAREN" => f!(|c, a| {
            let mut args = fwd(a.t(3).args());
            if args.is_empty() {
                return Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::Resume(None)))));
            }
            if args.len() == 1 {
                let (l, e) = args.pop().expect("1 個");
                if l.is_some() {
                    return syntax_error(a.start(), "resume の引数にラベルは付けられません");
                }
                return Ok(Sv::Exp(Box::new(
                    c.exp(a.sloc, ExpKind::Resume(Some(Box::new(e)))),
                )));
            }
            syntax_error(a.start(), "resume の引数は高々1個です")
        }),
        "atom: PERFORM perform_op LPAREN call_args RPAREN trailing_block" => {
            f!(|c, a| syntax_error(a.sp(6), "perform には末尾ブロックを付けられません"))
        }
        "atom: RESUME LPAREN call_args RPAREN trailing_block" => {
            f!(|c, a| syntax_error(a.sp(5), "resume には末尾ブロックを付けられません"))
        }
        "atom: RUN lower_id run_body" => f!(|c, a| {
            let body = a.t(3).exp();
            let h = a.t(2).s();
            Ok(Sv::Exp(Box::new(
                c.exp(a.sloc, ExpKind::Run(h, Box::new(body))),
            )))
        }),
        "atom: lambda_block" => f!(|c, a| syntax_error(
            a.start(),
            "{ x => ... } と { case ... } は末尾ブロックとしてだけ書けます(呼び出しの直後の同じ行に置くか、fn(x) => ... と書きます)"
        )),
        "perform_op: long_id" => f!(|c, a| Ok(Sv::LongId(a.t(1).lid()))),
        "block: LBRACE_BLOCK items RBRACE"
        | "run_body: LBRACE_BLOCK items RBRACE"
        | "run_body: LBRACE_RECORD items RBRACE"
        | "run_body: LBRACE_TYPE items RBRACE" => f!(|c, a| {
            let items = fwd(a.t(2).decls());
            Ok(Sv::Exp(Box::new(block_of_items(c, a.sloc, items)?)))
        }),
        "call_args:" => f!(|c, a| Ok(Sv::Args(vec![]))),
        "call_args: arg_list" => f!(|c, a| Ok(Sv::Args(a.t(1).args()))),
        "arg_list: arg" | "arg_list: arg COMMA" => f!(|c, a| Ok(Sv::Args(vec![a.t(1).arg()]))),
        "arg_list: arg COMMA arg_list" => f!(|c, a| {
            let mut l = a.t(3).args();
            l.push(a.t(1).arg());
            Ok(Sv::Args(l))
        }),
        "arg: lower_id EQ exp" => f!(|c, a| {
            let e = a.t(3).exp();
            Ok(Sv::Arg(Box::new((Some(a.t(1).s()), e))))
        }),
        "arg: exp" => f!(|c, a| Ok(Sv::Arg(Box::new((None, a.t(1).exp()))))),
        "record_exp: LBRACE_RECORD RBRACE" => {
            f!(|c, a| Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::RecordEmpty)))))
        }
        "record_exp: LBRACE_RECORD EXTENDS exp RBRACE" => {
            f!(|c, a| Ok(Sv::Exp(Box::new(a.t(3).exp()))))
        }
        "record_exp: LBRACE_RECORD field_list RBRACE" => f!(|c, a| {
            let fs = fwd(a.t(2).rfields());
            let args = fs.into_iter().map(|(l, e)| (Some(l), e)).collect();
            Ok(Sv::Exp(Box::new(record_of_args(c, a.sloc, args))))
        }),
        "record_exp: LBRACE_RECORD field_list EXTENDS exp RBRACE" => f!(|c, a| {
            let mut row = a.t(4).exp();
            let fs = a.t(2).rfields();
            // 逆順の列なので、そのまま回すと末尾のフィールドから積める。
            for (l, v) in fs {
                row = c.exp(a.sloc, ExpKind::RecordExtend(Box::new(row), l, Box::new(v)));
            }
            Ok(Sv::Exp(Box::new(row)))
        }),
        "record_exp: LBRACE_RECORD exp WITH field_list RBRACE" => f!(|c, a| {
            let p2 = a.sp(2);
            let fs = fwd(a.t(4).rfields());
            let base = a.t(2).exp();
            match &base.kind {
                ExpKind::Ident(LongId(v)) if v.len() == 1 && !is_upper(&v[0]) => {}
                _ => return syntax_error(p2, "レコード更新の基底は小文字始まりの識別子 1 個です"),
            }
            let mut acc = base;
            for (l, v) in fs {
                acc = c.exp(a.sloc, ExpKind::RecordUpdate(Box::new(acc), l, Box::new(v)));
            }
            Ok(Sv::Exp(Box::new(acc)))
        }),
        "field_list: field" | "field_list: field COMMA" => {
            f!(|c, a| Ok(Sv::RFields(vec![a.t(1).rfield()])))
        }
        "field_list: field COMMA field_list" => f!(|c, a| {
            let mut l = a.t(3).rfields();
            l.push(a.t(1).rfield());
            Ok(Sv::RFields(l))
        }),
        "field: lower_id EQ exp" => f!(|c, a| {
            let e = a.t(3).exp();
            Ok(Sv::RField(Box::new((a.t(1).s(), e))))
        }),
        "field: lower_id" => f!(|c, a| {
            let l = a.t(1).s();
            let e = c.exp(a.sloc, ExpKind::Ident(LongId(vec![l.clone()])));
            Ok(Sv::RField(Box::new((l, e))))
        }),
        "paren_exp: LPAREN RPAREN" => {
            f!(|c, a| Ok(Sv::Exp(Box::new(c.exp(a.sloc, ExpKind::RecordEmpty)))))
        }
        "paren_exp: LPAREN exp RPAREN" => f!(|c, a| Ok(Sv::Exp(Box::new(a.t(2).exp())))),
        "paren_exp: LPAREN exp COMMA RPAREN" => f!(|c, a| {
            let e = a.t(2).exp();
            Ok(Sv::Exp(Box::new(tuple_exp(c, a.sloc, vec![e]))))
        }),
        "paren_exp: LPAREN exp COMMA exp_list RPAREN" => f!(|c, a| {
            let mut l = a.t(4).exps();
            l.push(a.t(2).exp());
            Ok(Sv::Exp(Box::new(tuple_exp(c, a.sloc, fwd(l)))))
        }),
        "exp_list: exp" | "exp_list: exp COMMA" => f!(|c, a| Ok(Sv::Exps(vec![a.t(1).exp()]))),
        "exp_list: exp COMMA exp_list" => f!(|c, a| {
            let mut l = a.t(3).exps();
            l.push(a.t(1).exp());
            Ok(Sv::Exps(l))
        }),

        // ---- 節 ----
        "clause_body: LBRACE_BLOCK clauses RBRACE"
        | "clause_body: LBRACE_RECORD clauses RBRACE"
        | "clause_body: LBRACE_TYPE clauses RBRACE" => {
            f!(|c, a| lbrace_body(a, Sv::Clauses, Sv::clauses))
        }
        "clauses:" => f!(|c, a| Ok(Sv::Clauses(vec![]))),
        "clauses: clause clauses" => f!(|c, a| {
            let mut l = a.t(2).clauses();
            l.push(a.t(1).clause());
            Ok(Sv::Clauses(l))
        }),
        "clause: CASE pat guard_opt EQ_GREATER exp" => f!(|c, a| {
            let body = a.t(5).exp();
            let guard = a.t(3).oexp();
            let pat = a.t(2).pat();
            Ok(Sv::Clause(Box::new(c.clause(a.sloc, pat, guard, body))))
        }),
        "guard_opt:" => f!(|c, a| Ok(Sv::OptExp(Box::new(None)))),
        "guard_opt: IF exp" => f!(|c, a| Ok(Sv::OptExp(Box::new(Some(a.t(2).exp()))))),

        // ---- パターン ----
        "pat: LOWLINE" | "bind_pat: LOWLINE" => {
            f!(|c, a| Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Wildcard)))))
        }
        "pat: NUMBER" => f!(|c, a| {
            let n = a.t(1).number();
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Number(n)))))
        }),
        "pat: HYPHEN NUMBER" => f!(|c, a| {
            let mut n = a.t(2).number();
            n.text = format!("-{}", n.text);
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Number(n)))))
        }),
        "pat: TEXT" => f!(|c, a| {
            let s = a.t(1).text();
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Text(s)))))
        }),
        "pat: BOOL" => f!(|c, a| {
            let b = a.t(1).boolean();
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Bool(b)))))
        }),
        "pat: long_id" => f!(|c, a| {
            let LongId(v) = a.t(1).lid();
            let k = if v.len() == 1 && !is_upper(&v[0]) {
                PatKind::Var(v.into_iter().next().expect("1 個"))
            } else {
                PatKind::Ctor(LongId(v), vec![])
            };
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, k))))
        }),
        "pat: long_id LPAREN pat_args RPAREN" => f!(|c, a| {
            let args = fwd(a.t(3).cappats());
            let li = a.t(1).lid();
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Ctor(li, args)))))
        }),
        "pat: HASH_IDENT" => f!(|c, a| {
            let s = a.t(1).text();
            let inner = c.pat(a.sloc, PatKind::Record(vec![], None));
            Ok(Sv::Pat(Box::new(
                c.pat(a.sloc, PatKind::Variant(s, Box::new(inner))),
            )))
        }),
        "pat: HASH_IDENT LPAREN pat_args RPAREN" => f!(|c, a| {
            let args = fwd(a.t(3).cappats());
            let s = a.t(1).text();
            let p = variant_pat_payload(c, a.sloc, args)?;
            Ok(Sv::Pat(Box::new(
                c.pat(a.sloc, PatKind::Variant(s, Box::new(p))),
            )))
        }),
        "pat: record_pat" | "pat: paren_pat" | "bind_pat: record_pat" | "bind_pat: paren_pat" => {
            f!(|c, a| Ok(Sv::Pat(Box::new(a.t(1).pat()))))
        }
        "bind_pat: lower_id" => f!(|c, a| {
            let x = a.t(1).s();
            Ok(Sv::Pat(Box::new(c.pat(a.sloc, PatKind::Var(x)))))
        }),
        "bind_pat: upper_path" => f!(|c, a| {
            let p = a.t(1).strs();
            Ok(Sv::Pat(Box::new(
                c.pat(a.sloc, PatKind::Ctor(LongId(p), vec![])),
            )))
        }),
        "bind_pat: upper_path LPAREN pat_args RPAREN" => f!(|c, a| {
            let args = fwd(a.t(3).cappats());
            let p = a.t(1).strs();
            Ok(Sv::Pat(Box::new(
                c.pat(a.sloc, PatKind::Ctor(LongId(p), args)),
            )))
        }),
        "pat_args:" => f!(|c, a| Ok(Sv::CapPats(vec![]))),
        "pat_args: pat_arg_list" => f!(|c, a| Ok(Sv::CapPats(a.t(1).cappats()))),
        "pat_arg_list: pat_arg" | "pat_arg_list: pat_arg COMMA" => {
            f!(|c, a| Ok(Sv::CapPats(vec![a.t(1).cappat()])))
        }
        "pat_arg_list: pat_arg COMMA pat_arg_list" => f!(|c, a| {
            let mut l = a.t(3).cappats();
            l.push(a.t(1).cappat());
            Ok(Sv::CapPats(l))
        }),
        "pat_arg: lower_id EQ pat" => f!(|c, a| {
            let pat = a.t(3).pat();
            Ok(Sv::CapPat(Box::new(CtorArgPat {
                label: Some(a.t(1).s()),
                pat,
            })))
        }),
        "pat_arg: pat" => f!(|c, a| Ok(Sv::CapPat(Box::new(CtorArgPat {
            label: None,
            pat: a.t(1).pat()
        })))),
        "record_pat: LBRACE_BLOCK rp_items RBRACE"
        | "record_pat: LBRACE_RECORD rp_items RBRACE"
        | "record_pat: LBRACE_TYPE rp_items RBRACE" => f!(|c, a| {
            let (fields, rest) = a.t(2).rpitems();
            let rest = match rest {
                Some(r) => r,
                None => c.pat(a.sloc, PatKind::Wildcard),
            };
            Ok(Sv::Pat(Box::new(c.pat(
                a.sloc,
                PatKind::Record(fwd(fields), Some(Box::new(rest))),
            ))))
        }),
        "rp_items:" => f!(|c, a| Ok(Sv::RpItems(Box::new((vec![], None))))),
        "rp_items: DOTDOTDOT lower_id" => f!(|c, a| {
            let x = a.t(2).s();
            let p = c.pat(a.sloc, PatKind::Var(x));
            Ok(Sv::RpItems(Box::new((vec![], Some(p)))))
        }),
        "rp_items: pat_field" => {
            f!(|c, a| Ok(Sv::RpItems(Box::new((vec![a.t(1).pfield()], None)))))
        }
        "rp_items: pat_field COMMA rp_items" => f!(|c, a| {
            let (mut fs, r) = a.t(3).rpitems();
            fs.push(a.t(1).pfield());
            Ok(Sv::RpItems(Box::new((fs, r))))
        }),
        "pat_field: lower_id EQ pat" => f!(|c, a| {
            let p = a.t(3).pat();
            Ok(Sv::PField(Box::new((a.t(1).s(), p))))
        }),
        "pat_field: lower_id" => f!(|c, a| {
            let l = a.t(1).s();
            let p = c.pat(a.sloc, PatKind::Var(l.clone()));
            Ok(Sv::PField(Box::new((l, p))))
        }),
        "paren_pat: LPAREN pp_items RPAREN" => f!(|c, a| {
            let (ps, rest, comma) = a.t(2).ppitems();
            if ps.len() == 1 && rest.is_none() && !comma {
                return Ok(Sv::Pat(Box::new(ps.into_iter().next().expect("1 個"))));
            }
            Ok(Sv::Pat(Box::new(pat_tuple(c, a.sloc, fwd(ps), rest))))
        }),
        "pp_items:" => f!(|c, a| Ok(Sv::PpItems(Box::new((vec![], None, true))))),
        "pp_items: DOTDOTDOT lower_id" => f!(|c, a| {
            let x = a.t(2).s();
            let p = c.pat(a.sloc, PatKind::Var(x));
            Ok(Sv::PpItems(Box::new((vec![], Some(p), true))))
        }),
        "pp_items: pat" => f!(|c, a| Ok(Sv::PpItems(Box::new((vec![a.t(1).pat()], None, false))))),
        "pp_items: pat COMMA pp_items" => f!(|c, a| {
            let (mut ps, r, _) = a.t(3).ppitems();
            ps.push(a.t(1).pat());
            Ok(Sv::PpItems(Box::new((ps, r, true))))
        }),

        // ---- 型 ----
        "ty: union_ty" | "ty: arrow_ty" | "union_ty: app_ty" | "app_ty: atom_ty"
        | "atom_ty: brace_ty" | "ty_arg: ty" | "eff: brace_ty" => {
            f!(|c, a| Ok(Sv::Ty(Box::new(a.t(1).ty()))))
        }
        "arrow_ty: LPAREN paren_ty_items RPAREN EQ_GREATER arrow_ret" => f!(|c, a| {
            let (ret, eff) = a.t(5).arrowret();
            let (params, _) = a.t(2).tyitems();
            Ok(Sv::Ty(Box::new(c.ty(
                a.sloc,
                TypeExpKind::Arrow(fwd(params), Box::new(ret), eff.map(Box::new)),
            ))))
        }),
        "arrow_ret: union_ty eff_opt" => f!(|c, a| {
            let e = a.t(2).oty();
            Ok(Sv::ArrowRet(Box::new((a.t(1).ty(), e))))
        }),
        "arrow_ret: arrow_ty" => f!(|c, a| Ok(Sv::ArrowRet(Box::new((a.t(1).ty(), None))))),
        "eff_opt:" => f!(|c, a| Ok(Sv::OptTy(Box::new(None)))),
        "eff_opt: AT eff" => f!(|c, a| Ok(Sv::OptTy(Box::new(Some(a.t(2).ty()))))),
        "union_ty: union_ty VERTICAL app_ty" => f!(|c, a| {
            let t3 = a.t(3).ty();
            let t1 = a.t(1).ty();
            let k = match t1.kind {
                TypeExpKind::Union(mut ts) => {
                    ts.push(t3);
                    TypeExpKind::Union(ts)
                }
                kind => TypeExpKind::Union(vec![
                    TypeExp {
                        meta: t1.meta,
                        kind,
                    },
                    t3,
                ]),
            };
            Ok(Sv::Ty(Box::new(c.ty(a.sloc, k))))
        }),
        "app_ty: atom_ty LBRACKET ty_args RBRACKET" => f!(|c, a| {
            let args = fwd(a.t(3).tys());
            let f = a.t(1).ty();
            Ok(Sv::Ty(Box::new(
                c.ty(a.sloc, TypeExpKind::Apply(Box::new(f), args)),
            )))
        }),
        "ty_args: ty_arg" | "ty_args: ty_arg COMMA" | "ty_list: ty" | "ty_list: ty COMMA" => {
            f!(|c, a| Ok(Sv::Tys(vec![a.t(1).ty()])))
        }
        "ty_args: ty_arg COMMA ty_args" | "ty_list: ty COMMA ty_list" => f!(|c, a| {
            let mut l = a.t(3).tys();
            l.push(a.t(1).ty());
            Ok(Sv::Tys(l))
        }),
        "ty_arg: LOWLINE" => f!(|c, a| Ok(Sv::Ty(Box::new(c.ty(a.sloc, TypeExpKind::Hole))))),
        "atom_ty: long_id" => f!(|c, a| {
            let li = a.t(1).lid();
            Ok(Sv::Ty(Box::new(c.ty(a.sloc, TypeExpKind::Ident(li)))))
        }),
        "atom_ty: HASH_IDENT" => f!(|c, a| {
            let s = a.t(1).text();
            Ok(Sv::Ty(Box::new(
                c.ty(a.sloc, TypeExpKind::VariantCase(s, None)),
            )))
        }),
        "atom_ty: HASH_IDENT LPAREN ty_list0 RPAREN" => f!(|c, a| {
            let ts = fwd(a.t(3).tys());
            let s = a.t(1).text();
            let payload = match ts.len() {
                0 => None,
                1 => ts.into_iter().next(),
                _ => Some(tuple_ty(c, a.sloc, ts)),
            };
            Ok(Sv::Ty(Box::new(c.ty(
                a.sloc,
                TypeExpKind::VariantCase(s, payload.map(Box::new)),
            ))))
        }),
        "atom_ty: LPAREN paren_ty_items RPAREN" => f!(|c, a| {
            let (tys, comma) = a.t(2).tyitems();
            if tys.len() == 1 && !comma {
                return Ok(Sv::Ty(Box::new(tys.into_iter().next().expect("1 個"))));
            }
            Ok(Sv::Ty(Box::new(tuple_ty(c, a.sloc, fwd(tys)))))
        }),
        "ty_list0:" => f!(|c, a| Ok(Sv::Tys(vec![]))),
        "ty_list0: ty_list" => f!(|c, a| Ok(Sv::Tys(a.t(1).tys()))),
        "paren_ty_items:" => f!(|c, a| Ok(Sv::TyItems(Box::new((vec![], true))))),
        "paren_ty_items: ty" => f!(|c, a| Ok(Sv::TyItems(Box::new((vec![a.t(1).ty()], false))))),
        "paren_ty_items: ty COMMA" => {
            f!(|c, a| Ok(Sv::TyItems(Box::new((vec![a.t(1).ty()], true)))))
        }
        "paren_ty_items: ty COMMA ty_list" => f!(|c, a| {
            let mut l = a.t(3).tys();
            l.push(a.t(1).ty());
            Ok(Sv::TyItems(Box::new((l, true))))
        }),
        "brace_ty: LBRACE_BLOCK RBRACE"
        | "brace_ty: LBRACE_RECORD RBRACE"
        | "brace_ty: LBRACE_TYPE RBRACE" => {
            f!(|c, a| Ok(Sv::Ty(Box::new(
                c.ty(a.sloc, TypeExpKind::BraceRow(vec![], None))
            ))))
        }
        "brace_ty: LBRACE_BLOCK EXTENDS ty RBRACE"
        | "brace_ty: LBRACE_RECORD EXTENDS ty RBRACE"
        | "brace_ty: LBRACE_TYPE EXTENDS ty RBRACE" => f!(|c, a| {
            let t = a.t(3).ty();
            Ok(Sv::Ty(Box::new(c.ty(
                a.sloc,
                TypeExpKind::BraceRow(vec![], Some(Box::new(t))),
            ))))
        }),
        "brace_ty: LBRACE_BLOCK brace_item_list RBRACE"
        | "brace_ty: LBRACE_RECORD brace_item_list RBRACE"
        | "brace_ty: LBRACE_TYPE brace_item_list RBRACE" => f!(|c, a| {
            let items = fwd(a.t(2).belems());
            Ok(Sv::Ty(Box::new(
                c.ty(a.sloc, TypeExpKind::BraceRow(items, None)),
            )))
        }),
        "brace_ty: LBRACE_BLOCK brace_item_list EXTENDS ty RBRACE"
        | "brace_ty: LBRACE_RECORD brace_item_list EXTENDS ty RBRACE"
        | "brace_ty: LBRACE_TYPE brace_item_list EXTENDS ty RBRACE" => f!(|c, a| {
            let t = a.t(4).ty();
            let items = fwd(a.t(2).belems());
            Ok(Sv::Ty(Box::new(c.ty(
                a.sloc,
                TypeExpKind::BraceRow(items, Some(Box::new(t))),
            ))))
        }),
        "brace_item_list: brace_item" | "brace_item_list: brace_item COMMA" => {
            f!(|c, a| Ok(Sv::BraceElems(vec![a.t(1).belem()])))
        }
        "brace_item_list: brace_item COMMA brace_item_list" => f!(|c, a| {
            let mut l = a.t(3).belems();
            l.push(a.t(1).belem());
            Ok(Sv::BraceElems(l))
        }),
        "brace_item: lower_id COLON ty" => f!(|c, a| {
            let t = a.t(3).ty();
            Ok(Sv::BraceElem(Box::new(BraceElem::Field(a.t(1).s(), t))))
        }),
        "brace_item: eff_name" => f!(|c, a| {
            let (li, args) = a.t(1).effname();
            Ok(Sv::BraceElem(Box::new(BraceElem::Label(li, args))))
        }),
        "eff: eff_name" => f!(|c, a| {
            let (li, args) = a.t(1).effname();
            if args.is_empty() {
                return Ok(Sv::Ty(Box::new(c.ty(a.sloc, TypeExpKind::Ident(li)))));
            }
            let head = c.ty(a.sloc, TypeExpKind::Ident(li));
            Ok(Sv::Ty(Box::new(
                c.ty(a.sloc, TypeExpKind::Apply(Box::new(head), args)),
            )))
        }),
        "eff_name: long_id" => f!(|c, a| Ok(Sv::EffName(Box::new((a.t(1).lid(), vec![]))))),
        "eff_name: long_id LBRACKET ty_args RBRACKET" => f!(|c, a| {
            let args = fwd(a.t(3).tys());
            Ok(Sv::EffName(Box::new((a.t(1).lid(), args))))
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn すべての規則に動作がある() {
        assert_eq!(super::unmapped_keys(), Vec::<&str>::new());
    }
}
