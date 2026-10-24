//! 生存区間、再開点の番号、スロットの割り当て(パス 12)。
//!
//! - レジスタの番号を詰める(どこにも現れないレジスタを除く)。
//! - 命令に通し番号を振り(引数は 0、ブロックを位相の順に、命令とブロックの終わりを 1 から)、
//!   各レジスタの区間を「最初に書く位置から最後に読む位置まで」とする。
//!   位相の順に並べたので、ある経路でレジスタが生きている位置は必ずこの区間に入る。
//!   区間が重ならないレジスタは、同時に生きることがない。
//! - スロットは `any` の区画(`any` と `str`。GC が走査する)と生の区画(`bool`、`i32`、`i64`、`f64`)に分け、
//!   区画ごとに区間の線形走査で割り当てる。
//!   引数は `any` の区画の 0 番から順に置く(呼ぶ側が書く位置である)。
//! - 末尾でない呼び出しを再開点とし、関数の中で位置の順に 0 から番号を振る。
//!   確保する命令を安全点とする。
//!   再開点には呼び出しの後で生きているレジスタ(呼び出しが書くものを除く)を、安全点には確保をまたいで
//!   生きているレジスタ(確保の後で生きているものから書くものを除き、被演算子を足したもの)を、
//!   正確な生存解析で求めて付ける。
//!   解析は書き換えの日誌を持つ 1 つの集合([`crate::bits::Timeline`])で進め、
//!   手間は、近くで使う入口の集合では変わったレジスタの数に、遠くで使うものでは集合の幅に比例する。
//!   点の集合は差分で持つ([`crate::live`])。

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use crate::bits::{Bits, Timeline};

use crate::ir::*;

/// Rep がどちらの区画に入るか(`true` なら `any` の区画)。
pub fn is_any_class(rep: Rep) -> bool {
    matches!(rep, Rep::Any | Rep::Str)
}

/// 命令の中のレジスタを書き換える。
fn map_inst_regs(i: &mut Inst, f: &impl Fn(Reg) -> Reg) {
    use Inst::*;
    let m = |r: &mut Reg| *r = f(*r);
    let mv = |v: &mut Vec<Reg>| v.iter_mut().for_each(|r| *r = f(*r));
    match i {
        Const { dst, .. } | EnvGet { dst, .. } => m(dst),
        DepthEnter | DepthCheck | DepthPush | DepthRestore => {}
        Move { dst, src } | Box { dst, src } | Unbox { dst, src } => {
            m(dst);
            m(src);
        }
        IntNeg { dst, a, .. }
        | FloatNeg { dst, a }
        | FloatMath { dst, a, .. }
        | FloatTrunc { dst, a }
        | FloatIsNan { dst, a }
        | Conv { dst, a, .. }
        | BoolNot { dst, a }
        | StrLen { dst, a }
        | Show { dst, a, .. }
        | ArrayLen { dst, a }
        | MArrayLen { dst, a }
        | MArrayFreeze { dst, a } => {
            m(dst);
            m(a);
        }
        IntArith { dst, a, b, .. }
        | IntCmp { dst, a, b, .. }
        | FloatArith { dst, a, b, .. }
        | FloatCmp { dst, a, b, .. }
        | BoolEq { dst, a, b }
        | StrConcat { dst, a, b }
        | StrCmp { dst, a, b, .. } => {
            m(dst);
            m(a);
            m(b);
        }
        StrSub { dst, s, pos, len } => {
            m(dst);
            m(s);
            m(pos);
            m(len);
        }
        CtorNew { dst, fields, .. } | RecNew { dst, fields, .. } => {
            m(dst);
            mv(fields);
        }
        CtorTag { dst, v } | CtorField { dst, v, .. } | VariantTag { dst, v } => {
            m(dst);
            m(v);
        }
        VariantPayload { dst, v } => {
            m(dst);
            m(v);
        }
        VariantNew { dst, payload, .. } => {
            m(dst);
            m(payload);
        }
        RecGet { dst, rec, .. }
        | RecField { dst, rec, .. }
        | RecHas { dst, rec, .. }
        | RecRestrict { dst, rec, .. } => {
            m(dst);
            m(rec);
        }
        RecExtend {
            dst, rec, value, ..
        }
        | RecUpdate {
            dst, rec, value, ..
        } => {
            m(dst);
            m(rec);
            m(value);
        }
        ClosureNew { dst, caps, .. } => {
            m(dst);
            mv(caps);
        }
        ClosureSetCap { clo, value, .. } => {
            m(clo);
            m(value);
        }
        MethodNew { dst, dict, own, .. } => {
            m(dst);
            m(dict);
            mv(own);
        }
        DictInst { dst, premises, .. } => {
            m(dst);
            mv(premises);
        }
        DictRecord { dst, fields } | DictVariant { dst, fields } => {
            m(dst);
            fields.iter_mut().for_each(|(_, r)| *r = f(*r));
        }
        GlobalGet { dst, .. }
        | GlobalIsInit { dst, .. }
        | InstIsInit { dst, .. }
        | UnimplementedValue { dst, .. } => m(dst),
        InstGetValue { dst, .. } => m(dst),
        GlobalSet { src, .. } | InstSetValue { src, .. } => m(src),
        GlobalMarkInit { .. } | InstMarkInit { .. } => {}
        RefNew { dst, value } => {
            m(dst);
            m(value);
        }
        RefGet { dst, r } => {
            m(dst);
            m(r);
        }
        RefSet { r, value } => {
            m(r);
            m(value);
        }
        ArrayGet { dst, a, index } | MArrayGet { dst, a, index } => {
            m(dst);
            m(a);
            m(index);
        }
        MArrayNew { dst, len, init } => {
            m(dst);
            m(len);
            m(init);
        }
        MArraySet { a, index, value } => {
            m(a);
            m(index);
            m(value);
        }
        CallDirect { dst, args, .. } => {
            m(dst);
            mv(args);
        }
        CallValue {
            dst, callee, args, ..
        } => {
            m(dst);
            m(callee);
            mv(args);
        }
        CallMethod {
            dst,
            dict,
            own,
            args,
            ..
        } => {
            m(dst);
            m(dict);
            mv(own);
            mv(args);
        }
        DictApp { dst, f: g, dicts } => {
            m(dst);
            m(g);
            mv(dicts);
        }
        ConsoleWrite { dst, s } => {
            m(dst);
            m(s);
        }
    }
}

