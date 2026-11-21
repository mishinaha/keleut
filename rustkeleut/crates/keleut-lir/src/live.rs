//! 再開点と安全点の並びと、各点で生存しているレジスタの集合。
//!
//! 点ごとに集合の全体を持つと、値を保持したまま確保や呼び出しを重ねる連鎖(長いリストのリテラルなど)があると、サイズの合計が点の個数と生存している値の個数の積になる。
//! そこで、各点の集合を、位置の順で次の点の集合からの差分(追加するレジスタと取り除くレジスタ)として持つ。
//! 差分を積み重ねた量が、その点の集合のサイズに 8 を足した値を超えた点では、差分ではなく集合の全体を持つ。
//! この集合の全体を**コピー**と呼ぶ。
//! 位置の順で最後の点も、必ずコピーにする。
//! こうすると、保持する量の合計は、最後の点のコピーを除けば差分の量の合計の 2 倍を超えず、1 つの点の集合を作り直す処理時間はその集合のサイズに比例する。

use std::collections::TryReserveError;

use crate::ir::{BlockId, Reg};

/// 関数の中の命令の位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point {
    /// 命令のあるブロック。
    pub block: BlockId,
    /// ブロックの中で、0 始まりで何番目の命令かを表す番号。
    pub index: u32,
}

/// 1 つの点が持つ差分かコピー。
///
/// `data[start..]` の先頭の `nadd` 個が追加するレジスタで、続く `nrem` 個が取り除くレジスタである。
/// コピー(`full` が真)では、`nadd` 個が集合そのもので、`nrem` は 0 である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    full: bool,
    start: u32,
    nadd: u32,
    nrem: u32,
}

/// 位置の順に並べた点と、各点で生存しているレジスタの集合。
///
/// `i` 番目の点の集合は、`i` 番目がコピーならそのコピーである。
/// そうでなければ、`i + 1` 番目の点の集合に `i` 番目の差分を適用したものである。
/// 最後の点は必ずコピーである。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Points {
    at: Vec<Point>,
    entries: Vec<Entry>,
    /// 各点について、その点か、それより後ろで最も近いコピーの添字。
    snap: Vec<u32>,
    data: Vec<Reg>,
}

impl Points {
    /// 点の個数を返す。
    pub fn len(&self) -> usize {
        self.at.len()
    }

