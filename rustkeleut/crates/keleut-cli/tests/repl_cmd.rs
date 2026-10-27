//! `keleut repl` と `keleut run --frontend rust` の試験(M100)。
//!
//! 期待は diktor/test/repl.t などの `diktor --repl` の記録と同じである。
//! 環境変数 `KELEUT_DIKTOR` を与えると、コーパスの全件を `diktor --repl` と `keleut repl` の両方に標準入力として読ませ、
//! 標準出力と終了状態を比べる。
//! 同じく、コーパスの全件を `diktor` と `keleut run --frontend rust` で実行して比べる。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn keleut_in(dir: Option<&Path>, args: &[&str], input: &str) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_keleut"));
    c.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = dir {
        c.current_dir(d);
    }
    let mut ch = c.spawn().unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    ch.wait_with_output().unwrap()
}

fn repl(args: &[&str], input: &str) -> (String, Option<i32>) {
    let mut a = vec!["repl"];
    a.extend_from_slice(args);
    let r = keleut_in(None, &a, input);
    (
        String::from_utf8_lossy(&r.stdout).into_owned(),
        r.status.code(),
    )
}

fn check(input: &str, want: &str) {
    let (out, code) = repl(&[], input);
    assert_eq!(out, want, "入力:\n{input}");
    assert_eq!(code, Some(0));
}

#[test]
fn 前の入力の束縛を使える() {
    check(
        "let a = 1\nlet geta(): Int32 = a\nlet a = \"s\"\na\ngeta()\n",
        "a : Int32 = 1\ngeta : () => Int32 = <fn>\na : String = \"s\"\n_ : String = \"s\"\n_ : Int32 = 1\n",
    );
}

#[test]
fn 型エラーの入力は単一化の結果も戻る() {
    check(
        "let id[A](x: A): A = x\nlet f = id(fn(y) => y)\nlet g = f(1); let bad = 1 + \"a\"\nf(\"a\")\ng\n",
        "id : (A) => A = <fn>\nf : (_A) => _A = <fn>\n\
         ! <stdin>:3:25: 型エラー: String は Integral のインスタンスではありません\n\
         _ : String = \"a\"\n! <stdin>:5:1: 型エラー: 未束縛の変数: g\n",
    );
}

#[test]
fn 実行時エラーの入力は戻り_それまでの出力は残る() {
    check(
        "let a = 2\nlet a = 3; let z = 5; echoln(\"before\"); 1 / 0\na\nz\n",
        "a : Int32 = 2\nbefore\n実行時エラー: ゼロ除算です\n_ : Int32 = 2\n\
         ! <stdin>:4:1: 型エラー: 未束縛の変数: z\n",
    );
}

#[test]
fn インスタンスも入力の単位で戻る() {
    check(
        "type class Tg[X] { val tg: (X) => Int32 }\n\
         type instance Tg[Int32] { let tg(x: Int32): Int32 = 1 }; 1 / 0\ntg(5)\n",
        "実行時エラー: ゼロ除算です\n! <stdin>:3:1: 型エラー: Int32 は Tg のインスタンスではありません\n",
    );
}

#[test]
fn トップレベルの_println_を印字する() {
    check(
        "println(\"hello\")\nwith_stdout(fn() => println(\"inner\"))\n",
        "hello\ninner\n",
    );
    // インスタンスのメソッドの値の初期化も、トップレベルで Print を行える。
    check(
        "type class Sz[A] { val sz: (A) => Int32 }\nnewtype Foo = Foo\n\
         type instance Sz[Foo] { let sz = { perform print(\"init\"); fn(x: Foo) => 1i32 } }\n\
         echoln(\"x\")\necholn(show(sz(Foo)))\n",
        "initx\n1\n",
    );
}

#[test]
fn 続きの行を待つ() {
    check(
        "let h(n: Int32): Int32 = {\n  n * 2\n}\nh(21)\n\"a\nb\"\n",
        "h : (Int32) => Int32 = <fn>\n_ : Int32 = 42\n_ : String = \"a\\nb\"\n",
    );
    check("let x = (1 +\n", "<stdin>: 入力が途中で終わっています\n");
}

