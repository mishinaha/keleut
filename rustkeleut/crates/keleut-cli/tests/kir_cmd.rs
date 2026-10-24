//! `keleut kir` の試験。
//!
//! kir crate の fixture(Diktor の書き出した `.kir` と `.kirt` の組)で、`dis` と `asm` が
//! Diktor の出力とバイト単位で一致することと、`conformance/kir-bad/` の試料がどれも 65 で拒否されることを確かめる。
//! 環境変数 `KELEUT_DIKTOR` に Diktor の実行ファイルを与えると、`tables` を `diktor --dump-ir-tables` とも比べる。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn keleut(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-kir-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn dis_と_asm_は_diktor_の出力と一致する() {
    let fx = root().join("crates/kir/tests/fixtures");
    let out = scratch("rt");
    let mut n = 0;
    for e in std::fs::read_dir(&fx).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "kir") {
            continue;
        }
        let bin = std::fs::read(&p).unwrap();
        let text = std::fs::read(p.with_extension("kirt")).unwrap();
        let r = keleut(&["kir", "verify", p.to_str().unwrap()], &out);
        assert!(r.status.success(), "{}: {r:?}", p.display());
        let r = keleut(&["kir", "dis", p.to_str().unwrap()], &out);
        assert!(r.status.success(), "{}: {r:?}", p.display());
        assert!(r.stdout == text, "{}: dis が違います", p.display());
        let t = p.with_extension("kirt");
        let r = keleut(&["kir", "asm", t.to_str().unwrap(), "-o", "a.kir"], &out);
        assert!(r.status.success(), "{}: {r:?}", p.display());
        assert!(
            std::fs::read(out.join("a.kir")).unwrap() == bin,
            "{}: asm が違います",
            p.display()
        );
        n += 1;
    }
    assert!(n >= 10, "fixture が {n} 組しかありません");
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn kir_bad_はどれも_65_で拒否される() {
    let dir = root().join("conformance/kir-bad");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        let is_kir = name.ends_with(".kir") || name.ends_with(".kirt");
        if !is_kir || name == "deep-base.kirt" {
            continue;
        }
        let r = keleut(&["kir", "verify", &name], &dir);
        assert_eq!(r.status.code(), Some(65), "{name}: {r:?}");
        assert!(r.stdout.is_empty(), "{name}");
        let msg = String::from_utf8(r.stderr).unwrap();
        assert!(
            msg.starts_with("keleut: KIR を読み込めません: ")
                || msg.starts_with("keleut: KIR の検証に失敗しました: "),
            "{name}: {msg}"
        );
        n += 1;
    }
    assert!(n >= 40, "試料が {n} 本しかありません");
    let r = keleut(&["kir", "verify", "deep-base.kirt"], &dir);
    assert!(r.status.success(), "{r:?}");
}

#[test]
fn 読めない入力と書けない出力() {
    let out = scratch("io");
    let r = keleut(&["kir", "verify", "nosuch.kir"], &out);
    assert_eq!(r.status.code(), Some(64));
    let fx = root().join("crates/kir/tests/fixtures/ex_loop.kirt");
    let r = keleut(
        &[
            "kir",
            "asm",
            fx.to_str().unwrap(),
            "-o",
            "no/such/dir/a.kir",
        ],
        &out,
    );
    assert_eq!(r.status.code(), Some(74), "{r:?}");
    let r = keleut(
        &[
            "kir",
            "dis",
            fx.to_str().unwrap(),
            "-o",
            "no/such/dir/a.kirt",
        ],
        &out,
    );
    assert_eq!(r.status.code(), Some(74), "{r:?}");
    let r = keleut(&["kir", "dis"], &out);
    assert_eq!(r.status.code(), Some(64));
    std::fs::remove_dir_all(&out).unwrap();
}

/// `deep-base.kirt` の main/total の本体を、深さ `n` の `not` の鎖にしたテキスト(`deep.t` と同じ作り方)。
fn not_chain(n: usize) -> Vec<u8> {
    let base = std::fs::read_to_string(root().join("conformance/kir-bad/deep-base.kirt")).unwrap();
    let mut t = String::new();
    for l in base.lines() {
        if l.contains("(call ntail noyield (global \"main/loop\")") {
            t += "  ";
            t += &"(not ".repeat(n - 1);
            t += "(lit i64 0)";
            t += &")".repeat(n - 1);
            t += ")\n";
        } else {
            t += l;
            t += "\n";
        }
    }
    t.into_bytes()
}

/// `ulimit -v`(KiB)の下で keleut を走らせる。
fn keleut_limited(kib: u64, args: &str, cwd: &Path) -> Output {
    Command::new("sh")
        .arg("-c")
        .arg(format!(
            "ulimit -v {kib} && exec '{}' {args}",
            env!("CARGO_BIN_EXE_keleut")
        ))
        .current_dir(cwd)
        .output()
        .unwrap()
}

#[test]
fn dis_はテキストを流しながら書く() {
    // 深さ 60,000 の正準のテキストは約 3.6 GB になる。全体をメモリに置くと 3 GB の上限で落ちる。
    let out = scratch("stream");
    std::fs::write(out.join("d.kirt"), not_chain(60_000)).unwrap();
    let r = keleut(&["kir", "asm", "d.kirt", "-o", "d.kir"], &out);
    assert!(r.status.success(), "{r:?}");
    let r = keleut_limited(3_000_000, "kir dis -o /dev/null d.kir", &out);
    assert_eq!(r.status.code(), Some(0), "{r:?}");
    let r = keleut_limited(3_000_000, "kir dis d.kir > /dev/null", &out);
    assert_eq!(r.status.code(), Some(0), "{r:?}");
    // 書き込みの途中で場所が尽きたら 74 である。
    let r = keleut(&["kir", "dis", "-o", "/dev/full", "d.kir"], &out);
    assert_eq!(r.status.code(), Some(74), "{r:?}");
    let msg = String::from_utf8(r.stderr).unwrap();
    assert!(
        msg.starts_with("keleut: KIR のテキストを書き出せません: /dev/full: "),
        "{msg}"
    );
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn 小さなスタックでも深い入力で落ちない() {
    // 深さ N_nest の入力は、既定のスタックでは通る。
    // スタックを小さくすると限度が下がり、スタックがあふれる前に入れ子の限度で 65 になる。
    let out = scratch("stack");
    std::fs::write(out.join("d.kirt"), not_chain(100_000)).unwrap();
    let r = keleut(&["kir", "asm", "d.kirt", "-o", "d.kir"], &out);
    assert!(r.status.success(), "{r:?}");
    for st in ["64k", "512k", "1m", "8m"] {
        for f in ["d.kirt", "d.kir"] {
            let r = keleut(&["kir", "verify", "--native-stack", st, f], &out);
            assert_eq!(r.status.code(), Some(65), "{st} {f}: {r:?}");
            let msg = String::from_utf8(r.stderr).unwrap();
            assert!(
                msg.contains("処理系の限度: KIR の読み込みで入れ子が深すぎます"),
                "{msg}"
            );
        }
    }
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn tables_は_diktor_と一致する() {
    let out = scratch("tables");
    let r = keleut(&["kir", "tables"], &out);
    assert!(r.status.success());
    assert_eq!(String::from_utf8_lossy(&r.stdout).lines().count(), 127);
    if let Ok(d) = std::env::var("KELEUT_DIKTOR") {
        let dk = Command::new(d).arg("--dump-ir-tables").output().unwrap();
        assert!(dk.status.success());
        assert!(dk.stdout == r.stdout, "diktor --dump-ir-tables と違います");
    }
    std::fs::remove_dir_all(&out).unwrap();
}
