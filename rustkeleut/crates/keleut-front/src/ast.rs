//! 脱糖した AST である(Diktor の syntax.ml の `Syntax.Make`)。
//!
//! 形は Diktor の木と 1 対 1 に対応する。
//! Diktor の木の各ノードに付く注釈(`Data.t`)は、本 crate では [`Meta`] で、
//! ノードの番号 [`NodeId`] と範囲を持つ。
//! 後の段(精緻化)の注釈は、番号で引く別表に置く。
//! Diktor が注釈をそのまま借りる箇所(`block_of_items` の `Let`、`with` の `Apply`)では、
//! 本 crate も [`Meta`] を写すので、同じ番号の 2 つのノードはできない
//! (借りた元のノードはその場で捨てる)。

use crate::pos::Span;
pub use crate::token::{NumSuffix, Number};

/// ノードの番号である。
pub type NodeId = u32;

/// ノードの注釈である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    /// 番号。
    pub id: NodeId,
    /// 範囲。
    pub span: Span,
    /// このノードを根とする部分木の深さ(葉が 1)。
    ///
    /// 構文解析は、作るノードの深さが限度を超えたら「処理系の限度」で止まる。
    /// そのため、後の段は木の深さが限度の内側にあることを前提に、再帰で木をたどってよい。
    pub depth: u32,
}

/// 名前のパス(`A.B.c`)である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongId(pub Vec<String>);

impl LongId {
    /// `.` でつないだ表示。
    pub fn show(&self) -> String {
        self.0.join(".")
    }
}

/// import の名前である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportName {
    /// パス(1 段か 2 段)。
    pub path: Vec<String>,
    /// 範囲。
    pub span: Span,
}

/// import 文である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportDecl {
    /// `from` の後ろの文字列。
    pub source: String,
    /// 名前。
    pub names: Vec<ImportName>,
    /// 範囲。
    pub span: Span,
}

/// 二項演算子である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    /// 字面(Diktor の `Prims.show_bin_op`)。
    pub fn show(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            Eq => "==",
            Ne => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            And => "&&",
            Or => "||",
        }
    }
}

/// 型パラメータの束縛子である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeParam {
    /// 名前(大文字も小文字も取る)。
    pub name: String,
    /// `F[_]` の穴の数(0 ならカインドは推論)。
    pub arity: u32,
    /// クラス制約。
    pub classes: Vec<LongId>,
}

/// 型式である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeExp {
    pub meta: Meta,
    pub kind: TypeExpKind,
}

/// 型式の形である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeExpKind {
    Ident(LongId),
    Apply(Box<TypeExp>, Vec<TypeExp>),
    Arrow(Vec<TypeExp>, Box<TypeExp>, Option<Box<TypeExp>>),
    BraceRow(Vec<BraceElem>, Option<Box<TypeExp>>),
    VariantCase(String, Option<Box<TypeExp>>),
    Union(Vec<TypeExp>),
    Hole,
}

/// `{ ... }` の型の要素である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BraceElem {
    Field(String, TypeExp),
    Label(LongId, Vec<TypeExp>),
}

/// パターンである。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pat {
    pub meta: Meta,
    pub kind: PatKind,
}

/// パターンの形である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatKind {
    Wildcard,
    Var(String),
    Bool(bool),
    Number(Number),
    Text(String),
    Record(Vec<(String, Pat)>, Option<Box<Pat>>),
    Ctor(LongId, Vec<CtorArgPat>),
    Variant(String, Box<Pat>),
    Annot(Box<Pat>, TypeExp),
}

/// コンストラクタのパターンの引数である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorArgPat {
    pub label: Option<String>,
    pub pat: Pat,
}

/// 式である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exp {
    pub meta: Meta,
    pub kind: ExpKind,
}

/// 式の形である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpKind {
    Bool(bool),
    Number(Number),
    Text(String),
    Ident(LongId),
    Hole,
    Apply(Box<Exp>, Box<Exp>),
    Construct(LongId, Vec<CtorArg>),
    Variant(String, Box<Exp>),
    BinOp(Box<Exp>, BinOp, Box<Exp>),
    Not(Box<Exp>),
    Neg(Box<Exp>),
    Lambda(Lambda),
    Let(Box<LetBinding>, Box<Exp>),
    LetRec(Vec<LetBinding>, Box<Exp>),
    Seq(Vec<Exp>),
    Match(Box<Exp>, Vec<Clause>),
    RecordEmpty,
    RecordExtend(Box<Exp>, String, Box<Exp>),
    RecordUpdate(Box<Exp>, String, Box<Exp>),
    RecordRestriction(Box<Exp>, String),
    RecordSelection(Box<Exp>, String),
    Perform(LongId, Box<Exp>),
    Handle(Box<Exp>, Vec<Clause>),
    Resume(Option<Box<Exp>>),
    Run(String, Box<Exp>),
}