#[test]
fn コマンド() {
    check(
        ":type fn(a) => a\nlet id[A](x: A): A = x\n:type id(None)\n:type       zzz\nlet x = 1\n:reset\nx\n:bogus\n",
        "(A) => A\nid : (A) => A = <fn>\nOption[_A]\n! <stdin>:4:13: 型エラー: 未束縛の変数: zzz\n\
         x : Int32 = 1\n! <stdin>:7:1: 型エラー: 未束縛の変数: x\n未知のコマンド: :bogus\n",
    );
    check("let x = 1\n:q\r\nlet y = 2\n", "x : Int32 = 1\n");
}

#[test]
fn 値の表示() {
    check(
        "newtype Pt = Pt(Int32, Int32)\n\
         type instance Show[Pt] { let show(p) = p match { case Pt(x, y) => \"<\" + show(x) + \",\" + show(y) + \">\" } }\n\
         Pt(1, 2)\nnewtype Q = Q(Int32)\nQ(3)\nlet p = (show, 1)\nlet q = (show, ???)\n\
         let r = {z = 4 extends {x = 1, y = 2}}\n",
        "_ : Pt = <1,2>\n_ : Q = Q(3)\np : [A: Show] ((A) => String, Int32) = (<fn>, 1)\n\
         q : [A: Show] ((A) => String, B) = <unevaluated>\n\
         r : {z: Int32, x: Int32, y: Int32} = {z = 4, x = 1, y = 2}\n",
    );
}

#[test]
fn プレリュードなしと使い方の誤り() {
    let (out, code) = repl(
        &["--no-prelude"],
        "effect Print = { print: (Int32) => Int32 }\nperform print(1)\n",
    );
    assert_eq!(
        out,
        "! <stdin>:2:1: 型エラー: エフェクト Print をここでは実行できません(ラベル Print がありません(行は閉じています))\n"
    );
    assert_eq!(code, Some(0));
    let (_, code) = repl(&["x.kel"], "");
    assert_eq!(code, Some(64));
}

/// レコードのフィールドは型の行の順に表示する(V221)。
///
/// Diktor は値を作った順に表示するので、注釈の型とリテラルの順が違う束縛と、レコードの更新では Diktor と違う。
/// 注釈の無いリテラルと拡張では Diktor と同じである。
#[test]
fn レコードのフィールドは型の行の順に表示する() {
    // Diktor は k を {n = 1, m = 2}、t を {y = 1, x = 5} と表示する。
    check(
        "let k: {m: Int32, n: Int32} = {n = 1, m = 2}\nlet q = {y = 1, x = 2}\nlet t = {q with x = 5}\n",
        "k : {m: Int32, n: Int32} = {m = 2, n = 1}\nq : {y: Int32, x: Int32} = {y = 1, x = 2}\n\
         t : {x: Int32, y: Int32} = {x = 5, y = 1}\n",
    );
}

/// 試験のための一時のファイルを書き、そのパスを返す。
fn temp_file(name: &str, text: &str) -> String {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().into_owned()
}

fn repl_err(args: &[&str], input: &str) -> (String, String, Option<i32>) {
    let mut a = vec!["repl"];
    a.extend_from_slice(args);
    let r = keleut_in(None, &a, input);
    (
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
        r.status.code(),
    )
}

/// プレリュードの初期化は始めるときに実行し、止まれば標準エラーに書いて 3 で終わる。
/// プレリュードの型エラーは 1 で終わる(Diktor と同じ)。
#[test]
fn プレリュードの誤りでセッションを終える() {
    let rt = temp_file(
        "repl_prelude_div.kel",
        "extern \"prim\" let __int32_div(x: Int32, y: Int32): Int32\nlet z: Int32 = __int32_div(1, 0)\n",
    );
    for input in ["", "1\nlet q = 2\nq\n"] {
        let (out, err, code) = repl_err(&["--prelude", &rt], input);
        assert_eq!(
            (out.as_str(), err.as_str()),
            ("", "実行時エラー: ゼロ除算です\n")
        );
        assert_eq!(code, Some(3));
    }
    let te = temp_file("repl_prelude_type.kel", "newtype T = A\nnewtype T = B\n");
    let (out, err, code) = repl_err(&["--prelude", &te], "1\n");
    assert_eq!(out, "");
    assert!(
        err.ends_with("型エラー: 標準環境の newtype T は再宣言できません\n"),
        "{err}"
    );
    assert_eq!(code, Some(1));
}

