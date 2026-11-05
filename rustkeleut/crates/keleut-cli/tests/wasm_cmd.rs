//! `keleut run --backend wasm` と `keleut build` の試験(計画 261024-1 の 6.10 節、M94)。
//!
//! Node(環境変数 `KELEUT_NODE`、無ければ PATH の `node`)が無ければ、何もせずに通る。
//!
//! - `tests/fixtures/run/` の各 `.kir` を WasmGC の後段で走らせ、記録した標準出力、標準エラー、終了状態と比べる
//!   (記録は `diktor --run-ir` の結果で、インタプリタの試験と同じものである)。
//! - `keleut build` の出力を `node DIR/main.mjs` だけで走らせる(`Worker` のスタックの経路)。
//! - 起動のときに閉じていた標準出力は、インタプリタと同じく 74 になる。
//!   `keleut build` の起動の台本 `DIR/run` も、閉じた標準出力で `keleut run --backend wasm` と同じ終わり方をする。
//! - 環境変数 `KELEUT_DIKTOR` を与えると、大きなフレームの形になる関数、ブロックの多い関数、引数の多い関数を、
//!   インタプリタと WasmGC の後段で走らせて比べる。
//!   何度も呼ぶ大きな関数(部分に分ける関数)と、多くの型のクロージャを呼ぶ関数も比べる。
//! - 取れないスタックの深さの限度、仮想メモリの上限、シグナルの下でも、型の誤りの 1 で終わったり、
//!   止まったままになったり、一時の場所や Node のプロセスを残したりしない。

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn node() -> std::ffi::OsString {
    std::env::var_os("KELEUT_NODE").unwrap_or_else(|| "node".into())
}

