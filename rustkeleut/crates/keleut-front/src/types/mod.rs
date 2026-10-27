//! 型検査の核である(Diktor の syntax.ml の `Type`、decls.ml、unify.ml、show.ml、exhaust.ml、elab.ml)。
//! 型検査を済ませた状態から KIR を作る手順([`emit`]、Diktor の ir_write.ml)も置く。
//!
//! Diktor の型検査は大域の可変状態(表と参照)を使うが、本モジュールはそれを 1 つの文脈 [`Tc`] に集める(計画 7 節)。
//! 型は添字で引く表(arena)に置き、型変数は添字の Union-Find にする。
//! Diktor が木の節点の可変の欄に書く注釈(型、名前の解決、辞書)は、節点の番号で引く別表に置く。
//!
//! Diktor との一致は次の 3 つで保つ。
//!
//! - 割り付けの同一性:Diktor が物理的な等しさ(`==`)で比べる値は、同じ添字のときだけ等しい。
//!   型の構成子を作るたびに新しい添字を割り付け、Diktor が共有する定数(`t_int32` など)だけを共有する。
//! - 名前の番号の順:Diktor は名前と型変数に同じ通し番号(`new_oid`)を振り、
//!   エフェクトやヴァリアントのラベルを番号の順に並べる箇所がある。
//!   名前を番号にする(`intern`)呼び出しを Diktor と同じ時点で行い、名前どうしの番号の前後を一致させる。
//! - 列の順:Diktor の連結リストの「先頭に足して後で逆にする」操作は、`Vec` の末尾に足す操作に写す。
//!
//! 再帰はスタックの残りを調べ、足りなくなったら [`Fail::Limit`] で止まる。
//! Diktor の型検査がスタックあふれで止まるときの「処理系の限度」の診断に当たる。
//! 型検査は [`check::on_check_stack`] が作る専用のスレッドで走らせる。
//! そのスレッドのスタックは既定で [`check::CHECK_STACK_BYTES`](8 GiB)にし、作れない環境では深さの上限に見込む大きさで作り直す。
//! `--native-stack` を与えたときはそのスタックを使い、深さの上限をそこで検査を終えられる深さまで下げる。
//! 検査を通らない再帰(木を捨てることなど)もこのスタックに収める。

pub mod check;
mod decl;
mod decls;
mod elab;
mod elab_bind;
pub mod emit;
mod exhaust;
mod flatten;
pub mod fx;
pub mod load;
pub mod pmap;
pub mod session;
mod show;
mod unify;
mod units;

use crate::ast::NodeId;
use crate::pos::Span;
use fx::{HashMap, HashSet};
use pmap::PMap;
use std::rc::Rc;

pub use decls::{AliasInfo, ClassInfo, CtorInfo, DataInfo, EffectInfo, FieldInfo, InstanceInfo};

/// 名前と型変数の通し番号(Diktor の `oid`)である。
pub type Oid = u32;

/// 型の表の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ty(u32);

/// カインドの表の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kind(u32);

/// 型変数のセル(Diktor の `tvar ref`)の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell(u32);

/// 辞書の穴の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HoleId(u32);

/// 単位(ファイル)の番号である(Diktor の `Decls.unit_id`)。
pub type UnitId = i32;

/// 型の形である(Diktor の `Type.ty`)。
#[derive(Clone, Debug)]
pub enum TyNode {
    Con(Oid, Rc<[Ty]>),
    App(Ty, Ty),
    Arrow(Ty, Ty, Ty),
    Record(Ty),
    Variant(Ty),
    RowEmpty,
    RowExtend(Oid, Ty, Ty),
    Var(Cell),
}

/// 型変数の情報である(Diktor の `var_info`)。
#[derive(Clone, Debug)]
pub struct VarInfo {
    pub vid: Oid,
    pub vlevel: i32,
    pub vkind: Kind,
    /// 制約(新しいものが先頭)。
    pub vcls: Vec<Oid>,
}

/// 型変数の状態である(Diktor の `tvar`)。
#[derive(Clone, Debug)]
pub enum TVar {
    Unbound(VarInfo),
    Link(Ty),
    Generic(VarInfo),
    Rigid(VarInfo),
}

