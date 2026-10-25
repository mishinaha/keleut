//! `keleut run --gc-stress` の試験。
//!
//! `tests/fixtures/run/` の各 KIR を、確保のたびに回収する旗を付けて走らせ、
//! 標準出力、標準エラー、終了状態が記録(旗なしの結果と同じ)と一致することを確かめる。
//! 根の規律に漏れがあれば、回収が生きている箱を空きにし、結果が変わるか処理系が落ちる。

use std::path::PathBuf;
use std::process::Command;

#[test]
fn fixture_の_kir_を_gc_stress_でも記録どおりに走らせる() {
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
    let mut fails = Vec::new();
    for n in &names {
        let mut args = vec!["run".to_string(), "--gc-stress".to_string()];
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
