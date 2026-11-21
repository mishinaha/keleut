//! module の平坦化である(Diktor の elab.ml の §11.42 の `flatten_modules` に当たる)。
//!
//! module の中の宣言を、名前を修飾名(`M.x`)に変えたトップレベルの宣言に展開する。

use super::decls::{mark_spelling, marks_unit, unit_of_spelling};
use super::fx::{HashMap, HashSet};
use super::{R, Tc, UnitId, at, display, noimpl, terr};
use crate::ast::{Decl, DeclKind, LetBinding, Newtype, Pat, PatKind};

/// `spell` が内部名(単位の接頭辞 `%N.` を付けた名前)を登録する表の種類である。
///
/// 型、型エイリアス、エフェクトの名前は `Synonyms`、コンストラクタの名前は `Ctors`、型クラスの名前は `Classes` の表に登録する。
#[derive(Clone, Copy)]
enum SpellTable {
    Synonyms,
    Ctors,
    Classes,
}

fn pat_names(p: &Pat, acc: &mut HashSet<String>) {
    match &p.kind {
        PatKind::Var(x) => {
            acc.insert(x.clone());
        }
        PatKind::Annot(q, _) => pat_names(q, acc),
        PatKind::Record(fs, tail) => {
            for (_, q) in fs {
                pat_names(q, acc);
            }
            if let Some(t) = tail {
                pat_names(t, acc);
            }
        }
        PatKind::Ctor(_, args) => {
            for a in args {
                pat_names(&a.pat, acc);
            }
        }
        PatKind::Variant(_, q) => pat_names(q, acc),
        _ => {}
    }
}

impl Tc {
    fn spell(&mut self, unit: UnitId, tbl: SpellTable, name: &str) -> String {
        let q = mark_spelling(unit, name);
        if marks_unit(unit) {
            let qo = self.intern(&q);
            let no = self.intern(name);
            let t = match tbl {
                SpellTable::Synonyms => &mut self.d.unit_synonyms,
                SpellTable::Ctors => &mut self.d.unit_ctors,
                SpellTable::Classes => &mut self.d.unit_classes,
            };
            t.insert((unit, no), qo);
        }
        q
    }

    fn qualifier(&mut self, unit: UnitId, name: &str) -> String {
        let q = mark_spelling(unit, name);
        if marks_unit(unit) {
            self.d
                .unit_qualifiers
                .insert((unit, name.to_string()), q.clone());
        }
        q
    }

    fn spell_ctors(&mut self, unit: UnitId, mut n: Newtype) -> Newtype {
        for c in &mut n.ctors {
            c.name = self.spell(unit, SpellTable::Ctors, &c.name);
        }
        n
    }

    /// module を平坦化する(Diktor の `flatten_modules`)。
    ///
    /// 宣言は値で受け取り、コピーせずに結果へムーブする。
    /// 木をコピーする再帰は型検査のスタック残量の検査(`guard`)を通らないうえ、深い木では時間もかかるからである。
    pub(crate) fn flatten_modules(
        &mut self,
        imported_vals: &[String],
        imported_types: &[String],
        unit: UnitId,
        decls: Vec<Decl>,
    ) -> R<Vec<Decl>> {
        let mut toplevel_vals: HashSet<String> = HashSet::default();
        let mut toplevel_types: HashSet<String> = HashSet::default();
        let mut class_methods: HashMap<String, Vec<String>> = HashMap::default();
        for c in self.d.class_order.clone() {
            let ci = self.d.classes[&c].clone();
            let s = self.name_of(c);
            let ms: Vec<String> = ci.ci_methods.iter().map(|(m, _)| m.clone()).collect();
            match unit_of_spelling(&s) {
                None => {
                    class_methods.insert(s.to_string(), ms);
                }
                Some(u) if u == unit => {
                    class_methods.insert(display(&s).to_string(), ms);
                }
                Some(_) => {}
            }
        }
        for d in &decls {
            match &d.kind {
                DeclKind::Let(b) => pat_names(&b.name, &mut toplevel_vals),
                DeclKind::LetRec(bs) => {
                    for b in bs {
                        pat_names(&b.name, &mut toplevel_vals);
                    }
                }
                DeclKind::Extern(ex) => {
                    toplevel_vals.insert(ex.name.clone());
                }
                DeclKind::Newtype(n) => {
                    toplevel_types.insert(n.name.clone());
                }
                DeclKind::Type(t) => {
                    toplevel_types.insert(t.name.clone());
                }
                DeclKind::Effect(e) => {
                    toplevel_types.insert(e.name.clone());
                }
                DeclKind::Class(c) => {
                    class_methods.insert(
                        c.name.clone(),
                        c.vals.iter().map(|v| v.name.clone()).collect(),
                    );
                }
                _ => {}
            }
        }
        for x in imported_vals {
            toplevel_vals.insert(x.clone());
        }
        for x in imported_types {
            toplevel_types.insert(x.clone());
        }
        let mut out = Vec::new();
        for node in decls {
            let span = node.meta.span;
            let r = self.flatten_one(unit, node, &toplevel_vals, &toplevel_types, &class_methods);
            let ds = at(span, r)?;
            for d in ds {
                self.d.decl_unit.insert(d.meta.id, unit);
                out.push(d);
            }
        }
        Ok(out)
    }

