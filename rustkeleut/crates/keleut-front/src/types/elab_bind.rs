//! 束縛と再帰群の精緻化である(Diktor の elab.ml の §11.26〜§11.30)。

use super::elab::{GroupRec, has_sub, irrefutable_pat, is_value, outer_eff_written};
use super::{
    Cell, DictInfo, Env, Evidence, Fail, GRef, Hole, Oid, R, TVar, Tc, Ty, TyNode, at, guard, terr,
};
use crate::ast::{ExpKind, LetBinding, NodeId, Pat, PatKind, TypeExpKind};
use crate::pos::Span;
use std::rc::Rc;

/// メソッドの期待する引数の型を名前から引く関数(Diktor の `expected_params`)である。
pub(crate) type ExpectedParams<'a> = &'a dyn Fn(&mut Tc, &str) -> R<Option<Vec<Ty>>>;

/// 再帰群の事前の型の形(Diktor の `pre_shape`)である。
#[derive(Clone, Debug)]
enum PreShape {
    Open(Cell),
    Fixed(u8, Vec<PreShape>),
    Row(Vec<(Oid, PreShape)>, Box<PreShape>),
}

/// 剛定数の並び(名前、型、セル)。
pub(crate) type Rigids = Vec<(String, Ty, Cell)>;

impl Tc {
    fn method_param_tys(&mut self, lvl: i32, expected: Option<Vec<Ty>>, params: &[Pat]) -> Vec<Ty> {
        let fresh: Vec<Ty> = params.iter().map(|_| self.new_var(lvl)).collect();
        match expected {
            Some(ets) if ets.len() == params.len() => params
                .iter()
                .zip(ets.into_iter().zip(fresh))
                .map(|(p, (e, f))| match p.kind {
                    PatKind::Var(_) | PatKind::Wildcard => e,
                    _ => f,
                })
                .collect(),
            _ => fresh,
        }
    }

    fn finish_binding_dicts(
        &mut self,
        id: NodeId,
        gen_: bool,
        rigids: &Rigids,
        hmark: i64,
        fn_ty: Ty,
    ) -> R<()> {
        let dps = if gen_ {
            self.dkeys_of(fn_ty)?
        } else {
            Vec::new()
        };
        if gen_ {
            self.check_tparam_constraints(rigids, &dps)?;
        }
        if !dps.is_empty() {
            self.set_dict(id, DictInfo::Abs(dps.clone()));
        }
        self.solve_holes_at(hmark, &dps)
    }

    fn queue_params(&mut self, params: &[Pat], tys: &[Ty]) {
        for (p, t) in params.iter().zip(tys.iter()) {
            if !irrefutable_pat(p) {
                self.ex_queue(vec![(p.clone(), false)], *t);
            }
        }
    }

    /// `let` の束縛の精緻化(Diktor の `elab_binding`)。
    pub(crate) fn elab_binding(
        &mut self,
        expected_params: Option<Vec<Ty>>,
        env: &Env,
        level: i32,
        eff: Ty,
        b: &LetBinding,
    ) -> R<Env> {
        guard()?;
        let r = self.elab_binding_inner(expected_params, env, level, eff, b);
        at(b.meta.span, r)
    }

