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

/// 試験ごとの作業の場所。捨てるときに中身ごと消す(試験が落ちたときも残さない)。
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("keleut-run-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
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
    // `--backend jit` は feature `llvm`(既定で有効)で組んだときだけ使え(M91)、外して組めば未実装の 4 である。
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
            .env("PATH", &*d)
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
    // 標準エラーの `<値>` のレコードの順は付録 B が読み手に任せるので、並べ替えてから比べる(D561)。
    if d.status.code() == k.status.code()
        && d.stdout == k.stdout
        && kconform::fieldsort::same_output(&d.stderr, &k.stderr)
    {
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

// ---- 仮想メモリの上限の下のフロントエンドとスレッドの確保(計画 261104-3 の 4.4 節、M110) ----

/// `sh -c 'ulimit -v KIB; exec keleut ARGS'` を `dir` で走らせる。
fn keleut_ulimit(dir: &Path, kib: u64, args: &[&str]) -> Output {
    Command::new("sh")
        .arg("-c")
        .arg(format!("ulimit -v {kib}; exec \"$0\" \"$@\""))
        .arg(env!("CARGO_BIN_EXE_keleut"))
        .args(args)
        .current_dir(dir)
        .env_remove("KELEUT_JIT_STATS")
        .env_remove("KELEUT_GC_STATS")
        .env_remove("KELEUT_TEST_ALLOC_FAIL")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn describe(r: &Output) -> String {
    format!(
        "終了状態 {:?}\n標準出力:\n{}\n標準エラー:\n{}",
        r.status.code(),
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    )
}

/// `src` を `dir/name.kel` に書き、上限なしの `keleut emit-kir` で `dir/name.kir` を作る。
fn write_kel_and_kir(dir: &Path, name: &str, src: &str) {
    std::fs::write(dir.join(format!("{name}.kel")), src).unwrap();
    let r = keleut_in(
        dir,
        &[
            "emit-kir",
            "-o",
            &format!("{name}.kir"),
            &format!("{name}.kel"),
        ],
    );
    assert_eq!(r.status.code(), Some(0), "{name}: {}", describe(&r));
}

/// 宣言 `n` 個と、200 万要素のリストを作って長さを数える末尾(`probe/small/gen_decls.py N list` と同じ)。
fn decls_source(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("let x_{i} = {i} + 1\n"));
    }
    s.push_str(
        "let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))\n\
         let rec len(xs: List[Int32], k: Int32): Int32 = xs match { case Nil => k case Cons(_, t) => len(t, k + 1) }\n\
         echoln(show(len(build(2000000, Nil), 0)))\n",
    );
    s.push_str(&format!("echoln(show(x_{}))\n", n - 1));
    s
}

/// 深さ 100 万の末尾でない再帰。
const DEEP_REC: &str = "let rec f(n: Int32): Int32 = if n == 0 then 0 else 1 + f(n - 1)\n\
                        echoln(show(f(1000000)))\n";

/// 深さ `n` の構造的な `Eq`(`conformance/stack/eq.t` の形)。
fn eq_source(n: u32) -> String {
    format!(
        "newtype T = Leaf | Node({{k: T}})\n\
         type instance Eq[T] {{ let eq(a, b) = a match {{ case Leaf => b match {{ case Leaf => true case Node(_) => false }} case Node(r) => b match {{ case Leaf => false case Node(s) => r == s }} }} }}\n\
         let rec mk(n: Int32, acc: T): T = if n == 0 then acc else mk(n - 1, Node({{k = acc}}))\n\
         echoln(show(mk({n}, Leaf) == mk({n}, Leaf)))\n"
    )
}

/// [`eq_source`] の比べ方を、`cancel` 節を持つハンドラの中で行う形。
fn eq_cancel_source(n: u32) -> String {
    format!(
        "effect Res = {{ use: () => Unit }}\n\
         newtype T = Leaf | Node({{k: T}})\n\
         type instance Eq[T] {{ let eq(a, b) = a match {{ case Leaf => b match {{ case Leaf => true case Node(_) => false }} case Node(r) => b match {{ case Leaf => false case Node(s) => r == s }} }} }}\n\
         let rec mk(n: Int32, acc: T): T = if n == 0 then acc else mk(n - 1, Node({{k = acc}}))\n\
         let main(): Boolean = run h {{ mk({n}, Leaf) == mk({n}, Leaf) }} handle {{ case use() => resume(()) case cancel => echoln(\"cancel\") }}\n\
         echoln(\"before\")\n\
         echoln(show(main()))\n"
    )
}

/// `ulimit -v 1000000` の下で走らせる構造的な `Eq` の深さ。
///
/// 最適化なしの構築は、比べ方の 1 段にネイティブのスタックを約 10 倍使う。
/// 深さ 20 万は `ulimit -v 4000000` でも足りず(`ulimit -v 6000000` で足りる)、`ulimit -v 1000000` では 25,000 段と 30,000 段の間でメモリが足りなくなる。
const EQ_DEPTH: u32 = if cfg!(debug_assertions) {
    20_000
} else {
    200_000
};

#[test]
fn 仮想メモリの上限の下でも_kir_の入力は実行のスレッドを縮めて走る() {
    // 直す前は、実行のスレッド(既定で約 1.1 GB)を作れずに 70 だった(V235)。
    let d = scratch("as-kir");
    write_kel_and_kir(&d, "decls", &decls_source(160_000));
    write_kel_and_kir(&d, "rec", DEEP_REC);
    write_kel_and_kir(&d, "eq", &eq_source(EQ_DEPTH));
    // 宣言 16 万個の `.kir` は、既定の後段では LLVM の分でヒープが本当に尽きることがあるので、インタプリタで走らせる。
    // 上限は、宣言 16 万個の `.kir` だけ 1,100,000 KiB にし、ほかは 1,000,000 KiB にする。
    // 宣言 16 万個の `.kir` は、リリースの構築で 930,000 KiB から 0 で 920,000 KiB では 3、最適化なしの構築で約 940,000 KiB から 0 なので、
    // 1,000,000 KiB では境から 1 割の余裕が無い。
    // 直す前の構築は、1,100,000 KiB でも実行のスレッドを作れずに 70 だった。
    let cases: &[(&[&str], &[u8], u64)] = &[
        (
            &["run", "--backend", "interp", "decls.kir"],
            b"2000000\n160000\n",
            1_100_000,
        ),
        (&["run", "rec.kir"], b"1000000\n", 1_000_000),
        (
            &["run", "--backend", "interp", "rec.kir"],
            b"1000000\n",
            1_000_000,
        ),
        (&["run", "eq.kir"], b"true\n", 1_000_000),
        (
            &["run", "--backend", "interp", "eq.kir"],
            b"true\n",
            1_000_000,
        ),
        (&["run", "rec.kel"], b"1000000\n", 1_000_000),
    ];
    for (args, out, limit) in cases {
        let r = keleut_ulimit(&d, *limit, args);
        assert!(
            r.status.code() == Some(0) && r.stdout == *out,
            "{args:?}: {}",
            describe(&r)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(feature = "llvm")]
#[test]
fn 仮想メモリの上限の下でも_jit_only_の深い再帰と構造的な_eq_は走る() {
    let d = scratch("as-jit");
    write_kel_and_kir(&d, "rec", DEEP_REC);
    write_kel_and_kir(&d, "eq", &eq_source(EQ_DEPTH));
    let cases: &[(&[&str], &[u8])] = &[
        (&["run", "--tier", "jit-only", "rec.kir"], b"1000000\n"),
        (&["run", "--tier", "jit-only", "eq.kir"], b"true\n"),
    ];
    for (args, out) in cases {
        let r = keleut_ulimit(&d, 1_000_000, args);
        assert!(
            r.status.code() == Some(0) && r.stdout == *out,
            "{args:?}: {}",
            describe(&r)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// 構造的な `Eq` が移るスレッドを作れなくなるまで比べ、`cancel` 節が走ってメモリの不足の 3 で終わることを確かめる。
///
/// `--native-stack 200m` と深さ 20 万は `verify/findings.md` の feasible-5 の形である。
/// 上限は 650,000 KiB にする。
/// 直す前は、650,000 KiB と 700,000 KiB で `新しいスタックのスレッドを作れません` の 70 だった。
/// 直した後のリリースの構築は、`--backend interp` が 725,000 KiB まで 3 で 750,000 KiB で 0、
/// `--tier jit-only` が 750,000 KiB まで 3 で 900,000 KiB で 0 である(移る先のスレッドを作れて最後まで比べる)。
fn expect_eq_hop_out_of_memory(tag: &str, backend: &[&str]) {
    let d = scratch(tag);
    std::fs::write(d.join("eqc.kel"), eq_cancel_source(200_000)).unwrap();
    let mut args = vec!["run", "--native-stack", "200m"];
    args.extend_from_slice(backend);
    args.push("eqc.kel");
    let r = keleut_ulimit(&d, 650_000, &args);
    assert!(
        r.status.code() == Some(3)
            && r.stdout == b"before\ncancel\n"
            && r.stderr == "実行時エラー: メモリ不足です\n".as_bytes(),
        "{backend:?}: {}",
        describe(&r)
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 構造的な_eq_が移るスレッドを作れなければ_cancel_節が走り_3_になる_interp() {
    expect_eq_hop_out_of_memory("eq-hop-interp", &["--backend", "interp"]);
}

#[cfg(feature = "llvm")]
#[test]
fn 構造的な_eq_が移るスレッドを作れなければ_cancel_節が走り_3_になる_jit() {
    expect_eq_hop_out_of_memory("eq-hop-jit", &["--tier", "jit-only"]);
}

/// 変換の役割の限度の行(段の名前を付けた行)。
fn stage_limit_line(stage: &str) -> String {
    format!("keleut: 処理系の限度: {stage}でメモリが足りません\n")
}

/// 予備を使い切った後に包みが書く固定の行。
const CONV_LIMIT_LINE: &str = "keleut: 処理系の限度: 型検査か実行の前の変換でメモリが足りません\n";

fn alloc_test_line(site: &str) -> String {
    format!("keleut: 試験: {site} の確保を失敗させました\n")
}

#[test]
fn 変換の役割の確保の失敗は_kel_で_1_kir_で_65_になる() {
    let d = scratch("conv-fail");
    write_kel_and_kir(&d, "hi", "echoln(\"hi\")\n");
    let cases = [
        (
            "conv:check:soft:0",
            "hi.kel",
            1,
            format!("{}{}", alloc_test_line("check"), stage_limit_line("型検査")),
        ),
        (
            "conv:check:hard:0",
            "hi.kel",
            1,
            format!("{}{CONV_LIMIT_LINE}", alloc_test_line("check")),
        ),
        (
            "conv:kir:soft:0",
            "hi.kir",
            65,
            format!(
                "{}{}",
                alloc_test_line("kir"),
                stage_limit_line("KIR の読み込み")
            ),
        ),
        (
            "conv:kir:hard:0",
            "hi.kir",
            65,
            format!("{}{CONV_LIMIT_LINE}", alloc_test_line("kir")),
        ),
    ];
    for (spec, input, code, err) in cases {
        let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(["run", "--backend", "interp", input])
            .current_dir(&d)
            .env("KELEUT_TEST_ALLOC_FAIL", spec)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            r.status.code() == Some(code) && r.stdout.is_empty() && r.stderr == err.as_bytes(),
            "{spec} {input}: {}",
            describe(&r)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// 関数の中で拡張の鎖を `n` 段作り、`cancel` 節を持つハンドラの中で呼ぶ(`probe/alloc/gen_chain.py func N`)。
fn func_chain_source(n: usize) -> String {
    let mut s =
        String::from("effect Res = { use: () => Unit }\nlet f(): Int32 = {\n  let s0 = {a = 0}\n");
    for i in 1..=n {
        s.push_str(&format!(
            "  let s{i} = {{a = {i}, b{} = 1 extends s{}}}\n",
            i % 7,
            i - 1
        ));
    }
    s.push_str(&format!("  s{n}.a\n}}\n"));
    s.push_str(
        "let main(): Int32 = run h { f() } handle { case use() => resume(()) case cancel => echoln(\"cancel\") }\n\
         echoln(\"before\")\necholn(show(main()))\n",
    );
    s
}

#[test]
fn 仮想メモリの上限の下で型検査の確保が尽きれば限度の行と_1_になる() {
    // 実際の `ulimit -v` の下で予備を使い切る道を見張る(直す前は `memory allocation of … failed` の 134 だった。V236)。
    let d = scratch("as-front");
    std::fs::write(d.join("f.kel"), func_chain_source(3000)).unwrap();
    for args in [
        &["run", "f.kel"][..],
        &["run", "--backend", "interp", "f.kel"],
    ] {
        let r = keleut_ulimit(&d, 524_288, args);
        assert!(
            r.status.code() == Some(1)
                && r.stdout.is_empty()
                && String::from_utf8_lossy(&r.stderr).ends_with(CONV_LIMIT_LINE),
            "{args:?}: {}",
            describe(&r)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 下げた_kir_の入れ子の限度は_kel_で_1_kir_で_65_になる() {
    let d = scratch("kir-nest");
    let n = 7000;
    let src = format!(
        "echoln(\"before\")\nlet x = {}1{}\necholn(show(x))\n",
        "1 + (".repeat(n),
        ")".repeat(n)
    );
    write_kel_and_kir(&d, "add", &src);
    // `.kir` の入力は、いまと同じく 65 である。
    let r = keleut_in(
        &d,
        &[
            "run",
            "--native-stack",
            "16m",
            "--backend",
            "interp",
            "add.kir",
        ],
    );
    assert!(
        r.status.code() == Some(65)
            && r.stdout.is_empty()
            && String::from_utf8_lossy(&r.stderr)
                .contains("処理系の限度: KIR の読み込みで入れ子が深すぎます"),
        "{}",
        describe(&r)
    );
    // `.kel` の入力では、同じプロセスの中で書いた KIR を検証が拒否すれば、限度の行と 1 である。
    // 最適化なしの構築では、フロントエンドの限度(16 MiB で約 440 段)が KIR の限度(約 1,500 段)より低く、
    // フロントエンドが受け付けて KIR の検証が拒否する深さが無いので、最適化ありの構築でだけ確かめる。
    if !cfg!(debug_assertions) {
        let r = keleut_in(
            &d,
            &[
                "run",
                "--native-stack",
                "16m",
                "--backend",
                "interp",
                "add.kel",
            ],
        );
        assert!(
            r.status.code() == Some(1)
                && r.stdout.is_empty()
                && String::from_utf8_lossy(&r.stderr)
                    .contains("処理系の限度: KIR の検証で入れ子が深すぎます"),
            "{}",
            describe(&r)
        );
    }
    // Diktor が書いた KIR を同じプロセスの中で読む `--frontend diktor` でも、`.kel` の入力なら 1 である。
    // Diktor の構文解析は `--native-stack` に縛られないので、最適化なしの構築でも KIR の読み込みの限度に当たる。
    if let Ok(diktor) = std::env::var("KELEUT_DIKTOR") {
        let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args([
                "run",
                "--frontend",
                "diktor",
                "--native-stack",
                "16m",
                "--backend",
                "interp",
                "add.kel",
            ])
            .current_dir(&d)
            .env("KELEUT_DIKTOR", &diktor)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            r.status.code() == Some(1)
                && r.stdout.is_empty()
                && String::from_utf8_lossy(&r.stderr)
                    .contains("処理系の限度: KIR の読み込みで入れ子が深すぎます"),
            "{}",
            describe(&r)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// 入れ子の形 `shape` の深さ `n` のプログラム(critic.md 6 節の 6 つの形)。
fn nest_source(shape: &str, n: usize) -> String {
    match shape {
        "block" => format!(
            "let r = {}a{}\necholn(show(r))\n",
            "{ let a = 0\n".repeat(n),
            " }".repeat(n)
        ),
        "add" => format!("echoln(show({}1{}))\n", "1 + (".repeat(n), ")".repeat(n)),
        "fn" => format!("let f = {}0\necholn(\"ok\")\n", "fn(x) => ".repeat(n)),
        "pattern" => format!(
            "let g(xs: List[Int32]): Int32 = xs match {{ case {}Nil{} => 1 case _ => 0 }}\necholn(show(g(Nil)))\n",
            "Cons(_, ".repeat(n),
            ")".repeat(n)
        ),
        "type" => format!(
            "let t: {}Int32{} = Nil\necholn(\"ok\")\n",
            "List[".repeat(n),
            "]".repeat(n)
        ),
        "match" => format!(
            "let h(n: Int32): Int32 = {}0{}\necholn(show(h(1)))\n",
            "n match { case _ => ".repeat(n),
            " }".repeat(n)
        ),
        _ => unreachable!(),
    }
}

#[test]
fn 受け付ける最大の深さの木を捨てても落ちない() {
    // `keleut run` はフロントエンドのスレッドの中で型検査の文脈と木を `drop` で捨てる(D570)。
    // 小さい `--native-stack` で、フロントエンドの限度の行が出ない最大の深さを二分探索で求め、
    // 探索で走らせたどの深さもシグナルで落ちないこと、最大の深さが 0 か KIR の検証の限度の 1 で終わることを確かめる。
    // 最適化なしの構築は 2.3 MiB より小さい値を上げるので、4m、8m、16m を使う(512k と 1m は記録に手で確かめた値がある)。
    const FRONT_LIMITS: [&str; 2] = ["構文解析で再帰が深すぎます", "型検査で再帰が深すぎます"];
    const KIR_LIMIT: &str = "処理系の限度: KIR の検証で入れ子が深すぎます";
    let shapes = ["block", "add", "fn", "pattern", "type", "match"];
    let stacks = ["4m", "8m", "16m"];
    let fails: Vec<String> = std::thread::scope(|s| {
        let hs: Vec<_> = shapes
            .iter()
            .flat_map(|&shape| stacks.iter().map(move |&stack| (shape, stack)))
            .map(|(shape, stack)| {
                s.spawn(move || {
                    let d = scratch(&format!("deepdrop-{shape}-{stack}"));
                    let mut fails = Vec::new();
                    // 深さ n を走らせ、フロントエンドの限度に当たったかを返す。
                    let mut probe = |n: usize| -> (bool, Output) {
                        std::fs::write(d.join("d.kel"), nest_source(shape, n)).unwrap();
                        let r = keleut_in(
                            &d,
                            &[
                                "run",
                                "--native-stack",
                                stack,
                                "--backend",
                                "interp",
                                "d.kel",
                            ],
                        );
                        let err = String::from_utf8_lossy(&r.stderr);
                        let lim = FRONT_LIMITS.iter().any(|l| err.contains(l));
                        if !matches!(r.status.code(), Some(0..=2)) {
                            fails.push(format!("{shape} {stack} 深さ {n}: {}", describe(&r)));
                        }
                        (lim, r)
                    };
                    let (mut lo, mut hi) = (1, 2);
                    while !probe(hi).0 {
                        lo = hi;
                        hi *= 2;
                        assert!(hi <= 1 << 20, "{shape} {stack}: 限度に当たらない");
                    }
                    while hi - lo > 1 {
                        let m = (lo + hi) / 2;
                        if probe(m).0 {
                            hi = m;
                        } else {
                            lo = m;
                        }
                    }
                    let (lim, r) = probe(lo);
                    let err = String::from_utf8_lossy(&r.stderr);
                    let ok = !lim
                        && (r.status.code() == Some(0)
                            || (r.status.code() == Some(1) && err.contains(KIR_LIMIT)));
                    if !ok {
                        fails.push(format!("{shape} {stack} 最大の深さ {lo}: {}", describe(&r)));
                    }
                    let _ = std::fs::remove_dir_all(&d);
                    fails
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