/// カインドの形である。
#[derive(Clone, Copy, Debug)]
enum KNode {
    Star,
    Row,
    Arrow(Kind, Kind),
    Var(Oid, Option<Kind>),
}

/// カインドを見た結果である([`Tc::kview`])。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KView {
    Star,
    Row,
    Arrow(Kind, Kind),
    Var(Kind),
}

/// 大域の参照(Diktor の `Tree.gref`)である。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GRef {
    Builtin(String),
    Method(String),
    Decl(NodeId, String),
}

/// 名前の解決の結果(Diktor の `Tree.resolved`)である。
#[derive(Clone, Debug)]
pub enum Resolved {
    Var(GRef),
    Op(Oid),
    NeverOp(Oid),
    ReturnClause,
    CancelClause,
    Ctor(Oid, Oid, Vec<usize>),
    CtorPat(Oid, Oid, Vec<Option<usize>>),
}

/// 辞書の鍵(型変数の番号とクラス)である。
pub type DKey = (Oid, Oid);

/// 証拠(Diktor の `Tree.evidence`)である。
#[derive(Clone, Debug)]
pub enum Evidence {
    Param(DKey),
    Inst(Oid, Oid, Vec<Evidence>),
    Record(Vec<(Oid, Evidence)>),
    Variant(Vec<(Oid, Evidence)>),
    Hole(HoleId),
}

/// 辞書の穴(Diktor の `Tree.hole`)である。
#[derive(Clone, Debug)]
pub struct Hole {
    pub h_id: Oid,
    pub h_cls: Oid,
    pub h_ty: Ty,
    pub h_loc: Option<Span>,
    pub h_sol: Option<Evidence>,
}

/// 節点の辞書の情報(Diktor の `Tree.dict_info`)である。
#[derive(Clone, Debug)]
pub enum DictInfo {
    Use(Vec<HoleId>),
    Abs(Vec<DKey>),
    Method(Vec<DKey>, Vec<DKey>, Vec<HoleId>),
    Instance(Vec<DKey>),
}

/// 型検査の失敗である(Diktor の例外)。
#[derive(Clone, Debug)]
pub enum Fail {
    /// `Type_error`。
    Type(String),
    /// `Type_error_at`。
    TypeAt(Span, String),
    /// `NotImplemented`。
    NotImpl(String),
    /// `NotImplemented_at`。
    NotImplAt(Span, String),
    /// `Import_error`。
    Import(Span, String),
    /// `Unify.Unsolved_param`(穴)。
    Unsolved(HoleId),
    /// スタックの不足(`Stack_overflow`)。
    Limit,
    /// 内部の誤り(`Panic`)。
    Bug(String),
}

/// 型検査の結果である。
pub type R<T> = Result<T, Fail>;

/// 位置の無い型エラーで失敗する(Diktor の `type_error`)。
pub(crate) fn terr<T>(msg: impl Into<String>) -> R<T> {
    Err(Fail::Type(msg.into()))
}

/// 内部の誤りで失敗する(Diktor の `bug`)。
pub(crate) fn bug<T>(msg: impl Into<String>) -> R<T> {
    Err(Fail::Bug(format!("[BUG] {}", msg.into())))
}

/// 未実装で失敗する(Diktor の `noimpl`)。
pub(crate) fn noimpl<T>(msg: impl Into<String>) -> R<T> {
    Err(Fail::NotImpl(msg.into()))
}

/// 位置の無い失敗に位置を付ける(Diktor の `at_node`)。
pub(crate) fn at<T>(span: Span, r: R<T>) -> R<T> {
    match r {
        Err(Fail::Type(m)) => Err(Fail::TypeAt(span, m)),
        Err(Fail::NotImpl(m)) => Err(Fail::NotImplAt(span, m)),
        r => r,
    }
}

