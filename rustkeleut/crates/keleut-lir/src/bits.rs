//! レジスタの集合。
//!
//! 固定長のビット列 `Bits` と、変更ログを持つ集合 `Timeline` を定義する。

use std::collections::VecDeque;

/// 固定長のビット列。レジスタの番号を要素とする集合を表す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bits(Vec<u64>);

impl Bits {
    /// `n` ビットの空の集合を作る。
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

    /// ワード数を返す。1 ワードは 64 ビットである。
    pub(crate) fn words(&self) -> usize {
        self.0.len()
    }

    /// 要素の個数を返す。
    fn count(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// `self` を `o` との積集合にする。
    fn and_with(&mut self, o: &Bits) {
        for (a, b) in self.0.iter_mut().zip(o.0.iter()) {
            *a &= *b;
        }
    }

    /// 要素を昇順に並べて返す。
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

    /// `self` と `o` の一方だけが含む要素を、昇順に並べて返す。
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

/// 保存した集合の持ち方。
enum Snap {
    /// 変更ログの長さが `t` だった時点の集合。
    Log(usize),
    /// 昇順に並べた要素の配列。
    Sparse(Vec<u32>),
    /// ビット列。
    Bits(Bits),
}

/// 表の項目。保存した集合 `snap` から `rem` の要素を除いた集合を表す。
/// `rem` が空でないのは、`snap` が `Log` のときだけである。
struct Slot {
    snap: Snap,
    rem: Vec<u32>,
    /// `Log` の集合と現在の集合の違いを求めるのに、これまでにかけた処理量。
    spent: usize,
}

/// 変更ログを持つレジスタの集合と、保存した集合の表。
///
/// データフロー解析は、1 つの集合(現在の集合)を書き換えながらブロックを順にたどる。
/// 変えたレジスタを変更ログに追記するので、変更ログの長さ `t` で、その時点の集合を指定できる。
/// ブロックの入口や出口の集合は、まず変更ログの長さとして保存し、コピーを作らない。
/// 保存した時点の集合と現在の集合の違いは、その時点より後の変更ログをたどれば分かる。
///
/// ブロック 1 つあたりの処理量が、集合の幅(ワード数)に比例する量を超えないように、次の 3 つの規則を置く。
/// 以下の `w` は、ワード数の 4 倍に 64 を足した値である。
///
/// - 保存した時点から変更ログが `w` 個より多く伸びたら、その集合を、変更ログを参照しない表現(要素の配列かビット列)に作り直して保持する(**実体化**)。
///   要素の個数がワード数以下なら要素の配列に、ワード数より多ければビット列にする。
///   1 つの集合を実体化するのは 1 度だけで、処理量はワード数と `w` に比例する。
/// - 現在の集合を一度に大きく変える(違いが `w` 個を超える)ときは、変更ログに追記せずに書き換える(**一括書き換え**)。
///   その前に、変更ログの長さとして保存した集合をすべて実体化し、変更ログを捨てて世代を 1 つ進める。
///   変更ログの長さは、同じ世代の中でだけ意味を持つ。
/// - 後続ブロックを持たないブロックを現在の集合を使わずに処理できるように、要素の配列を集合として保存できる。
pub(crate) struct Timeline {
    /// 現在の集合のビット列。
    live: Bits,
    /// 現在の集合の要素(順不同)。
    list: Vec<u32>,
    /// 各レジスタの `list` の中の位置。現在の集合に入っていなければ `u32::MAX` である。
    pos: Vec<u32>,
    /// 変更ログ。変えたレジスタと、変えた後に集合がそのレジスタを含むかどうかを、変えた順に記録する。
    log: Vec<(u32, bool)>,
    /// 世代。一括書き換えのたびに 1 つ進む。
    gen_: u32,
    /// 保存した集合の表。
    slots: Vec<Option<Slot>>,
    /// 変更ログの長さとして保存した集合を、表の添字と時点の組で持つキュー。
    /// 時点の昇順に並ぶ。
    /// 表の項目を上書きしたり捨てたりした後も、古い組が残ることがある。
    queue: VecDeque<(usize, usize)>,
    /// 実体化と一括書き換えのしきい値。
    w: usize,
    /// 違いを求めるときに、調べたレジスタに付ける目印。値が `epoch` に等しければ、その回の計算で調べてある。
    stamp: Vec<u32>,
    epoch: u32,
    /// 処理量のカウンタ。変更ログに追記した回数と、集合の要素やワードを読み書きした回数を足していく。
    work: u64,
}

impl Timeline {
    /// `nr` 個のレジスタを要素にできる空の集合と、`nslots` 項目の空の表を作る。
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

    /// これまでの処理量を返す。
    pub(crate) fn work(&self) -> u64 {
        self.work
    }

    /// 現在の時点を、世代と変更ログの長さの組で返す。
    pub(crate) fn now(&self) -> (u32, usize) {
        (self.gen_, self.log.len())
    }

    /// 現在の集合をビット列として返す。
    pub(crate) fn bits(&self) -> &Bits {
        &self.live
    }

    /// 現在の集合が `r` を含めば真を返す。
    pub(crate) fn contains(&self, r: u32) -> bool {
        self.live.contains(r as usize)
    }

    /// 現在の世代の時点 `t` より後に変えたレジスタを、変えた順に返す。
    /// 同じレジスタが 2 回以上現れることがある。
    pub(crate) fn changed_since(&self, t: usize) -> impl Iterator<Item = u32> + '_ {
        self.log[t..].iter().map(|&(r, _)| r)
    }

    /// `on` が真なら現在の集合に `r` を追加し、偽なら現在の集合から `r` を取り除く。
    /// 変更ログには追記しない。
    /// 集合が変われば真を返す。
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

    /// `on` が真なら現在の集合に `r` を追加し、偽なら現在の集合から `r` を取り除く。
    /// 集合が変われば、変更ログに追記する。
    pub(crate) fn set(&mut self, r: u32, on: bool) {
        if self.set_raw(r, on) {
            self.log.push((r, on));
            self.work += 1;
        }
    }

    /// `epoch` を 1 つ進めて返す。
    /// 一周して 0 になったら、`stamp` をすべて 0 にして 1 から始める。
    fn next_epoch(&mut self) -> u32 {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.iter_mut().for_each(|x| *x = 0);
            self.epoch = 1;
        }
        self.epoch
    }

