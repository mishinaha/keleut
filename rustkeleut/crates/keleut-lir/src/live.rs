//! 再開点と安全点の列と、各点で生きているレジスタの集合。
//!
//! 集合を点ごとに丸ごと持つと、値を持ち続けたまま確保や呼び出しを重ねる鎖(長いリストのリテラルなど)で、
//! 大きさの合計が点の数と生きている値の数の積になる。
//! そこで、各点の集合を、位置の順で次の点の集合からの差分(足すレジスタと除くレジスタ)で持つ。
//! 差分を積み重ねた量がその点の集合の大きさを超えたら、その点では集合を丸ごと持つ(**写し**)。
//! こうすると、持つ量の合計は差分の量の合計の 2 倍を超えず、1 つの点の集合を作り直す手間は
//! その集合の大きさに比例する。

use std::collections::HashMap;

use crate::ir::{BlockId, Reg};

/// 関数の中の命令の位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point {
    /// ブロック。
    pub block: BlockId,
    /// ブロックの中の命令の位置。
    pub index: u32,
}

/// 1 つの点の記録。`data[start..]` の `nadd` 個が足すレジスタ(写しなら集合そのもの)、続く `nrem` 個が除くレジスタである。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    full: bool,
    start: u32,
    nadd: u32,
    nrem: u32,
}

/// 点の列(位置の順)と、各点で生きているレジスタの集合。
///
/// `i` 番目の点の集合は、`i` 番目が写しならその写しで、そうでなければ `i + 1` 番目の集合に
/// `i` 番目の差分を当てたものである。
/// 最後の点は必ず写しである。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Points {
    at: Vec<Point>,
    entries: Vec<Entry>,
    /// 各点から後ろへ見て最も近い写しの添字。
    snap: Vec<u32>,
    data: Vec<Reg>,
}

impl Points {
    /// 点の数。
    pub fn len(&self) -> usize {
        self.at.len()
    }

    /// 点が無いか。
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }

    /// `i` 番目の点の位置。
    pub fn point(&self, i: usize) -> Point {
        self.at[i]
    }

    /// 点の位置の列。
    pub fn points(&self) -> &[Point] {
        &self.at
    }

    /// `i` 番目の点で生きているレジスタ(番号の昇順)。
    pub fn live(&self, i: usize) -> Vec<Reg> {
        let j = self.snap[i] as usize;
        let e = self.entries[j];
        let base = &self.data[e.start as usize..(e.start + e.nadd) as usize];
        if i == j {
            return base.to_vec();
        }
        // 写しから前へ差分を順に当てる。各レジスタの最後の当て方だけが効く。
        let mut over: HashMap<Reg, bool> = HashMap::new();
        for k in (i..j).rev() {
            let e = self.entries[k];
            let s = e.start as usize;
            let a = e.nadd as usize;
            for &r in &self.data[s..s + a] {
                over.insert(r, true);
            }
            for &r in &self.data[s + a..s + a + e.nrem as usize] {
                over.insert(r, false);
            }
        }
        let mut set: Vec<Reg> = base
            .iter()
            .copied()
            .filter(|r| over.get(r) != Some(&false))
            .collect();
        set.extend(
            over.iter()
                .filter(|&(r, &on)| on && base.binary_search(r).is_err())
                .map(|(&r, _)| r),
        );
        set.sort_unstable();
        set
    }

    /// 点と、そこで生きているレジスタを順に返す。
    pub fn iter(&self) -> impl Iterator<Item = (Point, Vec<Reg>)> + '_ {
        (0..self.len()).map(|i| (self.at[i], self.live(i)))
    }

    /// 持っているレジスタの数の合計(差分と写し)。
    pub fn stored_regs(&self) -> usize {
        self.data.len()
    }

    /// 点と集合の列(位置の順)から作る。集合は番号の昇順でなければならない。
    pub fn from_sets(sets: Vec<(Point, Vec<Reg>)>) -> Points {
        let mut b = Builder::default();
        let mut prev: Option<Vec<Reg>> = None;
        for (pt, set) in sets.into_iter().rev() {
            let (add, rem) = match &prev {
                Some(p) => diff(p, &set),
                None => (set.clone(), Vec::new()),
            };
            b.push(pt, &add, &rem, set.len(), || set.clone());
            prev = Some(set);
        }
        b.finish()
    }
}