thread_local! {
    /// いまのスレッドで使ってよいスタックの下端である。
    /// 決めていない間は `usize::MAX` で、どの [`guard`] も失敗する。
    static STACK_FLOOR: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// 大きさ `stack_bytes` のスタックで、[`guard`] が残しておく余白のバイト数である。
///
/// 余白は、検査を通らない再帰(木を捨てることなど)と、限度で止まった後の後始末に使う。
pub(crate) fn guard_margin(stack_bytes: usize) -> usize {
    (stack_bytes / 8).clamp(256 << 10, 4 << 20)
}

/// いまのスレッドのスタックの下端を決める。
///
/// `stack_bytes` はスレッドのスタックの大きさである。
/// 下端から [`guard_margin`] バイト手前までを使ってよい範囲にする。
/// スレッドの入口で呼ぶ([`check::on_check_stack`] が呼ぶ)。
pub(crate) fn set_stack_floor(stack_bytes: usize) {
    let here = 0u8;
    let sp = std::ptr::addr_of!(here) as usize;
    let margin = guard_margin(stack_bytes);
    STACK_FLOOR.with(|f| f.set(sp.saturating_sub(stack_bytes.saturating_sub(margin))));
}

/// スタックの残りが足りなければ [`Fail::Limit`] で失敗する。
///
/// 下端を決めていないスレッドでは、つねに失敗する。
/// 型検査を [`check::on_check_stack`] の外で走らせても、プロセスがスタックあふれで落ちないようにするためである。
#[inline]
pub(crate) fn guard() -> R<()> {
    let here = 0u8;
    let sp = std::ptr::addr_of!(here) as usize;
    if STACK_FLOOR.with(|f| sp < f.get()) {
        Err(Fail::Limit)
    } else {
        Ok(())
    }
}

/// 宣言列を処理するときの出力の行き先(Diktor の `process_decls` の `emit`)である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Emit {
    /// 何も出さない(プレリュード)。
    None,
    /// 型の行と警告を出す(起点の単位)。
    All,
    /// 警告だけを、単位のパスを前置して出す(起点でない単位)。
    Warnings(UnitId),
}

/// `type_check` の出力の行(Diktor の `Elab.out_line`)である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutLine {
    Binding(String),
    Warning(String),
}

/// 型検査の文脈である(Diktor の大域の状態のすべて)。
///
/// 対話的な実行は、入力の前の写しを取り、受理しなかった入力の後で写しに戻す。
#[derive(Clone)]
pub struct Tc {
    // ---- 通し番号と名前 ----
    next_oid: Oid,
    intern_map: HashMap<Rc<str>, Oid>,
    name_map: HashMap<Oid, Rc<str>>,

    // ---- 型、セル、カインド ----
    tys: Vec<TyNode>,
    cells: Vec<TVar>,
    kinds: Vec<KNode>,

    // ---- 定数 ----
    pub(crate) l_item: Oid,
    pub(crate) o_boolean: Oid,
    pub(crate) o_int32: Oid,
    pub(crate) o_int64: Oid,
    pub(crate) o_float64: Oid,
    pub(crate) o_string: Oid,
    pub(crate) o_never: Oid,
    pub(crate) cls_integral: Oid,
    pub(crate) cls_fractional: Oid,
    pub(crate) eff_heap: Oid,
    pub(crate) ty_ref: Oid,
    pub(crate) ty_mutable_array: Oid,
    pub(crate) eff_blocking: Oid,
    pub(crate) t_boolean: Ty,
    pub(crate) t_int32: Ty,
    pub(crate) t_int64: Ty,
    pub(crate) t_float64: Ty,
    pub(crate) t_string: Ty,
    pub(crate) t_never: Ty,
    pub(crate) t_unit: Ty,
    pub(crate) row_empty: Ty,
    pub(crate) k_star: Kind,
    pub(crate) k_row: Kind,

    // ---- 木の注釈 ----
    pub(crate) node_ty: fx::NodeMap<Ty>,
    pub(crate) resolved: fx::NodeMap<Resolved>,
    pub(crate) dict: fx::NodeMap<DictInfo>,
    pub(crate) holes_arena: Vec<Hole>,

    // ---- 宣言表(decls.ml) ----
    pub(crate) d: decls::Decls,

