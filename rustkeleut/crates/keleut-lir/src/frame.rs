//! 生存区間、再開点の番号、スロットの割り当て(パス 12)。
//!
//! - レジスタの番号を詰める。
//!   どこにも現れないレジスタは取り除く。
//! - 命令に通し番号を振る。
//!   引数を書く位置を 0 とし、ブロックをトポロジカル順にたどって、命令と終端命令に 1 から続けて番号を振る。
//!   番号はブロックごとに振り直さず、関数の中で通して数える。
//!   各レジスタの生存区間は、そのレジスタを読むか書く位置のうち、最初のものから最後のものまでとする。
//!   ブロックはトポロジカル順に並んでいるので、ある経路でレジスタが生存している位置は必ずこの生存区間に入る。
//!   したがって、生存区間の重ならない 2 つのレジスタが同時に生存することはない。
//! - スロットは、`any` の領域(`any` と `str`。GC が走査する)と、ボックス化していない値の領域(`bool`、`i32`、`i64`、`f64`)に分け、
//!   領域ごとに、生存区間の線形走査で割り当てる。
//!   引数は `any` の領域の 0 番から順に置く(呼び出し元が値を書く位置である)。
//! - 非末尾呼び出しを再開点とし、関数の中で位置の順に 0 から番号を振る。
//!   メモリを確保する命令を安全点とする。
//!   再開点には、呼び出しの後で生存しているレジスタの集合を付ける。
//!   呼び出しが書くレジスタは、この集合から除く。
//!   安全点には、確保をまたいで生存しているレジスタの集合を付ける。
//!   この集合は、確保の後で生存しているレジスタから命令が書くレジスタを除き、オペランドを加えたものである。
//!   どちらの集合も、正確な生存解析で求める。
//!   解析は、変更ログを持つ 1 つの集合([`crate::bits::Timeline`])を書き換えながら進める。
//!   処理量は、近くで使う入口の集合では変わったレジスタの個数に、遠くで使う入口の集合では集合の幅に比例する。
//!   再開点と安全点(以下、点)の集合は、位置の順で次の点の集合からの差分で保持する([`crate::live`])。

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use crate::bits::{Bits, Timeline};

use crate::ir::*;

/// Rep が `rep` のレジスタを `any` の領域に置くなら真を、ボックス化していない値の領域に置くなら偽を返す。
pub fn is_any_class(rep: Rep) -> bool {
    matches!(rep, Rep::Any | Rep::Str)
}

/// 命令の中の各レジスタを、`f` を適用した結果に書き換える。
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
        Fx { dst, args, .. } => {
            if let Some(d) = dst {
                m(d);
            }
            mv(args);
        }
    }
}

/// 終端命令の中の各レジスタを、`f` を適用した結果に書き換える。
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
        Term::Fx { args, .. } => mv(args),
    }
}

/// どこにも現れないレジスタを取り除き、残ったレジスタの番号を詰める。
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

/// 各レジスタの生存区間を、開始位置と終了位置の組で返す。
/// どこにも現れないレジスタは `None` になる。
///
/// 開始位置と終了位置は、そのレジスタを読むか書く位置のうち、最初のものと最後のものである。
/// 最後に読んだ後に書く命令があれば、生存区間はその命令の位置まで延びる。
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

/// 再開点か安全点の並びを作る途中の状態。
///
/// 最後に追加した点の集合 `rec` と、そのときの時点(世代と変更ログの長さ)を持つ。
/// 次の点では、その時点より後に変更ログに追記されたレジスタと、前後の点の命令が書くレジスタとオペランドだけを比較して、差分を作る。
/// その間に一括書き換えがあれば、`rec` と現在の集合をビット列で比較する。
/// 現在の集合を使わずに処理したブロックの点([`Table::record_local`])では、`rec` と点の集合を直接比較する。
/// このとき `rec` で変えたレジスタを `dirty` に追加し、次の点で比較する候補に加える。
struct Table {
    /// 最後に追加した点の集合。
    rec: Bits,
    /// `rec` の要素(順不同)。
    rec_list: Vec<u32>,
    /// 各レジスタの `rec_list` の中の位置。`rec` に入っていなければ `u32::MAX` である。
    rec_pos: Vec<u32>,
    /// 最後に点を追加したときの時点(世代と変更ログの長さ)。
    since: (u32, usize),
    /// 前の点の集合を作るときに、現在の集合から除いたか、現在の集合に加えたレジスタ(命令が書くレジスタとオペランド)。
    dev: Vec<u32>,
    /// 現在の集合を使わない点で、`rec` に加えたか `rec` から除いたレジスタ。
    dirty: Vec<u32>,
    /// 作っている途中の点の並び。
    out: crate::live::Builder,
    /// 処理量のカウンタ。
    /// 比較したレジスタの個数と、集合全体をコピーしたときにコピーしたレジスタの個数を足していく。
    /// `rec` と現在の集合をビット列で比較したときは、ビット列のワード数も足す。
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

