//! `keleut check` の入れ子の上限と import のテスト。
//!
//! 深い入力でも、プロセスがスタックオーバーフローで異常終了しないことを確かめる。
//! `--native-stack` で小さいスタックを指定したときは、深さの上限をスタックに収まる値まで下げることと、
//! 上限を超えた入力に対して `処理系の限度: 構文解析で再帰が深すぎます` の診断を書き、終了コード 2 で終了することを確かめる。

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

/// `diktor/test/run_exit.t` と同じく、`1 + (...)` を 2 万段入れ子にした式のソースを返す。
fn deep_source() -> String {
    format!("let x = {}1{}\n", "1 + (".repeat(20000), ")".repeat(20000))
}

#[test]
fn 小さいスタックでも深い入力で落ちない() {
    let d = scratch("deep");
    let p = d.join("deep.kel");
    std::fs::write(&p, deep_source()).unwrap();
    let p = p.to_str().unwrap();
    for stack in ["4m", "16m"] {
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

/// 文を 2 万個並べたブロックのソースを返す。
///
/// 文は `let` の連鎖に脱糖されるので、ブロックを閉じる 1 回の還元で深さ 2 万の木になる。
fn long_block_source() -> String {
    let mut s = String::from("let r = {\n");
    for i in 0..20000 {
        s.push_str(&format!("  let x{i} = {i}\n"));
    }
    s.push_str("  0\n}\n");
    s
}

#[test]
fn 限度を超えた長いブロックを捨てても落ちない() {
    let d = scratch("block");
    let p = d.join("block.kel");
    std::fs::write(&p, long_block_source()).unwrap();
    let p = p.to_str().unwrap();
    for stack in ["4m", "16m"] {
        let r = keleut(&["check", "--native-stack", stack, p]);
        let err = String::from_utf8_lossy(&r.stderr);
        assert_eq!(r.status.code(), Some(2), "{stack}: {err}");
        assert!(
            err.contains("処理系の限度: 構文解析で再帰が深すぎます"),
            "{stack}: {err}"
        );
    }
    let r = keleut(&["check", p]);
    assert_eq!(r.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&r.stdout), "r : Int32\n");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn 長い_import_の鎖も小さいスタックで型検査する() {
    // f0 が f1 を、f1 が f2 を、というように検索パスで import していく、3,000 段の import の連鎖を作る。
    let d = scratch("chain");
    let n = 3000;
    for i in 0..n {
        let src = if i + 1 < n {
            format!(
                "from \"f{}\" import v{}\npub let v{i}: Int32 = v{} + 1\n",
                i + 1,
                i + 1,
                i + 1
            )
        } else {
            format!("pub let v{i}: Int32 = 0\n")
        };
        std::fs::write(d.join(format!("f{i}.kel")), src).unwrap();
    }
    let m = d.join("m.kel");
    std::fs::write(&m, "from \"f0\" import v0\nlet r = v0\n").unwrap();
    let dir = d.to_str().unwrap();
    let r = keleut(&[
        "check",
        "--no-prelude",
        "--native-stack",
        "4m",
        "--import-path",
        dir,
        m.to_str().unwrap(),
    ]);
    assert_eq!(
        (
            r.status.code(),
            String::from_utf8_lossy(&r.stdout).into_owned()
        ),
        (Some(0), "r : Int32\n".to_string()),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
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
    let r = keleut(&["check", "--no-prelude", "--native-stack", "4m", p]);
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

/// import 先を検索パスで探すことと、import 先を開けないときに終了コード 64 で終了することを確かめる。
#[test]
fn import_の検索パスと開けない_import_先() {
    let d = scratch("import");
    std::fs::create_dir_all(d.join("sp/std")).unwrap();
    std::fs::write(d.join("sp/std/list.kel"), "pub let x: Int32 = 1\n").unwrap();
    std::fs::write(d.join("m.kel"), "from \"std/list\" import x\nlet y = x\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(args)
            .current_dir(&d)
            .output()
            .unwrap()
    };
    let r = run(&["check", "--import-path", "sp", "m.kel"]);
    assert_eq!(String::from_utf8_lossy(&r.stdout), "y : Int32\n");
    assert_eq!(r.status.code(), Some(0));
    let r = run(&["check", "m.kel"]);
    assert_eq!(
        String::from_utf8_lossy(&r.stdout),
        "! m.kel:1:1: import エラー: import 先が検索パスに見つかりません: std/list\n"
    );
    assert_eq!(r.status.code(), Some(1));
    std::fs::create_dir_all(d.join("dir.kel")).unwrap();
    std::fs::write(d.join("dd.kel"), "from \"./dir\" import x\n").unwrap();
    let r = run(&["check", "dd.kel"]);
    assert_eq!(
        String::from_utf8_lossy(&r.stderr),
        "keleut: ファイルを開けません: dir.kel: Is a directory\n"
    );
    assert_eq!(r.status.code(), Some(64));
    std::fs::remove_dir_all(&d).unwrap();
}
