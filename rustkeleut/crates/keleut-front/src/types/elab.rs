//! 精緻化の式の部分である(Diktor の elab.ml の §11.1〜§11.25)。
//!
//! 型式、パターン、式、handle の検査を持つ。
//! 束縛と再帰群は `elab_bind`、宣言は `decl` に置く。

use super::unify::type_msg;
use super::{
    Cell, DictInfo, Env, Fail, GRef, KView, Kind, Oid, R, Resolved, TVar, Tc, Ty, TyNode, UnitId,
    at, bug, guard, noimpl, terr,
};
use crate::ast::{
    BinOp, BraceElem, Clause, CtorArgPat, Exp, ExpKind, LetBinding, LongId, NodeId, NumSuffix,
    Number, Pat, PatKind, TypeExp, TypeExpKind, TypeParam,
};
use crate::pos::Span;
use std::rc::Rc;

/// resume を書けない理由(Diktor の `no_resume`)である。
#[derive(Clone, Copy, Debug)]
pub enum NoResume {
    NeverClause(Oid),
    Guard,
    ReturnClause,
    CancelClause,
}

/// 再帰群の記録(Diktor の `group_rec`)である。
#[derive(Clone)]
pub struct GroupRec {
    pub(crate) g_pres: Vec<(Ty, NodeId)>,
    pub(crate) g_cur: Option<NodeId>,
    /// 参照(式、参照した束縛、参照された束縛、辞書の鍵と穴)。足した順。
    pub(crate) g_refs: Vec<GroupRef>,
}

/// 再帰群の中の参照(式、その位置、参照した束縛、参照された束縛、辞書の鍵と穴)である。
pub(crate) type GroupRef = (
    NodeId,
    Span,
    Option<NodeId>,
    NodeId,
    Vec<((Oid, Oid), super::HoleId)>,
);

/// pub の注釈の検査の文脈(Diktor の `pub_scope`)である。
#[derive(Clone)]
pub(crate) struct PubScope {
    ps_module: Option<String>,
    ps_unit: UnitId,
    ps_tparams: Vec<String>,
    ps_alias: Option<String>,
}

/// 演算子の意味(Diktor の `Prims.bin_op_sem`)。
pub(crate) fn bin_op_sem(op: BinOp) -> Option<(&'static str, &'static str)> {
    use BinOp::*;
    match op {
        Add => Some(("Add", "add")),
        Sub => Some(("Sub", "sub")),
        Mul => Some(("Mul", "mul")),
        Div => Some(("Div", "div")),
        Rem => Some(("Rem", "rem")),
        Eq | Ne => Some(("Eq", "eq")),
        Lt => Some(("Ord", "lt")),
        Le => Some(("Ord", "le")),
        Gt => Some(("Ord", "gt")),
        Ge => Some(("Ord", "ge")),
        And | Or => None,
    }
}

/// ランタイムが実装を持つエフェクト(Diktor の `Prims.runtime_effects`)。
const RUNTIME_EFFECTS: &[&str] = &["Console"];

/// 数値型の無い名前(Diktor の `absent_numeric`)。
const ABSENT_NUMERIC: &[&str] = &[
    "Int8", "Int16", "UInt8", "UInt16", "UInt32", "UInt64", "Float32",
];

pub(crate) fn has_sub(sub: &str, s: &str) -> bool {
    s.contains(sub)
}

fn row_failure(msg: &str) -> bool {
    has_sub("行型ではありません", msg)
        || has_sub("スコープ付きの型", msg)
        || has_sub("ラベル ", msg)
        || has_sub("は注釈で固定された行変数", msg)
}

fn escape_failure(msg: &str) -> bool {
    has_sub("がスコープの外に漏れています", msg)
}

fn op_row_note(what: &str) -> String {
    format!(
        "({what}が求める関数の行は空 = 純粋です。操作の型の矢印は書いたとおりに読み、@ の省略は @ {{}} と読みます。エフェクトを起こす関数を通すなら、操作の型のその矢印に @ で行を書いてください。§9)"
    )
}

pub(crate) fn show_long_id(li: &LongId) -> String {
    li.0.join(".")
}

fn upper_initial(s: &str) -> bool {
    s.as_bytes().first().is_some_and(|c| c.is_ascii_uppercase())
}

fn lower_initial(s: &str) -> bool {
    s.as_bytes().first().is_some_and(|c| c.is_ascii_lowercase())
}

/// パターンが値を問わず一致するか(Diktor の `irrefutable_pat`)。
pub(crate) fn irrefutable_pat(p: &Pat) -> bool {
    match &p.kind {
        PatKind::Var(_) | PatKind::Wildcard => true,
        PatKind::Annot(q, _) => irrefutable_pat(q),
        PatKind::Record(fields, rest) => {
            fields.iter().all(|(_, q)| irrefutable_pat(q))
                && rest.as_ref().is_none_or(|r| irrefutable_pat(r))
        }
        _ => false,
    }
}

/// パターンが束縛する名前(Diktor の `pat_var_names`)。
pub(crate) fn pat_var_names(p: &Pat) -> Vec<String> {
    match &p.kind {
        PatKind::Var(x) => vec![x.clone()],
        PatKind::Annot(q, _) => pat_var_names(q),
        PatKind::Record(fs, tail) => {
            let mut v: Vec<String> = fs.iter().flat_map(|(_, q)| pat_var_names(q)).collect();
            if let Some(t) = tail {
                v.extend(pat_var_names(t));
            }
            v
        }
        PatKind::Ctor(_, args) => args.iter().flat_map(|a| pat_var_names(&a.pat)).collect(),
        PatKind::Variant(_, q) => pat_var_names(q),
        _ => Vec::new(),
    }
}

/// 値か(Diktor の `is_value`)。
pub(crate) fn is_value(e: &Exp) -> bool {
    match &e.kind {
        ExpKind::Bool(_)
        | ExpKind::Number(_)
        | ExpKind::Text(_)
        | ExpKind::Ident(_)
        | ExpKind::Hole
        | ExpKind::Lambda(_)
        | ExpKind::RecordEmpty => true,
        ExpKind::Variant(_, v) => is_value(v),
        ExpKind::Construct(_, args) => args.iter().all(|a| is_value(&a.exp)),
        ExpKind::RecordExtend(r, _, v) => is_value(r) && is_value(v),
        ExpKind::RecordRestriction(r, _) => is_value(r),
        _ => false,
    }
}

/// 引数の個数(Diktor の `args_arity`)。
fn args_arity(e: &Exp) -> Option<usize> {
    let mut n = 0;
    let mut cur = e;
    loop {
        match &cur.kind {
            ExpKind::RecordEmpty => return Some(n),
            ExpKind::RecordExtend(rest, l, _) if l == "_item" => {
                n += 1;
                cur = rest;
            }
            _ => return None,
        }
    }
}

/// 外側の矢印に `@` を書いたか(Diktor の `outer_eff_written`)。
pub(crate) fn outer_eff_written(b: &LetBinding) -> bool {
    match &b.params {
        Some(_) => b.eff.is_some(),
        None => match &b.ret {
            Some(t) => match &t.kind {
                TypeExpKind::Arrow(_, _, eff) => eff.is_some(),
                _ => true,
            },
            None => false,
        },
    }
}

impl Env {
    /// 局所の束縛を足す(Diktor の `add_local`)。
    pub(crate) fn add_local(&self, x: &str, t: Ty) -> Env {
        let mut e = self.clone();
        e.values = e.values.add(Rc::from(x), t);
        e.tops = e.tops.remove(x);
        e
    }

    pub(crate) fn with_types(&self, rigids: &[(String, Ty, Cell)]) -> Env {
        let mut e = self.clone();
        for (n, t, _) in rigids {
            e.types = e.types.add(Rc::from(n.as_str()), *t);
        }
        e
    }
}

impl Tc {
    /// 大域の束縛を足す(Diktor の `add_top`)。
    pub(crate) fn add_top(&mut self, env: &Env, x: &str, t: Ty, g: GRef) -> Env {
        self.top_types.insert(g.clone(), t);
        let mut e = env.clone();
        e.values = e.values.add(Rc::from(x), t);
        e.tops = e.tops.add(Rc::from(x), g);
        e
    }

    /// 警告を足す(Diktor の `warn`)。
    pub(crate) fn warn(&mut self, msg: String) {
        self.warnings.push(msg);
    }

    // ---- 数値リテラル ----

    /// 数値リテラルの型(Diktor の `number_ty`)。
    pub(crate) fn number_ty(&mut self, loc: Span, level: i32, n: &Number) -> R<Ty> {
        match n.suffix {
            Some(NumSuffix::Int(_) | NumSuffix::UInt(_)) if n.is_float => terr(format!(
                "数値リテラル {} は浮動小数の本体に整数接尾辞が付いています",
                n.show()
            )),
            Some(NumSuffix::Int(32)) => Ok(self.t_int32),
            Some(NumSuffix::Int(64)) => Ok(self.t_int64),
            Some(NumSuffix::Float(64)) => Ok(self.t_float64),
            Some(_) => terr(format!(
                "数値リテラル {} の接尾辞 {} はありません(接尾辞は i32、i64、f64 だけです)",
                n.show(),
                n.suffix_text
            )),
            None => {
                let v = self.new_var(level);
                let c = if n.is_float {
                    self.cls_fractional
                } else {
                    self.cls_integral
                };
                self.add_class(None, v, c)?;
                self.note_literal(loc, v, c);
                Ok(v)
            }
        }
    }

    /// 数値リテラルのパターンの範囲を調べる(Diktor の `check_pattern_numbers`)。
    pub(crate) fn check_pattern_numbers(&mut self) -> R<()> {
        let pats = std::mem::take(&mut self.pattern_numbers);
        for (id, span, n) in pats {
            let t = self.get_ty(id)?;
            let t = self.repr(t);
            let i32o = self.intern("Int32");
            let i64o = self.intern("Int64");
            let fits = match self.node_ref(t) {
                TyNode::Con(c, args) if args.is_empty() && *c == i32o => {
                    Some(("Int32", super::exhaust::ocaml_int32_opt(&n.text).is_some()))
                }
                TyNode::Con(c, args) if args.is_empty() && *c == i64o => {
                    Some(("Int64", super::exhaust::ocaml_int64_opt(&n.text).is_some()))
                }
                _ => None,
            };
            if let Some((tyname, false)) = fits {
                return Err(Fail::TypeAt(
                    span,
                    format!(
                        "数値リテラルのパターン {} は {tyname} の範囲を超えるので、どの値にも一致しません",
                        n.show()
                    ),
                ));
            }
        }
        Ok(())
    }

    fn unbound_value(&mut self, name: &str, scoped: Option<Oid>) -> R<Ty> {
        match scoped {
            Some(_) => terr(format!("未束縛の変数: {name}")),
            None => {
                let o = self.intern(name);
                let qs: Vec<Oid> = self
                    .val_synonym_candidates(o)
                    .into_iter()
                    .filter(|q| self.d.value_visibility.get(q).is_none_or(|v| v.vis_pub))
                    .collect();
                if qs.is_empty() {
                    terr(format!("未束縛の変数: {name}"))
                } else {
                    let names: Vec<String> = qs.iter().map(|q| self.display_of(*q)).collect();
                    terr(format!(
                        "未束縛の変数: {name}({} と修飾してください)",
                        names.join(" か ")
                    ))
                }
            }
        }
    }

    // ---- pub の純粋な行 ----

    pub(crate) fn new_pub_pure_row(&mut self) -> Ty {
        let e = self.row_empty;
        let r = self.new_cell(TVar::Link(e));
        self.pub_pure_cells.insert(r);
        self.tvar(r)
    }

    fn is_pub_pure_row(&self, eff: Ty) -> bool {
        let mut cur = eff;
        loop {
            match self.node_ref(cur) {
                TyNode::Var(r) if self.pub_pure_cells.contains(r) => return true,
                TyNode::Var(r) => match self.cell(*r) {
                    TVar::Link(t) => cur = *t,
                    _ => return false,
                },
                TyNode::RowExtend(_, _, rest) => cur = *rest,
                _ => return false,
            }
        }
    }

    fn closed_row_arg(&mut self, t: Ty) -> bool {
        let t = self.repr(t);
        match self.node(t) {
            TyNode::RowEmpty => true,
            TyNode::RowExtend(..) => {
                let (_, tail) = self.row_fields(t);
                self.is_row_empty(tail)
            }
            _ => false,
        }
    }