/// ブロックの終わりの中のレジスタを書き換える。
fn map_term_regs(t: &mut Term, f: &impl Fn(Reg) -> Reg) {
    let mv = |v: &mut Vec<Reg>| v.iter_mut().for_each(|r| *r = f(*r));
    match t {
        Term::Jump(_) | Term::Unimplemented { .. } => {}
        Term::Branch { cond, .. } => *cond = f(*cond),
        Term::Return(r) => *r = f(*r),
        Term::TailCallDirect { args, .. } => mv(args),
        Term::TailCallValue { callee, args, .. } => {
            *callee = f(*callee);
            mv(args);
        }
        Term::TailCallMethod {
            dict, own, args, ..
        } => {
            *dict = f(*dict);
            mv(own);
            mv(args);
        }
        Term::Error { operand, .. } => {
            if let Some(r) = operand {
                *r = f(*r);
            }
        }
    }
}

/// どこにも現れないレジスタを除き、番号を詰める。
pub(crate) fn compact(func: &mut Func) {
    let n = func.regs.len();
    let mut seen = vec![false; n];
    for &p in &func.params {
        seen[p.index()] = true;
    }
    for b in &func.blocks {
        for i in &b.insts {
            if let Some(d) = i.def() {
                seen[d.index()] = true;
            }
            for u in i.uses() {
                seen[u.index()] = true;
            }
        }
        for u in b.term.uses() {
            seen[u.index()] = true;
        }
    }
    if seen.iter().all(|&s| s) {
        return;
    }
    let mut new_of = vec![u32::MAX; n];
    let mut regs = Vec::new();
    for (i, &s) in seen.iter().enumerate() {
        if s {
            new_of[i] = regs.len() as u32;
            regs.push(func.regs[i]);
        }
    }
    let f = |r: Reg| Reg(new_of[r.index()]);
    for p in &mut func.params {
        *p = f(*p);
    }
    for b in &mut func.blocks {
        for i in &mut b.insts {
            map_inst_regs(i, &f);
        }
        map_term_regs(&mut b.term, &f);
    }
    func.regs = regs;
}

