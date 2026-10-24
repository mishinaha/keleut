//! 宣言の精緻化と、宣言列の処理である(Diktor の elab.ml の §11.31〜§11.41 と §11.43。§11.42 は flatten.rs)。

use super::decls::{ClassInfo, CtorInfo, DataInfo, EffectInfo, FieldInfo, UnitTable};
use super::elab::{pat_var_names, show_long_id};
use super::elab_bind::Rigids;
use super::{
    DictInfo, Env, Fail, GRef, KView, Kind, OutLine, R, TVar, Tc, Ty, TyNode, VarInfo, at, bug,
    display, terr,
};
use crate::ast::{
    ClassDecl, Decl, DeclKind, Effect, InstanceDecl, LetBinding, LongId, Newtype, PatKind,
    TypeAlias, TypeExpKind, TypeParam,
};
use std::collections::HashMap;
use std::rc::Rc;

/// クラスのメソッドの後で走らせる検査の材料(メソッドの名前、型、メソッドの型パラメータ)である。
type ClassCheck = (String, Ty, Vec<(String, Ty)>);

/// クラスのメソッドの名前と型の並びである。
type Methods = Vec<(String, Ty)>;

fn binding_name(b: &LetBinding) -> Option<String> {
    match &b.name.kind {
        PatKind::Var(x) => Some(x.clone()),
        _ => None,
    }
}

/// 投機の失敗を捨てる(Diktor の `with Type_error _ | Type_error_at _ | NotImplemented _ | NotImplemented_at _ -> ()`)。
fn swallow(r: R<()>) -> R<()> {
    match r {
        Err(Fail::Type(_) | Fail::TypeAt(..) | Fail::NotImpl(_) | Fail::NotImplAt(..)) => Ok(()),
        r => r,
    }
}

impl Tc {
    fn newtype_param_env(&mut self, env: &Env, n: &Newtype) -> (Vec<VarInfo>, Env) {
        let no = self.intern(&n.name);
        let k = self.con_kind(no, n.params.len());
        let mut head_kinds = Vec::new();
        let mut cur = k;
        let mut left = n.params.len();
        while left > 0 {
            match self.kview(cur) {
                KView::Arrow(a, r) => {
                    head_kinds.push(a);
                    cur = r;
                    left -= 1;
                }
                _ => {
                    for _ in 0..left {
                        head_kinds.push(self.new_kind_var());
                    }
                    left = 0;
                }
            }
        }
        let mut params = Vec::new();
        for (tp, hk) in n.params.iter().zip(head_kinds) {
            let kind = if tp.arity > 0 {
                self.k_arrow(tp.arity)
            } else {
                hk
            };
            params.push(VarInfo {
                vid: self.new_oid(),
                vlevel: 0,
                vkind: kind,
                vcls: Vec::new(),
            });
        }
        let mut env2 = env.clone();
        for (tp, i) in n.params.iter().zip(params.iter()) {
            let c = self.new_cell(TVar::Generic(i.clone()));
            let t = self.tvar(c);
            env2.types = env2.types.add(Rc::from(tp.name.as_str()), t);
        }
        (params, env2)
    }

