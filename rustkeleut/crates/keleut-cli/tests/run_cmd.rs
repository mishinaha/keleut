//! `keleut run` の試験(計画 261024-1 の 6.2 節、M87)。
//!
//! - `tests/fixtures/run/` の各 `.kir` を走らせ、記録した標準出力、標準エラー、終了状態と比べる。
//!   記録は `diktor --run-ir` の結果である(作り方は同じ場所の `README.md`)。
//! - 環境変数 `KELEUT_DIKTOR` に Diktor の実行形を与えると、`.kel` を Diktor で KIR にする経路と、
//!   `conformance/corpus/` の全件を Diktor の木の評価器と比べる試験も走らせる。

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn keleut_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-run-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// fixture ごとに足す選択肢。
fn extra_args(name: &str) -> &'static [&'static str] {
    match name {
        "deep" => &["--max-depth", "100"],
        _ => &[],
    }
}

#[test]
fn fixture_の_kir_を記録どおりに走らせる() {
    let dir = root().join("tests/fixtures/run");
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
        let mut args = vec!["run"];
        args.extend_from_slice(extra_args(n));
        let kir = format!("{n}.kir");
        args.push(&kir);
        let r = keleut_in(&dir, &args);
        let want_out = std::fs::read(dir.join(format!("{n}.stdout"))).unwrap();
        let want_err = std::fs::read(dir.join(format!("{n}.stderr"))).unwrap();
        let want_code: i32 = std::fs::read_to_string(dir.join(format!("{n}.code")))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        if r.stdout != want_out || r.stderr != want_err || r.status.code() != Some(want_code) {
            fails.push(format!(
                "{n}: 終了状態 {:?}(記録 {want_code})\n標準出力:\n{}\n標準エラー:\n{}",
                r.status.code(),
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn 使い方の誤りと不正な_kir_と未実装の後段() {
    let d = scratch("usage");
    assert_eq!(keleut_in(&d, &["run"]).status.code(), Some(64));
    assert_eq!(
        keleut_in(&d, &["run", "--bogus", "a.kel"]).status.code(),
        Some(64)
    );
    assert_eq!(
        keleut_in(&d, &["run", "nosuch.kir"]).status.code(),
        Some(64)
    );
    std::fs::write(d.join("bad.kir"), b"KIR\x00garbage").unwrap();
    let r = keleut_in(&d, &["run", "bad.kir"]);
    assert_eq!(
        r.status.code(),
        Some(65),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let num = root().join("tests/fixtures/run/num.kir");
    let num = num.to_str().unwrap();
    for b in ["jit", "wasm"] {
        let r = keleut_in(&d, &["run", "--backend", b, num]);
        assert_eq!(r.status.code(), Some(4));
        assert!(r.stdout.is_empty());
    }
    // 小さいネイティブのスタックでも、インタプリタの呼び出しはネイティブのスタックを使わない。
    let deep = root().join("tests/fixtures/run/deep.kir");
    let r = keleut_in(
        &d,
        &[
            "run",
            "--native-stack",
            "512k",
            "--max-depth",
            "100",
            deep.to_str().unwrap(),
        ],
    );
    assert_eq!(r.status.code(), Some(3));
    assert_eq!(r.stdout, b"90\n");
}

#[test]
fn 深い非末尾の再帰は限度の手前まで走り限度で止まる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    let d = scratch("deep");
    std::fs::write(
        d.join("d.kel"),
        "let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)\n\
         echoln(show(f(1048566)))\n\
         echoln(show(f(1048577)))\n",
    )
    .unwrap();
    let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args(["run", "d.kel"])
        .current_dir(&d)
        .env("KELEUT_DIKTOR", &diktor)
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(3));
    assert_eq!(r.stdout, b"1048566\n");
    assert_eq!(
        String::from_utf8_lossy(&r.stderr),
        "実行時エラー: 論理的な深さが限度 1048576 を超えました\n"
    );
}

/// コーパスの 1 件の比べた結果。
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Same,
    Unimplemented,
    Differs(String),
}

fn compare_one(diktor: &str, corpus: &Path, rel: &str, flags: &str) -> Verdict {
    let src = corpus.join(rel);
    let dir = src.parent().unwrap();
    let file = src.file_name().unwrap().to_str().unwrap();
    let mut fl: Vec<&str> = if flags == "?" {
        Vec::new()
    } else {
        flags.split_whitespace().collect()
    };
    let run_d = |fl: &[&str]| {
        Command::new(diktor)
            .args(fl)
            .arg(file)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let mut d = run_d(&fl);
    if flags == "?" && d.status.code() == Some(1) {
        fl = vec!["--no-prelude"];
        d = run_d(&fl);
    }
    let k = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .arg("run")
        .args(&fl)
        .arg(file)
        .current_dir(dir)
        .env("KELEUT_DIKTOR", diktor)
        .env("TMPDIR", std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    if d.status.code() == k.status.code() && d.stdout == k.stdout && d.stderr == k.stderr {
        return Verdict::Same;
    }
    if k.status.code() == Some(4) && String::from_utf8_lossy(&k.stderr).contains("keleut: 未実装:")
    {
        return Verdict::Unimplemented;
    }
    Verdict::Differs(format!(
        "{rel}: diktor {:?} / keleut {:?}\n--- diktor の標準エラー\n{}--- keleut の標準エラー\n{}",
        d.status.code(),
        k.status.code(),
        String::from_utf8_lossy(&d.stderr),
        String::from_utf8_lossy(&k.stderr)
    ))
}

#[test]
fn コーパスの全件を_diktor_と比べる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    let corpus = root().join("../../conformance/corpus");
    let index = std::fs::read_to_string(corpus.join("index.tsv")).unwrap();
    let rows: Vec<(String, String)> = index
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let p = it.next().unwrap().to_string();
            let _ = it.next();
            (p, it.next().unwrap_or("").to_string())
        })
        .collect();
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    let chunk = rows.len().div_ceil(jobs);
    let results: Vec<Verdict> = std::thread::scope(|s| {
        let hs: Vec<_> = rows
            .chunks(chunk)
            .map(|c| {
                let (diktor, corpus) = (&diktor, &corpus);
                s.spawn(move || {
                    c.iter()
                        .map(|(p, f)| compare_one(diktor, corpus, p, f))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let same = results.iter().filter(|v| **v == Verdict::Same).count();
    let unimpl = results
        .iter()
        .filter(|v| **v == Verdict::Unimplemented)
        .count();
    let diffs: Vec<&String> = results
        .iter()
        .filter_map(|v| match v {
            Verdict::Differs(m) => Some(m),
            _ => None,
        })
        .collect();
    eprintln!(
        "コーパス {} 本: 一致 {same}、未実装(4)で止まったもの {unimpl}、差 {}",
        rows.len(),
        diffs.len()
    );
    assert!(
        diffs.is_empty(),
        "{}",
        diffs
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn dump_bytecode_は各関数の逆アセンブルを印字する() {
    let dir = root().join("tests/fixtures/run");
    let r = keleut_in(&dir, &["dump", "bytecode", "div0.kir"]);
    assert_eq!(r.status.code(), Some(0), "{r:?}");
    let out = String::from_utf8(r.stdout).unwrap();
    assert!(out.starts_with("(code #0 "), "{out}");
    assert!(out.contains("return"), "{out}");
    let r = keleut_in(&dir, &["dump", "bytecode"]);
    assert_eq!(r.status.code(), Some(64));
}

#[test]
fn 小さすぎるネイティブのスタックは使い方の誤りになる() {
    let dir = root().join("tests/fixtures/run");
    let r = keleut_in(&dir, &["run", "--native-stack", "256k", "div0.kir"]);
    assert_eq!(r.status.code(), Some(64), "{r:?}");
    let r = keleut_in(&dir, &["run", "--native-stack", "512k", "div0.kir"]);
    let want: i32 = std::fs::read_to_string(dir.join("div0.code"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(r.status.code(), Some(want), "{r:?}");
}