/// 各レジスタの区間(最初に書く位置、最後に読む位置)。どこにも現れなければ `None`。
pub(crate) fn intervals(func: &Func) -> Vec<Option<(u32, u32)>> {
    let mut iv: Vec<Option<(u32, u32)>> = vec![None; func.regs.len()];
    let touch = |iv: &mut Vec<Option<(u32, u32)>>, r: Reg, pos: u32| {
        let e = &mut iv[r.index()];
        *e = Some(match *e {
            None => (pos, pos),
            Some((s, t)) => (s.min(pos), t.max(pos)),
        });
    };
    for &p in &func.params {
        touch(&mut iv, p, 0);
    }
    let mut pos = 1u32;
    for b in &func.blocks {
        for i in &b.insts {
            for u in i.uses() {
                touch(&mut iv, u, pos);
            }
            if let Some(d) = i.def() {
                touch(&mut iv, d, pos);
            }
            pos += 1;
        }
        for u in b.term.uses() {
            touch(&mut iv, u, pos);
        }
        pos += 1;
    }
    iv
}

/// 再開点か安全点の列を作る途中の状態。
///
/// 最後に足した点の集合 `rec` と、そのときの時点(世代と日誌の長さ)を持つ。
/// 次の点では、それ以後に日誌に積んだレジスタと、前後の点の命令が書くレジスタと被演算子だけを
/// 比べて差分を作る。
/// その間に跳びがあれば、`rec` と今の集合をビット列で比べる。
/// 今の集合を使わずに処理したブロックの点([`Table::record_local`])では、`rec` と点の集合を直に比べ、
/// `rec` で変えたレジスタを `dirty` に積んで、次の点で比べる候補に足す。
struct Table {
    rec: Bits,
    /// `rec` の要素(順不同)と、各レジスタの `rec_list` の中の位置(無ければ `u32::MAX`)。
    rec_list: Vec<u32>,
    rec_pos: Vec<u32>,
    since: (u32, usize),
    /// 前の点で、生存の集合から外したか足したレジスタ(書くレジスタと被演算子)。
    dev: Vec<u32>,
    /// 今の集合を使わない点で、`rec` に足したか除いたレジスタ。
    dirty: Vec<u32>,
    out: crate::live::Builder,
    /// 手間(比べたレジスタと、写しに入れたレジスタの数)。
    work: u64,
}

impl Table {
    fn new(nr: usize) -> Table {
        Table {
            rec: Bits::new(nr),
            rec_list: Vec::new(),
            rec_pos: vec![u32::MAX; nr],
            since: (0, 0),
            dev: Vec::new(),
            dirty: Vec::new(),
            out: crate::live::Builder::default(),
            work: 0,
        }
    }

    fn rec_insert(&mut self, r: u32) {
        self.rec.insert(r as usize);
        self.rec_pos[r as usize] = self.rec_list.len() as u32;
        self.rec_list.push(r);
    }

    fn rec_remove(&mut self, r: u32) {
        self.rec.remove(r as usize);
        let i = self.rec_pos[r as usize] as usize;
        self.rec_pos[r as usize] = u32::MAX;
        self.rec_list.swap_remove(i);
        if let Some(&m) = self.rec_list.get(i) {
            self.rec_pos[m as usize] = i as u32;
        }
    }

    /// 点を足す。点の集合は、`tl` の今の集合から `def` を除き、`extra` を足したものである。
    fn record(&mut self, pt: Point, tl: &Timeline, def: Option<Reg>, extra: &[Reg]) {
        let mut ex: Vec<u32> = extra.iter().map(|r| r.0).collect();
        ex.sort_unstable();
        let in_c = |r: u32| (tl.contains(r) && def != Some(Reg(r))) || ex.binary_search(&r).is_ok();
        let (g, t) = tl.now();
        let mut cand: Vec<u32> = if self.since.0 == g {
            tl.changed_since(self.since.1).collect()
        } else {
            self.work += self.rec.words() as u64;
            self.rec.xor_ones(tl.bits())
        };
        cand.append(&mut self.dev);
        cand.append(&mut self.dirty);
        cand.extend(def.map(|d| d.0));
        cand.extend_from_slice(&ex);
        cand.sort_unstable();
        cand.dedup();
        self.work += cand.len() as u64;
        let mut add = Vec::new();
        let mut rem = Vec::new();
        for r in cand {
            match (in_c(r), self.rec.contains(r as usize)) {
                (true, false) => add.push(Reg(r)),
                (false, true) => rem.push(Reg(r)),
                _ => {}
            }
        }
        self.since = (g, t);
        self.dev.extend(def.map(|d| d.0));
        self.dev.extend_from_slice(&ex);
        for r in &add {
            self.rec_insert(r.0);
        }
        for r in &rem {
            self.rec_remove(r.0);
        }
        self.push(pt, &add, &rem);
    }

