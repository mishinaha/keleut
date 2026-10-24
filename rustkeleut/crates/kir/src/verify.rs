//! KIR の検証(IRSpec 13.8 節、13.9 節)。
//!
//! 読み込んだプログラムが KIR の規則を満たすかを確かめる。
//! 検証は型の健全性を保証せず、型を使わずに計算できる性質だけを確かめる。
//! 確かめる順と誤りの文面は Diktor の第21章(`ir_check.ml`)に合わせてある。
//! 最初に見つけた誤りを返し、残りは調べない。
//!
//! | 性質 | 確かめ方 |
//! |---|---|
//! | 名前 | 表ごとに記号が一意で、`単位の鍵/名前` の形をし、単位の鍵が単位の表にある |
//! | 組み込みの表 | 組み込みの型、クラス、値が付録 A の順で表の先頭にあり、ほかに組み込みの項目が無い |
//! | 操作の印 | `runtime` の印はプレリュードの `Console` の `write` だけに付き、そのエフェクトを `handle` しない |
//! | 関数の形 | 種類ごとの引数、辞書、捕獲の数と、関数を作る場所との対応 |
//! | スロット | 番号がテキストの順に 1 ずつ増え、範囲にあり、束縛の有効範囲の中でだけ読む |
//! | 捕獲 | 個数と Rep が、作られる関数の欄と合う |
//! | 辞書 | 使う位置のクラスと辞書のクラスが合い、`dictapp` の辞書の数が受け取る側と合う |
//! | インスタンス | 実装がクラスのメソッドをちょうど 1 回ずつ覆う |
//! | 末尾の印 | 再計算した末尾位置と一致する |
//! | 操作節の分類 | 再計算した N、T、G と一致する |
//! | `resume` の位置 | 操作節の本体か、その下のハンドラ式の本体の中だけにある |
//! | 初期化 | 大域とインスタンスをちょうど 1 回ずつ定義する |
//!
//! プログラムは任意の値でありうるので、表の添字もすべて範囲を確かめてから引く。
//! 再帰はノードの深さで数え、深さが限度を超えたら拒否する。

use std::collections::{HashMap, HashSet};

use crate::builtins;
use crate::error::{Error, ErrorKind, VERIFY_LIMIT_MESSAGE};
use crate::model::*;

type V<T> = Result<T, Error>;

macro_rules! fail {
    ($($arg:tt)*) => {
        return Err(Error::new(ErrorKind::Invalid, format!($($arg)*)))
    };
}

fn limit<T>() -> V<T> {
    Err(Error::new(ErrorKind::Limit, VERIFY_LIMIT_MESSAGE))
}

/// 関数を作った場所。
#[derive(Clone, Copy)]
enum Origin {
    /// 大域、初期化の手順、インスタンスの項目が指す根の関数。
    Root,
    /// ほかの関数の本体の中の `closure`、`dictabs`、`handle` の部分。
    Site {
        parent: FnId,
        h_tail: bool,
        /// 操作節なら(エフェクト, 操作, 書いた分類)。
        h_op: Option<(EffectId, u32, ClauseClass)>,
        /// 操作節の guard の欄の中で作られたなら真。
        in_guard: bool,
    },
}

/// 持ち主をたどった結果の覚え書き([`Ctx::owner`])。
#[derive(Clone, Copy)]
enum OwnerMemo {
    Unseen,
    Walking,
    Done(Option<FnId>),
}

/// 式の構文ごとの確かめ方。
type CheckArm<'c> = fn(&Ctx<'c>, &mut FState<'_>, usize, bool, bool, &Exp) -> V<()>;

struct Ctx<'a> {
    p: &'a Program,
    lim: usize,
    origins: Vec<Option<Origin>>,
}

impl<'a> Ctx<'a> {
    fn ty(&self, t: TypeId) -> V<&'a Type> {
        match self.p.types.get(t.index()) {
            Some(x) => Ok(x),
            None => fail!("型の添字 {} が範囲外です", t.0),
        }
    }

    fn ctor(&self, t: TypeId, c: u32) -> V<&'a Ctor> {
        let ty = self.ty(t)?;
        match ty.ctors.get(c as usize) {
            Some(x) => Ok(x),
            None => fail!("型 {} のコンストラクタの添字 {c} が範囲外です", ty.sym),
        }
    }

    fn eff(&self, e: EffectId) -> V<&'a Effect> {
        match self.p.effects.get(e.index()) {
            Some(x) => Ok(x),
            None => fail!("エフェクトの添字 {} が範囲外です", e.0),
        }
    }

    fn op(&self, e: EffectId, o: u32) -> V<&'a Op> {
        let ef = self.eff(e)?;
        match ef.ops.get(o as usize) {
            Some(x) => Ok(x),
            None => fail!("エフェクト {} の操作の添字 {o} が範囲外です", ef.sym),
        }
    }

    fn cls(&self, c: ClassId) -> V<&'a Class> {
        match self.p.classes.get(c.index()) {
            Some(x) => Ok(x),
            None => fail!("クラスの添字 {} が範囲外です", c.0),
        }
    }

    fn meth(&self, c: ClassId, m: u32) -> V<&'a ClassMethod> {
        let cl = self.cls(c)?;
        match cl.methods.get(m as usize) {
            Some(x) => Ok(x),
            None => fail!("クラス {} のメソッドの添字 {m} が範囲外です", cl.sym),
        }
    }

    fn inst(&self, i: InstanceId) -> V<&'a Instance> {
        match self.p.instances.get(i.index()) {
            Some(x) => Ok(x),
            None => fail!("インスタンスの添字 {} が範囲外です", i.0),
        }
    }

    fn glob(&self, g: GlobalId) -> V<&'a Global> {
        match self.p.globals.get(g.index()) {
            Some(x) => Ok(x),
            None => fail!("大域の添字 {} が範囲外です", g.0),
        }
    }

    fn func(&self, f: FnId) -> V<&'a Function> {
        match self.p.functions.get(f.index()) {
            Some(x) => Ok(x),
            None => fail!("関数の添字 {} が範囲外です", f.0),
        }
    }

    fn exp(&self, id: ExpId) -> V<&'a Exp> {
        match self.p.nodes.exp(id) {
            Some(x) => Ok(x),
            None => fail!("式のノード {} がありません", id.0),
        }
    }

    fn dict(&self, id: DictId) -> V<&'a Dict> {
        match self.p.nodes.dict(id) {
            Some(x) => Ok(x),
            None => fail!("辞書のノード {} がありません", id.0),
        }
    }

    fn pat(&self, id: PatId) -> V<&'a Pat> {
        match self.p.nodes.pat(id) {
            Some(x) => Ok(x),
            None => fail!("パターンのノード {} がありません", id.0),
        }
    }

    fn nest(&self, d: usize) -> V<()> {
        if d > self.lim {
            return limit();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 表と名前
// ---------------------------------------------------------------------------

fn unique_syms<'x>(what: &str, syms: impl Iterator<Item = &'x str>) -> V<()> {
    let mut seen = HashSet::new();
    for s in syms {
        if !seen.insert(s) {
            fail!("{what}の記号が重なります: {s}");
        }
    }
    Ok(())
}

fn unique_names<'x>(what: &str, owner: &str, names: impl Iterator<Item = &'x str>) -> V<()> {
    let mut seen = HashSet::new();
    for n in names {
        if !seen.insert(n) {
            fail!("{what} {owner} の中で名前 {n} が重なります");
        }
    }
    Ok(())
}