    fn flatten_one(
        &mut self,
        unit: UnitId,
        node: Decl,
        toplevel_vals: &HashSet<String>,
        toplevel_types: &HashSet<String>,
        class_methods: &HashMap<String, Vec<String>>,
    ) -> R<Vec<Decl>> {
        let meta = node.meta;
        let with = |kind: DeclKind| Decl { meta, kind };
        Ok(match node.kind {
            DeclKind::Newtype(mut n) => {
                n.name = self.spell(unit, SpellTable::Synonyms, &n.name);
                vec![with(DeclKind::Newtype(self.spell_ctors(unit, n)))]
            }
            DeclKind::Type(mut t) => {
                t.name = self.spell(unit, SpellTable::Synonyms, &t.name);
                vec![with(DeclKind::Type(t))]
            }
            DeclKind::Effect(mut e) => {
                e.name = self.spell(unit, SpellTable::Synonyms, &e.name);
                vec![with(DeclKind::Effect(e))]
            }
            DeclKind::Class(mut c) => {
                self.qualifier(unit, &c.name);
                c.name = self.spell(unit, SpellTable::Classes, &c.name);
                vec![with(DeclKind::Class(c))]
            }
            DeclKind::Module(_, mname, body) => {
                let sname = self.qualifier(unit, &mname);
                let mut out = Vec::new();
                for bnode in body {
                    let span = bnode.meta.span;
                    let r = self.flatten_member(
                        unit,
                        &mname,
                        &sname,
                        bnode,
                        toplevel_vals,
                        toplevel_types,
                        class_methods,
                    );
                    out.extend(at(span, r)?);
                }
                out
            }
            kind => vec![with(kind)],
        })
    }

    fn claim_val(
        &mut self,
        mname: &str,
        sname: &str,
        x: &str,
        toplevel_vals: &HashSet<String>,
        class_methods: &HashMap<String, Vec<String>>,
    ) -> R<()> {
        if toplevel_vals.contains(x) {
            return terr(format!(
                "module {mname} の {x} はトップレベルの {x} と同名です(module 内の名前とトップレベル名は同名にできません)"
            ));
        }
        if class_methods
            .get(mname)
            .is_some_and(|ms| ms.iter().any(|m| m == x))
        {
            return terr(format!(
                "module {mname} の {x} は型クラス {mname} のメソッド {x} と修飾名が衝突します(module か メソッドを改名してください)"
            ));
        }
        if !self.d.in_prelude && self.is_builtin_value(&format!("{mname}.{x}")) {
            return terr(format!(
                "module {mname} の {x} は組み込みの {mname}.{x} と同名です(組み込みの名前は宣言できません)"
            ));
        }
        let q = format!("{sname}.{x}");
        let qo = self.intern(&q);
        let xo = self.intern(x);
        self.d
            .module_val_synonyms
            .insert((sname.to_string(), xo), qo);
        let qo = self.intern(&q);
        let xo = self.intern(x);
        self.add_val_synonym(xo, qo);
        Ok(())
    }

