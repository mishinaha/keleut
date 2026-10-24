//! 網羅性と到達不能の検査である(Diktor の exhaust.ml、第10章。Maranget の usefulness)。

use super::{Fail, Oid, R, Resolved, TVar, Tc, Ty, TyNode, bug, guard};
use crate::ast::{Pat, PatKind};
use crate::escape::ocaml_escaped;
use std::collections::HashMap;
use std::rc::Rc;

/// リテラルである(変種の順は Diktor の `lit` と同じで、比較の順になる)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Lit {
    Bool(bool),
    Text(String),
    Num(String),
}

/// 照合の検査に使うパターンである。
///
/// 部分のパターンは `Rc` で共有する。
/// 行列を特殊化するたびに部分を写すと、深いパターンで時間と記憶が深さの 2 乗に比例するからである。
#[derive(Clone, Debug, PartialEq, Eq)]
enum IPat {
    Wild,
    Lit(Lit),
    Variant(Oid, Rc<IPat>),
    Ctor(Oid, Rc<[IPat]>),
    Record(Rc<[(Oid, IPat)]>, bool),
}

/// 構成子である(変種の順は Diktor の `ctor` と同じで、比較の順になる)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ctor {
    Variant(Oid),
    Data(Oid),
    Lit(Lit),
    Record(Vec<Oid>),
}

/// 待ち行列の項目である。
pub struct Entry {
    rows: Vec<(Pat, bool)>,
    ty: Ty,
}

// ---- OCaml の数の読み ----

/// OCaml の `parse_intnat`(`Int32.of_string` と `Int64.of_string`)。
fn ocaml_parse_int(s: &str, nbits: u32) -> Option<i64> {
    let b = s.as_bytes();
    let mut p = 0;
    let mut sign = 1i32;
    if p < b.len() && b[p] == b'-' {
        sign = -1;
        p += 1;
    } else if p < b.len() && b[p] == b'+' {
        p += 1;
    }
    let mut base = 10u64;
    let mut signed = true;
    if p + 1 < b.len() && b[p] == b'0' {
        match b[p + 1] {
            b'x' | b'X' => {
                base = 16;
                signed = false;
                p += 2;
            }
            b'o' | b'O' => {
                base = 8;
                signed = false;
                p += 2;
            }
            b'b' | b'B' => {
                base = 2;
                signed = false;
                p += 2;
            }
            b'u' | b'U' => {
                signed = false;
                p += 2;
            }
            _ => {}
        }
    }
    let digit = |c: u8| -> Option<u64> {
        match c {
            b'0'..=b'9' => Some((c - b'0') as u64),
            b'a'..=b'f' => Some((c - b'a' + 10) as u64),
            b'A'..=b'F' => Some((c - b'A' + 10) as u64),
            _ => None,
        }
    };
    let threshold = u64::MAX / base;
    let d = digit(*b.get(p)?).filter(|d| *d < base)?;
    let mut res = d;
    p += 1;
    while p < b.len() {
        let c = b[p];
        if c == b'_' {
            p += 1;
            continue;
        }
        match digit(c).filter(|d| *d < base) {
            None => break,
            Some(d) => {
                if res > threshold {
                    return None;
                }
                res = res.wrapping_mul(base).wrapping_add(d);
                if res < d {
                    return None;
                }
            }
        }
        p += 1;
    }
    if p != b.len() {
        return None;
    }
    if signed {
        if sign >= 0 {
            if res >= 1u64 << (nbits - 1) {
                return None;
            }
        } else if res > 1u64 << (nbits - 1) {
            return None;
        }
    } else if nbits < 64 && res >= 1u64 << nbits {
        return None;
    }
    let v = if sign < 0 {
        (res as i64).wrapping_neg()
    } else {
        res as i64
    };
    Some(match nbits {
        32 => v as i32 as i64,
        _ => v,
    })
}

/// `Int32.of_string_opt`。
pub(crate) fn ocaml_int32_opt(s: &str) -> Option<i32> {
    ocaml_parse_int(s, 32).map(|v| v as i32)
}

/// `Int64.of_string_opt`。
pub(crate) fn ocaml_int64_opt(s: &str) -> Option<i64> {
    ocaml_parse_int(s, 64)
}

