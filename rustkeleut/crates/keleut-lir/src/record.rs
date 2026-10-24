//! レコードの形(パス 6)。
//!
//! レコードの値は、形とフィールドの列からなる。
//! 形はラベルの番号の昇順に並べたラベルの列で、同じラベルが 2 つ以上あるときは外側のものを先に置く
//! (正準の配置。LangSpec §18.3 によりラベルの順は結果に効かない)。
//!
//! KIR 0.3 は型を持たないので、下ろしが形を知るのは、同じ関数の中で作ったレコードと、
//! `closed` のレコードのパターン(タプルのパターン)の対象だけである。
//! `open` のパターンの対象は、パターンに書かないフィールドを持ちうるので、形が分からない。
//! 同じ関数の中で `rec_empty` と `rec_extend` の鎖から作ったレコードは、配置(ラベルとフィールドの値のレジスタ)
//! を覚えておき、選択、制限、更新、照合を位置で行う。
//! ほかのレコードには、実行時に形の表を引く命令(`rec_extend`、`rec_field`、`rec_restrict`、`rec_update`)を使う。
//!
//! 覚えておく配置は、フィールドが [`KNOWN_RECORD_FIELDS`] 個以下のものに限る。
//! 形の分かるレコードの制限、更新、拡張は、配置を写した `rec_new` を出すので、手間と出す被演算子の数が配置の長さに比例する。
//! 配置を限りなく覚えると、それを重ねる鎖(同じレコードを更新し続ける鎖、`let` で束ねたレコードを拡張し続ける鎖など)で、
//! 下ろしの手間と LIR の大きさが鎖の長さの 2 乗になる。
//! 上限を超えるレコードは形の分からないレコードと同じに扱うので、1 つの箇所が出す命令の大きさは定数で抑えられる。

use kir::{Exp, ExpId};

use crate::ir::*;
use crate::lower::{Layout, Low, V};

/// 下ろしが配置を覚えておくレコードのフィールドの数の上限。
pub const KNOWN_RECORD_FIELDS: usize = 64;

/// パターンのラベルの列の各要素が、正準の配置の何番目に来るか。
pub(crate) fn canonical_positions(labels: &[LabelId]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..labels.len()).collect();
    order.sort_by_key(|&i| labels[i]);
    let mut pos = vec![0u32; labels.len()];
    for (p, &i) in order.iter().enumerate() {
        pos[i] = p as u32;
    }
    pos
}

/// 配置の最も外側の `l` の位置。
fn find_outer(layout: &Layout, l: LabelId) -> Option<usize> {
    let at = layout.partition_point(|&(x, _)| x < l);
    (layout.get(at).map(|&(x, _)| x) == Some(l)).then_some(at)
}

impl Low<'_, '_> {
    /// 配置からレコードを作る。
    /// フィールドが [`KNOWN_RECORD_FIELDS`] 個以下のときだけ、配置を覚えておく。
    fn rec_new(&mut self, layout: Layout) -> V {
        if layout.is_empty() {
            let r = self.b.konst(Const::Unit);
            self.known.insert(r, Vec::new());
            return V::any(r);
        }
        let shape = self.p.shape(layout.iter().map(|&(l, _)| l).collect());
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::RecNew {
            dst,
            shape,
            fields: layout.iter().map(|&(_, r)| r).collect(),
        });
        if layout.len() <= KNOWN_RECORD_FIELDS {
            self.known.insert(dst, layout);
        }
        V::any(dst)
    }

    /// `rec_extend` の鎖。値を外側から順に評価し、最後に元のレコードを評価する。
    pub(crate) fn rec_extend(&mut self, e: ExpId) -> Option<V> {
        let k = self.p.k;
        let mut chain: Vec<(LabelId, Reg)> = Vec::new();
        let mut cur = e;
        while let Some(Exp::RecExtend {
            label,
            value,
            record,
        }) = k.nodes.exp(cur)
        {
            let v = self.exp(*value, false)?;
            let r = self.boxed(v);
            let l = self.p.label(label);
            chain.push((l, r));
            cur = *record;
        }
        if !matches!(k.nodes.exp(cur), Some(Exp::RecEmpty)) {
            let base = self.exp(cur, false)?;
            let base = self.boxed(base);
            match self.known.get(&base) {
                Some(l) => chain.extend_from_slice(l),
                None => return Some(self.extend_dyn(base, &chain)),
            }
        }
        // 鎖は外側から順に並び、元の配置はその内側にある。
        // 安定な整列は同じラベルの中でこの順を保つので、外側のフィールドが先に来る(正準の配置)。
        chain.sort_by_key(|&(l, _)| l);
        Some(self.rec_new(chain))
    }

    /// 形の分からないレコード `base` に、鎖のフィールドを内側から順に足す。
    fn extend_dyn(&mut self, base: Reg, chain: &[(LabelId, Reg)]) -> V {
        let mut rec = base;
        for &(label, value) in chain.iter().rev() {
            let dst = self.b.reg(Rep::Any);
            self.b.emit(Inst::RecExtend {
                dst,
                rec,
                label,
                value,
            });
            rec = dst;
        }
        V::any(rec)
    }

    /// `rec_select`。
    pub(crate) fn rec_select(&mut self, label: &str, record: ExpId) -> Option<V> {
        let r = self.exp(record, false)?;
        let rec = self.boxed(r);
        let l = self.p.label(label);
        if let Some(layout) = self.known.get(&rec)
            && let Some(at) = find_outer(layout, l)
        {
            return Some(V::any(layout[at].1));
        }
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::RecField {
            dst,
            rec,
            label: l,
            nth: 0,
        });
        Some(V::any(dst))
    }

    /// `rec_restrict`。
    pub(crate) fn rec_restrict(&mut self, label: &str, record: ExpId) -> Option<V> {
        let r = self.exp(record, false)?;
        let rec = self.boxed(r);
        let l = self.p.label(label);
        if let Some(mut layout) = self.known.get(&rec).cloned()
            && let Some(at) = find_outer(&layout, l)
        {
            layout.remove(at);
            return Some(self.rec_new(layout));
        }
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::RecRestrict { dst, rec, label: l });
        Some(V::any(dst))
    }

    /// `rec_update`。レコードを先に、値を後に評価する。
    pub(crate) fn rec_update(&mut self, label: &str, record: ExpId, value: ExpId) -> Option<V> {
        let r = self.exp(record, false)?;
        let rec = self.boxed(r);
        let v = self.exp(value, false)?;
        let value = self.boxed(v);
        let l = self.p.label(label);
        if let Some(mut layout) = self.known.get(&rec).cloned()
            && let Some(at) = find_outer(&layout, l)
        {
            layout[at].1 = value;
            return Some(self.rec_new(layout));
        }
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::RecUpdate {
            dst,
            rec,
            label: l,
            value,
        });
        Some(V::any(dst))
    }
}
