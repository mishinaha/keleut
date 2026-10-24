//! ファイルの名前空間である(Diktor の elab.ml の §11.45)。
//!
//! import のあるプログラムは、単位(ソースファイル)の列として型検査する。
//! 単位の列は依存される側が先で、最後が起点である([`super::load`])。
//! 各単位は次の順に処理する([`super::check`] の `check_units`)。
//!
//! 1. import の名前を、import 先の単位の公開表で引き([`Tc::resolve_import_names`])、import どうしの衝突を検査する([`import_claims`])。
//! 2. 単位の宣言を平坦化する。
//! 3. import した名前とファイルのトップレベルの宣言の衝突を検査する([`Tc::check_own_clashes`])。
//! 4. 標準環境だけを処理した環境に import した名前を足して、単位のスコープを作る([`Tc::apply_imports`])。
//! 5. 宣言を型検査する。
//! 6. 起点でない単位なら、公開表を作る([`Tc::public_table`])。
//!
//! 公開表と衝突の検査は平坦化の前の宣言を読むが、平坦化は宣言を値で移す。
//! そこで、平坦化の前に、名前と公開の印と節点の番号だけを写した骨組み([`Skel`])を作っておく。

use super::decls::mark_spelling;
use super::elab::pat_var_names;
use super::fx::HashMap;
use super::{Env, Fail, GRef, Oid, R, Tc, Ty, UnitId, at, terr};
use crate::ast::{Decl, DeclKind, ImportDecl, NodeId};
use crate::pos::Span;

/// 束縛 1 つの骨組みである。
#[derive(Clone, Debug)]
pub(crate) struct SkelBind {
    id: NodeId,
    is_pub: bool,
    names: Vec<String>,
}

/// 平坦化の前の宣言の骨組みである。
#[derive(Clone, Debug)]
pub(crate) struct Skel {
    span: Span,
    id: NodeId,
    kind: SkelKind,
}

#[derive(Clone, Debug)]
enum SkelKind {
    Let(Vec<SkelBind>),
    Extern(bool, String),
    Newtype(bool, String, Vec<String>),
    Type(bool, String),
    Effect(bool, String),
    Class(bool, String, Vec<String>),
    Module(bool, String, Vec<Skel>),
    Other,
}

/// 宣言列の骨組みを作る。
pub(crate) fn skeleton(decls: &[Decl]) -> Vec<Skel> {
    decls
        .iter()
        .map(|d| {
            let bind = |b: &crate::ast::LetBinding| SkelBind {
                id: b.meta.id,
                is_pub: b.is_pub,
                names: pat_var_names(&b.name),
            };
            let kind = match &d.kind {
                DeclKind::Let(b) => SkelKind::Let(vec![bind(b)]),
                DeclKind::LetRec(bs) => SkelKind::Let(bs.iter().map(bind).collect()),
                DeclKind::Extern(ex) => SkelKind::Extern(ex.is_pub, ex.name.clone()),
                DeclKind::Newtype(n) => SkelKind::Newtype(
                    n.is_pub,
                    n.name.clone(),
                    n.ctors.iter().map(|c| c.name.clone()).collect(),
                ),
                DeclKind::Type(t) => SkelKind::Type(t.is_pub, t.name.clone()),
                DeclKind::Effect(e) => SkelKind::Effect(e.is_pub, e.name.clone()),
                DeclKind::Class(c) => SkelKind::Class(
                    c.is_pub,
                    c.name.clone(),
                    c.vals.iter().map(|v| v.name.clone()).collect(),
                ),
                DeclKind::Module(p, m, body) => SkelKind::Module(*p, m.clone(), skeleton(body)),
                DeclKind::Instance(_) | DeclKind::Exp(_) => SkelKind::Other,
            };
            Skel {
                span: d.meta.span,
                id: d.meta.id,
                kind,
            }
        })
        .collect()
}

/// 公開表の項目(Diktor の `pub_entry`)である。
#[derive(Clone, Debug)]
pub(crate) enum PubEntry {
    Value(Ty, GRef),
    /// 型の綴りと、コンストラクタの(名前, 綴り)。
    Type(Oid, Vec<(String, Oid)>),
    Effect(Oid),
    /// クラスの綴りと、メソッドの(名前, 型)。
    Class(Oid, Vec<(String, Ty)>),
    /// 綴り、公開のメンバー、全メンバーの名前。
    Module(String, Vec<(String, PubEntry)>, Vec<String>),
}

/// 単位の公開表(Diktor の `unit_table`)である。
#[derive(Clone, Debug, Default)]
pub(crate) struct UnitTable {
    pubs: Vec<(String, PubEntry)>,
    all: Vec<String>,
}