/// `float_of_string_opt`(桁区切りの `_` を除いてから読む)。
fn ocaml_float_opt(s: &str) -> Option<f64> {
    let t: String = s.chars().filter(|c| *c != '_').collect();
    if t.is_empty() {
        return None;
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        if hex.is_empty() || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let mut v: u128 = 0;
        let mut f = 0f64;
        let mut big = false;
        for c in hex.bytes() {
            let d = (c as char).to_digit(16)? as u128;
            if !big {
                match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
                    Some(x) => v = x,
                    None => {
                        big = true;
                        f = v as f64 * 16.0 + d as f64;
                    }
                }
            } else {
                f = f * 16.0 + d as f64;
            }
        }
        let r = if big { f } else { v as f64 };
        return Some(if neg { -r } else { r });
    }
    let ok = body
        .bytes()
        .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'));
    if !ok {
        return match body {
            "nan" | "inf" | "infinity" => {
                body.parse::<f64>().ok().map(|v| if neg { -v } else { v })
            }
            _ => None,
        };
    }
    body.parse::<f64>().ok().map(|v| if neg { -v } else { v })
}

/// 数値の字面の鍵(Diktor の `norm_text`)。
///
/// Int32、Int64、Float64 の 3 通りの読みを並べる。
/// 浮動小数の成分は ±0.0 を 1 つにまとめたビット列で表す(Diktor の `%.17g` と同じく値を一意に決める)。
fn norm_text(s: &str) -> String {
    let a = ocaml_int32_opt(s).map_or("X".to_string(), |v| v.to_string());
    let b = ocaml_int64_opt(s).map_or("X".to_string(), |v| v.to_string());
    let c = ocaml_float_opt(s).map_or("X".to_string(), |f| {
        if f == 0.0 {
            "0".to_string()
        } else {
            format!("{:016x}", f.to_bits())
        }
    });
    format!("{a}|{b}|{c}")
}

impl Tc {
    fn ex_convert(&mut self, p: &Pat) -> R<IPat> {
        guard()?;
        Ok(match &p.kind {
            PatKind::Wildcard | PatKind::Var(_) => IPat::Wild,
            PatKind::Bool(b) => IPat::Lit(Lit::Bool(*b)),
            PatKind::Text(s) => IPat::Lit(Lit::Text(s.clone())),
            PatKind::Number(n) => IPat::Lit(Lit::Num(norm_text(&n.text))),
            PatKind::Annot(sub, _) => self.ex_convert(sub)?,
            PatKind::Variant(s, sub) => {
                let l = self.intern(s);
                IPat::Variant(l, Rc::new(self.ex_convert(sub)?))
            }
            PatKind::Record(fields, rest) => {
                let mut fs = Vec::new();
                for (l, sub) in fields {
                    let lo = self.intern(l);
                    fs.push((lo, self.ex_convert(sub)?));
                }
                IPat::Record(fs.into(), rest.is_none())
            }
            PatKind::Ctor(_, args) => match self.resolved.get(&p.meta.id).cloned() {
                Some(Resolved::CtorPat(_, ctor, field_to_arg)) => {
                    let mut subs = Vec::new();
                    for ai in field_to_arg {
                        subs.push(match ai {
                            Some(i) => self.ex_convert(&args[i].pat)?,
                            None => IPat::Wild,
                        });
                    }
                    IPat::Ctor(ctor, subs.into())
                }
                _ => return bug("PCtor が解決されていません(elab_pat が resolved を書いていない)"),
            },
        })
    }

    fn ctor_fields_of(&self, c: Oid) -> Vec<super::FieldInfo> {
        let owner = self.d.ctor_owner[&c];
        let dd = &self.d.datas[&owner];
        dd.dd_ctors
            .iter()
            .find(|ct| ct.ct_name == c)
            .map(|ct| ct.ct_fields.clone())
            .unwrap_or_default()
    }

    fn ex_arity(&self, c: &Ctor) -> usize {
        match c {
            Ctor::Variant(_) => 1,
            Ctor::Data(c) => self.ctor_fields_of(*c).len(),
            Ctor::Lit(_) => 0,
            Ctor::Record(ls) => ls.len(),
        }
    }