    /// 現在の集合を、変更ログの長さとして表の `i` 番目の項目に保存する。
    pub(crate) fn save(&mut self, i: usize) {
        let t = self.log.len();
        self.slots[i] = Some(Slot {
            snap: Snap::Log(t),
            rem: Vec::new(),
            spent: 0,
        });
        self.queue.push_back((i, t));
    }

    /// 昇順に並べた要素の配列 `v` を、集合として表の `i` 番目の項目に保存する。
    pub(crate) fn save_list(&mut self, i: usize, v: Vec<u32>) {
        self.work += v.len() as u64;
        self.slots[i] = Some(Slot {
            snap: Snap::Sparse(v),
            rem: Vec::new(),
            spent: 0,
        });
    }

    /// 表の `i` 番目の項目を捨てる。
    pub(crate) fn free(&mut self, i: usize) {
        self.slots[i] = None;
    }

    /// 表の `i` 番目の項目に集合があれば真を返す。
    pub(crate) fn has(&self, i: usize) -> bool {
        self.slots[i].is_some()
    }

    /// 表の `i` 番目の項目を実体化する。
    /// 項目が空のときと、すでに実体化してあるときは、何もしない。
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

    /// 変更ログの長さとして保存した集合のうち、保存した後に変更ログが `w` 個より多く伸びたものを実体化する。
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

    /// 表の `i` 番目の項目の集合と現在の集合を比較し、一方だけが含むレジスタを返す。
    ///
    /// 組の 2 番目の値は、項目の集合がそのレジスタを含めば真である。
    /// `off` が偽なら、項目の集合だけが含むレジスタに限って返す。
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
                // 時点 t より後でそのレジスタを最初に変えた記録から、時点 t の集合がそのレジスタを含んでいたかどうかが分かる。
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

    /// 違い `d` を現在の集合に適用する。
    /// `d` の要素が `w` 個を超えていれば、変更ログに追記せず、一括書き換えを行う。
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

    /// 現在の集合を、表の `i` 番目の項目の集合に置き換える。
    pub(crate) fn restore(&mut self, i: usize) {
        let d = self.diff(i, true);
        self.settle(i);
        self.apply(d);
    }

    /// 表の `i` 番目の項目が `Log` で、違いを求めるのにかけた処理量の合計が `w` を超えていれば、その項目を実体化する。
    fn settle(&mut self, i: usize) {
        if self.slots[i].as_ref().is_some_and(|s| s.spent > self.w) {
            self.materialize(i);
        }
    }

    /// 現在の集合を空にする。
    pub(crate) fn clear(&mut self) {
        let d: Vec<(u32, bool)> = self.list.iter().map(|&r| (r, false)).collect();
        self.work += d.len() as u64;
        self.apply(d);
    }

    /// 現在の集合を、表の `i` 番目の項目の集合との和集合にする。
    pub(crate) fn union(&mut self, i: usize) {
        let d = self.diff(i, false);
        self.settle(i);
        self.apply(d);
    }

    /// 表の `i` 番目の項目の集合を、現在の集合との積集合にする。
    /// 項目が空なら、現在の集合を保存する。
    pub(crate) fn meet_into(&mut self, i: usize) {
        if self.slots[i].is_none() {
            self.save(i);
            return;
        }
        let is_log = matches!(self.slots[i].as_ref().map(|s| &s.snap), Some(Snap::Log(_)));
        if is_log {
            // 積集合は、現在の集合から、項目の集合が含まない要素を除いたものである。
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

    /// テスト用の小さな擬似乱数生成器(xorshift)。
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

    /// 乱数で選んだ操作を重ねながら、現在の集合と保存した集合を、単純な実装で持った集合と比較する。
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
                // 保存した集合を現在の集合に復元して確かめてから、元の集合に戻す。
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
