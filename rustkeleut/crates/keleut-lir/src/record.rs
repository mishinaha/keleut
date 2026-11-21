//! レコードのシェイプ(パス 6)。
//!
//! レコードの値は、シェイプとフィールドの並びからなる。
//! シェイプは、ラベルの番号の昇順に並べたラベルの並びである。
//! 同じラベルが 2 つ以上あるときは、外側のものを先に置く。
//! この並べ方を**正準順**(canonical。一意に決まる順序)と呼ぶ。
//! 異なるラベルの順序は型の一致に影響しない(LangSpec §7.2、§18.3)ので、フィールドはラベルの番号の順に並べてよい。
//!
//! KIR 1.0 は型を持たないので、変換器にシェイプが分かるレコードは次の 2 つだけである。
//!
//! - 同じ関数の中で作ったレコード。`rec_empty` と `rec_extend` の連鎖から作ったものと、そこからフィールドの除去や更新で作ったものである。
//! - `closed` のレコードのパターン(タプルのパターン)の照合対象。
//!
//! `open` のレコードのパターンの照合対象は、パターンに書かないフィールドを持つことがあるので、シェイプが分からない。
//! 同じ関数の中で作ったレコードについては、**レイアウト**を記録しておき、フィールドの選択、除去、更新、照合を位置で行う。
//! レイアウトは、ラベルとフィールドの値のレジスタの組を正準順に並べたもので、正準なレイアウトとも呼ぶ。
//! それ以外のレコードには、実行時にシェイプを調べる命令(`rec_extend`、`rec_field`、`rec_restrict`、`rec_update`)を使う。
//!
//! レイアウトを記録するのは、フィールドが [`KNOWN_RECORD_FIELDS`] 個以下のレコードだけである。
//! シェイプの分かるレコードに対するフィールドの除去、更新、拡張は、レイアウトをコピーして `rec_new` を生成するので、変換の処理時間と生成するオペランドの個数が、レイアウトの長さに比例する。
//! レイアウトを無制限に記録すると、同じレコードを更新し続ける連鎖や、`let` で束縛したレコードを拡張し続ける連鎖で、LIR への変換にかかる時間と LIR のサイズが、連鎖の長さの 2 乗に比例する。
//! 上限を超えるレコードは、シェイプの分からないレコードと同じように扱うので、レコードの操作 1 つから生成する命令の量は一定の上限を超えない。

use kir::{Exp, ExpId};

use crate::ir::*;
use crate::lower::{Layout, Low, V};

/// 変換器がレイアウトを記録するレコードの、フィールドの個数の上限。
pub const KNOWN_RECORD_FIELDS: usize = 64;

/// パターンのラベルの並びの各要素について、正準なレイアウトの中での位置(0 始まり)を返す。
///
/// 同じラベルどうしは、パターンに書いた順を保つ。
pub(crate) fn canonical_positions(labels: &[LabelId]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..labels.len()).collect();
    order.sort_by_key(|&i| labels[i]);
    let mut pos = vec![0u32; labels.len()];
    for (p, &i) in order.iter().enumerate() {
        pos[i] = p as u32;
    }
    pos
}

/// レイアウトの中で、ラベル `l` の最も外側のフィールドの位置を返す。
/// `l` のフィールドがなければ `None` を返す。
fn find_outer(layout: &Layout, l: LabelId) -> Option<usize> {
    let at = layout.partition_point(|&(x, _)| x < l);
    (layout.get(at).map(|&(x, _)| x) == Some(l)).then_some(at)
}

impl Low<'_, '_> {
    /// レイアウトからレコードを作る。
    ///
    /// フィールドが [`KNOWN_RECORD_FIELDS`] 個以下のときだけ、レイアウトを記録する。
    /// 空のレイアウトでは、`rec_new` を生成せずに定数の Unit を使う。
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

    /// `rec_extend` の連鎖を変換する。
    /// フィールドの値を外側から順に評価し、最後に連鎖の元のレコードを評価する。
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
        // chain には、連鎖のフィールドが外側から順に並び、その後ろに元のレコードのレイアウトが続く。
        // 安定な整列は同じラベルの中でこの順序を保つので、外側のフィールドが先に来て、正準なレイアウトになる。
        chain.sort_by_key(|&(l, _)| l);
        Some(self.rec_new(chain))
    }

    /// シェイプの分からないレコード `base` に、連鎖のフィールドを内側から順に `rec_extend` で追加する。
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

    /// `rec_select`(フィールドの選択)を変換する。
    /// 同じラベルが複数あれば、最も外側のフィールドを読む。
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

    /// `rec_restrict`(フィールドの除去)を変換する。
    /// 同じラベルが複数あれば、最も外側のフィールドを取り除く。
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

    /// `rec_update`(フィールドの更新)を変換する。
    /// レコードを先に、値を後に評価する。
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