    fn record_labels(&mut self, ty: Ty) -> Vec<Oid> {
        let ty = self.repr(ty);
        match self.node(ty) {
            TyNode::Record(row) => self.row_fields(row).0.into_iter().map(|(l, _)| l).collect(),
            _ => Vec::new(),
        }
    }

    fn ctor_of(&mut self, p: &IPat, ty: Ty) -> Option<Ctor> {
        match p {
            IPat::Wild => None,
            IPat::Lit(l) => Some(Ctor::Lit(l.clone())),
            IPat::Variant(l, _) => Some(Ctor::Variant(*l)),
            IPat::Ctor(c, _) => Some(Ctor::Data(*c)),
            IPat::Record(..) => Some(Ctor::Record(self.record_labels(ty))),
        }
    }

    fn field_type_of(&mut self, row: Ty, label: Oid) -> Ty {
        let (fs, _) = self.row_fields(row);
        match fs.iter().find(|(l, _)| *l == label) {
            Some((_, t)) => *t,
            None => self.new_var(0),
        }
    }

    fn sub_types(&mut self, c: &Ctor, ty: Ty) -> R<Vec<Ty>> {
        let rt = self.repr(ty);
        match (c, self.node(rt)) {
            (Ctor::Variant(l), TyNode::Variant(row)) => Ok(vec![self.field_type_of(row, *l)]),
            (Ctor::Data(ctor), TyNode::Con(n, args)) if self.d.datas.contains_key(&n) => {
                let dd = self.d.datas[&n].clone();
                let subst: Vec<(Oid, Ty)> = if dd.dd_params.len() == args.len() {
                    dd.dd_params
                        .iter()
                        .map(|i| i.vid)
                        .zip(args.iter().copied())
                        .collect()
                } else {
                    Vec::new()
                };
                let fields = self.ctor_fields_of(*ctor);
                let mut out = Vec::new();
                for f in fields {
                    out.push(self.subst_params(0, &subst, f.fi_ty)?);
                }
                Ok(out)
            }
            (Ctor::Record(_), TyNode::Record(row)) => {
                Ok(self.row_fields(row).0.into_iter().map(|(_, t)| t).collect())
            }
            (Ctor::Lit(_), _) => Ok(Vec::new()),
            _ => {
                let n = self.ex_arity(c);
                Ok((0..n).map(|_| self.new_var(0)).collect())
            }
        }
    }

    fn specialize(&self, c: &Ctor, rows: &[Vec<IPat>]) -> Vec<Vec<IPat>> {
        let mut out = Vec::new();
        for row in rows {
            let Some((h, rest)) = row.split_first() else {
                continue;
            };
            match (h, c) {
                (IPat::Wild, _) => {
                    let mut v = vec![IPat::Wild; self.ex_arity(c)];
                    v.extend_from_slice(rest);
                    out.push(v);
                }
                (IPat::Variant(l1, sub), Ctor::Variant(l2)) if l1 == l2 => {
                    let mut v = vec![(**sub).clone()];
                    v.extend_from_slice(rest);
                    out.push(v);
                }
                (IPat::Ctor(l1, subs), Ctor::Data(l2)) if l1 == l2 => {
                    let mut v = subs.to_vec();
                    v.extend_from_slice(rest);
                    out.push(v);
                }
                (IPat::Lit(a), Ctor::Lit(b)) if a == b => out.push(rest.to_vec()),
                (IPat::Record(fs, _), Ctor::Record(labels)) => {
                    let mut v = align_record_pat(labels, fs);
                    v.extend_from_slice(rest);
                    out.push(v);
                }
                _ => {}
            }
        }
        out
    }

