//! 型検査の中核である(Diktor の syntax.ml の `Type`、decls.ml、unify.ml、show.ml、exhaust.ml、elab.ml に対応する)。
//! 型検査を済ませた状態から KIR を作る処理 [`emit`] も、このモジュールに置く。
//! これは Diktor の ir_write.ml に対応する。
//!
//! Diktor の型検査はグローバルな可変の状態(表と参照)を使うが、本モジュールはそれを 1 つのコンテキスト [`Tc`] にまとめる。
//! 型は添字で引く表(arena)に置き、型変数は添字で表した Union-Find にする。
//! Diktor が木のノードの可変のフィールドに書く注釈(型、名前解決の結果、辞書)は、ノードの番号で引く別の表に置く。
//!
//! Diktor と同じ結果になるように、次の 3 つを守る。
//!
//! - 値の同一性:Diktor が物理的な等しさ(`==`)で比べる値は、同じ添字のときだけ等しいものとして扱う。
//!   そのために、型のノードを作るたびに新しい添字を割り当て、Diktor が共有する定数(`t_int32` など)だけを共有する。
//! - 名前の番号の順序:Diktor は名前と型変数に同じ通し番号(`new_oid`)を振り、エフェクトやヴァリアントのラベルを番号の順に並べる箇所がある。
//!   そのために、名前を番号にする呼び出し(`intern`)を Diktor と同じ時点で行い、名前どうしの番号の大小を一致させる。
//! - リストの順序:Diktor が連結リストの先頭に追加して後で逆順にする操作は、`Vec` の末尾に追加する操作に置き換える。
//!
//! 再帰する関数はスタックの残りを調べ、足りなくなったら [`Fail::Limit`] を返して型検査をやめる。
//! これは、Diktor の型検査がスタックオーバーフローになったときに出す `処理系の限度` の診断に相当する。
//! 型検査は、[`check::on_check_stack`] が作る専用のスレッドで実行する。
//! そのスレッドのスタックサイズは、既定で [`check::CHECK_STACK_BYTES`](8 GiB)である。
//! このサイズのスタックを作れない環境では、深さの上限に必要なサイズで作り直す。
//! `--native-stack` を指定したときはそのサイズのスタックを使い、深さの上限を、そのスタックで型検査を終えられる深さまで下げる。
//! スタックの残りを調べない再帰(木の解放など)も、このスタックの中で実行する。

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

/// 名前と型変数に振る通し番号である(Diktor の `oid`)。
pub type Oid = u32;

/// 型を表す、型の表の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ty(u32);

/// カインドを表す、カインドの表の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Kind(u32);

/// 型変数のセル(Diktor の `tvar ref`)を表す、セルの表の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell(u32);

/// 辞書の穴を表す、穴の表の添字である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HoleId(u32);

/// 単位(ファイル)の番号である(Diktor の `Decls.unit_id`)。
pub type UnitId = i32;

/// 型のノードである(Diktor の `Type.ty`)。型の最も外側の構造を表し、子の型は添字で持つ。
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
    /// 型クラスの制約の並び。新しいものが先頭にある。
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

/// カインドのノードである。
#[derive(Clone, Copy, Debug)]
enum KNode {
    Star,
    Row,
    Arrow(Kind, Kind),
    Var(Oid, Option<Kind>),
}

/// 代表までたどったカインドの構造である([`Tc::kview`] が返す)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KView {
    Star,
    Row,
    Arrow(Kind, Kind),
    Var(Kind),
}

/// グローバルへの参照である(Diktor の `Tree.gref`)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GRef {
    Builtin(String),
    Method(String),
    Decl(NodeId, String),
}

/// 名前解決の結果である(Diktor の `Tree.resolved`)。
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

/// 辞書のキーである。型変数の番号とクラスの番号の組で表す。
pub type DKey = (Oid, Oid);

/// 証拠(evidence。どのインスタンスの辞書を使うかを表す項)である。Diktor の `Tree.evidence` に対応する。
#[derive(Clone, Debug)]
pub enum Evidence {
    Param(DKey),
    Inst(Oid, Oid, Vec<Evidence>),
    Record(Vec<(Oid, Evidence)>),
    Variant(Vec<(Oid, Evidence)>),
    Hole(HoleId),
}

/// 辞書の穴である(Diktor の `Tree.hole`)。
#[derive(Clone, Debug)]
pub struct Hole {
    pub h_id: Oid,
    pub h_cls: Oid,
    pub h_ty: Ty,
    pub h_loc: Option<Span>,
    pub h_sol: Option<Evidence>,
}

