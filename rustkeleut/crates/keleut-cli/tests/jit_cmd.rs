//! `keleut run --backend jit` の試験(M91、M92。feature `llvm` で組んだときだけ走る。`llvm` は既定の feature である)。
//!
//! `tests/fixtures/run/` の各 KIR を `--backend jit --tier jit-only` で走らせ、
//! 標準出力、標準エラー、終了状態が記録(インタプリタの結果)と一致することを確かめる。
//! `--gc-stress` を足した実行も同じ記録と比べる(JIT のスロットへの書き出しと安全点の漏れを捉える)。
//! 段の設定 `threshold`、`alternating`、`mixed`(M92)と、`--backend` を省いた既定でも同じ記録と比べる。
//! 翻訳の重い関数を JIT にしないことと、翻訳の途中で実行が終われば翻訳を待たずに終わることも確かめる(`tests/fixtures/heavy/`)。
#![cfg(feature = "llvm")]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

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
    check_with(&["--backend", "jit", "--tier", "jit-only"], extra);
}

fn check_with(mode: &[&str], extra: &[&str]) {
    let (dir, names) = fixtures();
    let mut fails = Vec::new();
    for n in &names {
        let mut args: Vec<String> = std::iter::once("run")
            .chain(mode.iter().copied())
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
fn 段の設定のどれでも記録どおりに走らせる() {
    for t in [
        "threshold=1",
        "threshold=3",
        "alternating=1",
        "alternating=2",
        "mixed=1",
        "mixed=2",
        "interp-only",
    ] {
        check_with(&["--tier", t], &[]);
    }
    check_with(&["--tier", "alternating=7"], &["--gc-stress"]);
    // `--backend` も `--tier` も省くと、jit の後段の threshold=1000 で走る。
    check_with(&[], &[]);
}

/// 仮想メモリの上限 512 MiB の下でも、JIT の段がインタプリタと同じに完走する。
///
/// 翻訳のスレッドのスタックの予約は上限に数えられるので、大きすぎると JIT の段だけが 3 か 70 で終わる。
/// kconform の組では `conformance/gc/gc.t` の上限のコマンドが同じことを確かめるが、ここでは `--backend` を省いた既定も確かめる。
/// 最適化なしの組みでは KIR の処理のスレッドのスタック(入れ子の 1 段に 8 KiB)が上限に収まらないので、`--release` でだけ走らせる。
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "最適化なしの組みでは KIR の処理のスレッドが上限に収まらない"
)]
fn 仮想メモリの上限の下で既定と段の設定のどれでも完走する() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ulimit");
    let bin = env!("CARGO_BIN_EXE_keleut");
    let mut fails = Vec::new();
    for t in [
        "",
        "--tier threshold=1",
        "--tier jit-only",
        "--tier alternating=1",
        "--tier mixed=1",
        "--backend interp",
    ] {
        let r = Command::new("bash")
            .current_dir(&dir)
            .arg("-c")
            .arg(format!(
                "ulimit -v 524288; exec '{bin}' run --native-stack 512k {t} gres.kir"
            ))
            .output()
            .unwrap();
        if r.stdout != b"200000\nG\n" || r.status.code() != Some(0) {
            fails.push(format!(
                "{t:?}: 終了状態 {:?}\n標準出力:\n{}\n標準エラー:\n{}",
                r.status.code(),
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// `tests/fixtures/heavy/` の KIR を走らせ、標準出力と終了状態を確かめて、標準エラーとかかった時間を返す。
fn run_heavy(name: &str, args: &[&str]) -> (String, Duration) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/heavy");
    let t = Instant::now();
    let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .current_dir(&dir)
        .env("KELEUT_JIT_STATS", "1")
        .arg("run")
        .args(args)
        .arg(format!("{name}.kir"))
        .output()
        .unwrap();
    let el = t.elapsed();
    let err = String::from_utf8_lossy(&r.stderr).into_owned();
    assert_eq!(r.stdout, b"4504500\n", "{name} {args:?}: {err}");
    assert_eq!(r.status.code(), Some(0), "{name} {args:?}: {err}");
    (err, el)
}

/// 翻訳の重さが上限を超える関数は、`--tier jit-only` でもインタプリタに残す。
///
/// 上限を入れる前は、この実行の翻訳に 110 秒ほどかかった。
#[test]
fn 翻訳の重い関数はインタプリタに残す() {
    for t in ["jit-only", "threshold=1", "alternating=1"] {
        let (err, el) = run_heavy("cons1000", &["--tier", t]);
        assert!(err.contains("インタプリタで走らせた関数 1、"), "{t}: {err}");
        assert!(el < Duration::from_secs(20), "{t}: {el:?}");
    }
}

/// 待たない頼みの翻訳の途中で実行が終われば、翻訳を待たずに終わる。
///
/// `--tier jit-only` は `f` の翻訳(0.6 秒ほど)を待ってから走るので、その時間は翻訳の時間を含む。
/// `--tier threshold=1` は `f` を 2 回目の呼び出しで翻訳に出し、翻訳の途中でプログラムが終わるので、それより十分に短く終わる。
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "最適化なしの組みでは実行の時間が翻訳の時間に近い"
)]
fn 翻訳の途中で実行が終われば翻訳を待たない() {
    let (_, jit) = run_heavy("cons200", &["--tier", "jit-only"]);
    let (_, thr) = run_heavy("cons200", &["--tier", "threshold=1"]);
    assert!(thr * 2 < jit, "threshold=1 {thr:?}、jit-only {jit:?}");
}