    // ---- 単一化(unify.ml) ----
    pub(crate) rigid_tparams: HashMap<Oid, String>,
    pub(crate) class_vars: Vec<(i64, Cell)>,
    class_seq: i64,
    pub(crate) literal_origins: Vec<(Cell, Oid, Span)>,
    pub(crate) holes: Vec<(i64, HoleId)>,
    pub(crate) decl_hole_floor: Oid,
    pub(crate) premise_origins: HashMap<DKey, (Ty, Oid)>,
    hole_seq: i64,
    pub(crate) last_rigid_clash: Option<(Ty, Ty)>,

    // ---- 精緻化(elab.ml) ----
    pub(crate) top_types: HashMap<GRef, Ty>,
    pub(crate) warnings: Vec<String>,
    pub(crate) current_out: Vec<OutLine>,
    pub(crate) pattern_numbers: Vec<(NodeId, Span, crate::ast::Number)>,
    pub(crate) pub_pure_cells: HashSet<Cell>,
    pub(crate) opened_rows: Vec<Cell>,
    pub(crate) speculating: bool,
    pub(crate) group_stack: Vec<elab::GroupRec>,

    // ---- 網羅性(exhaust.ml) ----
    pub(crate) pending: Vec<exhaust::Entry>,
}

/// 精緻化の環境(Diktor の `Elab.env`)である。
#[derive(Clone)]
pub struct Env {
    pub(crate) values: PMap<Rc<str>, Ty>,
    pub(crate) types: PMap<Rc<str>, Ty>,
    pub(crate) resume_ty: Option<(Ty, Ty)>,
    pub(crate) no_resume: Option<elab::NoResume>,
    pub(crate) tops: PMap<Rc<str>, GRef>,
}

impl Default for Tc {
    fn default() -> Self {
        Tc::new()
    }
}

impl Tc {
    /// Diktor のモジュールの初期化(syntax.ml の `Type` の定数と decls.ml の `register_builtins`)を済ませた文脈。
    pub fn new() -> Tc {
        let mut tc = Tc {
            next_oid: 0,
            intern_map: HashMap::default(),
            name_map: HashMap::default(),
            tys: Vec::new(),
            cells: Vec::new(),
            kinds: Vec::new(),
            l_item: 0,
            o_boolean: 0,
            o_int32: 0,
            o_int64: 0,
            o_float64: 0,
            o_string: 0,
            o_never: 0,
            cls_integral: 0,
            cls_fractional: 0,
            eff_heap: 0,
            ty_ref: 0,
            ty_mutable_array: 0,
            eff_blocking: 0,
            t_boolean: Ty(0),
            t_int32: Ty(0),
            t_int64: Ty(0),
            t_float64: Ty(0),
            t_string: Ty(0),
            t_never: Ty(0),
            t_unit: Ty(0),
            row_empty: Ty(0),
            k_star: Kind(0),
            k_row: Kind(0),
            node_ty: fx::NodeMap::default(),
            resolved: fx::NodeMap::default(),
            dict: fx::NodeMap::default(),
            holes_arena: Vec::new(),
            d: decls::Decls::default(),
            rigid_tparams: HashMap::default(),
            class_vars: Vec::new(),
            class_seq: 0,
            literal_origins: Vec::new(),
            holes: Vec::new(),
            decl_hole_floor: 0,
            premise_origins: HashMap::default(),
            hole_seq: 0,
            last_rigid_clash: None,
            top_types: HashMap::default(),
            warnings: Vec::new(),
            current_out: Vec::new(),
            pattern_numbers: Vec::new(),
            pub_pure_cells: HashSet::default(),
            opened_rows: Vec::new(),
            speculating: false,
            group_stack: Vec::new(),
            pending: Vec::new(),
        };
        tc.row_empty = tc.mk(TyNode::RowEmpty);
        tc.k_star = tc.mk_kind(KNode::Star);
        tc.k_row = tc.mk_kind(KNode::Row);
        // syntax.ml の Type の定数(定義の順に intern する)。
        tc.l_item = tc.intern("_item");
        tc.o_boolean = tc.intern("Boolean");
        tc.t_boolean = tc.con0(tc.o_boolean);
        tc.o_int32 = tc.intern("Int32");
        tc.t_int32 = tc.con0(tc.o_int32);
        tc.o_int64 = tc.intern("Int64");
        tc.t_int64 = tc.con0(tc.o_int64);
        tc.o_float64 = tc.intern("Float64");
        tc.t_float64 = tc.con0(tc.o_float64);
        tc.o_string = tc.intern("String");
        tc.t_string = tc.con0(tc.o_string);
        tc.o_never = tc.intern("Never");
        tc.t_never = tc.con0(tc.o_never);
        let re = tc.row_empty;
        tc.t_unit = tc.mk(TyNode::Record(re));
        tc.cls_integral = tc.intern("Integral");
        tc.cls_fractional = tc.intern("Fractional");
        tc.eff_heap = tc.intern("Heap");
        tc.ty_ref = tc.intern("Ref");
        tc.ty_mutable_array = tc.intern("MutableArray");
        tc.eff_blocking = tc.intern("Blocking");
        // decls.ml の初期化。
        tc.intern("Float64");
        tc.register_builtins();
        // exhaust.ml の初期化。
        tc.intern("Boolean");
        tc
    }