    /// 点を足す。点の集合は、`live`(昇順)から `def` を除き、`extra` を足したものである。
    fn record_local(&mut self, pt: Point, live: &[u32], def: Option<Reg>, extra: &[Reg]) {
        let mut c: Vec<u32> = live
            .iter()
            .copied()
            .filter(|&r| def != Some(Reg(r)))
            .collect();
        c.extend(extra.iter().map(|r| r.0));
        c.sort_unstable();
        c.dedup();
        let add: Vec<Reg> = c
            .iter()
            .filter(|&&r| !self.rec.contains(r as usize))
            .map(|&r| Reg(r))
            .collect();
        let mut rem: Vec<Reg> = self
            .rec_list
            .iter()
            .filter(|&&r| c.binary_search(&r).is_err())
            .map(|&r| Reg(r))
            .collect();
        rem.sort_unstable();
        self.work += (c.len() + self.rec_list.len()) as u64;
        for r in &add {
            self.rec_insert(r.0);
            self.dirty.push(r.0);
        }
        for r in &rem {
            self.rec_remove(r.0);
            self.dirty.push(r.0);
        }
        self.push(pt, &add, &rem);
    }

    fn push(&mut self, pt: Point, add: &[Reg], rem: &[Reg]) {
        let list = &self.rec_list;
        let work = &mut self.work;
        self.out.push(pt, add, rem, list.len(), || {
            *work += list.len() as u64;
            let mut v: Vec<Reg> = list.iter().map(|&r| Reg(r)).collect();
            v.sort_unstable();
            v
        });
    }
}

/// 今の集合を使わずに処理するブロックの大きさ(命令と被演算子の数)の上限。
const SMALL: usize = 64;

/// 命令と被演算子の数が [`SMALL`] 以下か。
fn is_small(b: &Block) -> bool {
    let mut n = b.term.uses().len();
    for i in &b.insts {
        n += 1 + i.uses().len();
        if n > SMALL {
            return false;
        }
    }
    n <= SMALL
}

/// 移る先の無い小さなブロックを、今の集合を使わずに処理する。点を足し、入口で生きているレジスタ(昇順)を返す。
fn local_block(func: &Func, bi: usize, tabs: &mut [Table; 2]) -> Vec<u32> {
    let b = &func.blocks[bi];
    let mut live: Vec<u32> = b.term.uses().iter().map(|r| r.0).collect();
    live.sort_unstable();
    live.dedup();
    for (ii, i) in b.insts.iter().enumerate().rev() {
        let call = i.is_call();
        let uses = i.uses();
        if call || i.allocates(&func.regs) {
            let pt = Point {
                block: BlockId(bi as u32),
                index: ii as u32,
            };
            if call {
                tabs[0].record_local(pt, &live, i.def(), &[]);
            } else {
                tabs[1].record_local(pt, &live, i.def(), &uses);
            }
        }
        if let Some(d) = i.def() {
            live.retain(|&r| r != d.0);
        }
        live.extend(uses.iter().map(|r| r.0));
        live.sort_unstable();
        live.dedup();
    }
    live
}

/// 正確な生存解析で、再開点と安全点の位置と、そこで生きているレジスタを求める。
///
/// ブロックを位相の逆の順にたどり、1 つの集合を書き換えていく([`Timeline`])。
/// ブロックの出口の集合は、移る先の入口の集合の和である。
/// 今の集合が直前に処理したブロックの入口の集合で、そのブロックが移る先にあれば、今の集合をそのまま使い、
/// 残りの先の集合を足す。
/// なければ、最初の先の入口の集合に戻してから残りを足す。
/// 移る先の無い小さなブロック(返すか誤りで終わるブロック)は、今の集合を使わずに要素の列で処理する
/// ([`local_block`])。
/// 移る先の無い大きなブロックでは、今の集合を空にする。
/// 生きている値の多い経路の途中に誤りで終わるブロックが並んでも、今の集合を空にしては戻す手間がかからない。
/// 入口の集合は、前任をすべて処理したら捨てる。
/// 再開点の集合は、呼び出しの後で生きているレジスタ(呼び出しが書くものを除く)である。
/// 安全点の集合は、確保をまたいで生きているレジスタで、確保の後で生きているもの(書くものを除く)に
/// 命令の被演算子を足したものである。
/// 確保が GC を起こしても、新しい値に写す前の被演算子が根に入るようにするためである。
pub(crate) fn points(func: &Func) -> (Points, Points) {
    let (r, s, _) = points_counted(func);
    (r, s)
}