/// import で束縛する名前 1 つ(名前, 項目, 位置, import 元のパス)である。
pub(crate) type Bind = (String, PubEntry, Span, String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ImportKind {
    Value,
    Type,
    Ctor,
    Class,
    Module,
}

/// import した名前の表。(種類, 名前) → (実体の鍵, import 元, 型の名前)。
pub(crate) type Claims = HashMap<(ImportKind, String), (String, String, String)>;

fn gref_key(g: &GRef) -> String {
    match g {
        GRef::Builtin(s) => format!("b:{s}"),
        GRef::Method(q) => format!("m:{q}"),
        GRef::Decl(o, x) => format!("d:{o}:{x}"),
    }
}

/// 1 つの import 文の名前を数える文脈である。
struct Claimer<'a> {
    seen: Claims,
    loc: Span,
    src: &'a str,
}

impl Claimer<'_> {
    fn claim(&mut self, kind: ImportKind, name: &str, key: String, owner: &str) -> R<()> {
        match self.seen.get(&(kind, name.to_string())) {
            Some((k, _, _)) if *k == key => Ok(()),
            Some((_, src0, _)) => Err(Fail::Import(
                self.loc,
                format!("{name} は {src0} からも import しています(別の宣言です)"),
            )),
            None => {
                self.seen.insert(
                    (kind, name.to_string()),
                    (key, self.src.to_string(), owner.to_string()),
                );
                Ok(())
            }
        }
    }

    fn ctors(&mut self, tc: &Tc, cs: &[(String, Oid)], owner: &str) -> R<()> {
        for (c, o) in cs {
            self.claim(ImportKind::Ctor, c, tc.name_of(*o).to_string(), owner)?;
        }
        Ok(())
    }
}

/// import した名前を種類ごとに数え、import どうしの衝突を検査する(Diktor の `import_claims`)。
pub(crate) fn import_claims(tc: &Tc, binds: &[Bind]) -> R<Claims> {
    let mut seen: Claims = HashMap::default();
    for (x, e, loc, src) in binds {
        let mut c = Claimer {
            seen: std::mem::take(&mut seen),
            loc: *loc,
            src,
        };
        match e {
            PubEntry::Value(_, g) => c.claim(ImportKind::Value, x, gref_key(g), "")?,
            PubEntry::Type(o, cs) => {
                c.claim(ImportKind::Type, x, tc.name_of(*o).to_string(), "")?;
                c.ctors(tc, cs, x)?;
            }
            PubEntry::Effect(o) => c.claim(ImportKind::Type, x, tc.name_of(*o).to_string(), "")?,
            PubEntry::Class(o, methods) => {
                let q = tc.name_of(*o).to_string();
                c.claim(ImportKind::Class, x, q.clone(), "")?;
                // メソッドの非修飾名も値の名前として数える。
                for (m, _) in methods {
                    c.claim(ImportKind::Value, m, format!("m:{q}.{m}"), "method")?;
                }
            }
            PubEntry::Module(sm, members, _) => {
                c.claim(ImportKind::Module, x, sm.clone(), "")?;
                for (y, e) in members {
                    if let PubEntry::Type(o, cs) = e {
                        // module と同名の型(コンパニオン)は、module 名で型として引ける。
                        if y == x {
                            c.claim(ImportKind::Type, x, tc.name_of(*o).to_string(), "")?;
                        }
                        c.ctors(tc, cs, y)?;
                    }
                }
            }
        }
        seen = c.seen;
    }
    Ok(seen)
}

impl Tc {
    /// 単位の公開表を作る(Diktor の `public_table`)。
    pub(crate) fn public_table(&mut self, unit: UnitId, env: &Env, raw: &[Skel]) -> UnitTable {
        let mut entries = self.scan_public(unit, env, "", raw);
        // 同じ名前の pub の値が 2 回宣言されていれば、後の宣言を公開する。
        let mut kept = Vec::new();
        for i in 0..entries.len() {
            let (p, x, e) = &entries[i];
            let dup = *p
                && matches!(e, Some(PubEntry::Value(..)))
                && entries[i + 1..]
                    .iter()
                    .any(|(p2, y, e2)| *p2 && y == x && matches!(e2, Some(PubEntry::Value(..))));
            kept.push(!dup);
        }
        let mut k = kept.into_iter();
        entries.retain(|_| k.next().unwrap_or(true));
        UnitTable {
            all: entries.iter().map(|(_, x, _)| x.clone()).collect(),
            pubs: entries
                .into_iter()
                .filter_map(|(p, x, e)| match e {
                    Some(e) if p => Some((x, e)),
                    _ => None,
                })
                .collect(),
        }
    }