    // ---- 通し番号と名前 ----

    /// 新しい通し番号(Diktor の `new_oid`)。
    pub(crate) fn new_oid(&mut self) -> Oid {
        let r = self.next_oid;
        self.next_oid += 1;
        r
    }

    /// 名前を番号にする(Diktor の `Type.intern`)。
    pub fn intern(&mut self, name: &str) -> Oid {
        if let Some(&o) = self.intern_map.get(name) {
            return o;
        }
        let o = self.new_oid();
        let rc: Rc<str> = Rc::from(name);
        self.intern_map.insert(rc.clone(), o);
        self.name_map.insert(o, rc);
        o
    }

    /// 番号の名前(Diktor の `Type.name_of`)。
    pub fn name_of(&self, o: Oid) -> Rc<str> {
        self.name_map
            .get(&o)
            .cloned()
            .unwrap_or_else(|| Rc::from(format!("?{o}")))
    }

    /// 番号の表示名(Diktor の `Type.display_of`)。
    pub fn display_of(&self, o: Oid) -> String {
        display(&self.name_of(o)).to_string()
    }

    // ---- 型の表 ----

    pub(crate) fn mk(&mut self, n: TyNode) -> Ty {
        self.tys.push(n);
        Ty((self.tys.len() - 1) as u32)
    }

    /// 型の形(写し)。
    pub(crate) fn node(&self, t: Ty) -> TyNode {
        self.tys[t.0 as usize].clone()
    }

    /// 型の形(借用)。
    pub(crate) fn node_ref(&self, t: Ty) -> &TyNode {
        &self.tys[t.0 as usize]
    }

    pub(crate) fn con0(&mut self, c: Oid) -> Ty {
        self.mk(TyNode::Con(c, Rc::from(Vec::new())))
    }

    pub(crate) fn con(&mut self, c: Oid, args: Vec<Ty>) -> Ty {
        self.mk(TyNode::Con(c, Rc::from(args)))
    }

    pub(crate) fn arrow(&mut self, p: Ty, r: Ty, e: Ty) -> Ty {
        self.mk(TyNode::Arrow(p, r, e))
    }

    pub(crate) fn record(&mut self, r: Ty) -> Ty {
        self.mk(TyNode::Record(r))
    }

    pub(crate) fn variant(&mut self, r: Ty) -> Ty {
        self.mk(TyNode::Variant(r))
    }

    pub(crate) fn extend(&mut self, l: Oid, f: Ty, rest: Ty) -> Ty {
        self.mk(TyNode::RowExtend(l, f, rest))
    }

    pub(crate) fn tvar(&mut self, c: Cell) -> Ty {
        self.mk(TyNode::Var(c))
    }

    /// セルの状態。
    pub(crate) fn cell(&self, c: Cell) -> &TVar {
        &self.cells[c.0 as usize]
    }

    /// セルを書き換える(Diktor の `set_tv`)。
    pub(crate) fn set_tv(&mut self, c: Cell, v: TVar) {
        self.cells[c.0 as usize] = v;
    }

    pub(crate) fn new_cell(&mut self, v: TVar) -> Cell {
        self.cells.push(v);
        Cell((self.cells.len() - 1) as u32)
    }