    fn elab_binding_inner(
        &mut self,
        expected_params: Option<Vec<Ty>>,
        env: &Env,
        level: i32,
        eff: Ty,
        b: &LetBinding,
    ) -> R<Env> {
        if b.is_pub {
            let tps = Tc::pub_tparams(env, &b.tparams);
            self.check_pub_annots(tps, b.params.as_deref(), b.ret.as_ref())?;
        }
        let is_fun = b.params.is_some();
        let gen_ = is_fun || is_value(&b.body);
        if !gen_ && !b.tparams.is_empty() {
            return terr(
                "非値の束縛に型パラメータは付けられません(値制限。関数にするか値を束縛してください)",
            );
        }
        let lvl = level + 1;
        let mark = self.class_mark();
        let hmark = self.hole_mark();
        let rigids = self.make_rigids(None, lvl, &b.tparams)?;
        let env_ty = env.with_types(&rigids);
        let fn_ty = match &b.params {
            Some(params) => {
                let mut seen = Vec::new();
                let param_tys = self.method_param_tys(lvl, expected_params.clone(), params);
                let mut env2 = env_ty.clone();
                for (p, t) in params.iter().zip(param_tys.iter()) {
                    env2 = self.elab_pat(&env2, lvl, &mut seen, *t, p)?;
                }
                let fn_eff = match &b.eff {
                    Some(e) => self.elab_eff(&env_ty, lvl, e)?,
                    None if b.is_pub => self.new_pub_pure_row(),
                    None => self.new_row_var(lvl),
                };
                let ret_ty = match &b.ret {
                    Some(t) => self.elab_value_type(&env_ty, lvl, "返り値の型注釈", t)?,
                    None => self.new_var(lvl),
                };
                let body_ty = self.elab_exp(&env2, lvl, fn_eff, &b.body)?;
                let sub = self.subsume_closed(lvl, body_ty, ret_ty)?;
                match self.unify(ret_ty, sub) {
                    Err(Fail::Type(msg)) if b.ret.is_some() => {
                        return terr(format!("注釈された返り値型を満たしません({msg})"));
                    }
                    r => r?,
                }
                self.queue_params(params, &param_tys);
                let row = self.closed_item_row(&param_tys);
                let p = self.record(row);
                self.arrow(p, ret_ty, fn_eff)
            }
            None => {
                let vty = match &b.ret {
                    None => self.new_var(lvl),
                    Some(t) => {
                        let ty = self.elab_value_type_outer(&env_ty, lvl, "型注釈", t)?;
                        let rt = self.repr(ty);
                        match (&t.kind, self.node(rt)) {
                            (TypeExpKind::Arrow(_, _, Some(_)), TyNode::Arrow(..)) => ty,
                            (TypeExpKind::Arrow(_, _, None), TyNode::Arrow(a, r, _))
                                if b.is_pub =>
                            {
                                let e = self.row_empty;
                                self.arrow(a, r, e)
                            }
                            _ => ty,
                        }
                    }
                };
                let body_ty = match (&expected_params, &b.ret, &b.body.kind) {
                    (Some(ets), None, ExpKind::Lambda(l))
                        if ets.len() == l.params.len()
                            && l.params
                                .iter()
                                .all(|p| matches!(p.kind, PatKind::Var(_) | PatKind::Wildcard)) =>
                    {
                        let rv = self.new_row_var(lvl);
                        let ret = self.new_var(lvl);
                        let row = self.closed_item_row(ets);
                        let p = self.record(row);
                        let t = self.arrow(p, ret, rv);
                        self.elab_check(None, &env_ty, lvl, eff, &b.body, t)?;
                        t
                    }
                    _ => self.elab_exp(&env_ty, lvl, eff, &b.body)?,
                };
                let body_ty = self.subsume_closed(lvl, body_ty, vty)?;
                if b.is_pub && !outer_eff_written(b) {
                    self.unify_pub_value_annot(vty, body_ty, &|msg| {
                        Fail::Type(format!("注釈された型を満たしません({msg})"))
                    })?;
                } else {
                    match self.unify(vty, body_ty) {
                        Err(Fail::Type(msg)) if b.ret.is_some() => {
                            return terr(format!("注釈された型を満たしません({msg})"));
                        }
                        r => r?,
                    }
                }
                vty
            }
        };
        self.set_ty(b.meta.id, fn_ty);
        match &b.name.kind {
            PatKind::Var(_) | PatKind::Wildcard => {
                let ws = self.ex_drain(level, &[fn_ty], gen_)?;
                for w in ws {
                    self.warn(w);
                }
                if gen_ {
                    self.check_ambiguity(false, level, &[fn_ty])?;
                    self.generalize(level, fn_ty)?;
                } else {
                    self.reclose_opened_tail(lvl, fn_ty)?;
                    self.lower_levels(mark, level, fn_ty)?;
                }
                self.release_rigids(&rigids)?;
                self.finish_binding_dicts(b.meta.id, gen_, &rigids, hmark, fn_ty)?;
                match &b.name.kind {
                    PatKind::Var(x) => Ok(env.add_local(x, fn_ty)),
                    _ => Ok(env.clone()),
                }
            }
            _ => {
                if !gen_ {
                    self.reclose_opened_tail(lvl, fn_ty)?;
                    self.lower_levels(mark, level, fn_ty)?;
                }
                let mut seen = Vec::new();
                let env2 = self.elab_pat(env, level, &mut seen, fn_ty, &b.name)?;
                self.ex_queue(vec![(b.name.clone(), false)], fn_ty);
                let ws = self.ex_drain(level, &[fn_ty], false)?;
                for w in ws {
                    self.warn(w);
                }
                self.release_rigids(&rigids)?;
                self.finish_binding_dicts(b.meta.id, false, &rigids, hmark, fn_ty)?;
                Ok(env2)
            }
        }
    }

