//! 初期化の検査の省略(パス 9。D506、IRSpec 10.3 節)。
//!
//! 大域 `g` を読む箇所の検査を省いてよいのは、次のどちらかのときだけである。
//!
//! - その箇所を含む最も外側のトップレベルの実体の手順が、`g` の手順より後にある。
//! - 両者が同じ `def_fns` の手順にある。
//!
//! 「最も外側のトップレベルの実体」は、関数を作った場所の鎖(`closure`、`dictabs`、`handle` の部分)を
//! たどって着く根の関数で決める。
//! 根の関数は、大域、初期化の手順、インスタンスの項目が指す関数である。
//! 手順の番号は、単位を初期化の順に並べ、各単位の手順を書いた順に数えた通し番号である。
//! 根が分からない関数(作る場所が 2 つ以上あるものや、どこからも作られないもの)では、検査を省かない。
//!
//! 種類 `builtin` の大域は、プログラムの開始の時点で初期化済みなので、検査を要らない。

use kir::{Exp, ExpId, FnId, GlobalId, GlobalKind, Program, Step};

/// 解析の結果。
#[derive(Default)]
pub(crate) struct Info {
    /// 関数ごとの、最も外側のトップレベルの実体の手順の番号。
    pub fn_step: Vec<Option<usize>>,
    /// 大域ごとの、それを定義する手順の番号。
    pub global_step: Vec<Option<usize>>,
    /// 手順ごとの、`def_fns` かどうか。
    pub is_def_fns: Vec<bool>,
}

impl Info {
    /// 関数 `f` の中で大域 `g` を読むとき、初期化の検査を省いてよいか。
    pub(crate) fn may_elide(&self, p: &Program, f: Option<FnId>, g: GlobalId) -> bool {
        if matches!(p.globals[g.index()].kind, GlobalKind::Builtin) {
            return true;
        }
        let Some(f) = f else { return false };
        match (self.fn_step[f.index()], self.global_step[g.index()]) {
            (Some(s), Some(t)) => s > t || (s == t && self.is_def_fns[s]),
            _ => false,
        }
    }
}

/// 関数の本体(と guard)のすべての式のノードを `on` に渡す(順は決めない)。
pub(crate) fn visit(p: &Program, f: &kir::Function, mut on: impl FnMut(&Exp)) {
    let mut stack: Vec<ExpId> = vec![f.body];
    if let Some(g) = f.guard {
        stack.push(g);
    }
    while let Some(e) = stack.pop() {
        let Some(x) = p.nodes.exp(e) else { continue };
        on(x);
        match x {
            Exp::Lit(_)
            | Exp::LitError(_)
            | Exp::Local(_)
            | Exp::Env(_)
            | Exp::Global(_)
            | Exp::RecEmpty
            | Exp::Hole
            | Exp::Method { .. }
            | Exp::Closure(_)
            | Exp::Dictabs(_)
            | Exp::Handle(_) => {}
            Exp::Construct { fields, .. } => stack.extend(fields.iter().map(|(_, e)| *e)),
            Exp::Variant { value, .. } => stack.push(*value),
            Exp::Call { func, args, .. } => {
                stack.push(*func);
                stack.extend(args.iter().copied());
            }
            Exp::Opcall { args, .. } | Exp::Perform { args, .. } => {
                stack.extend(args.iter().copied())
            }
            Exp::Not(a) => stack.push(*a),
            Exp::And(a, b) | Exp::Or(a, b) => {
                stack.push(*a);
                stack.push(*b);
            }
            Exp::Dictapp { func, .. } => stack.push(*func),
            Exp::Let { value, body, .. } | Exp::Letpat { value, body, .. } => {
                stack.push(*value);
                stack.push(*body);
            }
            Exp::Letrec { items, body } => {
                stack.extend(items.iter().map(|(_, e)| *e));
                stack.push(*body);
            }
            Exp::Seq(es) => stack.extend(es.iter().copied()),
            Exp::Match { scrutinee, clauses } => {
                stack.push(*scrutinee);
                for c in clauses {
                    if let Some(g) = c.guard {
                        stack.push(g);
                    }
                    stack.push(c.body);
                }
            }
            Exp::RecExtend { value, record, .. } | Exp::RecUpdate { value, record, .. } => {
                stack.push(*value);
                stack.push(*record);
            }
            Exp::RecSelect { record, .. } | Exp::RecRestrict { record, .. } => stack.push(*record),
            Exp::Resume { value, .. } => stack.push(*value),
        }
    }
}