    /// 型変数のセルなら、その添字。
    pub(crate) fn var_cell(&self, t: Ty) -> Option<Cell> {
        match self.node_ref(t) {
            TyNode::Var(c) => Some(*c),
            _ => None,
        }
    }

    /// 型を代表に辿る(Diktor の `Type.repr`)。リンクは辿った先に縮める。
    pub(crate) fn repr(&mut self, t: Ty) -> Ty {
        let mut chain: Vec<(Cell, Ty)> = Vec::new();
        let mut cur = t;
        while let TyNode::Var(c) = self.node_ref(cur)
            && let TVar::Link(next) = self.cell(*c)
        {
            let (c, next) = (*c, *next);
            chain.push((c, next));
            cur = next;
        }
        let fin = self.repr_app(cur);
        for (c, link) in chain {
            if link != fin {
                self.set_tv(c, TVar::Link(fin));
            }
        }
        fin
    }

    fn repr_app(&mut self, t: Ty) -> Ty {
        match self.node(t) {
            TyNode::App(f, a) => {
                let f2 = self.repr(f);
                match self.node(f2) {
                    TyNode::Con(c, args) => {
                        let mut v = args.to_vec();
                        v.push(a);
                        self.con(c, v)
                    }
                    _ => {
                        if f2 == f {
                            t
                        } else {
                            self.mk(TyNode::App(f2, a))
                        }
                    }
                }
            }
            _ => t,
        }
    }

    /// 型の適用(Diktor の `Type.tapp`)。
    pub(crate) fn tapp(&mut self, f: Ty, a: Ty) -> Ty {
        match self.node(f) {
            TyNode::Con(c, args) => {
                let mut v = args.to_vec();
                v.push(a);
                self.con(c, v)
            }
            _ => self.mk(TyNode::App(f, a)),
        }
    }

    /// 適用の背骨(Diktor の `Type.app_spine`)。
    pub(crate) fn app_spine(&mut self, t: Ty) -> (Ty, Vec<Ty>) {
        let t = self.repr(t);
        match self.node(t) {
            TyNode::App(f, a) => {
                let (h, mut args) = self.app_spine(f);
                args.push(a);
                (h, args)
            }
            _ => (t, Vec::new()),
        }
    }

    /// 行の欄と末尾(Diktor の `Type.row_fields`)。
    pub(crate) fn row_fields(&mut self, t: Ty) -> (Vec<(Oid, Ty)>, Ty) {
        let mut fs = Vec::new();
        let mut cur = self.repr(t);
        loop {
            match self.node(cur) {
                TyNode::RowExtend(l, f, rest) => {
                    fs.push((l, f));
                    cur = self.repr(rest);
                }
                _ => return (fs, cur),
            }
        }
    }

    /// 行の末尾の型変数(Diktor の `Type.row_tail_var`)。
    pub(crate) fn row_tail_var(&mut self, t: Ty) -> Option<Cell> {
        let (_, tail) = self.row_fields(t);
        self.var_cell(tail)
    }

    /// 2 つの行をつなぐ(Diktor の `Type.row_append`)。
    pub(crate) fn row_append(&mut self, a: Ty, b: Ty) -> R<Ty> {
        let mut fs = Vec::new();
        let mut cur = self.repr(a);
        let tail = loop {
            match self.node(cur) {
                TyNode::RowExtend(l, f, rest) => {
                    fs.push((l, f));
                    cur = self.repr(rest);
                }
                TyNode::RowEmpty => break b,
                _ => {
                    let rb = self.repr(b);
                    if self.is_row_empty(rb) {
                        break cur;
                    }
                    return bug("row_append: open row on the left");
                }
            }
        };
        let mut acc = tail;
        for (l, f) in fs.into_iter().rev() {
            acc = self.extend(l, f, acc);
        }
        Ok(acc)
    }

    /// 型の形が空の行か(構造の比較 `= TRowEmpty`)。
    pub(crate) fn is_row_empty(&self, t: Ty) -> bool {
        matches!(self.node_ref(t), TyNode::RowEmpty)
    }