    fn claim_con(
        &mut self,
        mname: &str,
        sname: &str,
        name: &str,
        is_pub: bool,
        toplevel_types: &HashSet<String>,
    ) -> R<String> {
        let qual = format!("{sname}.{name}");
        let qo = self.intern(&qual);
        let no = self.intern(name);
        self.d
            .module_con_synonyms
            .insert((sname.to_string(), no), qo);
        let qo = self.intern(&qual);
        let no = self.intern(name);
        self.add_con_hint(no, qo);
        if name == mname
            && (toplevel_types.contains(name) || {
                let o = self.intern(name);
                self.d.reserved_type_names.contains(&o)
            })
        {
            return terr(format!(
                "module {mname} のコンパニオン型 {name} は既存の型 {name} と同名です(module 内の型とトップレベルの型は同名にできません)"
            ));
        }
        let qo = self.intern(&qual);
        self.d.con_visibility.insert(
            qo,
            super::decls::Visibility {
                vis_module: sname.to_string(),
                vis_pub: is_pub,
            },
        );
        Ok(qual)
    }

    fn vis(&mut self, sname: &str, x: &str, is_pub: bool) {
        let o = self.intern(&format!("{sname}.{x}"));
        self.d.value_visibility.insert(
            o,
            super::decls::Visibility {
                vis_module: sname.to_string(),
                vis_pub: is_pub,
            },
        );
    }

    fn rename_binding(mut b: LetBinding, name: String) -> LetBinding {
        b.name = Pat {
            meta: b.name.meta,
            kind: PatKind::Var(name),
        };
        b
    }

    #[allow(clippy::too_many_arguments)]
    fn flatten_member(
        &mut self,
        unit: UnitId,
        mname: &str,
        sname: &str,
        bnode: Decl,
        toplevel_vals: &HashSet<String>,
        toplevel_types: &HashSet<String>,
        class_methods: &HashMap<String, Vec<String>>,
    ) -> R<Vec<Decl>> {
        let meta = bnode.meta;
        let record_module = |tc: &mut Tc, kind: DeclKind| {
            tc.d.decl_module.insert(meta.id, sname.to_string());
            Decl { meta, kind }
        };
        Ok(match bnode.kind {
            DeclKind::Newtype(mut n) => {
                n.name = self.claim_con(mname, sname, &n.name, n.is_pub, toplevel_types)?;
                let n = self.spell_ctors(unit, n);
                vec![record_module(self, DeclKind::Newtype(n))]
            }
            DeclKind::Type(mut t) => {
                t.name = self.claim_con(mname, sname, &t.name, t.is_pub, toplevel_types)?;
                vec![record_module(self, DeclKind::Type(t))]
            }
            DeclKind::Let(b) => match &b.name.kind {
                PatKind::Var(x) => {
                    let x = x.clone();
                    self.claim_val(mname, sname, &x, toplevel_vals, class_methods)?;
                    self.vis(sname, &x, b.is_pub);
                    let b2 = Tc::rename_binding(b, format!("{sname}.{x}"));
                    vec![record_module(self, DeclKind::Let(b2))]
                }
                _ => {
                    return terr(format!(
                        "module 内の let はパターン束縛にできません: module {mname}"
                    ));
                }
            },
            DeclKind::LetRec(bs) => {
                let mut out = Vec::new();
                for b in bs {
                    match &b.name.kind {
                        PatKind::Var(x) => {
                            let x = x.clone();
                            self.claim_val(mname, sname, &x, toplevel_vals, class_methods)?;
                            self.vis(sname, &x, b.is_pub);
                            out.push(Tc::rename_binding(b, format!("{sname}.{x}")));
                        }
                        _ => return terr("module 内の let rec はパターン束縛にできません"),
                    }
                }
                vec![record_module(self, DeclKind::LetRec(out))]
            }
            kind @ DeclKind::Instance(_) => vec![record_module(self, kind)],
            DeclKind::Extern(mut ex) => {
                self.claim_val(mname, sname, &ex.name, toplevel_vals, class_methods)?;
                self.vis(sname, &ex.name, ex.is_pub);
                ex.name = format!("{sname}.{}", ex.name);
                vec![record_module(self, DeclKind::Extern(ex))]
            }
            DeclKind::Module(..) => return noimpl("module の入れ子(M10)"),
            DeclKind::Effect(_) => {
                return noimpl(format!("module 内の effect 宣言(M10): module {mname}"));
            }
            DeclKind::Class(_) => {
                return noimpl(format!("module 内の type class 宣言(M10): module {mname}"));
            }
            DeclKind::Exp(_) => return noimpl(format!("module 内の式文(M10): module {mname}")),
        })
    }
}
