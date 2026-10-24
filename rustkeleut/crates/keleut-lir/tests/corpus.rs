//! KIR の fixture とコーパスの全件を下ろして検証する。
//!
//! - `crates/kir/tests/fixtures/*.kir` は常に確かめる。
//! - 環境変数 `KELEUT_DIKTOR` に Diktor の実行形を与えると、`conformance/corpus/` の各 `.kel` を
//!   `diktor --emit-ir` で KIR にし(旗は `index.tsv` のもの。`?` なら旗なしで試し、型検査の誤りなら
//!   `--no-prelude` で試し直す)、`diktor` が 0 で終わったものをすべて下ろして検証する。
//!   与えなければ、この部分は何もしない。
//! - 環境変数 `KIR_CORPUS_DIR` にディレクトリを与えると、その中の `.kir` をすべて下ろして検証する。

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 下ろして検証し、KIR の関数がすべて同じ添字にあることを確かめる。
fn check(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let k = kir::load(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let p = keleut_lir::lower(&k).map_err(|e| format!("{}: {e}", path.display()))?;
    if p.kir_funcs as usize != k.functions.len() {
        return Err(format!("{}: 関数の数が合わない", path.display()));
    }
    for (i, f) in k.functions.iter().enumerate() {
        if p.funcs[i].sym != f.sym {
            return Err(format!("{}: 関数 {i} の記号が合わない", path.display()));
        }
    }
    if keleut_lir::print(&p).is_empty() {
        return Err(format!("{}: 印字が空", path.display()));
    }
    Ok(())
}

#[test]
fn fixture_をすべて下ろす() {
    let dir = root().join("../kir/tests/fixtures");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) == Some("kir") {
            check(&path, &std::fs::read(&path).unwrap()).unwrap();
            n += 1;
        }
    }
    assert_eq!(n, 16);
}

#[test]
fn kir_corpus_dir_の_kir_をすべて下ろす() {
    let Ok(dir) = std::env::var("KIR_CORPUS_DIR") else {
        return;
    };
    let mut fails = Vec::new();
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) == Some("kir") {
            n += 1;
            if let Err(m) = check(&path, &std::fs::read(&path).unwrap()) {
                fails.push(m);
            }
        }
    }
    eprintln!("KIR_CORPUS_DIR: {n} 本");
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn コーパスの全件を_diktor_で書き出して下ろす() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    let corpus = root().join("../../conformance/corpus");
    let index = std::fs::read_to_string(corpus.join("index.tsv")).unwrap();
    let tmp = std::env::temp_dir().join(format!("keleut-lir-corpus-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let rows: Vec<(String, String)> = index
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let path = it.next().unwrap_or("").to_string();
            let _src = it.next();
            (path, it.next().unwrap_or("").to_string())
        })
        .collect();
    let (mut pass, mut skip) = (0, 0);
    let mut fails = Vec::new();
    for (i, (path, flags)) in rows.iter().enumerate() {
        let kel = corpus.join(path);
        let out = tmp.join(format!("{i}.kir"));
        let tries: Vec<Vec<&str>> = match flags.as_str() {
            "?" => vec![vec![], vec!["--no-prelude"]],
            "" => vec![vec![]],
            f => vec![f.split_whitespace().collect()],
        };
        let mut code = None;
        for fl in tries {
            let st = Command::new(&diktor)
                .args(&fl)
                .arg("--emit-ir")
                .arg(&out)
                .arg(&kel)
                .current_dir(kel.parent().unwrap())
                .output()
                .unwrap();
            code = st.status.code();
            if code != Some(1) {
                break;
            }
        }
        if code != Some(0) {
            skip += 1;
            continue;
        }
        match check(&kel, &std::fs::read(&out).unwrap()) {
            Ok(()) => pass += 1,
            Err(m) => fails.push(m),
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "コーパス: {} 本のうち、下ろした {pass} 本、対象外 {skip} 本、失敗 {} 本",
        rows.len(),
        fails.len()
    );
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(pass > 0);
}