    /// `prefix` は module の中なら綴りの頭(`%3.M.`)で、トップレベルなら空である。
    fn scan_public(
        &mut self,
        unit: UnitId,
        env: &Env,
        prefix: &str,
        decls: &[Skel],
    ) -> Vec<(bool, String, Option<PubEntry>)> {
        let mut out = Vec::new();
        let ty_spell = |x: &str| {
            if prefix.is_empty() {
                mark_spelling(unit, x)
            } else {
                format!("{prefix}{x}")
            }
        };
        for d in decls {
            match &d.kind {
                SkelKind::Let(bs) => {
                    for b in bs {
                        for x in &b.names {
                            let g = GRef::Decl(b.id, format!("{prefix}{x}"));
                            let e = self
                                .top_types
                                .get(&g)
                                .map(|t| PubEntry::Value(*t, g.clone()));
                            out.push((b.is_pub, x.clone(), e));
                        }
                    }
                }
                SkelKind::Extern(p, name) => {
                    let g = GRef::Decl(d.id, format!("{prefix}{name}"));
                    let e = self
                        .top_types
                        .get(&g)
                        .map(|t| PubEntry::Value(*t, g.clone()));
                    out.push((*p, name.clone(), e));
                }
                SkelKind::Newtype(p, name, ctors) => {
                    // OCaml の評価の順(構成子の引数は右から)に合わせ、コンストラクタを先に intern する。
                    let cs: Vec<(String, Oid)> = ctors
                        .iter()
                        .map(|c| (c.clone(), self.intern(&mark_spelling(unit, c))))
                        .collect();
                    let o = self.intern(&ty_spell(name));
                    out.push((*p, name.clone(), Some(PubEntry::Type(o, cs))));
                }
                SkelKind::Type(p, name) => {
                    let o = self.intern(&ty_spell(name));
                    out.push((*p, name.clone(), Some(PubEntry::Type(o, Vec::new()))));
                }
                SkelKind::Effect(p, name) => {
                    let o = self.intern(&mark_spelling(unit, name));
                    out.push((*p, name.clone(), Some(PubEntry::Effect(o))));
                }
                SkelKind::Class(p, name, vals) => {
                    let cs = mark_spelling(unit, name);
                    let methods: Vec<(String, Ty)> = vals
                        .iter()
                        .filter_map(|v| {
                            env.values
                                .get(format!("{cs}.{v}").as_str())
                                .map(|t| (v.clone(), *t))
                        })
                        .collect();
                    let o = self.intern(&cs);
                    out.push((*p, name.clone(), Some(PubEntry::Class(o, methods))));
                }
                SkelKind::Module(p, m, body) => {
                    let sm = mark_spelling(unit, m);
                    let members = self.scan_public(unit, env, &format!("{sm}."), body);
                    let all = members.iter().map(|(_, x, _)| x.clone()).collect();
                    let pubs = members
                        .into_iter()
                        .filter_map(|(p, x, e)| match e {
                            Some(e) if p => Some((x, e)),
                            _ => None,
                        })
                        .collect();
                    out.push((*p, m.clone(), Some(PubEntry::Module(sm, pubs, all))));
                }
                SkelKind::Other => {}
            }
        }
        out
    }

    /// import 文の名前を公開表で引き、束縛する名前の列を返す(Diktor の `resolve_import_names`)。
    pub(crate) fn resolve_import_names(tbl: &UnitTable, im: &ImportDecl) -> R<Vec<Bind>> {
        let src = &im.source;
        let mut out = Vec::new();
        for n in &im.names {
            let fail = |msg: String| Err(Fail::Import(n.span, msg));
            let pick = |name: &str,
                        pubs: &[(String, PubEntry)],
                        all: &[String],
                        what: &str,
                        out: &mut Vec<Bind>|
             -> R<()> {
                let es: Vec<&PubEntry> = pubs
                    .iter()
                    .filter(|(y, _)| y == name)
                    .map(|(_, e)| e)
                    .collect();
                if es.is_empty() {
                    if all.iter().any(|y| y == name) {
                        return Err(Fail::Import(
                            n.span,
                            format!("{src} の {what} は公開されていません"),
                        ));
                    }
                    return Err(Fail::Import(
                        n.span,
                        format!("{src} に {what} という公開の宣言はありません"),
                    ));
                }
                for e in es {
                    out.push((name.to_string(), e.clone(), n.span, src.clone()));
                }
                Ok(())
            };
            match n.path.as_slice() {
                [x] => pick(x, &tbl.pubs, &tbl.all, x, &mut out)?,
                [m, x] => {
                    let found = tbl.pubs.iter().find_map(|(y, e)| match e {
                        PubEntry::Module(_, ms, all) if y == m => Some((ms, all)),
                        _ => None,
                    });
                    match found {
                        None => {
                            if tbl.all.iter().any(|y| y == m) {
                                return fail(format!("{src} の {m} は公開されていません"));
                            }
                            return fail(format!("{src} に {m} という公開の宣言はありません"));
                        }
                        Some((ms, all)) => pick(x, ms, all, &format!("{m}.{x}"), &mut out)?,
                    }
                }
                _ => return super::bug("import の名前は 2 段まで"),
            }
        }
        Ok(out)
    }

