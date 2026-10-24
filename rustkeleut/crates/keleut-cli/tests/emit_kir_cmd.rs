//! `keleut emit-kir` の終了状態と出力の試験。
//!
//! 終了状態と診断の形は Diktor の `--emit-ir` と同じである(計画 261024-1 の 6.2 節)。
//! 途中で終わったときは、出力先のファイルを作らない。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn keleut(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_keleut"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-emit-kir-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const OK: &str =
    "let rec f(n: Int32): Int32 = if n == 0 then 0 else n + f(n - 1)\necholn(show(f(10)))\n";

#[test]
fn バイナリとテキストを書く() {
    let d = scratch("ok");
    std::fs::write(d.join("ok.kel"), OK).unwrap();
    let r = keleut(&d, &["emit-kir", "ok.kel"]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(r.stdout.starts_with(b"\0KIR"));
    let p = kir::load(&r.stdout).unwrap();
    let r = keleut(&d, &["emit-kir", "--text", "-o", "ok.kirt", "ok.kel"]);
    assert_eq!(r.status.code(), Some(0));
    assert!(r.stdout.is_empty());
    let text = std::fs::read(d.join("ok.kirt")).unwrap();
    assert_eq!(kir::write_kirt(&p).unwrap(), text);
    let r = keleut(&d, &["emit-kir", "-o", "ok.kir", "ok.kel"]);
    assert_eq!(r.status.code(), Some(0));
    assert_eq!(
        std::fs::read(d.join("ok.kir")).unwrap(),
        kir::write_kir(&p).unwrap()
    );
    std::fs::remove_dir_all(&d).unwrap();
}

/// 終了状態と、標準エラーに含まれる文面と、出力先のファイルを作らないことを確かめる。
fn fails(d: &Path, args: &[&str], code: i32, msg: &str) {
    let mut v = vec!["emit-kir", "-o", "out.kir"];
    v.extend_from_slice(args);
    let r = keleut(d, &v);
    let err = String::from_utf8_lossy(&r.stderr);
    assert_eq!(r.status.code(), Some(code), "{args:?}: {err}");
    assert!(err.contains(msg), "{args:?}: {err}");
    assert!(r.stdout.is_empty(), "{args:?}");
    assert!(!d.join("out.kir").exists(), "{args:?}");
}

#[test]
fn 誤りの終了状態() {
    let d = scratch("err");
    std::fs::write(d.join("ty.kel"), "let x: Int32 = \"a\"\n").unwrap();
    std::fs::write(d.join("syn.kel"), "let = 1\n").unwrap();
    std::fs::write(
        d.join("ex.kel"),
        "let f(b: Boolean): Int32 = b match { case true => 1 }\n",
    )
    .unwrap();
    fails(&d, &["ty.kel"], 1, "型エラー");
    fails(&d, &["syn.kel"], 2, "構文エラー");
    fails(
        &d,
        &["nosuch.kel"],
        64,
        "keleut: ファイルを開けません: nosuch.kel: No such file or directory",
    );
    fails(&d, &["--typo", "ty.kel"], 64, "unknown option: --typo");
    // 網羅性の警告は標準エラーに出し、--strict-exhaustive では 1 で終わる。
    let r = keleut(&d, &["emit-kir", "-o", "ex.kir", "ex.kel"]);
    assert_eq!(r.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&r.stderr).contains('⚠'));
    fails(&d, &["--strict-exhaustive", "ex.kel"], 1, "⚠");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn 書き出せないときは_74() {
    let d = scratch("io");
    std::fs::write(d.join("ok.kel"), OK).unwrap();
    let r = keleut(&d, &["emit-kir", "-o", "nodir/x.kir", "ok.kel"]);
    assert_eq!(r.status.code(), Some(74));
    assert_eq!(
        String::from_utf8_lossy(&r.stderr),
        "keleut: KIR のバイナリを書き出せません: nodir/x.kir: No such file or directory\n"
    );
    if Path::new("/dev/full").exists() {
        let r = keleut(&d, &["emit-kir", "--text", "-o", "/dev/full", "ok.kel"]);
        assert_eq!(r.status.code(), Some(74));
        assert_eq!(
            String::from_utf8_lossy(&r.stderr),
            "keleut: KIR のテキストを書き出せません: No space left on device\n"
        );
    }
    std::fs::remove_dir_all(&d).unwrap();
}

/// diktor/test/ir_run.t の、入れ子の限度をちょうど超える式である。
#[test]
fn 入れ子の限度を超えるプログラムは書かずに_1() {
    let d = scratch("nest");
    let nest = |n: usize| {
        format!(
            "let x = {}1{}\necholn(show(x))\n",
            "1 + (".repeat(n),
            ")".repeat(n)
        )
    };
    std::fs::write(d.join("max.kel"), nest(99_999)).unwrap();
    std::fs::write(d.join("over.kel"), nest(100_000)).unwrap();
    let r = keleut(&d, &["emit-kir", "-o", "max.kir", "max.kel"]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    for text in [false, true] {
        let mut args = vec![];
        if text {
            args.push("--text");
        }
        args.push("over.kel");
        fails(
            &d,
            &args,
            1,
            "keleut: KIR を書き出せません: 処理系の限度: KIR の書き出しで入れ子が深すぎます",
        );
    }
    std::fs::remove_dir_all(&d).unwrap();
}

/// 標準出力がプロセスの始めから閉じていれば、Diktor と同じく 74 で終わる。
///
/// Rust の標準ライブラリは閉じた標準出力を `/dev/null` で開き直すので、何もしなければ出力が黙って捨てられる。
#[cfg(target_os = "linux")]
#[test]
fn 閉じた標準出力には書けずに_74() {
    let d = scratch("closed");
    std::fs::write(d.join("ok.kel"), OK).unwrap();
    std::fs::write(d.join("ty.kel"), "let x: Int32 = \"a\"\n").unwrap();
    let k = env!("CARGO_BIN_EXE_keleut");
    for (args, what) in [
        ("emit-kir ok.kel", "KIR のバイナリを書き出せません"),
        ("emit-kir --text ok.kel", "KIR のテキストを書き出せません"),
        ("check ty.kel", "標準出力に書き出せません"),
    ] {
        let r = Command::new("sh")
            .current_dir(&d)
            .arg("-c")
            .arg(format!("exec \"$0\" {args} >&-"))
            .arg(k)
            .output()
            .unwrap();
        assert_eq!(r.status.code(), Some(74), "{args}");
        assert_eq!(
            String::from_utf8_lossy(&r.stderr),
            format!("keleut: {what}: Bad file descriptor\n"),
            "{args}"
        );
    }
    // 開いていれば、/dev/null に書くのは誤りでない。
    let r = Command::new("sh")
        .current_dir(&d)
        .arg("-c")
        .arg("exec \"$0\" emit-kir ok.kel >/dev/null")
        .arg(k)
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(0));
    std::fs::remove_dir_all(&d).unwrap();
}

/// テキストは全体をメモリに置かずに流しながら書く(Diktor の `Ir_text.output` と同じ)。
///
/// 深さ 20,000 の式のテキストは約 800 MB である。
/// 仮想メモリを 1.5 GB(うち 1 GB は `--native-stack` のスタック)に抑えても書き終えることで、
/// テキストを組み上げてから書く作りに戻っていないことを確かめる。
#[cfg(target_os = "linux")]
#[test]
fn 深いプログラムのテキストを流しながら書く() {
    let d = scratch("stream");
    let n = 20_000;
    std::fs::write(
        d.join("deep.kel"),
        format!("let v: Int32 = {}1{}\n", "-(".repeat(n), ")".repeat(n)),
    )
    .unwrap();
    let r = Command::new("sh")
        .current_dir(&d)
        .arg("-c")
        .arg(
            "ulimit -v 1500000 && exec \"$0\" emit-kir --text --no-prelude --native-stack 1g \
             -o /dev/null deep.kel",
        )
        .arg(env!("CARGO_BIN_EXE_keleut"))
        .output()
        .unwrap();
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    std::fs::remove_dir_all(&d).unwrap();
}