    // ---- 再帰群 ----

    fn mentions(&mut self, r: Cell, t: Ty) -> R<bool> {
        guard()?;
        let t = self.repr(t);
        Ok(match self.node(t) {
            TyNode::Var(v) => v == r,
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    if self.mentions(r, *a)? {
                        return Ok(true);
                    }
                }
                false
            }
            TyNode::App(f, a) => self.mentions(r, f)? || self.mentions(r, a)?,
            TyNode::Arrow(a, ret, e) => {
                self.mentions(r, a)? || self.mentions(r, ret)? || self.mentions(r, e)?
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.mentions(r, row)?,
            TyNode::RowEmpty => false,
            TyNode::RowExtend(_, f, rest) => self.mentions(r, f)? || self.mentions(r, rest)?,
        })
    }

    fn unbound_vars(&mut self, acc: &mut Vec<Cell>, t: Ty) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(r) => {
                if matches!(self.cell(r), TVar::Unbound(_)) && !acc.contains(&r) {
                    acc.insert(0, r);
                }
            }
            TyNode::RowEmpty => {}
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    self.unbound_vars(acc, *a)?;
                }
            }
            TyNode::App(f, a) => {
                self.unbound_vars(acc, f)?;
                self.unbound_vars(acc, a)?;
            }
            TyNode::Arrow(a, ret, _) => {
                self.unbound_vars(acc, a)?;
                self.unbound_vars(acc, ret)?;
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.unbound_vars(acc, row)?,
            TyNode::RowExtend(_, f, rest) => {
                self.unbound_vars(acc, f)?;
                self.unbound_vars(acc, rest)?;
            }
        }
        Ok(())
    }

    fn kids(&self, t: Ty) -> (u8, Vec<Ty>) {
        match self.node_ref(t) {
            TyNode::Arrow(a, r, _) => (1, vec![*a, *r]),
            TyNode::Record(r) => (2, vec![*r]),
            TyNode::Variant(r) => (3, vec![*r]),
            TyNode::RowExtend(_, f, rest) => (4, vec![*f, *rest]),
            TyNode::Con(_, args) => (5, args.to_vec()),
            TyNode::App(f, a) => (6, vec![*f, *a]),
            TyNode::Var(_) | TyNode::RowEmpty => (0, Vec::new()),
        }
    }

    fn shape(&mut self, t: Ty) -> R<PreShape> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Var(r) if matches!(self.cell(r), TVar::Unbound(_)) => Ok(PreShape::Open(r)),
            TyNode::RowExtend(..) => {
                let (fs, tl) = self.row_fields(t);
                let mut v = Vec::new();
                for (l, f) in fs {
                    v.push((l, self.shape(f)?));
                }
                Ok(PreShape::Row(v, Box::new(self.shape(tl)?)))
            }
            _ => {
                let (k, ts) = self.kids(t);
                let mut v = Vec::new();
                for x in ts {
                    v.push(self.shape(x)?);
                }
                Ok(PreShape::Fixed(k, v))
            }
        }
    }

    fn rigids_in(&mut self, own: &[Cell], t: Ty) -> R<Vec<Cell>> {
        guard()?;
        let t = self.repr(t);
        if let Some(r) = self.var_cell(t)
            && own.contains(&r)
        {
            return Ok(vec![r]);
        }
        let (_, ts) = self.kids(t);
        let mut out = Vec::new();
        for x in ts {
            out.extend(self.rigids_in(own, x)?);
        }
        Ok(out)
    }

    fn fixed_rigids(&mut self, own: &[Cell], joint: &[Cell], sh: &PreShape, f: Ty) -> R<Vec<Cell>> {
        guard()?;
        let f = self.repr(f);
        if let PreShape::Open(r) = sh {
            return if joint.contains(r) {
                self.rigids_in(own, f)
            } else {
                Ok(Vec::new())
            };
        }
        if let Some(r) = self.var_cell(f)
            && own.contains(&r)
        {
            return Ok(vec![r]);
        }
        match sh {
            PreShape::Row(sfs, stl) if matches!(self.node_ref(f), TyNode::RowExtend(..)) => {
                let (bfs, btl) = self.row_fields(f);
                let mut left: Vec<(Oid, PreShape)> = sfs.clone();
                let mut found = Vec::new();
                let mut extra = false;
                for (l, bf) in bfs {
                    match left.iter().position(|(l2, _)| *l2 == l) {
                        Some(i) => {
                            let (_, sh2) = left.remove(i);
                            found.extend(self.fixed_rigids(own, joint, &sh2, bf)?);
                        }
                        None => {
                            found.extend(self.rigids_in(own, bf)?);
                            extra = true;
                        }
                    }
                }
                if left.is_empty() && !extra {
                    found.extend(self.fixed_rigids(own, joint, stl, btl)?);
                } else {
                    found.extend(self.rigids_in(own, btl)?);
                }
                Ok(found)
            }
            PreShape::Fixed(k, shs) => {
                let (k2, ts) = self.kids(f);
                if *k == k2 && shs.len() == ts.len() {
                    let mut out = Vec::new();
                    for (s, t) in shs.iter().zip(ts) {
                        out.extend(self.fixed_rigids(own, joint, s, t)?);
                    }
                    Ok(out)
                } else {
                    self.rigids_in(own, f)
                }
            }
            _ => self.rigids_in(own, f),
        }
    }

    /// `let rec` の群の精緻化(Diktor の `elab_rec_bindings`)。
    pub(crate) fn elab_rec_bindings(
        &mut self,
        top: bool,
        expected_params: ExpectedParams,
        env: &Env,
        level: i32,
        eff: Ty,
        bs: &[LetBinding],
    ) -> R<Env> {
        guard()?;
        let lvl = level + 1;
        let mut tps: Vec<Rigids> = Vec::new();
        for b in bs {
            let r = self.make_rigids(None, lvl, &b.tparams);
            tps.push(at(b.meta.span, r)?);
        }
        // 名前と事前の型。
        let mut names: Vec<(String, Ty)> = Vec::new();
        let mut sigs: Vec<(Option<Ty>, Option<Ty>)> = Vec::new();
        for (b, rigids) in bs.iter().zip(tps.iter()) {
            match (&b.params, &b.body.kind) {
                (None, ExpKind::Lambda(_)) | (Some(_), _) => {}
                (None, _) => return terr("let rec の右辺は関数でなければなりません"),
            }
            match &b.name.kind {
                PatKind::Var(x) => {
                    type Sig = ((String, Ty), (Option<Ty>, Option<Ty>));
                    let r = (|| -> R<Sig> {
                        let env_ty = env.with_types(rigids);
                        let arrow = |tc: &mut Tc, n: usize, e: Ty| {
                            let ret = tc.new_var(lvl);
                            let ps: Vec<Ty> = (0..n).map(|_| tc.new_var(lvl)).collect();
                            let row = tc.closed_item_row(&ps);
                            let p = tc.record(row);
                            tc.arrow(p, ret, e)
                        };
                        match (&b.params, &b.eff, &b.ret) {
                            (Some(params), Some(e), _) => {
                                let e = self.elab_eff(&env_ty, lvl, e)?;
                                Ok(((x.clone(), arrow(self, params.len(), e)), (Some(e), None)))
                            }
                            (Some(params), None, _) if b.is_pub => {
                                let e = self.new_pub_pure_row();
                                Ok(((x.clone(), arrow(self, params.len(), e)), (Some(e), None)))
                            }
                            (None, _, Some(t)) => {
                                let ty = self.elab_value_type_outer(&env_ty, lvl, "型注釈", t)?;
                                let rt = self.repr(ty);
                                let ty = match (&t.kind, self.node(rt)) {
                                    (TypeExpKind::Arrow(_, _, None), TyNode::Arrow(a, r, _))
                                        if b.is_pub =>
                                    {
                                        let e = self.row_empty;
                                        self.arrow(a, r, e)
                                    }
                                    _ => ty,
                                };
                                Ok(((x.clone(), ty), (None, Some(ty))))
                            }
                            _ => Ok(((x.clone(), self.new_var(lvl)), (None, None))),
                        }
                    })();
                    let (n, s) = at(b.meta.span, r)?;
                    names.push(n);
                    sigs.push(s);
                }
                _ => return terr("let rec の束縛はパターンにできません"),
            }
        }
        let bind_names = |tc: &mut Tc, env: &Env, names: &[(String, Ty)]| -> Env {
            let mut e = env.clone();
            for (b, (x, t)) in bs.iter().zip(names.iter()) {
                e = if top {
                    tc.add_top(&e, x, *t, GRef::Decl(b.meta.id, x.clone()))
                } else {
                    e.add_local(x, *t)
                };
            }
            e
        };
        let env_rec = bind_names(self, env, &names);
        let mut group_rigids: Rigids = Vec::new();
        let mut rec_arg_queue: Vec<(&[Pat], Vec<Ty>)> = Vec::new();
        self.group_stack.push(GroupRec {
            g_pres: names
                .iter()
                .zip(bs.iter())
                .map(|((_, pre), b)| (*pre, b.meta.id))
                .collect(),
            g_cur: None,
            g_refs: Vec::new(),
        });
        let gidx = self.group_stack.len() - 1;
        let mut member_marks: Vec<(NodeId, i64)> = Vec::new();
        let mut pre_vars: Vec<(Ty, Vec<Cell>)> = Vec::new();
        for (_, pre) in &names {
            let mut acc = Vec::new();
            self.unbound_vars(&mut acc, *pre)?;
            pre_vars.push((*pre, acc));
        }
        let mut settled_before: Vec<(NodeId, Vec<Cell>)> = Vec::new();
        self.last_rigid_clash = None;
        let loop_result = (|| -> R<()> {
            for (i, b) in bs.iter().enumerate() {
                let pre = names[i].1;
                let (sig_eff, sig_vty) = sigs[i];
                let rigids = &tps[i];
                let r = (|| -> R<()> {
                    self.group_stack[gidx].g_cur = Some(b.meta.id);
                    let pv = pre_vars
                        .iter()
                        .find(|(p, _)| *p == pre)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    let mut settled = false;
                    for r in pv {
                        let t = self.tvar(r);
                        let t = self.repr(t);
                        let unb = self
                            .var_cell(t)
                            .is_some_and(|c| matches!(self.cell(c), TVar::Unbound(_)));
                        if !unb {
                            settled = true;
                            break;
                        }
                    }
                    let snap = if settled {
                        Some(self.shape(pre)?)
                    } else {
                        None
                    };
                    let hm = self.hole_mark();
                    member_marks.push((b.meta.id, hm));
                    if b.is_pub {
                        let tps2 = Tc::pub_tparams(&env_rec, &b.tparams);
                        self.check_pub_annots(tps2, b.params.as_deref(), b.ret.as_ref())?;
                    }
                    let env_ty = env_rec.with_types(rigids);
                    let fn_ty = match &b.params {
                        Some(params) => {
                            let mut seen = Vec::new();
                            let exp = match &b.name.kind {
                                PatKind::Var(x) => expected_params(self, x)?,
                                _ => None,
                            };
                            let param_tys = self.method_param_tys(lvl, exp, params);
                            let mut env2 = env_ty.clone();
                            for (p, t) in params.iter().zip(param_tys.iter()) {
                                env2 = self.elab_pat(&env2, lvl, &mut seen, *t, p)?;
                            }
                            let fn_eff = match sig_eff {
                                Some(e) => e,
                                None => self.new_row_var(lvl),
                            };
                            let ret_ty = match &b.ret {
                                Some(t) => {
                                    self.elab_value_type(&env_ty, lvl, "返り値の型注釈", t)?
                                }
                                None => self.new_var(lvl),
                            };
                            let body_ty = self.elab_exp(&env2, lvl, fn_eff, &b.body)?;
                            let sub = self.subsume_closed(lvl, body_ty, ret_ty)?;
                            self.unify(ret_ty, sub)?;
                            rec_arg_queue.push((params, param_tys.clone()));
                            let row = self.closed_item_row(&param_tys);
                            let p = self.record(row);
                            self.arrow(p, ret_ty, fn_eff)
                        }
                        None => {
                            let vty = match sig_vty {
                                Some(t) => t,
                                None => self.new_var(lvl),
                            };
                            let t0 = self.elab_exp(&env_ty, lvl, eff, &b.body)?;
                            let body_ty = self.subsume_closed(lvl, t0, vty)?;
                            if b.is_pub && !outer_eff_written(b) {
                                self.unify_pub_value_annot(vty, body_ty, &Fail::Type)?;
                            } else {
                                self.unify(vty, body_ty)?;
                            }
                            vty
                        }
                    };
                    if let Some(sh) = &snap {
                        let own: Vec<Cell> = rigids.iter().map(|(_, _, r)| *r).collect();
                        let mut opens = Vec::new();
                        collect_opens(&mut opens, sh);
                        let joint: Vec<Cell> = opens
                            .iter()
                            .copied()
                            .filter(|r| opens.iter().filter(|r2| *r2 == r).count() >= 2)
                            .collect();
                        let fr = self.fixed_rigids(&own, &joint, sh, fn_ty)?;
                        settled_before.insert(0, (b.meta.id, fr));
                    }
                    match self.unify(pre, fn_ty) {
                        Err(Fail::Type(msg)) if clash(&msg) => {
                            let note = self.group_note(
                                true,
                                b.meta.id,
                                bs,
                                &names,
                                &tps,
                                gidx,
                                &settled_before,
                            )?;
                            return match note {
                                Some(n) => terr(format!("{msg}{n}")),
                                None => terr(msg),
                            };
                        }
                        r => r?,
                    }
                    self.set_ty(b.meta.id, fn_ty);
                    let mut shared = Vec::new();
                    let mut own = Vec::new();
                    for rg in rigids.iter() {
                        let mut m = false;
                        for (_, other) in &names {
                            if *other != pre && self.mentions(rg.2, *other)? {
                                m = true;
                                break;
                            }
                        }
                        if m {
                            shared.push(rg.clone());
                        } else {
                            own.push(rg.clone());
                        }
                    }
                    self.release_rigids(&own)?;
                    shared.append(&mut group_rigids);
                    group_rigids = shared;
                    Ok(())
                })();
                let r = at(b.meta.span, r);
                match r {
                    Err(Fail::TypeAt(loc, msg)) if clash(&msg) => {
                        let note = self.group_note(
                            false,
                            b.meta.id,
                            bs,
                            &names,
                            &tps,
                            gidx,
                            &settled_before,
                        )?;
                        return Err(match note {
                            Some(n) => Fail::TypeAt(loc, format!("{msg}{n}")),
                            None => Fail::TypeAt(loc, msg),
                        });
                    }
                    r => r?,
                }
            }
            Ok(())
        })();
        let g = self.group_stack.remove(gidx);
        loop_result?;
        self.release_rigids(&group_rigids)?;
        for (params, param_tys) in &rec_arg_queue {
            self.queue_params(params, param_tys);
        }
        let tys: Vec<Ty> = names.iter().map(|(_, t)| *t).collect();
        let ws = self.ex_drain(level, &tys, true)?;
        for w in ws {
            self.warn(w);
        }
        self.check_ambiguity(false, level, &tys)?;
        for t in &tys {
            self.generalize(level, *t)?;
        }
        self.finish_group_dicts(bs, &names, &tps, &g, &member_marks)?;
        Ok(bind_names(self, env, &names))
    }

    #[allow(clippy::too_many_arguments)]
    fn group_note(
        &mut self,
        at_pre: bool,
        cur: NodeId,
        bs: &[LetBinding],
        names: &[(String, Ty)],
        tps: &[Rigids],
        gidx: usize,
        settled_before: &[(NodeId, Vec<Cell>)],
    ) -> R<Option<String>> {
        let clashed: Vec<Cell> = match self.last_rigid_clash {
            Some((t1, t2)) => {
                let mut v = Vec::new();
                for t in [t1, t2] {
                    let t = self.repr(t);
                    if let Some(r) = self.var_cell(t) {
                        v.push(r);
                    }
                }
                v
            }
            None => Vec::new(),
        };
        let ci = bs.iter().position(|b| b.meta.id == cur).expect("群の束縛");
        let (x, pre) = names[ci].clone();
        let own = &tps[ci];
        let mut flowed = Vec::new();
        for (j, (y, pre_y)) in names.iter().enumerate() {
            for (tp, _, r) in &tps[j] {
                if *pre_y != pre && clashed.contains(r) && self.mentions(*r, pre)? {
                    flowed.push(format!(
                        "{y} の型パラメータ {tp} が {x} の型に入り込んでいます"
                    ));
                }
            }
        }
        let mut callers: Vec<String> = Vec::new();
        for (_, _, from, tgt, _) in &self.group_stack[gidx].g_refs {
            if let Some(f) = from
                && *tgt == cur
                && *f != cur
            {
                let fi = bs.iter().position(|b| b.meta.id == *f).expect("群の束縛");
                callers.push(names[fi].0.clone());
            }
        }
        callers.sort();
        callers.dedup();
        let facts: Vec<String> = if !own.iter().any(|(_, _, r)| clashed.contains(r)) {
            Vec::new()
        } else if !flowed.is_empty() {
            flowed.clone()
        } else if at_pre
            && settled_before
                .iter()
                .find(|(n, _)| *n == cur)
                .map(|(_, rs)| rs.clone())
                .unwrap_or_default()
                .iter()
                .any(|r| clashed.contains(r))
        {
            callers
                .iter()
                .map(|y| format!("{y} の本体が、型パラメータを持つ {x} を参照しています"))
                .collect()
        } else {
            Vec::new()
        };
        let sections = if !flowed.is_empty() {
            "§6.3"
        } else {
            "§6.3、§18.2"
        };
        if facts.is_empty() {
            Ok(None)
        } else {
            Ok(Some(format!(
                "(再帰群の中で、前にある束縛から後ろにある束縛への参照は単相です。{}。前にある束縛の本体が後ろの束縛の型パラメータを何かの型に決める形は、型検査に通りません。{sections})",
                facts.join("。")
            )))
        }
    }

    fn finish_group_dicts(
        &mut self,
        bs: &[LetBinding],
        names: &[(String, Ty)],
        tps: &[Rigids],
        g: &GroupRec,
        member_marks: &[(NodeId, i64)],
    ) -> R<()> {
        let mut dps_of: Vec<(NodeId, Vec<(Oid, Oid)>)> = Vec::new();
        for (b, (_, t)) in bs.iter().zip(names.iter()) {
            dps_of.push((b.meta.id, self.dkeys_of(*t)?));
        }
        for ((id, dps), rigids) in dps_of.iter().zip(tps.iter()) {
            if !dps.is_empty() {
                self.set_dict(*id, DictInfo::Abs(dps.clone()));
            }
            self.check_tparam_constraints(rigids, dps)?;
        }
        let dps_for = |id: NodeId| -> Vec<(Oid, Oid)> {
            dps_of
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, d)| d.clone())
                .unwrap_or_default()
        };
        let mut ends: Vec<i64> = member_marks.iter().skip(1).map(|(_, m)| *m).collect();
        ends.push(self.hole_mark());
        for ((bn, lo), hi) in member_marks.iter().zip(ends) {
            let dps = dps_for(*bn);
            match self.solve_holes_range(*lo, hi, &dps) {
                Err(Fail::Unsolved(h)) => {
                    let hd = self.hole(h).clone();
                    return Err(Fail::TypeAt(
                        hd.h_loc.unwrap_or_default(),
                        format!(
                            "曖昧な制約: {} の辞書を、この式を含む再帰群の束縛の型から決められません(群のほかの束縛の型にだけ現れる型変数に制約が付いています。注釈で型を決めてください)",
                            self.display_of(hd.h_cls)
                        ),
                    ));
                }
                r => r?,
            }
        }
        for (node, span, from, tgt, keyed) in g.g_refs.iter().rev() {
            let need = dps_for(*tgt);
            if need.is_empty() {
                continue;
            }
            let have = match from {
                Some(f) => dps_for(*f),
                None => Vec::new(),
            };
            let mut hs = Vec::new();
            for k in &need {
                match keyed.iter().find(|(k2, _)| k2 == k) {
                    Some((_, h)) => hs.push(*h),
                    None => {
                        if !have.contains(k) {
                            return Err(Fail::TypeAt(
                                *span,
                                "曖昧な制約: 再帰群の中のこの参照が要る辞書を、呼ぶ側の束縛の型から決められません(注釈で型を決めてください)".into(),
                            ));
                        }
                        let h_id = self.new_oid();
                        let u = self.t_unit;
                        hs.push(self.add_hole(Hole {
                            h_id,
                            h_cls: k.1,
                            h_ty: u,
                            h_loc: Some(*span),
                            h_sol: Some(Evidence::Param(*k)),
                        }));
                    }
                }
            }
            self.set_dict(*node, DictInfo::Use(hs));
        }
        Ok(())
    }

    /// トップレベルの行(Diktor の `toplevel_eff`)。
    pub(crate) fn toplevel_eff(&mut self, extra: &[String]) -> Ty {
        let mut names: Vec<String> = vec!["Console".into(), "Blocking".into()];
        names.extend(extra.iter().cloned());
        let mut acc = self.row_empty;
        for n in names.iter().rev() {
            let o = self.intern(n);
            if self.prelude_owned("effect", o) {
                let u = self.t_unit;
                acc = self.extend(o, u, acc);
            }
        }
        acc
    }

    /// 初めの環境(Diktor の `initial_env`)。
    pub(crate) fn initial_env(&self) -> Env {
        let refs = self.builtin_value_refs();
        let mut values = super::pmap::PMap::new();
        let mut tops = super::pmap::PMap::new();
        for (n, t, g) in refs {
            values = values.add(Rc::from(n.as_str()), t);
            tops = tops.add(Rc::from(n.as_str()), g);
        }
        Env {
            values,
            types: super::pmap::PMap::new(),
            resume_ty: None,
            no_resume: None,
            tops,
        }
    }
}

fn clash(msg: &str) -> bool {
    has_sub("スコープ付きの型が一致しません", msg) && !has_sub("再帰群の中で、前にある束縛", msg)
}

fn collect_opens(acc: &mut Vec<Cell>, sh: &PreShape) {
    match sh {
        PreShape::Open(r) => acc.insert(0, *r),
        PreShape::Fixed(_, shs) => {
            for s in shs {
                collect_opens(acc, s);
            }
        }
        PreShape::Row(fs, tl) => {
            for (_, s) in fs {
                collect_opens(acc, s);
            }
            collect_opens(acc, tl);
        }
    }
}

/// 位置の既定値を使う(未使用の警告を避ける)。
#[allow(dead_code)]
fn _span(_: Span) {}
