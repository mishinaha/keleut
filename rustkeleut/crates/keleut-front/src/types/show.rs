//! 型の表示である(Diktor の show.ml、第9章)。
//!
//! 型変数の名前は、表示の中で初めて現れた順に、カインドごとの名簿から振る。
//! 同じ表示名の別の型(別の単位の同名の型)が 1 つの表示に並ぶときは、単位の名前を添える。

use super::fx::{HashMap, HashSet};
use super::{Fail, KView, Oid, R, TVar, Tc, Ty, TyNode, VarInfo, guard};

struct St {
    seen: HashMap<String, Vec<Oid>>,
    regions: HashSet<Oid>,
    names: HashMap<Oid, String>,
    constrained: Vec<(String, Vec<Oid>)>,
    star_count: usize,
    region_count: usize,
    arrow_count: usize,
    row_count: usize,
    rigid_count: usize,
}

fn pick(pool: &str, i: usize) -> String {
    let cs: Vec<char> = pool.chars().collect();
    let n = cs.len();
    let mut s = cs[i % n].to_string();
    if i >= n {
        s.push_str(&(i / n).to_string());
    }
    s
}

impl Tc {
    fn show_note(&self, st: &mut St, n: Oid) {
        let d = self.display_of(n);
        let prev = st.seen.entry(d).or_default();
        if !prev.contains(&n) {
            prev.insert(0, n);
        }
    }

    fn show_region(&mut self, st: &mut St, t: Ty) {
        let t = self.repr(t);
        if let Some(c) = self.var_cell(t)
            && let TVar::Unbound(i) | TVar::Generic(i) = self.cell(c)
        {
            st.regions.insert(i.vid);
        }
    }

    fn show_collect(&mut self, st: &mut St, structural: bool, t: Ty) -> R<()> {
        guard()?;
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Con(n, args) => {
                self.show_note(st, n);
                if (n == self.ty_ref || n == self.ty_mutable_array) && !args.is_empty() {
                    self.show_region(st, args[0]);
                }
                for a in args.iter() {
                    self.show_collect(st, false, *a)?;
                }
            }
            TyNode::App(f, a) => {
                self.show_collect(st, false, f)?;
                self.show_collect(st, false, a)?;
            }
            TyNode::Arrow(p, r, e) => {
                self.show_collect(st, false, p)?;
                self.show_collect(st, false, r)?;
                self.show_collect(st, false, e)?;
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.show_collect(st, true, row)?,
            TyNode::RowExtend(l, f, rest) => {
                self.show_note(st, l);
                if l == self.eff_heap && !structural {
                    self.show_region(st, f);
                }
                self.show_collect(st, false, f)?;
                self.show_collect(st, structural, rest)?;
            }
            TyNode::RowEmpty | TyNode::Var(_) => {}
        }
        Ok(())
    }

    fn show_con_name(&self, st: &St, n: Oid) -> String {
        let d = self.display_of(n);
        match st.seen.get(&d) {
            Some(v) if v.len() >= 2 => {
                let u = self.entity_unit(n);
                format!("{d}({})", self.unit_path(u))
            }
            _ => d,
        }
    }

    fn show_record_cs(&self, st: &mut St, name: String, cs: &[Oid]) -> String {
        let mut uniq: Vec<Oid> = Vec::new();
        for c in cs {
            if !self.is_predicate(*c) && !uniq.contains(c) {
                uniq.push(*c);
            }
        }
        if !uniq.is_empty() && !st.constrained.iter().any(|(n, _)| *n == name) {
            st.constrained.push((name.clone(), uniq));
        }
        name
    }

    fn show_var_name(&mut self, st: &mut St, i: &VarInfo, generic: bool) -> String {
        let n = match st.names.get(&i.vid) {
            Some(n) => n.clone(),
            None => {
                let base = match self.kview(i.vkind) {
                    KView::Row => {
                        st.row_count += 1;
                        format!("R{}", st.row_count)
                    }
                    KView::Arrow(..) => {
                        let n = st.arrow_count;
                        st.arrow_count += 1;
                        pick("FGH", n)
                    }
                    _ if st.regions.contains(&i.vid) => {
                        let n = st.region_count;
                        st.region_count += 1;
                        pick("h", n)
                    }
                    _ => {
                        let n = st.star_count;
                        st.star_count += 1;
                        pick("ABCDE", n)
                    }
                };
                let base = if generic { base } else { format!("_{base}") };
                st.names.insert(i.vid, base.clone());
                base
            }
        };
        self.show_record_cs(st, n, &i.vcls)
    }