    /// ファイルのトップレベルの宣言が、import した名前と衝突していないかを確かめる(Diktor の `check_own_clashes`)。
    pub(crate) fn check_own_clashes(seen: &Claims, raw: &[Skel]) -> R<()> {
        let clash = |kind: ImportKind, name: &str| -> R<()> {
            match seen.get(&(kind, name.to_string())) {
                Some((_, _, o)) if o == "method" => Ok(()),
                Some(_) => terr(format!("{name} は import した名前と同じです")),
                None => Ok(()),
            }
        };
        let clash_ctors = |ctors: &[String]| -> R<()> {
            for c in ctors {
                if let Some((_, _, owner)) = seen.get(&(ImportKind::Ctor, c.clone())) {
                    return terr(format!(
                        "コンストラクタ {c} は import した型 {owner} のコンストラクタと同じ名前です"
                    ));
                }
            }
            Ok(())
        };
        for d in raw {
            let r = (|| -> R<()> {
                match &d.kind {
                    SkelKind::Let(bs) => {
                        for b in bs {
                            for x in &b.names {
                                clash(ImportKind::Value, x)?;
                            }
                        }
                    }
                    SkelKind::Extern(_, x) => clash(ImportKind::Value, x)?,
                    SkelKind::Newtype(_, n, cs) => {
                        clash(ImportKind::Type, n)?;
                        clash_ctors(cs)?;
                    }
                    SkelKind::Type(_, n) | SkelKind::Effect(_, n) => clash(ImportKind::Type, n)?,
                    SkelKind::Class(_, n, _) => clash(ImportKind::Class, n)?,
                    SkelKind::Module(_, m, body) => {
                        clash(ImportKind::Module, m)?;
                        for b in body {
                            if let SkelKind::Newtype(_, _, cs) = &b.kind {
                                clash_ctors(cs)?;
                            }
                        }
                    }
                    SkelKind::Other => {}
                }
                Ok(())
            })();
            at(d.span, r)?;
        }
        Ok(())
    }

    /// import した名前を単位のスコープに張る(Diktor の `apply_imports`)。
    pub(crate) fn apply_imports(&mut self, unit: UnitId, env: &Env, binds: &[Bind]) -> Env {
        let mut env = env.clone();
        for (x, e, _, _) in binds {
            match e {
                PubEntry::Value(t, g) => env = self.add_top(&env, x, *t, g.clone()),
                PubEntry::Type(o, cs) => {
                    let k = self.intern(x);
                    self.d.unit_synonyms.insert((unit, k), *o);
                    self.apply_ctors(unit, cs);
                }
                PubEntry::Effect(o) => {
                    let k = self.intern(x);
                    self.d.unit_synonyms.insert((unit, k), *o);
                }
                PubEntry::Class(o, methods) => {
                    let q = self.name_of(*o).to_string();
                    let k = self.intern(x);
                    self.d.unit_classes.insert((unit, k), *o);
                    self.d.unit_qualifiers.insert((unit, x.clone()), q.clone());
                    // メソッドは修飾名と非修飾名の両方で引ける。非修飾名は先勝ち。
                    for (m, t) in methods {
                        let g = GRef::Method(format!("{q}.{m}"));
                        env = self.add_top(&env, &format!("{q}.{m}"), *t, g.clone());
                        if !env.values.contains(m.as_str()) {
                            env = self.add_top(&env, m, *t, g);
                        }
                    }
                }
                PubEntry::Module(sm, members, _) => {
                    self.d.unit_qualifiers.insert((unit, x.clone()), sm.clone());
                    for (y, e) in members {
                        match e {
                            PubEntry::Value(t, g) => {
                                env = self.add_top(&env, &format!("{sm}.{y}"), *t, g.clone())
                            }
                            PubEntry::Type(o, cs) => {
                                // module と同名の型(コンパニオン)は、module 名だけで型として引ける。
                                if y == x {
                                    let k = self.intern(x);
                                    self.d.unit_synonyms.insert((unit, k), *o);
                                }
                                self.apply_ctors(unit, cs);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        env
    }

    fn apply_ctors(&mut self, unit: UnitId, cs: &[(String, Oid)]) {
        for (c, o) in cs {
            let k = self.intern(c);
            self.d.unit_ctors.insert((unit, k), *o);
        }
    }
}