    /// `repr t` が空の行か。
    pub(crate) fn repr_is_empty(&mut self, t: Ty) -> bool {
        let t = self.repr(t);
        self.is_row_empty(t)
    }

    /// 新しい型変数(Diktor の `Type.new_var`)。
    pub(crate) fn new_var_k(&mut self, kind: Kind, classes: Vec<Oid>, level: i32) -> Ty {
        let vid = self.new_oid();
        let c = self.new_cell(TVar::Unbound(VarInfo {
            vid,
            vlevel: level,
            vkind: kind,
            vcls: classes,
        }));
        self.tvar(c)
    }

    pub(crate) fn new_var(&mut self, level: i32) -> Ty {
        let k = self.k_star;
        self.new_var_k(k, Vec::new(), level)
    }

    pub(crate) fn new_row_var(&mut self, level: i32) -> Ty {
        let k = self.k_row;
        self.new_var_k(k, Vec::new(), level)
    }

    /// 新しい剛定数のセル(Diktor の `Type.new_rigid_ref`)。
    pub(crate) fn new_rigid_ref(&mut self, kind: Kind, classes: Vec<Oid>, level: i32) -> Cell {
        let vid = self.new_oid();
        self.new_cell(TVar::Rigid(VarInfo {
            vid,
            vlevel: level,
            vkind: kind,
            vcls: classes,
        }))
    }

    pub(crate) fn new_rigid(&mut self, kind: Kind, classes: Vec<Oid>, level: i32) -> Ty {
        let c = self.new_rigid_ref(kind, classes, level);
        self.tvar(c)
    }

    /// 一般化された型変数(Diktor の `Decls.generic`)。
    pub(crate) fn generic(&mut self, kind: Kind, classes: Vec<Oid>) -> Ty {
        let vid = self.new_oid();
        let c = self.new_cell(TVar::Generic(VarInfo {
            vid,
            vlevel: 0,
            vkind: kind,
            vcls: classes,
        }));
        self.tvar(c)
    }

    /// 引数の閉じた行(Diktor の `closed_args_row`、`closed_item_row`)。
    pub(crate) fn closed_item_row(&mut self, tys: &[Ty]) -> Ty {
        let mut acc = self.row_empty;
        for &t in tys.iter().rev() {
            acc = self.extend(self.l_item, t, acc);
        }
        acc
    }

    // ---- カインド ----

    fn mk_kind(&mut self, k: KNode) -> Kind {
        self.kinds.push(k);
        Kind((self.kinds.len() - 1) as u32)
    }

    /// 新しいカインド変数(Diktor の `new_kind_var`)。
    pub(crate) fn new_kind_var(&mut self) -> Kind {
        let id = self.new_oid();
        self.mk_kind(KNode::Var(id, None))
    }

    pub(crate) fn k_arrow2(&mut self, a: Kind, b: Kind) -> Kind {
        self.mk_kind(KNode::Arrow(a, b))
    }

    /// `[_, ...] Type`(Diktor の `k_arrow`)。
    pub(crate) fn k_arrow(&mut self, arity: u32) -> Kind {
        if arity == 0 {
            self.k_star
        } else {
            let r = self.k_arrow(arity - 1);
            let s = self.k_star;
            self.k_arrow2(s, r)
        }
    }

    /// カインドを代表に辿る(Diktor の `kind_repr`)。
    pub(crate) fn kind_repr(&mut self, k: Kind) -> Kind {
        match self.kinds[k.0 as usize] {
            KNode::Var(id, Some(next)) => {
                let k2 = self.kind_repr(next);
                if k2 != next {
                    self.kinds[k.0 as usize] = KNode::Var(id, Some(k2));
                }
                k2
            }
            _ => k,
        }
    }

    /// 代表に辿ったカインドの形。
    pub(crate) fn kview(&mut self, k: Kind) -> KView {
        let k = self.kind_repr(k);
        match self.kinds[k.0 as usize] {
            KNode::Star => KView::Star,
            KNode::Row => KView::Row,
            KNode::Arrow(a, b) => KView::Arrow(a, b),
            KNode::Var(..) => KView::Var(k),
        }
    }

