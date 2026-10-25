//! `keleut run --backend wasm` と `keleut build` の試験(計画 261024-1 の 6.10 節、M94)。
//!
//! Node(環境変数 `KELEUT_NODE`、無ければ PATH の `node`)が無ければ、何もせずに通る。
//!
//! - `tests/fixtures/run/` の各 `.kir` を WasmGC の後段で走らせ、記録した標準出力、標準エラー、終了状態と比べる
//!   (記録は `diktor --run-ir` の結果で、インタプリタの試験と同じものである)。
//! - `keleut build` の出力を `node DIR/main.mjs` だけで走らせる(`Worker` のスタックの経路)。
//! - 起動のときに閉じていた標準出力は、インタプリタと同じく 74 になる。
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
        for f in ["program.wasm", "rt.wasm", "main.mjs"] {
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