fn symbol_form(units: &HashSet<&str>, what: &str, sym: &str) -> V<()> {
    match sym.rfind('/') {
        None => fail!("{what}の記号 {sym} が 単位の鍵/名前 の形ではありません"),
        Some(i) => {
            if i == sym.len() - 1 {
                fail!("{what}の記号 {sym} の名前の部分が空です");
            }
            if !units.contains(&sym[..i]) {
                fail!(
                    "{what}の記号 {sym} の単位の鍵 {} が単位の表にありません",
                    &sym[..i]
                );
            }
        }
    }
    Ok(())
}

/// `runtime` の印が付いてよい操作(IRSpec 13.8 節の 14)。
fn is_runtime_op(e: &Effect, o: &Op) -> bool {
    e.sym == "prelude/Console" && o.name == "write" && o.arity == 1
}

// ---------------------------------------------------------------------------
// 関数を作る場所
// ---------------------------------------------------------------------------

impl Ctx<'_> {
    fn sites(
        &self,
        f: FnId,
        in_guard: bool,
        e: ExpId,
        d: usize,
        acc: &mut Vec<(FnId, FnKind, Origin)>,
    ) -> V<()> {
        self.nest(d)?;
        let n = d + 1;
        let site = |h_tail, h_op| Origin::Site {
            parent: f,
            h_tail,
            h_op,
            in_guard,
        };
        match self.exp(e)? {
            Exp::Lit(_)
            | Exp::LitError(_)
            | Exp::Local(_)
            | Exp::Env(_)
            | Exp::Global(_)
            | Exp::RecEmpty
            | Exp::Hole
            | Exp::Method { .. } => {}
            Exp::Closure(pt) => acc.push((pt.func, FnKind::Lambda, site(false, None))),
            Exp::Dictabs(pt) => acc.push((pt.func, FnKind::Dictabs, site(false, None))),
            Exp::Construct { fields, .. } => {
                for (_, x) in fields {
                    self.sites(f, in_guard, *x, n, acc)?;
                }
            }
            Exp::Variant { value: x, .. }
            | Exp::Not(x)
            | Exp::RecSelect { record: x, .. }
            | Exp::RecRestrict { record: x, .. }
            | Exp::Resume { value: x, .. }
            | Exp::Dictapp { func: x, .. } => self.sites(f, in_guard, *x, n, acc)?,
            Exp::Call { func, args, .. } => {
                self.sites(f, in_guard, *func, n, acc)?;
                for x in args {
                    self.sites(f, in_guard, *x, n, acc)?;
                }
            }
            Exp::Opcall { args: xs, .. } | Exp::Perform { args: xs, .. } | Exp::Seq(xs) => {
                for x in xs {
                    self.sites(f, in_guard, *x, n, acc)?;
                }
            }
            Exp::And(a, b)
            | Exp::Or(a, b)
            | Exp::Let {
                value: a, body: b, ..
            }
            | Exp::Letpat {
                value: a, body: b, ..
            }
            | Exp::RecExtend {
                value: a,
                record: b,
                ..
            }
            | Exp::RecUpdate {
                record: a,
                value: b,
                ..
            } => {
                self.sites(f, in_guard, *a, n, acc)?;
                self.sites(f, in_guard, *b, n, acc)?;
            }
            Exp::Letrec { items, body } => {
                for (_, x) in items {
                    self.sites(f, in_guard, *x, n, acc)?;
                }
                self.sites(f, in_guard, *body, n, acc)?;
            }
            Exp::Match { scrutinee, clauses } => {
                self.sites(f, in_guard, *scrutinee, n, acc)?;
                for c in clauses {
                    if let Some(g) = c.guard {
                        self.sites(f, in_guard, g, n, acc)?;
                    }
                    self.sites(f, in_guard, c.body, n, acc)?;
                }
            }
            Exp::Handle(h) => {
                acc.push((h.body.func, FnKind::Handlebody, site(h.tail, None)));
                for o in &h.ops {
                    acc.push((
                        o.part.func,
                        FnKind::Opclause,
                        site(h.tail, Some((h.effect, o.op, o.class))),
                    ));
                }
                if let Some(pt) = &h.ret {
                    acc.push((pt.func, FnKind::Retclause, site(h.tail, None)));
                }
                if let Some(pt) = &h.cancel {
                    acc.push((pt.func, FnKind::Cancelclause, site(h.tail, None)));
                }
            }
        }
        Ok(())
    }

    fn origins(&mut self) -> V<()> {
        let p = self.p;
        let mut origins: Vec<Option<Origin>> = vec![None; p.functions.len()];
        let mut add = |ctx: &Ctx<'_>, f: FnId, kind: FnKind, o: Origin| -> V<()> {
            let func = ctx.func(f)?;
            if origins[f.index()].is_some() {
                fail!("関数 {} を作る場所が 2 つ以上あります", func.sym);
            }
            if func.kind != kind {
                fail!(
                    "関数 {} の種類 {} が、作る場所の求める {} と違います",
                    func.sym,
                    func.kind.name(),
                    kind.name()
                );
            }
            origins[f.index()] = Some(o);
            Ok(())
        };
        for g in &p.globals {
            match g.kind {
                GlobalKind::Fn(f) => add(self, f, FnKind::Lambda, Origin::Root)?,
                GlobalKind::Dictabs(f) => add(self, f, FnKind::Dictabs, Origin::Root)?,
                _ => {}
            }
        }
        for u in &p.units {
            for s in &u.init {
                match s {
                    Step::DefValue { func, .. }
                    | Step::DefPattern { func, .. }
                    | Step::RunExp(func) => add(self, *func, FnKind::Thunk, Origin::Root)?,
                    _ => {}
                }
            }
        }
        for i in &p.instances {
            for m in &i.methods {
                add(self, m.func, FnKind::Dictabs, Origin::Root)?;
            }
        }
        for (i, f) in p.functions.iter().enumerate() {
            let fid = FnId(i as u32);
            let mut acc = Vec::new();
            if let Some(g) = f.guard {
                self.sites(fid, true, g, 1, &mut acc)?;
            }
            self.sites(fid, false, f.body, 1, &mut acc)?;
            for (g, kind, o) in acc {
                add(self, g, kind, o)?;
            }
        }
        for (i, f) in p.functions.iter().enumerate() {
            if origins[i].is_none() {
                fail!("関数 {} を作る場所がありません", f.sym);
            }
        }
        self.origins = origins;
        Ok(())
    }

    fn origin(&self, f: FnId) -> Origin {
        self.origins[f.index()].unwrap_or(Origin::Root)
    }
}

