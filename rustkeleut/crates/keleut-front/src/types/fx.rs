//! 型検査の表に使う速いハッシュ(rustc の FxHash と同じ計算)と、ノードの番号で引く表である。
//!
//! 型検査の表の鍵は、名前の番号、セルの添字、ノードの番号、短い文字列である。
//! 標準の `RandomState`(SipHash)は、敵対的な入力への耐性のために遅い。
//! 型検査の入力は利用者のプログラムで、型検査の結果は表の走査の順に依らない(走査する表は登録の順の列を別に持つ)ので、
//! 決定的で速いハッシュを使う。

use crate::ast::NodeId;
use std::hash::{BuildHasherDefault, Hasher};

/// FxHash の計算をするハッシュ関数である。
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

/// ノードの番号で引く表である(ノードの番号は 1 つの構文解析の文脈で密に振られる)。
#[derive(Clone, Debug)]
pub struct NodeMap<T> {
    v: Vec<Option<T>>,
}

impl<T> Default for NodeMap<T> {
    fn default() -> Self {
        NodeMap { v: Vec::new() }
    }
}

impl<T> NodeMap<T> {
    /// 番号 `id` の値を置く(前の値は捨てる)。
    #[inline]
    pub fn insert(&mut self, id: NodeId, x: T) {
        let i = id as usize;
        if i >= self.v.len() {
            self.v.resize_with(i + 1, || None);
        }
        self.v[i] = Some(x);
    }

    /// 番号 `id` の値。
    #[inline]
    pub fn get(&self, id: &NodeId) -> Option<&T> {
        self.v.get(*id as usize).and_then(|x| x.as_ref())
    }
}