/// トップレベルの `Print` は、プレリュードの `with_stdout` に依らず標準出力に印字する。
#[test]
fn トップレベルの_print_はプレリュードの_with_stdout_に依らない() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../diktor/lib/prelude.kel"),
    )
    .unwrap();
    let ws = "let with_stdout[A, E](body: () => A @ {Print, Console extends E}): A @ {Console extends E} =\n  \
              body() handle {\n    case print(message) => resume(perform write(message))\n    case return(x) => x\n  }\n";
    assert!(src.contains(ws));
    let input = "println(\"x\")\nlet f(): Unit @ Print = println(\"y\")\nf()\n";
    let want = "x\nf : () => {} @ {Print} = <fn>\ny\n";
    let none = temp_file("repl_prelude_nows.kel", &src.replace(ws, ""));
    let other = temp_file(
        "repl_prelude_ows.kel",
        &src.replace(
            ws,
            &ws.replace("perform write(message)", "perform write(\"[\" + message)"),
        ),
    );
    for p in [none, other] {
        let (out, code) = repl(&["--prelude", &p], input);
        assert_eq!(out, want, "{p}");
        assert_eq!(code, Some(0));
    }
}

/// 同名の値を持つ前の入力の module が複数あれば、名前の最も小さい module を挙げる(Diktor と同じ)。
#[test]
fn 前の入力の_module_との衝突は名前の最も小さい_module_を挙げる() {
    check(
        "module B { pub let v: Int32 = 1 }\nmodule A { pub let v: Int32 = 2 }\nlet v = 3\n",
        "B.v : Int32 = 1\nA.v : Int32 = 2\n\
         ! <stdin>:3:1: 型エラー: トップレベルの v は、前の入力の module A の v と同名です(module 内の名前とトップレベル名は同名にできません)\n",
    );
}

/// 閉じた標準入力では 74 で終わり、`--strict-exhaustive` と後段の組の選択肢は受けて捨てる。
#[test]
fn 標準入力と受けて捨てる旗() {
    let k = env!("CARGO_BIN_EXE_keleut");
    let r = Command::new("sh")
        .arg("-c")
        .arg(format!("exec '{k}' repl <&-"))
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(74));
    assert_eq!(
        String::from_utf8_lossy(&r.stderr),
        "keleut: 標準入力を読めません: Bad file descriptor\n"
    );
    let (out, code) = repl(&["--strict-exhaustive", "--backend", "wasm"], "1\n");
    assert_eq!((out.as_str(), code), ("_ : Int32 = 1\n", Some(0)));
}

