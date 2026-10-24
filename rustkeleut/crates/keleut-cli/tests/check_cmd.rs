//! `keleut check` の限度の試験。
//!
//! 計画 261024-1 の 6.11 節のとおり、深い入力でもプロセスをスタックあふれで落とさないことを確かめる。
//! `--native-stack` で小さいスタックを与えたときは、深さの限度をスタックに収まるまで下げ、
//! 構文解析の限度の診断(終了状態 2)で止まる。

use std::path::PathBuf;
use std::process::{Command, Output};

fn keleut(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-check-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// diktor/test/run_exit.t の 2 万段の `1 + (...)` の式である。
fn deep_source() -> String {
    format!("let x = {}1{}\n", "1 + (".repeat(20000), ")".repeat(20000))
}

#[test]
fn 小さいスタックでも深い入力で落ちない() {
    let d = scratch("deep");
    let p = d.join("deep.kel");
    std::fs::write(&p, deep_source()).unwrap();
    let p = p.to_str().unwrap();
    for stack in ["512k", "1m", "4m"] {
        for extra in [&[][..], &["--no-prelude"][..]] {
            let mut args = vec!["check", "--native-stack", stack];
            args.extend_from_slice(extra);
            args.push(p);
            let r = keleut(&args);
            let err = String::from_utf8_lossy(&r.stderr);
            assert_eq!(r.status.code(), Some(2), "{args:?}: {err}");
            assert!(
                err.contains("処理系の限度: 構文解析で再帰が深すぎます"),
                "{args:?}: {err}"
            );
        }
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn 余白に満たないスタックは使い方の誤りである() {
    let d = scratch("small");
    let p = d.join("a.kel");
    std::fs::write(&p, "let x = 1\n").unwrap();
    let p = p.to_str().unwrap();
    let r = keleut(&["check", "--no-prelude", "--native-stack", "256k", p]);
    assert_eq!(r.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&r.stderr).contains("--native-stack is too small"));
    let r = keleut(&["check", "--no-prelude", "--max-nest", "4000000000", p]);
    assert_eq!(r.status.code(), Some(64));
    let r = keleut(&["check", "--no-prelude", "--native-stack", "512k", p]);
    assert_eq!(r.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&r.stdout), "x : Int32\n");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn 深いヴァリアントのパターンの網羅性の検査() {
    let d = scratch("pat");
    let p = d.join("pat.kel");
    let n = 3000;
    let src = format!(
        "let v = {}1{}\nlet f = v match {{ case {}x{} => x }}\n",
        "#A(".repeat(n),
        ")".repeat(n),
        "#A(".repeat(n),
        ")".repeat(n)
    );
    std::fs::write(&p, src).unwrap();
    let r = keleut(&["check", "--no-prelude", p.to_str().unwrap()]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(String::from_utf8_lossy(&r.stdout).ends_with("f : Int32\n"));
    std::fs::remove_dir_all(&d).unwrap();
}