/// [`points`] を行い、その手間([`Timeline`] と [`Table`] の読み書きの量)も返す。
pub(crate) fn points_counted(func: &Func) -> (Points, Points, u64) {
    let nb = func.blocks.len();
    let nr = func.regs.len();
    let succs: Vec<Vec<usize>> = func
        .blocks
        .iter()
        .map(|b| {
            let mut v: Vec<usize> = b.term.succs().iter().map(|s| s.index()).collect();
            v.dedup();
            v
        })
        .collect();
    let mut npred = vec![0u32; nb];
    for v in &succs {
        for &s in v {
            npred[s] += 1;
        }
    }
    let mut tl = Timeline::new(nr, nb);
    // 今の集合を入口の集合とするブロック。
    let mut prev: Option<usize> = None;
    let mut tabs = [Table::new(nr), Table::new(nr)];
    for bi in (0..nb).rev() {
        let b = &func.blocks[bi];
        let ss = &succs[bi];
        if ss.is_empty() && is_small(b) {
            let v = local_block(func, bi, &mut tabs);
            if npred[bi] > 0 {
                tl.save_list(bi, v);
            }
            continue;
        }
        let base = prev.filter(|p| ss.contains(p));
        match (base, ss.first()) {
            (Some(_), _) => {}
            (None, Some(&s)) => tl.restore(s),
            (None, None) => tl.clear(),
        }
        for (k, &s) in ss.iter().enumerate() {
            let skip = match base {
                Some(p) => s == p,
                None => k == 0,
            };
            if !skip {
                tl.union(s);
            }
        }
        for &s in ss {
            npred[s] -= 1;
            if npred[s] == 0 {
                tl.free(s);
            }
        }
        for u in b.term.uses() {
            tl.set(u.0, true);
        }
        for (ii, i) in b.insts.iter().enumerate().rev() {
            let call = i.is_call();
            let uses = i.uses();
            if call || i.allocates(&func.regs) {
                let pt = Point {
                    block: BlockId(bi as u32),
                    index: ii as u32,
                };
                if call {
                    tabs[0].record(pt, &tl, i.def(), &[]);
                } else {
                    tabs[1].record(pt, &tl, i.def(), &uses);
                }
            }
            if let Some(d) = i.def() {
                tl.set(d.0, false);
            }
            for u in uses {
                tl.set(u.0, true);
            }
        }
        if npred[bi] > 0 {
            tl.save(bi);
        }
        prev = Some(bi);
        tl.tick();
    }
    let [r, s] = tabs;
    let work = tl.work() + r.work + s.work;
    (r.out.finish(), s.out.finish(), work)
}

/// パス 12 を関数に行う。
pub(crate) fn assign(func: &mut Func) {
    compact(func);
    let iv = intervals(func);
    let n = func.regs.len();
    let mut slot = vec![Slot::Any(0); n];
    let mut is_param = vec![false; n];
    for (i, &p) in func.params.iter().enumerate() {
        slot[p.index()] = Slot::Any(i as u32);
        is_param[p.index()] = true;
    }
    let nparams = func.params.len() as u32;
    let mut counts = [nparams, 0u32];
    for (class, any) in [(0usize, true), (1usize, false)] {
        let mut order: Vec<usize> = (0..n)
            .filter(|&r| !is_param[r] && iv[r].is_some() && is_any_class(func.regs[r]) == any)
            .collect();
        order.sort_by_key(|&r| (iv[r].unwrap().0, r));
        // 使っているスロットと、その区間の終わり。
        let mut active: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        let mut free: BTreeSet<u32> = BTreeSet::new();
        let base = if any { nparams } else { 0 };
        let mut next = base;
        for r in order {
            let (s, e) = iv[r].unwrap();
            while let Some(&Reverse((end, sl))) = active.peek() {
                if end >= s {
                    break;
                }
                active.pop();
                free.insert(sl);
            }
            let sl = match free.pop_first() {
                Some(x) => x,
                None => {
                    next += 1;
                    next - 1
                }
            };
            active.push(Reverse((e, sl)));
            slot[r] = if any { Slot::Any(sl) } else { Slot::Raw(sl) };
        }
        counts[class] = next;
    }
    let (resume_points, safepoints) = points(func);
    func.frame = Some(Frame {
        slot,
        any_slots: counts[0],
        raw_slots: counts[1],
        resume_points,
        safepoints,
    });
}