/// 木のノードに付ける辞書の情報である(Diktor の `Tree.dict_info`)。
#[derive(Clone, Debug)]
pub enum DictInfo {
    Use(Vec<HoleId>),
    Abs(Vec<DKey>),
    Method(Vec<DKey>, Vec<DKey>, Vec<HoleId>),
    Instance(Vec<DKey>),
}

/// 型検査の失敗を表す。各バリアントは Diktor の例外に対応する。
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
    /// `Unify.Unsolved_param`。値は辞書の穴である。
    Unsolved(HoleId),
    /// スタック不足(`Stack_overflow`)。
    Limit,
    /// 内部エラー(`Panic`)。
    Bug(String),
}

/// 型検査の関数が返す結果の型である。失敗は [`Fail`] で表す。
pub type R<T> = Result<T, Fail>;

/// 位置のない型エラーを表す `Err` を返す(Diktor の `type_error`)。
pub(crate) fn terr<T>(msg: impl Into<String>) -> R<T> {
    Err(Fail::Type(msg.into()))
}

/// 内部エラーを表す `Err` を返す。メッセージの先頭に `[BUG] ` を付ける(Diktor の `bug`)。
pub(crate) fn bug<T>(msg: impl Into<String>) -> R<T> {
    Err(Fail::Bug(format!("[BUG] {}", msg.into())))
}

/// 未実装の機能を使ったことを表す `Err` を返す(Diktor の `noimpl`)。
pub(crate) fn noimpl<T>(msg: impl Into<String>) -> R<T> {
    Err(Fail::NotImpl(msg.into()))
}

/// 位置のない失敗に位置を付ける(Diktor の `at_node`)。
pub(crate) fn at<T>(span: Span, r: R<T>) -> R<T> {
    match r {
        Err(Fail::Type(m)) => Err(Fail::TypeAt(span, m)),
        Err(Fail::NotImpl(m)) => Err(Fail::NotImplAt(span, m)),
        r => r,
    }
}

thread_local! {
    /// 現在のスレッドで使ってよいスタックの下端のアドレスである。
    /// 決めていない間は `usize::MAX` で、どの [`guard`] も失敗する。
    static STACK_FLOOR: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// サイズが `stack_bytes` のスタックで、[`guard`] が使わずに残す余裕のバイト数を返す。
///
/// 余裕は、スタックの残りを調べない再帰(木の解放など)と、スタック不足で型検査をやめた後の後始末に使う。
pub(crate) fn guard_margin(stack_bytes: usize) -> usize {
    (stack_bytes / 8).clamp(256 << 10, 4 << 20)
}

/// 現在のスレッドで使ってよいスタックの下端を決める。
///
/// `stack_bytes` はスレッドのスタックサイズである。
/// スタックの実際の下端より [`guard_margin`] バイト手前までを、使ってよい範囲にする。
/// スレッドの先頭で呼ぶ。[`check::on_check_stack`] が呼んでいる。
pub(crate) fn set_stack_floor(stack_bytes: usize) {
    let here = 0u8;
    let sp = std::ptr::addr_of!(here) as usize;
    let margin = guard_margin(stack_bytes);
    STACK_FLOOR.with(|f| f.set(sp.saturating_sub(stack_bytes.saturating_sub(margin))));
}

/// スタックの残りが足りなければ [`Fail::Limit`] で失敗する。
///
/// 下端を決めていないスレッドでは、呼ぶたびに失敗する。
/// 型検査を [`check::on_check_stack`] の外で実行しても、プロセスがスタックオーバーフローで異常終了しないようにするためである。
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

/// 宣言の並びを処理するときに、何を出力するかを表す(Diktor の `process_decls` の `emit`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Emit {
    /// 何も出力しない(プレリュード)。
    None,
    /// 型を表示する行と警告を出力する(起点の単位)。
    All,
    /// 警告だけを、先頭に単位のパスを付けて出力する(起点でない単位)。
    Warnings(UnitId),
}

/// `type_check` が出力する 1 行である(Diktor の `Elab.out_line`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutLine {
    Binding(String),
    Warning(String),
}

/// 型検査のコンテキストである。Diktor がグローバルに持つ状態を、すべてここに持つ。
///
/// 対話的な実行(REPL)は、入力の前にマークを取り、受理しなかった入力の後でコンテキストをマークの時点に戻す(`Tc::snapshot`)。
#[derive(Clone)]
pub struct Tc {
    // ---- 通し番号と名前 ----
    next_oid: Oid,
    intern_map: fx::JMap<Rc<str>, Oid>,
    name_map: fx::JMap<Oid, Rc<str>>,

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
    /// セルの書き換えの記録である。続く `hole_trail` は穴の、`kind_trail` はカインドの書き換えの記録である。
    /// 対話的な実行で、入力を取り消すときに使う([`Tc::snapshot`])。
    cell_trail: fx::Trail<TVar>,
    hole_trail: fx::Trail<Hole>,
    kind_trail: fx::Trail<KNode>,