// ---------------------------------------------------------------------------
// 関数の欄と本体
// ---------------------------------------------------------------------------

struct FState<'a> {
    f: &'a Function,
    slots: Vec<Rep>,
    scope: Vec<bool>,
    next: usize,
    resumes: Vec<bool>,
    in_guard: bool,
}

impl FState<'_> {
    fn sym(&self) -> &str {
        &self.f.sym
    }

    fn var_ok(&self, v: Cap) -> V<()> {
        match v {
            Cap::Local(i) => {
                let i = i as usize;
                if i >= self.slots.len() {
                    fail!("関数 {}: スロット {i} が範囲外です", self.sym());
                }
                if !self.scope[i] {
                    fail!(
                        "関数 {}: スロット {i} を束縛の有効範囲の外で読みます",
                        self.sym()
                    );
                }
            }
            Cap::Env(i) => {
                if i as usize >= self.f.caps.len() {
                    fail!("関数 {}: 捕獲 {i} が範囲外です", self.sym());
                }
            }
            Cap::DParam(i) => {
                if i >= self.f.dicts {
                    fail!("関数 {}: 辞書の引数 {i} が範囲外です", self.sym());
                }
            }
        }
        Ok(())
    }

    fn var_rep(&self, v: Cap) -> Rep {
        match v {
            Cap::Local(i) => self.slots[i as usize],
            Cap::Env(i) => self.f.caps[i as usize],
            Cap::DParam(_) => Rep::Any,
        }
    }

    fn bind(&mut self, i: u32) -> V<()> {
        let i = i as usize;
        if i != self.next {
            fail!(
                "関数 {}: スロット {i} の束縛が現れる順と合いません(次は {})",
                self.sym(),
                self.next
            );
        }
        if i >= self.slots.len() {
            fail!("関数 {}: スロット {i} が範囲外です", self.sym());
        }
        self.next = i + 1;
        Ok(())
    }

    fn set_scope(&mut self, lo: usize, hi: usize, on: bool) {
        for s in &mut self.scope[lo..hi] {
            *s = on;
        }
    }

    fn mark(&self, what: &str, want: bool, got: bool) -> V<()> {
        if want != got {
            fail!(
                "関数 {}: {what} の末尾の印が、再計算した結果と違います",
                self.sym()
            );
        }
        Ok(())
    }
}

/// ラベルの列が、綴りのバイト列の昇順(同じラベルの重なりは許す)に並ぶか。
fn sorted_labels(st: &FState<'_>, what: &str, fs: &[(String, DictId)]) -> V<()> {
    for w in fs.windows(2) {
        if w[0].0 > w[1].0 {
            fail!("関数 {}: {what}のラベルが昇順に並んでいません", st.sym());
        }
    }
    Ok(())
}

fn builtin_name(sym: &str) -> Option<&str> {
    sym.strip_prefix("builtin/")
}

