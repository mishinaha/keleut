//! 単一化、行の書き換え、一般化、クラス制約、辞書の穴を実装する(Diktor の unify.ml、第8章)。
//!
//! 辞書の穴(Diktor の `unify.ml` の §8.11 では「証拠の穴」)は、`instantiate_ev` が型スキームを具体化するときに、予約述語を除く制約ごとに 1 つ作る。
//! 証拠を作る途中で、型がまだ決まっていない単一化変数か、囲む束縛の辞書パラメータにキーのないスコーレム定数に行き当たったときにも、`evidence_of` が穴を作る。
//! 穴の型が決まった後で、`solve_holes` が証拠(どのインスタンスの辞書を使うかを表す項)を穴に入れる。

use super::fx::{HashMap, HashSet};
use super::{
    Cell, DKey, Evidence, Fail, Hole, HoleId, KView, Kind, Oid, R, TVar, Tc, Ty, TyNode, VarInfo,
    bug, guard, terr,
};
use crate::pos::Span;

/// 失敗が位置のない型エラー([`Fail::Type`])のときだけ、そのメッセージを返す。
/// Diktor の `try ... with Type_error msg` に相当する。
pub(crate) fn type_msg<T>(r: &R<T>) -> Option<&str> {
    match r {
        Err(Fail::Type(m)) => Some(m.as_str()),
        _ => None,
    }
}

/// リージョンの位置にある 2 つの型の分類である(Diktor の `unify_region` の `shape`)。
/// 単一化に失敗したときに、どのメッセージで報告するかをこの分類で決める。
enum Shape {
    /// 両方がスコーレム定数(`TVar::Rigid`)である。
    Rigids,
    /// 少なくとも一方が単一化変数(`TVar::Unbound`)であるか、両方が型変数である(両方がスコーレム定数の場合を除く)。
    Vars,
    /// `Rigids` でも `Vars` でもない。
    /// このとき、2 つの型の少なくとも一方は型変数でない。
    /// 値は、型変数でない型である(両方が型変数でなければ 1 つ目の型)。
    NonRegion(Ty),
}

impl Tc {
    // ---- カインド ----

    fn drop_arrows(&mut self, k: Kind, n: usize) -> R<Kind> {
        let mut k = k;
        for _ in 0..n {
            match self.kview(k) {
                KView::Arrow(_, r) => k = r,
                KView::Var(r) => {
                    let res = self.new_kind_var();
                    let arg = self.new_kind_var();
                    let ar = self.k_arrow2(arg, res);
                    self.set_klink(r, ar);
                    k = res;
                }
                KView::Star | KView::Row => {
                    return terr("型構成子ではない型に型引数が適用されています");
                }
            }
        }
        Ok(k)
    }

    /// セル `c` の型変数の情報を返す(Diktor の `var_info_of`)。
    /// リンク(`TVar::Link`)になっているセルを渡すと、内部エラーになる。
    pub(crate) fn var_info_of(&self, c: Cell) -> R<VarInfo> {
        match self.cell(c) {
            TVar::Unbound(i) | TVar::Generic(i) | TVar::Rigid(i) => Ok(i.clone()),
            TVar::Link(_) => bug("var_info_of: Link"),
        }
    }