    fn speculate_newtype(&mut self, env: &Env, n: &Newtype) -> R<()> {
        let (_, env2) = self.newtype_param_env(env, n);
        self.speculating = true;
        let mut r = Ok(());
        'outer: for c in &n.ctors {
            for f in &c.fields {
                let x = self.elab_type(&env2, 1, &f.ty).map(|_| ());
                if let Err(e) = swallow(x) {
                    r = Err(e);
                    break 'outer;
                }
            }
        }
        self.speculating = false;
        r
    }

    fn speculate_alias(&mut self, env: &Env, t: &TypeAlias) -> R<()> {
        let o = self.intern(&t.name);
        let Some(info) = self.d.aliases.get(&o).cloned() else {
            return Ok(());
        };
        self.speculating = true;
        let r = (|| -> R<()> {
            let rigids = self.make_rigids(Some(&info.al_kinds), 1, &info.al_params)?;
            let env_ty = env.with_types(&rigids);
            match info.al_kind.as_deref() {
                Some("EffectRow") => self.elab_eff(&env_ty, 1, &info.al_body)?,
                _ => self.elab_type(&env_ty, 1, &info.al_body)?,
            };
            Ok(())
        })();
        self.speculating = false;
        swallow(r)
    }

    fn register_newtype(&mut self, env: &Env, n: &Newtype) -> R<()> {
        let (params, env2) = self.newtype_param_env(env, n);
        let mut ctors = Vec::new();
        for c in &n.ctors {
            let mut fields = Vec::new();
            for f in &c.fields {
                if n.is_pub {
                    let sc = self.pub_scope(n.params.iter().map(|tp| tp.name.clone()).collect());
                    let x = self.unwritten_effect(&[], &sc, &f.ty);
                    at(
                        f.ty.meta.span,
                        Tc::pub_effect_error("pub な newtype のフィールド", x),
                    )?;
                }
                let ty = self.elab_type(&env2, 1, &f.ty)?;
                let what = format!("コンストラクタ {} のフィールドの型", display(&c.name));
                let r = self.check_value_kind(&what, ty);
                at(f.ty.meta.span, r)?;
                self.generalize(0, ty)?;
                let fi_label = f.label.as_ref().map(|l| self.intern(l));
                fields.push(FieldInfo {
                    fi_label,
                    fi_ty: ty,
                });
            }
            let ct_name = self.intern(&c.name);
            ctors.push(CtorInfo {
                ct_name,
                ct_fields: fields,
            });
        }
        let dd_name = self.intern(&n.name);
        self.add_data(DataInfo {
            dd_name,
            dd_params: params,
            dd_ctors: ctors,
            dd_opaque: false,
        })
    }

    fn register_effect(&mut self, env: &Env, e: &Effect) -> R<()> {
        if !e.params.is_empty() {
            return terr("effect 宣言に型パラメータは書けません(sample.kel §9)");
        }
        for (op, te) in &e.ops {
            let r = (|| -> R<()> {
                if op == "return" || op == "cancel" {
                    return terr(format!(
                        "操作名 {op} は予約されています(handle の {op} 節と衝突するため宣言できません)"
                    ));
                }
                if !op
                    .as_bytes()
                    .first()
                    .is_some_and(|c| c.is_ascii_lowercase())
                {
                    return terr(format!(
                        "操作名 {op} は英小文字で始めてください(handle の節が操作名として読めません)"
                    ));
                }
                Ok(())
            })();
            at(te.meta.span, r)?;
        }
        let mut ops = Vec::new();
        for (op, te) in &e.ops {
            match te.kind {
                TypeExpKind::Arrow(..) => {
                    let ty = self.elab_type(env, 1, te)?;
                    self.generalize(0, ty)?;
                    let o = self.intern(op);
                    ops.push((o, ty));
                }
                _ => {
                    return Err(Fail::TypeAt(
                        te.meta.span,
                        format!("操作 {op} の型は矢印型でなければなりません"),
                    ));
                }
            }
        }
        for (i, (op, _)) in e.ops.iter().enumerate() {
            if let Some((_, te2)) = e.ops[i + 1..].iter().find(|(o, _)| o == op) {
                return Err(Fail::TypeAt(
                    te2.meta.span,
                    format!("操作 {op} が二重に宣言されています"),
                ));
            }
        }
        let ef_name = self.intern(&e.name);
        self.set_effect_ops(EffectInfo {
            ef_name,
            ef_ops: ops,
        });
        Ok(())
    }

    /// 型クラスを登録する(Diktor の `register_class`)。
    ///
    /// メソッドの並びと、後で走らせる検査(メソッドの型に現れない型パラメータの制約)を返す。
    fn register_class(&mut self, env: &Env, c: &ClassDecl) -> R<(Methods, Vec<ClassCheck>)> {
        let cls = self.intern(&c.name);
        let dn = display(&c.name).to_string();
        let dno = self.intern(&dn);
        if self.reserved_predicate(dno) {
            return terr(format!(
                "{dn} は予約されたリテラル述語です(ユーザ宣言不可、D8)"
            ));
        }
        let param = match c.params.as_slice() {
            [p] => p,
            _ => {
                return terr(
                    "type class のパラメータは1個です(多パラメータ型クラスは意図的に排除、sample.kel:356)",
                );
            }
        };
        let param_kind = if param.arity > 0 {
            self.k_arrow(param.arity)
        } else {
            self.k_star
        };
        if !param.classes.is_empty() {
            return terr(
                "クラスパラメータに制約は書けません(スーパークラスは入れない裁定です。両方が要るときは [A: Eq + Ord] のように並べて書いてください)",
            );
        }
        let pinfo = VarInfo {
            vid: self.new_oid(),
            vlevel: 0,
            vkind: param_kind,
            vcls: vec![cls],
        };
        let pcell = self.new_cell(TVar::Generic(pinfo.clone()));
        let pvar = self.tvar(pcell);
        let mut methods = Vec::new();
        let mut checks = Vec::new();
        for v in &c.vals {
            let mut mt_params = Vec::new();
            for tp in &v.tparams {
                let kind = if tp.arity > 0 {
                    self.k_arrow(tp.arity)
                } else {
                    self.new_kind_var()
                };
                let mut classes = Vec::new();
                for li in &tp.classes {
                    let o = self.intern(&show_long_id(li));
                    classes.push(self.resolve_class(o));
                }
                let vid = self.new_oid();
                let cell = self.new_cell(TVar::Generic(VarInfo {
                    vid,
                    vlevel: 0,
                    vkind: kind,
                    vcls: classes,
                }));
                mt_params.push((tp.name.clone(), self.tvar(cell)));
            }
            let mut env2 = env.clone();
            env2.types = env2.types.add(Rc::from(param.name.as_str()), pvar);
            for (n, t) in &mt_params {
                env2.types = env2.types.add(Rc::from(n.as_str()), *t);
            }
            let ty = self.elab_type_outer(&env2, 1, &v.ty)?;
            let rt = self.repr(ty);
            let ty = match (&v.ty.kind, self.node(rt)) {
                (TypeExpKind::Arrow(_, _, None), TyNode::Arrow(a, r, _)) => {
                    let e = self.row_empty;
                    self.arrow(a, r, e)
                }
                _ => ty,
            };
            self.generalize(0, ty)?;
            for (_, t) in &mt_params {
                let rt = self.repr(*t);
                if let Some(cell) = self.var_cell(rt) {
                    let k = self.var_info_of(cell)?.vkind;
                    self.default_kind(k)?;
                }
            }
            let chk = (|| -> R<()> {
                let rt = self.repr(ty);
                match self.node(rt) {
                    TyNode::Arrow(..) => {
                        if !self.dkeys_of(ty)?.iter().any(|(vid, _)| *vid == pinfo.vid) {
                            return terr(format!(
                                "メソッド {} の型にクラスパラメータが現れません(どのインスタンスのメソッドかを型から決められません)",
                                v.name
                            ));
                        }
                        Ok(())
                    }
                    _ => terr(format!(
                        "メソッド {} の型は矢印型でなければなりません",
                        v.name
                    )),
                }
            })();
            at(v.ty.meta.span, chk)?;
            checks.push((v.name.clone(), ty, mt_params));
            methods.push((v.name.clone(), ty));
        }
        for (i, v) in c.vals.iter().enumerate() {
            if let Some(w) = c.vals[i + 1..].iter().find(|w| w.name == v.name) {
                return Err(Fail::TypeAt(
                    w.ty.meta.span,
                    format!("メソッド {} が二重に宣言されています", v.name),
                ));
            }
        }
        for (m, _) in &methods {
            let order = self.d.class_order.clone();
            for o in order {
                let other = self.d.classes[&o].clone();
                if self.display_of(other.ci_name) != self.display_of(cls)
                    && self.visible_in_unit(UnitTable::Classes, other.ci_name)
                    && other.ci_methods.iter().any(|(n, _)| n == m)
                {
                    return terr(format!(
                        "メソッド名 {m} は型クラス {} が既に宣言しています(非修飾名が衝突するため、v0 では同名メソッドを複数のクラスに宣言できません)",
                        self.display_of(other.ci_name)
                    ));
                }
            }
        }
        self.add_class_decl(ClassInfo {
            ci_name: cls,
            ci_param: pinfo,
            ci_param_kind: param_kind,
            ci_derive_structural: false,
            ci_builtin: false,
            ci_methods: methods.clone(),
        })?;
        Ok((methods, checks))
    }

    /// クラスのメソッドの型に現れない型パラメータの制約を調べる(`register_class` が返す検査)。
    fn run_class_check(&mut self, name: &str, ty: Ty, mt_params: &[(String, Ty)]) -> R<()> {
        let dks = self.dkeys_of(ty)?;
        for (pn, t) in mt_params {
            let rt = self.repr(*t);
            if let Some(cell) = self.var_cell(rt) {
                let i = self.var_info_of(cell)?;
                for c in &i.vcls {
                    if !self.is_predicate(*c) && !dks.contains(&(i.vid, *c)) {
                        return terr(format!(
                            "曖昧な制約: メソッド {name} の型パラメータ {pn} の制約 {} をメソッドの型から決められません(メソッドの型に現れない型パラメータに制約が付いています)",
                            self.display_of(*c)
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn instance_head(&mut self, i: &InstanceDecl) -> R<(super::Oid, super::Oid, usize)> {
        let co = self.intern(&i.class);
        let cls = self.resolve_class(co);
        let head = match i.args.as_slice() {
            [h] => h,
            _ => return terr("type instance の型引数は1個です(D11)"),
        };
        let (con, holes) = match &head.kind {
            TypeExpKind::Ident(LongId(comps)) => {
                let o = self.intern(&comps.join("."));
                (self.resolve_con(o), 0)
            }
            TypeExpKind::Apply(h, args) if matches!(&h.kind, TypeExpKind::Ident(_)) => {
                let TypeExpKind::Ident(LongId(comps)) = &h.kind else {
                    unreachable!()
                };
                for a in args {
                    if !matches!(a.kind, TypeExpKind::Hole) {
                        return terr("インスタンス頭の型引数は _ だけです(List[_] の形)");
                    }
                }
                let o = self.intern(&comps.join("."));
                (self.resolve_con(o), args.len())
            }
            _ => return terr("インスタンス頭は 型構成子 か 型構成子[_, ...] の形で書いてください"),
        };
        self.check_con_visible(con)?;
        let k = self.con_kind(con, holes);
        let arity = self.kind_arity(k)?;
        if holes != arity {
            return terr(format!(
                "インスタンス頭 {} は型引数を {arity} 個取りますが、_ が {holes} 個書かれています",
                self.display_of(con)
            ));
        }
        let np = i.tparams.len();
        if np > holes {
            return terr(format!(
                "インスタンスの型パラメータが {np} 個ありますが、頭 {} の _ は {holes} 個です",
                self.display_of(con)
            ));
        }
        Ok((cls, con, holes))
    }

    fn register_instance(&mut self, i: &InstanceDecl) -> R<()> {
        let (cls, con, holes) = self.instance_head(i)?;
        if self.reserved_predicate(cls) {
            return terr(format!(
                "{} は予約されたリテラル述語です(インスタンスは宣言できません、D8)",
                i.class
            ));
        }
        let Some(ci) = self.find_class(cls) else {
            return terr(format!("未知のクラス: {}", i.class));
        };
        if !self.d.con_kinds.contains_key(&con) {
            return terr(format!("未知の型構成子: {}", self.display_of(con)));
        }
        let mut k = self.con_kind(con, holes);
        for tp in &i.tparams {
            match self.kview(k) {
                KView::Arrow(a, r) => {
                    let kb = if tp.arity > 0 {
                        self.k_arrow(tp.arity)
                    } else {
                        self.new_kind_var()
                    };
                    if !self.same_kind(a, kb)? {
                        return terr(format!(
                            "インスタンスの型パラメータ {} のカインドが頭 {} の引数と一致しません",
                            tp.name,
                            self.display_of(con)
                        ));
                    }
                    k = r;
                }
                _ => {
                    return bug(
                        "instance: 束縛子が頭のアリティを超えています(D96 の検査が先に落とすはず)",
                    );
                }
            }
        }
        if !self.same_kind(ci.ci_param_kind, k)? {
            return terr(format!(
                "インスタンス頭 {} のカインドがクラス {} のパラメータと一致しません",
                self.display_of(con),
                i.class
            ));
        }
        let mut premises = Vec::new();
        for (idx, tp) in i.tparams.iter().enumerate() {
            for c in self.class_names_of(tp)? {
                premises.push((idx, c));
            }
        }
        let mut methods: Vec<super::Oid> = Vec::new();
        for d in &i.body {
            let mut name_of_b = |tc: &mut Tc, b: &LetBinding| -> R<()> {
                match binding_name(b) {
                    Some(x) => {
                        methods.push(tc.intern(&x));
                        Ok(())
                    }
                    None => terr("インスタンス本体の let は名前束縛でなければなりません"),
                }
            };
            match &d.kind {
                DeclKind::Let(b) => name_of_b(self, b)?,
                DeclKind::LetRec(bs) => {
                    for b in bs {
                        name_of_b(self, b)?;
                    }
                }
                _ => return terr("インスタンス本体には let(と let rec)だけが書けます"),
            }
        }
        for m in &methods {
            let mut found = false;
            for (m2, _) in &ci.ci_methods {
                if self.intern(m2) == *m {
                    found = true;
                    break;
                }
            }
            if !found {
                return terr(format!(
                    "クラス {} にメソッド {} はありません",
                    i.class,
                    self.display_of(*m)
                ));
            }
        }
        for (m, _) in &ci.ci_methods {
            let mo = self.intern(m);
            if !methods.contains(&mo) {
                return terr(format!(
                    "インスタンスがメソッドを網羅していません: {m} が漏れています"
                ));
            }
        }
        if !self.d.in_prelude && self.find_instance(cls, con).is_none() {
            let cur = self.d.current_unit;
            if self.entity_unit(cls) != cur && self.entity_unit(con) != cur {
                let c = self.display_of(cls);
                let t = self.display_of(con);
                return terr(format!(
                    "{c}[{t}] のインスタンスは、{c} か {t} を宣言したファイルにだけ書けます"
                ));
            }
        }
        self.add_instance(false, cls, con, premises)
    }

    /// 完全な注釈を持つ束縛の署名(Diktor の `signature_of_binding`)。
    fn signature_of_binding(&mut self, env: &Env, b: &LetBinding) -> R<Option<Ty>> {
        let params_annotated = match &b.params {
            None => true,
            Some(ps) => ps.iter().all(|p| matches!(p.kind, PatKind::Annot(..))),
        };
        let head_effected = |t: &crate::ast::TypeExp| match &t.kind {
            TypeExpKind::Arrow(_, _, eff) => eff.is_some(),
            _ => true,
        };
        let full = params_annotated
            && b.ret.is_some()
            && match &b.params {
                Some(_) => b.eff.is_some() || b.is_pub,
                None => match &b.ret {
                    Some(t) => head_effected(t) || b.is_pub,
                    None => false,
                },
            };
        if !full {
            return Ok(None);
        }
        let r = (|| -> R<Ty> {
            let lvl = 1;
            let rigids = self.make_rigids(None, lvl, &b.tparams)?;
            let env_ty = env.with_types(&rigids);
            let ty = match &b.params {
                Some(ps) => {
                    let mut param_tys = Vec::new();
                    for p in ps {
                        match &p.kind {
                            PatKind::Annot(_, te) => {
                                param_tys.push(self.elab_value_type(&env_ty, lvl, "型注釈", te)?)
                            }
                            _ => return bug("assert false"),
                        }
                    }
                    let fn_eff = match &b.eff {
                        Some(e) => self.elab_eff(&env_ty, lvl, e)?,
                        None => self.row_empty,
                    };
                    let ret_ty = match &b.ret {
                        Some(t) => self.elab_value_type(&env_ty, lvl, "返り値の型注釈", t)?,
                        None => return bug("assert false"),
                    };
                    let row = self.closed_item_row(&param_tys);
                    let p = self.record(row);
                    self.arrow(p, ret_ty, fn_eff)
                }
                None => match &b.ret {
                    Some(t) => {
                        let ty = self.elab_value_type_outer(&env_ty, lvl, "型注釈", t)?;
                        let rt = self.repr(ty);
                        match (&t.kind, self.node(rt)) {
                            (TypeExpKind::Arrow(_, _, None), TyNode::Arrow(a, r, _))
                                if b.is_pub =>
                            {
                                let e = self.row_empty;
                                self.arrow(a, r, e)
                            }
                            _ => ty,
                        }
                    }
                    None => return bug("assert false"),
                },
            };
            self.generalize(0, ty)?;
            self.release_rigids(&rigids)?;
            Ok(ty)
        })();
        match r {
            Ok(t) => Ok(Some(t)),
            Err(Fail::Type(_) | Fail::TypeAt(..) | Fail::NotImpl(_) | Fail::NotImplAt(..)) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    fn expected_of(&mut self, ci: &ClassInfo, head_ty: Ty, mname: &str) -> R<Ty> {
        match ci.ci_methods.iter().find(|(n, _)| n == mname) {
            Some((_, scheme)) => {
                let mut memo = HashMap::new();
                memo.insert(ci.ci_param.vid, head_ty);
                self.map_generics_with(
                    &mut memo,
                    &mut |tc, info| {
                        let c = tc.new_cell(TVar::Generic(info.clone()));
                        tc.tvar(c)
                    },
                    *scheme,
                )
            }
            None => bug("instance: メソッドスキーマ未登録"),
        }
    }

    fn expected_params_of(
        &mut self,
        ci: &ClassInfo,
        head_ty: Ty,
        mname: &str,
    ) -> R<Option<Vec<Ty>>> {
        let e = self.expected_of(ci, head_ty, mname)?;
        let e = self.repr(e);
        match self.node(e) {
            TyNode::Arrow(p, _, _) => {
                let mut memo = HashMap::new();
                let t = self.map_generics_with(
                    &mut memo,
                    &mut |tc, i| tc.new_var_k(i.vkind, Vec::new(), 1),
                    p,
                )?;
                let t = self.repr(t);
                match self.node(t) {
                    TyNode::Record(row) => {
                        let (fields, tail) = self.row_fields(row);
                        if self.repr_is_empty(tail) && fields.iter().all(|(l, _)| *l == self.l_item)
                        {
                            Ok(Some(fields.into_iter().map(|(_, t)| t).collect()))
                        } else {
                            Ok(None)
                        }
                    }
                    _ => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn instance_subsume(
        &mut self,
        ci: &ClassInfo,
        head_ty: Ty,
        pk: &[(super::Oid, super::Oid)],
        b: &LetBinding,
        mname: &str,
        inferred: Ty,
    ) -> R<()> {
        let lvl = 1;
        let expected = self.expected_of(ci, head_ty, mname)?;
        let mut smemo = HashMap::new();
        let skol = self.map_generics_with(
            &mut smemo,
            &mut |tc, i| tc.new_rigid(i.vkind, i.vcls.clone(), lvl),
            expected,
        )?;
        let mut mk = Vec::new();
        for (gi, c) in self.constraints_of(expected)? {
            let t = smemo[&gi.vid];
            let t = self.repr(t);
            match self.var_cell(t) {
                Some(r) => mk.push((self.var_info_of(r)?.vid, c)),
                None => return bug("instance: 剛定数ではありません"),
            }
        }
        let amark = self.hole_mark();
        let (inf, adapter) = self.instantiate_ev(b.meta.span, lvl, inferred)?;
        let wrap = |r: R<()>| match r {
            Err(Fail::Type(msg)) => terr(format!(
                "インスタンスメソッド {mname} がクラス宣言の型を満たしません({msg})"
            )),
            r => r,
        };
        let ri = self.repr(inf);
        let rs = self.repr(skol);
        match (self.node(ri), self.node(rs)) {
            (TyNode::Arrow(ia, ir, ie), TyNode::Arrow(sa, sr, se)) => {
                let r = self.check_arity(inf, skol, ia, sa);
                wrap(r)?;
                let r = self.unify(ia, sa);
                wrap(r)?;
                let r = self.unify(ir, sr);
                wrap(r)?;
                let (_, tail) = self.row_fields(ie);
                let ie = if self.repr_is_empty(tail) && !self.repr_is_empty(ie) {
                    let v = self.new_row_var(lvl);
                    self.row_append(ie, v)?
                } else {
                    ie
                };
                let rie = self.repr(ie);
                if let Some(c) = self.var_cell(rie)
                    && matches!(self.cell(c), TVar::Unbound(_))
                {
                    let e = self.row_empty;
                    self.unify(ie, e)?;
                }
                if !self.repr_is_empty(ie) {
                    let (sfs, stail) = self.row_fields(se);
                    let bare = sfs.is_empty() && {
                        let st = self.repr(stail);
                        matches!(self.node_ref(st), TyNode::Var(_) | TyNode::RowEmpty)
                    };
                    let culprit = if super::elab::outer_eff_written(b) {
                        " の注釈に @ {} 以外の行を書いています。元の報告: "
                    } else {
                        match b.params {
                            Some(_) => " の本体がエフェクトを起こしています。元の報告: ",
                            None => " の初期化式の値の型の最外の行が空ではありません。元の報告: ",
                        }
                    };
                    match self.unify(ie, se) {
                        Err(Fail::Type(msg)) => {
                            if bare && row_failure_msg(&msg) {
                                return terr(format!(
                                    "型クラスのメソッドの実装は純粋でなければなりません(宣言の最外の行にラベルがありません — 仕様 §9)。インスタンスメソッド {mname}{culprit}{msg}"
                                ));
                            } else {
                                return terr(format!(
                                    "インスタンスメソッド {mname} がクラス宣言の型を満たしません({msg})"
                                ));
                            }
                        }
                        r => r?,
                    }
                }
            }
            _ => {
                let r = self.unify(inf, skol);
                wrap(r)?;
            }
        }
        let mut params = mk.clone();
        params.extend_from_slice(pk);
        self.solve_holes_at(amark, &params)?;
        let own = match self.dict.get(&b.meta.id) {
            Some(DictInfo::Abs(ks)) => ks.clone(),
            _ => Vec::new(),
        };
        self.set_dict(b.meta.id, DictInfo::Method(own, mk, adapter));
        Ok(())
    }

    fn check_init_row(
        &mut self,
        b: &LetBinding,
        pk: &[(super::Oid, super::Oid)],
        eff0: Ty,
        mname: &str,
        rho: Ty,
    ) -> R<()> {
        let mk = match self.dict.get(&b.meta.id) {
            Some(DictInfo::Method(_, mk, _)) => mk.clone(),
            _ => Vec::new(),
        };
        let per_use = !pk.is_empty() || !mk.is_empty();
        let allowed = if per_use { self.row_empty } else { eff0 };
        let top: Vec<super::Oid> = self
            .row_fields(allowed)
            .0
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        let extra: Vec<super::Oid> = self
            .row_fields(rho)
            .0
            .into_iter()
            .map(|(l, _)| l)
            .filter(|l| !top.contains(l))
            .collect();
        if !extra.is_empty() {
            let names: Vec<String> = extra.iter().map(|l| self.display_of(*l)).collect();
            let names = names.join("、");
            return if per_use {
                terr(format!(
                    "エフェクト {names} をインスタンスメソッド {mname} の初期化式では実行できません(メソッドを使うたびに評価する初期化式は、エフェクトを起こせません)"
                ))
            } else {
                terr(format!(
                    "エフェクト {names} をインスタンスメソッド {mname} の初期化式では実行できません(初期化式が起こせるのはトップレベルの評価と同じエフェクトです)"
                ))
            };
        }
        let (_, tail) = self.row_fields(rho);
        let tail = self.repr(tail);
        if self.var_cell(tail).is_some() {
            let e = self.row_empty;
            self.unify(tail, e)?;
        }
        Ok(())
    }

    fn check_instance_bodies(
        &mut self,
        env: &Env,
        eff0: Ty,
        inode: &Decl,
        i: &InstanceDecl,
    ) -> R<()> {
        let (cls, con, _) = self.instance_head(i)?;
        let Some(ci) = self.find_class(cls) else {
            return bug("instance: class 未登録");
        };
        let imark = self.hole_mark();
        let head_rigids = self.make_rigids(None, 1, &i.tparams)?;
        let head_ty = self.con(con, head_rigids.iter().map(|(_, t, _)| *t).collect());
        let mut pk = Vec::new();
        for (_, _, r) in &head_rigids {
            let info = self.var_info_of(*r)?;
            for c in &info.vcls {
                if !self.is_predicate(*c) {
                    pk.push((info.vid, *c));
                }
            }
        }
        self.set_dict(inode.meta.id, DictInfo::Instance(pk.clone()));
        let env = env.with_types(&head_rigids);
        for d in &i.body {
            match &d.kind {
                DeclKind::Let(b) => {
                    let Some(mname) = binding_name(b) else {
                        return bug("instance: 名前なし");
                    };
                    let rho = self.new_row_var(0);
                    let ep = self.expected_params_of(&ci, head_ty, &mname)?;
                    let env2 = self.elab_binding(ep, &env, 0, rho, b)?;
                    let inferred = *env2.values.get(mname.as_str()).expect("メソッド");
                    let r = self.instance_subsume(&ci, head_ty, &pk, b, &mname, inferred);
                    at(b.meta.span, r)?;
                    let r = self.check_init_row(b, &pk, eff0, &mname, rho);
                    at(b.meta.span, r)?;
                }
                DeclKind::LetRec(bs) => {
                    let rho = self.new_row_var(0);
                    let ci2 = ci.clone();
                    let ep = move |tc: &mut Tc, m: &str| tc.expected_params_of(&ci2, head_ty, m);
                    let env2 = self.elab_rec_bindings(false, &ep, &env, 0, rho, bs)?;
                    for b in bs {
                        let Some(mname) = binding_name(b) else {
                            return bug("instance: 名前なし");
                        };
                        let inferred = *env2.values.get(mname.as_str()).expect("メソッド");
                        let r = self.instance_subsume(&ci, head_ty, &pk, b, &mname, inferred);
                        at(b.meta.span, r)?;
                    }
                    for b in bs {
                        let Some(mname) = binding_name(b) else {
                            return bug("instance: 名前なし");
                        };
                        let r = self.check_init_row(b, &pk, eff0, &mname, rho);
                        at(b.meta.span, r)?;
                    }
                }
                _ => return terr("インスタンス本体には let だけが書けます"),
            }
        }
        self.solve_holes_at(0.max(imark), &pk)?;
        self.release_rigids(&head_rigids)?;
        Ok(())
    }

    fn register_companion(&mut self, tyname: &str) -> R<()> {
        let Some(sm) = self.d.current_module.clone() else {
            return Ok(());
        };
        let m = display(&sm).to_string();
        if tyname != format!("{sm}.{m}") {
            return Ok(());
        }
        let taken = |tc: &mut Tc, which: u8| {
            let a = tc.intern(&sm);
            let has = |tc: &Tc, o| match which {
                0 => tc.d.con_kinds.contains_key(&o),
                1 => tc.d.aliases.contains_key(&o),
                _ => tc.d.effects.contains_key(&o),
            };
            if has(tc, a) {
                return true;
            }
            let b = tc.intern(&m);
            has(tc, b)
        };
        let mo = |tc: &mut Tc| {
            let o = tc.intern(&m);
            tc.d.reserved_type_names.contains(&o)
        };
        if taken(self, 0) || taken(self, 1) || mo(self) {
            terr(format!(
                "module {m} のコンパニオン型 {m} は既存の型 {m} と同名です(module 内の型とトップレベルの型は同名にできません)"
            ))
        } else if taken(self, 2) {
            terr(format!(
                "module {m} のコンパニオン型 {m} は既存のエフェクト {m} と同名です(型とエフェクトは同名にできません)"
            ))
        } else if super::decls::marks_unit(self.d.current_unit) {
            let t = self.intern(tyname);
            let mo = self.intern(&m);
            self.d.unit_synonyms.insert((self.d.current_unit, mo), t);
            Ok(())
        } else {
            let t = self.intern(tyname);
            let mo = self.intern(&m);
            self.add_con_synonym(mo, t);
            Ok(())
        }
    }

    fn no_param_constraints(what: &str, tparams: &[TypeParam]) -> R<()> {
        for tp in tparams {
            if !tp.classes.is_empty() {
                return terr(format!(
                    "{what} の型パラメータ {} には型クラスの制約を書けません",
                    tp.name
                ));
            }
        }
        Ok(())
    }

    fn check_signature_dicts(&mut self, span: crate::pos::Span, fn_ty: Ty, sg: Ty) -> R<()> {
        let a: Vec<super::Oid> = self.dkeys_of(fn_ty)?.into_iter().map(|(_, c)| c).collect();
        let b: Vec<super::Oid> = self.dkeys_of(sg)?.into_iter().map(|(_, c)| c).collect();
        if a != b {
            return at(span, bug("署名と本体の辞書パラメータが一致しません"));
        }
        Ok(())
    }

    fn show_binding_line(&mut self, env: &Env, b: &LetBinding) -> R<()> {
        if let Some(x) = binding_name(b)
            && let Some(t) = env.values.get(x.as_str()).copied()
        {
            let s = self.show(t)?;
            self.current_out
                .push(OutLine::Binding(format!("{} : {s}", display(&x))));
        }
        Ok(())
    }

    /// 宣言列を処理する(Diktor の `process_decls`)。
    pub(crate) fn process_decls(
        &mut self,
        toplevel_extra: &[String],
        env: &Env,
        decls: &[Decl],
        emit: bool,
    ) -> R<Env> {
        let eff0 = self.toplevel_eff(toplevel_extra);
        // パス 1a: 型の名前とカインドの頭、エフェクトの名前。
        for node in decls {
            let r = self.with_decl_scope(node.meta.id, |tc| match &node.kind {
                DeclKind::Type(t) => {
                    Tc::no_param_constraints(
                        &format!("型エイリアス {}", display(&t.name)),
                        &t.params,
                    )?;
                    tc.register_companion(&t.name)?;
                    let al_name = tc.intern(&t.name);
                    let al_kinds = t
                        .params
                        .iter()
                        .map(|tp| {
                            if tp.arity > 0 {
                                tc.k_arrow(tp.arity)
                            } else {
                                tc.new_kind_var()
                            }
                        })
                        .collect();
                    tc.add_alias(super::AliasInfo {
                        al_name,
                        al_params: t.params.clone(),
                        al_kinds,
                        al_kind: t.kind.clone(),
                        al_body: t.body.clone(),
                        al_module: tc.d.current_module.clone(),
                        al_unit: tc.d.current_unit,
                    })
                }
                DeclKind::Newtype(n) => {
                    Tc::no_param_constraints(&format!("newtype {}", display(&n.name)), &n.params)?;
                    let no = tc.intern(&n.name);
                    tc.claim_type_name("newtype", no)?;
                    tc.register_companion(&n.name)?;
                    let no = tc.intern(&n.name);
                    if tc.prelude_owned("data", no) && !tc.d.in_prelude {
                        return terr(format!(
                            "標準環境の newtype {} は再宣言できません",
                            display(&n.name)
                        ));
                    }
                    let mut k = tc.k_star;
                    for tp in n.params.iter().rev() {
                        let a = if tp.arity > 0 {
                            tc.k_arrow(tp.arity)
                        } else {
                            tc.new_kind_var()
                        };
                        k = tc.k_arrow2(a, k);
                    }
                    let no = tc.intern(&n.name);
                    tc.d.con_kinds.insert(no, k);
                    Ok(())
                }
                DeclKind::Effect(e) => {
                    let o = tc.intern(&e.name);
                    tc.declare_effect(o)
                }
                _ => Ok(()),
            });
            at(node.meta.span, r)?;
        }
        // パス 1a の 2 周目: カインドが決まるまで投機する。
        let mut prev = self.kinds_settled(decls)?;
        loop {
            for node in decls {
                let r = self.with_decl_scope(node.meta.id, |tc| match &node.kind {
                    DeclKind::Newtype(n) => tc.speculate_newtype(env, n),
                    DeclKind::Type(t) => tc.speculate_alias(env, t),
                    _ => Ok(()),
                });
                at(node.meta.span, r)?;
            }
            let now = self.kinds_settled(decls)?;
            if now > prev {
                prev = now;
            } else {
                break;
            }
        }
        // パス 1b: newtype、effect、type class の登録。
        let mut class_checks: Vec<(usize, Vec<ClassCheck>)> = Vec::new();
        let mut env = env.clone();
        for (idx, node) in decls.iter().enumerate() {
            let r = self.with_decl_scope(node.meta.id, |tc| match &node.kind {
                DeclKind::Newtype(n) => tc.register_newtype(&env, n).map(|_| None),
                DeclKind::Effect(e) => tc.register_effect(&env, e).map(|_| None),
                DeclKind::Class(c) => {
                    let (methods, checks) = tc.register_class(&env, c)?;
                    let mut e2 = env.clone();
                    for (mn, ty) in &methods {
                        let q = format!("{}.{mn}", c.name);
                        e2 = tc.add_top(&e2, &q, *ty, GRef::Method(q.clone()));
                        if !e2.values.contains(mn.as_str()) {
                            e2 = tc.add_top(&e2, mn, *ty, GRef::Method(q.clone()));
                        }
                    }
                    Ok(Some((e2, checks)))
                }
                _ => Ok(None),
            });
            if let Some((e2, checks)) = at(node.meta.span, r)? {
                env = e2;
                class_checks.push((idx, checks));
            }
        }
        // 1b の後始末: クラス名の検査と、エイリアスの投機。
        for (idx, node) in decls.iter().enumerate() {
            let r = self.with_decl_scope(node.meta.id, |tc| match &node.kind {
                DeclKind::Class(c) => {
                    for v in &c.vals {
                        for tp in &v.tparams {
                            tc.class_names_of(tp)?;
                        }
                    }
                    if let Some((_, checks)) = class_checks.iter().find(|(i, _)| *i == idx) {
                        for (name, ty, mt) in checks.clone() {
                            tc.run_class_check(&name, ty, &mt)?;
                        }
                    }
                    Ok(())
                }
                DeclKind::Type(t) => tc.speculate_alias(&env, t),
                _ => Ok(()),
            });
            at(node.meta.span, r)?;
        }
        // 1b の後始末の 2 周目: カインドの既定化。
        for node in decls {
            let r = self.with_decl_scope(node.meta.id, |tc| -> R<()> {
                match &node.kind {
                    DeclKind::Newtype(n) => {
                        let no = tc.intern(&n.name);
                        let k = tc.con_kind(no, n.params.len());
                        tc.default_kind(k)?;
                        if let Some(dd) = tc.d.datas.get(&no).cloned() {
                            for i in &dd.dd_params {
                                tc.default_kind(i.vkind)?;
                            }
                            for (tp, i) in n.params.iter().zip(dd.dd_params.iter()) {
                                if let KView::Arrow(..) = tc.kview(i.vkind)
                                    && tc.codomain_is_row(i.vkind)?
                                {
                                    return terr(format!(
                                        "newtype {} の型パラメータ {} への適用が行として使われています(行を返す型構成子はありません)",
                                        n.name, tp.name
                                    ));
                                }
                            }
                        }
                        Ok(())
                    }
                    DeclKind::Type(t) => {
                        let o = tc.intern(&t.name);
                        if let Some(info) = tc.d.aliases.get(&o).cloned() {
                            for k in &info.al_kinds {
                                tc.default_kind(*k)?;
                            }
                            for (tp, k) in info.al_params.iter().zip(info.al_kinds.iter()) {
                                if let KView::Arrow(..) = tc.kview(*k)
                                    && tc.codomain_is_row(*k)?
                                {
                                    return terr(format!(
                                        "型エイリアス {} の型パラメータ {} への適用が行として使われています(行を返す型構成子はありません)",
                                        t.name, tp.name
                                    ));
                                }
                            }
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            });
            at(node.meta.span, r)?;
        }
        // パス 1c: インスタンスと署名。
        for node in decls {
            let r = self.with_decl_scope(node.meta.id, |tc| -> R<Option<Env>> {
                match &node.kind {
                    DeclKind::Instance(i) => {
                        tc.register_instance(i)?;
                        Ok(None)
                    }
                    DeclKind::Let(b) => {
                        let sg = tc.signature_of_binding(&env, b)?;
                        match (binding_name(b), sg) {
                            (Some(x), Some(ty)) if !env.values.contains(x.as_str()) => Ok(Some(
                                tc.add_top(&env, &x, ty, GRef::Decl(b.meta.id, x.clone())),
                            )),
                            _ => Ok(None),
                        }
                    }
                    DeclKind::LetRec(bs) => {
                        let mut e = env.clone();
                        for b in bs {
                            let sg = tc.signature_of_binding(&e, b)?;
                            if let (Some(x), Some(ty)) = (binding_name(b), sg)
                                && !e.values.contains(x.as_str())
                            {
                                e = tc.add_top(&e, &x, ty, GRef::Decl(b.meta.id, x.clone()));
                            }
                        }
                        Ok(Some(e))
                    }
                    _ => Ok(None),
                }
            });
            if let Some(e) = at(node.meta.span, r)? {
                env = e;
            }
        }
        self.unify_reset();
        for node in decls {
            env = self.step(&env, eff0, node, emit)?;
        }
        if let Some(h) = self.unsolved_holes_oldest_first().first().copied() {
            let hd = self.hole(h).clone();
            return Err(Fail::TypeAt(
                hd.h_loc.unwrap_or_default(),
                format!(
                    "曖昧な制約: {} を満たす型が決まりません(一般化しない束縛か式文の型変数です。注釈で型を決めてください)",
                    self.display_of(hd.h_cls)
                ),
            ));
        }
        Ok(env)
    }

    /// カインドの値域が行か(Diktor の `codomain k = KRow`)。
    ///
    /// Diktor の `codomain` は末尾再帰なので、循環したカインドでは止まらない。
    /// ここでは辿る回数に上限を置き、超えたらスタックの不足と同じ処理系の限度にする。
    fn codomain_is_row(&mut self, k: Kind) -> R<bool> {
        let mut cur = k;
        for _ in 0..(1u32 << 24) {
            match self.kview(cur) {
                KView::Arrow(_, r) => cur = r,
                KView::Row => return Ok(true),
                _ => return Ok(false),
            }
        }
        Err(Fail::Limit)
    }

    /// 矢印のカインドの、決まった引数のカインドの数(Diktor の `settled` の `count`)。
    fn count_known(&mut self, k: Kind) -> R<usize> {
        super::guard()?;
        match self.kview(k) {
            KView::Arrow(a, r) => {
                let known = usize::from(!matches!(self.kview(a), KView::Var(_)));
                Ok(known + self.count_known(r)?)
            }
            _ => Ok(0),
        }
    }

    fn kinds_settled(&mut self, decls: &[Decl]) -> R<usize> {
        let mut acc = 0;
        for node in decls {
            let r = self.with_decl_scope(node.meta.id, |tc| -> R<usize> {
                Ok(match &node.kind {
                    DeclKind::Newtype(n) => {
                        let no = tc.intern(&n.name);
                        let k = tc.con_kind(no, n.params.len());
                        tc.count_known(k)?
                    }
                    DeclKind::Type(t) => {
                        let o = tc.intern(&t.name);
                        match tc.d.aliases.get(&o).cloned() {
                            Some(info) => info
                                .al_kinds
                                .iter()
                                .filter(|k| !matches!(tc.kview(**k), KView::Var(_)))
                                .count(),
                            None => 0,
                        }
                    }
                    _ => 0,
                })
            });
            acc += r?;
        }
        Ok(acc)
    }

    /// 宣言の 1 歩(Diktor の `process_decls` の `step`)。
    fn step(&mut self, env: &Env, eff0: Ty, node: &Decl, emit: bool) -> R<Env> {
        let wbefore = self.warnings.len();
        let r = self.with_decl_scope(node.meta.id, |tc| -> R<Env> {
            let env2 = tc.step_inner(env, eff0, node, emit)?;
            tc.default_numerics()?;
            tc.check_pattern_numbers()?;
            tc.solve_holes_at(0, &[])?;
            tc.opened_rows.clear();
            Ok(env2)
        });
        let env2 = at(node.meta.span, r)?;
        let fresh: Vec<String> = self.warnings[wbefore..].to_vec();
        if emit {
            for w in fresh {
                self.current_out.push(OutLine::Warning(w));
            }
        }
        Ok(env2)
    }

    fn step_inner(&mut self, env: &Env, eff0: Ty, node: &Decl, emit: bool) -> R<Env> {
        match &node.kind {
            DeclKind::Type(t) => {
                let o = self.intern(&t.name);
                let info = self.d.aliases[&o].clone();
                let lvl = 1;
                let rigids = self.make_rigids(Some(&info.al_kinds), lvl, &info.al_params)?;
                let env_ty = env.with_types(&rigids);
                match info.al_kind.as_deref() {
                    Some("EffectRow") => {
                        self.elab_eff(&env_ty, lvl, &info.al_body)?;
                    }
                    _ => {
                        let ty = self.elab_type(&env_ty, lvl, &info.al_body)?;
                        let rt = self.repr(ty);
                        let bare_param = match self.var_cell(rt) {
                            Some(r2) => rigids.iter().any(|(_, _, r)| *r == r2),
                            None => false,
                        };
                        if !bare_param {
                            let r = self.check_alias_body_kind(display(&t.name), ty);
                            at(info.al_body.meta.span, r)?;
                        }
                    }
                }
                self.release_rigids(&rigids)?;
                Ok(env.clone())
            }
            DeclKind::Let(b) => {
                let env2 = self.elab_binding(None, env, 0, eff0, b)?;
                let sg = self.signature_of_binding(env, b)?;
                if let Some(x) = binding_name(b)
                    && let Some(sg) = sg
                {
                    let ft = *env2.values.get(x.as_str()).expect("束縛");
                    self.check_signature_dicts(b.meta.span, ft, sg)?;
                }
                let mut env3 = env2;
                for x in pat_var_names(&b.name) {
                    let g = GRef::Decl(b.meta.id, x.clone());
                    if let Some(t) = env3.values.get(x.as_str()).copied() {
                        self.top_types.insert(g.clone(), t);
                    }
                    env3.tops = env3.tops.add(Rc::from(x.as_str()), g);
                }
                let bt = self.get_ty(b.meta.id)?;
                self.check_ambiguity(true, 0, &[bt])?;
                self.default_numerics()?;
                if emit {
                    self.show_binding_line(&env3, b)?;
                }
                Ok(env3)
            }
            DeclKind::LetRec(bs) => {
                let env2 = self.elab_rec_bindings(true, &|_, _| Ok(None), env, 0, eff0, bs)?;
                for b in bs {
                    let sg = self.signature_of_binding(env, b)?;
                    if let Some(x) = binding_name(b)
                        && let Some(sg) = sg
                    {
                        let ft = *env2.values.get(x.as_str()).expect("束縛");
                        self.check_signature_dicts(b.meta.span, ft, sg)?;
                    }
                }
                let mut tys = Vec::new();
                for b in bs {
                    tys.push(self.get_ty(b.meta.id)?);
                }
                self.check_ambiguity(true, 0, &tys)?;
                self.default_numerics()?;
                if emit {
                    for b in bs {
                        self.show_binding_line(&env2, b)?;
                    }
                }
                Ok(env2)
            }
            DeclKind::Exp(e) => {
                let hmark = self.hole_mark();
                let mark = self.class_mark();
                let value = super::elab::is_value(e);
                let t = self.elab_exp(env, if value { 1 } else { 0 }, eff0, e)?;
                let ws = self.ex_drain(0, &[t], true)?;
                for w in ws {
                    self.warn(w);
                }
                if value && self.has_constrained_unbound(t)? {
                    self.check_ambiguity(false, 0, &[t])?;
                    self.generalize(0, t)?;
                    let dps = self.dkeys_of(t)?;
                    if !dps.is_empty() {
                        self.set_dict(node.meta.id, DictInfo::Abs(dps.clone()));
                    }
                    self.solve_holes_at(hmark, &dps)?;
                } else {
                    if value {
                        self.lower_levels(mark, 0, t)?;
                    }
                    self.check_ambiguity(true, 0, &[t])?;
                    self.default_numerics()?;
                }
                if emit {
                    let s = self.show(t)?;
                    self.current_out.push(OutLine::Binding(format!("_ : {s}")));
                }
                Ok(env.clone())
            }
            DeclKind::Extern(ex) => {
                if ex.abi != "prim" && ex.abi != "C" {
                    return terr(format!(
                        "未知の extern リンケージ: {}(prim か C を指定してください)",
                        ex.abi
                    ));
                }
                if ex.is_pub {
                    self.check_pub_annots(
                        ex.tparams.iter().map(|tp| tp.name.clone()).collect(),
                        Some(&ex.params),
                        ex.ret.as_ref(),
                    )?;
                }
                let cu = self.d.current_unit;
                self.add_extern(cu, &ex.prim, &ex.name)?;
                let lvl = 1;
                let rigids: Rigids = self.make_rigids(None, lvl, &ex.tparams)?;
                let env_ty = env.with_types(&rigids);
                let mut seen = Vec::new();
                let param_tys: Vec<Ty> = ex.params.iter().map(|_| self.new_var(lvl)).collect();
                let mut e2 = env_ty.clone();
                for (p, t) in ex.params.iter().zip(param_tys.iter()) {
                    e2 = self.elab_pat(&e2, lvl, &mut seen, *t, p)?;
                }
                let fn_eff = match &ex.eff {
                    Some(e) => self.elab_eff(&env_ty, lvl, e)?,
                    None if ex.abi == "C" => {
                        let (b, u, e) = (self.eff_blocking, self.t_unit, self.row_empty);
                        self.extend(b, u, e)
                    }
                    None => self.new_row_var(lvl),
                };
                let ret_ty = match &ex.ret {
                    Some(t) => self.elab_value_type(&env_ty, lvl, "返り値の型注釈", t)?,
                    None => self.new_var(lvl),
                };
                if ex.abi == "C"
                    && let Some((arg_cons, ret_con, rendered)) = self.c_known_signature(&ex.prim)
                {
                    let mut ok = param_tys.len() == arg_cons.len();
                    if ok {
                        for (t, c) in param_tys.iter().zip(arg_cons.iter()) {
                            if !self.is_con0(*t, *c) {
                                ok = false;
                                break;
                            }
                        }
                    }
                    ok = ok && self.is_con0(ret_ty, ret_con);
                    if !ok {
                        return terr(format!(
                            "extern \"C\" の既知名 {} の型は {rendered} でなければなりません",
                            ex.prim
                        ));
                    }
                }
                let row = self.closed_item_row(&param_tys);
                let p = self.record(row);
                let ty = self.arrow(p, ret_ty, fn_eff);
                self.generalize(0, ty)?;
                self.release_rigids(&rigids)?;
                let dps = self.dkeys_of(ty)?;
                self.check_tparam_constraints(&rigids, &dps)?;
                if !dps.is_empty() {
                    self.set_dict(node.meta.id, DictInfo::Abs(dps));
                }
                if emit {
                    let s = self.show(ty)?;
                    self.current_out
                        .push(OutLine::Binding(format!("{} : {s}", display(&ex.name))));
                }
                Ok(self.add_top(env, &ex.name, ty, GRef::Decl(node.meta.id, ex.name.clone())))
            }
            DeclKind::Newtype(_) | DeclKind::Effect(_) | DeclKind::Class(_) => Ok(env.clone()),
            DeclKind::Instance(i) => {
                self.check_instance_bodies(env, eff0, node, i)?;
                let mut tys = Vec::new();
                for d in &i.body {
                    match &d.kind {
                        DeclKind::Let(b) => tys.push(self.get_ty(b.meta.id)?),
                        DeclKind::LetRec(bs) => {
                            for b in bs {
                                tys.push(self.get_ty(b.meta.id)?);
                            }
                        }
                        _ => {}
                    }
                }
                self.check_ambiguity(true, 0, &tys)?;
                Ok(env.clone())
            }
            DeclKind::Module(..) => {
                bug("module が平坦化されていません(flatten_modules を先に呼んでください)")
            }
        }
    }

    /// プログラムの初めの状態にする(Diktor の `start_program`)。
    pub(crate) fn start_program(&mut self) {
        self.warnings.clear();
        self.unify_reset();
        self.pending.clear();
        self.pattern_numbers.clear();
        self.group_stack.clear();
        self.opened_rows.clear();
        self.pub_pure_cells.clear();
    }

    /// プレリュードを検査する(Diktor の `check_prelude`)。
    pub(crate) fn check_prelude(&mut self, prelude: &[Decl]) -> R<Env> {
        self.d.in_prelude = true;
        let env0 = self.initial_env();
        let r = self.process_decls(&[], &env0, prelude, false);
        self.d.in_prelude = false;
        r
    }

    /// 単位を検査する(Diktor の `check_unit`)。
    pub(crate) fn check_unit(&mut self, unit: super::UnitId, env: &Env, decls: &[Decl]) -> R<Env> {
        let saved = self.d.current_unit;
        self.d.current_unit = unit;
        let r = self.process_decls(&[], env, decls, true);
        self.d.current_unit = saved;
        r
    }
}

fn row_failure_msg(msg: &str) -> bool {
    super::elab::has_sub("行型ではありません", msg)
        || super::elab::has_sub("スコープ付きの型", msg)
        || super::elab::has_sub("ラベル ", msg)
        || super::elab::has_sub("は注釈で固定された行変数", msg)
}

/// 未使用の警告を避ける。
#[allow(dead_code)]
fn _unused(_: &ClassDecl) {}