    pub(crate) fn has_closed_arrow(&mut self, ty: Ty) -> R<bool> {
        guard()?;
        let ty = self.repr(ty);
        Ok(match self.node(ty) {
            TyNode::Arrow(args, ret, eff) => {
                let (_, tail) = self.row_fields(eff);
                self.is_row_empty(tail)
                    || self.has_closed_arrow(args)?
                    || self.has_closed_arrow(ret)?
            }
            TyNode::Con(_, tys) => {
                for t in tys.iter() {
                    if self.closed_row_arg(*t) || self.has_closed_arrow(*t)? {
                        return Ok(true);
                    }
                }
                false
            }
            TyNode::App(f, a) => self.has_closed_arrow(f)? || self.has_closed_arrow(a)?,
            TyNode::Record(r) | TyNode::Variant(r) => self.has_closed_arrow(r)?,
            TyNode::RowExtend(_, f, rest) => {
                self.has_closed_arrow(f)? || self.has_closed_arrow(rest)?
            }
            TyNode::RowEmpty | TyNode::Var(_) => false,
        })
    }

    fn check_call_arity(&mut self, what: &str, arg: &Exp, params: Ty) -> R<()> {
        if let (Some(n), Some(m)) = (args_arity(arg), self.item_arity(params))
            && n != m
        {
            return terr(format!(
                "引数の個数が一致しません: {what} {m} 個の引数を取りますが、{n} 個を渡しています"
            ));
        }
        Ok(())
    }

    /// 閉じた矢印の行を開く(Diktor の `open_closed_arrow`)。
    pub(crate) fn open_closed_arrow(&mut self, level: i32, ty: Ty) -> R<Ty> {
        let r = self.repr(ty);
        match self.node(r) {
            TyNode::Arrow(a, ret, e) => {
                let (_, tail) = self.row_fields(e);
                if self.repr_is_empty(tail) {
                    let v = self.new_row_var(level);
                    if let Some(rv) = self.var_cell(v) {
                        self.opened_rows.push(rv);
                    }
                    let e2 = self.row_append(e, v)?;
                    Ok(self.arrow(a, ret, e2))
                } else {
                    Ok(ty)
                }
            }
            _ => Ok(ty),
        }
    }

    /// 開いた末尾を閉じ直す(Diktor の `reclose_opened_tail`)。
    pub(crate) fn reclose_opened_tail(&mut self, lvl: i32, ty: Ty) -> R<()> {
        let ty = self.repr(ty);
        if let TyNode::Arrow(_, _, e) = self.node(ty) {
            let (_, tail) = self.row_fields(e);
            let tail = self.repr(tail);
            if let Some(r) = self.var_cell(tail)
                && let TVar::Unbound(i) = self.cell(r).clone()
                && i.vlevel >= lvl
            {
                let opened = self.opened_rows.clone();
                let mut found = false;
                for rv in opened {
                    let t = self.tvar(rv);
                    let t = self.repr(t);
                    if self.var_cell(t) == Some(r) {
                        found = true;
                        break;
                    }
                }
                if found {
                    let t = self.tvar(r);
                    let e = self.row_empty;
                    self.unify(t, e)?;
                }
            }
        }
        Ok(())
    }

    /// 期待型が矢印なら閉じた行を開く(Diktor の `subsume_closed`)。
    pub(crate) fn subsume_closed(&mut self, level: i32, ty: Ty, expected: Ty) -> R<Ty> {
        let e = self.repr(expected);
        match self.node_ref(e) {
            TyNode::Arrow(..) => self.open_closed_arrow(level, ty),
            _ => Ok(ty),
        }
    }

    /// pub の値の注釈と本体を単一化する(Diktor の `unify_pub_value_annot`)。
    pub(crate) fn unify_pub_value_annot(
        &mut self,
        vty: Ty,
        body: Ty,
        other: &dyn Fn(String) -> Fail,
    ) -> R<()> {
        let ta = self.repr(vty);
        let tb = self.repr(body);
        match (self.node(ta), self.node(tb)) {
            (TyNode::Arrow(p1, r1, e1), TyNode::Arrow(p2, r2, e2)) => {
                let r = (|| {
                    self.check_arity(ta, tb, p1, p2)?;
                    self.unify(p1, p2)?;
                    self.unify(r1, r2)
                })();
                match r {
                    Err(Fail::Type(msg)) => return Err(other(msg)),
                    r => r?,
                }
                match self.unify(e1, e2) {
                    Err(Fail::Type(msg)) => terr(format!(
                        "pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: {msg})"
                    )),
                    r => r,
                }
            }
            _ => match self.unify(vty, body) {
                Err(Fail::Type(msg)) => Err(other(msg)),
                r => r,
            },
        }
    }

    // ---- pub の注釈 ----

    fn lookup_alias_in(
        &mut self,
        sc: &PubScope,
        name: &str,
    ) -> Option<(Oid, Rc<super::AliasInfo>)> {
        let saved = self.d.current_module.clone();
        let saved_unit = self.d.current_unit;
        self.d.current_module = sc.ps_module.clone();
        self.d.current_unit = sc.ps_unit;
        let o = self.intern(name);
        let oid = self.resolve_con(o);
        let r = match (
            self.d.aliases.get(&oid).cloned(),
            self.con_visible_here(oid),
        ) {
            (_, Some(false)) => None,
            (Some(info), _) => Some((oid, info)),
            (None, _) => None,
        };
        self.d.current_module = saved;
        self.d.current_unit = saved_unit;
        r
    }

    fn unwritten_alias_body(
        &mut self,
        expanding: &[Oid],
        sc: &PubScope,
        li: &LongId,
        nargs: usize,
    ) -> Option<Option<String>> {
        if li.0.len() == 1 && sc.ps_tparams.contains(&li.0[0]) {
            return None;
        }
        match self.lookup_alias_in(sc, &show_long_id(li)) {
            None => None,
            Some((_, info)) if info.al_kind.as_deref() == Some("EffectRow") => None,
            Some((oid, _)) if expanding.contains(&oid) => None,
            Some((_, info)) if nargs != info.al_params.len() => None,
            Some((oid, info)) => {
                let sc2 = PubScope {
                    ps_module: info.al_module.clone(),
                    ps_unit: info.al_unit,
                    ps_tparams: info.al_params.iter().map(|tp| tp.name.clone()).collect(),
                    ps_alias: Some(self.display_of(oid)),
                };
                let mut exp2 = vec![oid];
                exp2.extend_from_slice(expanding);
                self.unwritten_effect(&exp2, &sc2, &info.al_body)
            }
        }
    }

    /// 注釈の中で `@` の無い矢印を探す(Diktor の `unwritten_effect`)。
    pub(crate) fn unwritten_effect(
        &mut self,
        expanding: &[Oid],
        sc: &PubScope,
        te: &TypeExp,
    ) -> Option<Option<String>> {
        let first = |tc: &mut Tc, ts: &[&TypeExp]| -> Option<Option<String>> {
            for t in ts {
                if let Some(r) = tc.unwritten_effect(expanding, sc, t) {
                    return Some(r);
                }
            }
            None
        };
        match &te.kind {
            TypeExpKind::Arrow(params, ret, eff) => {
                if eff.is_none() {
                    Some(sc.ps_alias.clone())
                } else {
                    let mut v: Vec<&TypeExp> = params.iter().collect();
                    v.push(ret);
                    first(self, &v)
                }
            }
            TypeExpKind::Ident(li) => self.unwritten_alias_body(expanding, sc, li, 0),
            TypeExpKind::Apply(f, args) => match &f.kind {
                TypeExpKind::Ident(li) => {
                    let v: Vec<&TypeExp> = args.iter().collect();
                    match first(self, &v) {
                        Some(r) => Some(r),
                        None => self.unwritten_alias_body(expanding, sc, li, args.len()),
                    }
                }
                _ => {
                    let mut v: Vec<&TypeExp> = vec![f];
                    v.extend(args.iter());
                    first(self, &v)
                }
            },
            TypeExpKind::BraceRow(elems, ext) => {
                for e in elems {
                    let r = match e {
                        BraceElem::Field(_, t) => first(self, &[t]),
                        BraceElem::Label(_, ts) => {
                            let v: Vec<&TypeExp> = ts.iter().collect();
                            first(self, &v)
                        }
                    };
                    if r.is_some() {
                        return r;
                    }
                }
                match ext {
                    Some(t) => self.unwritten_effect(expanding, sc, t),
                    None => None,
                }
            }
            TypeExpKind::VariantCase(_, Some(t)) => self.unwritten_effect(expanding, sc, t),
            TypeExpKind::Union(ts) => {
                let v: Vec<&TypeExp> = ts.iter().collect();
                first(self, &v)
            }
            TypeExpKind::VariantCase(_, None) | TypeExpKind::Hole => None,
        }
    }

    pub(crate) fn pub_scope(&self, tparams: Vec<String>) -> PubScope {
        PubScope {
            ps_module: self.d.current_module.clone(),
            ps_unit: self.d.current_unit,
            ps_tparams: tparams,
            ps_alias: None,
        }
    }

    pub(crate) fn pub_tparams(env: &Env, tps: &[TypeParam]) -> Vec<String> {
        let mut v: Vec<String> = tps.iter().map(|tp| tp.name.clone()).collect();
        v.extend(env.types.keys().iter().map(|k| k.to_string()));
        v
    }

    pub(crate) fn pub_effect_error(what: &str, r: Option<Option<String>>) -> R<()> {
        match r {
            None => Ok(()),
            Some(None) => terr(format!(
                "{what}には完全な型注釈が必要です(注釈の中の矢印に @ がありません)"
            )),
            Some(Some(a)) => terr(format!(
                "{what}には完全な型注釈が必要です(型エイリアス {a} の本体の矢印に @ がありません)"
            )),
        }
    }

    /// pub な宣言の注釈を調べる(Diktor の `check_pub_annots`)。
    pub(crate) fn check_pub_annots(
        &mut self,
        tparams: Vec<String>,
        params: Option<&[Pat]>,
        ret: Option<&TypeExp>,
    ) -> R<()> {
        let sc = self.pub_scope(tparams);
        let what = "pub な宣言";
        if let Some(ps) = params {
            for p in ps {
                match &p.kind {
                    PatKind::Annot(_, te) => {
                        let r = self.unwritten_effect(&[], &sc, te);
                        Self::pub_effect_error(what, r)?;
                    }
                    _ => {
                        return terr(
                            "pub な宣言には完全な型注釈が必要です(引数に型注釈がありません)",
                        );
                    }
                }
            }
        }
        match ret {
            None => terr("pub な宣言には完全な型注釈が必要です(返り値の型注釈がありません)"),
            Some(te) => match (params, &te.kind) {
                (None, TypeExpKind::Arrow(ps, r, _)) => {
                    for t in ps.iter().chain(std::iter::once(&**r)) {
                        let x = self.unwritten_effect(&[], &sc, t);
                        Self::pub_effect_error(what, x)?;
                    }
                    Ok(())
                }
                _ => {
                    let x = self.unwritten_effect(&[], &sc, te);
                    Self::pub_effect_error(what, x)
                }
            },
        }
    }

    // ---- カインドの誤り ----

    fn kind_raw_is(&self, k: Kind, row: bool) -> bool {
        matches!(
            (self.kinds[k.0 as usize], row),
            (super::KNode::Row, true) | (super::KNode::Star, false)
        )
    }

    fn kind_error<T>(&mut self, what: &str, expected: &str, ty: Ty) -> R<T> {
        let k = self.kind_of(ty)?;
        let annot = match self.kview(k) {
            KView::Var(_) => String::new(),
            _ => format!(" :: {}", self.show_kind(k)?),
        };
        let s = self.show(ty)?;
        terr(format!(
            "{what}のカインドが {expected} ではありません: {s}{annot}"
        ))
    }

    fn unresolved_kind(&mut self, ty: Ty) -> R<bool> {
        let k = self.kind_of(ty)?;
        Ok(matches!(self.kview(k), KView::Var(_)))
    }

    pub(crate) fn check_value_kind(&mut self, what: &str, ty: Ty) -> R<()> {
        let k = self.kind_of(ty)?;
        let s = self.k_star;
        if !self.same_kind(k, s)? {
            return self.kind_error(what, "Type", ty);
        }
        Ok(())
    }

    pub(crate) fn check_alias_body_kind(&mut self, name: &str, ty: Ty) -> R<()> {
        let k = self.kind_of(ty)?;
        if let KView::Row = self.kview(k) {
            let s = self.show(ty)?;
            return terr(format!(
                "Type エイリアス {name} の本体のカインドが Type ではありません: {s} :: Row(行に名前を付けるには : EffectRow を付けてください)"
            ));
        }
        Ok(())
    }

    fn unknown_type(&self, n: &str, oid: Oid) -> String {
        let cands: Vec<Oid> = self
            .con_synonym_candidates(oid)
            .into_iter()
            .filter(|q| self.d.con_visibility.get(q).is_none_or(|v| v.vis_pub))
            .collect();
        if !cands.is_empty() {
            let names: Vec<String> = cands.iter().map(|q| self.display_of(*q)).collect();
            format!("未知の型: {n}({} と修飾してください)", names.join(" か "))
        } else if ABSENT_NUMERIC.contains(&n) {
            format!("未知の型: {n}(数値型は Int32、Int64、Float64 だけです)")
        } else {
            format!("未知の型: {n}")
        }
    }

    // ---- 型式 ----

