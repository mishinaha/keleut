//! 永続的な順序つきの写像である(OCaml の `Map.Make`)。
//!
//! 精緻化の環境は、束縛を足すたびに新しい環境を作り、古い環境もそのまま使い続ける。
//! 写像を丸ごと写すと深い `let` の鎖で 2 乗の手間になるので、
//! 節点を `Rc` で共有する AVL 木にして、足すことと除くことを対数の手間にする。
//! 列挙はキーの昇順で、OCaml の `Map.bindings` と同じ順である。

use std::cmp::Ordering;
use std::rc::Rc;

/// 木の節点である。
struct Node<K, V> {
    key: K,
    val: V,
    left: Tree<K, V>,
    right: Tree<K, V>,
    height: u32,
}

type Tree<K, V> = Option<Rc<Node<K, V>>>;

/// 永続的な写像である。
pub struct PMap<K, V> {
    root: Tree<K, V>,
}

impl<K, V> Clone for PMap<K, V> {
    fn clone(&self) -> Self {
        PMap {
            root: self.root.clone(),
        }
    }
}

impl<K, V> Default for PMap<K, V> {
    fn default() -> Self {
        PMap { root: None }
    }
}

fn height<K, V>(t: &Tree<K, V>) -> u32 {
    t.as_ref().map_or(0, |n| n.height)
}

fn mk<K, V>(key: K, val: V, left: Tree<K, V>, right: Tree<K, V>) -> Tree<K, V> {
    let height = height(&left).max(height(&right)) + 1;
    Some(Rc::new(Node {
        key,
        val,
        left,
        right,
        height,
    }))
}

fn bal<K: Clone, V: Clone>(key: K, val: V, l: Tree<K, V>, r: Tree<K, V>) -> Tree<K, V> {
    let hl = height(&l);
    let hr = height(&r);
    if hl > hr + 1 {
        let ln = l.as_ref().expect("左が高い");
        if height(&ln.left) >= height(&ln.right) {
            mk(
                ln.key.clone(),
                ln.val.clone(),
                ln.left.clone(),
                mk(key, val, ln.right.clone(), r),
            )
        } else {
            let lr = ln.right.as_ref().expect("左の右が高い");
            mk(
                lr.key.clone(),
                lr.val.clone(),
                mk(
                    ln.key.clone(),
                    ln.val.clone(),
                    ln.left.clone(),
                    lr.left.clone(),
                ),
                mk(key, val, lr.right.clone(), r),
            )
        }
    } else if hr > hl + 1 {
        let rn = r.as_ref().expect("右が高い");
        if height(&rn.right) >= height(&rn.left) {
            mk(
                rn.key.clone(),
                rn.val.clone(),
                mk(key, val, l, rn.left.clone()),
                rn.right.clone(),
            )
        } else {
            let rl = rn.left.as_ref().expect("右の左が高い");
            mk(
                rl.key.clone(),
                rl.val.clone(),
                mk(key, val, l, rl.left.clone()),
                mk(
                    rn.key.clone(),
                    rn.val.clone(),
                    rl.right.clone(),
                    rn.right.clone(),
                ),
            )
        }
    } else {
        mk(key, val, l, r)
    }
}

fn add<K: Ord + Clone, V: Clone>(t: &Tree<K, V>, key: K, val: V) -> Tree<K, V> {
    match t {
        None => mk(key, val, None, None),
        Some(n) => match key.cmp(&n.key) {
            Ordering::Equal => mk(key, val, n.left.clone(), n.right.clone()),
            Ordering::Less => bal(
                n.key.clone(),
                n.val.clone(),
                add(&n.left, key, val),
                n.right.clone(),
            ),
            Ordering::Greater => bal(
                n.key.clone(),
                n.val.clone(),
                n.left.clone(),
                add(&n.right, key, val),
            ),
        },
    }
}

fn remove_min<K: Clone, V: Clone>(n: &Node<K, V>) -> (K, V, Tree<K, V>) {
    match &n.left {
        None => (n.key.clone(), n.val.clone(), n.right.clone()),
        Some(l) => {
            let (k, v, l2) = remove_min(l);
            (k, v, bal(n.key.clone(), n.val.clone(), l2, n.right.clone()))
        }
    }
}

fn merge<K: Clone, V: Clone>(l: &Tree<K, V>, r: &Tree<K, V>) -> Tree<K, V> {
    match (l, r) {
        (None, t) | (t, None) => t.clone(),
        (_, Some(rn)) => {
            let (k, v, r2) = remove_min(rn);
            bal(k, v, l.clone(), r2)
        }
    }
}

fn remove<K, V, Q>(t: &Tree<K, V>, key: &Q) -> Tree<K, V>
where
    K: Ord + Clone + std::borrow::Borrow<Q>,
    V: Clone,
    Q: ?Sized + Ord,
{
    match t {
        None => None,
        Some(n) => match key.cmp(n.key.borrow()) {
            Ordering::Equal => merge(&n.left, &n.right),
            Ordering::Less => bal(
                n.key.clone(),
                n.val.clone(),
                remove(&n.left, key),
                n.right.clone(),
            ),
            Ordering::Greater => bal(
                n.key.clone(),
                n.val.clone(),
                n.left.clone(),
                remove(&n.right, key),
            ),
        },
    }
}

impl<K: Ord + Clone, V: Clone> PMap<K, V> {
    /// 空の写像。
    pub fn new() -> Self {
        PMap { root: None }
    }

    /// `key` を `val` に写す写像を足した写像を返す。
    pub fn add(&self, key: K, val: V) -> Self {
        PMap {
            root: add(&self.root, key, val),
        }
    }

    /// `key` を除いた写像を返す。
    pub fn remove<Q: ?Sized + Ord>(&self, key: &Q) -> Self
    where
        K: std::borrow::Borrow<Q>,
    {
        if self.get(key).is_none() {
            return self.clone();
        }
        PMap {
            root: remove(&self.root, key),
        }
    }

    /// `key` の値。
    pub fn get<Q: ?Sized + Ord>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
    {
        let mut t = &self.root;
        while let Some(n) = t {
            match key.cmp(n.key.borrow()) {
                Ordering::Equal => return Some(&n.val),
                Ordering::Less => t = &n.left,
                Ordering::Greater => t = &n.right,
            }
        }
        None
    }

    /// `key` があるか。
    pub fn contains<Q: ?Sized + Ord>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
    {
        self.get(key).is_some()
    }

    /// キーの昇順の列。
    pub fn keys(&self) -> Vec<K> {
        let mut out = Vec::new();
        let mut stack: Vec<&Rc<Node<K, V>>> = Vec::new();
        let mut t = &self.root;
        loop {
            while let Some(n) = t {
                stack.push(n);
                t = &n.left;
            }
            match stack.pop() {
                None => break,
                Some(n) => {
                    out.push(n.key.clone());
                    t = &n.right;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 足して除いて引ける() {
        let mut m = PMap::new();
        for i in 0..1000u32 {
            m = m.add((i * 7919) % 1000, i);
        }
        let old = m.clone();
        for i in 0..500u32 {
            m = m.remove(&i);
        }
        assert_eq!(m.keys(), (500..1000).collect::<Vec<_>>());
        assert_eq!(old.keys().len(), 1000);
        assert_eq!(m.get(&700), old.get(&700));
        assert!(m.get(&3).is_none());
        assert!(old.contains(&3));
    }
}
