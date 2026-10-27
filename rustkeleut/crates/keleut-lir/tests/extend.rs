//! 単位を足していく下ろし([`keleut_lir::Extender`])の試験。
//!
//! `crates/kir/tests/fixtures/*.kir` を、全体を 1 回で足す形と、単位を 1 つずつ足す形で下ろす。

use keleut_lir::{Extender, LabelId, Program};
use std::path::PathBuf;

fn fixtures() -> Vec<(String, kir::Program)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kir/tests/fixtures");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "kir"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|p| {
            let k = kir::load(&std::fs::read(&p).unwrap()).unwrap();
            (p.file_stem().unwrap().to_string_lossy().into_owned(), k)
        })
        .collect()
}

/// プログラム `p` の先頭の `n` 個の単位と、その単位の項目だけを残したもの(表は単位の順に並ぶ)。
fn prefix(p: &kir::Program, n: usize) -> kir::Program {
    let keys: Vec<&str> = p.units[..n].iter().map(|u| u.key.as_str()).collect();
    let mine = |s: &str| keys.contains(&kir::unit_of_symbol(s));
    let mut q = p.clone();
    q.units.truncate(n);
    q.types.retain(|t| mine(&t.sym));
    q.effects.retain(|t| mine(&t.sym));
    q.classes.retain(|t| mine(&t.sym));
    q.instances.retain(|t| mine(&t.sym));
    q.globals.retain(|t| mine(&t.sym));
    q.functions.retain(|t| mine(&t.sym));
    q
}

/// 空の状態から全体を 1 回で足すと、[`keleut_lir::lower`] と同じ LIR になる。
#[test]
fn 全体を_1_回で足すと_lower_と同じ() {
    for (name, k) in fixtures() {
        let want = keleut_lir::lower(&k).unwrap();
        let mut lp = Program::empty();
        Extender::new()
            .extend(&k, &mut lp, kir::N_NEST, true)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(keleut_lir::print(&lp), keleut_lir::print(&want), "{name}");
    }
}

/// 単位を 1 つずつ足しても、関数、大域、初期化の関数の数は同じで、全体の検証に通る。
/// 足す前に形の表の後ろに足した形(実行時が作った形)は、その番号のまま残り、後の形はその後ろに並ぶ。
#[test]
fn 単位を_1_つずつ足す() {
    for (name, k) in fixtures() {
        let whole = keleut_lir::lower(&k).unwrap();
        let mut lp = Program::empty();
        let mut ext = Extender::new();
        let mut runtime: Vec<(usize, Vec<LabelId>)> = Vec::new();
        for n in 2..=k.units.len() {
            let q = prefix(&k, n);
            let f0 = lp.funcs.len();
            ext.extend(&q, &mut lp, kir::N_NEST, true)
                .unwrap_or_else(|e| panic!("{name} の {n} 個の単位: {e}"));
            // 足した関数は前の関数の後ろに続き、前の関数は変わらない。
            assert!(lp.funcs.len() > f0, "{name}");
            if !lp.labels.is_empty() {
                // 実行時が作った形の代わりに、どの形とも違う形を足しておく。
                let mut s: Vec<LabelId> = (0..lp.labels.len() as u32).map(LabelId).collect();
                s.push(LabelId(0));
                s.sort();
                if !lp.shapes.contains(&s) {
                    runtime.push((lp.shapes.len(), s.clone()));
                    lp.shapes.push(s);
                }
            }
        }
        keleut_lir::verify(&lp).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(lp.funcs.len(), whole.funcs.len(), "{name}");
        assert_eq!(lp.globals.len(), whole.globals.len(), "{name}");
        assert_eq!(lp.init.len(), whole.init.len(), "{name}");
        assert_eq!(lp.rt_handle.is_some(), whole.rt_handle.is_some(), "{name}");
        for (i, s) in &runtime {
            assert_eq!(&lp.shapes[*i], s, "{name}");
        }
        let mut seen = std::collections::HashSet::new();
        for s in &lp.shapes {
            assert!(seen.insert(s.clone()), "{name}: 形が重なる");
        }
    }
}
