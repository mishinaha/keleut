//! Diktor の書き出した `.kir` と `.kirt` の組で、読み書きの往復を確かめる。
//!
//! `tests/fixtures/` の組は `diktor --emit-ir` と `--emit-ir-text` の出力である(作り方は `tests/fixtures/README.md`)。
//! 環境変数 `KIR_CORPUS_DIR` を与えると、そのディレクトリの組もすべて確かめる。

mod common;

#[test]
fn fixtures_roundtrip() {
    let pairs = common::pairs(&common::fixtures());
    assert!(pairs.len() >= 8, "fixtures が足りません");
    let mut bad = Vec::new();
    for (name, kir, kirt) in &pairs {
        if let Err(e) = common::roundtrip(kir, kirt) {
            bad.push(format!("{name}: {e}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn corpus_roundtrip() {
    let Ok(dir) = std::env::var("KIR_CORPUS_DIR") else {
        return;
    };
    let pairs = common::pairs(std::path::Path::new(&dir));
    let mut bad = Vec::new();
    for (name, kir, kirt) in &pairs {
        if let Err(e) = common::roundtrip(kir, kirt) {
            bad.push(format!("{name}: {e}"));
        }
    }
    eprintln!("corpus: {} 組, 失敗 {}", pairs.len(), bad.len());
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
