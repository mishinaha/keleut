//! レジスタの集合(ビット列と、書き換えの日誌を持つ集合)。

use std::collections::VecDeque;

/// 固定の大きさのビット列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bits(Vec<u64>);

impl Bits {
    /// `n` ビットの空の集合。
    pub(crate) fn new(n: usize) -> Bits {
        Bits(vec![0; n.div_ceil(64)])
    }

    pub(crate) fn insert(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }

    pub(crate) fn remove(&mut self, i: usize) {
        self.0[i / 64] &= !(1 << (i % 64));
    }

    pub(crate) fn contains(&self, i: usize) -> bool {
        self.0[i / 64] & (1 << (i % 64)) != 0
    }

    /// 語(64 ビット)の数。
    pub(crate) fn words(&self) -> usize {
        self.0.len()
    }

    /// 要素の数。
    fn count(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// `o` との積にする。
    fn and_with(&mut self, o: &Bits) {
        for (a, b) in self.0.iter_mut().zip(o.0.iter()) {
            *a &= *b;
        }
    }

    /// 含む要素を昇順に並べる。
    fn ones(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for (w, &x) in self.0.iter().enumerate() {
            let mut x = x;
            while x != 0 {
                out.push((w * 64) as u32 + x.trailing_zeros());
                x &= x - 1;
            }
        }
        out
    }

    /// `self` と `o` の一方だけが含む要素を昇順に並べる。
    pub(crate) fn xor_ones(&self, o: &Bits) -> Vec<u32> {
        let mut out = Vec::new();
        for (w, (&a, &b)) in self.0.iter().zip(o.0.iter()).enumerate() {
            let mut x = a ^ b;
            while x != 0 {
                out.push((w * 64) as u32 + x.trailing_zeros());
                x &= x - 1;
            }
        }
        out
    }
}

/// 取っておいた集合。
enum Snap {
    /// 日誌の長さが `t` だった時点の集合。
    Log(usize),
    /// 要素の列(昇順)。
    Sparse(Vec<u32>),
    /// ビット列。
    Bits(Bits),
}

/// 取っておいた集合 `snap` から `rem` を除いたもの。`rem` が空でないのは `snap` が `Log` のときだけである。
struct Slot {
    snap: Snap,
    rem: Vec<u32>,
    /// `Log` の集合と今の集合の違いを求めるのに費やした手間。
    spent: usize,
}

/// 書き換えの日誌を持つレジスタの集合と、取っておいた集合の表。
///
/// データフロー解析は、1 つの集合(今の集合)を書き換えながらブロックを順にたどる。
/// 変えたレジスタを日誌に積むので、日誌の長さ `t` が、その時点の集合を指す名前になる。
/// ブロックの入口や出口の集合は、まず日誌の長さとして取っておき、写しを作らない。
/// 取っておいた時点の集合と今の集合の違いは、その後の日誌をたどれば分かる。
///
/// 手間がブロックごとに集合の幅(語の数)に比例する量を超えないよう、次の 3 つの決まりを置く。
///
/// - 取っておいた時点から日誌が `w` 個より伸びたら、その集合を作り直して持つ(**書き出し**)。
///   要素が語の数より少なければ要素の列で、多ければビット列で持つ。
///   1 つの集合を書き出すのは 1 度だけで、手間は語の数と `w` に比例する。
/// - 今の集合を一度に大きく変える(違いが `w` 個を超える)ときは、日誌に積まずに書き換える(**跳び**)。
///   その前に日誌の長さで取っておいた集合をすべて書き出し、日誌を捨てて世代を 1 つ進める。
///   日誌の長さは世代の中でだけ意味を持つ。
/// - 移る先の無いブロックは今の集合を使わずに処理できるよう、要素の列を集合として取っておける。
pub(crate) struct Timeline {
    live: Bits,
    /// 今の集合の要素(順不同)と、各レジスタの `list` の中の位置(無ければ `u32::MAX`)。
    list: Vec<u32>,
    pos: Vec<u32>,
    /// 変えたレジスタと、変えた後に含むか。
    log: Vec<(u32, bool)>,
    gen_: u32,
    slots: Vec<Option<Slot>>,
    /// 日誌の長さで取っておいた集合(表の添字と時点)。時点の昇順で、古い行が残ることがある。
    queue: VecDeque<(usize, usize)>,
    w: usize,
    /// 違いを求めるときに、見たレジスタに付ける印。
    stamp: Vec<u32>,
    epoch: u32,
    /// 手間(日誌の記録と、集合の要素と語を読み書きした数)。
    work: u64,
}

impl Timeline {
    /// `nr` 個のレジスタの空の集合と、`nslots` 行の空の表。
    pub(crate) fn new(nr: usize, nslots: usize) -> Timeline {
        let live = Bits::new(nr);
        let w = live.words() * 4 + 64;
        Timeline {
            live,
            list: Vec::new(),
            pos: vec![u32::MAX; nr],
            log: Vec::new(),
            gen_: 0,
            slots: (0..nslots).map(|_| None).collect(),
            queue: VecDeque::new(),
            w,
            stamp: vec![0; nr],
            epoch: 0,
            work: 0,
        }
    }