    fn complete_sig(&mut self, ty: Ty) -> Option<Vec<Ctor>> {
        let ty = self.repr(ty);
        match self.node(ty) {
            TyNode::Variant(row) => {
                let (fs, tail) = self.row_fields(row);
                if self.repr_is_empty(tail) {
                    let mut v: Vec<Ctor> = fs.into_iter().map(|(l, _)| Ctor::Variant(l)).collect();
                    v.sort();
                    v.dedup();
                    Some(v)
                } else {
                    None
                }
            }
            TyNode::Con(n, args) => {
                if let Some(dd) = self.d.datas.get(&n) {
                    if dd.dd_opaque {
                        None
                    } else {
                        Some(
                            dd.dd_ctors
                                .iter()
                                .map(|ct| Ctor::Data(ct.ct_name))
                                .collect(),
                        )
                    }
                } else if args.is_empty() && n == self.o_boolean {
                    Some(vec![
                        Ctor::Lit(Lit::Bool(true)),
                        Ctor::Lit(Lit::Bool(false)),
                    ])
                } else {
                    None
                }
            }
            TyNode::Record(row) => {
                let ls = self.row_fields(row).0.into_iter().map(|(l, _)| l).collect();
                Some(vec![Ctor::Record(ls)])
            }
            _ => None,
        }
    }

    fn rebuild(&self, c: &Ctor, ws: Vec<IPat>) -> Vec<IPat> {
        match c {
            Ctor::Variant(l) => {
                let mut it = ws.into_iter();
                let first = it.next().unwrap_or(IPat::Wild);
                let mut v = vec![IPat::Variant(*l, Rc::new(first))];
                v.extend(it);
                v
            }
            Ctor::Data(l) => {
                let n = self.ex_arity(c).min(ws.len());
                let mut ws = ws;
                let rest = ws.split_off(n);
                let mut v = vec![IPat::Ctor(*l, ws.into())];
                v.extend(rest);
                v
            }
            Ctor::Lit(l) => {
                let mut v = vec![IPat::Lit(l.clone())];
                v.extend(ws);
                v
            }
            Ctor::Record(ls) => {
                let n = ls.len().min(ws.len());
                let mut ws = ws;
                let rest = ws.split_off(n);
                let fs = ls.iter().copied().zip(ws).collect();
                let mut v = vec![IPat::Record(fs, true)];
                v.extend(rest);
                v
            }
        }
    }

    fn close_row(&mut self, ty: Ty) -> R<()> {
        let ty = self.repr(ty);
        if let TyNode::Variant(row) = self.node(ty)
            && let Some(tv) = self.row_tail_var(row)
        {
            let t = self.tvar(tv);
            let e = self.row_empty;
            self.unify(t, e)?;
        }
        Ok(())
    }

    fn roots_of(&mut self, rows: &[Vec<IPat>], ty: Ty) -> Vec<Ctor> {
        let mut v = Vec::new();
        for r in rows {
            if let Some(h) = r.first()
                && let Some(c) = self.ctor_of(h, ty)
            {
                v.push(c);
            }
        }
        v.sort();
        v.dedup();
        v
    }

    fn close_variant_rows(&mut self, rows: &[Vec<IPat>], tys: &[Ty]) -> R<()> {
        guard()?;
        let Some((&ty0, tys_tail)) = tys.split_first() else {
            return Ok(());
        };
        if !rows.is_empty() {
            let ty0 = self.repr(ty0);
            let has_default = rows.iter().any(|r| matches!(r.first(), Some(IPat::Wild)));
            let roots = self.roots_of(rows, ty0);
            if !has_default && !roots.is_empty() {
                self.close_row(ty0)?;
            }
            let ty = self.repr(ty0);
            for c in &roots {
                let sp = self.specialize(c, rows);
                let mut ts = self.sub_types(c, ty)?;
                ts.extend_from_slice(tys_tail);
                self.close_variant_rows(&sp, &ts)?;
            }
            let dm = default_matrix(rows);
            self.close_variant_rows(&dm, tys_tail)?;
        }
        Ok(())
    }