    /// 点を追加する。
    /// 点の集合は、`tl` の現在の集合から `def` を除き、`extra` を加えたものである。
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

    /// 現在の集合を使わずに点を追加する。
    /// 点の集合は、`live`(昇順)から `def` を除き、`extra` を加えたものである。
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

/// 現在の集合を使わずに処理するブロックのサイズの上限。
/// サイズは、命令の個数と、命令と終端命令のオペランドの個数の和である。
const SMALL: usize = 64;

/// ブロックのサイズが [`SMALL`] 以下なら真を返す。
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

/// 後続ブロックを持たない小さなブロックを、現在の集合を使わずに処理する。
/// ブロックの中の点を `tabs` に追加し、入口で生存しているレジスタを昇順に並べて返す。
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

/// 正確な生存解析で、再開点と安全点の位置と、各点で生存しているレジスタを求める。
///
/// ブロックを逆トポロジカル順にたどり、1 つの集合(現在の集合)を書き換えていく([`Timeline`])。
/// ブロックの出口の集合は、後続ブロックの入口の集合の和集合である。
/// 現在の集合が直前に処理したブロックの入口の集合で、そのブロックが後続ブロックの 1 つなら、現在の集合をそのまま使い、残りの後続ブロックの入口の集合を加える。
/// そうでなければ、現在の集合を最初の後続ブロックの入口の集合にしてから、残りを加える。
/// 後続ブロックを持たない小さなブロック(`return`、`error`、末尾呼び出しなどで終わるブロック)は、現在の集合を使わず、要素の配列で処理する([`local_block`])。
/// 後続ブロックを持たない大きなブロックでは、現在の集合を空にする。
/// 小さなブロックを別に処理するので、生存している値の多い経路の途中にエラーで終わるブロックが並んでいても、現在の集合を空にして元に戻す処理を繰り返さずに済む。
/// 入口の集合は、先行ブロックをすべて処理したら捨てる。
///
/// 再開点の集合は、呼び出しの後で生存しているレジスタから、呼び出しが書くレジスタを除いたものである。
/// 安全点の集合は、確保をまたいで生存しているレジスタである。
/// 確保の後で生存しているレジスタから命令が書くレジスタを除き、命令のオペランドを加えて求める。
/// オペランドを加えるのは、確保が GC を起こしても、新しい値にコピーする前のオペランドがルートに残るようにするためである。
pub(crate) fn points(func: &Func) -> (Points, Points) {
    let (r, s, _) = points_counted(func);
    (r, s)
}

/// [`points`] と同じ解析を行い、その処理量([`Timeline`] と [`Table`] のカウンタの合計)も返す。
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
    // 現在の集合が入口の集合になっているブロック。
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

/// パス 11(yield の枝刈り)を行う。
/// 再開点ごとに、yield を受け取ることがあれば真を返す。
///
/// `prune` が真なら、呼び出しの命令が持つ `yield` の印(KIR の `may_yield`)をそのまま使う。
/// `dict_app` と、エフェクトの命令のうち非末尾呼び出しの一種であるものは印を持たないので、「受け取る」と判定する。
/// `prune` が偽なら(`--no-yield-prune`)、すべての再開点を「受け取る」として扱う。
/// インタプリタは、どの再開点からでもフレームの実行を続けられるので、この判定の結果によって実行するコードを変えない。
/// 判定の結果は、「受け取らない」と判定した再開点に yield が届いたことを検出するために使う(`doc/lir.md` の 17 節)。
pub(crate) fn yield_points(func: &Func, points: &Points, prune: bool) -> Vec<bool> {
    points
        .points()
        .iter()
        .map(|p| !prune || func.blocks[p.block.index()].insts[p.index as usize].call_yields())
        .collect()
}

/// 関数 `func` にパス 11 とパス 12 を行い、結果をフレームの情報(`func.frame`)として付ける。
pub(crate) fn assign(func: &mut Func, prune: bool) {
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
        // 使用中のスロットと、そのスロットを使うレジスタの生存区間の終わり。
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
    let yield_points = yield_points(func, &resume_points, prune);
    func.frame = Some(Frame {
        slot,
        any_slots: counts[0],
        raw_slots: counts[1],
        resume_points,
        safepoints,
        yield_points,
    });
}