    fn set_klink(&mut self, k: Kind, to: Kind) {
        if let KNode::Var(id, _) = self.kinds[k.0 as usize] {
            self.kinds[k.0 as usize] = KNode::Var(id, Some(to));
        }
    }

    /// カインドを同じにする(Diktor の `same_kind`)。
    ///
    /// Diktor と同じく出現の検査をしないので、カインドは循環しうる。
    /// 循環したカインドどうしの比較は再帰が尽きず、スタックの不足([`Fail::Limit`])で止まる。
    pub(crate) fn same_kind(&mut self, a: Kind, b: Kind) -> R<bool> {
        guard()?;
        let ra = self.kind_repr(a);
        let rb = self.kind_repr(b);
        Ok(
            match (self.kinds[ra.0 as usize], self.kinds[rb.0 as usize]) {
                (KNode::Star, KNode::Star) | (KNode::Row, KNode::Row) => true,
                (KNode::Arrow(a1, a2), KNode::Arrow(b1, b2)) => {
                    self.same_kind(a1, b1)? && self.same_kind(a2, b2)?
                }
                (KNode::Var(..), KNode::Var(..)) if ra == rb => true,
                (KNode::Var(..), _) => {
                    self.set_klink(ra, rb);
                    true
                }
                (_, KNode::Var(..)) => {
                    self.set_klink(rb, ra);
                    true
                }
                _ => false,
            },
        )
    }

    /// 残ったカインド変数を `Type` にする(Diktor の `default_kind`)。
    pub(crate) fn default_kind(&mut self, k: Kind) -> R<()> {
        guard()?;
        match self.kview(k) {
            KView::Var(r) => {
                let s = self.k_star;
                self.set_klink(r, s)
            }
            KView::Arrow(a, b) => {
                self.default_kind(a)?;
                self.default_kind(b)?;
            }
            KView::Star | KView::Row => {}
        }
        Ok(())
    }

    /// 矢印のカインドの引数の個数(Diktor の `show_kind` の `args` と、elab.ml の `arity`)。
    pub(crate) fn kind_arity(&mut self, k: Kind) -> R<usize> {
        guard()?;
        match self.kview(k) {
            KView::Arrow(_, r) => Ok(1 + self.kind_arity(r)?),
            _ => Ok(0),
        }
    }

    /// カインドの表示(Diktor の `show_kind`)。
    pub(crate) fn show_kind(&mut self, k: Kind) -> R<String> {
        Ok(match self.kview(k) {
            KView::Star => "Type".to_string(),
            KView::Row => "Row".to_string(),
            KView::Arrow(..) => {
                let n = self.kind_arity(k)?;
                format!("[{}] Type", vec!["_"; n].join(", "))
            }
            KView::Var(r) => match self.kinds[r.0 as usize] {
                KNode::Var(id, _) => format!("?k{id}"),
                _ => unreachable!(),
            },
        })
    }

    // ---- 木の注釈 ----

    pub(crate) fn set_ty(&mut self, id: NodeId, t: Ty) {
        self.node_ty.insert(id, t);
    }

    pub(crate) fn get_ty(&self, id: NodeId) -> R<Ty> {
        match self.node_ty.get(&id) {
            Some(t) => Ok(*t),
            None => bug("get_ty: node not elaborated"),
        }
    }

    pub(crate) fn set_resolved(&mut self, id: NodeId, r: Resolved) {
        self.resolved.insert(id, r);
    }

    pub(crate) fn set_dict(&mut self, id: NodeId, d: DictInfo) {
        self.dict.insert(id, d);
    }

    pub(crate) fn hole(&self, h: HoleId) -> &Hole {
        &self.holes_arena[h.0 as usize]
    }

    pub(crate) fn add_hole(&mut self, h: Hole) -> HoleId {
        self.holes_arena.push(h);
        HoleId((self.holes_arena.len() - 1) as u32)
    }
}

/// 名前の表示形(Diktor の `Type.display`)。
///
/// 単位の印の付いた綴り `%N.name` は、印を除いた `name` で表示する。
pub fn display(name: &str) -> &str {
    if name.len() > 1 && name.starts_with('%') {
        match name.find('.') {
            Some(i) => &name[i + 1..],
            None => name,
        }
    } else {
        name
    }
}