    fn missing(&mut self, rows: &[Vec<IPat>], tys: &[Ty]) -> R<Option<Vec<IPat>>> {
        guard()?;
        let Some((&ty, tys_tail)) = tys.split_first() else {
            return Ok(if rows.is_empty() {
                Some(Vec::new())
            } else {
                None
            });
        };
        let ty = self.repr(ty);
        let roots = self.roots_of(rows, ty);
        let sig = self.complete_sig(ty);
        if sig_complete(&sig, &roots) {
            for c in sig.unwrap_or_default() {
                let sp = self.specialize(&c, rows);
                let mut ts = self.sub_types(&c, ty)?;
                ts.extend_from_slice(tys_tail);
                if let Some(w) = self.missing(&sp, &ts)? {
                    return Ok(Some(self.rebuild(&c, w)));
                }
            }
            Ok(None)
        } else {
            let dm = default_matrix(rows);
            match self.missing(&dm, tys_tail)? {
                None => Ok(None),
                Some(w) => {
                    let head = match &sig {
                        Some(s) => match s.iter().find(|c| !roots.contains(c)) {
                            Some(c) => {
                                let n = self.ex_arity(c);
                                self.rebuild(c, vec![IPat::Wild; n]).remove(0)
                            }
                            None => IPat::Wild,
                        },
                        None => {
                            let used_num: Vec<&String> = roots
                                .iter()
                                .filter_map(|c| match c {
                                    Ctor::Lit(Lit::Num(n)) => Some(n),
                                    _ => None,
                                })
                                .collect();
                            let used_txt: Vec<&String> = roots
                                .iter()
                                .filter_map(|c| match c {
                                    Ctor::Lit(Lit::Text(s)) => Some(s),
                                    _ => None,
                                })
                                .collect();
                            if !used_txt.is_empty() {
                                let mut i = 0;
                                loop {
                                    let s = "a".repeat(i);
                                    if !used_txt.contains(&&s) {
                                        break IPat::Lit(Lit::Text(s));
                                    }
                                    i += 1;
                                }
                            } else if used_num.is_empty() {
                                IPat::Wild
                            } else {
                                let f64o = self.intern("Float64");
                                let is_float_col = match self.node(ty) {
                                    TyNode::Con(n, args) => args.is_empty() && n == f64o,
                                    TyNode::Var(c) => match self.cell(c) {
                                        TVar::Unbound(i) => i.vcls.contains(&self.cls_fractional),
                                        _ => false,
                                    },
                                    _ => false,
                                };
                                let mut i = 0usize;
                                loop {
                                    let s = if is_float_col {
                                        format!("{i}.0")
                                    } else {
                                        i.to_string()
                                    };
                                    if !used_num.contains(&&norm_text(&s)) {
                                        break IPat::Lit(Lit::Num(s));
                                    }
                                    i += 1;
                                }
                            }
                        }
                    };
                    let mut v = vec![head];
                    v.extend(w);
                    Ok(Some(v))
                }
            }
        }
    }