fn have_node() -> bool {
    Command::new(node())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
    let d = std::env::temp_dir().join(format!("keleut-wasm-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixture_names() -> Vec<String> {
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
    names
}

fn record(name: &str) -> (Vec<u8>, Vec<u8>, i32) {
    let dir = root().join("tests/fixtures/run");
    (
        std::fs::read(dir.join(format!("{name}.stdout"))).unwrap(),
        std::fs::read(dir.join(format!("{name}.stderr"))).unwrap(),
        std::fs::read_to_string(dir.join(format!("{name}.code")))
            .unwrap()
            .trim()
            .parse()
            .unwrap(),
    )
}

#[test]
fn fixture_の_kir_を_wasm_で記録どおりに走らせる() {
    if !have_node() {
        eprintln!("node が無いので飛ばす");
        return;
    }
    let dir = root().join("tests/fixtures/run");
    let mut fails = Vec::new();
    for n in fixture_names() {
        let mut args = vec!["run", "--backend", "wasm"];
        if n == "deep" {
            args.extend_from_slice(&["--max-depth", "100"]);
        }
        let kir = format!("{n}.kir");
        args.push(&kir);
        let r = keleut_in(&dir, &args);
        let (out, err, code) = record(&n);
        if r.stdout != out || r.stderr != err || r.status.code() != Some(code) {
            fails.push(format!(
                "{n}: 終了状態 {:?}(記録 {code})\n標準出力:\n{}\n標準エラー:\n{}",
                r.status.code(),
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn build_の出力は_node_だけで走る() {
    if !have_node() {
        eprintln!("node が無いので飛ばす");
        return;
    }
    let d = scratch("build");
    let fx = root().join("tests/fixtures/run");
    for n in ["num", "records", "matchfail", "effect"] {
        let out = d.join(n);
        let kir = fx.join(format!("{n}.kir"));
        let r = keleut_in(
            &d,
            &[
                "build",
                "--target",
                "wasm",
                "-o",
                out.to_str().unwrap(),
                kir.to_str().unwrap(),
            ],
        );
        assert_eq!(
            r.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&r.stderr)
        );
        for f in ["program.wasm", "rt.wasm", "main.mjs", "run"] {
            assert!(out.join(f).exists(), "{n}: {f}");
        }
        let r = Command::new(node())
            .arg(out.join("main.mjs"))
            .env_remove("KELEUT_WASM_DIRECT")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let (o, e, c) = record(n);
        assert_eq!(
            (r.stdout, r.stderr, r.status.code()),
            (o, e, Some(c)),
            "{n}"
        );
    }
    // 使い方の誤り。
    assert_eq!(keleut_in(&d, &["build"]).status.code(), Some(64));
    let num = fx.join("num.kir");
    assert_eq!(
        keleut_in(&d, &["build", "-o", "x", num.to_str().unwrap()])
            .status
            .code(),
        Some(64)
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 閉じた標準出力は_74_になる() {
    if !have_node() {
        eprintln!("node が無いので飛ばす");
        return;
    }
    let num = root().join("tests/fixtures/run/num.kir");
    let k = env!("CARGO_BIN_EXE_keleut");
    let run = |b: &str| {
        Command::new("sh")
            .arg("-c")
            .arg(format!("'{k}' run --backend {b} '{}' >&-", num.display()))
            .output()
            .unwrap()
    };
    let (w, i) = (run("wasm"), run("interp"));
    assert_eq!(w.status.code(), Some(74));
    assert_eq!(w.status.code(), i.status.code());
    assert_eq!(w.stderr, i.stderr);
}

/// `sh -c` で `CMD` を走らせる。
///
/// `KELEUT_NODE` があれば、その場所を `PATH` の先頭に置く(`DIR/run` は `PATH` の `node` を起動する)。
fn sh_out(cmd: &str, envs: &[(&str, &str)]) -> Output {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd).stdin(Stdio::null());
    if let Some(n) = std::env::var_os("KELEUT_NODE") {
        let n = PathBuf::from(n);
        if let Some(d) = n.parent().filter(|d| !d.as_os_str().is_empty()) {
            let mut p = d.as_os_str().to_owned();
            p.push(":");
            p.push(std::env::var_os("PATH").unwrap_or_default());
            c.env("PATH", p);
        }
    }
    c.env_remove("KELEUT_STDOUT_CLOSED")
        .env_remove("KELEUT_WASM_DIRECT")
        .envs(envs.iter().copied())
        .output()
        .unwrap()
}

#[test]
fn build_の起動の台本は閉じた標準出力で_run_と同じ() {
    if !have_node() {
        eprintln!("node が無いので飛ばす");
        return;
    }
    let d = scratch("launcher");
    let k = env!("CARGO_BIN_EXE_keleut");
    // 書くプログラム、書かないプログラム、書く前に実行時エラーで止まるプログラム。
    let progs = [
        (
            "writes",
            "with_stdout(fn() => println(\"hello\"))\n",
            74,
            "hello\n",
        ),
        ("quiet", "let x = 1\n", 0, ""),
        ("error", "let x: Int32 = 1 / 0\n", 3, ""),
    ];
    for (name, src, closed, open) in progs {
        let kel = d.join(format!("{name}.kel"));
        std::fs::write(&kel, src).unwrap();
        let out = d.join(name);
        let r = keleut_in(
            &d,
            &[
                "build",
                "--target",
                "wasm",
                "-o",
                out.to_str().unwrap(),
                kel.to_str().unwrap(),
            ],
        );
        assert_eq!(
            r.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        let run = out.join("run");
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(&run).unwrap().permissions().mode();
            assert_eq!(m & 0o777, 0o755, "{name}: run の許可");
        }
        let run = run.display();
        let kel = kel.display();
        // 閉じた標準出力。
        let a = sh_out(&format!("'{run}' >&-"), &[]);
        let b = sh_out(&format!("'{k}' run --backend wasm '{kel}' >&-"), &[]);
        assert_eq!(a.status.code(), Some(closed), "{name}: DIR/run >&-");
        assert_eq!(
            (a.status.code(), &a.stderr),
            (b.status.code(), &b.stderr),
            "{name}: DIR/run >&- と keleut run --backend wasm >&-"
        );
        // 開いた標準出力。
        let n = sh_out(&format!("'{run}' >/dev/null"), &[]);
        let ok = if closed == 74 { 0 } else { closed };
        assert_eq!(n.status.code(), Some(ok), "{name}: DIR/run >/dev/null");
        let a = sh_out(&format!("'{run}'"), &[]);
        let m = Command::new(node())
            .arg(out.join("main.mjs"))
            .env_remove("KELEUT_STDOUT_CLOSED")
            .env_remove("KELEUT_WASM_DIRECT")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(a.stdout, open.as_bytes(), "{name}: DIR/run の出力");
        assert_eq!(
            (&a.stdout, &a.stderr, a.status.code()),
            (&m.stdout, &m.stderr, m.status.code()),
            "{name}: DIR/run と node DIR/main.mjs"
        );
        // 環境に残った KELEUT_STDOUT_CLOSED は、開いた標準出力では除く。
        let e = sh_out(&format!("'{run}'"), &[("KELEUT_STDOUT_CLOSED", "1")]);
        assert_eq!(
            (e.status.code(), &e.stdout),
            (Some(ok), &a.stdout),
            "{name}: KELEUT_STDOUT_CLOSED=1 DIR/run"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `.kel` を Diktor で KIR にし、両方の後段で走らせて結果を比べる。
fn same_on_both(diktor: &str, d: &Path, name: &str, src: &str, extra: &[&str]) {
    let kel = d.join(format!("{name}.kel"));
    let kir = d.join(format!("{name}.kir"));
    std::fs::write(&kel, src).unwrap();
    let st = Command::new(diktor)
        .arg("--emit-ir")
        .arg(&kir)
        .arg(&kel)
        .env("OCAMLRUNPARAM", "l=8M")
        .output()
        .unwrap();
    assert!(
        st.status.success(),
        "{}",
        String::from_utf8_lossy(&st.stderr)
    );
    let mut a = vec!["run", "--backend", "interp"];
    a.extend_from_slice(extra);
    a.push(kir.to_str().unwrap());
    let i = keleut_in(d, &a);
    a[2] = "wasm";
    let w = keleut_in(d, &a);
    assert_eq!(
        (&w.stdout, &w.stderr, w.status.code()),
        (&i.stdout, &i.stderr, i.status.code()),
        "{name}: wasm の標準エラー: {}",
        String::from_utf8_lossy(&w.stderr)
    );
}

#[test]
fn 大きなフレームとブロックの多い関数を_interp_と比べる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    let d = scratch("big");
    // 400 個の値を生かしたまま呼び出しを重ねる関数を、深さ 10 万まで再帰させる(大きなフレームの形)。
    let mut src = String::from(
        "let id(x: Int32): Int32 = x\nlet rec f(n: Int32): Int32 = if n == 0 then 0 else {\n",
    );
    for k in 0..400 {
        src.push_str(&format!("  let a{k} = id(n + {k})\n"));
    }
    src.push_str("  let s = f(n - 1)\n  s + 1 + (a0");
    for k in 1..400 {
        src.push_str(&format!(" + a{k}"));
    }
    src.push_str(") - (a0");
    for k in 1..400 {
        src.push_str(&format!(" + a{k}"));
    }
    src.push_str(")\n}\necholn(show(f(100000)))\n");
    same_on_both(&diktor, &d, "bigframe", &src, &[]);
    // 同じ関数が限度で 3 になる。
    same_on_both(&diktor, &d, "bigframe", &src, &["--max-depth", "5000"]);
    // 5,000 段の if の鎖(1 万個を超えるブロック)。
    let mut src = String::from("let f(x: Int32): Int32 = ");
    for k in 0..5000 {
        src.push_str(&format!("if x == {k} then {} else ", k * 2));
    }
    src.push_str("0\necholn(show(f(4999)))\necholn(show(f(77)))\n");
    same_on_both(&diktor, &d, "blocks", &src, &[]);
    let _ = std::fs::remove_dir_all(&d);
}

/// `big` の本体を `body` の行にした関数を、`loop` から `calls` 回呼ぶ `.kel`(`header` は先頭の宣言)。
fn hot_src(header: &str, body: &[String], result: &str, calls: u32, handle: &str) -> String {
    let row = if handle.is_empty() { "" } else { " @ {Ask}" };
    let mut s = format!("{header}let big(n: Int32): Int32{row} = {{\n");
    for l in body {
        s.push_str(&format!("  {l}\n"));
    }
    s.push_str(&format!("  {result}\n}}\n"));
    s.push_str(&format!(
        "let rec loop(i: Int32, acc: Int32): Int32{row} = if i == 0 then acc else loop(i - 1, acc + big(i))\n"
    ));
    s.push_str(&format!("echoln(show(loop({calls}, 0){handle}))\n"));
    s
}

#[test]
fn 何度も呼ぶ大きな関数を_interp_と比べる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    let d = scratch("hot");
    // V8 は何度も呼ぶ関数を最適化の翻訳にかける。1 つの Wasm の関数が大きいと、その翻訳が記憶を使い尽くして
    // Node が SIGTRAP で落ちた(出力の後でも 70 になった)。重い関数は部分に分けるので、どれも 0 で終わる。
    // 1 万段の if の鎖を 2 万回呼ぶ。
    let mut src = String::from("let f(x: Int32): Int32 = ");
    for k in 0..10_000 {
        src.push_str(&format!("if x == {k} then {} else ", k * 2));
    }
    src.push_str(
        "0\nlet rec loop(i: Int32, acc: Int32): Int32 = if i == 0 then acc else loop(i - 1, acc + f(i % 10000))\n\
         echoln(show(loop(20000, 0)))\n",
    );
    same_on_both(&diktor, &d, "hotif", &src, &[]);
    // 6,000 個の値を生かす直線の関数を 2,000 回呼ぶ(大きなフレームの形)。
    let lets: Vec<String> = (0..6000)
        .map(|i| format!("let v{i} = n * {i} + 1"))
        .collect();
    let sum: Vec<String> = (0..6000).map(|i| format!("v{i}")).collect();
    same_on_both(
        &diktor,
        &d,
        "hotlets",
        &hot_src("", &lets, &sum.join(" + "), 2000, ""),
        &[],
    );
    // 1,400 個の perform を持つ関数を、T の節の下で 3,000 回呼ぶ。
    let lets: Vec<String> = (0..1400)
        .map(|i| format!("let v{i} = perform ask({i}) + n"))
        .collect();
    let sum: Vec<String> = (0..1400).map(|i| format!("v{i}")).collect();
    same_on_both(
        &diktor,
        &d,
        "hotperform",
        &hot_src(
            "effect Ask = { ask: (Int32) => Int32 }\n",
            &lets,
            &sum.join(" + "),
            3000,
            " handle { case ask(x) => resume(x + 1) }",
        ),
        &[],
    );
    // 部分に分けた関数の末尾でない再帰。論理的な 1 段は部分の 1 つのフレームなので、深さの限度は変わらない。
    let mut src = String::from("let rec f(n: Int32): Int32 = if n == 0 then 0 else {\n");
    for k in 0..800 {
        src.push_str(&format!("  let a{k} = n * {k} + 1\n"));
    }
    src.push_str("  let r = f(n - 1)\n  r + 1 + 0 * (a0");
    for k in 1..800 {
        src.push_str(&format!(" + a{k}"));
    }
    src.push_str(")\n}\necholn(show(f(100000)))\n");
    same_on_both(&diktor, &d, "splitrec", &src, &[]);
    same_on_both(&diktor, &d, "splitrec", &src, &["--max-depth", "5000"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn g_の節の取り出しと再開を_interp_と比べる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    let d = scratch("gres");
    let gen_src = "effect Gen = { yield: (Int32) => Unit }\n\
               let rec count(i: Int32, n: Int32): Unit @ {Gen} = if i == n then () else { perform yield(i); count(i + 1, n) }\n";
    // 300 個の値を生かしたまま、既定の L の近くまで末尾でない resume を重ねる節。
    // 論理的な 1 段に節とハンドラと rt_resume のフレームが重なるので、節は予算(B_wasm からその 2 つを引いたもの)に収め、
    // 大きなフレームの形の節は小さな部分に分ける。
    // 予算を B_wasm のままにすると 150 個の値で、部分に分けずに大きなフレームの形にすると 180 個の値で、
    // 限度の近くで V8 の RangeError の 70 になった(interp は 0)。
    let lets: Vec<String> = (0..300)
        .map(|k| format!("let a{k} = x * {}", k + 3))
        .collect();
    let sum: Vec<String> = (0..300).map(|k| format!("a{k}")).collect();
    let src = format!(
        "{gen_src}let total(n: Int32): Int32 = count(0, n) handle {{ case yield(x) => {{ {}; let r = resume(()); r + 1 + ({}) - ({}) }} case return(u) => 0 }}\n\
         echoln(show(total(1048560)))\n",
        lets.join("; "),
        sum.join(" + "),
        sum.join(" + ")
    );
    same_on_both(&diktor, &d, "bigclause", &src, &[]);
    // 1,400 個の perform を持つ関数(部分に分ける)を G の節の下で 200 回呼ぶ。
    // どの perform も継続を取り出し、部分の再開の入口から続ける。
    let lets: Vec<String> = (0..1400)
        .map(|i| format!("let v{i} = perform ask({i}) + n"))
        .collect();
    let sum: Vec<String> = (0..1400).map(|i| format!("v{i}")).collect();
    same_on_both(
        &diktor,
        &d,
        "hotg",
        &hot_src(
            "effect Ask = { ask: (Int32) => Int32 }\n",
            &lets,
            &sum.join(" + "),
            200,
            " handle { case ask(x) => { let r = resume(x + 1); r } }",
        ),
        &[],
    );
    // 800 個の値を生かす部分に分けた関数と、170 個の値を生かす大きなフレームの形の関数を 1,000 段重ね、
    // その上で 50 回 yield する。どの継続も 1,000 段のフレームを持ち、再開のたびにすべてを再開の入口から続ける。
    for (name, nv) in [("splitg", 800), ("bigg", 170)] {
        let mut src = String::from(
            "effect Ask = { ask: (Int32) => Int32 }\n\
             let rec ys(i: Int32): Int32 @ {Ask} = if i == 0 then 0 else perform ask(i) + ys(i - 1)\n\
             let rec f(n: Int32): Int32 @ {Ask} = if n == 0 then ys(50) else {\n",
        );
        for k in 0..nv {
            src.push_str(&format!("  let a{k} = n * {k} + 1\n"));
        }
        src.push_str("  let r = f(n - 1)\n  r + 1 + 0 * (a0");
        for k in 1..nv {
            src.push_str(&format!(" + a{k}"));
        }
        src.push_str(
            ")\n}\necholn(show(f(1000) handle { case ask(x) => { let r = resume(x % 7); r } }))\n",
        );
        same_on_both(&diktor, &d, name, &src, &[]);
    }
    // 限度を超える再開(depth_limit)。継続を使っていないので、cancel 節が走る。
    let src = format!(
        "{gen_src}let t(n: Int32): Int32 = count(0, n) handle {{ case yield(x) => {{ let r = resume(()); r + 1 }} case return(u) => 0 case cancel => echoln(\"cancel\") }}\n\
         echoln(show(t(4000)))\necholn(show(t(6000)))\n"
    );
    same_on_both(&diktor, &d, "chainlimit", &src, &["--max-depth", "5000"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// 引数の数が 1〜`nk` のクロージャを値として呼びながら、`nv` 個の値を生かして `depth` 段まで再帰する `.kel`。
fn closures_src(nv: usize, nk: usize, depth: u64) -> String {
    let mut s = String::new();
    for k in 1..=nk {
        let ps: Vec<String> = (0..k).map(|i| format!("x{i}: Int32")).collect();
        let ps = ps.join(", ");
        // if の値にして、直の呼び出しでなく値の呼び出しにする。
        s.push_str(&format!(
            "let k{k} = if true then fn({ps}) => x0 else fn({ps}) => x0\n"
        ));
    }
    s.push_str("let rec f(n: Int32): Int32 = if n == 0 then 0 else {\n");
    for i in 1..=nv {
        s.push_str(&format!("  let a{i} = n * {i} + {i}\n"));
    }
    s.push_str("  let s0 = 0\n");
    for k in 1..=nk {
        let args = vec!["n"; k].join(", ");
        s.push_str(&format!("  let s{k} = s{} + k{k}({args})\n", k - 1));
    }
    s.push_str("  let r = f(n - 1)\n  r + 1 + 0 * (");
    let terms: Vec<String> = (1..=nv)
        .map(|i| format!("a{i}"))
        .chain([format!("s{nk}")])
        .collect();
    s.push_str(&terms.join(" + "));
    s.push_str(")\n}\n");
    s.push_str(&format!("echoln(show(f({depth})))\n"));
    s
}

#[test]
fn 多くの型のクロージャを呼ぶ関数の_1_段は_b_wasm_に収まる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    let d = scratch("clo");
    // 値を呼ぶ一時の局所をクロージャの型ごとに足していたときは、この関数の局所が 229 個になり、
    // 1 段が B_wasm を超えて、既定の L で L − 10 段の再帰が V8 の RangeError の 70 になった。
    // 翻訳は書き終えた関数の推定が B_wasm を超えたら 70 にするので、翻訳が通ることが 1 段の上限を確かめる。
    // 直した後の L − 10 段の実行は 36 秒かかる(記録の M94 の節)ので、ここでは深さ 10 万で比べる。
    same_on_both(&diktor, &d, "clo60", &closures_src(100, 60, 100_000), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

/// 引数が `n` 個の関数を、直の呼び出し、クロージャの呼び出し、メソッドの呼び出しで使う `.kel`。
/// 直の呼び出しとクロージャの呼び出しは、末尾でない再帰で `depth` 段まで降りる。
fn many_params_src(n: usize, depth: u64) -> String {
    let ps: Vec<String> = (0..n).map(|i| format!("a{i}: Int32")).collect();
    let ty = format!("({}) => Int32", vec!["Int32"; n].join(", "));
    let rot: Vec<String> = (2..n)
        .map(|i| format!("a{i}"))
        .chain(["a1".to_string()])
        .collect();
    let names: Vec<String> = (0..n).map(|i| format!("a{i}")).collect();
    let rest: Vec<String> = (1..n).map(|i| i.to_string()).collect();
    let mut s = String::new();
    s.push_str(&format!(
        "let rec f({}): Int32 = if a0 == 0 then a1 else a{} + f(a0 - 1, {})\n",
        ps.join(", "),
        n - 1,
        rot.join(", ")
    ));
    s.push_str(&format!("echoln(show(f({depth}, {})))\n", rest.join(", ")));
    s.push_str(&format!(
        "let rec g(k: {ty}, {}): Int32 = if a0 == 0 then a1 else a{} + k(a0 - 1, {})\n",
        ps.join(", "),
        n - 1,
        rot.join(", ")
    ));
    s.push_str(&format!(
        "let rec h({}): Int32 = g(h, {})\n",
        ps.join(", "),
        names.join(", ")
    ));
    s.push_str(&format!("echoln(show(h({depth}, {})))\n", rest.join(", ")));
    s.push_str(&format!(
        "type class Big[A] {{ val big: (A, {}) => Int32 }}\n",
        vec!["Int32"; n - 1].join(", ")
    ));
    let bs: Vec<String> = (1..n).map(|i| format!("b{i}")).collect();
    s.push_str(&format!(
        "type instance Big[Int32] {{ let big(x, {}) = x + b{} }}\n",
        bs.join(", "),
        n - 1
    ));
    s.push_str(&format!(
        "let callbig[A: Big](x: A): Int32 = big(x, {})\necholn(show(callbig(7)))\n",
        rest.join(", ")
    ));
    s
}

#[test]
fn 引数の多い関数を限度の近くまで再帰させる() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    let d = scratch("params");
    // 既定の L(2^20)で L − 10 段。引数が 32 個を超える分は配列にまとめて渡す。
    // まとめる前は、引数が 60 個を超えると V8 のスタックが L より前に尽きた。
    // 深い再帰の間の確保は V8 の回収に深いスタックを走査させて遅い(64 個で約 15 秒)ので、ほかの数は浅く確かめる。
    same_on_both(
        &diktor,
        &d,
        "params64",
        &many_params_src(64, 1_048_566),
        &[],
    );
    for n in [33, 100] {
        same_on_both(
            &diktor,
            &d,
            &format!("params{n}"),
            &many_params_src(n, 10_000),
            &[],
        );
    }
    // Wasm の関数の引数の数の上限(1,000)を超える関数。
    same_on_both(&diktor, &d, "params1001", &many_params_src(1001, 10), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 取れないスタックの深さの限度は_70_になる() {
    if !have_node() {
        return;
    }
    let num = root().join("tests/fixtures/run/num.kir");
    let num = num.to_str().unwrap();
    for b in ["wasm", "interp"] {
        let o = keleut_in(
            &root(),
            &["run", "--backend", b, "--max-depth", "100000000", num],
        );
        assert_eq!(
            o.status.code(),
            Some(70),
            "{b}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(String::from_utf8_lossy(&o.stderr).contains("内部エラー"));
    }
}

#[test]
fn 仮想メモリの上限の下でも止まったままにならない() {
    if !have_node() {
        return;
    }
    let num = root().join("tests/fixtures/run/num.kir");
    let k = env!("CARGO_BIN_EXE_keleut");
    // 2 GiB では Worker のスタックを取れずに 70 になる。
    // 8 GiB では走り切るはずだが(この機械で 0)、V8 の予約の量に依るので 70 も許す。
    // どちらも型の誤りの 1 や、時間切れ(124)にはならない。
    for (v, want) in [(2_097_152, &[70][..]), (8_388_608, &[0, 70][..])] {
        let o = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "ulimit -v {v}; exec timeout 60 '{k}' run --backend wasm '{}'",
                num.display()
            ))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let c = o.status.code().unwrap_or(-1);
        assert!(
            want.contains(&c),
            "ulimit -v {v}: 終了状態 {c}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

#[test]
fn シグナルで終わっても一時の場所と_node_を残さない() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    use std::os::unix::process::ExitStatusExt;
    let d = scratch("signal");
    let tmp = d.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let kel = d.join("inf.kel");
    let kir = d.join("inf.kir");
    std::fs::write(
        &kel,
        "let rec loop(n: Int64): Int64 = loop(n + 1i64)\necholn(show(loop(0i64)))\n",
    )
    .unwrap();
    let st = Command::new(&diktor)
        .arg("--emit-ir")
        .arg(&kir)
        .arg(&kel)
        .status()
        .unwrap();
    assert!(st.success());
    for sig in [15, 9] {
        let mut c = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(["run", "--backend", "wasm"])
            .arg(&kir)
            .env("TMPDIR", &tmp)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        Command::new("kill")
            .arg(format!("-{sig}"))
            .arg(c.id().to_string())
            .status()
            .unwrap();
        let s = c.wait().unwrap();
        assert_eq!(s.signal(), Some(sig));
        std::thread::sleep(std::time::Duration::from_millis(500));
        let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().collect();
        assert!(left.is_empty(), "シグナル {sig} の後に残った: {left:?}");
        let ps = Command::new("pgrep")
            .arg("-f")
            .arg(tmp.to_str().unwrap())
            .output()
            .unwrap();
        assert!(
            ps.stdout.is_empty(),
            "シグナル {sig} の後に残った node: {}",
            String::from_utf8_lossy(&ps.stdout)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 無視する扱いで起動されたシグナルでは_interp_と同じく終わらない() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    if !have_node() {
        return;
    }
    let d = scratch("ignored-signal");
    let kel = d.join("slow.kel");
    let kir = d.join("slow.kir");
    std::fs::write(
        &kel,
        "let rec down(n: Int64): Int64 = if n == 0i64 then 0i64 else down(n - 1i64)\n\
         echoln(show(down(50000000i64)))\necholn(show(down(50000000i64)))\n",
    )
    .unwrap();
    let st = Command::new(&diktor)
        .arg("--emit-ir")
        .arg(&kir)
        .arg(&kel)
        .status()
        .unwrap();
    assert!(st.success());
    // nohup の下の SIGHUP と、シェルの台本の中の後ろで走る仕事の SIGINT。
    for sig in ["HUP", "INT"] {
        let run = |backend: &str| {
            Command::new("sh")
                .arg("-c")
                .arg(format!("trap '' {sig}; exec \"$0\" \"$@\""))
                .arg(env!("CARGO_BIN_EXE_keleut"))
                .args(["run", "--backend", backend])
                .arg(&kir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        };
        let cs = [run("interp"), run("wasm")];
        std::thread::sleep(std::time::Duration::from_millis(300));
        for c in &cs {
            Command::new("kill")
                .arg(format!("-{sig}"))
                .arg(c.id().to_string())
                .status()
                .unwrap();
        }
        let [i, w] = cs.map(|c| c.wait_with_output().unwrap());
        assert_eq!(i.status.code(), Some(0), "SIG{sig} の interp");
        assert_eq!(
            (&w.stdout, &w.stderr, w.status.code()),
            (&i.stdout, &i.stderr, i.status.code()),
            "SIG{sig} の wasm の標準エラー: {}",
            String::from_utf8_lossy(&w.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `let a0 = V * 3; …` を `nv` 個と、それらを使う式(値は 0)。
fn live_lets(var: &str, nv: usize) -> (String, String) {
    if nv == 0 {
        return (String::new(), "0".into());
    }
    let ls: String = (0..nv)
        .map(|i| format!("let a{i} = {var} * {}; ", i + 3))
        .collect();
    let s = (0..nv)
        .map(|i| format!("a{i}"))
        .collect::<Vec<_>>()
        .join(" + ");
    (ls, format!("({s}) - ({s})"))
}

/// `cancel` 節の中に直に書いたハンドラ式を重ねるプログラム(計画 261104-3 の探りの `gen.py` の形 c と形 f)。
///
/// 形 c(`GADD=1`)では、`go` が値を生かしてハンドラ式を末尾でなく書き、その `cancel` 節が値を生かして
/// ハンドラ式を直に書き、その本体が値を生かして `go` を末尾でなく呼ぶ。
/// 論理的な同じ深さに `go`、`$rt_handle`、`cancel` 節、内側の `$rt_handle` が重なる。
/// 形 f では、内側のハンドラ式に G の節 `ask` を足し、本体が `ask` を `perform` してから `go` を呼ぶ。
/// G の節は末尾で `resume` するので、内側の `$rt_handle` は `rt_resume` として本体を写し戻す。
/// どちらも 1 つの `n` が論理的な 2 段で、出力は形 c が `2`、`bottom`、`7`、形 f が `bottom`、`7` である。
fn cancel_nest_src(shape: char, nv: usize, n: u32) -> String {
    let (lm, cm) = live_lets("m", nv);
    let (lk, ck) = live_lets("k", nv);
    let mut s = String::new();
    if shape == 'c' {
        s.push_str("effect Stop = { stop: () => Int32 }\neffect Zz = { zz: () => Unit }\n");
        s.push_str("effect Ask = { ask: () => Int32 }\n");
        s.push_str(
            "echoln(show((perform ask()) handle { case ask() => { let r = resume(1); r + 1 } }))\n",
        );
        s.push_str(&format!(
            "let rec go(m: Int32): Int32 @ {{Stop, Console}} = if m == 0 then {{ echoln(\"bottom\"); 0 }} else \
             {{ {lm}let q = (perform stop()) handle {{ case zz() => resume(()) \
             case cancel => {{ {lm}let r = {{ let k = m - 1; {lk}let z = go(k); z + ({ck}) }} handle {{ case stop() => 0 }}; \
             if r + ({cm}) != 0 then echoln(\"bad\") else () }} }}; q + ({cm}) }}\n"
        ));
        s.push_str(&format!(
            "echoln(show(go({n}) handle {{ case stop() => 7 }}))\n"
        ));
    } else {
        s.push_str(
            "effect Stop = { stop: () => Int32, ask: () => Int32 }\neffect Zz = { zz: () => Unit }\n",
        );
        s.push_str(&format!(
            "let rec go(m: Int32): Int32 @ {{Stop, Console}} = if m == 0 then {{ echoln(\"bottom\"); 0 }} else \
             {{ {lm}let q = (perform stop()) handle {{ case zz() => resume(()) \
             case cancel => {{ {lm}let r = {{ let k = m - 1; {lk}let a = perform ask(); let z = go(k); z + a - 2 + ({ck}) }} \
             handle {{ case stop() => 0 case ask() => if m < 0 then {{ let t = resume(1); t }} else resume(2) }}; \
             if r + ({cm}) != 0 then echoln(\"bad\") else () }} }}; q + ({cm}) }}\n"
        ));
        s.push_str(&format!(
            "echoln(show(go({n}) handle {{ case stop() => 7 case ask() => 5 }}))\n"
        ));
    }
    s
}

/// `cancel` 節の中のハンドラ式を `--max-depth 600000` の限度の近く(論理的な深さ約 598,000)まで重ねる。
fn deep_cancel_nest(d: &Path) -> PathBuf {
    let kel = d.join("c92.kel");
    std::fs::write(&kel, cancel_nest_src('c', 92, 299_000)).unwrap();
    kel
}

fn run_depth(d: &Path, args: &[&str], kel: &Path) -> (Vec<u8>, Vec<u8>, Option<i32>) {
    let mut a = vec!["run"];
    a.extend_from_slice(args);
    a.extend_from_slice(&["--max-depth", "600000", kel.to_str().unwrap()]);
    let o = keleut_in(d, &a);
    (o.stdout, o.stderr, o.status.code())
}

#[test]
fn cancel_節の中のハンドラ式を重ねても_interp_と同じ() {
    if !have_node() {
        return;
    }
    // 1 段が B_wasm を超える形(V234)。スタックは B_prog で決まり、--stack-size の直の経路で走る。
    let d = scratch("cancel-nest");
    let kel = deep_cancel_nest(&d);
    let w = run_depth(&d, &["--backend", "wasm"], &kel);
    let i = run_depth(&d, &["--backend", "interp"], &kel);
    assert_eq!(
        (&i.0[..], i.2),
        (&b"2\nbottom\n7\n"[..], Some(0)),
        "{}",
        String::from_utf8_lossy(&i.1)
    );
    assert_eq!(w, i, "{}", String::from_utf8_lossy(&w.1));
    // 直す前の keleut は Worker の経路でなら host の 64 MiB の余裕で通るので、直の経路を通ったことを確かめる。
    // 仮想メモリの上限などで直の経路を取れない環境では、この試験は直したことを見分けられない。
    let stats = wasm_stats_stderr(&d, &kel);
    assert!(
        stats.contains("keleut-host: path=direct\n"),
        "直の経路を通らなかった(RLIMIT_AS が有限か、RLIMIT_STACK の上限が低い): {stats}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `KELEUT_WASM_STATS=1` で `keleut run --backend wasm --max-depth 600000` を走らせた標準エラー。
fn wasm_stats_stderr(d: &Path, kel: &Path) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args([
            "run",
            "--backend",
            "wasm",
            "--max-depth",
            "600000",
            kel.to_str().unwrap(),
        ])
        .current_dir(d)
        .env("KELEUT_WASM_STATS", "1")
        .env_remove("KELEUT_WASM_DIRECT")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[cfg(feature = "llvm")]
#[test]
fn cancel_節の中のハンドラ式を重ねても_jit_only_と同じ() {
    if !have_node() {
        return;
    }
    let d = scratch("cancel-nest-jit");
    let kel = deep_cancel_nest(&d);
    let w = run_depth(&d, &["--backend", "wasm"], &kel);
    let j = run_depth(&d, &["--backend", "jit", "--tier", "jit-only"], &kel);
    assert_eq!(
        (&j.0[..], j.2),
        (&b"2\nbottom\n7\n"[..], Some(0)),
        "{}",
        String::from_utf8_lossy(&j.1)
    );
    assert_eq!(w, j, "{}", String::from_utf8_lossy(&w.1));
    let stats = wasm_stats_stderr(&d, &kel);
    assert!(
        stats.contains("keleut-host: path=direct\n"),
        "直の経路を通らなかった(RLIMIT_AS が有限か、RLIMIT_STACK の上限が低い): {stats}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `keleut build --target wasm --max-depth 600000` で書く。
fn build_600000(d: &Path, kel: &Path, out: &Path) {
    build_at(d, kel, out, "600000");
}

/// `KELEUT_WASM_STATS=1 keleut build --target wasm --max-depth DEPTH` で書き、標準エラーを返す。
fn build_at(d: &Path, kel: &Path, out: &Path, depth: &str) -> String {
    let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .args([
            "build",
            "--target",
            "wasm",
            "--max-depth",
            depth,
            "-o",
            out.to_str().unwrap(),
            kel.to_str().unwrap(),
        ])
        .current_dir(d)
        .env("KELEUT_WASM_STATS", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&r.stderr).into_owned();
    assert_eq!(r.status.code(), Some(0), "{err}");
    err
}

#[test]
fn worker_の経路のスタックは_b_prog_で決まる() {
    if !have_node() {
        return;
    }
    let d = scratch("cancel-nest-meta");
    let kel = d.join("c.kel");
    std::fs::write(&kel, cancel_nest_src('c', 92, 1000)).unwrap();
    let out = d.join("out");
    build_600000(&d, &kel, &out);
    let main = std::fs::read_to_string(out.join("main.mjs")).unwrap();
    // k = 1 なので B_prog = 1,040 + 320 + 1,040 + 320 + 176 = 2,896。
    assert!(
        main.contains("\"bProg\":2896,\"cancelNest\":1,"),
        "META に bProg と cancelNest が無い"
    );
    let r = Command::new(node())
        .arg(out.join("main.mjs"))
        .env_remove("KELEUT_WASM_DIRECT")
        .env("KELEUT_WASM_STATS", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(r.stdout, b"2\nbottom\n7\n");
    assert_eq!(r.status.code(), Some(0));
    // (L + R) × B_prog + 32 MiB を KiB に切り上げ、MiB に切り上げて 64 MiB を足す。
    let (l, b) = (600_000u64, 2896u64);
    let kib = ((l + (l / 64).max(1024)) * b + (32 << 20)).div_ceil(1024);
    let mb = kib.div_ceil(1024) + 64;
    assert_eq!(mb, 1779);
    assert_eq!(
        String::from_utf8_lossy(&r.stderr),
        format!("keleut-host: path=worker stackSizeMb={mb}\n")
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn worker_の経路で_g_の節の再開を重ねても_interp_と同じ() {
    if !have_node() {
        return;
    }
    // 形 f: 内側の $rt_handle が rt_resume として本体を写し戻す。
    let d = scratch("cancel-nest-worker");
    let kel = d.join("f92.kel");
    std::fs::write(&kel, cancel_nest_src('f', 92, 299_000)).unwrap();
    let out = d.join("out");
    build_600000(&d, &kel, &out);
    let r = Command::new(node())
        .arg(out.join("main.mjs"))
        .env_remove("KELEUT_WASM_DIRECT")
        .env_remove("KELEUT_WASM_STATS")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let i = run_depth(&d, &["--backend", "interp"], &kel);
    assert_eq!(
        (&i.0[..], i.2),
        (&b"bottom\n7\n"[..], Some(0)),
        "{}",
        String::from_utf8_lossy(&i.1)
    );
    assert_eq!(
        (r.stdout, r.stderr, r.status.code()),
        i,
        "node DIR/main.mjs と keleut run --backend interp"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn worker_のスタックは_stack_cap_で切る() {
    if !have_node() {
        return;
    }
    // k = 1、L = 6,000,000 では (L + R) × B_prog + 32 MiB が STACK_CAP(16 GiB)を超える。
    let d = scratch("cancel-nest-cap");
    let kel = d.join("c.kel");
    std::fs::write(&kel, cancel_nest_src('c', 92, 1000)).unwrap();
    let out = d.join("out");
    let err = build_at(&d, &kel, &out, "6000000");
    assert!(
        err.contains("k = 1、B_prog = 2896 バイト、r = 0、スタックを STACK_CAP で切った\n"),
        "{err}"
    );
    let r = Command::new(node())
        .arg(out.join("main.mjs"))
        .env_remove("KELEUT_WASM_DIRECT")
        .env("KELEUT_WASM_STATS", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    // 16 GiB に host の 64 MiB を足した値。
    assert_eq!(
        String::from_utf8_lossy(&r.stderr),
        "keleut-host: path=worker stackSizeMb=16448\n"
    );
    let i = keleut_in(
        &d,
        &[
            "run",
            "--backend",
            "interp",
            "--max-depth",
            "6000000",
            kel.to_str().unwrap(),
        ],
    );
    assert_eq!(
        (&i.stdout[..], i.status.code()),
        (&b"2\nbottom\n7\n"[..], Some(0))
    );
    assert_eq!((r.stdout, r.status.code()), (i.stdout, i.status.code()));
    // STACK_CAP が k = 0 の大きさより小さい深さでは、k = 0 の大きさで切る(書くだけで走らせない)。
    let out2 = d.join("out2");
    let err = build_at(&d, &kel, &out2, "2000000000");
    assert!(
        err.contains("、スタックを k = 0 の大きさで切った\n"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `return` 節が値を生かして `go` を末尾でなく呼ぶプログラム(実施記録 261104-4 の M106 の形 R)。
///
/// `go` は値を `nv` 個生かしてハンドラ式を末尾でなく書き、その `return` 節は値を `nv2` 個生かして `go` を呼ぶ。
/// `$rt_handle` が `return` 節を末尾で呼んで置き換えるので、論理的な同じ深さに `go` と `return` 節が重なる。
/// 1 つの `n` が論理的な 1 段で、出力は `2`、`bottom`、`0` である。
fn ret_clause_src(nv: usize, nv2: usize, n: u32) -> String {
    let (lm, cm) = live_lets("m", nv);
    let (lx, cx) = live_lets("x", nv2);
    let mut s = String::new();
    s.push_str("effect Stop = { stop: () => Int32 }\neffect Zz = { zz: () => Unit }\n");
    s.push_str("effect Ask = { ask: () => Int32 }\n");
    s.push_str(
        "echoln(show((perform ask()) handle { case ask() => { let r = resume(1); r + 1 } }))\n",
    );
    s.push_str(&format!(
        "let rec go(m: Int32): Int32 @ {{Console}} = if m == 0 then {{ echoln(\"bottom\"); 0 }} else \
         {{ {lm}let q = (m) handle {{ case zz() => resume(()) \
         case return(x) => {{ {lx}let z = go(x - 1); z + ({cx}) }} }}; q + ({cm}) }}\n"
    ));
    s.push_str(&format!("echoln(show(go({n})))\n"));
    s
}

#[test]
fn return_節が重なっても既定の限度の内側で_interp_と同じ() {
    if !have_node() {
        return;
    }
    // 1 段が約 1,801 バイトで B_wasm を超える形。直す前の keleut は既定の L の内側の深さ 1,000,000 で 70 になった。
    let d = scratch("ret-overlap");
    let kel = d.join("r.kel");
    std::fs::write(&kel, ret_clause_src(92, 155, 1_000_000)).unwrap();
    let run = |backend: &str| {
        let o = keleut_in(&d, &["run", "--backend", backend, kel.to_str().unwrap()]);
        (o.stdout, o.stderr, o.status.code())
    };
    let i = run("interp");
    assert_eq!(
        (&i.0[..], i.2),
        (&b"2\nbottom\n0\n"[..], Some(0)),
        "{}",
        String::from_utf8_lossy(&i.1)
    );
    let w = run("wasm");
    assert_eq!(w, i, "{}", String::from_utf8_lossy(&w.1));
    let _ = std::fs::remove_dir_all(&d);
}

/// 同じ深さに重なる節の鎖のプログラム(M106 の検証の探りの `rch.py` と `nch.py`)。
///
/// `never` が偽なら、`go` の書くハンドラ式の `return` 節が、値を生かしてハンドラ式を直に書き、
/// その `return` 節がさらにハンドラ式を書くことを `chain` 段まで重ね、最後の `return` 節が `go` を呼ぶ。
/// `never` が真なら、`return` 節の代わりに分類 N の操作節 `q` を重ねる。
/// どちらの節も `$rt_handle` が末尾で呼んで自分を置き換えるので、鎖の節はすべて同じ深さに重なる。
/// `gadd` が真なら、先頭に G の節を持つハンドラ式を 1 つ書く。
fn clause_chain_src(
    never: bool,
    gadd: bool,
    chain: usize,
    nv: usize,
    nv2: usize,
    n: u32,
) -> String {
    let lets = |v: &str, k: usize| -> (String, String) {
        if k == 0 {
            return (String::new(), "0".into());
        }
        let ls: String = (0..k)
            .map(|i| format!("let {v}{i} = {v} * {}; ", i + 3))
            .collect();
        let s = (0..k)
            .map(|i| format!("{v}{i}"))
            .collect::<Vec<_>>()
            .join(" + ");
        (ls, format!("({s}) - ({s})"))
    };
    let mut out = vec![if never {
        "effect Q = { q: (Int32) => Never }".to_string()
    } else {
        "effect Zz = { zz: () => Unit }".to_string()
    }];
    if gadd {
        out.push("effect Ask = { ask: () => Int32 }".into());
        out.push(
            "echoln(show((perform ask()) handle { case ask() => { let r = resume(1); r + 1 } }))"
                .into(),
        );
    }
    // 節の頭(`x{j}` を受け取る節を持つハンドラ式の、節の本体の前まで)。
    let head = |subject: &str, j: usize| {
        if never {
            format!("(perform q({subject})) handle {{ case q(x{j}) => ")
        } else {
            format!("({subject}) handle {{ case zz() => resume(()) case return(x{j}) => ")
        }
    };
    let (lm, cm) = lets("m", nv);
    let mut inner = String::new();
    for j in (0..chain).rev() {
        let v = format!("x{j}");
        let (lx, cx) = lets(&v, nv2);
        let body = if j == chain - 1 {
            format!("go({v} - 1)")
        } else {
            format!("{}{inner} }}", head(&v, j + 1))
        };
        inner = format!("{{ {lx}let z = {body}; z + ({cx}) }}");
    }
    out.push(format!(
        "let rec go(m: Int32): Int32 @ {{Console}} = if m == 0 then {{ echoln(\"bottom\"); 0 }} else \
         {{ {lm}let q = {}{inner} }}; q + ({cm}) }}",
        head("m", 0)
    ));
    out.push(format!("echoln(show(go({n})))"));
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// 節の鎖のプログラムを `--backend wasm` と `--backend interp` で走らせ、同じであることを確かめる。
///
/// `max_depth` が `None` なら既定の L で走らせる。
fn clause_chain_same(tag: &str, src: String, max_depth: Option<&str>, expect: &[u8]) {
    let d = scratch(tag);
    let kel = d.join("c.kel");
    std::fs::write(&kel, src).unwrap();
    let run = |backend: &str| {
        let mut args = vec!["run", "--backend", backend];
        if let Some(l) = max_depth {
            args.extend(["--max-depth", l]);
        }
        args.push(kel.to_str().unwrap());
        let o = keleut_in(&d, &args);
        (o.stdout, o.stderr, o.status.code())
    };
    let i = run("interp");
    assert_eq!(
        (&i.0[..], i.2),
        (expect, Some(0)),
        "{}",
        String::from_utf8_lossy(&i.1)
    );
    let w = run("wasm");
    assert_eq!(w, i, "{}", String::from_utf8_lossy(&w.1));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn return_節の鎖が重なっても既定の限度の内側で_interp_と同じ() {
    if !have_node() {
        return;
    }
    // return 節の鎖 6 段(r = 6)。直す前の keleut と、r を真偽で数えた版(B_prog = 2,752)は、深さ約 1,000,000 で 70 になった。
    clause_chain_same(
        "ret-chain",
        clause_chain_src(false, false, 6, 60, 60, 1_000_000),
        None,
        b"bottom\n0\n",
    );
}

#[test]
fn n_の節の鎖が重なっても既定の限度の内側で_interp_と同じ() {
    if !have_node() {
        return;
    }
    // 分類 N の操作節の鎖 6 段(r = 6)。1 段は約 4,194 バイトである。
    // 直す前の keleut は B_prog = B_wasm のまま直の経路で走り、L = 400,000 で深さ約 399,000 の入力が 70 になった。
    // 既定の L と深さ 1,000,000 でも同じく 70 になるが、kw で 60 秒かかるので L を下げた。
    clause_chain_same(
        "never-chain",
        clause_chain_src(true, true, 6, 60, 60, 399_000),
        Some("400000"),
        b"2\nbottom\n0\n",
    );
}

/// `cancel` 節と、分類 N の操作節(`never` が偽なら `return` 節)が交互に重なるプログラム(M106 の再検証の探りの `gen3.py`)。
///
/// `go` の書くハンドラ式の `cancel` 節が、N の節を持つハンドラ式を直に書く。
/// その N の節は、`cancel` 節を持つハンドラ式を直に書き、その `cancel` 節がさらに N の節を持つハンドラ式を書く。
/// これを `d` 組重ね、最後の `cancel` 節の中のハンドラ式の本体が `go` を末尾でなく呼ぶ。
/// 道の節はすべて同じ深さに重なる。
/// 1 つの `n` が論理的な 2 段で、出力は `2`、`bottom`、`7` である(`cancel` 節の中の `perform` の抑制を標準エラーに書く)。
fn alt_chain_src(never: bool, d: usize, nv: usize, n: u32) -> String {
    let lets = |var: &str, p: &str| -> (String, String) {
        if nv == 0 {
            return (String::new(), "0".into());
        }
        let ls: String = (0..nv)
            .map(|i| format!("let {p}{i} = {var} * {}; ", i + 3))
            .collect();
        let s = (0..nv)
            .map(|i| format!("{p}{i}"))
            .collect::<Vec<_>>()
            .join(" + ");
        (ls, format!("(({s}) - ({s}))"))
    };
    let mut out = vec![
        "effect Stop = { stop: () => Int32 }".to_string(),
        "effect Zz = { zz: () => Unit }".into(),
        "effect Q = { q: (Int32) => Never }".into(),
        "effect Ask = { ask: () => Int32 }".into(),
        "echoln(show((perform ask()) handle { case ask() => { let r = resume(1); r + 1 } }))"
            .into(),
    ];
    let (lm, cm) = lets("m", "a");
    let (lf, cf) = lets("m", "b");
    let (lk, ck) = lets("k", "d");
    let mut expr =
        format!("{{ let k = x - 1; {lk}let z = go(k); z + {ck} }} handle {{ case stop() => 0 }}");
    for i in 0..d {
        let (lx, cx) = lets("x", &format!("c{i}_"));
        let (ly, cy) = lets("x", &format!("e{i}_"));
        let canc = format!(
            "{{ let s = perform stop(); s }} handle {{ case zz() => resume(()) case cancel => \
             {{ {ly}let r3 = {expr}; if r3 + {cy} != 0 then echoln(\"bad\") else () }} }}"
        );
        let body = format!("{{ {lx}let r2 = {canc}; r2 + {cx} }}");
        expr = if never {
            format!("(perform q(x)) handle {{ case q(x) => {body} }}")
        } else {
            format!("(x) handle {{ case zz() => resume(()) case return(x) => {body} }}")
        };
    }
    out.push(format!(
        "let rec go(m: Int32): Int32 @ {{Stop, Console}} = if m == 0 then {{ echoln(\"bottom\"); 0 }} else \
         {{ {lm}let q = (perform stop()) handle {{ case zz() => resume(()) case cancel => \
         {{ {lf}let r = {{ let x = m; {expr} }}; if r + {cf} != 0 then echoln(\"bad\") else () }} }}; q + {cm} }}"
    ));
    out.push(format!(
        "echoln(show(go({n}) handle {{ case stop() => 7 }}))"
    ));
    let mut s = out.join("\n");
    s.push('\n');
    s
}

#[test]
fn cancel_節と_n_の節が交互に重なっても_interp_と同じ() {
    if !have_node() {
        return;
    }
    // cancel 節と N の節を交互に 4 組重ねる(k = 5、r = 4、B_prog = 14,160 バイト)。1 段は約 3,543 バイトである。
    // k と r を別々に数えた版は B_prog = 2,896 のままで、L = 300,000 で深さ約 298,000 の入力が 70 になった。
    clause_chain_same(
        "alt-chain",
        alt_chain_src(true, 4, 40, 149_000),
        Some("300000"),
        b"2\nbottom\n7\n",
    );
}

/// 関数 `nm` の中で、`{a = i, b{i % 7} = 1 extends s{i-1}}` の拡張を `n` 段重ねる(計画 261104-3 の探りの
/// `gen_chain.py` の `fun` の形)。
/// 長さ i の形を i 個作るので、形の表のラベルの合計は約 n² になる(V240)。
fn shape_chain_fn(nm: &str, a: &str, b: &str, n: u32) -> String {
    let mut s = format!("let {nm}(): Int32 = {{\n  let s0 = {{{a} = 0}}\n");
    for i in 1..=n {
        s.push_str(&format!(
            "  let s{i} = {{{a} = {i}, {b}{} = 1 extends s{}}}\n",
            i % 7,
            i - 1
        ));
    }
    s.push_str(&format!("  s{n}.{a}\n}}\n"));
    s
}

/// `cancel` 節の中で何をするか(`shape_oom_src`)。
#[derive(Clone, Copy, Debug, PartialEq)]
enum InCancel {
    /// `cancel` を書くだけ。
    Light,
    /// 別のラベルの鎖を作り、もう一度形の表を広げようとする。
    Again,
    /// 2 MiB の文字列を書く(host の受け渡しの領域の 64 KiB より長い)。
    Big,
}

/// 形の表の鎖を `cancel` 節を持つハンドラの中で呼ぶプログラム。
/// その外側にも `cancel` 節を持つハンドラを置き、巻き戻しが続くことを見る。
fn shape_oom_src(c: InCancel) -> String {
    let mut s = String::from("effect Res = { use: () => Unit }\n");
    s.push_str(&shape_chain_fn("f", "a", "b", 1000));
    s.push_str(&shape_chain_fn("g", "c", "d", 1000));
    s.push_str(
        "let rec rep(s: String, n: Int32): String = if n == 0 then s else rep(s + s, n - 1)\n",
    );
    let inner = match c {
        InCancel::Light => "echoln(\"cancel\")",
        InCancel::Again => "{ echoln(\"cancel\"); echoln(show(g())) }",
        InCancel::Big => "{ echoln(\"cancel\"); echoln(rep(\"a\", 21)) }",
    };
    s.push_str(&format!(
        "let main(): Int32 = run h {{ f() }} handle {{ case use() => resume(()) case cancel => {inner} }}\n"
    ));
    s.push_str(
        "let outer(): Int32 = run h2 { main() } handle { case use() => resume(()) case cancel => echoln(\"outer\") }\n",
    );
    s.push_str("echoln(\"before\")\necholn(show(outer()))\n");
    s
}

#[test]
fn 線形メモリを広げられず形の表が満ちたらメモリの不足になる() {
    if !have_node() {
        return;
    }
    // 試験用の KELEUT_TEST_WASM_MEM_GROW で、線形メモリを起動の後に 1 MiB までしか広げさせない。
    // host の受け渡しの領域の 64 KiB は起動のときに取るので上限に数えず、形の表を 2 倍に広げるところで尽きる。
    // 直す前は、host の alloc が「線形メモリを広げられません」の内部エラー(70)で終わっていた。
    // cancel 節の中でもう一度表が満ちたときは、cancel 節からの脱出として抑制の行を書き、巻き戻しを続ける。
    // 形の表が線形メモリを使い切った後でも、cancel 節は受け渡しの領域を広げずに 2 MiB の文字列を書ける。
    // 前は受け渡しの領域を広げられず、「線形メモリを広げられません」の内部エラー(70)で cancel 節の出力が失われていた。
    let d = scratch("shape-oom");
    for c in [InCancel::Light, InCancel::Again, InCancel::Big] {
        let kel = d.join(format!("s{c:?}.kel"));
        std::fs::write(&kel, shape_oom_src(c)).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(["run", "--backend", "wasm", kel.to_str().unwrap()])
            .current_dir(&d)
            .env("KELEUT_TEST_WASM_MEM_GROW", "1048576")
            .env_remove("KELEUT_WASM_STATS")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let want_stderr = if c == InCancel::Again {
            "cancel 節で例外が抑制されました: メモリ不足です\n実行時エラー: メモリ不足です\n"
        } else {
            "実行時エラー: メモリ不足です\n"
        };
        let mut want_stdout = b"before\ncancel\n".to_vec();
        if c == InCancel::Big {
            want_stdout.extend(std::iter::repeat_n(b'a', 1 << 21));
            want_stdout.push(b'\n');
        }
        want_stdout.extend(b"outer\n");
        assert!(
            o.stdout == want_stdout
                && String::from_utf8_lossy(&o.stderr) == want_stderr
                && o.status.code() == Some(3),
            "{c:?}: stdout {} バイト、stderr {:?}、{:?}",
            o.stdout.len(),
            String::from_utf8_lossy(&o.stderr),
            o.status.code()
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}