    /// これまでの手間。
    pub(crate) fn work(&self) -> u64 {
        self.work
    }

    /// 今の時点(世代と日誌の長さ)。
    pub(crate) fn now(&self) -> (u32, usize) {
        (self.gen_, self.log.len())
    }

    /// 今の集合のビット列。
    pub(crate) fn bits(&self) -> &Bits {
        &self.live
    }

    /// 今の集合が `r` を含むか。
    pub(crate) fn contains(&self, r: u32) -> bool {
        self.live.contains(r as usize)
    }

    /// 時点 `t`(今の世代)より後に変えたレジスタ(重なることがある)。
    pub(crate) fn changed_since(&self, t: usize) -> impl Iterator<Item = u32> + '_ {
        self.log[t..].iter().map(|&(r, _)| r)
    }

    fn set_raw(&mut self, r: u32, on: bool) -> bool {
        let ri = r as usize;
        if self.live.contains(ri) == on {
            return false;
        }
        if on {
            self.live.insert(ri);
            self.pos[ri] = self.list.len() as u32;
            self.list.push(r);
        } else {
            self.live.remove(ri);
            let i = self.pos[ri] as usize;
            self.pos[ri] = u32::MAX;
            self.list.swap_remove(i);
            if let Some(&m) = self.list.get(i) {
                self.pos[m as usize] = i as u32;
            }
        }
        true
    }

    /// `r` を足すか(`on`)除き、日誌に積む。
    pub(crate) fn set(&mut self, r: u32, on: bool) {
        if self.set_raw(r, on) {
            self.log.push((r, on));
            self.work += 1;
        }
    }

