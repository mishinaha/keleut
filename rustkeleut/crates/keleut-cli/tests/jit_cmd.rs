//! `keleut run --backend jit` の試験(M91。feature `llvm` を付けたときだけ走る)。
//!
//! `tests/fixtures/run/` の各 KIR を `--backend jit --tier jit-only` で走らせ、
//! 標準出力、標準エラー、終了状態が記録(インタプリタの結果)と一致することを確かめる。
//! `--gc-stress` を足した実行も同じ記録と比べる(JIT のスロットへの書き出しと安全点の漏れを捉える)。
#![cfg(feature = "llvm")]

use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> (PathBuf, Vec<String>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension().and_then(|x| x.to_str()) == Some("kir"))
                .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    assert!(names.len() >= 15, "{names:?}");
    (dir, names)
}

fn check(extra: &[&str]) {
    let (dir, names) = fixtures();
    let mut fails = Vec::new();
    for n in &names {
        let mut args: Vec<String> = ["run", "--backend", "jit", "--tier", "jit-only"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        args.extend(extra.iter().map(|s| s.to_string()));
        if n == "deep" {
            args.extend(["--max-depth".to_string(), "100".to_string()]);
        }
        args.push(format!("{n}.kir"));
        let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .current_dir(&dir)
            .args(&args)
            .output()
            .unwrap();
        let want_out = std::fs::read(dir.join(format!("{n}.stdout"))).unwrap();
        let want_err = std::fs::read(dir.join(format!("{n}.stderr"))).unwrap();
        let want_code: i32 = std::fs::read_to_string(dir.join(format!("{n}.code")))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        if r.stdout != want_out || r.stderr != want_err || r.status.code() != Some(want_code) {
            fails.push(format!(
                "{n}: 終了状態 {:?}(記録 {want_code})\n標準エラー:\n{}",
                r.status.code(),
                String::from_utf8_lossy(&r.stderr)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn fixture_の_kir_を_jit_で記録どおりに走らせる() {
    check(&[]);
}

#[test]
fn fixture_の_kir_を_jit_と_gc_stress_で記録どおりに走らせる() {
    check(&["--gc-stress"]);
}

#[test]
fn 段の切り替えの設定は_m92_まで未実装() {
    let (dir, _) = fixtures();
    for t in ["threshold=1", "alternating=1", "mixed=1"] {
        let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .current_dir(&dir)
            .args(["run", "--backend", "jit", "--tier", t, "num.kir"])
            .output()
            .unwrap();
        assert_eq!(r.status.code(), Some(4), "{t}");
    }
}