    fn useful(&mut self, rows: &[Vec<IPat>], q: &[IPat], tys: &[Ty]) -> R<bool> {
        guard()?;
        let Some((&ty, tys_tail)) = tys.split_first() else {
            return Ok(rows.is_empty());
        };
        let ty = self.repr(ty);
        let head = q.first().cloned().unwrap_or(IPat::Wild);
        match self.ctor_of(&head, ty) {
            Some(c) => {
                let q2 = match self.specialize(&c, &[q.to_vec()]).pop() {
                    Some(q2) => q2,
                    None => return bug("useful: specialize q"),
                };
                let sp = self.specialize(&c, rows);
                let mut ts = self.sub_types(&c, ty)?;
                ts.extend_from_slice(tys_tail);
                self.useful(&sp, &q2, &ts)
            }
            None => {
                let roots = self.roots_of(rows, ty);
                let sig = self.complete_sig(ty);
                if sig_complete(&sig, &roots) {
                    for c in sig.unwrap_or_default() {
                        let sp = self.specialize(&c, rows);
                        let mut q2 = vec![IPat::Wild; self.ex_arity(&c)];
                        q2.extend_from_slice(&q[1..]);
                        let mut ts = self.sub_types(&c, ty)?;
                        ts.extend_from_slice(tys_tail);
                        if self.useful(&sp, &q2, &ts)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                } else {
                    let dm = default_matrix(rows);
                    self.useful(&dm, &q[1..], tys_tail)
                }
            }
        }
    }

    fn show_ipat(&self, p: &IPat) -> String {
        match p {
            IPat::Wild => "_".to_string(),
            IPat::Lit(l) => show_lit(l),
            IPat::Variant(l, sub) if matches!(&**sub, IPat::Record(fs, true) if fs.is_empty()) => {
                format!("#{}", self.display_of(*l))
            }
            IPat::Variant(l, sub) => format!("#{}({})", self.display_of(*l), self.show_ipat(sub)),
            IPat::Ctor(c, subs) if subs.is_empty() => self.display_of(*c),
            IPat::Ctor(c, subs) => format!(
                "{}({})",
                self.display_of(*c),
                subs.iter()
                    .map(|s| self.show_ipat(s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            IPat::Record(fs, true) if fs.is_empty() => "()".to_string(),
            IPat::Record(fs, closed) => {
                if *closed && !fs.is_empty() && fs.iter().all(|(l, _)| *l == self.l_item) {
                    let inner = fs
                        .iter()
                        .map(|(_, p)| self.show_ipat(p))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("({inner}{}", if fs.len() == 1 { ",)" } else { ")" })
                } else {
                    format!(
                        "{{{}}}",
                        fs.iter()
                            .map(|(l, p)| format!(
                                "{} = {}",
                                self.display_of(*l),
                                self.show_ipat(p)
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
        }
    }

    /// 検査を待ち行列に足す(Diktor の `Exhaust.queue`)。
    pub(crate) fn ex_queue(&mut self, rows: Vec<(Pat, bool)>, ty: Ty) {
        self.pending.push(Entry { rows, ty });
    }

    fn empty_open_scrutinee(&mut self, e: &Entry) -> Option<i32> {
        if !e.rows.is_empty() {
            return None;
        }
        let t = self.repr(e.ty);
        match self.var_cell(t).map(|c| self.cell(c).clone()) {
            Some(TVar::Unbound(i)) if i.vcls.is_empty() => Some(i.vlevel),
            _ => None,
        }
    }

    fn settle_empty(&mut self, e: &Entry) -> R<()> {
        if self.empty_open_scrutinee(e).is_some() {
            let t = self.repr(e.ty);
            if self.var_cell(t).is_some() {
                let n = self.t_never;
                match self.unify(t, n) {
                    Err(Fail::Type(_)) | Ok(()) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    fn undetermined(&mut self, ty: Ty) -> bool {
        let t = self.repr(ty);
        match self.var_cell(t).map(|c| self.cell(c).clone()) {
            Some(TVar::Unbound(i)) => !i.vcls.iter().any(|c| self.is_predicate(*c)),
            _ => false,
        }
    }

    fn ty_key(&mut self, ty: Ty, b: &mut String) -> R<()> {
        guard()?;
        let ty = self.repr(ty);
        match self.node(ty) {
            TyNode::Con(n, args) => {
                b.push_str(&n.to_string());
                b.push('[');
                for a in args.iter() {
                    self.ty_key(*a, b)?;
                    b.push(',');
                }
                b.push(']');
            }
            TyNode::Record(r) => {
                b.push('{');
                self.ty_key(r, b)?;
                b.push('}');
            }
            TyNode::Variant(r) => {
                b.push('<');
                self.ty_key(r, b)?;
                b.push('>');
            }
            TyNode::RowEmpty => b.push('.'),
            TyNode::RowExtend(l, f, rest) => {
                b.push_str(&l.to_string());
                b.push(':');
                self.ty_key(f, b)?;
                b.push(';');
                self.ty_key(rest, b)?;
            }
            TyNode::Arrow(..) | TyNode::App(..) | TyNode::Var(_) => b.push('_'),
        }
        Ok(())
    }

    fn inhabited(&mut self, ty: Ty) -> R<bool> {
        let mut memo = HashMap::new();
        self.inhabited_go(&mut memo, &[], ty)
    }

    fn inhabited_go(&mut self, memo: &mut HashMap<String, bool>, seen: &[Oid], ty: Ty) -> R<bool> {
        guard()?;
        let ty = self.repr(ty);
        match self.node(ty) {
            TyNode::Con(n, _) if self.d.datas.contains_key(&n) => {
                let mut k = String::new();
                self.ty_key(ty, &mut k)?;
                match memo.get(&k) {
                    Some(r) => Ok(*r),
                    None => {
                        if seen.contains(&n) {
                            Ok(true)
                        } else {
                            let mut s2 = vec![n];
                            s2.extend_from_slice(seen);
                            let r = self.inhabited_expand(memo, &s2, ty)?;
                            memo.insert(k, r);
                            Ok(r)
                        }
                    }
                }
            }
            _ => self.inhabited_expand(memo, seen, ty),
        }
    }

    fn inhabited_expand(
        &mut self,
        memo: &mut HashMap<String, bool>,
        seen: &[Oid],
        ty: Ty,
    ) -> R<bool> {
        match self.complete_sig(ty) {
            None => Ok(true),
            Some(cs) => {
                for c in cs {
                    let subs = self.sub_types(&c, ty)?;
                    let mut all = true;
                    for s in subs {
                        if !self.inhabited_go(memo, seen, s)? {
                            all = false;
                            break;
                        }
                    }
                    if all {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    fn check_entry(&mut self, e: &Entry) -> R<Vec<String>> {
        let mut out = Vec::new();
        let mut all: Vec<(Vec<IPat>, bool)> = Vec::new();
        for (p, g) in &e.rows {
            all.push((vec![self.ex_convert(p)?], *g));
        }
        let unguarded: Vec<Vec<IPat>> = all
            .iter()
            .filter(|(_, g)| !g)
            .map(|(r, _)| r.clone())
            .collect();
        let tys = [e.ty];
        self.close_variant_rows(&unguarded, &tys)?;
        let single = e.rows.len() == 1;
        if single && !self.inhabited(e.ty)? {
            return Ok(Vec::new());
        }
        match self.missing(&unguarded, &tys)? {
            Some(_) if e.rows.is_empty() && self.undetermined(e.ty) => out.push(
                "match が非網羅的です。照合対象の型は、網羅性を調べた時点で Never のようなコンストラクタを持たない型と分かっていません(節のない match {} が網羅的なのは、照合対象がそのような型のときだけです)".to_string(),
            ),
            Some(w) => out.push(format!(
                "match が非網羅的です。例えば {} が漏れています",
                w.iter().map(|p| self.show_ipat(p)).collect::<Vec<_>>().join(", ")
            )),
            None => {}
        }
        for (i, (row, _)) in all.iter().enumerate() {
            let prior: Vec<Vec<IPat>> = all[..i]
                .iter()
                .filter(|(_, g)| !g)
                .map(|(r, _)| r.clone())
                .collect();
            if !self.useful(&prior, row, &tys)? {
                out.push(if single {
                    "このパターンには一致する値がありません".to_string()
                } else {
                    format!("第 {} 節は到達不能です(冗長)", i + 1)
                });
            }
        }
        Ok(out)
    }

    fn mentions_var(&mut self, r: Cell2, t: Ty) -> R<bool> {
        guard()?;
        let t = self.repr(t);
        Ok(match self.node(t) {
            TyNode::Var(v) => v == r,
            TyNode::Con(_, args) => {
                for a in args.iter() {
                    if self.mentions_var(r, *a)? {
                        return Ok(true);
                    }
                }
                false
            }
            TyNode::App(f, a) => self.mentions_var(r, f)? || self.mentions_var(r, a)?,
            TyNode::Arrow(a, ret, e) => {
                self.mentions_var(r, a)? || self.mentions_var(r, ret)? || self.mentions_var(r, e)?
            }
            TyNode::Record(row) | TyNode::Variant(row) => self.mentions_var(r, row)?,
            TyNode::RowEmpty => false,
            TyNode::RowExtend(_, f, rest) => {
                self.mentions_var(r, f)? || self.mentions_var(r, rest)?
            }
        })
    }

    /// 待ち行列を検査して警告を返す(Diktor の `Exhaust.drain`)。
    pub(crate) fn ex_drain(&mut self, level: i32, tys: &[Ty], gen_: bool) -> R<Vec<String>> {
        let entries = std::mem::take(&mut self.pending);
        let mut later = Vec::new();
        let mut now = Vec::new();
        for e in entries {
            let defer = match self.empty_open_scrutinee(&e) {
                None => false,
                Some(l) => level > 0 && (l <= level || (self.entry_visible(&e, tys)? && !gen_)),
            };
            if defer {
                later.push(e);
            } else {
                now.push(e);
            }
        }
        self.pending = later;
        for e in &now {
            if let Some(l) = self.empty_open_scrutinee(e)
                && l > level
                && !self.entry_visible(e, tys)?
            {
                self.settle_empty(e)?;
            }
        }
        let mut out = Vec::new();
        for e in &now {
            out.extend(self.check_entry(e)?);
        }
        Ok(out)
    }

    fn entry_visible(&mut self, e: &Entry, tys: &[Ty]) -> R<bool> {
        let t = self.repr(e.ty);
        match self.var_cell(t) {
            Some(r) => {
                for &ty in tys {
                    if self.mentions_var(r, ty)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            None => Ok(false),
        }
    }

    /// 照合の検査に使う型の網羅性(Diktor の `Exhaust.missing` を、パターンの列に当てたもの)。
    pub(crate) fn ex_missing_pats(&mut self, pats: &[&Pat], tys: &[Ty]) -> R<bool> {
        let mut row = Vec::new();
        for p in pats {
            row.push(self.ex_convert(p)?);
        }
        Ok(self.missing(&[row], tys)?.is_none())
    }
}

type Cell2 = super::Cell;

fn align_record_pat(labels: &[Oid], fields: &[(Oid, IPat)]) -> Vec<IPat> {
    let mut used = vec![false; fields.len()];
    labels
        .iter()
        .map(|l| {
            for (i, (l2, p)) in fields.iter().enumerate() {
                if l2 == l && !used[i] {
                    used[i] = true;
                    return p.clone();
                }
            }
            IPat::Wild
        })
        .collect()
}

fn default_matrix(rows: &[Vec<IPat>]) -> Vec<Vec<IPat>> {
    rows.iter()
        .filter_map(|r| match r.split_first() {
            Some((IPat::Wild, rest)) => Some(rest.to_vec()),
            _ => None,
        })
        .collect()
}

fn sig_complete(sig: &Option<Vec<Ctor>>, roots: &[Ctor]) -> bool {
    match sig {
        Some(s) if s.is_empty() => true,
        Some(s) => s.iter().all(|c| roots.contains(c)),
        None => false,
    }
}

fn show_lit(l: &Lit) -> String {
    match l {
        Lit::Bool(b) => b.to_string(),
        Lit::Text(s) => format!("\"{}\"", ocaml_escaped(s.as_bytes())),
        Lit::Num(n) => n.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocaml_の整数の読みと同じ() {
        assert_eq!(ocaml_int32_opt("0xFFFFFFFF"), Some(-1));
        assert_eq!(ocaml_int32_opt("2147483648"), None);
        assert_eq!(ocaml_int32_opt("-2147483648"), Some(i32::MIN));
        assert_eq!(ocaml_int32_opt("1_000"), Some(1000));
        assert_eq!(ocaml_int32_opt("0x1_0"), Some(16));
        assert_eq!(ocaml_int32_opt("_1"), None);
        assert_eq!(ocaml_int64_opt("0xFFFFFFFFFFFFFFFF"), Some(-1));
        assert_eq!(ocaml_int64_opt("9223372036854775808"), None);
        assert_eq!(ocaml_int32_opt("1.5"), None);
    }

    #[test]
    fn ocaml_の浮動小数の読みと同じ() {
        assert_eq!(ocaml_float_opt("1."), Some(1.0));
        assert_eq!(ocaml_float_opt("1.e5"), Some(1e5));
        assert_eq!(ocaml_float_opt("1_0.5"), Some(10.5));
        assert_eq!(ocaml_float_opt("0x1F"), Some(31.0));
        assert_eq!(ocaml_float_opt("0b1"), None);
        assert_eq!(norm_text("1"), norm_text("0x1"));
        assert_ne!(norm_text("1."), norm_text("1"));
        assert_eq!(norm_text("1."), norm_text("1.0"));
        assert_eq!(norm_text("0.0"), norm_text("-0.0"));
    }
}
