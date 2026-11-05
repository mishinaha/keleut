//! 形の表を、Rust で書いた素朴な模型と乱数の操作列で比べる。
//!
//! 模型は形を昇順のラベルの列で持ち、同じ列に同じ番号を振る。
//! 小さな容量から始めて、表を写して広げる経路も通す。

use std::collections::HashMap;

use keleut_rtcore::{ShapeId, ShapeTable};

struct Model {
    shapes: Vec<Vec<u32>>,
    ids: HashMap<Vec<u32>, u32>,
}

impl Model {
    fn intern(&mut self, l: Vec<u32>) -> u32 {
        if let Some(&id) = self.ids.get(&l) {
            return id;
        }
        let id = self.shapes.len() as u32;
        self.shapes.push(l.clone());
        self.ids.insert(l, id);
        id
    }
}

#[test]
fn shapes_match_model() {
    let mut t = ShapeTable::with_capacity(1, 0).unwrap();
    let mut m = Model {
        shapes: vec![vec![]],
        ids: HashMap::from([(vec![], 0)]),
    };
    let mut st: u64 = 0x9E3779B97F4A7C15;
    let mut rnd = move |n: u64| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st % n
    };
    let mut pos = Vec::new();
    for _ in 0..200_000 {
        let n = m.shapes.len() as u64;
        let id = rnd(n) as u32;
        let label = rnd(12) as u32;
        match rnd(4) {
            0 => {
                // 書いた順のラベル(重なりを含む)から作る。
                let k = rnd(8) as usize;
                let written: Vec<u32> = (0..k).map(|_| rnd(12) as u32).collect();
                let got = t.intern(&written, &mut pos).unwrap();
                let mut sorted = written.clone();
                sorted.sort();
                assert_eq!(got.0, m.intern(sorted.clone()));
                // 書いた i 番目の位置は、同じラベルの中で書いた順(先が外側)になる。
                for (i, &l) in written.iter().enumerate() {
                    let before = written[..i].iter().filter(|&&x| x == l).count();
                    let lb = sorted.partition_point(|&x| x < l);
                    assert_eq!(pos[i] as usize, lb + before);
                }
            }
            1 => {
                let (got, p) = t.extend(ShapeId(id), label).unwrap();
                let mut l = m.shapes[id as usize].clone();
                let lb = l.partition_point(|&x| x < label);
                l.insert(lb, label);
                assert_eq!(p as usize, lb);
                assert_eq!(got.0, m.intern(l));
            }
            2 => {
                let got = t.restrict(ShapeId(id), label).unwrap();
                let l = &m.shapes[id as usize];
                match l.iter().position(|&x| x == label) {
                    None => assert_eq!(got, None),
                    Some(i) => {
                        let mut l = l.clone();
                        l.remove(i);
                        let (gid, p) = got.expect("restrict");
                        assert_eq!(p as usize, i);
                        assert_eq!(gid.0, m.intern(l));
                    }
                }
            }
            _ => {
                let l = &m.shapes[id as usize];
                assert_eq!(t.labels(ShapeId(id)), &l[..]);
                assert_eq!(
                    t.find(ShapeId(id), label),
                    l.iter().position(|&x| x == label).map(|i| i as u32)
                );
            }
        }
        assert_eq!(t.count() as usize, m.shapes.len());
    }
    assert!(m.shapes.len() > 1000);
    for (id, l) in m.shapes.iter().enumerate() {
        assert_eq!(t.labels(ShapeId(id as u32)), &l[..]);
    }
}