    fn ty_elab(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        outer: bool,
        t: &TypeExp,
    ) -> R<Ty> {
        guard()?;
        let r = self.ty_elab_inner(env, level, expanding, outer, t);
        at(t.meta.span, r)
    }

    fn con_ident(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        name: &str,
        short: Option<&str>,
    ) -> R<Ty> {
        let o = self.intern(name);
        let oid = self.resolve_con(o);
        self.check_con_visible(oid)?;
        if let Some(info) = self.d.aliases.get(&oid).cloned() {
            return self.expand_alias(env, level, expanding, &info, &[]);
        }
        if let Some(k) = self.d.con_kinds.get(&oid).copied() {
            if self.kind_raw_is(k, false) {
                Ok(self.con0(oid))
            } else {
                terr(format!("型構成子 {name} には型引数が必要です"))
            }
        } else {
            match short {
                Some(n) => terr(self.unknown_type(n, oid)),
                None => terr(format!("未知の型: {name}")),
            }
        }
    }

    fn ty_elab_inner(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        outer: bool,
        t: &TypeExp,
    ) -> R<Ty> {
        match &t.kind {
            TypeExpKind::Ident(li) if li.0.len() == 1 => {
                let n = &li.0[0];
                match env.types.get(n.as_str()) {
                    Some(t) => Ok(*t),
                    None => self.con_ident(env, level, expanding, n, Some(n)),
                }
            }
            TypeExpKind::Ident(li) => {
                self.con_ident(env, level, expanding, &show_long_id(li), None)
            }
            TypeExpKind::Apply(head, args) => match &head.kind {
                TypeExpKind::Ident(LongId(comps)) if comps.len() > 1 => {
                    let name = comps.join(".");
                    let o = self.intern(&name);
                    let oid = self.resolve_con(o);
                    self.check_con_visible(oid)?;
                    if let Some(info) = self.d.aliases.get(&oid).cloned() {
                        return self.expand_alias(env, level, expanding, &info, args);
                    }
                    if self.d.con_kinds.contains_key(&oid) {
                        let k = self.con_kind(oid, args.len());
                        if self.kind_arity(k)? != args.len() {
                            terr(format!("型構成子 {name} の引数の個数が不正です"))
                        } else {
                            let a = self.con_args(env, level, expanding, &name, k, args)?;
                            Ok(self.con(oid, a))
                        }
                    } else {
                        terr(format!("未知の型: {name}"))
                    }
                }
                TypeExpKind::Ident(LongId(comps)) if comps.len() == 1 => {
                    let n = &comps[0];
                    match env.types.get(n.as_str()).copied() {
                        Some(t0) => {
                            for a in args {
                                check_no_hole(a)?;
                            }
                            let mut acc = t0;
                            for a in args {
                                let at_ = self.ty_elab(env, level, expanding, false, a)?;
                                let acc2 = self.tapp(acc, at_);
                                self.kind_of(acc2)?;
                                let ka = self.kind_of(acc)?;
                                if let KView::Arrow(d, _) = self.kview(ka) {
                                    let kat = self.kind_of(at_)?;
                                    if !self.same_kind(d, kat)? {
                                        let ds = self.show_kind(d)?;
                                        let s = self.show(at_)?;
                                        let kat2 = self.kind_of(at_)?;
                                        let ks = self.show_kind(kat2)?;
                                        return terr(format!(
                                            "型引数のカインドが一致しません: {ds} を期待しましたが {s} は {ks} です"
                                        ));
                                    }
                                }
                                acc = acc2;
                            }
                            Ok(acc)
                        }
                        None => {
                            let o = self.intern(n);
                            let oid = self.resolve_con(o);
                            self.check_con_visible(oid)?;
                            if let Some(info) = self.d.aliases.get(&oid).cloned() {
                                return self.expand_alias(env, level, expanding, &info, args);
                            }
                            if self.d.con_kinds.contains_key(&oid) {
                                let k = self.con_kind(oid, args.len());
                                let expected = self.kind_arity(k)?;
                                if expected != args.len() {
                                    terr(format!(
                                        "型構成子 {n} の引数は {expected} 個必要です({} 個与えられました)",
                                        args.len()
                                    ))
                                } else {
                                    let a = self.con_args(env, level, expanding, n, k, args)?;
                                    Ok(self.con(oid, a))
                                }
                            } else {
                                terr(self.unknown_type(n, oid))
                            }
                        }
                    }
                }
                _ => terr("型適用の頭は型名でなければなりません"),
            },
            TypeExpKind::Arrow(params, ret, eff_opt) => {
                let mut param_tys = Vec::new();
                for p in params {
                    param_tys.push(self.value_type(env, level, expanding, "矢印の引数の型", p)?);
                }
                let eff = match eff_opt {
                    Some(e) => self.eff_elab(true, env, level, expanding, e)?,
                    None => {
                        if outer {
                            self.new_row_var(level)
                        } else {
                            self.row_empty
                        }
                    }
                };
                let rt = self.value_type(env, level, expanding, "矢印の返り値の型", ret)?;
                let row = self.closed_item_row(&param_tys);
                let p = self.record(row);
                Ok(self.arrow(p, rt, eff))
            }
            TypeExpKind::BraceRow(elems, ext) => {
                let tail = match ext {
                    None => self.row_empty,
                    Some(e) => {
                        let tt = self.ty_elab(env, level, expanding, false, e)?;
                        let r = self.repr(tt);
                        match self.node(r) {
                            TyNode::Record(row) => row,
                            TyNode::App(..) if self.unresolved_kind(r)? => {
                                return self.kind_error("extends の右の型", "Row", r);
                            }
                            _ => {
                                let k = self.kind_of(r)?;
                                let ok = self.kind_raw_is(k, true) || {
                                    let k2 = self.kind_of(r)?;
                                    let kr = self.k_row;
                                    self.same_kind(k2, kr)?
                                };
                                if ok {
                                    r
                                } else {
                                    return terr(
                                        "extends の右は行かレコード型でなければなりません",
                                    );
                                }
                            }
                        }
                    }
                };
                let mut acc = tail;
                for elem in elems.iter().rev() {
                    match elem {
                        BraceElem::Field(l, ft) => {
                            let what = if l == "_item" {
                                "タプルの要素の型".to_string()
                            } else {
                                format!("レコードのフィールド {l} の型")
                            };
                            let fty = self.value_type(env, level, expanding, &what, ft)?;
                            let lo = self.intern(l);
                            acc = self.extend(lo, fty, acc);
                        }
                        BraceElem::Label(..) => {
                            return terr("エフェクトラベルはこの位置(レコード型)では使えません");
                        }
                    }
                }
                Ok(self.record(acc))
            }
            TypeExpKind::VariantCase(s, payload) => {
                let pty = match payload {
                    None => self.t_unit,
                    Some(p) => self.value_type(
                        env,
                        level,
                        expanding,
                        &format!("ヴァリアント #{s} の積載の型"),
                        p,
                    )?,
                };
                let l = self.intern(s);
                let e = self.row_empty;
                let row = self.extend(l, pty, e);
                Ok(self.variant(row))
            }
            TypeExpKind::Union(ts) => {
                let n = ts.len();
                let mut rows = Vec::new();
                for (i, te) in ts.iter().enumerate() {
                    let is_last = i == n - 1;
                    let row = match &te.kind {
                        TypeExpKind::VariantCase(s, payload) => {
                            let pty = match payload {
                                None => self.t_unit,
                                Some(p) => self.value_type(
                                    env,
                                    level,
                                    expanding,
                                    &format!("ヴァリアント #{s} の積載の型"),
                                    p,
                                )?,
                            };
                            let l = self.intern(s);
                            let e = self.row_empty;
                            self.extend(l, pty, e)
                        }
                        _ => {
                            let tt = self.ty_elab(env, level, expanding, false, te)?;
                            let r = self.repr(tt);
                            match self.node(r) {
                                TyNode::Variant(row) => {
                                    let (_, tail) = self.row_fields(row);
                                    if !is_last && !self.repr_is_empty(tail) {
                                        return terr(
                                            "ヴァリアント和の途中の要素は閉じていなければなりません",
                                        );
                                    }
                                    row
                                }
                                TyNode::App(..) if self.unresolved_kind(r)? => {
                                    return self.kind_error("ヴァリアント和の末尾の型", "Row", r);
                                }
                                _ => {
                                    let k = self.kind_of(r)?;
                                    let kr = self.k_row;
                                    if self.same_kind(k, kr)? {
                                        if is_last {
                                            r
                                        } else {
                                            return terr(
                                                "ヴァリアント和の途中に行変数は置けません",
                                            );
                                        }
                                    } else {
                                        let s = self.show(r)?;
                                        return terr(format!(
                                            "ヴァリアント和の要素になれない型です: {s}"
                                        ));
                                    }
                                }
                            }
                        }
                    };
                    rows.push(row);
                }
                let mut acc: Option<Ty> = None;
                for r in rows.into_iter().rev() {
                    acc = Some(match acc {
                        None => r,
                        Some(rest) => self.row_append(r, rest)?,
                    });
                }
                let row = acc.unwrap_or(self.row_empty);
                Ok(self.variant(row))
            }
            TypeExpKind::Hole => terr("_ はこの位置では使えません(インスタンス頭の List[_] 専用)"),
        }
    }

    fn value_type(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        what: &str,
        t: &TypeExp,
    ) -> R<Ty> {
        let ty = self.ty_elab(env, level, expanding, false, t)?;
        let r = self.check_value_kind(what, ty);
        at(t.meta.span, r)?;
        Ok(ty)
    }

    fn con_args(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        cname: &str,
        k: Kind,
        args: &[TypeExp],
    ) -> R<Vec<Ty>> {
        let mut out = Vec::new();
        let mut k = k;
        for (i, a) in args.iter().enumerate() {
            let (pk, kr) = match self.kview(k) {
                KView::Arrow(a2, r) => (a2, r),
                _ => {
                    let s = self.k_star;
                    (self.new_kind_var(), s)
                }
            };
            check_no_hole(a)?;
            let ambiguous = self.speculating
                && matches!(self.kview(pk), KView::Var(_))
                && matches!(&a.kind, TypeExpKind::BraceRow(e, _) if e.is_empty());
            let t = if ambiguous {
                self.t_unit
            } else {
                match self.kview(pk) {
                    KView::Row => self.eff_elab(false, env, level, expanding, a)?,
                    _ => self.ty_elab(env, level, expanding, false, a)?,
                }
            };
            if !ambiguous && !self.arg_kind_ok(pk, t)? {
                let m = self.arg_kind_mismatch(pk, t)?;
                return terr(format!(
                    "型構成子 {cname} の第{}引数のカインドが一致しません: {m}",
                    i + 1
                ));
            }
            out.push(t);
            k = kr;
        }
        Ok(out)
    }

    fn arg_kind_ok(&mut self, pk: Kind, t: Ty) -> R<bool> {
        let rt = self.repr(t);
        let is_app = matches!(self.node_ref(rt), TyNode::App(..));
        if matches!(self.kview(pk), KView::Row) && is_app {
            let k = self.kind_of(t)?;
            Ok(matches!(self.kview(k), KView::Row))
        } else {
            let k = self.kind_of(t)?;
            Ok(self.same_kind(pk, k)?)
        }
    }

    fn arg_kind_mismatch(&mut self, pk: Kind, t: Ty) -> R<String> {
        let k0 = self.kind_of(t)?;
        let k = self.kind_repr(k0);
        let tail = match self.kview(k) {
            KView::Var(_) => " は行になりません".to_string(),
            _ => format!(" は {} です", self.show_kind(k)?),
        };
        let s = self.show(t)?;
        let p = self.show_kind(pk)?;
        Ok(format!("{p} を期待しましたが {s}{tail}"))
    }

    fn find_row_alias(&mut self, n: &str) -> Option<(Oid, Rc<super::AliasInfo>)> {
        let c = self.intern(n);
        let g = self.resolve_con_global(c);
        let l = self.resolve_con(c);
        for oid in [l, g] {
            if let Some(info) = self.d.aliases.get(&oid)
                && info.al_kind.as_deref() == Some("EffectRow")
            {
                return Some((oid, info.clone()));
            }
        }
        None
    }