/// コンストラクタの引数である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorArg {
    pub label: Option<String>,
    pub exp: Exp,
}

/// 無名関数である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lambda {
    pub params: Vec<Pat>,
    pub body: Box<Exp>,
}

/// `case` の節である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    pub meta: Meta,
    pub pat: Pat,
    pub guard: Option<Exp>,
    pub body: Exp,
}

/// `let` の束縛である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LetBinding {
    pub meta: Meta,
    pub is_pub: bool,
    pub name: Pat,
    pub tparams: Vec<TypeParam>,
    /// `None` は値の束縛、`Some` は関数の定義。
    pub params: Option<Vec<Pat>>,
    pub ret: Option<TypeExp>,
    pub eff: Option<TypeExp>,
    pub body: Exp,
}

/// 型の別名の宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeAlias {
    pub is_pub: bool,
    pub name: String,
    pub params: Vec<TypeParam>,
    pub kind: Option<String>,
    pub body: TypeExp,
}

/// コンストラクタの宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

/// コンストラクタのフィールドである。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDecl {
    pub label: Option<String>,
    pub ty: TypeExp,
}

/// `newtype` の宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Newtype {
    pub is_pub: bool,
    pub name: String,
    pub params: Vec<TypeParam>,
    /// コンストラクタの並び(`Never` は空)。
    pub ctors: Vec<CtorDecl>,
}

/// `effect` の宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    pub is_pub: bool,
    pub name: String,
    pub params: Vec<TypeParam>,
    pub ops: Vec<(String, TypeExp)>,
}

/// クラスの `val` である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassVal {
    pub name: String,
    pub tparams: Vec<TypeParam>,
    pub ty: TypeExp,
}

/// `type class` の宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassDecl {
    pub is_pub: bool,
    pub name: String,
    pub params: Vec<TypeParam>,
    pub vals: Vec<ClassVal>,
}

/// `type instance` の宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceDecl {
    pub tparams: Vec<TypeParam>,
    pub class: String,
    pub args: Vec<TypeExp>,
    pub body: Vec<Decl>,
}

/// `extern` の宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternDecl {
    pub is_pub: bool,
    pub abi: String,
    pub name: String,
    /// 実装名(非修飾の元の名前)。
    pub prim: String,
    pub tparams: Vec<TypeParam>,
    pub params: Vec<Pat>,
    pub ret: Option<TypeExp>,
    pub eff: Option<TypeExp>,
}

/// 宣言である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    pub meta: Meta,
    pub kind: DeclKind,
}

/// 宣言の形である。
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum DeclKind {
    Type(TypeAlias),
    Newtype(Newtype),
    Effect(Effect),
    Class(ClassDecl),
    Instance(InstanceDecl),
    Let(LetBinding),
    LetRec(Vec<LetBinding>),
    /// pub、名前、本体。
    Module(bool, String, Vec<Decl>),
    Extern(ExternDecl),
    Exp(Exp),
}

/// 1 つのファイルの構文解析の結果である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<Decl>,
}

// ---- 部分木の深さ ----
//
// どの関数も、子のノードが持つ深さだけを見て、子の深さの最大を返す(再帰しない)。

fn mx(a: u32, b: u32) -> u32 {
    a.max(b)
}

fn ty_opt(t: &Option<TypeExp>) -> u32 {
    t.as_ref().map_or(0, |t| t.meta.depth)
}

fn tyb_opt(t: &Option<Box<TypeExp>>) -> u32 {
    t.as_ref().map_or(0, |t| t.meta.depth)
}

impl TypeExpKind {
    /// 子の深さの最大。
    pub fn child_depth(&self) -> u32 {
        use TypeExpKind::*;
        match self {
            Ident(_) | Hole => 0,
            Apply(f, args) => args.iter().fold(f.meta.depth, |d, t| mx(d, t.meta.depth)),
            Arrow(ps, r, e) => ps
                .iter()
                .fold(mx(r.meta.depth, tyb_opt(e)), |d, t| mx(d, t.meta.depth)),
            BraceRow(elems, ext) => elems.iter().fold(tyb_opt(ext), |d, e| match e {
                BraceElem::Field(_, t) => mx(d, t.meta.depth),
                BraceElem::Label(_, args) => args.iter().fold(d, |d, t| mx(d, t.meta.depth)),
            }),
            VariantCase(_, t) => tyb_opt(t),
            Union(ts) => ts.iter().fold(0, |d, t| mx(d, t.meta.depth)),
        }
    }
}