    fn show_rigid_name(&self, st: &mut St, i: &VarInfo) -> String {
        match st.names.get(&i.vid) {
            Some(n) => n.clone(),
            None => {
                st.rigid_count += 1;
                let n = format!("ς{}", st.rigid_count);
                st.names.insert(i.vid, n.clone());
                n
            }
        }
    }

    fn is_tuple_row(&mut self, fields: &[(Oid, Ty)], tail: Ty) -> bool {
        let tail = self.repr(tail);
        self.is_row_empty(tail) && fields.iter().all(|(l, _)| *l == self.l_item)
    }

    fn show_is_unit(&mut self, t: Ty) -> bool {
        let t = self.repr(t);
        match self.node(t) {
            TyNode::Record(r) => self.repr_is_empty(r),
            _ => false,
        }
    }

    fn show_go(&mut self, st: &mut St, t: Ty) -> R<String> {
        guard()?;
        let t = self.repr(t);
        Ok(match self.node(t) {
            TyNode::Var(v) => match self.cell(v).clone() {
                TVar::Unbound(i) => self.show_var_name(st, &i, false),
                TVar::Generic(i) => self.show_var_name(st, &i, true),
                TVar::Rigid(i) => self.show_rigid_name(st, &i),
                TVar::Link(t) => self.show_go(st, t)?,
            },
            TyNode::Con(n, args) if args.is_empty() => self.show_con_name(st, n),
            TyNode::Con(n, args) => {
                let head = self.show_con_name(st, n);
                let mut parts = Vec::new();
                for a in args.iter() {
                    parts.push(self.show_go(st, *a)?);
                }
                format!("{head}[{}]", parts.join(", "))
            }
            TyNode::App(..) => {
                let (h, args) = self.app_spine(t);
                let hs = self.show_go(st, h)?;
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.show_go(st, a)?);
                }
                format!("{hs}[{}]", parts.join(", "))
            }
            TyNode::Arrow(p, r, e) => {
                let ps = self.show_args(st, p)?;
                let rs = self.show_go(st, r)?;
                let es = self.show_eff_suffix(st, e)?;
                let rr = self.repr(r);
                let rs = if matches!(self.node_ref(rr), TyNode::Arrow(..)) && !es.is_empty() {
                    format!("({rs})")
                } else {
                    rs
                };
                format!("{ps} => {rs}{es}")
            }
            TyNode::Record(row) => {
                let (fields, tail) = self.row_fields(row);
                if self.is_tuple_row(&fields, tail) && !fields.is_empty() {
                    let mut parts = Vec::new();
                    for (_, t) in &fields {
                        parts.push(self.show_go(st, *t)?);
                    }
                    let close = if fields.len() == 1 { ",)" } else { ")" };
                    format!("({}{close}", parts.join(", "))
                } else {
                    let tail = self.repr(tail);
                    let empty = self.is_row_empty(tail);
                    match (fields.is_empty(), empty) {
                        (true, true) => "{}".to_string(),
                        (true, false) => format!("{{extends {}}}", self.show_go(st, tail)?),
                        (false, true) => {
                            let fs = self.show_fields(st, &fields)?;
                            format!("{{{fs}}}")
                        }
                        (false, false) => {
                            let fs = self.show_fields(st, &fields)?;
                            let ts = self.show_go(st, tail)?;
                            format!("{{{fs} extends {ts}}}")
                        }
                    }
                }
            }
            TyNode::Variant(row) => {
                let (fields, tail) = self.row_fields(row);
                let mut parts = Vec::new();
                for (l, t) in &fields {
                    if self.show_is_unit(*t) {
                        parts.push(format!("#{}", self.display_of(*l)));
                    } else {
                        let s = self.show_go(st, *t)?;
                        parts.push(format!("#{}({s})", self.display_of(*l)));
                    }
                }
                let tail = self.repr(tail);
                if !self.is_row_empty(tail) {
                    parts.push(self.show_go(st, tail)?);
                }
                if parts.is_empty() {
                    "#|".to_string()
                } else {
                    parts.join(" | ")
                }
            }
            TyNode::RowEmpty => "{}".to_string(),
            TyNode::RowExtend(..) => self.show_eff_row(st, t)?,
        })
    }

    fn show_fields(&mut self, st: &mut St, fields: &[(Oid, Ty)]) -> R<String> {
        let mut parts = Vec::new();
        for (l, t) in fields {
            let s = self.show_go(st, *t)?;
            parts.push(format!("{}: {s}", self.display_of(*l)));
        }
        Ok(parts.join(", "))
    }

    fn show_args(&mut self, st: &mut St, p: Ty) -> R<String> {
        let p = self.repr(p);
        match self.node(p) {
            TyNode::Record(row) => {
                let (fields, tail) = self.row_fields(row);
                if self.is_tuple_row(&fields, tail) {
                    let mut parts = Vec::new();
                    for (_, t) in &fields {
                        parts.push(self.show_go(st, *t)?);
                    }
                    Ok(format!("({})", parts.join(", ")))
                } else {
                    let r = self.record(row);
                    self.show_go(st, r)
                }
            }
            _ => self.show_go(st, p),
        }
    }

    fn show_eff_row(&mut self, st: &mut St, row: Ty) -> R<String> {
        let (fields, tail) = self.row_fields(row);
        let mut parts = Vec::new();
        for (l, t) in &fields {
            if self.show_is_unit(*t) {
                parts.push(self.show_con_name(st, *l));
            } else {
                let head = self.show_con_name(st, *l);
                let s = self.show_go(st, *t)?;
                parts.push(format!("{head}[{s}]"));
            }
        }
        let tail = self.repr(tail);
        let ext = if self.is_row_empty(tail) {
            String::new()
        } else {
            let pre = if parts.is_empty() {
                "extends "
            } else {
                " extends "
            };
            format!("{pre}{}", self.show_go(st, tail)?)
        };
        Ok(format!("{{{}{ext}}}", parts.join(", ")))
    }

    fn show_eff_suffix(&mut self, st: &mut St, e: Ty) -> R<String> {
        let e = self.repr(e);
        match self.node_ref(e) {
            TyNode::RowEmpty => Ok(" @ {}".to_string()),
            TyNode::Var(_) => Ok(String::new()),
            _ => Ok(format!(" @ {}", self.show_eff_row(st, e)?)),
        }
    }

    /// 型の列を表示し、制約の前置きを返す(Diktor の `show_all`)。
    fn show_all(&mut self, ts: &[Ty]) -> R<(Vec<String>, String)> {
        let mut st = St {
            seen: HashMap::default(),
            regions: HashSet::default(),
            names: HashMap::default(),
            constrained: Vec::new(),
            star_count: 0,
            region_count: 0,
            arrow_count: 0,
            row_count: 0,
            rigid_count: 0,
        };
        for &t in ts {
            self.show_collect(&mut st, false, t)?;
        }
        let mut strs = Vec::new();
        for &t in ts {
            strs.push(self.show_go(&mut st, t)?);
        }
        let ctx = if st.constrained.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = st
                .constrained
                .iter()
                .map(|(n, cls)| {
                    let mut names: Vec<String> = cls.iter().map(|c| self.display_of(*c)).collect();
                    names.sort();
                    format!("{n}: {}", names.join(" + "))
                })
                .collect();
            format!("[{}] ", parts.join(", "))
        };
        Ok((strs, ctx))
    }

    /// 型の表示(Diktor の `Show.show`)。
    ///
    /// 表示の途中でスタックが足りなくなったときは、[`Fail::Limit`] で失敗する。
    /// Diktor の `show` が `Stack_overflow` を投げて限度の診断になるのと同じである。
    pub(crate) fn show(&mut self, t: Ty) -> R<String> {
        let (strs, ctx) = self.show_all(&[t]).map_err(|_| Fail::Limit)?;
        Ok(format!("{ctx}{}", strs[0]))
    }

    /// 2 つの型の表示(Diktor の `Show.show2`)。
    pub(crate) fn show2(&mut self, a: Ty, b: Ty) -> R<String> {
        let (strs, _) = self.show_all(&[a, b]).map_err(|_| Fail::Limit)?;
        Ok(format!("{} と {}", strs[0], strs[1]))
    }

    /// 2 つの型の表示の組(Diktor の `Show.show_pair`)。
    pub(crate) fn show_pair(&mut self, a: Ty, b: Ty) -> R<(String, String)> {
        let (mut strs, _) = self.show_all(&[a, b]).map_err(|_| Fail::Limit)?;
        let y = strs.pop().unwrap_or_default();
        let x = strs.pop().unwrap_or_default();
        Ok((x, y))
    }
}