    /// 点が 1 つもなければ真を返す。
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }

    /// `i` 番目の点の位置を返す。
    pub fn point(&self, i: usize) -> Point {
        self.at[i]
    }

    /// すべての点の位置を、位置の順に並べたスライスで返す。
    pub fn points(&self) -> &[Point] {
        &self.at
    }

    /// `i` 番目の点で生存しているレジスタを、番号の昇順で返す。
    ///
    /// メモリを確保できなければ `handle_alloc_error` で異常終了する。
    pub fn live(&self, i: usize) -> Vec<Reg> {
        let mut out = Vec::new();
        let mut work = LiveWork::default();
        self.live_into(i, &mut out, &mut work)
            .unwrap_or_else(|_| std::alloc::handle_alloc_error(std::alloc::Layout::new::<Reg>()));
        out
    }

    /// `i` 番目の点で生存しているレジスタを、番号の昇順で `out` の後ろに追加する。
    ///
    /// `out` と作業用バッファ `work` のメモリは `try_reserve` で確保し、確保できなければ失敗を返す。
    /// それ以外にメモリを確保しないので、GC の実行中でも呼び出せる。
    /// GC は `out` と `work` を再利用して、この関数を呼び出す。
    /// 失敗したとき、`out` に途中までのレジスタが残ることがある。
    pub fn live_into(
        &self,
        i: usize,
        out: &mut Vec<Reg>,
        work: &mut LiveWork,
    ) -> Result<(), TryReserveError> {
        let j = self.snap[i] as usize;
        let e = self.entries[j];
        let base = &self.data[e.start as usize..(e.start + e.nadd) as usize];
        if i == j {
            out.try_reserve(base.len())?;
            out.extend_from_slice(base);
            return Ok(());
        }
        // コピーから前へ、差分を順に適用した結果を求める。
        // 各レジスタの結果は、そのレジスタを含む差分のうち、i に最も近い点の差分で決まる。
        // 同じ点の差分が同じレジスタを追加し、かつ取り除くときは、取り除くほうを採る。
        // (レジスタ, 点, 追加するかどうか)の組を並べて昇順に整列すると、各レジスタの最初の組が、結果を決める差分である。
        let over = &mut work.over;
        over.clear();
        let n: usize = (i..j)
            .map(|k| (self.entries[k].nadd + self.entries[k].nrem) as usize)
            .sum();
        over.try_reserve(n)?;
        for k in i..j {
            let e = self.entries[k];
            let s = e.start as usize;
            let a = e.nadd as usize;
            for &r in &self.data[s..s + a] {
                over.push((r, k as u32, true));
            }
            for &r in &self.data[s + a..s + a + e.nrem as usize] {
                over.push((r, k as u32, false));
            }
        }
        over.sort_unstable();
        over.dedup_by_key(|x| x.0);
        out.try_reserve(base.len() + over.len())?;
        let (mut x, mut y) = (0, 0);
        while x < base.len() || y < over.len() {
            if y >= over.len() || (x < base.len() && base[x] < over[y].0) {
                out.push(base[x]);
                x += 1;
            } else {
                let (r, _, on) = over[y];
                if on {
                    out.push(r);
                }
                if x < base.len() && base[x] == r {
                    x += 1;
                }
                y += 1;
            }
        }
        Ok(())
    }

    /// 点と、その点で生存しているレジスタの組を、位置の順に返す。
    pub fn iter(&self) -> impl Iterator<Item = (Point, Vec<Reg>)> + '_ {
        (0..self.len()).map(|i| (self.at[i], self.live(i)))
    }

    /// 差分とコピーとして保持しているレジスタの個数の合計を返す。
    pub fn stored_regs(&self) -> usize {
        self.data.len()
    }

    /// 位置の順に並べた点と集合の組から [`Points`] を作る。
    ///
    /// 各集合は番号の昇順に並んでいなければならない。
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

/// [`Points::live_into`] の作業用バッファ。
///
/// 再利用すれば、容量が足りている間は呼び出しのたびにメモリを確保し直さずに済む。
#[derive(Debug, Default)]
pub struct LiveWork {
    over: Vec<(Reg, u32, bool)>,
}

/// `from` から `to` への差分を、(追加するレジスタ, 取り除くレジスタ)の組で返す。
///
/// 引数も結果も、番号の昇順に並んでいる。
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

/// 点を位置の逆順に受け取って [`Points`] を作るビルダ。
#[derive(Default)]
pub(crate) struct Builder {
    at: Vec<Point>,
    entries: Vec<Entry>,
    data: Vec<Reg>,
    /// 最後に作ったコピーより後に追加した差分の量。
    acc: usize,
}

impl Builder {
    /// 位置の逆順で次の点を追加する。
    ///
    /// `add` と `rem` は、直前に追加した点の集合からの差分(昇順)で、`size` はこの点の集合の要素数である。
    /// `full` はこの点の集合(昇順)を返す関数で、この点をコピーにするときだけ呼ぶ。
    /// 最初に追加する点は、必ずコピーにする。
    /// それ以外の点は、最後に作ったコピーより後に追加した差分の量(この点の差分を含む)が `size + 8` を超えたときにコピーにする。
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

    /// 点を位置の順に並べ直し、[`Points`] にして返す。
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
        let (mut out, mut work) = (Vec::new(), LiveWork::default());
        for (i, (pt, s)) in sets.iter().enumerate() {
            assert_eq!(p.point(i), *pt);
            assert_eq!(&p.live(i), s);
            // 出力先と作業用バッファを再利用しても、同じ集合になる。
            out.clear();
            p.live_into(i, &mut out, &mut work).unwrap();
            assert_eq!(&out, s);
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