impl PatKind {
    /// 子の深さの最大。
    pub fn child_depth(&self) -> u32 {
        use PatKind::*;
        match self {
            Wildcard | Var(_) | Bool(_) | Number(_) | Text(_) => 0,
            Record(fs, rest) => fs
                .iter()
                .fold(rest.as_ref().map_or(0, |p| p.meta.depth), |d, (_, p)| {
                    mx(d, p.meta.depth)
                }),
            Ctor(_, args) => args.iter().fold(0, |d, a| mx(d, a.pat.meta.depth)),
            Variant(_, p) => p.meta.depth,
            Annot(p, t) => mx(p.meta.depth, t.meta.depth),
        }
    }
}

impl ExpKind {
    /// 子の深さの最大。
    pub fn child_depth(&self) -> u32 {
        use ExpKind::*;
        let clauses = |cs: &[Clause]| cs.iter().fold(0, |d, c| mx(d, c.meta.depth));
        match self {
            Bool(_) | Number(_) | Text(_) | Ident(_) | Hole | RecordEmpty | Resume(None) => 0,
            Apply(f, a) => mx(f.meta.depth, a.meta.depth),
            Construct(_, args) => args.iter().fold(0, |d, a| mx(d, a.exp.meta.depth)),
            Variant(_, e)
            | Not(e)
            | Neg(e)
            | RecordRestriction(e, _)
            | RecordSelection(e, _)
            | Perform(_, e)
            | Run(_, e) => e.meta.depth,
            Resume(Some(e)) => e.meta.depth,
            BinOp(l, _, r) => mx(l.meta.depth, r.meta.depth),
            Lambda(l) => l
                .params
                .iter()
                .fold(l.body.meta.depth, |d, p| mx(d, p.meta.depth)),
            Let(b, e) => mx(b.meta.depth, e.meta.depth),
            LetRec(bs, e) => bs.iter().fold(e.meta.depth, |d, b| mx(d, b.meta.depth)),
            Seq(es) => es.iter().fold(0, |d, e| mx(d, e.meta.depth)),
            Match(e, cs) | Handle(e, cs) => mx(e.meta.depth, clauses(cs)),
            RecordExtend(r, _, v) | RecordUpdate(r, _, v) => mx(r.meta.depth, v.meta.depth),
        }
    }
}

impl Clause {
    /// 子(パターン、ガード、本体)の深さの最大。
    pub fn child_depth(&self) -> u32 {
        mx(
            mx(self.pat.meta.depth, self.body.meta.depth),
            self.guard.as_ref().map_or(0, |g| g.meta.depth),
        )
    }
}

impl LetBinding {
    /// 子の深さの最大。
    pub fn child_depth(&self) -> u32 {
        let ps = self
            .params
            .as_ref()
            .map_or(0, |ps| ps.iter().fold(0, |d, p| mx(d, p.meta.depth)));
        mx(
            mx(self.name.meta.depth, self.body.meta.depth),
            mx(ps, mx(ty_opt(&self.ret), ty_opt(&self.eff))),
        )
    }
}

impl DeclKind {
    /// 子の深さの最大。
    pub fn child_depth(&self) -> u32 {
        use DeclKind::*;
        let decls = |ds: &[Decl]| ds.iter().fold(0, |d, x| mx(d, x.meta.depth));
        match self {
            Type(t) => t.body.meta.depth,
            Newtype(n) => n
                .ctors
                .iter()
                .flat_map(|c| c.fields.iter())
                .fold(0, |d, f| mx(d, f.ty.meta.depth)),
            Effect(e) => e.ops.iter().fold(0, |d, (_, t)| mx(d, t.meta.depth)),
            Class(c) => c.vals.iter().fold(0, |d, v| mx(d, v.ty.meta.depth)),
            Instance(i) => i
                .args
                .iter()
                .fold(decls(&i.body), |d, t| mx(d, t.meta.depth)),
            Let(b) => b.meta.depth,
            LetRec(bs) => bs.iter().fold(0, |d, b| mx(d, b.meta.depth)),
            Module(_, _, ds) => decls(ds),
            Extern(e) => e
                .params
                .iter()
                .fold(mx(ty_opt(&e.ret), ty_opt(&e.eff)), |d, p| {
                    mx(d, p.meta.depth)
                }),
            Exp(e) => e.meta.depth,
        }
    }
}
