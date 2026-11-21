//! 型検査の表に使う高速なハッシュ関数と、ノードの番号で引く表、書き換えを取り消せる表である。
//! ハッシュ関数の計算は rustc の FxHash と同じである。
//!
//! 型検査の表のキーは、名前の番号、セルの添字、ノードの番号、短い文字列である。
//! 標準の `RandomState`(SipHash)は、敵対的な入力に耐えるように作られているので遅い。
//! 型検査の入力はユーザ自身のプログラムなので、その耐性は必要ない。
//! また、型検査の結果は表を走査する順によらない(走査する表は、登録の順を別の配列に持つ)。
//! そのため、決定的で高速なハッシュ関数を使う。

use crate::ast::NodeId;
use std::hash::{BuildHasherDefault, Hasher};

/// FxHash と同じ計算をするハッシュ関数である。
#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut b = bytes;
        while b.len() >= 8 {
            self.add(u64::from_le_bytes(b[..8].try_into().expect("8 バイト")));
            b = &b[8..];
        }
        if b.len() >= 4 {
            self.add(u32::from_le_bytes(b[..4].try_into().expect("4 バイト")) as u64);
            b = &b[4..];
        }
        for &x in b {
            self.add(x as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.add(i as u32 as u64);
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// FxHash を使う `HashMap` である。作るときは `HashMap::default()` を使う。
pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
/// FxHash を使う `HashSet` である。
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

/// 配列(`Vec`)の書き換えを取り消すための変更ログである。
/// REPL が入力を取り消すときに使う([`super::Tc::snapshot`])。
///
/// [`Trail::begin`] でマークを置く。
/// その後、マークの時点の長さより前にある項目を書き換えるときは、書き換える前に [`Trail::record`] で元の値を記録する。
/// [`Trail::rollback`] は、記録した値を逆順に書き戻し、配列をマークの時点の長さに切り詰める。
/// マークは入れ子にできる。
/// [`Trail::commit`] は最も内側のマークを外すが、外側のマークが残っていれば変更ログは残す。
/// 処理時間は、マークの後に書き換えた項目と追加した項目の個数に比例し、配列の長さにはよらない。
#[derive(Clone, Debug)]
pub struct Trail<T> {
    /// マークごとの(配列の長さ, 変更ログの長さ)。
    marks: Vec<(usize, usize)>,
    undo: Vec<(usize, T)>,
}

impl<T> Default for Trail<T> {
    fn default() -> Self {
        Trail {
            marks: Vec::new(),
            undo: Vec::new(),
        }
    }
}

impl<T: Clone> Trail<T> {
    /// 配列 `v` の `i` 番目の項目を書き換える前に呼ぶ。
    /// 最も内側のマークの時点の長さより前にある項目なら、元の値を記録する。
    #[inline]
    pub fn record(&mut self, v: &[T], i: usize) {
        if let Some(&(len, _)) = self.marks.last()
            && i < len
        {
            self.undo.push((i, v[i].clone()));
        }
    }

    /// 長さ `len` の配列にマークを置く。
    pub fn begin(&mut self, len: usize) {
        self.marks.push((len, self.undo.len()));
    }

    /// 最も内側のマークを外す。マークがなくなれば変更ログを捨てる。
    pub fn commit(&mut self) {
        self.marks.pop();
        if self.marks.is_empty() {
            self.undo.clear();
        }
    }

    /// 配列 `v` を最も内側のマークの時点に戻し、そのマークを外す。
    pub fn rollback(&mut self, v: &mut Vec<T>) {
        let Some((len, u)) = self.marks.pop() else {
            return;
        };
        while self.undo.len() > u {
            let (i, x) = self.undo.pop().expect("記録");
            if i < v.len() {
                v[i] = x;
            }
        }
        v.truncate(len);
    }
}

/// ノードの番号で引く表である。
/// ノードの番号は、構文解析のコンテキスト 1 つの中で 0 から連続して振られるので、番号を添字とする配列で持つ。
///
/// 書き換えを記録して、マークの時点に戻せる([`Trail`]、[`NodeMap::begin`])。
#[derive(Clone, Debug)]
pub struct NodeMap<T> {
    v: Vec<Option<T>>,
    trail: Trail<Option<T>>,
}

impl<T> Default for NodeMap<T> {
    fn default() -> Self {
        NodeMap {
            v: Vec::new(),
            trail: Trail::default(),
        }
    }
}

impl<T: Clone> NodeMap<T> {
    /// 番号 `id` に値を置く。前の値は捨てる。
    #[inline]
    pub fn insert(&mut self, id: NodeId, x: T) {
        let i = id as usize;
        if i >= self.v.len() {
            self.v.resize_with(i + 1, || None);
        }
        self.trail.record(&self.v, i);
        self.v[i] = Some(x);
    }

    /// 変更ログにマークを置く。
    pub fn begin(&mut self) {
        self.trail.begin(self.v.len());
    }

    /// 最も内側のマークを外す。
    pub fn commit(&mut self) {
        self.trail.commit();
    }

    /// 最も内側のマークの時点に戻し、そのマークを外す。
    ///
    /// 配列は切り詰めず、マークの後に拡張した部分を空に戻す。
    /// ノードの番号は入力をまたいで増え続ける。
    /// 切り詰めると、次の入力が配列を現在の番号まで拡張し直すことになり、処理時間がそれまでに振った番号の個数に比例してしまう。
    pub fn rollback(&mut self) {
        let Some(&(len, _)) = self.trail.marks.last() else {
            return;
        };
        let n = self.v.len();
        self.trail.rollback(&mut self.v);
        self.v.resize_with(n, || None);
        for x in &mut self.v[len..] {
            *x = None;
        }
    }
}

impl<T> NodeMap<T> {
    /// 番号 `id` の値を返す。
    #[inline]
    pub fn get(&self, id: &NodeId) -> Option<&T> {
        self.v.get(*id as usize).and_then(|x| x.as_ref())
    }
}

/// 書き換えを記録して、マークの時点に戻せる `HashMap` である。
/// REPL が入力を取り消すときに使う([`super::Tc::snapshot`])。
///
/// 読むときは `Deref` を通して `HashMap` として読める。
/// 書き換えは [`JMap::insert`]、[`JMap::remove`]、[`JMap::get_mut`]、[`JMap::get_or_insert_with`] だけで行う。
/// マーク([`JMap::begin`])がある間は、書き換える前の値を変更ログに記録する。
/// [`JMap::rollback`] は、記録した値を逆順に書き戻す。
/// マークは入れ子にできる。
/// [`JMap::commit`] は最も内側のマークを外すが、外側のマークが残っていれば変更ログは残す。
/// 処理時間は、マークの後に書き換えた項目の個数に比例し、表のサイズにはよらない。
#[derive(Clone, Debug)]
pub struct JMap<K, V> {
    m: HashMap<K, V>,
    /// マークごとの変更ログの長さ。
    marks: Vec<usize>,
    /// (キー, 書き換える前の値)の並び。キーがなかったときの値は `None` である。
    undo: Vec<(K, Option<V>)>,
}

impl<K, V> Default for JMap<K, V> {
    fn default() -> Self {
        JMap {
            m: HashMap::default(),
            marks: Vec::new(),
            undo: Vec::new(),
        }
    }
}

impl<K, V> std::ops::Deref for JMap<K, V> {
    type Target = HashMap<K, V>;
    #[inline]
    fn deref(&self) -> &HashMap<K, V> {
        &self.m
    }
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> JMap<K, V> {
    /// `k` に `v` を置き、前の値を返す。
    #[inline]
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        if self.marks.is_empty() {
            return self.m.insert(k, v);
        }
        let old = self.m.insert(k.clone(), v);
        self.undo.push((k, old.clone()));
        old
    }

    /// `k` を削除し、前の値を返す。
    pub fn remove(&mut self, k: &K) -> Option<V> {
        let (k, old) = self.m.remove_entry(k)?;
        if !self.marks.is_empty() {
            self.undo.push((k, Some(old.clone())));
        }
        Some(old)
    }

    /// `k` の値を書き換えるために可変で借用する。
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        if !self.marks.is_empty()
            && let Some(old) = self.m.get(k)
        {
            self.undo.push((k.clone(), Some(old.clone())));
        }
        self.m.get_mut(k)
    }

    /// `k` の値を書き換えるために可変で借用する。
    /// 値がなければ `f` の値を置く(`entry(k).or_insert_with(f)` と同じ)。
    pub fn get_or_insert_with(&mut self, k: K, f: impl FnOnce() -> V) -> &mut V {
        if !self.marks.is_empty() {
            let old = self.m.get(&k).cloned();
            self.undo.push((k.clone(), old));
        }
        self.m.entry(k).or_insert_with(f)
    }

    /// 変更ログにマークを置く。
    pub fn begin(&mut self) {
        self.marks.push(self.undo.len());
    }

    /// 最も内側のマークを外す。マークがなくなれば変更ログを捨てる。
    pub fn commit(&mut self) {
        self.marks.pop();
        if self.marks.is_empty() {
            self.undo.clear();
        }
    }

    /// 最も内側のマークの時点に戻し、そのマークを外す。
    pub fn rollback(&mut self) {
        let Some(u) = self.marks.pop() else {
            return;
        };
        while self.undo.len() > u {
            let (k, old) = self.undo.pop().expect("記録");
            match old {
                Some(v) => {
                    self.m.insert(k, v);
                }
                None => {
                    self.m.remove(&k);
                }
            }
        }
    }
}

/// 書き換えを記録して、マークの時点に戻せる `HashSet` である。規則は [`JMap`] と同じである。
#[derive(Clone, Debug)]
pub struct JSet<K> {
    s: HashSet<K>,
    marks: Vec<usize>,
    /// マークがある間に追加したキーの並び。すでに集合にあったキーは記録しない。
    undo: Vec<K>,
}

impl<K> Default for JSet<K> {
    fn default() -> Self {
        JSet {
            s: HashSet::default(),
            marks: Vec::new(),
            undo: Vec::new(),
        }
    }
}

impl<K> std::ops::Deref for JSet<K> {
    type Target = HashSet<K>;
    #[inline]
    fn deref(&self) -> &HashSet<K> {
        &self.s
    }
}

impl<K: std::hash::Hash + Eq + Clone> JSet<K> {
    /// `k` を追加する。新しく追加したなら `true` を返す。
    #[inline]
    pub fn insert(&mut self, k: K) -> bool {
        if self.marks.is_empty() {
            return self.s.insert(k);
        }
        if self.s.contains(&k) {
            return false;
        }
        self.undo.push(k.clone());
        self.s.insert(k)
    }

    /// 変更ログにマークを置く。
    pub fn begin(&mut self) {
        self.marks.push(self.undo.len());
    }

    /// 最も内側のマークを外す。マークがなくなれば変更ログを捨てる。
    pub fn commit(&mut self) {
        self.marks.pop();
        if self.marks.is_empty() {
            self.undo.clear();
        }
    }

    /// 最も内側のマークの時点に戻し、そのマークを外す。
    pub fn rollback(&mut self) {
        let Some(u) = self.marks.pop() else {
            return;
        };
        while self.undo.len() > u {
            let k = self.undo.pop().expect("記録");
            self.s.remove(&k);
        }
    }
}
