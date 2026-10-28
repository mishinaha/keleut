//! `keleut compat-diktor` の試験(計画 261024-1 の 6.2 節、8.1 節、M101)。
//!
//! Diktor の旗を Diktor と同じに解釈し、対応する keleut のサブコマンドと同じ出力と終了状態を返すことを確かめる。
//! Diktor を呼ばないので、Diktor の無い環境でも走る。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-compat-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn keleut_in(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args(args)
        .current_dir(dir)
        .env("KELEUT_DIKTOR", dir.join("no-such-diktor"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin).unwrap();
    c.wait_with_output().unwrap()
}

fn same(a: &Output, b: &Output) -> bool {
    a.status.code() == b.status.code() && a.stdout == b.stdout && a.stderr == b.stderr
}

#[test]
fn diktor_の旗を対応するサブコマンドに写す() {
    let d = scratch("modes");
    std::fs::write(d.join("a.kel"), "let x = 1 + 2\necholn(show(x))\n").unwrap();
    std::fs::write(
        d.join("w.kel"),
        "let g(b: Boolean): Int32 = b match { case true => 1 }\necholn(show(g(true)))\n",
    )
    .unwrap();
    std::fs::write(d.join("p.kel"), "let y = 5\n").unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    let cases: &[(&[&str], &[&str])] = &[
        // 実行。組の選択肢(後段の旗)を Diktor の旗と混ぜて並べてよい。
        (
            &["compat-diktor", "--backend", "interp", "a.kel"],
            &["run", "--backend", "interp", "a.kel"],
        ),
        (
            &[
                "compat-diktor",
                "--frontend",
                "rust",
                "--import-path",
                "lib",
                "--backend",
                "interp",
                "--tier",
                "interp-only",
                "a.kel",
            ],
            &[
                "run",
                "--frontend",
                "rust",
                "--backend",
                "interp",
                "--tier",
                "interp-only",
                "--import-path",
                "lib",
                "a.kel",
            ],
        ),
        // 網羅漏れの警告は実行の前に出る。`--strict-exhaustive` だけの呼び出しも実行で、警告が残れば 1 である。
        (
            &["compat-diktor", "--backend", "interp", "w.kel"],
            &["run", "--backend", "interp", "w.kel"],
        ),
        (
            &["compat-diktor", "--strict-exhaustive", "w.kel"],
            &["run", "--strict-exhaustive", "w.kel"],
        ),
        // 型検査とダンプ。モードは後に書いたものが効き、後段の旗は使わない。
        (
            &[
                "compat-diktor",
                "--dump-ast",
                "--type-check",
                "--backend",
                "wasm",
                "a.kel",
            ],
            &["check", "a.kel"],
        ),
        (
            &[
                "compat-diktor",
                "--type-check",
                "--prelude",
                "p.kel",
                "a.kel",
            ],
            &["check", "--prelude", "p.kel", "a.kel"],
        ),
        (
            &[
                "compat-diktor",
                "--type-check",
                "--dump-tokens",
                "--no-prelude",
                "a.kel",
            ],
            &["dump", "tokens", "a.kel"],
        ),
        (
            &["compat-diktor", "--dump-ast", "a.kel"],
            &["dump", "ast", "a.kel"],
        ),
    ];
    for (compat, direct) in cases {
        let a = keleut_in(&d, compat, b"");
        let b = keleut_in(&d, direct, b"");
        assert!(same(&a, &b), "{compat:?} と {direct:?}:\n{a:?}\n{b:?}");
    }
    let r = keleut_in(&d, &["compat-diktor", "--backend", "interp", "a.kel"], b"");
    assert_eq!(
        (r.status.code(), r.stdout.as_slice()),
        (Some(0), &b"3\n"[..])
    );
    let r = keleut_in(&d, &["compat-diktor", "--strict-exhaustive", "w.kel"], b"");
    assert_eq!(r.status.code(), Some(1), "{r:?}");
    // `--repl` は `keleut repl` に移り、標準入力から読む。
    let input = b"let z = 40 + 2\nz\n";
    let a = keleut_in(
        &d,
        &["compat-diktor", "--tier", "interp-only", "--repl"],
        input,
    );
    let b = keleut_in(&d, &["repl"], input);
    assert!(same(&a, &b), "{a:?}\n{b:?}");
    assert_eq!(a.status.code(), Some(0));
    // `--frontend diktor` は `keleut run` と同じく Diktor を呼ぶ(ここでは呼べずに 64)。
    let r = keleut_in(&d, &["compat-diktor", "--frontend", "diktor", "a.kel"], b"");
    assert_eq!(r.status.code(), Some(64), "{r:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 使い方の誤りは_64_で終わる() {
    let d = scratch("usage");
    std::fs::write(d.join("a.kel"), "echoln(\"x\")\n").unwrap();
    for args in [
        &["compat-diktor"][..],
        &["compat-diktor", "--no-prelude"],
        &["compat-diktor", "--type-check"],
        &["compat-diktor", "--repl", "a.kel"],
        &["compat-diktor", "--typo", "a.kel"],
        &["compat-diktor", "a.kel", "--prelude"],
        &["compat-diktor", "--emit-ir", "o.kir", "a.kel"],
        &["compat-diktor", "--emit-ir-text", "o.kirt", "a.kel"],
        &["compat-diktor", "--run-ir", "o.kir"],
        &["compat-diktor", "--via-ir", "a.kel"],
        &["compat-diktor", "--dump-ir-tables"],
        // Diktor の `--max-depth` は IR の旗である(木の評価器には効かない)。
        &["compat-diktor", "--max-depth", "5", "a.kel"],
        &[
            "compat-diktor",
            "--backend",
            "interp",
            "--",
            "--max-depth",
            "5",
            "a.kel",
        ],
        // `--` の後に後段の選択肢は書けない。
        &["compat-diktor", "--", "--backend", "interp", "a.kel"],
        // `--help` と `-h` は、Diktor と同じく知らない旗である。
        &["compat-diktor", "--help"],
        &["compat-diktor", "-h", "a.kel"],
    ] {
        let r = keleut_in(&d, args, b"");
        assert_eq!(r.status.code(), Some(64), "{args:?}: {r:?}");
        assert!(r.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&r.stderr).starts_with("keleut compat-diktor: "),
            "{args:?}: {r:?}"
        );
    }
    // IR の旗の呼び出しは何も書かない。
    assert!(!d.join("o.kir").exists() && !d.join("o.kirt").exists());
    // 後段の旗の誤りは `keleut run` が報告する。
    let r = keleut_in(&d, &["compat-diktor", "--backend", "x", "a.kel"], b"");
    assert_eq!(r.status.code(), Some(64));
    let _ = std::fs::remove_dir_all(&d);
}