    fn find_alias(&mut self, n: &str) -> R<Option<Rc<super::AliasInfo>>> {
        let c = self.intern(n);
        let found = match self.find_row_alias(n) {
            Some(r) => Some(r),
            None => {
                let ru = self.resolve_unit(c);
                if self.d.effects.contains_key(&ru) {
                    None
                } else {
                    let g = self.resolve_con_global(c);
                    let l = self.resolve_con(c);
                    let mut r = None;
                    for oid in [l, g] {
                        if let Some(info) = self.d.aliases.get(&oid) {
                            r = Some((oid, info.clone()));
                            break;
                        }
                    }
                    r
                }
            }
        };
        match found {
            Some((oid, info)) => {
                if info.al_kind.as_deref() == Some("EffectRow") {
                    self.check_con_visible(oid)?;
                }
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    fn unknown_effect(&mut self, n: &str) -> String {
        let o = self.intern(n);
        let rc = self.resolve_con(o);
        if self.d.con_kinds.contains_key(&rc) {
            return format!("型 {n} はエフェクトではありません(ここにはエフェクト行が要ります)");
        }
        let o2 = self.intern(n);
        let cands: Vec<Oid> = self
            .con_synonym_candidates(o2)
            .into_iter()
            .filter(|q| {
                self.d.con_visibility.get(q).is_none_or(|v| v.vis_pub)
                    && self
                        .d
                        .aliases
                        .get(q)
                        .is_some_and(|i| i.al_kind.as_deref() == Some("EffectRow"))
            })
            .collect();
        if cands.is_empty() {
            format!("未知のエフェクト: {n}")
        } else {
            let names: Vec<String> = cands.iter().map(|q| self.display_of(*q)).collect();
            format!(
                "未知のエフェクト: {n}({} と修飾してください)",
                names.join(" か ")
            )
        }
    }

    fn row_closed(&mut self, r: Ty) -> bool {
        let (_, tail) = self.row_fields(r);
        self.is_row_empty(tail)
    }

    fn splice_row(&mut self, alias_name: &str, expanded: Ty, acc: Ty) -> R<Ty> {
        let (fs, tail) = self.row_fields(expanded);
        let base = if self.is_row_empty(tail) {
            acc
        } else if self.row_closed(acc) {
            self.row_append(acc, tail)?
        } else {
            return terr(format!(
                "エイリアス {alias_name} は開いた行に展開されるので、extends や別の開いた行と同じ行には置けません(行変数は 1 つまで)"
            ));
        };
        let mut r = base;
        for (l, a) in fs.into_iter().rev() {
            r = self.extend(l, a, r);
        }
        Ok(r)
    }

    fn expand_alias(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        info: &super::AliasInfo,
        args: &[TypeExp],
    ) -> R<Ty> {
        if expanding.contains(&info.al_name) {
            return terr(format!(
                "型エイリアス {} が再帰しています(エイリアスは非再帰)",
                self.display_of(info.al_name)
            ));
        }
        if args.len() != info.al_params.len() {
            return terr(format!(
                "型エイリアス {} の引数は {} 個必要です({} 個与えられました。部分適用は禁止)",
                self.display_of(info.al_name),
                info.al_params.len(),
                args.len()
            ));
        }
        if info.al_kinds.len() != info.al_params.len() {
            return bug(format!(
                "expand_alias: {} の al_kinds の長さが al_params と違います",
                self.display_of(info.al_name)
            ));
        }
        let mut arg_tys = Vec::new();
        for (a, &pk) in args.iter().zip(info.al_kinds.iter()) {
            check_no_hole(a)?;
            let ambiguous = self.speculating
                && matches!(self.kview(pk), KView::Var(_))
                && matches!(&a.kind, TypeExpKind::BraceRow(e, _) if e.is_empty());
            let t = if ambiguous {
                self.ty_elab(env, level, expanding, false, a)?;
                self.new_var_k(pk, Vec::new(), level)
            } else {
                match self.kview(pk) {
                    KView::Row => self.eff_elab(false, env, level, expanding, a)?,
                    _ => self.ty_elab(env, level, expanding, false, a)?,
                }
            };
            if !ambiguous && !self.arg_kind_ok(pk, t)? {
                let m = self.arg_kind_mismatch(pk, t)?;
                return terr(format!(
                    "型エイリアス {} の型引数のカインドが一致しません: {m}",
                    self.display_of(info.al_name)
                ));
            }
            arg_tys.push(t);
        }
        let mut types = super::pmap::PMap::new();
        for (tp, t) in info.al_params.iter().zip(arg_tys) {
            types = types.add(Rc::from(tp.name.as_str()), t);
        }
        let mut env2 = env.clone();
        env2.types = types;
        let mut exp2 = vec![info.al_name];
        exp2.extend_from_slice(expanding);
        let saved = self.d.current_module.clone();
        let saved_unit = self.d.current_unit;
        self.d.current_module = info.al_module.clone();
        self.d.current_unit = info.al_unit;
        let r = match info.al_kind.as_deref() {
            Some("EffectRow") => self.eff_elab(true, &env2, level, &exp2, &info.al_body),
            _ => self.ty_elab(&env2, level, &exp2, false, &info.al_body),
        };
        self.d.current_module = saved;
        self.d.current_unit = saved_unit;
        r
    }

    fn eff_elab(
        &mut self,
        check_row: bool,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        t: &TypeExp,
    ) -> R<Ty> {
        guard()?;
        let r = self.eff_elab_inner(check_row, env, level, expanding, t);
        at(t.meta.span, r)
    }

    fn eff_label_arg(
        &mut self,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        args: &[TypeExp],
        none: Option<Ty>,
    ) -> R<Ty> {
        match (args, none) {
            ([], Some(u)) => Ok(u),
            ([a], _) => self.ty_elab(env, level, expanding, false, a),
            _ => terr("エフェクトラベルの引数は1個までです"),
        }
    }

    fn eff_elab_inner(
        &mut self,
        check_row: bool,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        t: &TypeExp,
    ) -> R<Ty> {
        match &t.kind {
            TypeExpKind::Ident(LongId(c)) if c.len() == 1 && env.types.contains(c[0].as_str()) => {
                let tv = *env.types.get(c[0].as_str()).expect("型パラメータ");
                let k = self.kind_of(tv)?;
                let kr = self.k_row;
                if self.same_kind(k, kr)? {
                    Ok(tv)
                } else {
                    terr(format!("行カインドではない型パラメータです: {}", c[0]))
                }
            }
            TypeExpKind::Ident(LongId(c)) if c.len() == 1 => {
                let n = &c[0];
                match self.find_alias(n)? {
                    Some(info) => {
                        if info.al_kind.as_deref() == Some("EffectRow") {
                            self.expand_alias(env, level, expanding, &info, &[])
                        } else {
                            terr(format!(
                                "エフェクト位置に Type エイリアス {n} は使えません(: EffectRow を付けてください)"
                            ))
                        }
                    }
                    None => {
                        let o = self.intern(n);
                        let e = self.resolve_unit(o);
                        if self.d.effects.contains_key(&e) {
                            let u = self.t_unit;
                            let em = self.row_empty;
                            Ok(self.extend(e, u, em))
                        } else {
                            let m = self.unknown_effect(n);
                            terr(m)
                        }
                    }
                }
            }
            TypeExpKind::Apply(head, args) if matches!(&head.kind, TypeExpKind::Ident(LongId(c)) if c.len() == 1) =>
            {
                let TypeExpKind::Ident(LongId(c)) = &head.kind else {
                    unreachable!()
                };
                let n = &c[0];
                let is_param = env.types.contains(n.as_str());
                if !is_param && self.find_row_alias(n).is_some() {
                    let (oid, info) = self.find_row_alias(n).expect("行の別名");
                    self.check_con_visible(oid)?;
                    return self.expand_alias(env, level, expanding, &info, args);
                }
                let o = self.intern(n);
                let ru = self.resolve_unit(o);
                if self.d.effects.contains_key(&ru) {
                    let field = self.eff_label_arg(env, level, expanding, args, None)?;
                    let o = self.intern(n);
                    let e = self.resolve_unit(o);
                    let em = self.row_empty;
                    return Ok(self.extend(e, field, em));
                }
                if !is_param {
                    let o = self.intern(n);
                    let oid = self.resolve_con(o);
                    if !(self.d.aliases.contains_key(&oid) || self.d.con_kinds.contains_key(&oid)) {
                        let m = self.unknown_effect(n);
                        return terr(m);
                    }
                }
                self.eff_default(check_row, env, level, expanding, t)
            }
            TypeExpKind::BraceRow(elems, ext) => {
                let tail = match ext {
                    None => self.row_empty,
                    Some(e) => {
                        let tt = self.eff_elab(true, env, level, expanding, e)?;
                        let k = self.kind_of(tt)?;
                        let kr = self.k_row;
                        if self.same_kind(k, kr)? {
                            tt
                        } else {
                            return terr("extends の右は行でなければなりません");
                        }
                    }
                };
                let mut acc = tail;
                for elem in elems.iter().rev() {
                    match elem {
                        BraceElem::Label(li, args) => {
                            let n = show_long_id(li);
                            let is_param = env.types.contains(n.as_str());
                            if args.is_empty() && is_param {
                                return terr(format!("行変数 {n} は extends の位置にのみ書けます"));
                            }
                            let found = if is_param { None } else { self.find_alias(&n)? };
                            match found {
                                Some(info) if info.al_kind.as_deref() == Some("EffectRow") => {
                                    let ex =
                                        self.expand_alias(env, level, expanding, &info, args)?;
                                    acc = self.splice_row(&n, ex, acc)?;
                                }
                                Some(_) => {
                                    return terr(format!(
                                        "エフェクト行に Type エイリアス {n} は置けません(: EffectRow を付けてください)"
                                    ));
                                }
                                None => {
                                    let o = self.intern(&n);
                                    let ru = self.resolve_unit(o);
                                    if self.d.effects.contains_key(&ru) {
                                        let u = self.t_unit;
                                        let field = self.eff_label_arg(
                                            env,
                                            level,
                                            expanding,
                                            args,
                                            Some(u),
                                        )?;
                                        let o = self.intern(&n);
                                        let e = self.resolve_unit(o);
                                        acc = self.extend(e, field, acc);
                                    } else {
                                        let m = self.unknown_effect(&n);
                                        return terr(m);
                                    }
                                }
                            }
                        }
                        BraceElem::Field(l, _) => {
                            return terr(format!("エフェクト行にフィールド {l} は書けません"));
                        }
                    }
                }
                Ok(acc)
            }
            _ => self.eff_default(check_row, env, level, expanding, t),
        }
    }

    fn eff_default(
        &mut self,
        check_row: bool,
        env: &Env,
        level: i32,
        expanding: &[Oid],
        t: &TypeExp,
    ) -> R<Ty> {
        let ty = self.ty_elab(env, level, expanding, false, t)?;
        if check_row {
            let k = self.kind_of(ty)?;
            if !matches!(self.kview(k), KView::Row) {
                return self.kind_error("エフェクト位置の型", "Row", ty);
            }
        }
        Ok(ty)
    }

    /// 注釈の型(外側の矢印の `@` の省略を行変数にする。Diktor の `elab_type_outer`)。
    pub(crate) fn elab_type_outer(&mut self, env: &Env, level: i32, t: &TypeExp) -> R<Ty> {
        let r = self.ty_elab(env, level, &[], true, t);
        at(t.meta.span, r)
    }

    /// 型式(Diktor の `elab_type`)。
    pub(crate) fn elab_type(&mut self, env: &Env, level: i32, t: &TypeExp) -> R<Ty> {
        let r = self.ty_elab(env, level, &[], false, t);
        at(t.meta.span, r)
    }

    /// 値の型式(Diktor の `elab_value_type`)。
    pub(crate) fn elab_value_type(
        &mut self,
        env: &Env,
        level: i32,
        what: &str,
        t: &TypeExp,
    ) -> R<Ty> {
        let r = self.value_type(env, level, &[], what, t);
        at(t.meta.span, r)
    }

    /// 外側の注釈の値の型式(Diktor の `elab_value_type_outer`)。
    pub(crate) fn elab_value_type_outer(
        &mut self,
        env: &Env,
        level: i32,
        what: &str,
        t: &TypeExp,
    ) -> R<Ty> {
        let ty = self.elab_type_outer(env, level, t)?;
        let r = self.check_value_kind(what, ty);
        at(t.meta.span, r)?;
        Ok(ty)
    }

    /// エフェクトの型式(Diktor の `elab_eff`)。
    pub(crate) fn elab_eff(&mut self, env: &Env, level: i32, t: &TypeExp) -> R<Ty> {
        let r = self.eff_elab(true, env, level, &[], t);
        at(t.meta.span, r)
    }

    // ---- パターン ----

    /// パターンの精緻化(Diktor の `elab_pat`)。
    pub(crate) fn elab_pat(
        &mut self,
        env: &Env,
        level: i32,
        seen: &mut Vec<String>,
        expected: Ty,
        p: &Pat,
    ) -> R<Env> {
        guard()?;
        self.set_ty(p.meta.id, expected);
        let r = self.elab_pat_inner(env, level, seen, expected, p);
        at(p.meta.span, r)
    }

    fn elab_pat_inner(
        &mut self,
        env: &Env,
        level: i32,
        seen: &mut Vec<String>,
        expected: Ty,
        p: &Pat,
    ) -> R<Env> {
        match &p.kind {
            PatKind::Wildcard => Ok(env.clone()),
            PatKind::Var(x) => {
                if seen.contains(x) {
                    terr(format!("同じパターン内で変数 {x} が重複しています"))
                } else {
                    seen.push(x.clone());
                    Ok(env.add_local(x, expected))
                }
            }
            PatKind::Annot(sub, te) => {
                let t = self.elab_value_type(env, level, "型注釈", te)?;
                self.unify(expected, t)?;
                self.elab_pat(env, level, seen, expected, sub)
            }
            PatKind::Bool(_) => {
                let b = self.t_boolean;
                self.unify(expected, b)?;
                Ok(env.clone())
            }
            PatKind::Text(_) => {
                let s = self.t_string;
                self.unify(expected, s)?;
                Ok(env.clone())
            }
            PatKind::Number(n) => {
                let t = self.number_ty(p.meta.span, level, n)?;
                self.unify(expected, t)?;
                self.pattern_numbers
                    .push((p.meta.id, p.meta.span, n.clone()));
                Ok(env.clone())
            }
            PatKind::Record(fields, rest) => {
                let ftys: Vec<Ty> = fields.iter().map(|_| self.new_var(level)).collect();
                let tail = match rest {
                    None => self.row_empty,
                    Some(_) => self.new_row_var(level),
                };
                let mut row = tail;
                for ((l, _), t) in fields.iter().zip(ftys.iter()).rev() {
                    let lo = self.intern(l);
                    row = self.extend(lo, *t, row);
                }
                let rt = self.record(row);
                self.unify(expected, rt)?;
                let mut env2 = env.clone();
                for ((_, sub), t) in fields.iter().zip(ftys.iter()) {
                    env2 = self.elab_pat(&env2, level, seen, *t, sub)?;
                }
                match rest {
                    None => Ok(env2),
                    Some(rp) => {
                        let rt = self.record(tail);
                        self.elab_pat(&env2, level, seen, rt, rp)
                    }
                }
            }
            PatKind::Variant(s, sub) => {
                let tf = self.new_var(level);
                let rest = self.new_row_var(level);
                let l = self.intern(s);
                let row = self.extend(l, tf, rest);
                let vt = self.variant(row);
                self.unify(expected, vt)?;
                self.elab_pat(env, level, seen, tf, sub)
            }
            PatKind::Ctor(li, args) => self.elab_ctor_pat(env, level, seen, expected, p, li, args),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn elab_ctor_pat(
        &mut self,
        env: &Env,
        level: i32,
        seen: &mut Vec<String>,
        expected: Ty,
        p: &Pat,
        li: &LongId,
        args: &[CtorArgPat],
    ) -> R<Env> {
        let cname = li.0.last().cloned().unwrap_or_default();
        if !upper_initial(&cname) {
            return terr(format!(
                "未知のコンストラクタパターン: {}(操作節は handle の中でのみ使えます)",
                show_long_id(li)
            ));
        }
        let co = self.intern(&cname);
        let ctor = self.resolve_ctor(co);
        let Some(&dname) = self.d.ctor_owner.get(&ctor) else {
            return terr(format!("未知のコンストラクタ: {}", show_long_id(li)));
        };
        self.check_ctor_visible(ctor, dname)?;
        let dd = self.d.datas[&dname].clone();
        if dd.dd_opaque {
            return terr(format!(
                "newtype {} の表現は ??? で隠されています",
                self.display_of(dname)
            ));
        }
        let Some(ct) = dd.dd_ctors.iter().find(|c| c.ct_name == ctor).cloned() else {
            return bug("List.find: コンストラクタ");
        };
        let nfields = ct.ct_fields.len();
        let mut field_to_arg: Vec<Option<usize>> = vec![None; nfields];
        let positional = args.iter().filter(|a| a.label.is_none()).count();
        if positional > 0 && args.len() != nfields {
            return terr(format!(
                "コンストラクタ {cname} のパターンは {nfields} 個のフィールドを取ります({} 個与えられました)",
                args.len()
            ));
        }
        for (ai, a) in args.iter().enumerate() {
            match &a.label {
                None => {
                    let Some(i) = (0..nfields).find(|i| field_to_arg[*i].is_none()) else {
                        return terr(format!(
                            "コンストラクタ {cname} のパターンの引数が多すぎます"
                        ));
                    };
                    field_to_arg[i] = Some(ai);
                }
                Some(l) => {
                    let lo = self.intern(l);
                    let Some(i) = ct.ct_fields.iter().position(|f| f.fi_label == Some(lo)) else {
                        return terr(format!(
                            "コンストラクタ {cname} にフィールド {l} はありません"
                        ));
                    };
                    if field_to_arg[i].is_some() {
                        return terr(format!("フィールド {l} が二重に指定されています"));
                    }
                    field_to_arg[i] = Some(ai);
                }
            }
        }
        self.set_resolved(
            p.meta.id,
            Resolved::CtorPat(dname, ctor, field_to_arg.clone()),
        );
        let mut subst = Vec::new();
        for i in &dd.dd_params {
            let v = self.new_class_var(i.vkind, i.vcls.clone(), level);
            subst.push((i.vid, v));
        }
        let ct_ty = self.con(dname, subst.iter().map(|(_, t)| *t).collect());
        self.unify(expected, ct_ty)?;
        let mut env2 = env.clone();
        for (fi, arg) in field_to_arg.iter().enumerate() {
            if let Some(ai) = arg {
                let f = &ct.ct_fields[fi];
                let ft = self.subst_params(level, &subst, f.fi_ty)?;
                env2 = self.elab_pat(&env2, level, seen, ft, &args[*ai].pat)?;
            }
        }
        Ok(env2)
    }

    // ---- 辞書の穴 ----

    fn record_group_ref(&mut self, id: NodeId, span: Span, sch: Ty, hs: &[super::HoleId]) -> R<()> {
        let need = self
            .group_stack
            .iter()
            .any(|g| g.g_pres.iter().any(|(p, _)| *p == sch));
        if !need {
            return Ok(());
        }
        let keys = self.dkeys_of(sch)?;
        for g in self.group_stack.iter_mut() {
            if let Some((_, target)) = g.g_pres.iter().find(|(p, _)| *p == sch) {
                let pairs: Vec<_> = keys.iter().copied().zip(hs.iter().copied()).collect();
                g.g_refs.push((id, span, g.g_cur, *target, pairs));
            }
        }
        Ok(())
    }

    /// 穴を解き、束縛の型から決められない辞書を誤りにする(Diktor の `solve_holes_at`)。
    pub(crate) fn solve_holes_at(&mut self, since: i64, params: &[(Oid, Oid)]) -> R<()> {
        match self.solve_holes(since, params) {
            Err(Fail::Unsolved(h)) => {
                let hd = self.hole(h).clone();
                Err(Fail::TypeAt(
                    hd.h_loc.unwrap_or_default(),
                    format!(
                        "曖昧な制約: {} の辞書を束縛の型から決められません(束縛の型に現れない型変数に制約が付いています。注釈で型を決めてください)",
                        self.display_of(hd.h_cls)
                    ),
                ))
            }
            r => r,
        }
    }

    pub(crate) fn check_tparam_constraints(
        &self,
        rigids: &[(String, Ty, Cell)],
        dps: &[(Oid, Oid)],
    ) -> R<()> {
        for (name, _, r) in rigids {
            let i = self.var_info_of(*r)?;
            for c in &i.vcls {
                if !self.is_predicate(*c) && !dps.contains(&(i.vid, *c)) {
                    return terr(format!(
                        "曖昧な制約: 型パラメータ {name} の制約 {} を束縛の型から決められません(束縛の型に現れない型パラメータに制約が付いています)",
                        self.display_of(*c)
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn has_constrained_unbound(&mut self, t: Ty) -> R<bool> {
        guard()?;
        let t = self.repr(t);
        Ok(match self.node(t) {
            TyNode::Var(v) => match self.cell(v) {
                TVar::Unbound(i) => {
                    !i.vcls.iter().any(|c| self.is_predicate(*c))
                        && i.vcls.iter().any(|c| !self.is_predicate(*c))
                }
                _ => false,
            },
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    if self.has_constrained_unbound(*a)? {
                        return Ok(true);
                    }
                }
                false
            }
            TyNode::App(f, a) => {
                self.has_constrained_unbound(f)? || self.has_constrained_unbound(a)?
            }
            TyNode::Arrow(p, r, e) => {
                self.has_constrained_unbound(p)?
                    || self.has_constrained_unbound(r)?
                    || self.has_constrained_unbound(e)?
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.has_constrained_unbound(row)?,
            TyNode::RowEmpty => false,
            TyNode::RowExtend(_, f, rest) => {
                self.has_constrained_unbound(f)? || self.has_constrained_unbound(rest)?
            }
        })
    }

    // ---- 式 ----

    /// 式の精緻化(Diktor の `elab_exp`)。
    pub(crate) fn elab_exp(&mut self, env: &Env, level: i32, eff: Ty, node: &Exp) -> R<Ty> {
        guard()?;
        let r = self.elab_exp_inner(env, level, eff, node);
        let t = at(node.meta.span, r)?;
        let t = match node.kind {
            ExpKind::Ident(_) => self.open_closed_arrow(level, t)?,
            _ => t,
        };
        self.set_ty(node.meta.id, t);
        Ok(t)
    }

    fn method_use(
        &mut self,
        node: &Exp,
        level: i32,
        cls: &str,
        m: &str,
        args: &[Ty],
        eff: Ty,
    ) -> R<Ty> {
        let scheme = self.method_scheme(cls, m)?;
        let ret = self.new_var(level);
        let (it, hs) = self.instantiate_ev(node.meta.span, level, scheme)?;
        self.set_dict(node.meta.id, DictInfo::Use(hs));
        let it = self.open_closed_arrow(level, it)?;
        let row = self.closed_item_row(args);
        let p = self.record(row);
        let want = self.arrow(p, ret, eff);
        self.unify(it, want)?;
        Ok(ret)
    }

    fn use_scheme(&mut self, node: &Exp, level: i32, sch: Ty) -> R<Ty> {
        let (t, hs) = self.instantiate_ev(node.meta.span, level, sch)?;
        if !hs.is_empty() {
            self.set_dict(node.meta.id, DictInfo::Use(hs.clone()));
        }
        self.record_group_ref(node.meta.id, node.meta.span, sch, &hs)?;
        Ok(t)
    }

    fn elab_exp_inner(&mut self, env: &Env, level: i32, eff: Ty, node: &Exp) -> R<Ty> {
        match &node.kind {
            ExpKind::Bool(_) => Ok(self.t_boolean),
            ExpKind::Text(_) => Ok(self.t_string),
            ExpKind::Number(n) => self.number_ty(node.meta.span, level, n),
            ExpKind::Ident(li) => {
                let name = self.qualify(&show_long_id(li));
                match env.values.get(name.as_str()).copied() {
                    Some(sch) => {
                        let o = self.intern(&name);
                        self.check_value_visible(o)?;
                        if let Some(g) = env.tops.get(name.as_str()) {
                            self.set_resolved(node.meta.id, Resolved::Var(g.clone()));
                        }
                        self.use_scheme(node, level, sch)
                    }
                    None => {
                        let scoped = match self.d.current_module.clone() {
                            Some(m) => {
                                let o = self.intern(&name);
                                self.d.module_val_synonyms.get(&(m, o)).copied()
                            }
                            None => None,
                        };
                        let found = scoped.and_then(|q| env.values.get(&*self.name_of(q)).copied());
                        match found {
                            Some(sch) => {
                                let q = scoped.expect("同義語");
                                match env.tops.get(&*self.name_of(q)) {
                                    Some(g) => {
                                        self.set_resolved(node.meta.id, Resolved::Var(g.clone()))
                                    }
                                    None => {
                                        return bug(format!(
                                            "同義語の先が実体を持ちません: {name}"
                                        ));
                                    }
                                }
                                self.use_scheme(node, level, sch)
                            }
                            None => match li.0.last() {
                                Some(last) if !last.is_empty() => {
                                    if upper_initial(last) {
                                        let last = last.clone();
                                        self.elab_construct(env, level, node, &last, None, &[])
                                    } else {
                                        self.unbound_value(&show_long_id(li), scoped)
                                    }
                                }
                                _ => self.unbound_value(&show_long_id(li), scoped),
                            },
                        }
                    }
                }
            }
            ExpKind::Hole => Ok(self.new_var(level)),
            ExpKind::Lambda(l) => {
                let param_tys: Vec<Ty> = l.params.iter().map(|_| self.new_var(level)).collect();
                let mut seen = Vec::new();
                let mut env2 = env.clone();
                for (p, t) in l.params.iter().zip(param_tys.iter()) {
                    env2 = self.elab_pat(&env2, level, &mut seen, *t, p)?;
                }
                let body_eff = self.new_row_var(level);
                let tr = self.elab_exp(&env2, level, body_eff, &l.body)?;
                for (p, t) in l.params.iter().zip(param_tys.iter()) {
                    if !irrefutable_pat(p) {
                        self.ex_queue(vec![(p.clone(), false)], *t);
                    }
                }
                let row = self.closed_item_row(&param_tys);
                let pr = self.record(row);
                Ok(self.arrow(pr, tr, body_eff))
            }
            ExpKind::Apply(f, arg) => {
                let tf = self.elab_exp(env, level, eff, f)?;
                let tf = self.open_closed_arrow(level, tf)?;
                let tr = self.new_var(level);
                let pvar = self.new_var(level);
                let want = self.arrow(pvar, tr, eff);
                let r = self.unify(tf, want);
                if let Some(msg) = type_msg(&r) {
                    let msg = msg.to_string();
                    let pub_pure = self.is_pub_pure_row(eff);
                    let rtf = self.repr(tf);
                    let callee_arrow = matches!(self.node_ref(rtf), TyNode::Arrow(..));
                    if pub_pure && callee_arrow && row_failure(&msg) && !escape_failure(&msg) {
                        return terr(format!(
                            "pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: {msg})"
                        ));
                    } else if callee_arrow && row_failure(&msg) && self.repr_is_empty(eff) {
                        return terr(format!(
                            "{msg}(この位置の行は空 = 純粋です — 注釈の @ {{}} か、高階の引数の行が @ {{}} だからです(入れ子の矢印の @ 省略も @ {{}} と読みます)。行を通すなら行変数を型パラメータに取ってください。§9)"
                        ));
                    } else {
                        return terr(msg);
                    }
                }
                r?;
                self.check_call_arity("呼び出す関数は", arg, pvar)?;
                self.elab_check(None, env, level, eff, arg, pvar)?;
                Ok(tr)
            }
            ExpKind::BinOp(l, op, r) => {
                let tl = self.elab_exp(env, level, eff, l)?;
                let tr = self.elab_exp(env, level, eff, r)?;
                match bin_op_sem(*op) {
                    None => {
                        let b = self.t_boolean;
                        self.unify(tl, b)?;
                        self.unify(tr, b)?;
                        Ok(b)
                    }
                    Some((cls, m)) => self.method_use(node, level, cls, m, &[tl, tr], eff),
                }
            }
            ExpKind::Not(e) => {
                let t = self.elab_exp(env, level, eff, e)?;
                let b = self.t_boolean;
                self.unify(t, b)?;
                Ok(b)
            }
            ExpKind::Neg(e) => {
                let te = self.elab_exp(env, level, eff, e)?;
                self.method_use(node, level, "Neg", "neg", &[te], eff)
            }
            ExpKind::RecordEmpty => Ok(self.t_unit),
            ExpKind::RecordExtend(rest, l, v) => {
                let tv = self.elab_exp(env, level, eff, v)?;
                let rest_row = self.new_row_var(level);
                let trest = self.elab_exp(env, level, eff, rest)?;
                let rr = self.record(rest_row);
                self.unify(trest, rr)?;
                let lo = self.intern(l);
                let row = self.extend(lo, tv, rest_row);
                Ok(self.record(row))
            }
            ExpKind::RecordSelection(r, l) => {
                let tf = self.new_var(level);
                let rest = self.new_row_var(level);
                let lo = self.intern(l);
                let row = self.extend(lo, tf, rest);
                let want = self.record(row);
                let tr = self.elab_exp(env, level, eff, r)?;
                self.unify(tr, want)?;
                Ok(tf)
            }
            ExpKind::RecordRestriction(r, l) => {
                let tf = self.new_var(level);
                let rest = self.new_row_var(level);
                let lo = self.intern(l);
                let row = self.extend(lo, tf, rest);
                let want = self.record(row);
                let tr = self.elab_exp(env, level, eff, r)?;
                self.unify(tr, want)?;
                Ok(self.record(rest))
            }
            ExpKind::RecordUpdate(r, l, v) => {
                let told = self.new_var(level);
                let rest = self.new_row_var(level);
                let lo = self.intern(l);
                let row = self.extend(lo, told, rest);
                let want = self.record(row);
                let tr = self.elab_exp(env, level, eff, r)?;
                self.unify(tr, want)?;
                let tv = self.elab_exp(env, level, eff, v)?;
                let lo = self.intern(l);
                let row = self.extend(lo, tv, rest);
                Ok(self.record(row))
            }
            ExpKind::Variant(s, v) => {
                let rv = self.new_row_var(level);
                let tv = self.elab_exp(env, level, eff, v)?;
                let l = self.intern(s);
                let row = self.extend(l, tv, rv);
                Ok(self.variant(row))
            }
            ExpKind::Seq(es) => {
                let Some((last, init)) = es.split_last() else {
                    return Ok(self.t_unit);
                };
                for s in init {
                    self.elab_exp(env, level, eff, s)?;
                }
                self.elab_exp(env, level, eff, last)
            }
            ExpKind::Let(b, body) => {
                let env2 = self.elab_binding(None, env, level, eff, b)?;
                self.elab_exp(&env2, level, eff, body)
            }
            ExpKind::LetRec(bs, body) => {
                let env2 = self.elab_rec_bindings(false, &|_, _| Ok(None), env, level, eff, bs)?;
                self.elab_exp(&env2, level, eff, body)
            }
            ExpKind::Match(scrut, clauses) => {
                let tscrut = self.elab_exp(env, level, eff, scrut)?;
                let tres = self.new_var(level);
                for c in clauses {
                    let mut seen = Vec::new();
                    let env2 = self.elab_pat(env, level, &mut seen, tscrut, &c.pat)?;
                    if let Some(g) = &c.guard {
                        let tg = self.elab_exp(&env2, level, eff, g)?;
                        let b = self.t_boolean;
                        self.unify(tg, b)?;
                    }
                    let tb = self.elab_exp(&env2, level, eff, &c.body)?;
                    self.unify(tb, tres)?;
                }
                let rows = clauses
                    .iter()
                    .map(|c| (c.pat.clone(), c.guard.is_some()))
                    .collect();
                self.ex_queue(rows, tscrut);
                Ok(tres)
            }
            ExpKind::Construct(li, args) => {
                let last = li.0.last().cloned().unwrap_or_default();
                let args: Vec<(Option<String>, &Exp)> =
                    args.iter().map(|a| (a.label.clone(), &a.exp)).collect();
                self.elab_construct(env, level, node, &last, Some(eff), &args)
            }
            ExpKind::Perform(li, arg) => {
                let (eff_name, op, scheme) = self.resolve_perform(eff, li)?;
                let full = format!("{}.{}", self.name_of(eff_name), self.name_of(op));
                let fo = self.intern(&full);
                self.set_resolved(node.meta.id, Resolved::Op(fo));
                let inst = self.instantiate(level, scheme)?;
                let inst = self.repr(inst);
                let (args_row, op_ret) = match self.node(inst) {
                    TyNode::Arrow(a, r, _) => (a, r),
                    _ => return bug("操作スキーマが矢印型ではありません"),
                };
                let dop = self.display_of(op);
                self.check_call_arity(&format!("操作 {dop} は"), arg, args_row)?;
                let note = op_row_note(&format!("操作 {dop} の引数の型"));
                self.elab_check(Some(&note), env, level, eff, arg, args_row)?;
                let u = self.t_unit;
                let rv = self.new_row_var(level);
                let want = self.extend(eff_name, u, rv);
                let r = self.unify(eff, want);
                if let Some(msg) = type_msg(&r) {
                    let msg = msg.to_string();
                    let de = self.display_of(eff_name);
                    if self.is_pub_pure_row(eff) {
                        return terr(
                            "pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください)",
                        );
                    } else if self.repr_is_empty(eff) {
                        return terr(format!(
                            "エフェクト {de} をここでは実行できません({msg})(この位置の行は空 = 純粋です — 注釈の @ {{}} か、高階の引数の行が @ {{}} だからです(入れ子の矢印の @ 省略も @ {{}} と読みます)。行を通すなら行変数を型パラメータに取ってください。§9)"
                        ));
                    } else {
                        return terr(format!("エフェクト {de} をここでは実行できません({msg})"));
                    }
                }
                r?;
                if self.is_never(op_ret) {
                    Ok(self.new_var(level))
                } else {
                    Ok(op_ret)
                }
            }
            ExpKind::Handle(body, clauses) => {
                self.elab_handle(env, level, eff, node, clauses, body)
            }
            ExpKind::Resume(arg) => match env.resume_ty {
                None => match env.no_resume {
                    Some(NoResume::NeverClause(op)) => terr(format!(
                        "再開しない操作 {} の節には resume を書けません(操作の返り値の型が Never です)",
                        self.display_of(op)
                    )),
                    Some(NoResume::Guard) => terr(
                        "操作節のガードには resume を書けません(ガードは節が選ばれる前に評価されるので、継続を再開できません)",
                    ),
                    Some(NoResume::ReturnClause) => {
                        terr("return 節には resume を書けません(再開する継続がありません)")
                    }
                    Some(NoResume::CancelClause) => {
                        terr("cancel 節には resume を書けません(再開する継続がありません)")
                    }
                    None => terr("resume は操作節の中でのみ使えます"),
                },
                Some((op_ret, tres)) => {
                    match arg {
                        Some(e) => {
                            let note = op_row_note("操作の返り値の型");
                            self.elab_check(Some(&note), env, level, eff, e, op_ret)?;
                        }
                        None => {
                            let u = self.t_unit;
                            self.unify(u, op_ret)?;
                        }
                    }
                    Ok(tres)
                }
            },
            ExpKind::Run(h, body) => {
                let result = self.new_var(level);
                let ks = self.k_star;
                let heap = self.new_rigid(ks, Vec::new(), level + 1);
                let mut env2 = env.clone();
                env2.types = env2.types.add(Rc::from(h.as_str()), heap);
                let eh = self.eff_heap;
                let e2 = self.extend(eh, heap, eff);
                let t = self.elab_exp(&env2, level + 1, e2, body)?;
                self.unify(result, t)?;
                Ok(result)
            }
        }
    }

    fn elab_construct(
        &mut self,
        env: &Env,
        level: i32,
        node: &Exp,
        cname: &str,
        eff: Option<Ty>,
        args: &[(Option<String>, &Exp)],
    ) -> R<Ty> {
        let co = self.intern(cname);
        let ctor = self.resolve_ctor(co);
        let Some(&dname) = self.d.ctor_owner.get(&ctor) else {
            return terr(format!("未知のコンストラクタ: {cname}"));
        };
        self.check_ctor_visible(ctor, dname)?;
        let dd = self.d.datas[&dname].clone();
        if dd.dd_opaque {
            return terr(format!(
                "newtype {} の表現は ??? で隠されています",
                self.display_of(dname)
            ));
        }
        let Some(ct) = dd.dd_ctors.iter().find(|c| c.ct_name == ctor).cloned() else {
            return bug("List.find: コンストラクタ");
        };
        let nfields = ct.ct_fields.len();
        let mut assigned = vec![false; nfields];
        let mut arg_to_field = Vec::new();
        for (label, _) in args {
            match label {
                None => {
                    let Some(i) = (0..nfields).find(|i| !assigned[*i]) else {
                        return terr(format!("コンストラクタ {cname} の引数が多すぎます"));
                    };
                    assigned[i] = true;
                    arg_to_field.push(i);
                }
                Some(l) => {
                    let lo = self.intern(l);
                    let Some(i) = ct.ct_fields.iter().position(|f| f.fi_label == Some(lo)) else {
                        return terr(format!(
                            "コンストラクタ {cname} にフィールド {l} はありません"
                        ));
                    };
                    if assigned[i] {
                        return terr(format!("フィールド {l} が二重に指定されています"));
                    }
                    assigned[i] = true;
                    arg_to_field.push(i);
                }
            }
        }
        if assigned.iter().any(|a| !a) {
            return terr(format!(
                "コンストラクタ {cname} の引数が不足しています(式では全フィールド必須)"
            ));
        }
        self.set_resolved(
            node.meta.id,
            Resolved::Ctor(dname, ctor, arg_to_field.clone()),
        );
        let mut subst = Vec::new();
        for i in &dd.dd_params {
            let v = self.new_class_var(i.vkind, i.vcls.clone(), level);
            subst.push((i.vid, v));
        }
        for (ai, (_, e)) in args.iter().enumerate() {
            let f = &ct.ct_fields[arg_to_field[ai]];
            let ety = match eff {
                Some(eff) => self.elab_exp(env, level, eff, e)?,
                None => return bug("elab_construct: 引数つきなのに eff がない"),
            };
            let fty = self.subst_params(level, &subst, f.fi_ty)?;
            let r = self.unify(ety, fty);
            if let Some(msg) = type_msg(&r)
                && row_failure(msg)
                && self.has_closed_arrow(f.fi_ty)?
            {
                return terr(format!(
                    "{msg}(コンストラクタ {cname} のフィールドの行です。newtype のフィールドの矢印は書いたとおりに読み、@ の省略は @ {{}} — 純粋 — です。行を通すなら行変数を型パラメータに取ってください。§9)"
                ));
            }
            r?;
        }
        Ok(self.con(dname, subst.iter().map(|(_, t)| *t).collect()))
    }

    /// 期待型に照らして式を検査する(Diktor の `elab_check`)。
    pub(crate) fn elab_check(
        &mut self,
        row_note: Option<&str>,
        env: &Env,
        level: i32,
        eff: Ty,
        node: &Exp,
        expected: Ty,
    ) -> R<()> {
        guard()?;
        let r = self.elab_check_inner(row_note, env, level, eff, node, expected);
        at(node.meta.span, r)
    }

    fn elab_check_fallback(
        &mut self,
        row_note: Option<&str>,
        env: &Env,
        level: i32,
        eff: Ty,
        node: &Exp,
        expected: Ty,
    ) -> R<()> {
        let t0 = self.elab_exp(env, level, eff, node)?;
        let ty = self.subsume_closed(level, t0, expected)?;
        let re = self.repr(expected);
        let rt = self.repr(ty);
        if let (Some(note), TyNode::Arrow(p1, r1, e1), TyNode::Arrow(p2, r2, e2)) =
            (row_note, self.node(rt), self.node(re))
            && self.repr_is_empty(e2)
        {
            self.check_arity(ty, expected, p1, p2)?;
            self.unify(p1, p2)?;
            self.unify(r1, r2)?;
            return match self.unify(e1, e2) {
                Err(Fail::Type(msg)) => terr(format!("{msg}{note}")),
                r => r,
            };
        }
        self.unify(ty, expected)
    }

    fn elab_check_inner(
        &mut self,
        row_note: Option<&str>,
        env: &Env,
        level: i32,
        eff: Ty,
        node: &Exp,
        expected: Ty,
    ) -> R<()> {
        let re = self.repr(expected);
        match (&node.kind, self.node(re)) {
            (ExpKind::Lambda(l), TyNode::Arrow(pexp, rexp, eexp)) => {
                let rp = self.repr(pexp);
                match self.node(rp) {
                    TyNode::Record(prow) => {
                        let (fields, tail) = self.row_fields(prow);
                        if self.repr_is_empty(tail)
                            && fields.len() == l.params.len()
                            && fields.iter().all(|(lb, _)| *lb == self.l_item)
                        {
                            let mut seen = Vec::new();
                            let mut env2 = env.clone();
                            for (p, (_, t)) in l.params.iter().zip(fields.iter()) {
                                env2 = self.elab_pat(&env2, level, &mut seen, *t, p)?;
                            }
                            self.set_ty(node.meta.id, expected);
                            self.elab_check(row_note, &env2, level, eexp, &l.body, rexp)?;
                            for (p, (_, t)) in l.params.iter().zip(fields.iter()) {
                                if !irrefutable_pat(p) {
                                    self.ex_queue(vec![(p.clone(), false)], *t);
                                }
                            }
                            Ok(())
                        } else {
                            self.elab_check_fallback(row_note, env, level, eff, node, expected)
                        }
                    }
                    _ => self.elab_check_fallback(row_note, env, level, eff, node, expected),
                }
            }
            (ExpKind::RecordExtend(rest, l, v), TyNode::Record(row)) => {
                let lo = self.intern(l);
                match self.rewrite_row(row, lo) {
                    Ok((fty, rest_row)) => {
                        self.set_ty(node.meta.id, expected);
                        self.elab_check(row_note, env, level, eff, v, fty)?;
                        let rr = self.record(rest_row);
                        self.elab_check(row_note, env, level, eff, rest, rr)
                    }
                    Err(Fail::Type(_)) => {
                        self.elab_check_fallback(row_note, env, level, eff, node, expected)
                    }
                    Err(e) => Err(e),
                }
            }
            _ => self.elab_check_fallback(row_note, env, level, eff, node, expected),
        }
    }

    fn resolve_perform(&mut self, eff: Ty, li: &LongId) -> R<(Oid, Oid, Ty)> {
        match li.0.as_slice() {
            [ename, op] => {
                let eo = self.intern(ename);
                let e = self.resolve_unit(eo);
                match self.find_effect(e) {
                    None => terr(format!("未知のエフェクト: {ename}")),
                    Some(info) => {
                        let opo = self.intern(op);
                        match info.ef_ops.iter().find(|(o, _)| *o == opo) {
                            Some((_, scheme)) => Ok((e, opo, *scheme)),
                            None => terr(format!("エフェクト {ename} に操作 {op} はありません")),
                        }
                    }
                }
            }
            [op] => {
                let opo = self.intern(op);
                let many = self.op_candidates(opo);
                let scheme_of = |tc: &Tc, e: Oid| -> R<Ty> {
                    match tc
                        .find_effect(e)
                        .and_then(|i| i.ef_ops.iter().find(|(o, _)| *o == opo).map(|(_, t)| *t))
                    {
                        Some(t) => Ok(t),
                        None => bug("List.assoc: 操作"),
                    }
                };
                match many.as_slice() {
                    [] => terr(format!("未知の操作: {op}")),
                    [e] => Ok((*e, opo, scheme_of(self, *e)?)),
                    _ => {
                        let labels: Vec<Oid> =
                            self.row_fields(eff).0.into_iter().map(|(l, _)| l).collect();
                        let in_row: Vec<Oid> = many
                            .iter()
                            .copied()
                            .filter(|e| labels.contains(e))
                            .collect();
                        match in_row.as_slice() {
                            [e] => Ok((*e, opo, scheme_of(self, *e)?)),
                            [] => {
                                let names: Vec<String> =
                                    many.iter().map(|e| self.display_of(*e)).collect();
                                terr(format!(
                                    "操作 {op} は複数のエフェクト({})に属します。{}.{op} のように修飾してください",
                                    names.join(", "),
                                    self.display_of(many[0])
                                ))
                            }
                            _ => {
                                let names: Vec<String> =
                                    in_row.iter().map(|e| self.display_of(*e)).collect();
                                let quals: Vec<String> = in_row
                                    .iter()
                                    .map(|e| format!("{}.{op}", self.display_of(*e)))
                                    .collect();
                                terr(format!(
                                    "操作 {op} は、この位置の行に現れる複数のエフェクト({})に属します。{} のどれかに修飾してください",
                                    names.join(", "),
                                    quals.join("、")
                                ))
                            }
                        }
                    }
                }
            }
            _ => terr(format!("不正な操作名です: {}", show_long_id(li))),
        }
    }

    fn check_resume_static(&mut self, in_lambda: bool, node: &Exp) -> R<()> {
        guard()?;
        let r = self.check_resume_inner(in_lambda, node);
        at(node.meta.span, r)
    }

    fn check_resume_binding(&mut self, in_lambda: bool, b: &LetBinding) -> R<()> {
        match b.params {
            Some(_) => self.check_resume_static(true, &b.body),
            None => self.check_resume_static(in_lambda, &b.body),
        }
    }

    fn check_resume_inner(&mut self, in_lambda: bool, node: &Exp) -> R<()> {
        let go = |tc: &mut Tc, e: &Exp| tc.check_resume_static(in_lambda, e);
        match &node.kind {
            ExpKind::Resume(arg) => {
                if in_lambda {
                    terr(
                        "resume は second-class です(クロージャに閉じ込める・節の外へ持ち出すことはできません)",
                    )
                } else {
                    match arg {
                        Some(a) => go(self, a),
                        None => Ok(()),
                    }
                }
            }
            ExpKind::Lambda(l) => self.check_resume_static(true, &l.body),
            ExpKind::Handle(b, _) => go(self, b),
            ExpKind::Bool(_)
            | ExpKind::Number(_)
            | ExpKind::Text(_)
            | ExpKind::Ident(_)
            | ExpKind::Hole
            | ExpKind::RecordEmpty => Ok(()),
            ExpKind::Apply(f, a) => {
                go(self, f)?;
                go(self, a)
            }
            ExpKind::Construct(_, args) => {
                for a in args {
                    go(self, &a.exp)?;
                }
                Ok(())
            }
            ExpKind::Variant(_, v) | ExpKind::Not(v) | ExpKind::Neg(v) => go(self, v),
            ExpKind::BinOp(l, _, r) => {
                go(self, l)?;
                go(self, r)
            }
            ExpKind::Let(b, rest) => {
                self.check_resume_binding(in_lambda, b)?;
                go(self, rest)
            }
            ExpKind::LetRec(bs, rest) => {
                for b in bs {
                    self.check_resume_binding(in_lambda, b)?;
                }
                go(self, rest)
            }
            ExpKind::Seq(es) => {
                for e in es {
                    go(self, e)?;
                }
                Ok(())
            }
            ExpKind::Match(scrut, cs) => {
                go(self, scrut)?;
                for c in cs {
                    if let Some(g) = &c.guard {
                        go(self, g)?;
                    }
                    go(self, &c.body)?;
                }
                Ok(())
            }
            ExpKind::RecordExtend(r, _, v) | ExpKind::RecordUpdate(r, _, v) => {
                go(self, r)?;
                go(self, v)
            }
            ExpKind::RecordRestriction(r, _) | ExpKind::RecordSelection(r, _) => go(self, r),
            ExpKind::Perform(_, a) => go(self, a),
            ExpKind::Run(_, b) => go(self, b),
        }
    }

    pub(crate) fn is_never(&mut self, t: Ty) -> bool {
        let n = self.intern("Never");
        self.is_con0(t, n)
    }

    fn elab_handle(
        &mut self,
        env: &Env,
        level: i32,
        eff: Ty,
        _node: &Exp,
        clauses: &[Clause],
        body: &Exp,
    ) -> R<Ty> {
        enum Cl<'a> {
            Op(Oid, Option<Oid>, &'a [CtorArgPat], &'a Clause),
            Return(&'a Pat, &'a Clause),
            Cancel(&'a Clause),
        }
        let mut classified = Vec::new();
        for c in clauses {
            let mut classify = || -> R<Cl> {
                match &c.pat.kind {
                    PatKind::Var(v) if v == "cancel" => Ok(Cl::Cancel(c)),
                    PatKind::Ctor(LongId(comps), args) => {
                        let rev: Vec<&String> = comps.iter().rev().collect();
                        match rev.as_slice() {
                            [kw, _, ..] if kw.as_str() == "return" || kw.as_str() == "cancel" => {
                                terr(format!(
                                    "{kw} 節は修飾できません({kw} は操作名ではなく handle の節の名前です。修飾を外してください)"
                                ))
                            }
                            [kw, ..] if kw.as_str() == "cancel" => {
                                if !args.is_empty() {
                                    noimpl("cancel(reason)(v0 は case cancel のみ)")
                                } else {
                                    Ok(Cl::Cancel(c))
                                }
                            }
                            [kw, ..] if kw.as_str() == "return" => match args.as_slice() {
                                [CtorArgPat { label: None, pat }] => Ok(Cl::Return(pat, c)),
                                _ => terr("return 節は case return(x) の形で書いてください"),
                            },
                            [op, quals @ ..] if lower_initial(op) => {
                                let q = if quals.is_empty() {
                                    None
                                } else {
                                    let qs: Vec<&str> =
                                        quals.iter().rev().map(|s| s.as_str()).collect();
                                    let qo = self.intern(&qs.join("."));
                                    Some(self.resolve_unit(qo))
                                };
                                let opo = self.intern(op);
                                Ok(Cl::Op(opo, q, args, c))
                            }
                            _ => terr(format!(
                                "handle の節は操作名 / return / cancel で始めてください: {}",
                                comps.join(".")
                            )),
                        }
                    }
                    _ => terr("handle の節は操作名 / return / cancel で始めてください"),
                }
            };
            classified.push(at(c.pat.meta.span, classify())?);
        }
        let ops: Vec<(Oid, Option<Oid>, &[CtorArgPat], &Clause)> = classified
            .iter()
            .filter_map(|c| match c {
                Cl::Op(o, q, a, c) => Some((*o, *q, *a, *c)),
                _ => None,
            })
            .collect();
        let rets: Vec<(&Pat, &Clause)> = classified
            .iter()
            .filter_map(|c| match c {
                Cl::Return(p, c) => Some((*p, *c)),
                _ => None,
            })
            .collect();
        let cancels: Vec<&Clause> = classified
            .iter()
            .filter_map(|c| match c {
                Cl::Cancel(c) => Some(*c),
                _ => None,
            })
            .collect();
        if rets.len() >= 2 {
            return Err(Fail::TypeAt(
                rets[1].1.pat.meta.span,
                "return 節は1つまでです".into(),
            ));
        }
        if cancels.len() >= 2 {
            return Err(Fail::TypeAt(
                cancels[1].pat.meta.span,
                "cancel 節は1つまでです".into(),
            ));
        }
        if ops.is_empty() {
            return terr("handle には少なくとも1つの操作節が必要です");
        }
        let mut quals: Vec<Oid> = ops.iter().filter_map(|(_, q, _, _)| *q).collect();
        let op_names: Vec<Oid> = ops.iter().map(|(o, _, _, _)| *o).collect();
        let runtime_provided = |tc: &Tc, e: Oid| {
            RUNTIME_EFFECTS.contains(&&*tc.name_of(e)) && tc.prelude_owned("effect", e)
        };
        let runtime_msg = |tc: &Tc, e: Oid| {
            match &*tc.name_of(e) {
            "Console" => "エフェクト Console はランタイムが提供するため、ユーザはハンドルできません(仕様 sample.kel:466, :538)。出力先を変えたいときは Print をハンドルしてください(プレリュードの with_stdout が Print を Console へ翻訳します)".to_string(),
            n => format!("エフェクト {n} はランタイムが提供するため、ユーザはハンドルできません"),
        }
        };
        quals.sort();
        quals.dedup();
        let target = match quals.as_slice() {
            [e] => {
                if self.find_effect(*e).is_none() {
                    return terr(format!("未知のエフェクト: {}", self.display_of(*e)));
                } else if runtime_provided(self, *e) {
                    return terr(runtime_msg(self, *e));
                }
                *e
            }
            [_, _, ..] => return terr("handle の節の修飾エフェクトが一致しません"),
            [] => {
                let declares = |tc: &Tc, e: Oid, op: Oid| {
                    tc.find_effect(e)
                        .is_some_and(|i| i.ef_ops.iter().any(|(o, _)| *o == op))
                };
                let mut candidates = Vec::new();
                for op in &op_names {
                    candidates.extend(self.op_candidates(*op));
                }
                candidates.sort();
                candidates.dedup();
                let all_effects: Vec<Oid> = candidates
                    .iter()
                    .copied()
                    .filter(|e| !runtime_provided(self, *e))
                    .collect();
                if all_effects.is_empty() && !candidates.is_empty() {
                    return terr(runtime_msg(self, candidates[0]));
                }
                let holds_all: Vec<Oid> = all_effects
                    .iter()
                    .copied()
                    .filter(|e| op_names.iter().all(|op| declares(self, *e, *op)))
                    .collect();
                let covered: Vec<Oid> = holds_all
                    .iter()
                    .copied()
                    .filter(|e| {
                        self.find_effect(*e)
                            .is_some_and(|i| i.ef_ops.iter().all(|(op, _)| op_names.contains(op)))
                    })
                    .collect();
                match covered.as_slice() {
                    [e] => *e,
                    [] => match holds_all.first() {
                        Some(e) => {
                            let info = self.find_effect(*e).expect("エフェクト");
                            let missing: Vec<String> = info
                                .ef_ops
                                .iter()
                                .filter(|(op, _)| !op_names.contains(op))
                                .map(|(op, _)| self.display_of(*op))
                                .collect();
                            return terr(format!(
                                "ハンドラが操作を網羅していません: {} の {} が漏れています",
                                self.display_of(*e),
                                missing.join(", ")
                            ));
                        }
                        None => {
                            let names: Vec<String> =
                                op_names.iter().map(|o| self.display_of(*o)).collect();
                            return terr(format!(
                                "この操作の組を宣言するエフェクトがありません: {}",
                                names.join(", ")
                            ));
                        }
                    },
                    es => {
                        let names: Vec<String> = es.iter().map(|e| self.display_of(*e)).collect();
                        return terr(format!(
                            "handle の対象エフェクトが曖昧です({})。{}.{} のように修飾してください",
                            names.join(", "),
                            self.display_of(es[0]),
                            self.display_of(op_names[0])
                        ));
                    }
                }
            }
        };
        let target_info = self.find_effect(target).expect("対象のエフェクト");
        for (op, _, _, c) in &ops {
            if !target_info.ef_ops.iter().any(|(o, _)| o == op) {
                return Err(Fail::TypeAt(
                    c.pat.meta.span,
                    format!(
                        "操作 {} はエフェクト {} に属しません",
                        self.display_of(*op),
                        self.display_of(target)
                    ),
                ));
            }
        }
        for (op, _) in &target_info.ef_ops {
            if !op_names.contains(op) {
                return terr(format!(
                    "ハンドラが操作を網羅していません: {} の {} が漏れています",
                    self.display_of(target),
                    self.display_of(*op)
                ));
            }
        }
        // 操作節の引数を精緻化し、総和的な節かを調べる。
        struct OpClause<'a> {
            op: Oid,
            args: &'a [CtorArgPat],
            c: &'a Clause,
            env2: Env,
            op_ret: Ty,
            total: bool,
        }
        let mut op_clauses: Vec<OpClause> = Vec::new();
        for (op, _, args, c) in &ops {
            let scheme = target_info
                .ef_ops
                .iter()
                .find(|(o, _)| o == op)
                .expect("操作")
                .1;
            let inst = self.instantiate(level, scheme)?;
            let inst = self.repr(inst);
            let (args_row, op_ret) = match self.node(inst) {
                TyNode::Arrow(a, r, _) => (a, r),
                _ => return bug("操作スキーマが矢印型ではありません"),
            };
            let full = format!("{}.{}", self.name_of(target), self.name_of(*op));
            let fo = self.intern(&full);
            let res = if self.is_never(op_ret) {
                Resolved::NeverOp(fo)
            } else {
                Resolved::Op(fo)
            };
            self.set_resolved(c.meta.id, res);
            let ar = self.repr(args_row);
            let param_tys: Vec<Ty> = match self.node(ar) {
                TyNode::Record(row) => self.row_fields(row).0.into_iter().map(|(_, t)| t).collect(),
                _ => Vec::new(),
            };
            let chk = (|| -> R<()> {
                if args.len() != param_tys.len() {
                    return terr(format!(
                        "操作 {} は {} 引数です(節には {} 個書かれています)",
                        self.display_of(*op),
                        param_tys.len(),
                        args.len()
                    ));
                }
                if args.iter().any(|a| a.label.is_some()) {
                    return terr("操作節の引数にラベルは書けません");
                }
                Ok(())
            })();
            at(c.pat.meta.span, chk)?;
            let mut seen = Vec::new();
            let mut env2 = env.clone();
            for (a, t) in args.iter().zip(param_tys.iter()) {
                env2 = self.elab_pat(&env2, level, &mut seen, *t, &a.pat)?;
            }
            let total = c.guard.is_none() && {
                let pats: Vec<&Pat> = args.iter().map(|a| &a.pat).collect();
                self.ex_missing_pats(&pats, &param_tys)?
            };
            op_clauses.push(OpClause {
                op: *op,
                args,
                c,
                env2,
                op_ret,
                total,
            });
        }
        for (op, _) in &target_info.ef_ops {
            let mine: Vec<&OpClause> = op_clauses.iter().filter(|oc| oc.op == *op).collect();
            if !mine.iter().any(|oc| oc.total) {
                let arity = mine.first().map_or(0, |oc| oc.args.len());
                let dop = self.display_of(*op);
                return terr(format!(
                    "操作 {dop} には、ガードがなく、どんな引数にも一致する節がありません(どの節にも一致しなかった操作を外側のハンドラへ回す規則はありません)。ガードの無い case {dop}({}) を最後に置いてください",
                    vec!["_"; arity].join(", ")
                ));
            }
        }
        let mut seen_total: Vec<Oid> = Vec::new();
        for oc in &op_clauses {
            if seen_total.contains(&oc.op) {
                let m = format!(
                    "操作 {} の節は到達しません(前の節が既に取りこぼしません)",
                    self.display_of(oc.op)
                );
                self.warn(m);
            }
            if oc.total && !seen_total.contains(&oc.op) {
                seen_total.push(oc.op);
            }
        }
        let u = self.t_unit;
        let beff = self.extend(target, u, eff);
        let body_ty = self.elab_exp(env, level, beff, body)?;
        let tres = self.new_var(level);
        match rets.as_slice() {
            [(p, c)] => {
                self.set_resolved(c.meta.id, Resolved::ReturnClause);
                if let Some(g) = &c.guard {
                    return Err(Fail::TypeAt(
                        g.meta.span,
                        "return 節にガードは書けません".into(),
                    ));
                }
                let mut seen = Vec::new();
                let mut env1 = env.clone();
                env1.resume_ty = None;
                env1.no_resume = Some(NoResume::ReturnClause);
                let env2 = self.elab_pat(&env1, level, &mut seen, body_ty, p)?;
                let tb = self.elab_exp(&env2, level, eff, &c.body)?;
                self.unify(tb, tres)?;
                if !irrefutable_pat(p) {
                    self.ex_queue(vec![((*p).clone(), false)], body_ty);
                }
            }
            _ => self.unify(body_ty, tres)?,
        }
        if let [c] = cancels.as_slice() {
            self.set_resolved(c.meta.id, Resolved::CancelClause);
            if let Some(g) = &c.guard {
                return Err(Fail::TypeAt(
                    g.meta.span,
                    "cancel 節にガードは書けません".into(),
                ));
            }
            let mut env2 = env.clone();
            env2.resume_ty = None;
            env2.no_resume = Some(NoResume::CancelClause);
            let tb = self.elab_exp(&env2, level, eff, &c.body)?;
            let u = self.t_unit;
            self.unify(tb, u)?;
        }
        for oc in &op_clauses {
            if let Some(g) = &oc.c.guard {
                let mut eg = oc.env2.clone();
                eg.resume_ty = None;
                eg.no_resume = Some(NoResume::Guard);
                let tg = self.elab_exp(&eg, level, eff, g)?;
                let b = self.t_boolean;
                self.unify(tg, b)?;
            }
            let never = self.is_never(oc.op_ret);
            let mut e2 = oc.env2.clone();
            if never {
                e2.resume_ty = None;
                e2.no_resume = Some(NoResume::NeverClause(oc.op));
            } else {
                e2.resume_ty = Some((oc.op_ret, tres));
                e2.no_resume = None;
            }
            if !never {
                self.check_resume_static(false, &oc.c.body)?;
            }
            let tb = self.elab_exp(&e2, level, eff, &oc.c.body)?;
            self.unify(tb, tres)?;
        }
        Ok(tres)
    }

    /// 組み込みクラスのメソッドの型(Diktor の `method_scheme`)。
    fn method_scheme(&mut self, cls: &str, m: &str) -> R<Ty> {
        let c = self.intern(cls);
        match self.find_class(c) {
            Some(ci) => match ci.ci_methods.iter().find(|(n, _)| n == m) {
                Some((_, t)) => Ok(*t),
                None => bug(format!("組み込みクラス {cls} にメソッド {m} がありません")),
            },
            None => bug(format!("組み込みクラス {cls} が未登録です")),
        }
    }

    /// 型パラメータの制約のクラス(Diktor の `class_names_of`)。
    pub(crate) fn class_names_of(&mut self, tp: &TypeParam) -> R<Vec<Oid>> {
        let mut out = Vec::new();
        for li in &tp.classes {
            let s = show_long_id(li);
            let o = self.intern(&s);
            let c = self.resolve_class(o);
            if self.find_class(c).is_none() {
                return terr(format!("未知のクラス: {s}"));
            }
            if self.reserved_predicate(c) {
                return terr(format!(
                    "{s} は予約されたリテラル述語です(制約には書けません、D8)"
                ));
            }
            out.push(c);
        }
        Ok(out)
    }

    /// 型パラメータの剛定数を作る(Diktor の `make_rigids`)。
    pub(crate) fn make_rigids(
        &mut self,
        kinds: Option<&[Kind]>,
        level: i32,
        tparams: &[TypeParam],
    ) -> R<Vec<(String, Ty, Cell)>> {
        let kinds: Vec<Kind> = match kinds {
            Some(ks) if ks.len() == tparams.len() => ks.to_vec(),
            _ => tparams
                .iter()
                .map(|tp| {
                    if tp.arity > 0 {
                        self.k_arrow(tp.arity)
                    } else {
                        self.new_kind_var()
                    }
                })
                .collect(),
        };
        let mut out = Vec::new();
        for (tp, kind) in tparams.iter().zip(kinds) {
            let classes = self.class_names_of(tp)?;
            let r = self.new_rigid_ref(kind, classes, level);
            if let TVar::Rigid(i) = self.cell(r) {
                let vid = i.vid;
                self.rigid_tparams.insert(vid, tp.name.clone());
            }
            let t = self.tvar(r);
            out.push((tp.name.clone(), t, r));
        }
        Ok(out)
    }

    /// 剛定数を一般化された変数にする(Diktor の `release_rigids`)。
    pub(crate) fn release_rigids(&mut self, rigids: &[(String, Ty, Cell)]) -> R<()> {
        for (_, _, r) in rigids {
            if let TVar::Rigid(i) = self.cell(*r).clone() {
                self.default_kind(i.vkind)?;
                self.set_tv(*r, TVar::Generic(i));
            }
        }
        Ok(())
    }
}

fn check_no_hole(t: &TypeExp) -> R<()> {
    match t.kind {
        TypeExpKind::Hole => terr("_ はこの位置では使えません(インスタンス頭の List[_] 専用)"),
        _ => Ok(()),
    }
}
