//! `keleut run --backend jit` のテスト。
//! feature `llvm`(既定で有効)を付けてビルドしたときだけ実行される。
//!
//! `tests/fixtures/run/` の各 KIR を `--backend jit --tier jit-only` で実行し、
//! 標準出力、標準エラー、終了コードが期待出力(インタプリタの結果)と一致することを確かめる。
//! `--gc-stress` を追加した実行も、同じ期待出力と比較する(JIT コードが値をスロットに書き出し忘れている箇所と、安全点の漏れを検出する)。
//! ティアの設定 `threshold`、`alternating`、`mixed` と、`--backend` を省いた既定の設定でも、同じ期待出力と比較する。
//! コンパイルのコストが大きい関数を JIT コンパイルしないことと、
//! コンパイルの途中で実行が終わればコンパイルの完了を待たずに終了することも確かめる(`tests/fixtures/heavy/`)。
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
                "{n}: 終了コード {:?}(期待値 {want_code})\n標準エラー:\n{}",
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
    // `--backend` も `--tier` も省くと、バックエンド `jit` の `threshold=1000` で実行する。
    check_with(&[], &[]);
}

/// 仮想メモリを 512 MiB に制限しても(`ulimit -v`)、JIT のティアの各設定がインタプリタと同じ結果で完走する。
///
/// コンパイルスレッドのスタックの予約は仮想メモリの上限に数えられるので、大きすぎると JIT を使う設定だけが終了コード 3 か 70 で終了する。
/// kconform のペアでは `conformance/gc/gc.t` の `ulimit -v` を付けたコマンドが同じことを確かめるが、ここでは `--backend` を省いた既定の設定も確かめる。
/// デバッグビルドでは、KIR を処理するスレッドのスタック(入れ子 1 段あたり 8 KiB)が上限に収まらないので、リリースビルド(`--release`)でだけ実行する。
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "デバッグビルドでは KIR の処理のスレッドが上限に収まらない"
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
                "{t:?}: 終了コード {:?}\n標準出力:\n{}\n標準エラー:\n{}",
                r.status.code(),
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// `tests/fixtures/heavy/` の KIR を実行し、標準出力と終了コードを確かめて、標準エラーの内容と実行時間を返す。
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

/// コンパイルのコストの見積もりが上限を超える関数は、`--tier jit-only` でも JIT コンパイルせず、インタプリタで実行する。
///
/// 上限がない場合の実測では、この実行のコンパイルに 110 秒ほどかかった。
#[test]
fn 翻訳の重い関数はインタプリタに残す() {
    for t in ["jit-only", "threshold=1", "alternating=1"] {
        let (err, el) = run_heavy("cons1000", &["--tier", t]);
        assert!(err.contains("インタプリタで実行した関数 1、"), "{t}: {err}");
        assert!(el < Duration::from_secs(20), "{t}: {el:?}");
    }
}

/// 完了を待たずに依頼したコンパイルの途中でプログラムの実行が終われば、`keleut run` はそのコンパイルが終わるのを待たずに終了する。
///
/// `--tier jit-only` は `f` のコンパイル(0.6 秒ほど)の完了を待ってから `f` を実行するので、実行時間はコンパイルの時間を含む。
/// `--tier threshold=1` は、`f` の 1 回目の呼び出しでコンパイルを依頼してその呼び出しをインタプリタで実行し、コンパイルの途中でプログラムが終わるので、それより十分に短い時間で終了する。
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "デバッグビルドでは実行の時間がコンパイルの時間に近い"
)]
fn 翻訳の途中で実行が終われば翻訳を待たない() {
    let (_, jit) = run_heavy("cons200", &["--tier", "jit-only"]);
    let (_, thr) = run_heavy("cons200", &["--tier", "threshold=1"]);
    assert!(thr * 2 < jit, "threshold=1 {thr:?}、jit-only {jit:?}");
}