    /// 型 `t` のカインドを返す(Diktor の `kind_of`)。
    pub(crate) fn kind_of(&mut self, t: Ty) -> R<Kind> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::RowEmpty | TyNode::RowExtend(..) => Ok(self.k_row),
            TyNode::Var(c) => Ok(self.var_info_of(c)?.vkind),
            TyNode::Con(n, args) => {
                let k = self.con_kind(n, args.len());
                self.drop_arrows(k, args.len())
            }
            TyNode::App(f, _) => {
                let k = self.kind_of(f)?;
                self.drop_arrows(k, 1)
            }
            TyNode::Arrow(..) | TyNode::Record(_) | TyNode::Variant(_) => Ok(self.k_star),
        }
    }

    // ---- occurs check とスコープからの脱出 ----

    fn escape_message(&mut self, v: Cell, i: &VarInfo) -> R<String> {
        let tv = self.tvar(v);
        let name = self.show(tv)?;
        let base = format!("スコープ付きの型 {name} がスコープの外に漏れています");
        Ok(match self.rigid_tparams.get(&i.vid).cloned() {
            None => base,
            Some(n) => format!(
                "{base}({name} は型パラメータ {n} です。{n} のスコープの外の型と共有された、型の決まっていない変数(注釈のない引数の型や、一般化されない束縛の型など)に {n} を入れようとしています)"
            ),
        })
    }

    fn occurs_adjust(&mut self, tv: Cell, lvl: i32, t: Ty) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(v) => {
                if v == tv {
                    return terr("occurs check に失敗しました(無限型が発生します)");
                }
                match self.cell(v).clone() {
                    TVar::Unbound(mut i) => {
                        if i.vlevel > lvl {
                            i.vlevel = lvl;
                            self.set_tv(v, TVar::Unbound(i));
                        }
                    }
                    TVar::Rigid(i) => {
                        if i.vlevel > lvl {
                            let m = self.escape_message(v, &i)?;
                            return terr(m);
                        }
                    }
                    TVar::Generic(_) => return bug("単一化中に Generic 変数が現れました"),
                    TVar::Link(_) => {}
                }
                Ok(())
            }
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    self.occurs_adjust(tv, lvl, *a)?;
                }
                Ok(())
            }
            TyNode::App(f, a) => {
                self.occurs_adjust(tv, lvl, f)?;
                self.occurs_adjust(tv, lvl, a)
            }
            TyNode::Arrow(p, r, e) => {
                self.occurs_adjust(tv, lvl, p)?;
                self.occurs_adjust(tv, lvl, r)?;
                self.occurs_adjust(tv, lvl, e)
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.occurs_adjust(tv, lvl, row),
            TyNode::RowEmpty => Ok(()),
            TyNode::RowExtend(_, f, rest) => {
                self.occurs_adjust(tv, lvl, f)?;
                self.occurs_adjust(tv, lvl, rest)
            }
        }
    }

    // ---- クラス制約 ----

    /// クラス `c` が予約述語(数値リテラルの型に付ける `Integral` と `Fractional`)であれば真を返す。
    pub(crate) fn is_predicate(&self, c: Oid) -> bool {
        c == self.cls_integral || c == self.cls_fractional
    }

    fn note_class_var(&mut self, v: Cell) {
        self.class_seq += 1;
        self.class_vars.push((self.class_seq, v));
    }

    /// 制約の付いた型変数の一覧 `class_vars` に最後に振った通し番号を返す(Diktor の `class_mark`)。
    /// この後で一覧に追加する項目は、これより大きい番号を持つ。
    pub(crate) fn class_mark(&self) -> i64 {
        self.class_seq
    }

    /// リテラルが制約を付けた型変数について、変数、制約のクラス、リテラルの位置を記録する(Diktor の `note_literal`)。
    /// 数値の既定化に失敗したときに、報告する位置を探すのに使う。
    /// `t` が型変数でなければ何もしない。
    pub(crate) fn note_literal(&mut self, loc: Span, t: Ty, c: Oid) {
        if let Some(r) = self.var_cell(t) {
            self.literal_origins.push((r, c, loc));
        }
    }

    /// 制約 `classes` の付いた新しい型変数を作る(Diktor の `new_class_var`)。
    /// 制約が 1 つでもあれば、作った変数を一覧 `class_vars` に追加する。
    pub(crate) fn new_class_var(&mut self, kind: Kind, classes: Vec<Oid>, level: i32) -> Ty {
        let has = !classes.is_empty();
        let v = self.new_var_k(kind, classes, level);
        if has && let Some(r) = self.var_cell(v) {
            self.note_class_var(r);
        }
        v
    }

    /// 型 `ty` の中の単一化変数 `v` を `t` に置き換えた型を返す(Diktor の `subst_unbound`)。
    /// 診断に表示する型を作るためだけに使い、セルは書き換えない。
    fn subst_unbound(&mut self, v: Cell, t: Ty, ty: Ty) -> R<Ty> {
        guard()?;
        let ty = self.repr(ty);
        Ok(match self.node(ty) {
            TyNode::Var(r) if r == v => t,
            TyNode::Var(_) => ty,
            TyNode::Con(n, args) => {
                let mut v2 = Vec::with_capacity(args.len());
                for a in args.iter() {
                    v2.push(self.subst_unbound(v, t, *a)?);
                }
                self.con(n, v2)
            }
            TyNode::App(g, a) => {
                let g2 = self.subst_unbound(v, t, g)?;
                let a2 = self.subst_unbound(v, t, a)?;
                self.tapp(g2, a2)
            }
            TyNode::Arrow(p, r, e) => {
                let p2 = self.subst_unbound(v, t, p)?;
                let r2 = self.subst_unbound(v, t, r)?;
                let e2 = self.subst_unbound(v, t, e)?;
                self.arrow(p2, r2, e2)
            }
            TyNode::Record(row) => {
                let r2 = self.subst_unbound(v, t, row)?;
                self.record(r2)
            }
            TyNode::Variant(row) => {
                let r2 = self.subst_unbound(v, t, row)?;
                self.variant(r2)
            }
            TyNode::RowEmpty => self.row_empty,
            TyNode::RowExtend(l, f, rest) => {
                let f2 = self.subst_unbound(v, t, f)?;
                let r2 = self.subst_unbound(v, t, rest)?;
                self.extend(l, f2, r2)
            }
        })
    }

    /// 型 `t` にクラス `c` の制約を付ける(Diktor の `add_class`)。
    ///
    /// `t` が単一化変数なら、変数に制約を記録する。
    /// 型構成子(`TyNode::Con`)なら、インスタンスがあることを確かめ、インスタンスの前提を型引数に再帰的に要求する。
    /// 構造的導出のあるクラスでは、閉じたレコードとヴァリアントの各フィールドの型に同じ制約を要求する。
    /// 制約を満たせないときは型エラーにする。
    ///
    /// `via` は、インスタンスの前提をたどって生じた要求のときだけ値を持つ。
    /// 値は、たどり始めた型とクラスの組(元の要求)で、型エラーのメッセージに添える。
    pub(crate) fn add_class(&mut self, via: Option<(Ty, Oid)>, t: Ty, c: Oid) -> R<()> {
        guard()?;
        let ci = match self.find_class(c) {
            Some(ci) => ci,
            None => return terr(format!("未知のクラス: {}", self.display_of(c))),
        };
        let kt = self.kind_of(t)?;
        if !self.same_kind(ci.ci_param_kind, kt)? {
            let ks = self.show_kind(ci.ci_param_kind)?;
            let ts = self.show(t)?;
            return terr(format!(
                "クラス {} は {} のクラスですが、{} に要求されました",
                self.display_of(c),
                ks,
                ts
            ));
        }
        let rt = self.repr(t);
        match self.node(rt) {
            TyNode::Var(v) => match self.cell(v).clone() {
                TVar::Unbound(mut i) => {
                    if !i.vcls.contains(&c) {
                        let vid = i.vid;
                        i.vcls.insert(0, c);
                        self.set_tv(v, TVar::Unbound(i));
                        self.note_class_var(v);
                        if let Some(via) = via {
                            self.premise_origins.entry((vid, c)).or_insert(via);
                        }
                    }
                    Ok(())
                }
                TVar::Rigid(i) | TVar::Generic(i) => {
                    if !i.vcls.contains(&c) {
                        if self.is_predicate(c) {
                            return terr(
                                "型パラメータに数値リテラルは使えません。0i32 のように接尾辞を付けるか具体型を使ってください",
                            );
                        }
                        let tv = self.tvar(v);
                        let dc = self.display_of(c);
                        return self.class_fail(
                            via,
                            Some(tv),
                            Some(format!("[A: {dc}] のように制約を書いてください")),
                            |s| format!("型パラメータ {s} は {dc} のインスタンスではありません"),
                        );
                    }
                    Ok(())
                }
                TVar::Link(_) => {
                    let dc = self.display_of(c);
                    self.class_fail(via, Some(t), None, |s| {
                        format!("{s} に {dc} 制約を付けられません")
                    })
                }
            },
            TyNode::Con(n, args) => match self.find_instance(c, n) {
                Some(ii) => {
                    let via2 = match via {
                        Some(_) => via,
                        None => Some((t, c)),
                    };
                    for (i, c2) in ii.ii_premises {
                        if i < args.len() {
                            self.add_class(via2, args[i], c2)?;
                        }
                    }
                    Ok(())
                }
                None => {
                    let dn = self.display_of(n);
                    let dc = self.display_of(c);
                    self.class_fail(via, None, None, |_| {
                        format!("{dn} は {dc} のインスタンスではありません")
                    })
                }
            },
            TyNode::Record(row) | TyNode::Variant(row) if ci.ci_derive_structural => {
                let (fields, tail) = self.row_fields(row);
                if self.repr_is_empty(tail) {
                    for (_, f) in fields {
                        self.add_class(via, f, c)?;
                    }
                    Ok(())
                } else {
                    let dc = self.display_of(c);
                    self.class_fail(via, Some(t), None, |s| {
                        format!(
                            "行変数を含む型 {s} に {dc} の構造的導出は適用できません(行が閉じていません)"
                        )
                    })
                }
            }
            TyNode::App(..) => {
                let dc = self.display_of(c);
                self.class_fail(via, Some(t), None, |s| {
                    format!(
                        "制約 {dc} を {s} に付けられません(先頭が型変数の適用には制約を貼れません)"
                    )
                })
            }
            _ => {
                let dc = self.display_of(c);
                self.class_fail(via, Some(rt), None, |s| {
                    format!("{s} は {dc} のインスタンスではありません")
                })
            }
        }
    }

    /// `add_class` の型エラーを作る(Diktor の `add_class` の中の `fail`)。
    ///
    /// `ty` を文字列にして `mk` に渡し、メッセージを作る。
    /// `via` があれば元の要求を、`advice` があれば直し方を、メッセージの後ろに続ける。
    fn class_fail(
        &mut self,
        via: Option<(Ty, Oid)>,
        ty: Option<Ty>,
        advice: Option<String>,
        mk: impl FnOnce(&str) -> String,
    ) -> R<()> {
        let origin = |tc: &Tc, vt: &str, vc: Oid| {
            format!(
                "。{vt} が {} であるための前提として要求されました",
                tc.display_of(vc)
            )
        };
        let (shown, suffix) = match (ty, via) {
            (Some(ty), None) => (self.show(ty)?, String::new()),
            (None, None) => (String::new(), String::new()),
            (Some(ty), Some((vt, vc))) => {
                let (s, o) = self.show_pair(ty, vt)?;
                let suf = origin(self, &o, vc);
                (s, suf)
            }
            (None, Some((vt, vc))) => {
                let s = self.show(vt)?;
                (String::new(), origin(self, &s, vc))
            }
        };
        let adv = match advice {
            None => String::new(),
            Some(a) => format!("。{a}"),
        };
        terr(format!("{}{suffix}{adv}", mk(&shown)))
    }

    // ---- 束縛 ----

    fn bind(&mut self, v: Cell, t: Ty) -> R<()> {
        match self.cell(v).clone() {
            TVar::Unbound(i) => {
                let kt = self.kind_of(t)?;
                if !self.same_kind(i.vkind, kt)? {
                    let tv = self.tvar(v);
                    let a = self.show(tv)?;
                    let k = self.show_kind(i.vkind)?;
                    let b = self.show(t)?;
                    return terr(format!("カインドが一致しません: {a} :: {k} と {b}"));
                }
                self.occurs_adjust(v, i.vlevel, t)?;
                for c in i.vcls.iter() {
                    let via = match self.premise_origins.get(&(i.vid, *c)).cloned() {
                        Some((vt, vc)) => Some((self.subst_unbound(v, t, vt)?, vc)),
                        None => None,
                    };
                    self.add_class(via, t, *c)?;
                }
                self.set_tv(v, TVar::Link(t));
                Ok(())
            }
            _ => bug("束縛できない変数への bind"),
        }
    }

    fn unbound_var(&self, t: Ty) -> Option<Cell> {
        match self.node_ref(t) {
            TyNode::Var(c) if matches!(self.cell(*c), TVar::Unbound(_)) => Some(*c),
            _ => None,
        }
    }

    /// 行 `row` から、最も左にあるラベル `label` のフィールドを取り出し、(フィールドの型, 残りの行)を返す(Diktor の `rewrite_row`)。
    ///
    /// ラベルが見つからないまま行の末尾の単一化変数に達したときは、その変数を、`label` のフィールドと新しい行変数からなる行に束縛する。
    /// 新しい変数は、元の行変数と同じレベルで作る。
    /// 行が閉じているときと、末尾がスコーレム定数の行変数のときは、型エラーにする。
    pub(crate) fn rewrite_row(&mut self, row: Ty, label: Oid) -> R<(Ty, Ty)> {
        let mut prefix: Vec<(Oid, Ty)> = Vec::new();
        let mut cur = row;
        let (f, rest) = loop {
            let r = self.repr(cur);
            match self.node(r) {
                TyNode::RowEmpty => {
                    return terr(format!(
                        "ラベル {} がありません(行は閉じています)",
                        self.display_of(label)
                    ));
                }
                TyNode::RowExtend(l, f, rest) if l == label => break (f, rest),
                TyNode::RowExtend(l, f, rest) => {
                    prefix.push((l, f));
                    cur = rest;
                }
                TyNode::Var(v) => match self.cell(v).clone() {
                    TVar::Unbound(i) if self.same_kind(i.vkind, self.k_row)? => {
                        let f2 = self.new_var(i.vlevel);
                        let rest2 = self.new_row_var(i.vlevel);
                        let ext = self.extend(label, f2, rest2);
                        self.set_tv(v, TVar::Link(ext));
                        break (f2, rest2);
                    }
                    TVar::Rigid(i) if self.same_kind(i.vkind, self.k_row)? => {
                        let tv = self.tvar(v);
                        let s = self.show(tv)?;
                        let dl = self.display_of(label);
                        return terr(format!(
                            "行 {s} は注釈で固定された行変数なので、ラベル {dl} を追加できません(注釈側に {dl} を(必要なら引数つきで)書き足してください)"
                        ));
                    }
                    _ => {
                        let tv = self.tvar(v);
                        let s = self.show(tv)?;
                        return terr(format!("行型ではありません: {s}"));
                    }
                },
                _ => {
                    let s = self.show(r)?;
                    return terr(format!("行型ではありません: {s}"));
                }
            }
        };
        let mut acc = rest;
        for (l, f0) in prefix.into_iter().rev() {
            acc = self.extend(l, f0, acc);
        }
        Ok((f, acc))
    }

    /// 型 `t` が閉じたレコードで、ラベルがすべて `_item` であれば、フィールドの個数を返す(Diktor の `item_arity`)。
    /// 関数の引数の型に対しては、位置引数の個数になる。
    /// それ以外の型には `None` を返す。
    pub(crate) fn item_arity(&mut self, t: Ty) -> Option<usize> {
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Record(row) => {
                let (fields, tail) = self.row_fields(row);
                if self.is_row_empty(tail) && fields.iter().all(|(l, _)| *l == self.l_item) {
                    Some(fields.len())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 型 `a` と `b` を単一化する(Diktor の `unify`)。
    pub(crate) fn unify(&mut self, a: Ty, b: Ty) -> R<()> {
        self.unify_in(false, a, b)
    }

    fn unify_in(&mut self, structural: bool, a: Ty, b: Ty) -> R<()> {
        guard()?;
        let t1 = self.repr(a);
        let t2 = self.repr(b);
        let same_var = matches!((self.node_ref(t1), self.node_ref(t2)), (TyNode::Var(r1), TyNode::Var(r2)) if r1 == r2);
        if t1 == t2 || same_var {
            return Ok(());
        }
        if let Some(v) = self.unbound_var(t1) {
            return self.bind(v, t2);
        }
        if let Some(v) = self.unbound_var(t2) {
            return self.bind(v, t1);
        }
        match (self.node(t1), self.node(t2)) {
            (TyNode::Con(n1, as1), TyNode::Con(n2, as2))
                if n1 == n2
                    && (n1 == self.ty_ref || n1 == self.ty_mutable_array)
                    && !as1.is_empty()
                    && !as2.is_empty()
                    && as1.len() == as2.len() =>
            {
                let nm = self.name_of(n1);
                let what = format!("{nm} の第 1 引数");
                self.unify_region(
                    &what,
                    &|msg| format!("ほかのリージョンの {nm} を使おうとしています({msg})"),
                    as1[0],
                    as2[0],
                )?;
                for (x, y) in as1[1..].iter().zip(as2[1..].iter()) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (TyNode::Con(n1, as1), TyNode::Con(n2, as2)) if n1 == n2 && as1.len() == as2.len() => {
                for (x, y) in as1.iter().zip(as2.iter()) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (TyNode::App(f1, a1), TyNode::App(f2, a2)) => {
                self.unify(f1, f2)?;
                self.unify(a1, a2)
            }
            (TyNode::App(f, x), TyNode::Con(n, args)) if !args.is_empty() => {
                let init = args[..args.len() - 1].to_vec();
                let last = args[args.len() - 1];
                let c = self.con(n, init);
                self.unify(f, c)?;
                self.unify(x, last)
            }
            (TyNode::Con(n, args), TyNode::App(f, x)) if !args.is_empty() => {
                let init = args[..args.len() - 1].to_vec();
                let last = args[args.len() - 1];
                let c = self.con(n, init);
                self.unify(c, f)?;
                self.unify(last, x)
            }
            (TyNode::Arrow(p1, r1, e1), TyNode::Arrow(p2, r2, e2)) => {
                self.check_arity(t1, t2, p1, p2)?;
                self.unify(p1, p2)?;
                self.unify(r1, r2)?;
                self.unify(e1, e2)
            }
            (TyNode::Record(r1), TyNode::Record(r2)) => self.unify_in(true, r1, r2),
            (TyNode::Variant(r1), TyNode::Variant(r2)) => self.unify_in(true, r1, r2),
            (TyNode::RowEmpty, TyNode::RowEmpty) => Ok(()),
            (TyNode::RowExtend(l, f, rest), _) => self.unify_row(structural, l, f, rest, t2),
            (_, TyNode::RowExtend(l, f, rest)) => self.unify_row(structural, l, f, rest, t1),
            (TyNode::Var(_), TyNode::RowEmpty) => {
                let s = self.show(t1)?;
                terr(format!(
                    "行 {s} は注釈で固定された行変数なので、空の行と一致させられません"
                ))
            }
            (TyNode::RowEmpty, TyNode::Var(_)) => {
                let s = self.show(t2)?;
                terr(format!(
                    "行 {s} は注釈で固定された行変数なので、空の行と一致させられません"
                ))
            }
            (TyNode::Var(_), _) | (_, TyNode::Var(_)) => {
                self.last_rigid_clash = Some((t1, t2));
                let s = self.show2(t1, t2)?;
                terr(format!("スコープ付きの型が一致しません: {s}"))
            }
            _ => {
                let s = self.show2(t1, t2)?;
                terr(format!("型が一致しません: {s}"))
            }
        }
    }

    /// 関数の型 `t1` と `t2` の引数の型 `p1` と `p2` について、引数の個数を比較する(Diktor の `check_arity`)。
    /// 個数は [`Tc::item_arity`] で求める。
    /// 両方の個数が求まり、かつ異なるときだけ、型エラーにする。
    pub(crate) fn check_arity(&mut self, t1: Ty, t2: Ty, p1: Ty, p2: Ty) -> R<()> {
        match (self.item_arity(p1), self.item_arity(p2)) {
            (Some(n1), Some(n2)) if n1 != n2 => {
                let strs = self.show2(t1, t2)?;
                terr(format!(
                    "引数の個数が一致しません: {strs}(引数は {n1} 個と {n2} 個です)"
                ))
            }
            _ => Ok(()),
        }
    }

    fn unify_row(&mut self, structural: bool, label: Oid, field: Ty, rest: Ty, row2: Ty) -> R<()> {
        let tail_before = self.row_tail_var(rest);
        let (field2, rest2) = self.rewrite_row(row2, label)?;
        if let Some(tv) = tail_before
            && matches!(self.cell(tv), TVar::Link(_))
        {
            return terr("再帰的な行型が発生しました");
        }
        if label == self.eff_heap && !structural {
            self.unify_region(
                "Heap の引数",
                &|msg| format!("ほかのリージョンのヒープを使おうとしています({msg})"),
                field,
                field2,
            )?;
        } else {
            self.unify(field, field2)?;
        }
        self.unify_in(structural, rest, rest2)
    }

    fn unify_region(&mut self, what: &str, both: &dyn Fn(&str) -> String, r1: Ty, r2: Ty) -> R<()> {
        let a = self.repr(r1);
        let b = self.repr(r2);
        let cell_of = |tc: &Tc, t: Ty| match tc.node_ref(t) {
            TyNode::Var(c) => Some(tc.cell(*c).clone()),
            _ => None,
        };
        let shape = match (cell_of(self, a), cell_of(self, b)) {
            (Some(TVar::Rigid(_)), Some(TVar::Rigid(_))) => Shape::Rigids,
            (Some(TVar::Unbound(_)), _) | (_, Some(TVar::Unbound(_))) => Shape::Vars,
            (Some(_), Some(_)) => Shape::Vars,
            (Some(_), None) => Shape::NonRegion(b),
            (None, _) => Shape::NonRegion(a),
        };
        let r = self.unify(r1, r2);
        match r {
            Err(Fail::Type(msg)) => match shape {
                Shape::NonRegion(t) => {
                    let s = self.show(t)?;
                    terr(format!(
                        "{what} {s} はリージョンではありません。{what}には run のリージョン変数か型パラメータを書きます"
                    ))
                }
                Shape::Rigids => terr(both(&msg)),
                Shape::Vars => terr(msg),
            },
            r => r,
        }
    }

    // ---- 数値の既定化と一般化 ----

    /// セル `r` の型変数をリンクの先までたどった結果が、セル `v` の型変数であれば真を返す。
    fn cell_is(&mut self, r: Cell, v: Cell) -> bool {
        match self.cell(r).clone() {
            TVar::Link(t) => {
                let t = self.repr(t);
                self.var_cell(t) == Some(v)
            }
            _ => r == v,
        }
    }

    fn ty_is_cell(&mut self, t: Ty, v: Cell) -> bool {
        let t = self.repr(t);
        self.var_cell(t) == Some(v)
    }

    fn default_to(&mut self, pred: Oid, v: Cell, t: Ty) -> R<()> {
        if let TVar::Unbound(i) = self.cell(v).clone() {
            for c in i.vcls.iter() {
                let via = match self.premise_origins.get(&(i.vid, *c)).cloned() {
                    Some((vt, vc)) => Some((self.subst_unbound(v, t, vt)?, vc)),
                    None => None,
                };
                let r = self.add_class(via, t, *c);
                if let Err(Fail::Type(msg)) = r {
                    return match self.default_origin(v, *c, pred) {
                        Some(loc) => Err(Fail::TypeAt(loc, msg)),
                        None => Err(Fail::Type(msg)),
                    };
                }
                r?;
            }
        }
        self.bind(v, t)
    }

    fn default_origin(&mut self, v: Cell, c: Oid, pred: Oid) -> Option<Span> {
        let holes = self.holes.clone();
        for (_, h) in holes {
            let hd = self.hole(h).clone();
            if hd.h_id > self.decl_hole_floor
                && hd.h_sol.is_none()
                && hd.h_cls == c
                && self.ty_is_cell(hd.h_ty, v)
            {
                return hd.h_loc;
            }
        }
        let lits = self.literal_origins.clone();
        for want in [c, pred] {
            for (r, c2, loc) in &lits {
                if *c2 == want && self.cell_is(*r, v) {
                    return Some(*loc);
                }
            }
        }
        None
    }

    /// 型 `t` を一般化する(Diktor の `generalize`)。
    ///
    /// `level` より深いレベルの単一化変数を、その場で `TVar::Generic` に書き換える。
    /// ただし、予約述語の付いた変数は一般化せず、`Integral` なら `Int32` に、`Fractional` なら `Float64` に決める。
    pub(crate) fn generalize(&mut self, level: i32, t: Ty) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(v) => {
                if let TVar::Unbound(i) = self.cell(v).clone()
                    && i.vlevel > level
                {
                    if i.vcls.contains(&self.cls_integral) {
                        let (p, d) = (self.cls_integral, self.t_int32);
                        self.default_to(p, v, d)?;
                    } else if i.vcls.contains(&self.cls_fractional) {
                        let (p, d) = (self.cls_fractional, self.t_float64);
                        self.default_to(p, v, d)?;
                    } else {
                        self.set_tv(v, TVar::Generic(i));
                    }
                }
                Ok(())
            }
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    self.generalize(level, *a)?;
                }
                Ok(())
            }
            TyNode::App(f, a) => {
                self.generalize(level, f)?;
                self.generalize(level, a)
            }
            TyNode::Arrow(p, r, e) => {
                self.generalize(level, p)?;
                self.generalize(level, r)?;
                self.generalize(level, e)
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.generalize(level, row),
            TyNode::RowEmpty => Ok(()),
            TyNode::RowExtend(_, f, rest) => {
                self.generalize(level, f)?;
                self.generalize(level, rest)
            }
        }
    }

    fn lower_cell(&mut self, v: Cell, level: i32) {
        if let TVar::Unbound(mut i) = self.cell(v).clone()
            && i.vlevel > level
        {
            i.vlevel = level;
            self.set_tv(v, TVar::Unbound(i));
        }
    }

    fn lower_go(&mut self, level: i32, t: Ty) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(v) => {
                self.lower_cell(v, level);
                Ok(())
            }
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    self.lower_go(level, *a)?;
                }
                Ok(())
            }
            TyNode::App(f, a) => {
                self.lower_go(level, f)?;
                self.lower_go(level, a)
            }
            TyNode::Arrow(p, r, e) => {
                self.lower_go(level, p)?;
                self.lower_go(level, r)?;
                self.lower_go(level, e)
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.lower_go(level, row),
            TyNode::RowEmpty => Ok(()),
            TyNode::RowExtend(_, f, rest) => {
                self.lower_go(level, f)?;
                self.lower_go(level, rest)
            }
        }
    }

    /// 一般化しない束縛の型 `t` に残った、`level` より深い単一化変数のレベルを `level` まで下げる(Diktor の `lower_levels`)。
    /// 一覧 `class_vars` のうち、通し番号が `since` より大きい項目の変数も同じように下げる。
    pub(crate) fn lower_levels(&mut self, since: i64, level: i32, t: Ty) -> R<()> {
        let vs: Vec<Cell> = self
            .class_vars
            .iter()
            .rev()
            .take_while(|(n, _)| *n > since)
            .map(|(_, v)| *v)
            .collect();
        for v in vs {
            self.lower_cell(v, level);
        }
        self.lower_go(level, t)
    }

    /// 一覧 `class_vars` に残っている、予約述語の付いた単一化変数を、既定の型に決める(Diktor の `default_numerics`)。
    ///
    /// `Integral` の付いた変数は `Int32` に、`Fractional` の付いた変数は `Float64` に決める。
    /// 一覧を古い順にたどるので、既定化の失敗が 2 つ以上あるときは、先に作った変数の失敗を報告する。
    /// 終わりに、`class_vars` とリテラルの位置の記録を空にする。
    /// あわせて `decl_hole_floor` を新しい番号に進め、これより前に作った穴を、既定化の失敗を報告する位置の候補から外す。
    pub(crate) fn default_numerics(&mut self) -> R<()> {
        let vs: Vec<Cell> = self.class_vars.iter().map(|(_, v)| *v).collect();
        for v in vs {
            if let TVar::Unbound(i) = self.cell(v).clone() {
                if i.vcls.contains(&self.cls_integral) {
                    let (p, d) = (self.cls_integral, self.t_int32);
                    self.default_to(p, v, d)?;
                } else if i.vcls.contains(&self.cls_fractional) {
                    let (p, d) = (self.cls_fractional, self.t_float64);
                    self.default_to(p, v, d)?;
                }
            }
        }
        self.class_vars.clear();
        self.literal_origins.clear();
        self.decl_hole_floor = self.new_oid();
        Ok(())
    }

    fn collect_vars(&mut self, t: Ty, acc: &mut HashSet<Oid>) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(r) => {
                if !matches!(self.cell(r), TVar::Link(_)) {
                    acc.insert(self.var_info_of(r)?.vid);
                }
                Ok(())
            }
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    self.collect_vars(*a, acc)?;
                }
                Ok(())
            }
            TyNode::App(f, a) => {
                self.collect_vars(f, acc)?;
                self.collect_vars(a, acc)
            }
            TyNode::Arrow(p, r, e) => {
                self.collect_vars(p, acc)?;
                self.collect_vars(r, acc)?;
                self.collect_vars(e, acc)
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.collect_vars(row, acc),
            TyNode::RowEmpty => Ok(()),
            TyNode::RowExtend(_, f, rest) => {
                self.collect_vars(f, acc)?;
                self.collect_vars(rest, acc)
            }
        }
    }

    /// 曖昧な制約を検査する(Diktor の `check_ambiguity`)。
    ///
    /// 制約の付いた単一化変数のうち、`tys` のどの型にも現れないものがあれば、型エラーにする。
    /// 対象は `level` より深い変数で、`all` が真のときはレベルによらずすべての変数である。
    /// 予約述語の付いた変数は、既定化で型が決まるので対象にしない。
    /// 検査に通ったときは、一覧 `class_vars` に単一化変数のままの項目だけを残す。
    pub(crate) fn check_ambiguity(&mut self, all: bool, level: i32, tys: &[Ty]) -> R<()> {
        let mut reach = HashSet::default();
        for &t in tys {
            self.collect_vars(t, &mut reach)?;
        }
        let entries = self.class_vars.clone();
        let mut kept_rev = Vec::new();
        for (n, v) in entries.iter().rev() {
            if let TVar::Unbound(i) = self.cell(*v).clone() {
                kept_rev.push((*n, *v));
                if (all || i.vlevel > level)
                    && !i.vcls.is_empty()
                    && !i.vcls.iter().any(|c| self.is_predicate(*c))
                    && !reach.contains(&i.vid)
                {
                    let mut names: Vec<String> =
                        i.vcls.iter().map(|c| self.display_of(*c)).collect();
                    names.sort();
                    return terr(format!(
                        "曖昧な制約: {} を満たす型が決まりません(結果の型に現れない型変数です。注釈で型を決めてください)",
                        names.join(" + ")
                    ));
                }
            }
        }
        kept_rev.reverse();
        self.class_vars = kept_rev;
        Ok(())
    }

    // ---- 具体化 ----

    /// 型 `t` の中の一般化された型変数(`TVar::Generic`)を、`f` が返す型に置き換えた型を返す(Diktor の `map_generics_with`)。
    /// 置き換え先は変数の番号ごとに `memo` に記録するので、`f` は変数 1 つにつき 1 回だけ呼ばれる。
    pub(crate) fn map_generics_with(
        &mut self,
        memo: &mut HashMap<Oid, Ty>,
        f: &mut dyn FnMut(&mut Tc, &VarInfo) -> Ty,
        t: Ty,
    ) -> R<Ty> {
        guard()?;
        let t = self.repr(t);
        Ok(match self.node(t) {
            TyNode::Var(v) => match self.cell(v).clone() {
                TVar::Generic(i) => match memo.get(&i.vid) {
                    Some(t) => *t,
                    None => {
                        let t = f(self, &i);
                        memo.insert(i.vid, t);
                        t
                    }
                },
                _ => self.tvar(v),
            },
            TyNode::Con(n, args) => {
                let mut v = Vec::with_capacity(args.len());
                for a in args.iter() {
                    v.push(self.map_generics_with(memo, f, *a)?);
                }
                self.con(n, v)
            }
            TyNode::App(g, a) => {
                let g2 = self.map_generics_with(memo, f, g)?;
                let a2 = self.map_generics_with(memo, f, a)?;
                self.tapp(g2, a2)
            }
            TyNode::Arrow(p, r, e) => {
                let p2 = self.map_generics_with(memo, f, p)?;
                let r2 = self.map_generics_with(memo, f, r)?;
                let e2 = self.map_generics_with(memo, f, e)?;
                self.arrow(p2, r2, e2)
            }
            TyNode::Record(row) => {
                let r2 = self.map_generics_with(memo, f, row)?;
                self.record(r2)
            }
            TyNode::Variant(row) => {
                let r2 = self.map_generics_with(memo, f, row)?;
                self.variant(r2)
            }
            TyNode::RowEmpty => self.row_empty,
            TyNode::RowExtend(l, fl, rest) => {
                let f2 = self.map_generics_with(memo, f, fl)?;
                let r2 = self.map_generics_with(memo, f, rest)?;
                self.extend(l, f2, r2)
            }
        })
    }

    /// 型スキーム `t` を具体化する(Diktor の `instantiate`)。
    /// 一般化された型変数を、カインドと制約を引き継いだ、レベル `level` の新しい型変数に置き換える。
    pub(crate) fn instantiate(&mut self, level: i32, t: Ty) -> R<Ty> {
        let mut memo = HashMap::default();
        self.map_generics_with(
            &mut memo,
            &mut |tc, i| tc.new_class_var(i.vkind, i.vcls.clone(), level),
            t,
        )
    }

    /// 一般化された型変数のうち、`args` に番号があるものを、対応する型に置き換える(Diktor の `subst_params`)。
    /// `args` にない変数は、[`Tc::instantiate`] と同じように新しい型変数に置き換える。
    pub(crate) fn subst_params(&mut self, level: i32, args: &[(Oid, Ty)], t: Ty) -> R<Ty> {
        let mut memo = HashMap::default();
        self.map_generics_with(
            &mut memo,
            &mut |tc, i| match args.iter().find(|(v, _)| *v == i.vid) {
                Some((_, t)) => *t,
                None => tc.new_class_var(i.vkind, i.vcls.clone(), level),
            },
            t,
        )
    }

    // ---- 辞書の穴 ----

    /// 辞書の穴の一覧 `holes` に最後に振った通し番号を返す(Diktor の `hole_mark`)。
    /// この後で一覧に追加する穴は、これより大きい番号を持つ。
    pub(crate) fn hole_mark(&self) -> i64 {
        self.hole_seq
    }

    fn note_hole(&mut self, h: HoleId) {
        self.hole_seq += 1;
        self.holes.push((self.hole_seq, h));
    }

    /// 型 `t` のクラス `c` の証拠を入れる辞書の穴を作り、一覧 `holes` に追加する。
    pub(crate) fn new_hole(&mut self, loc: Span, c: Oid, t: Ty) -> HoleId {
        let h_id = self.new_oid();
        let h = self.add_hole(Hole {
            h_id,
            h_cls: c,
            h_ty: t,
            h_loc: Some(loc),
            h_sol: None,
        });
        self.note_hole(h);
        h
    }

    fn row_spine(&mut self, t: Ty) -> (Vec<(Oid, Ty)>, Ty) {
        self.row_fields(t)
    }

    /// フィールドを、ラベル名のバイト列の昇順に並べ替える。
    /// 同じラベルのフィールドは、元の順序を保つ。
    pub(crate) fn sort_row_fields(&self, fields: &mut [(Oid, Ty)]) {
        fields
            .sort_by(|(a, _), (b, _)| self.name_of(*a).as_bytes().cmp(self.name_of(*b).as_bytes()));
    }

    /// 型スキーム `t` の制約を、正準順(canonical。型だけから一意に決まる順序)に並べて返す(Diktor の `constraints_of`)。
    ///
    /// 型を左から右へたどり、一般化された型変数が初めて現れた順に、その変数の制約を並べる。
    /// 行は、フィールドをラベル名のバイト列の昇順にたどる。
    /// 1 つの変数の制約は `class_rank` の昇順に並べ、予約述語は取り除く。
    pub(crate) fn constraints_of(&mut self, t: Ty) -> R<Vec<(VarInfo, Oid)>> {
        let mut seen = HashSet::default();
        let mut acc = Vec::new();
        self.constraints_go(t, &mut seen, &mut acc)?;
        Ok(acc)
    }

    fn constraints_go(
        &mut self,
        t: Ty,
        seen: &mut HashSet<Oid>,
        acc: &mut Vec<(VarInfo, Oid)>,
    ) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(v) => {
                if let TVar::Generic(i) = self.cell(v).clone()
                    && !seen.contains(&i.vid)
                {
                    seen.insert(i.vid);
                    let mut cs: Vec<Oid> = i
                        .vcls
                        .iter()
                        .copied()
                        .filter(|c| !self.is_predicate(*c))
                        .collect();
                    cs.sort_by_key(|c| self.class_rank(*c));
                    for c in cs {
                        acc.push((i.clone(), c));
                    }
                }
                Ok(())
            }
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    self.constraints_go(*a, seen, acc)?;
                }
                Ok(())
            }
            TyNode::App(f, a) => {
                self.constraints_go(f, seen, acc)?;
                self.constraints_go(a, seen, acc)
            }
            TyNode::Arrow(p, r, e) => {
                self.constraints_go(p, seen, acc)?;
                self.constraints_go(r, seen, acc)?;
                self.constraints_go(e, seen, acc)
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.constraints_go(row, seen, acc),
            TyNode::RowEmpty => Ok(()),
            TyNode::RowExtend(..) => {
                let (mut fields, rest) = self.row_spine(t);
                self.sort_row_fields(&mut fields);
                for (_, f) in fields {
                    self.constraints_go(f, seen, acc)?;
                }
                self.constraints_go(rest, seen, acc)
            }
        }
    }

    /// 型スキーム `t` の辞書のキー(型変数の番号とクラスの組)を、正準順に並べて返す(Diktor の `dkeys_of`)。
    pub(crate) fn dkeys_of(&mut self, t: Ty) -> R<Vec<DKey>> {
        Ok(self
            .constraints_of(t)?
            .into_iter()
            .map(|(i, c)| (i.vid, c))
            .collect())
    }

    /// 型スキーム `t` を [`Tc::instantiate`] と同じように具体化し、制約ごとに辞書の穴を作る(Diktor の `instantiate_ev`)。
    /// 具体化した型と、正準順に並べた穴を返す。
    pub(crate) fn instantiate_ev(&mut self, loc: Span, level: i32, t: Ty) -> R<(Ty, Vec<HoleId>)> {
        let cs = self.constraints_of(t)?;
        let mut memo = HashMap::default();
        let t2 = self.map_generics_with(
            &mut memo,
            &mut |tc, i| tc.new_class_var(i.vkind, i.vcls.clone(), level),
            t,
        )?;
        let mut hs = Vec::with_capacity(cs.len());
        for (i, c) in cs {
            let ty = memo[&i.vid];
            hs.push(self.new_hole(loc, c, ty));
        }
        Ok((t2, hs))
    }

    fn evidence_of(
        &mut self,
        loc: Option<Span>,
        params: &[DKey],
        origin: HoleId,
        c: Oid,
        t: Ty,
    ) -> R<Evidence> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(v) => match self.cell(v).clone() {
                TVar::Generic(i) => {
                    if params.contains(&(i.vid, c)) {
                        Ok(Evidence::Param((i.vid, c)))
                    } else {
                        Err(Fail::Unsolved(origin))
                    }
                }
                TVar::Rigid(i) if params.contains(&(i.vid, c)) => Ok(Evidence::Param((i.vid, c))),
                _ => {
                    let loc = loc.unwrap_or_default();
                    Ok(Evidence::Hole(self.new_hole(loc, c, t)))
                }
            },
            TyNode::Con(n, args) => match self.find_instance(c, n) {
                Some(ii) => {
                    let mut evs = Vec::new();
                    for (i, c2) in ii.ii_premises {
                        let a = match args.get(i) {
                            Some(a) => *a,
                            None => return bug("List.nth"),
                        };
                        evs.push(self.evidence_of(loc, params, origin, c2, a)?);
                    }
                    Ok(Evidence::Inst(c, n, evs))
                }
                None => bug(format!(
                    "証拠: インスタンスがありません: {}[{}]",
                    self.display_of(c),
                    self.display_of(n)
                )),
            },
            TyNode::Record(row) | TyNode::Variant(row) => {
                let is_rec = matches!(self.node_ref(t), TyNode::Record(_));
                let structural = self.find_class(c).is_some_and(|ci| ci.ci_derive_structural);
                if !structural {
                    return bug(format!(
                        "証拠: 構造的導出のないクラスの構造的な型です: {}",
                        self.display_of(c)
                    ));
                }
                let (mut fields, _) = self.row_fields(row);
                self.sort_row_fields(&mut fields);
                let mut evs = Vec::new();
                for (l, f) in fields {
                    evs.push((l, self.evidence_of(loc, params, origin, c, f)?));
                }
                Ok(if is_rec {
                    Evidence::Record(evs)
                } else {
                    Evidence::Variant(evs)
                })
            }
            _ => {
                let s = self.show(t)?;
                bug(format!("証拠: 解けない型 {s}"))
            }
        }
    }

    /// 一覧 `holes` のうち、通し番号が `since` より大きい穴を解く(Diktor の `solve_holes`)。
    pub(crate) fn solve_holes(&mut self, since: i64, params: &[DKey]) -> R<()> {
        self.solve_holes_range(since, i64::MAX, params)
    }

    /// 一覧 `holes` のうち、通し番号が `lo` より大きく `hi` 以下の穴を解き、一覧から外す(Diktor の `solve_holes_range`)。
    ///
    /// `params` は、囲む束縛の辞書パラメータのキーである。
    /// 型がまだ単一化変数である穴と、`params` にキーのないスコーレム定数の穴は、解かずに新しい番号で一覧に追加し直す。
    pub(crate) fn solve_holes_range(&mut self, lo: i64, hi: i64, params: &[DKey]) -> R<()> {
        let all = std::mem::take(&mut self.holes);
        let (mine, older): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|(n, _)| *n > lo && *n <= hi);
        self.holes = older;
        for (_, h) in mine {
            if self.hole(h).h_sol.is_none() {
                let ht = self.hole(h).h_ty;
                let hc = self.hole(h).h_cls;
                let hl = self.hole(h).h_loc;
                let rt = self.repr(ht);
                let cellv = self.var_cell(rt).map(|c| self.cell(c).clone());
                match cellv {
                    Some(TVar::Unbound(_)) => self.note_hole(h),
                    Some(TVar::Rigid(i)) if !params.contains(&(i.vid, hc)) => self.note_hole(h),
                    _ => {
                        let ev = self.evidence_of(hl, params, h, hc, ht)?;
                        self.set_hole_sol(h, ev);
                    }
                }
            }
        }
        Ok(())
    }

    /// 一覧 `holes` にある解けていない穴を、古い順に返す(Diktor の `List.rev (unsolved_holes ())`)。
    pub(crate) fn unsolved_holes_oldest_first(&self) -> Vec<HoleId> {
        self.holes
            .iter()
            .filter(|(_, h)| self.hole(*h).h_sol.is_none())
            .map(|(_, h)| *h)
            .collect()
    }

    /// 単一化の状態を初期の状態に戻す(Diktor の `Unify.reset`)。
    ///
    /// 制約の付いた型変数の一覧、リテラルの位置の記録、元の要求の表、辞書の穴の一覧を空にする。
    /// あわせて `decl_hole_floor` を新しい番号に進め、これより前に作った穴を、既定化の失敗を報告する位置の候補から外す。
    /// 通し番号は戻さない。
    pub(crate) fn unify_reset(&mut self) {
        self.class_vars.clear();
        self.literal_origins.clear();
        self.decl_hole_floor = self.new_oid();
        self.premise_origins.clear();
        self.holes.clear();
    }
}