    // ---- 宣言表(decls.ml) ----
    pub(crate) d: decls::Decls,

    // ---- 単一化(unify.ml) ----
    pub(crate) rigid_tparams: fx::JMap<Oid, String>,
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

/// 精緻化の環境である(Diktor の `Elab.env`)。
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
    /// Diktor の各モジュールが初期化で行う処理を済ませたコンテキストを作る。
    ///
    /// syntax.ml の `Type` の定数を作り、decls.ml の `register_builtins` を呼び、decls.ml と exhaust.ml が初期化で行う `intern` を同じ順に行う。
    pub fn new() -> Tc {
        let mut tc = Tc {
            next_oid: 0,
            intern_map: fx::JMap::default(),
            name_map: fx::JMap::default(),
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
            cell_trail: fx::Trail::default(),
            hole_trail: fx::Trail::default(),
            kind_trail: fx::Trail::default(),
            d: decls::Decls::default(),
            rigid_tparams: fx::JMap::default(),
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
        // syntax.ml の Type の定数。Diktor が定義している順に intern する。
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

    /// 新しい通し番号を返す(Diktor の `new_oid`)。
    pub(crate) fn new_oid(&mut self) -> Oid {
        let r = self.next_oid;
        self.next_oid += 1;
        r
    }

    /// 名前を番号にする(Diktor の `Type.intern`)。同じ名前には同じ番号を返す。
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

    /// 番号 `o` の名前を返す(Diktor の `Type.name_of`)。名前のない番号には `?` と番号をつないだ文字列を返す。
    pub fn name_of(&self, o: Oid) -> Rc<str> {
        self.name_map
            .get(&o)
            .cloned()
            .unwrap_or_else(|| Rc::from(format!("?{o}")))
    }

    /// 番号 `o` の表示名を返す(Diktor の `Type.display_of`)。
    pub fn display_of(&self, o: Oid) -> String {
        display(&self.name_of(o)).to_string()
    }

    // ---- 型の表 ----

    pub(crate) fn mk(&mut self, n: TyNode) -> Ty {
        self.tys.push(n);
        Ty((self.tys.len() - 1) as u32)
    }

    /// 型 `t` のノードをコピーして返す。
    pub(crate) fn node(&self, t: Ty) -> TyNode {
        self.tys[t.0 as usize].clone()
    }

    /// 型 `t` のノードへの参照を返す。
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

    /// セル `c` の状態を返す。
    pub(crate) fn cell(&self, c: Cell) -> &TVar {
        &self.cells[c.0 as usize]
    }

    /// セルを書き換える(Diktor の `set_tv`)。入力を取り消せるように、書き換える前の値を記録する。
    pub(crate) fn set_tv(&mut self, c: Cell, v: TVar) {
        self.cell_trail.record(&self.cells, c.0 as usize);
        self.cells[c.0 as usize] = v;
    }

    pub(crate) fn new_cell(&mut self, v: TVar) -> Cell {
        self.cells.push(v);
        Cell((self.cells.len() - 1) as u32)
    }

    /// `t` のノードが型変数なら、そのセルの添字を返す。代表まではたどらない。
    pub(crate) fn var_cell(&self, t: Ty) -> Option<Cell> {
        match self.node_ref(t) {
            TyNode::Var(c) => Some(*c),
            _ => None,
        }
    }

    /// 型を代表までたどる(Diktor の `Type.repr`)。途中のリンクは、代表を直接指すように書き換える(経路圧縮)。
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

    /// 型 `f` を型 `a` に適用した型を作る(Diktor の `Type.tapp`)。
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

    /// 型適用を、先頭の型と引数の並びに分解する(Diktor の `Type.app_spine`)。
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

    /// 行を、フィールド(ラベルと型の組)の並びと行の末尾に分解する(Diktor の `Type.row_fields`)。
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

    /// 行の末尾が型変数なら、そのセルの添字を返す(Diktor の `Type.row_tail_var`)。
    pub(crate) fn row_tail_var(&mut self, t: Ty) -> Option<Cell> {
        let (_, tail) = self.row_fields(t);
        self.var_cell(tail)
    }

    /// 2 つの行を連結する(Diktor の `Type.row_append`)。
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

    /// `t` のノードが空の行なら真を返す。代表まではたどらない(Diktor の構造の比較 `= TRowEmpty`)。
    pub(crate) fn is_row_empty(&self, t: Ty) -> bool {
        matches!(self.node_ref(t), TyNode::RowEmpty)
    }

    /// `t` を代表までたどった型が空の行なら真を返す。
    pub(crate) fn repr_is_empty(&mut self, t: Ty) -> bool {
        let t = self.repr(t);
        self.is_row_empty(t)
    }

    /// 新しい型変数を作る(Diktor の `Type.new_var`)。
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

    /// 新しいスコーレム定数(型パラメータを固定した定数。コードでは `Rigid`)のセルを作る(Diktor の `Type.new_rigid_ref`)。
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

    /// 一般化された型変数を作る(Diktor の `Decls.generic`)。
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

    /// 引数の型の並び `tys` から、各型をラベル `_item` のフィールドにした閉じた行を作る(Diktor の `closed_args_row`、`closed_item_row`)。
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

    /// 新しいカインド変数を作る(Diktor の `new_kind_var`)。
    pub(crate) fn new_kind_var(&mut self) -> Kind {
        let id = self.new_oid();
        self.mk_kind(KNode::Var(id, None))
    }

    pub(crate) fn k_arrow2(&mut self, a: Kind, b: Kind) -> Kind {
        self.mk_kind(KNode::Arrow(a, b))
    }

    /// 引数が `arity` 個のカインド `[_, ...] Type` を作る(Diktor の `k_arrow`)。
    pub(crate) fn k_arrow(&mut self, arity: u32) -> Kind {
        if arity == 0 {
            self.k_star
        } else {
            let r = self.k_arrow(arity - 1);
            let s = self.k_star;
            self.k_arrow2(s, r)
        }
    }

    /// カインドを代表までたどる(Diktor の `kind_repr`)。
    pub(crate) fn kind_repr(&mut self, k: Kind) -> Kind {
        match self.kinds[k.0 as usize] {
            KNode::Var(id, Some(next)) => {
                let k2 = self.kind_repr(next);
                if k2 != next {
                    self.kind_trail.record(&self.kinds, k.0 as usize);
                    self.kinds[k.0 as usize] = KNode::Var(id, Some(k2));
                }
                k2
            }
            _ => k,
        }
    }

    /// カインドを代表までたどり、その構造を返す。
    pub(crate) fn kview(&mut self, k: Kind) -> KView {
        let k = self.kind_repr(k);
        match self.kinds[k.0 as usize] {
            KNode::Star => KView::Star,
            KNode::Row => KView::Row,
            KNode::Arrow(a, b) => KView::Arrow(a, b),
            KNode::Var(..) => KView::Var(k),
        }
    }

    pub(crate) fn set_klink(&mut self, k: Kind, to: Kind) {
        if let KNode::Var(id, _) = self.kinds[k.0 as usize] {
            self.kind_trail.record(&self.kinds, k.0 as usize);
            self.kinds[k.0 as usize] = KNode::Var(id, Some(to));
        }
    }

    /// 2 つのカインドを単一化し、一致させられれば真を返す(Diktor の `same_kind`)。
    ///
    /// Diktor と同じく occurs check をしないので、カインドは循環することがある。
    /// 循環したカインドどうしを比べると再帰が終わらず、スタック不足([`Fail::Limit`])で失敗する。
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

    /// 決まらずに残ったカインド変数を `Type` に決める(Diktor の `default_kind`)。
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

    /// 矢印のカインドが取る引数の個数を返す(Diktor の `show_kind` の `args` と、elab.ml の `arity`)。
    pub(crate) fn kind_arity(&mut self, k: Kind) -> R<usize> {
        guard()?;
        match self.kview(k) {
            KView::Arrow(_, r) => Ok(1 + self.kind_arity(r)?),
            _ => Ok(0),
        }
    }

    /// カインドを文字列にする(Diktor の `show_kind`)。
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

    /// 穴 `h` の解を設定する。入力を取り消せるように、書き換える前の値を記録する。
    pub(crate) fn set_hole_sol(&mut self, h: HoleId, ev: Evidence) {
        self.hole_trail.record(&self.holes_arena, h.0 as usize);
        self.holes_arena[h.0 as usize].h_sol = Some(ev);
    }

    pub(crate) fn add_hole(&mut self, h: Hole) -> HoleId {
        self.holes_arena.push(h);
        HoleId((self.holes_arena.len() - 1) as u32)
    }
}

/// 名前の表示名を返す(Diktor の `Type.display`)。
///
/// 単位の接頭辞 `%N.` が付いた内部名 `%N.name` からは、接頭辞を取り除いた `name` を返す。
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