/// KIR の検証のスレッドを作れないような資源の不足は、入力の誤りではないので、
/// 診断を標準エラーに書いてセッションを 70 で終える(`keleut run` と同じ)。
///
/// どのスレッドを作るところで止まるかは構築の種類で変わる
/// (リリースの構築では KIR の検証のスレッド、最適化なしの構築ではフロントエンドのスレッド)ので、
/// 診断の文面は先頭だけを比べる。
#[test]
fn 資源の不足でセッションを_70_で終える() {
    let k = env!("CARGO_BIN_EXE_keleut");
    let mut ch = Command::new("sh")
        .arg("-c")
        .arg(format!("ulimit -v 524288; exec '{k}' repl --no-prelude"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(b"1 + 1\nlet x = 2\nx\n")
        .unwrap();
    let r = ch.wait_with_output().unwrap();
    assert_eq!(r.status.code(), Some(70));
    assert_eq!(String::from_utf8_lossy(&r.stdout), "");
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.starts_with("keleut: "), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
}

/// 入れ子が KIR の限度を超えた入力は、処理系の限度として受理せず、後の入力を処理する(V224)。
#[test]
fn kir_の入れ子の限度を超えた入力は受理しない() {
    let n = 100_000;
    let e = format!("{}1{}", "1 + (".repeat(n), ")".repeat(n));
    let input = format!("let y = 1\nlet x = {e}\necholn(show(y))\nx + y\n");
    check(
        &input,
        "y : Int32 = 1\n<stdin>: 処理系の限度: KIR の書き出しで入れ子が深すぎます\n1\n\
         ! <stdin>:4:1: 型エラー: 未束縛の変数: x\n",
    );
}

/// コーパスの全件を比べる 2 つの試験を順に走らせるための錠である。
///
/// どちらもコーパスの全件で keleut を起動し、1 つの keleut が大きなスタックのスレッドを作るので、
/// 並行して走らせて資源を奪い合わないようにする。
static CORPUS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 食い違った 1 件の、両方の標準出力、標準エラー、終了状態を書く。
///
/// keleut の標準エラーに内部エラー(スレッドを作れないなど)があれば、先頭に印を付ける。
fn describe(f: &Path, d: &Output, k: &Output) -> String {
    let kerr = String::from_utf8_lossy(&k.stderr);
    let mark = if kerr.contains("keleut: 内部エラー") || kerr.contains("スレッドを作れません")
    {
        "[keleut の内部エラー] "
    } else {
        ""
    };
    format!(
        "{mark}{}\n  diktor: {:?}\n    stdout={:?}\n    stderr={:?}\n  keleut: {:?}\n    stdout={:?}\n    stderr={:?}",
        f.display(),
        d.status,
        String::from_utf8_lossy(&d.stdout),
        String::from_utf8_lossy(&d.stderr),
        k.status,
        String::from_utf8_lossy(&k.stdout),
        kerr,
    )
}

/// `KELEUT_DIKTOR` があれば、その diktor を返す。
fn diktor() -> Option<PathBuf> {
    std::env::var_os("KELEUT_DIKTOR").map(PathBuf::from)
}

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/corpus");
    let mut v = Vec::new();
    for d in std::fs::read_dir(&root).unwrap() {
        let d = d.unwrap().path();
        if d.is_dir() {
            for f in std::fs::read_dir(&d).unwrap() {
                let f = f.unwrap().path();
                if f.extension().is_some_and(|e| e == "kel") {
                    v.push(f);
                }
            }
        }
    }
    v.sort();
    v
}

#[test]
fn コーパスを_repl_に読ませて_diktor_と比べる() {
    let Some(dk) = diktor() else {
        return;
    };
    let _g = CORPUS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut bad = Vec::new();
    for f in corpus() {
        let src = std::fs::read_to_string(&f).unwrap_or_default();
        let dir = f.parent().unwrap();
        let mut c = Command::new(&dk);
        c.arg("--repl")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut ch = c.spawn().unwrap();
        ch.stdin.take().unwrap().write_all(src.as_bytes()).unwrap();
        let d = ch.wait_with_output().unwrap();
        let k = keleut_in(Some(dir), &["repl"], &src);
        // レコードの更新(`{r with l = …}`)の値は、Diktor はフィールドの位置を保ち、keleut は型の行の順に表示する。
        let out_differs = d.stdout != k.stdout && !src.contains(" with ");
        if out_differs || d.status.code() != k.status.code() {
            bad.push(describe(&f, &d, &k));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn コーパスを_frontend_rust_で実行して_diktor_と比べる() {
    let Some(dk) = diktor() else {
        return;
    };
    let _g = CORPUS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut bad = Vec::new();
    for f in corpus() {
        let dir = f.parent().unwrap();
        let name = f.file_name().unwrap().to_str().unwrap();
        let d = Command::new(&dk)
            .arg(name)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let k = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(["run", "--frontend", "rust", name])
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        if d.stdout != k.stdout || d.status.code() != k.status.code() {
            bad.push(describe(&f, &d, &k));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn frontend_rust_の終了状態() {
    let d = std::env::temp_dir().join(format!("keleut-repl-cmd-front-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let run = |src: &str, extra: &[&str]| {
        std::fs::write(d.join("a.kel"), src).unwrap();
        let mut a = vec!["run", "--frontend", "rust"];
        a.extend_from_slice(extra);
        a.push("a.kel");
        let r = Command::new(env!("CARGO_BIN_EXE_keleut"))
            .args(&a)
            .current_dir(&d)
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&r.stdout).into_owned(),
            String::from_utf8_lossy(&r.stderr).into_owned(),
            r.status.code(),
        )
    };
    assert_eq!(
        run("echoln(\"hi\")\n", &[]),
        ("hi\n".into(), String::new(), Some(0))
    );
    let (_, e, c) = run("let x: Int32 = \"s\"\n", &[]);
    assert_eq!(c, Some(1));
    assert!(e.contains("型エラー"), "{e}");
    let (_, e, c) = run("let x = (\n", &[]);
    assert_eq!(c, Some(2));
    assert!(e.contains("構文エラー"), "{e}");
    assert_eq!(run("echoln(show(1 / 0))\n", &[]).2, Some(3));
    let src =
        "newtype T = A | B\nlet f(t: T): Int32 = t match { case A => 1 }\necholn(show(f(A)))\n";
    assert_eq!(run(src, &[]).2, Some(0));
    assert_eq!(run(src, &["--strict-exhaustive"]).2, Some(1));
    std::fs::remove_dir_all(&d).unwrap();
}