/// 関数の本体が作る関数(`closure`、`dictabs`、`handle` の各部分)を集める。
fn created(p: &Program, f: &kir::Function, out: &mut Vec<FnId>) {
    visit(p, f, |x| match x {
        Exp::Closure(part) | Exp::Dictabs(part) => out.push(part.func),
        Exp::Handle(h) => {
            out.push(h.body.func);
            out.extend(h.ops.iter().map(|o| o.part.func));
            out.extend(h.ret.iter().map(|q| q.func));
            out.extend(h.cancel.iter().map(|q| q.func));
        }
        _ => {}
    });
}

/// プログラムを解析する(単位を足したプログラムでは、足した部分について解析を進める)。
///
/// `info` は、関数 `f0` 個、単位 `u0` 個までのプログラムの解析の結果である(どちらも 0 なら空)。
/// 足した単位の手順は前の手順の後ろに番号を続け、足した関数の根は足した関数の中でたどる
/// (前の関数は後ろの関数を作らないので、前の結果は変わらない)。
pub(crate) fn extend(p: &Program, info: &mut Info, f0: usize, u0: usize) {
    let nf = p.functions.len();
    let ng = p.globals.len();
    let n = nf - f0;
    let global_step = &mut info.global_step;
    global_step.resize(ng, None);
    let is_def_fns = &mut info.is_def_fns;
    let mut root_step: Vec<Option<usize>> = vec![None; n];
    let mut step = is_def_fns.len();
    let set_root = |root_step: &mut Vec<Option<usize>>, f: FnId, s: usize| {
        if let Some(slot) = f.index().checked_sub(f0).and_then(|i| root_step.get_mut(i)) {
            *slot = Some(s);
        }
    };
    for u in &p.units[u0..] {
        for st in &u.init {
            match st {
                Step::DefValue { global, func } => {
                    global_step[global.index()] = Some(step);
                    set_root(&mut root_step, *func, step);
                }
                Step::DefPattern { func, globals, .. } => {
                    for g in globals {
                        global_step[g.index()] = Some(step);
                    }
                    set_root(&mut root_step, *func, step);
                }
                Step::DefFns(gs) => {
                    for g in gs {
                        global_step[g.index()] = Some(step);
                        if let GlobalKind::Fn(f) | GlobalKind::Dictabs(f) =
                            p.globals[g.index()].kind
                        {
                            set_root(&mut root_step, f, step);
                        }
                    }
                }
                Step::DefDictabs(g) => {
                    global_step[g.index()] = Some(step);
                    if let GlobalKind::Dictabs(f) = p.globals[g.index()].kind {
                        set_root(&mut root_step, f, step);
                    }
                }
                Step::DefInstance(i) => {
                    for m in &p.instances[i.index()].methods {
                        set_root(&mut root_step, m.func, step);
                    }
                }
                Step::RunExp(f) => set_root(&mut root_step, *f, step),
            }
            is_def_fns.push(matches!(st, Step::DefFns(_)));
            step += 1;
        }
    }
    // 作る場所の親。2 つ以上の場所から作られる関数は、親を決めない。
    // 添字は足した関数の中の位置である。
    const MANY: u32 = u32::MAX - 1;
    const NONE: u32 = u32::MAX;
    let mut parent = vec![NONE; n];
    let mut buf = Vec::new();
    for (i, f) in p.functions.iter().enumerate().skip(f0) {
        buf.clear();
        created(p, f, &mut buf);
        for c in &buf {
            if let Some(slot) = c.index().checked_sub(f0).and_then(|j| parent.get_mut(j)) {
                *slot = if *slot == NONE { (i - f0) as u32 } else { MANY };
            }
        }
    }
    // 親の鎖をたどって根の手順を決める。たどった結果は覚えて、各関数を 1 度だけたどる。
    let mut fn_step: Vec<Option<usize>> = vec![None; n];
    let mut done = vec![false; n];
    let mut path = Vec::new();
    for start in 0..n {
        let mut cur = start;
        let mut result: Option<usize> = None;
        path.clear();
        loop {
            if done[cur] {
                result = fn_step[cur];
                break;
            }
            if path.len() > n {
                break;
            }
            path.push(cur);
            if root_step[cur].is_some() && parent[cur] == NONE {
                result = root_step[cur];
                break;
            }
            match parent[cur] {
                NONE | MANY => break,
                q => {
                    if root_step[cur].is_some() {
                        // 根でもあり作られもする関数は、どちらの手順で走るか決まらない。
                        break;
                    }
                    cur = q as usize;
                }
            }
        }
        for &x in &path {
            fn_step[x] = result;
            done[x] = true;
        }
    }
    info.fn_step.truncate(f0);
    info.fn_step.resize(f0, None);
    info.fn_step.extend(fn_step);
}
