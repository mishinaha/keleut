//! `keleut run` の試験(計画 261024-1 の 6.2 節、M87)。
//!
//! - `tests/fixtures/run/` の各 `.kir` を走らせ、記録した標準出力、標準エラー、終了状態と比べる。
//!   記録は `diktor --run-ir` の結果である(作り方は同じ場所の `README.md`)。
//! - 環境変数 `KELEUT_DIKTOR` に Diktor の実行形を与えると、`.kel` を Diktor で KIR にする経路(`--frontend diktor`)と、
//!   `conformance/corpus/` の全件を Diktor の木の評価器と比べる試験も走らせる。
//! - `.kel` の既定のフロントエンドが Rust(crate keleut-front)であることを、Diktor の無い環境で確かめる(M99)。

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
    // `--backend jit` は feature `llvm` を付けて組んだときだけ使え(M91)、付けなければ未実装の 4 である。
    let r = keleut_in(&d, &["run", "--backend", "jit", num]);
    if cfg!(feature = "llvm") {
        assert_eq!(r.status.code(), Some(0));
    } else {
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
        .args(["run", "--frontend", "diktor", "d.kel"])
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

#[test]
fn 既定のフロントエンドは_rust_で_diktor_も選べる() {
    let d = scratch("frontend");
    std::fs::write(d.join("a.kel"), "echoln(show(1 + 2))\n").unwrap();
    // Diktor を呼べない環境(`KELEUT_DIKTOR` が無いファイルを指し、PATH に diktor が無い)。
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(args)
            .current_dir(&d)
            .env("KELEUT_DIKTOR", d.join("no-such-diktor"))
            .env("PATH", &d)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    for args in [
        &["run", "a.kel"][..],
        &["run", "--frontend", "rust", "a.kel"],
        &["run", "--backend", "interp", "a.kel"],
    ] {
        let r = run(args);
        assert_eq!(
            (r.status.code(), r.stdout.as_slice()),
            (Some(0), &b"3\n"[..]),
            "{args:?}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
    // `--frontend diktor` は Diktor を起動しようとし、起動できなければ使い方の誤り(64)で終わる。
    let r = run(&["run", "--frontend", "diktor", "a.kel"]);
    assert_eq!(r.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&r.stderr).contains("を起動できません"));
    // 仮想メモリの上限の下では、`--native-stack` を与えなければ、フロントエンドのスレッドのスタックを上限に合わせて小さくする
    // (`--max-depth 1000` はインタプリタのスタックを上限に収めるためである)。
    // `--native-stack` を与えれば、フロントエンドもそのスタックで走る(V227)。
    // LLVM を付けない構築では、スタックを小さくしなくても深さの限度に見込むスタックで作り直せて通る。
    // feature `llvm` の構築では `libLLVM.so` が仮想メモリの大部分を使うので、この確かめは小さくすることに頼る。
    // 最適化なしの構築では、KIR の符号化と検証のスレッド(約 797 MiB)が上限に収まらないので確かめない。
    if !cfg!(debug_assertions) {
        for opts in ["--native-stack 512k", "--max-depth 1000"] {
            let r = Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "ulimit -v 524288; exec \"$0\" run {opts} --backend interp a.kel"
                ))
                .arg(env!("CARGO_BIN_EXE_keleut"))
                .current_dir(&d)
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(
                (r.status.code(), r.stdout.as_slice()),
                (Some(0), &b"3\n"[..]),
                "ulimit -v, {opts}: {}",
                String::from_utf8_lossy(&r.stderr)
            );
        }
    }
    // `keleut build` も既定で Rust のフロントエンドを使い、`--frontend diktor` を受け付ける。
    let out = d.join("out");
    let r = run(&[
        "build",
        "--target",
        "wasm",
        "-o",
        out.to_str().unwrap(),
        "a.kel",
    ]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(out.join("program.wasm").exists());
    let r = run(&[
        "build",
        "--target",
        "wasm",
        "-o",
        out.to_str().unwrap(),
        "--frontend",
        "diktor",
        "a.kel",
    ]);
    assert_eq!(r.status.code(), Some(64));
    let _ = std::fs::remove_dir_all(&d);
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
        .args(["run", "--frontend", "diktor"])
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
fn may_yield_の印の誤りは刈り込みのときだけ_70_になる() {
    let dir = root().join("tests/fixtures/run");
    let r = keleut_in(&dir, &["run", "noyield.kirt"]);
    assert_eq!(r.status.code(), Some(70), "{r:?}");
    assert!(String::from_utf8_lossy(&r.stderr).contains("noyield の再開点"));
    let r = keleut_in(&dir, &["run", "--no-yield-prune", "noyield.kirt"]);
    assert_eq!(r.status.code(), Some(0), "{r:?}");
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

#[test]
fn native_stack_はフロントエンドのスタックも決める() {
    // V227: `keleut run` の `--native-stack` は、インタプリタのスタックとともに Rust のフロントエンドのスタックも決める。
    let d = scratch("front-stack");
    let mut e = String::from("1");
    for _ in 0..20000 {
        e = format!("1 + ({e})");
    }
    std::fs::write(
        d.join("deep.kel"),
        format!("echoln(\"before\")\nlet x = {e}\necholn(show(x))\n"),
    )
    .unwrap();
    std::fs::write(d.join("a.kel"), "echoln(show(1 + 2))\n").unwrap();
    // 既定のスタックでは 2 万段の式が型検査に通り、実行して 0 で終わる。
    let r = keleut_in(&d, &["run", "--backend", "interp", "deep.kel"]);
    assert_eq!(
        (r.status.code(), r.stdout.as_slice()),
        (Some(0), &b"before\n20001\n"[..]),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    // 4 MiB では構文解析の限度が下がり、`keleut check --native-stack 4m` と同じく構文解析の限度の 2 で終わる。
    let r = keleut_in(
        &d,
        &[
            "run",
            "--native-stack",
            "4m",
            "--backend",
            "interp",
            "deep.kel",
        ],
    );
    assert_eq!(r.status.code(), Some(2), "{r:?}");
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("処理系の限度: 構文解析で再帰が深すぎます"),
        "{r:?}"
    );
    assert!(r.stdout.is_empty());
    // フロントエンドの下限より小さい値(最適化なしの構築の 512 KiB)は、下限まで上げて使い、使い方の誤りにしない。
    let r = keleut_in(
        &d,
        &[
            "run",
            "--native-stack",
            "512k",
            "--backend",
            "interp",
            "a.kel",
        ],
    );
    assert_eq!(
        (r.status.code(), r.stdout.as_slice()),
        (Some(0), &b"3\n"[..]),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn native_stack_は_kir_の読み込みのスタックも決める() {
    // 計画 6.11 節: `--native-stack` は KIR の読み込みと検証のスタックも決め、入れ子の限度をそれに収まる深さまで下げる。
    let d = scratch("kir-stack");
    let mut e = String::from("1");
    for _ in 0..3000 {
        e = format!("1 + ({e})");
    }
    std::fs::write(
        d.join("deep.kel"),
        format!("echoln(\"before\")\nlet x = {e}\necholn(show(x))\n"),
    )
    .unwrap();
    let r = keleut_in(&d, &["emit-kir", "-o", "deep.kir", "deep.kel"]);
    assert_eq!(r.status.code(), Some(0), "{r:?}");
    // 既定のスタックでは読めて、実行して 0 で終わる。
    let r = keleut_in(&d, &["run", "--backend", "interp", "deep.kir"]);
    assert_eq!(
        (r.status.code(), r.stdout.as_slice()),
        (Some(0), &b"before\n3001\n"[..]),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    // 4 MiB では入れ子の限度が下がり、スタックあふれで落ちずに 65 で終わる。
    let r = keleut_in(
        &d,
        &[
            "run",
            "--native-stack",
            "4m",
            "--backend",
            "interp",
            "deep.kir",
        ],
    );
    assert_eq!(r.status.code(), Some(65), "{r:?}");
    assert!(
        String::from_utf8_lossy(&r.stderr)
            .contains("処理系の限度: KIR の読み込みで入れ子が深すぎます"),
        "{r:?}"
    );
    assert!(r.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn source_only_は名前が_kir_で終わる入力もソースとして読む() {
    let d = scratch("source-only");
    std::fs::write(d.join("x.kir"), "echoln(\"hi\")\n").unwrap();
    std::fs::write(d.join("a.kel"), "echoln(\"a\")\n").unwrap();
    // 内部の旗が無ければ、`.kir` は KIR として読み(65)、`.kel` と混ぜれば使い方の誤り(64)である。
    let r = keleut_in(&d, &["run", "--backend", "interp", "x.kir"]);
    assert_eq!(r.status.code(), Some(65), "{r:?}");
    let r = keleut_in(&d, &["run", "--backend", "interp", "a.kel", "x.kir"]);
    assert_eq!(r.status.code(), Some(64), "{r:?}");
    for (args, out) in [
        (
            &["run", "--source-only", "--backend", "interp", "x.kir"][..],
            &b"hi\n"[..],
        ),
        (
            &[
                "run",
                "--source-only",
                "--backend",
                "interp",
                "a.kel",
                "x.kir",
            ],
            b"a\nhi\n",
        ),
    ] {
        let r = keleut_in(&d, args);
        assert_eq!(
            (r.status.code(), r.stdout.as_slice()),
            (Some(0), out),
            "{args:?}: {r:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}