/// 深さ `n` の `1 + (…)` の式を持つプログラムである。
fn nested(n: usize) -> String {
    format!(
        "let x = {}1{}\necholn(show(x))\n",
        "1 + (".repeat(n),
        ")".repeat(n)
    )
}

#[test]
fn 区切りの前の選択肢を後段に渡す() {
    let d = scratch("separator");
    std::fs::write(
        d.join("r.kel"),
        "let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)\necholn(show(f(100)))\n",
    )
    .unwrap();
    // `--` の前の `--max-depth` は `keleut run` の論理的な深さの限度である。
    let a = keleut_in(
        &d,
        &[
            "compat-diktor",
            "--backend",
            "interp",
            "--max-depth",
            "5",
            "--",
            "r.kel",
        ],
        b"",
    );
    let b = keleut_in(
        &d,
        &["run", "--backend", "interp", "--max-depth", "5", "r.kel"],
        b"",
    );
    assert!(same(&a, &b), "{a:?}\n{b:?}");
    assert_eq!(a.status.code(), Some(3), "{a:?}");
    let r = keleut_in(
        &d,
        &["compat-diktor", "--backend", "interp", "--", "r.kel"],
        b"",
    );
    assert_eq!(
        (r.status.code(), r.stdout.as_slice()),
        (Some(0), &b"100\n"[..]),
        "{r:?}"
    );
    // 型検査にも `--native-stack` を渡すので、構文解析の限度は実行と同じである。
    std::fs::write(d.join("d200.kel"), nested(200)).unwrap();
    for mode in [&[][..], &["--type-check"]] {
        let mut args = vec!["compat-diktor", "--native-stack", "512k", "--"];
        args.extend_from_slice(mode);
        args.push("d200.kel");
        let r = keleut_in(&d, &args, b"");
        assert_eq!(r.status.code(), Some(2), "{args:?}: {r:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 名前が_kir_で終わる入力もソースとして読む() {
    let d = scratch("source-kir");
    let src = "echoln(\"hi\")\n";
    std::fs::write(d.join("x.kir"), src).unwrap();
    std::fs::write(d.join("y.kirt"), src).unwrap();
    std::fs::write(d.join("a.kel"), "echoln(\"a\")\n").unwrap();
    for (args, out) in [
        (
            &["compat-diktor", "--backend", "interp", "x.kir"][..],
            "hi\n",
        ),
        (&["compat-diktor", "--backend", "interp", "y.kirt"], "hi\n"),
        (
            &["compat-diktor", "--backend", "interp", "a.kel", "x.kir"],
            "a\nhi\n",
        ),
    ] {
        let r = keleut_in(&d, args, b"");
        assert_eq!(
            (r.status.code(), String::from_utf8_lossy(&r.stdout).as_ref()),
            (Some(0), out),
            "{args:?}: {r:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}