/// `from` から `to` への差分(足すもの、除くもの)。どれも昇順。
fn diff(from: &[Reg], to: &[Reg]) -> (Vec<Reg>, Vec<Reg>) {
    let (mut add, mut rem) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < from.len() || j < to.len() {
        if j >= to.len() || (i < from.len() && from[i] < to[j]) {
            rem.push(from[i]);
            i += 1;
        } else if i >= from.len() || to[j] < from[i] {
            add.push(to[j]);
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }
    (add, rem)
}

/// 点を位置の逆の順に受け取って [`Points`] を作る。
#[derive(Default)]
pub(crate) struct Builder {
    at: Vec<Point>,
    entries: Vec<Entry>,
    data: Vec<Reg>,
    /// 最後の写しの後に積んだ差分の量。
    acc: usize,
}

impl Builder {
    /// 次の点(位置の逆の順)を足す。
    ///
    /// `add` と `rem` は、直前に足した点の集合からの差分(昇順)で、`size` はこの点の集合の大きさである。
    /// `full` は、写しにするときだけ呼んでこの点の集合(昇順)を得る。
    pub(crate) fn push(
        &mut self,
        pt: Point,
        add: &[Reg],
        rem: &[Reg],
        size: usize,
        full: impl FnOnce() -> Vec<Reg>,
    ) {
        self.at.push(pt);
        let start = self.data.len() as u32;
        self.acc += add.len() + rem.len();
        if self.entries.is_empty() || self.acc > size + 8 {
            let set = full();
            debug_assert_eq!(set.len(), size);
            self.entries.push(Entry {
                full: true,
                start,
                nadd: set.len() as u32,
                nrem: 0,
            });
            self.data.extend_from_slice(&set);
            self.acc = 0;
        } else {
            self.entries.push(Entry {
                full: false,
                start,
                nadd: add.len() as u32,
                nrem: rem.len() as u32,
            });
            self.data.extend_from_slice(add);
            self.data.extend_from_slice(rem);
        }
    }

    /// 位置の順に並べ直して取り出す。
    pub(crate) fn finish(mut self) -> Points {
        self.at.reverse();
        self.entries.reverse();
        let n = self.entries.len();
        let mut snap = vec![0u32; n];
        let mut last = 0u32;
        for i in (0..n).rev() {
            if self.entries[i].full {
                last = i as u32;
            }
            snap[i] = last;
        }
        Points {
            at: self.at,
            entries: self.entries,
            snap,
            data: self.data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs(v: &[u32]) -> Vec<Reg> {
        v.iter().map(|&x| Reg(x)).collect()
    }

    #[test]
    fn 差分から集合を作り直す() {
        let sets: Vec<(Point, Vec<Reg>)> = (0..200u32)
            .map(|i| {
                let v: Vec<u32> = (0..i % 37).map(|k| k * 3 + (i % 5)).collect();
                (
                    Point {
                        block: BlockId(i / 10),
                        index: i % 10,
                    },
                    regs(&v),
                )
            })
            .collect();
        let p = Points::from_sets(sets.clone());
        assert_eq!(p.len(), sets.len());
        for (i, (pt, s)) in sets.iter().enumerate() {
            assert_eq!(p.point(i), *pt);
            assert_eq!(&p.live(i), s);
        }
    }

    #[test]
    fn 少しずつ育つ集合は線形の量で持つ() {
        let n = 5000u32;
        let sets: Vec<(Point, Vec<Reg>)> = (0..n)
            .map(|i| {
                (
                    Point {
                        block: BlockId(0),
                        index: i,
                    },
                    regs(&(0..n - i).collect::<Vec<_>>()),
                )
            })
            .collect();
        let p = Points::from_sets(sets);
        assert!(p.stored_regs() < 4 * n as usize, "{}", p.stored_regs());
        assert_eq!(p.live(0).len(), n as usize);
        assert_eq!(p.live(n as usize - 1), regs(&[0]));
    }
}