    fn next_epoch(&mut self) -> u32 {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.iter_mut().for_each(|x| *x = 0);
            self.epoch = 1;
        }
        self.epoch
    }

    /// 今の集合を表の `i` 行に取っておく(日誌の長さで)。
    pub(crate) fn save(&mut self, i: usize) {
        let t = self.log.len();
        self.slots[i] = Some(Slot {
            snap: Snap::Log(t),
            rem: Vec::new(),
            spent: 0,
        });
        self.queue.push_back((i, t));
    }

    /// 要素の列 `v`(昇順)を表の `i` 行に取っておく。
    pub(crate) fn save_list(&mut self, i: usize, v: Vec<u32>) {
        self.work += v.len() as u64;
        self.slots[i] = Some(Slot {
            snap: Snap::Sparse(v),
            rem: Vec::new(),
            spent: 0,
        });
    }

    /// 表の `i` 行を捨てる。
    pub(crate) fn free(&mut self, i: usize) {
        self.slots[i] = None;
    }

    /// 表の `i` 行に集合があるか。
    pub(crate) fn has(&self, i: usize) -> bool {
        self.slots[i].is_some()
    }

    /// 表の `i` 行を、日誌に頼らない形で書き出す。
    fn materialize(&mut self, i: usize) {
        let Some(slot) = self.slots[i].as_mut() else {
            return;
        };
        let Snap::Log(t) = slot.snap else {
            return;
        };
        let mut b = self.live.clone();
        for &(r, on) in self.log[t..].iter().rev() {
            if on {
                b.remove(r as usize);
            } else {
                b.insert(r as usize);
            }
        }
        for &r in &slot.rem {
            b.remove(r as usize);
        }
        self.work += (2 * b.words() + self.log.len() - t + slot.rem.len()) as u64;
        slot.rem = Vec::new();
        slot.snap = if b.count() <= b.words() {
            Snap::Sparse(b.ones())
        } else {
            Snap::Bits(b)
        };
    }

    /// 日誌の長さで取っておいた集合のうち、日誌が `w` 個より伸びたものを書き出す。
    pub(crate) fn tick(&mut self) {
        let now = self.log.len();
        while let Some(&(i, t)) = self.queue.front() {
            let live = matches!(
                self.slots[i].as_ref().map(|s| &s.snap),
                Some(Snap::Log(u)) if *u == t
            );
            if live && now - t <= self.w {
                break;
            }
            self.queue.pop_front();
            if live {
                self.materialize(i);
            }
        }
    }

    /// 表の `i` 行の集合と今の集合とで違うレジスタと、`i` 行の集合が含むか。
    ///
    /// `off` が偽なら、`i` 行の集合だけが含むものを求める。
    fn diff(&mut self, i: usize, off: bool) -> Vec<(u32, bool)> {
        let e = self.next_epoch();
        let mut out = Vec::new();
        let slot = self.slots[i].as_ref().expect("取っておいた集合");
        match &slot.snap {
            Snap::Log(t) => {
                for &r in &slot.rem {
                    self.stamp[r as usize] = e;
                    if off && self.live.contains(r as usize) {
                        out.push((r, false));
                    }
                }
                // 時点 t の後でレジスタを最初に変えた記録から、時点 t の値が分かる。
                for &(r, on) in &self.log[*t..] {
                    let ri = r as usize;
                    if self.stamp[ri] != e {
                        self.stamp[ri] = e;
                        if !on != self.live.contains(ri) && (!on || off) {
                            out.push((r, !on));
                        }
                    }
                }
                let c = self.log.len() - t + slot.rem.len();
                self.work += c as u64;
                self.slots[i].as_mut().expect("取っておいた集合").spent += c;
            }
            Snap::Sparse(v) => {
                for &r in v {
                    self.stamp[r as usize] = e;
                    if !self.live.contains(r as usize) {
                        out.push((r, true));
                    }
                }
                self.work += v.len() as u64;
                if off {
                    if self.list.len() <= 2 * self.live.words() {
                        for &r in &self.list {
                            if self.stamp[r as usize] != e {
                                out.push((r, false));
                            }
                        }
                        self.work += self.list.len() as u64;
                    } else {
                        let mut b = Bits::new(self.stamp.len());
                        for &r in v {
                            b.insert(r as usize);
                        }
                        for r in b.xor_ones(&self.live) {
                            if !b.contains(r as usize) {
                                out.push((r, false));
                            }
                        }
                        self.work += 2 * b.words() as u64 + out.len() as u64;
                    }
                }
            }
            Snap::Bits(b) => {
                for r in b.xor_ones(&self.live) {
                    let on = b.contains(r as usize);
                    if on || off {
                        out.push((r, on));
                    }
                }
                self.work += b.words() as u64 + out.len() as u64;
            }
        }
        out
    }

    /// 違いを当てる。大きければ跳びにする。
    fn apply(&mut self, d: Vec<(u32, bool)>) {
        if d.len() <= self.w {
            for (r, on) in d {
                self.set(r, on);
            }
            return;
        }
        let q = std::mem::take(&mut self.queue);
        for (i, _) in q {
            self.materialize(i);
        }
        self.work += d.len() as u64;
        for (r, on) in d {
            self.set_raw(r, on);
        }
        self.log.clear();
        self.gen_ += 1;
    }

    /// 今の集合を表の `i` 行の集合にする。
    pub(crate) fn restore(&mut self, i: usize) {
        let d = self.diff(i, true);
        self.settle(i);
        self.apply(d);
    }

    /// 違いを求める手間が集合の幅に見合う量を超えた `Log` の行を書き出す。
    fn settle(&mut self, i: usize) {
        if self.slots[i].as_ref().is_some_and(|s| s.spent > self.w) {
            self.materialize(i);
        }
    }

    /// 今の集合を空にする。
    pub(crate) fn clear(&mut self) {
        let d: Vec<(u32, bool)> = self.list.iter().map(|&r| (r, false)).collect();
        self.work += d.len() as u64;
        self.apply(d);
    }

    /// 今の集合に表の `i` 行の集合を足す。
    pub(crate) fn union(&mut self, i: usize) {
        let d = self.diff(i, false);
        self.settle(i);
        self.apply(d);
    }

    /// 表の `i` 行の集合を、今の集合との積にする。行が空なら今の集合を取っておく。
    pub(crate) fn meet_into(&mut self, i: usize) {
        if self.slots[i].is_none() {
            self.save(i);
            return;
        }
        let is_log = matches!(self.slots[i].as_ref().map(|s| &s.snap), Some(Snap::Log(_)));
        if is_log {
            // 積は「今の集合から、行の集合が含まないものを除いたもの」である。
            let d = self.diff(i, true);
            let rem: Vec<u32> = d
                .into_iter()
                .filter(|&(_, on)| !on)
                .map(|(r, _)| r)
                .collect();
            let t = self.log.len();
            let spent = self.slots[i].as_ref().map_or(0, |s| s.spent);
            self.slots[i] = Some(Slot {
                snap: Snap::Log(t),
                rem,
                spent,
            });
            self.queue.push_back((i, t));
            self.settle(i);
            return;
        }
        let live = &self.live;
        let slot = self.slots[i].as_mut().expect("取っておいた集合");
        match &mut slot.snap {
            Snap::Sparse(v) => {
                self.work += v.len() as u64;
                v.retain(|&r| live.contains(r as usize));
            }
            Snap::Bits(b) => {
                b.and_with(live);
                self.work += 2 * b.words() as u64;
                if b.count() <= b.words() {
                    slot.snap = Snap::Sparse(b.ones());
                }
            }
            Snap::Log(_) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 小さな擬似乱数(xorshift)。
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    fn set_of(tl: &Timeline) -> Vec<u32> {
        let mut v = tl.list.clone();
        v.sort_unstable();
        assert_eq!(v, tl.live.ones());
        v
    }

    /// 取っておいた集合を、素朴に持った集合と比べながら、乱数で操作を重ねる。
    #[test]
    fn 取っておいた集合は素朴な写しと一致する() {
        for seed in 1..40u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let nr = 1 + rng.below(300);
            let ns = 1 + rng.below(12);
            let mut tl = Timeline::new(nr, ns);
            let mut model: Vec<Option<Vec<u32>>> = vec![None; ns];
            let mut cur: Vec<bool> = vec![false; nr];
            for step in 0..3000 {
                let i = rng.below(ns);
                let op = rng.below(10);
                match op {
                    0..=3 => {
                        let r = rng.below(nr) as u32;
                        let on = rng.below(2) == 0;
                        tl.set(r, on);
                        cur[r as usize] = on;
                    }
                    4 => {
                        tl.save(i);
                        model[i] = Some(set_of(&tl));
                    }
                    5 => {
                        let mut v: Vec<u32> =
                            (0..nr as u32).filter(|_| rng.below(8) == 0).collect();
                        v.dedup();
                        tl.save_list(i, v.clone());
                        model[i] = Some(v);
                    }
                    6 if model[i].is_some() => {
                        tl.restore(i);
                        let m = model[i].clone().unwrap();
                        cur = vec![false; nr];
                        m.iter().for_each(|&r| cur[r as usize] = true);
                    }
                    7 if model[i].is_some() => {
                        tl.union(i);
                        model[i]
                            .clone()
                            .unwrap()
                            .iter()
                            .for_each(|&r| cur[r as usize] = true);
                    }
                    8 => {
                        tl.meet_into(i);
                        let now: Vec<u32> = (0..nr as u32).filter(|&r| cur[r as usize]).collect();
                        model[i] = Some(match &model[i] {
                            None => now,
                            Some(m) => m.iter().copied().filter(|&r| cur[r as usize]).collect(),
                        });
                    }
                    9 if rng.below(20) == 0 => {
                        tl.clear();
                        cur = vec![false; nr];
                    }
                    _ => {}
                }
                tl.tick();
                let want: Vec<u32> = (0..nr as u32).filter(|&r| cur[r as usize]).collect();
                assert_eq!(set_of(&tl), want, "seed {seed} step {step} op {op}");
                // 取っておいた集合を戻して確かめ、今の集合に戻す。
                if i != ns - 1 && model[i].is_some() && rng.below(4) == 0 {
                    let m = model[i].clone().unwrap();
                    tl.save(ns - 1);
                    model[ns - 1] = Some(want.clone());
                    tl.restore(i);
                    assert_eq!(set_of(&tl), m, "seed {seed}");
                    tl.restore(ns - 1);
                    assert_eq!(set_of(&tl), want, "seed {seed}");
                }
            }
        }
    }
}