impl Ctx<'_> {
    fn check_shape(&self, origin: Origin, f: &Function) -> V<()> {
        let np = f.params.len();
        let need = |what: &str, ok: bool| -> V<()> {
            if !ok {
                fail!(
                    "関数 {}({})の{what}が規則と合いません",
                    f.sym,
                    f.kind.name()
                );
            }
            Ok(())
        };
        if let Origin::Root = origin {
            need("捕獲の数", f.caps.is_empty())?;
        }
        if f.kind != FnKind::Dictabs {
            need("辞書の引数の数", f.dicts == 0)?;
        }
        if f.kind != FnKind::Opclause {
            need("パターンとガード", f.pats.is_empty() && f.guard.is_none())?;
        }
        match f.kind {
            FnKind::Lambda => Ok(()),
            FnKind::Dictabs | FnKind::Thunk | FnKind::Handlebody | FnKind::Cancelclause => {
                need("引数の数", np == 0)
            }
            FnKind::Retclause => need("引数の数", np == 1),
            FnKind::Opclause => {
                need("パターンの数", f.pats.len() == np)?;
                match origin {
                    Origin::Site {
                        h_op: Some((e, o, _)),
                        ..
                    } => need("引数の数", np == self.op(e, o)?.arity as usize),
                    _ => need("作る場所", false),
                }
            }
        }
    }

    /// 辞書の式を確かめる。`cls` は、その位置で辞書に求めるクラスで、型を使わずに分かる位置でだけ `Some` である。
    fn check_dict(&self, st: &FState<'_>, d: usize, cls: Option<ClassId>, id: DictId) -> V<()> {
        self.nest(d)?;
        let want = |what: &str, c: ClassId| -> V<()> {
            if let Some(k) = cls
                && k != c
            {
                fail!(
                    "関数 {}: クラス {} の辞書の位置に、クラス {} の{what}があります",
                    st.sym(),
                    self.cls(k)?.sym,
                    self.cls(c)?.sym
                );
            }
            Ok(())
        };
        match self.dict(id)? {
            Dict::DParam(i) => st.var_ok(Cap::DParam(*i)),
            Dict::Env(i) => st.var_ok(Cap::Env(*i)),
            Dict::Inst { instance, premises } => {
                let inst = self.inst(*instance)?;
                want("インスタンス", inst.class)?;
                if premises.len() != inst.premises as usize {
                    fail!(
                        "関数 {}: インスタンス {} の前提の数が違います",
                        st.sym(),
                        inst.sym
                    );
                }
                for &q in premises {
                    self.check_dict(st, d + 1, None, q)?;
                }
                Ok(())
            }
            Dict::Record(fs) | Dict::Variant(fs) => {
                let eq = self.builtin_eq();
                match eq {
                    Some(eq) => want("構造的な辞書", eq)?,
                    None => fail!("関数 {}: クラス builtin/Eq がありません", st.sym()),
                }
                sorted_labels(st, "構造的な辞書", fs)?;
                for (_, q) in fs {
                    self.check_dict(st, d + 1, eq, *q)?;
                }
                Ok(())
            }
            Dict::Builtin { class, ty } => {
                let c = self.cls(*class)?;
                let t = self.ty(*ty)?;
                let ok = match (builtin_name(&c.sym), builtin_name(&t.sym)) {
                    (Some(cn), Some(tn)) => builtins::is_builtin_instance(cn, tn),
                    _ => false,
                };
                if !ok {
                    fail!(
                        "関数 {}: 組み込みのインスタンス {}[{}] は付録 A.3 にありません",
                        st.sym(),
                        c.sym,
                        t.sym
                    );
                }
                want("組み込みのインスタンス", *class)
            }
        }
    }

    /// クラスの表で `builtin/Eq` の位置。
    fn builtin_eq(&self) -> Option<ClassId> {
        self.p
            .classes
            .iter()
            .position(|c| c.sym == "builtin/Eq")
            .map(|i| ClassId(i as u32))
    }

    fn check_pat(&self, st: &mut FState<'_>, d: usize, id: PatId) -> V<()> {
        self.nest(d)?;
        match self.pat(id)? {
            Pat::Wild | Pat::Lit(_) => {}
            Pat::Bind(i) => st.bind(*i)?,
            Pat::Record { fields, tail } => {
                for (_, q) in fields {
                    self.check_pat(st, d + 1, *q)?;
                }
                if let RecordTail::Rest(q) = tail {
                    self.check_pat(st, d + 1, *q)?;
                }
            }
            Pat::Ctor { ty, ctor, fields } => {
                let ct = self.ctor(*ty, *ctor)?;
                let mut prev: i64 = -1;
                for (i, _) in fields {
                    if i64::from(*i) <= prev || *i >= ct.arity {
                        fail!(
                            "関数 {}: コンストラクタ {} のパターンのフィールドの位置 {i} が不正です",
                            st.sym(),
                            ct.name
                        );
                    }
                    prev = i64::from(*i);
                }
                for (_, q) in fields {
                    self.check_pat(st, d + 1, *q)?;
                }
            }
            Pat::Variant { pat, .. } => self.check_pat(st, d + 1, *pat)?,
        }
        Ok(())
    }

    /// 関数を作る部分の捕獲の列は、作られる関数の `caps` と同じ数で、同じ Rep でなければならない。
    fn check_part(&self, st: &FState<'_>, kind: FnKind, pt: &Part) -> V<()> {
        let g = self.func(pt.func)?;
        if g.kind != kind {
            fail!("関数 {}: {} の種類が違います", st.sym(), g.sym);
        }
        if pt.caps.len() != g.caps.len() {
            fail!("関数 {}: {} の捕獲の数が違います", st.sym(), g.sym);
        }
        for (&v, &r) in pt.caps.iter().zip(&g.caps) {
            st.var_ok(v)?;
            if st.var_rep(v) != r {
                fail!("関数 {}: {} の捕獲の Rep が違います", st.sym(), g.sym);
            }
        }
        Ok(())
    }

    /// 式を確かめる。`tail` はその位置が末尾位置か、`spine` は操作節の背骨の上かである。
    ///
    /// 再帰の 1 段のスタックを小さく保つため、構文ごとの確かめ方を別の関数に分け、
    /// ここでは式の形から関数を選んで呼ぶだけにする。
    fn check_exp(
        &self,
        st: &mut FState<'_>,
        d: usize,
        tail: bool,
        spine: bool,
        id: ExpId,
    ) -> V<()> {
        self.nest(d)?;
        let e = self.exp(id)?;
        let arm: CheckArm<'_> = match e {
            Exp::Lit(_)
            | Exp::LitError(_)
            | Exp::RecEmpty
            | Exp::Hole
            | Exp::Local(_)
            | Exp::Env(_)
            | Exp::Global(_) => Self::ck_leaf,
            Exp::Construct { .. } => Self::ck_construct,
            Exp::Variant { .. }
            | Exp::Not(_)
            | Exp::RecSelect { .. }
            | Exp::RecRestrict { .. }
            | Exp::RecExtend { .. }
            | Exp::RecUpdate { .. } => Self::ck_plain,
            Exp::Call { .. } => Self::ck_call,
            Exp::Method { .. } | Exp::Opcall { .. } => Self::ck_method,
            Exp::And(..) | Exp::Or(..) => Self::ck_andor,
            Exp::Closure(_) | Exp::Dictabs(_) => Self::ck_part,
            Exp::Dictapp { .. } => Self::ck_dictapp,
            Exp::Let { .. } => Self::ck_let,
            Exp::Letpat { .. } => Self::ck_letpat,
            Exp::Letrec { .. } => Self::ck_letrec,
            Exp::Seq(_) => Self::ck_seq,
            Exp::Match { .. } => Self::ck_match,
            Exp::Perform { .. } => Self::ck_perform,
            Exp::Handle(_) => Self::ck_handle,
            Exp::Resume { .. } => Self::ck_resume,
        };
        arm(self, st, d + 1, tail, spine, e)
    }

    fn ck_leaf(&self, st: &mut FState<'_>, _n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        match e {
            Exp::Local(i) => st.var_ok(Cap::Local(*i)),
            Exp::Env(i) => st.var_ok(Cap::Env(*i)),
            Exp::Global(g) => self.glob(*g).map(|_| ()),
            _ => Ok(()),
        }
    }

    fn ck_construct(&self, st: &mut FState<'_>, n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        let Exp::Construct { ty, ctor, fields } = e else {
            return Ok(());
        };
        let ct = self.ctor(*ty, *ctor)?;
        // フィールドの数はファイルが決める任意の u32 なので、その大きさの表は作らない
        let mut seen = HashSet::new();
        for (i, _) in fields {
            if *i >= ct.arity || !seen.insert(*i) {
                fail!(
                    "関数 {}: コンストラクタ {} のフィールドの位置 {i} が不正です",
                    st.sym(),
                    ct.name
                );
            }
        }
        if seen.len() != ct.arity as usize {
            fail!(
                "関数 {}: コンストラクタ {} のフィールドが足りません",
                st.sym(),
                ct.name
            );
        }
        for (_, x) in fields {
            self.check_exp(st, n, false, false, *x)?;
        }
        Ok(())
    }

    /// 子がどれも末尾位置にない、検査の無い構文。
    fn ck_plain(&self, st: &mut FState<'_>, n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        match e {
            Exp::Variant { value: x, .. }
            | Exp::Not(x)
            | Exp::RecSelect { record: x, .. }
            | Exp::RecRestrict { record: x, .. } => self.check_exp(st, n, false, false, *x),
            Exp::RecExtend {
                value: a,
                record: b,
                ..
            }
            | Exp::RecUpdate {
                record: a,
                value: b,
                ..
            } => {
                self.check_exp(st, n, false, false, *a)?;
                self.check_exp(st, n, false, false, *b)
            }
            _ => Ok(()),
        }
    }

    fn ck_call(&self, st: &mut FState<'_>, n: usize, tail: bool, _s: bool, e: &Exp) -> V<()> {
        let Exp::Call {
            tail: t,
            func,
            args,
            ..
        } = e
        else {
            return Ok(());
        };
        st.mark("call", tail, *t)?;
        self.check_exp(st, n, false, false, *func)?;
        for x in args {
            self.check_exp(st, n, false, false, *x)?;
        }
        Ok(())
    }

    fn ck_method(&self, st: &mut FState<'_>, n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        match e {
            Exp::Method {
                class,
                method,
                dict,
                own,
            } => {
                let m = self.meth(*class, *method)?;
                if own.len() != m.dicts as usize {
                    fail!(
                        "関数 {}: メソッド {} の辞書の数が違います",
                        st.sym(),
                        m.name
                    );
                }
                self.check_dict(st, n, Some(*class), *dict)?;
                for &o in own {
                    self.check_dict(st, n, None, o)?;
                }
                Ok(())
            }
            Exp::Opcall {
                class,
                method,
                dict,
                args,
            } => {
                let m = self.meth(*class, *method)?;
                if m.dicts != 0 {
                    fail!(
                        "関数 {}: 演算子の呼び出しのメソッド {} が自分の辞書を持ちます",
                        st.sym(),
                        m.name
                    );
                }
                self.check_dict(st, n, Some(*class), *dict)?;
                for x in args {
                    self.check_exp(st, n, false, false, *x)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn ck_andor(&self, st: &mut FState<'_>, n: usize, tail: bool, spine: bool, e: &Exp) -> V<()> {
        let (Exp::And(a, b) | Exp::Or(a, b)) = e else {
            return Ok(());
        };
        self.check_exp(st, n, false, false, *a)?;
        self.check_exp(st, n, tail, spine, *b)
    }

    fn ck_part(&self, st: &mut FState<'_>, _n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        match e {
            Exp::Closure(pt) => self.check_part(st, FnKind::Lambda, pt),
            Exp::Dictabs(pt) => self.check_part(st, FnKind::Dictabs, pt),
            _ => Ok(()),
        }
    }

    fn ck_dictapp(&self, st: &mut FState<'_>, n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        let Exp::Dictapp { func, dicts } = e else {
            return Ok(());
        };
        let k = dicts.len();
        let count = |what: &str, want: usize| -> V<()> {
            if k != want {
                fail!(
                    "関数 {}: {what} に渡す辞書の数 {k} が、受け取る数 {want} と違います",
                    st.sym()
                );
            }
            Ok(())
        };
        match self.exp(*func)? {
            Exp::Global(g) => {
                let gl = self.glob(*g)?;
                match &gl.kind {
                    GlobalKind::Extern { dicts, .. } => {
                        if *dicts == 0 {
                            fail!(
                                "関数 {}: 辞書を受け取らない extern の大域 {} に辞書を渡します",
                                st.sym(),
                                gl.sym
                            );
                        }
                        count(&gl.sym, *dicts as usize)?;
                    }
                    GlobalKind::Dictabs(f) => count(&gl.sym, self.func(*f)?.dicts as usize)?,
                    _ => fail!(
                        "関数 {}: 辞書を受け取らない大域 {} に辞書を渡します",
                        st.sym(),
                        gl.sym
                    ),
                }
            }
            Exp::Dictabs(pt) => {
                let f = self.func(pt.func)?;
                count(&f.sym, f.dicts as usize)?;
            }
            _ => {}
        }
        self.check_exp(st, n, false, false, *func)?;
        for &q in dicts {
            self.check_dict(st, n, None, q)?;
        }
        Ok(())
    }

    fn ck_let(&self, st: &mut FState<'_>, n: usize, tail: bool, spine: bool, e: &Exp) -> V<()> {
        let Exp::Let { slot, value, body } = e else {
            return Ok(());
        };
        st.bind(*slot)?;
        if matches!(self.exp(*value)?, Exp::Closure(_) | Exp::Dictabs(_))
            && st.slots[*slot as usize] != Rep::Any
        {
            fail!(
                "関数 {}: 関数を束縛するスロット {slot} の Rep が any でありません",
                st.sym()
            );
        }
        self.check_exp(st, n, false, false, *value)?;
        let i = *slot as usize;
        st.set_scope(i, i + 1, true);
        self.check_exp(st, n, tail, spine, *body)?;
        st.set_scope(i, i + 1, false);
        Ok(())
    }

    fn ck_letpat(&self, st: &mut FState<'_>, n: usize, tail: bool, spine: bool, e: &Exp) -> V<()> {
        let Exp::Letpat { pat, value, body } = e else {
            return Ok(());
        };
        let lo = st.next;
        self.check_pat(st, n, *pat)?;
        let hi = st.next;
        self.check_exp(st, n, false, false, *value)?;
        st.set_scope(lo, hi, true);
        self.check_exp(st, n, tail, spine, *body)?;
        st.set_scope(lo, hi, false);
        Ok(())
    }

    fn ck_letrec(&self, st: &mut FState<'_>, n: usize, tail: bool, spine: bool, e: &Exp) -> V<()> {
        let Exp::Letrec { items, body } = e else {
            return Ok(());
        };
        if items.is_empty() {
            fail!("関数 {}: letrec の項目がありません", st.sym());
        }
        let lo = st.next;
        for (i, _) in items {
            st.bind(*i)?;
            if st.slots[*i as usize] != Rep::Any {
                fail!(
                    "関数 {}: 関数を束縛するスロット {i} の Rep が any でありません",
                    st.sym()
                );
            }
        }
        let hi = st.next;
        st.set_scope(lo, hi, true);
        for (_, x) in items {
            match self.exp(*x)? {
                Exp::Closure(_) | Exp::Dictabs(_) => self.check_exp(st, n, false, false, *x)?,
                _ => fail!(
                    "関数 {}: letrec の項目が closure でも dictabs でもありません",
                    st.sym()
                ),
            }
        }
        self.check_exp(st, n, tail, spine, *body)?;
        st.set_scope(lo, hi, false);
        Ok(())
    }

    fn ck_seq(&self, st: &mut FState<'_>, n: usize, tail: bool, spine: bool, e: &Exp) -> V<()> {
        let Exp::Seq(xs) = e else {
            return Ok(());
        };
        if xs.len() < 2 {
            fail!("関数 {}: seq の要素が 2 個より少なくなっています", st.sym());
        }
        let last = xs.len() - 1;
        for (j, x) in xs.iter().enumerate() {
            if j == last {
                self.check_exp(st, n, tail, spine, *x)?;
            } else {
                self.check_exp(st, n, false, false, *x)?;
            }
        }
        Ok(())
    }

    fn ck_match(&self, st: &mut FState<'_>, n: usize, tail: bool, spine: bool, e: &Exp) -> V<()> {
        let Exp::Match { scrutinee, clauses } = e else {
            return Ok(());
        };
        self.check_exp(st, n, false, false, *scrutinee)?;
        for c in clauses {
            let lo = st.next;
            self.check_pat(st, n, c.pat)?;
            let hi = st.next;
            st.set_scope(lo, hi, true);
            if let Some(g) = c.guard {
                self.check_exp(st, n, false, false, g)?;
            }
            self.check_exp(st, n, tail, spine, c.body)?;
            st.set_scope(lo, hi, false);
        }
        Ok(())
    }

    fn ck_perform(&self, st: &mut FState<'_>, n: usize, _t: bool, _s: bool, e: &Exp) -> V<()> {
        let Exp::Perform { effect, op, args } = e else {
            return Ok(());
        };
        let o = self.op(*effect, *op)?;
        if args.len() != o.arity as usize {
            fail!("関数 {}: 操作 {} の引数の数が違います", st.sym(), o.name);
        }
        for x in args {
            self.check_exp(st, n, false, false, *x)?;
        }
        Ok(())
    }

    fn ck_handle(&self, st: &mut FState<'_>, _n: usize, tail: bool, _s: bool, e: &Exp) -> V<()> {
        let Exp::Handle(h) = e else {
            return Ok(());
        };
        st.mark("handle", tail, h.tail)?;
        if h.ops.is_empty() {
            fail!("関数 {}: ハンドラ式の操作節がありません", st.sym());
        }
        let ef = self.eff(h.effect)?;
        if ef.ops.iter().any(|o| is_runtime_op(ef, o)) {
            fail!(
                "関数 {}: runtime の印の付いた操作を持つエフェクト {} を handle します",
                st.sym(),
                ef.sym
            );
        }
        self.check_part(st, FnKind::Handlebody, &h.body)?;
        for o in &h.ops {
            self.op(h.effect, o.op)?;
            self.check_part(st, FnKind::Opclause, &o.part)?;
        }
        if let Some(pt) = &h.ret {
            self.check_part(st, FnKind::Retclause, pt)?;
        }
        if let Some(pt) = &h.cancel {
            self.check_part(st, FnKind::Cancelclause, pt)?;
        }
        Ok(())
    }

    fn ck_resume(&self, st: &mut FState<'_>, n: usize, _t: bool, spine: bool, e: &Exp) -> V<()> {
        let Exp::Resume { tail: t, value } = e else {
            return Ok(());
        };
        if st.in_guard {
            fail!("関数 {}: 操作節のガードに resume があります", st.sym());
        }
        if *t != spine {
            fail!(
                "関数 {}: resume の末尾の印が、再計算した背骨と違います",
                st.sym()
            );
        }
        st.resumes.push(*t);
        self.check_exp(st, n, false, false, *value)
    }

    /// 本体の根が末尾位置か、背骨の根かを決める(IRSpec 5.3 節)。
    /// N かどうかは、書いた分類ではなく操作の `never` の印で決める。
    fn roots(&self, origin: Origin, f: &Function) -> V<(bool, bool)> {
        let (never, htail) = match origin {
            Origin::Site {
                h_op: Some((e, o, _)),
                h_tail,
                ..
            } => (self.op(e, o)?.never, h_tail),
            Origin::Site { h_tail, .. } => (false, h_tail),
            Origin::Root => (false, false),
        };
        let tail = match f.kind {
            FnKind::Lambda => true,
            FnKind::Opclause => never && htail,
            FnKind::Retclause => htail,
            _ => false,
        };
        Ok((tail, f.kind == FnKind::Opclause && !never))
    }

    fn check_fn(&self, fid: FnId, f: &Function) -> V<Vec<bool>> {
        let origin = self.origin(fid);
        self.check_shape(origin, f)?;
        let mut slots = f.params.clone();
        slots.extend_from_slice(&f.locals);
        let np = f.params.len();
        let ns = slots.len();
        let mut st = FState {
            f,
            scope: (0..ns).map(|i| i < np).collect(),
            slots,
            next: np,
            resumes: Vec::new(),
            in_guard: false,
        };
        for &q in &f.pats {
            self.check_pat(&mut st, 1, q)?;
        }
        let hi = st.next;
        st.set_scope(np, hi, true);
        if let Some(g) = f.guard {
            st.in_guard = true;
            self.check_exp(&mut st, 1, false, false, g)?;
            st.in_guard = false;
        }
        let (tail, spine) = self.roots(origin, f)?;
        self.check_exp(&mut st, 1, tail, spine, f.body)?;
        if st.next != ns {
            fail!(
                "関数 {}: 束縛されないスロットがあります(束縛は {} 個、スロットは {ns} 個)",
                f.sym,
                st.next
            );
        }
        Ok(st.resumes)
    }

    /// `resume` を含む関数の持ち主(IRSpec 13.8 節の 9)。
    ///
    /// `handlebody` の関数の作る場所をたどる。
    /// 作る場所の鎖が輪になっていれば、どの操作節にも届かないので持ち主を持たない。
    ///
    /// たどった結果は `memo` に覚え、どの関数も一度しかたどらない。
    /// `handlebody` の関数が長い鎖をなすとき、関数ごとに鎖をたどり直すと入力の大きさの 2 乗の時間がかかるからである。
    /// `memo` の値は、まだたどっていなければ `Unseen`、いまたどっている途中なら `Walking`、決まれば `Done` である。
    /// たどる途中で `Walking` の関数に戻れば鎖は輪になっており、その途中の関数はどれも持ち主を持たない。
    fn owner(&self, memo: &mut [OwnerMemo], f: FnId) -> V<Option<FnId>> {
        let mut path = Vec::new();
        let mut cur = f;
        let found = loop {
            let func = self.func(cur)?;
            match memo[cur.index()] {
                OwnerMemo::Done(o) => break o,
                OwnerMemo::Walking => break None,
                OwnerMemo::Unseen => {}
            }
            memo[cur.index()] = OwnerMemo::Walking;
            path.push(cur);
            match (func.kind, self.origin(cur)) {
                (FnKind::Opclause, _) => break Some(cur),
                (
                    FnKind::Handlebody,
                    Origin::Site {
                        parent,
                        in_guard: false,
                        ..
                    },
                ) => cur = parent,
                _ => break None,
            }
        };
        for g in path {
            memo[g.index()] = OwnerMemo::Done(found);
        }
        Ok(found)
    }

    fn check_classes(&self, resumes: &[Vec<bool>]) -> V<()> {
        let p = self.p;
        let mut owned: HashMap<FnId, Vec<bool>> = HashMap::new();
        let mut memo = vec![OwnerMemo::Unseen; p.functions.len()];
        for (i, rs) in resumes.iter().enumerate() {
            if rs.is_empty() {
                continue;
            }
            let fid = FnId(i as u32);
            match self.owner(&mut memo, fid)? {
                None => fail!(
                    "関数 {}: resume を書けない関数に resume があります",
                    p.functions[i].sym
                ),
                Some(o) => owned.entry(o).or_default().extend_from_slice(rs),
            }
        }
        for (i, f) in p.functions.iter().enumerate() {
            let fid = FnId(i as u32);
            if let Origin::Site {
                h_op: Some((e, o, written)),
                ..
            } = self.origin(fid)
            {
                let never = self.op(e, o)?.never;
                let rs = owned.get(&fid).map(|v| v.as_slice()).unwrap_or(&[]);
                if never && !rs.is_empty() {
                    fail!("関数 {}: N の操作節に resume があります", f.sym);
                }
                let want = if never {
                    ClauseClass::N
                } else if rs.iter().all(|&t| t) {
                    ClauseClass::T
                } else {
                    ClauseClass::G
                };
                if want != written {
                    fail!(
                        "関数 {}: 操作節の分類 {} が、再計算した {} と違います",
                        f.sym,
                        written.name(),
                        want.name()
                    );
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 単位と初期化、インスタンス、組み込みの表
// ---------------------------------------------------------------------------

impl Ctx<'_> {
    fn def_pattern_binds(&self, id: PatId, d: usize, next: &mut u32) -> V<()> {
        self.nest(d)?;
        match self.pat(id)? {
            Pat::Wild | Pat::Lit(_) => {}
            Pat::Bind(i) => {
                if *i != *next {
                    fail!("def_pattern のパターンの bind {i} が大域の列の順と合いません");
                }
                *next += 1;
            }
            Pat::Record { fields, tail } => {
                for (_, q) in fields {
                    self.def_pattern_binds(*q, d + 1, next)?;
                }
                if let RecordTail::Rest(q) = tail {
                    self.def_pattern_binds(*q, d + 1, next)?;
                }
            }
            Pat::Ctor { ty, ctor, fields } => {
                let ct = self.ctor(*ty, *ctor)?;
                let mut prev: i64 = -1;
                for (i, _) in fields {
                    if i64::from(*i) <= prev || *i >= ct.arity {
                        fail!(
                            "def_pattern のコンストラクタ {} のフィールドの位置が不正です",
                            ct.name
                        );
                    }
                    prev = i64::from(*i);
                }
                for (_, q) in fields {
                    self.def_pattern_binds(*q, d + 1, next)?;
                }
            }
            Pat::Variant { pat, .. } => self.def_pattern_binds(*pat, d + 1, next)?,
        }
        Ok(())
    }

    fn check_units(&self) -> V<()> {
        let p = self.p;
        if let Some(u) = p.units.first()
            && !u.init.is_empty()
        {
            fail!("builtin の単位が初期化の手順を持ちます");
        }
        let mut defined: HashSet<GlobalId> = HashSet::new();
        let mut inst_defined: HashSet<InstanceId> = HashSet::new();
        let define = |defined: &mut HashSet<GlobalId>,
                      u: &Unit,
                      g: GlobalId,
                      ok: fn(&GlobalKind) -> bool,
                      what: &str|
         -> V<()> {
            let gl = self.glob(g)?;
            if !ok(&gl.kind) {
                fail!("大域 {} の種類が {what} の手順と合いません", gl.sym);
            }
            if unit_of_symbol(&gl.sym) != u.key {
                fail!("大域 {} を単位 {} の手順が定義します", gl.sym, u.key);
            }
            if !defined.insert(g) {
                fail!("大域 {} を 2 回定義します", gl.sym);
            }
            Ok(())
        };
        let is_value: fn(&GlobalKind) -> bool = |k| matches!(k, GlobalKind::Value(_));
        for u in &p.units {
            for s in &u.init {
                match s {
                    Step::DefValue { global, func } => {
                        define(&mut defined, u, *global, is_value, "def_value")?;
                        if let GlobalKind::Value(r) = self.glob(*global)?.kind
                            && self.func(*func)?.result != r
                        {
                            fail!(
                                "大域 {} の Rep が初期化の関数の結果の Rep と違います",
                                self.glob(*global)?.sym
                            );
                        }
                    }
                    Step::DefPattern { globals, pat, .. } => {
                        for &g in globals {
                            define(&mut defined, u, g, is_value, "def_pattern")?;
                        }
                        let mut next = 0u32;
                        self.def_pattern_binds(*pat, 1, &mut next)?;
                        if next as usize != globals.len() {
                            fail!("def_pattern のパターンが大域の列のすべてを束縛しません");
                        }
                    }
                    Step::DefFns(gs) => {
                        if gs.is_empty() {
                            fail!("def_fns の大域がありません");
                        }
                        for &g in gs {
                            define(
                                &mut defined,
                                u,
                                g,
                                |k| {
                                    matches!(
                                        k,
                                        GlobalKind::Fn(_)
                                            | GlobalKind::Dictabs(_)
                                            | GlobalKind::Extern { .. }
                                    )
                                },
                                "def_fns",
                            )?;
                        }
                    }
                    Step::DefDictabs(g) => define(
                        &mut defined,
                        u,
                        *g,
                        |k| matches!(k, GlobalKind::Dictabs(_)),
                        "def_dictabs",
                    )?,
                    Step::DefInstance(i) => {
                        let inst = self.inst(*i)?;
                        if unit_of_symbol(&inst.sym) != u.key {
                            fail!(
                                "インスタンス {} を単位 {} の手順が定義します",
                                inst.sym,
                                u.key
                            );
                        }
                        if !inst_defined.insert(*i) {
                            fail!("インスタンス {} を 2 回定義します", inst.sym);
                        }
                    }
                    Step::RunExp(f) => {
                        self.func(*f)?;
                    }
                }
            }
        }
        for (i, g) in p.globals.iter().enumerate() {
            let gid = GlobalId(i as u32);
            match g.kind {
                GlobalKind::Builtin => {
                    if defined.contains(&gid) {
                        fail!("組み込みの大域 {} を手順が定義します", g.sym);
                    }
                }
                _ => {
                    if !defined.contains(&gid) {
                        fail!("大域 {} を定義する手順がありません", g.sym);
                    }
                }
            }
        }
        for (i, inst) in p.instances.iter().enumerate() {
            if !inst_defined.contains(&InstanceId(i as u32)) {
                fail!("インスタンス {} を定義する手順がありません", inst.sym);
            }
        }
        Ok(())
    }

    fn check_instances(&self) -> V<()> {
        for i in &self.p.instances {
            let cl = self.cls(i.class)?;
            self.ty(i.head)?;
            let mut names = Vec::new();
            for m in &i.methods {
                names.push(self.meth(i.class, m.method)?.name.as_str());
            }
            unique_names("インスタンス", &i.sym, names.into_iter())?;
            for m in &i.methods {
                let cm = self.meth(i.class, m.method)?;
                if m.dicts != cm.dicts {
                    fail!(
                        "インスタンス {} のメソッド {} の辞書の数がクラスと違います",
                        i.sym,
                        cm.name
                    );
                }
                let f = self.func(m.func)?;
                if u64::from(f.dicts) != u64::from(i.premises) + u64::from(m.dicts) {
                    fail!(
                        "インスタンス {} のメソッド {} の実装の辞書の引数の数が違います",
                        i.sym,
                        cm.name
                    );
                }
            }
            let mut implemented = vec![false; cl.methods.len()];
            for m in &i.methods {
                if let Some(b) = implemented.get_mut(m.method as usize) {
                    *b = true;
                }
            }
            for (k, cm) in cl.methods.iter().enumerate() {
                if !implemented[k] {
                    fail!(
                        "インスタンス {} がクラス {} のメソッド {} を実装しません",
                        i.sym,
                        cl.sym,
                        cm.name
                    );
                }
            }
        }
        Ok(())
    }

    fn check_builtins(&self) -> V<()> {
        let p = self.p;
        let is_builtin = |s: &str| unit_of_symbol(s) == "builtin";
        // 型
        if p.types.len() < builtins::TYPES.len()
            || !builtins::TYPES
                .iter()
                .zip(&p.types)
                .all(|(w, t)| builtin_name(&t.sym) == Some(*w) && t.ctors.is_empty())
        {
            fail!("型の表の先頭が、付録 A の組み込みの型の並びと合いません");
        }
        for t in &p.types[builtins::TYPES.len()..] {
            if is_builtin(&t.sym) {
                fail!("組み込みでない位置に組み込みの型 {} があります", t.sym);
            }
        }
        // クラス
        if p.classes.len() < builtins::CLASSES.len()
            || !builtins::CLASSES
                .iter()
                .zip(&p.classes)
                .all(|((w, ms), c)| {
                    builtin_name(&c.sym) == Some(*w)
                        && c.methods.len() == ms.len()
                        && c.methods
                            .iter()
                            .zip(ms.iter())
                            .all(|(m, n)| m.name == *n && m.dicts == 0)
                })
        {
            fail!("クラスの表の先頭が、付録 A の組み込みのクラスの並びと合いません");
        }
        for c in &p.classes[builtins::CLASSES.len()..] {
            if is_builtin(&c.sym) {
                fail!("組み込みでない位置に組み込みのクラス {} があります", c.sym);
            }
        }
        // 値
        if p.globals.len() < builtins::VALUES.len()
            || !builtins::VALUES
                .iter()
                .zip(&p.globals)
                .all(|(w, g)| builtin_name(&g.sym) == Some(*w) && g.kind == GlobalKind::Builtin)
        {
            fail!("大域の表の先頭が、付録 A の組み込みの値の並びと合いません");
        }
        for g in &p.globals[builtins::VALUES.len()..] {
            if is_builtin(&g.sym) || g.kind == GlobalKind::Builtin {
                fail!("組み込みでない位置に組み込みの値 {} があります", g.sym);
            }
        }
        for e in &p.effects {
            if is_builtin(&e.sym) {
                fail!("単位 builtin のエフェクト {} があります", e.sym);
            }
        }
        for i in &p.instances {
            if is_builtin(&i.sym) {
                fail!("単位 builtin のインスタンス {} があります", i.sym);
            }
        }
        for e in &p.effects {
            for o in &e.ops {
                if o.runtime != is_runtime_op(e, o) {
                    fail!(
                        "エフェクト {} の操作 {} の runtime の印が規則と合いません",
                        e.sym,
                        o.name
                    );
                }
            }
        }
        Ok(())
    }
}

/// プログラムを検証する。`max_nest` はノードの深さの上限(IRSpec 13.9 節の `N_nest`)である。
///
/// 深さに比例してスタックを使うので、深いプログラムは大きなスタックの上で呼ぶ
/// ([`crate::verify()`] はそうする)。
pub fn check(p: &Program, max_nest: usize) -> Result<(), Error> {
    unique_syms("型", p.types.iter().map(|t| t.sym.as_str()))?;
    unique_syms("エフェクト", p.effects.iter().map(|e| e.sym.as_str()))?;
    unique_syms("クラス", p.classes.iter().map(|c| c.sym.as_str()))?;
    unique_syms("インスタンス", p.instances.iter().map(|i| i.sym.as_str()))?;
    unique_syms("大域", p.globals.iter().map(|g| g.sym.as_str()))?;
    unique_syms("関数", p.functions.iter().map(|f| f.sym.as_str()))?;
    unique_syms("単位", p.units.iter().map(|u| u.key.as_str()))?;
    let keys: Vec<&str> = p.units.iter().map(|u| u.key.as_str()).collect();
    if !(keys.len() >= 3
        && keys[0] == "builtin"
        && keys[1] == "prelude"
        && keys[keys.len() - 1] == "main")
    {
        fail!("単位の列が builtin、prelude で始まり main で終わる形になっていません");
    }
    let units: HashSet<&str> = keys.into_iter().collect();
    for t in &p.types {
        symbol_form(&units, "型", &t.sym)?;
    }
    for e in &p.effects {
        symbol_form(&units, "エフェクト", &e.sym)?;
    }
    for c in &p.classes {
        symbol_form(&units, "クラス", &c.sym)?;
    }
    for i in &p.instances {
        symbol_form(&units, "インスタンス", &i.sym)?;
    }
    for g in &p.globals {
        symbol_form(&units, "大域", &g.sym)?;
    }
    for f in &p.functions {
        symbol_form(&units, "関数", &f.sym)?;
    }
    for t in &p.types {
        unique_names("型", &t.sym, t.ctors.iter().map(|c| c.name.as_str()))?;
    }
    for e in &p.effects {
        unique_names("エフェクト", &e.sym, e.ops.iter().map(|o| o.name.as_str()))?;
    }
    for c in &p.classes {
        unique_names("クラス", &c.sym, c.methods.iter().map(|m| m.name.as_str()))?;
    }
    let mut ctx = Ctx {
        p,
        lim: max_nest,
        origins: Vec::new(),
    };
    ctx.check_instances()?;
    ctx.check_units()?;
    ctx.check_builtins()?;
    ctx.origins()?;
    let mut resumes = Vec::with_capacity(p.functions.len());
    for (i, f) in p.functions.iter().enumerate() {
        resumes.push(ctx.check_fn(FnId(i as u32), f)?);
    }
    ctx.check_classes(&resumes)
}
