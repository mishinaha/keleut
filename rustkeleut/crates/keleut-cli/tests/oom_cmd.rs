//! 確保の失敗を `KELEUT_TEST_ALLOC_FAIL` で起こし、`keleut run` の終わり方を確かめる試験。
//!
//! 各例は `cancel` 節を持つ処理器の中で確保を失敗させ、
//! 試験の行で狙った場所に当たったことを確かめてから、
//! `before`、`cancel` 節の出力(節の中で作った列の表示を含む)、
//! `実行時エラー: メモリ不足です` と終了状態 3 を確かめる。
//! `--tier` を使う例は LLVM の後段を組んだときだけ走らせる。

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// 処理器の中で末尾再帰の仕事を続ける。
/// 回収と呼び出しの引数の領域の確保がこの中で起きる(欄を足すレコードを行の多相な関数に渡さないので、`rec_fields` は起こらない)。
/// `cancel` 節は列を作って表示する。
const LOOP: &str = r#"effect Res = { use: () => Unit }
effect Ask = { ask: (Int32) => Int32 }
let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
let rec len(l: List[Int32], k: Int32): Int32 = l match { case Nil => k case Cons(_, t) => len(t, k + 1) }
let rec work(i: Int32, s: Int32): Int32 = if i == 0 then s else {
  let r = { a = i, b = s };
  let r2 = {c = i extends r};
  work(i - 1, s + len(build(50, Nil), 0) + r2.c - r2.a + perform Ask.ask(1))
}
let main(): Int32 = run h { run a { work(20000, 0) } handle { case ask(x) => resume(x) } } handle { case use() => resume(()) case cancel => { echoln("cancel"); echoln(show(build(5, Nil))) } }
echoln("before")
echoln(show(main()))
"#;

/// 処理器の中で末尾でない再帰を深く続ける。
/// 値のスタックの塊と、行の多相な関数でのレコードの欄の確保がこの中で起きる。
const DEEP: &str = r#"effect Res = { use: () => Unit }
let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
let ext[R](p: {extends R}, i: Int32): {c: Int32 extends R} = {c = i extends p}
let rec deep(n: Int32): Int32 = if n == 0 then 0 else {
  let r = ext({a = n}, n);
  1 + deep(n - 1) + r.c - r.a
}
let main(): Int32 = run h { deep(300000) } handle { case use() => resume(()) case cancel => { echoln("cancel"); echoln(show(build(5, Nil))) } }
echoln("before")
echoln(show(main()))
"#;

/// 処理器の中で、`cancel` 節を持つ内側の処理器を脱出で抜けることを繰り返す。
/// 内側の処理器のフレームが脱出の状態語を退避するので、回収の間に退避した状態語の表が広がる。
const PEND: &str = r#"effect Res = { use: () => Unit }
effect Fail = { fail: (Int32) => Never }
let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
let inner(x: Int32): Int32 @ {Fail} = run g { perform fail(x) } handle { case use() => resume(()) case cancel => () }
let attempt(x: Int32): Int32 = inner(x) handle { case fail(v) => v }
let rec rounds(k: Int32, a: Int32): Int32 = if k == 0 then a else rounds(k - 1, (a + attempt(k)) % 1000003)
let main(): Int32 = run h { rounds(200000, 0) } handle { case use() => resume(()) case cancel => { echoln("cancel"); echoln(show(build(5, Nil))) } }
echoln("before")
echoln(show(main()))
"#;

/// `DEEP` の `cancel` 節で、末尾でない再帰を 16,000 段続けるもの。
///
/// `cancel` 節のフレームは、ハンドラのフレームの後から 1 つ目の塊の終わりを越え、2 つ目の塊に入る。
/// `exec:chunk:hard:2` では 2 つ目の塊の確保が失敗した後なので、2 つ目の塊には値スタックの予備の塊を使うしかない。
/// 段の数は、予備の塊を使わない構築で落ちる 11,000 段と、使う構築で通る 21,796 段の間から選んだ。
fn deep_spare() -> String {
    let s = DEEP
        .replace(
            r#"echoln(show(build(5, Nil))) } }"#,
            r#"echoln(show(build(5, Nil))); echoln(show(down(16000))) } }"#,
        )
        .replace(
            "let rec deep",
            "let rec down(n: Int32): Int32 = if n == 0 then 0 else 1 + down(n - 1)\nlet rec deep",
        );
    assert_eq!(s.matches("down(").count(), 3);
    s
}

/// `LOOP` の `cancel` 節で、新しく呼ぶ関数を長く走らせるもの。
/// `threshold=N` では、`cancel` 節が走る間も翻訳のスレッドが翻訳を続ける。
#[cfg(feature = "llvm")]
fn slow_cancel() -> String {
    let s = LOOP
        .replace(
            r#"echoln(show(build(5, Nil))) } }"#,
            r#"echoln(show(build(5, Nil))); echoln(show(slow(3000000, 0))) } }"#,
        )
        .replace(
            "let rec work",
            "let rec slow(i: Int32, s: Int32): Int32 = if i == 0 then s else slow(i - 1, s + i % 7)\nlet rec work",
        );
    assert_eq!(s.matches("slow(").count(), 3);
    s
}

/// `LOOP` の `cancel` 節を確保の少ない形にしたもの。
fn light() -> String {
    let s = LOOP.replace(
        r#"case cancel => { echoln("cancel"); echoln(show(build(5, Nil))) }"#,
        r#"case cancel => echoln("cancel")"#,
    );
    assert_ne!(s, LOOP);
    s
}

/// `cancel` 節まで走ったときの標準出力。
const CANCELLED: &str = "before\ncancel\n[1, 2, 3, 4, 5]\n";
/// `light()` が `cancel` 節まで走ったときの標準出力。
const CANCELLED_LIGHT: &str = "before\ncancel\n";

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-oom-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `src` を書いた `.kel` を `KELEUT_TEST_ALLOC_FAIL=spec` で走らせる。
fn run(tag: &str, src: &str, spec: &str, backend: &[&str], envs: &[(&str, &str)]) -> Output {
    let d = scratch(tag);
    let f = d.join("p.kel");
    std::fs::write(&f, src).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_keleut"));
    c.arg("run")
        .args(backend)
        .arg(&f)
        .env_remove("KELEUT_JIT_STATS")
        .env_remove("KELEUT_GC_STATS")
        .env("KELEUT_TEST_ALLOC_FAIL", spec)
        .stdin(Stdio::null());
    for (k, v) in envs {
        c.env(k, v);
    }
    let r = c.output().unwrap();
    std::fs::remove_dir_all(&d).unwrap();
    r
}

fn test_line(site: &str) -> String {
    format!("keleut: 試験: {site} の確保を失敗させました\n")
}

fn describe(r: &Output) -> String {
    format!(
        "終了状態 {:?}\n標準出力:\n{}\n標準エラー:\n{}",
        r.status.code(),
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    )
}

/// 狙った場所の試験の行のあとに `cancel` 節が走り、メモリ不足の実行時エラーの 3 で終わることを確かめる。
fn expect_cancelled(tag: &str, src: &str, spec: &str, site: &str, backend: &[&str]) {
    expect_out(tag, src, spec, site, backend, CANCELLED);
}

fn expect_out(tag: &str, src: &str, spec: &str, site: &str, backend: &[&str], out: &str) {
    let r = run(tag, src, spec, backend, &[]);
    let want_err = format!("{}実行時エラー: メモリ不足です\n", test_line(site));
    assert!(
        r.status.code() == Some(3) && r.stdout == out.as_bytes() && r.stderr == want_err.as_bytes(),
        "{spec} {backend:?}: {}",
        describe(&r)
    );
}

const INTERP: &[&str] = &["--backend", "interp"];
#[cfg(feature = "llvm")]
const JIT_ONLY: &[&str] = &["--tier", "jit-only"];

/// `SKIP` の 6 は、落としても続けられる確保(はじめの 6 回)を越えて、
/// 落とせば印付けを打ち切る確保に当たる。
#[test]
fn 回収の中の確保の一度の失敗で_cancel_節が走る_interp() {
    expect_cancelled(
        "collect-soft-i",
        LOOP,
        "exec:collect:soft:6",
        "collect",
        INTERP,
    );
}

#[cfg(feature = "llvm")]
#[test]
fn 回収の中の確保の一度の失敗で_cancel_節が走る_jit() {
    expect_cancelled(
        "collect-soft-j",
        LOOP,
        "exec:collect:soft:6",
        "collect",
        JIT_ONLY,
    );
}

/// 印付けを打ち切って印を消し、掃除をせずにメモリの不足を返す道を通す。
/// `cancel` 節は確保の少ない形にする。
#[test]
fn 回収の中の確保がすべて失敗しても_cancel_節が走る_interp() {
    expect_out(
        "collect-hard-i",
        &light(),
        "exec:collect:hard:0",
        "collect",
        INTERP,
        CANCELLED_LIGHT,
    );
}

#[cfg(feature = "llvm")]
#[test]
fn 回収の中の確保がすべて失敗しても_cancel_節が走る_jit() {
    expect_out(
        "collect-hard-j",
        &light(),
        "exec:collect:hard:0",
        "collect",
        JIT_ONLY,
        CANCELLED_LIGHT,
    );
}

#[test]
fn レコードの欄の確保の失敗で_cancel_節が走る_interp() {
    expect_cancelled(
        "rec-i",
        DEEP,
        "exec:rec_fields:soft:3",
        "rec_fields",
        INTERP,
    );
}

#[cfg(feature = "llvm")]
#[test]
fn レコードの欄の確保の失敗で_cancel_節が走る_jit() {
    expect_cancelled(
        "rec-j",
        DEEP,
        "exec:rec_fields:soft:3",
        "rec_fields",
        JIT_ONLY,
    );
}

#[test]
fn 呼び出しの引数の確保の失敗で_cancel_節が走る_interp() {
    expect_cancelled("args-i", LOOP, "exec:call_args:soft:0", "call_args", INTERP);
}

#[cfg(feature = "llvm")]
#[test]
fn 呼び出しの引数の確保の失敗で_cancel_節が走る_jit() {
    expect_cancelled(
        "args-j",
        LOOP,
        "exec:call_args:soft:0",
        "call_args",
        JIT_ONLY,
    );
}

#[test]
fn 値のスタックの塊の確保の失敗で_cancel_節が走る_interp() {
    expect_cancelled("chunk-soft-i", DEEP, "exec:chunk:soft:2", "chunk", INTERP);
}

#[cfg(feature = "llvm")]
#[test]
fn 値のスタックの塊の確保の失敗で_cancel_節が走る_jit() {
    expect_cancelled("chunk-soft-j", DEEP, "exec:chunk:soft:2", "chunk", JIT_ONLY);
}

/// 2 つ目の塊から後の確保がすべて失敗しても、`cancel` 節は値のスタックの予備の塊にフレームを置いて走る。
/// `hard` は大域の予備に触れないので、`cancel` 節の 2 つ目の塊は予備の塊しかない([`deep_spare`])。
#[test]
fn 値のスタックの塊がすべて失敗しても予備の塊で_cancel_節が走る_interp() {
    expect_out(
        "chunk-hard-i",
        &deep_spare(),
        "exec:chunk:hard:2",
        "chunk",
        INTERP,
        "before\ncancel\n[1, 2, 3, 4, 5]\n16000\n",
    );
}

#[cfg(feature = "llvm")]
#[test]
fn 値のスタックの塊がすべて失敗しても予備の塊で_cancel_節が走る_jit() {
    expect_out(
        "chunk-hard-j",
        &deep_spare(),
        "exec:chunk:hard:2",
        "chunk",
        JIT_ONLY,
        "before\ncancel\n[1, 2, 3, 4, 5]\n16000\n",
    );
}

/// 退避した状態語の表を広げる確保がすべて失敗しても、メモリの不足の状態語は空けておいた項目に退避され、`cancel` 節が走る。
#[test]
fn 退避した状態語の表を広げられなくても_cancel_節が走る_interp() {
    expect_cancelled("pending-i", PEND, "exec:pending:hard:0", "pending", INTERP);
}

#[cfg(feature = "llvm")]
#[test]
fn 退避した状態語の表を広げられなくても_cancel_節が走る_jit() {
    expect_cancelled(
        "pending-j",
        PEND,
        "exec:pending:hard:0",
        "pending",
        JIT_ONLY,
    );
}

/// 翻訳の確保が失敗すると、その関数の翻訳を捨てて翻訳をやめ、インタプリタで走り続ける。
#[cfg(feature = "llvm")]
#[test]
fn 翻訳の確保の失敗のあとはインタプリタで走り続ける() {
    let r = run(
        "jit-soft",
        LOOP,
        "jit:jit:soft:0",
        JIT_ONLY,
        &[("KELEUT_JIT_STATS", "1")],
    );
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(
        r.status.code() == Some(0)
            && r.stdout == b"before\n1020000\n"
            && err.starts_with(&test_line("jit"))
            && err.contains("翻訳した関数 0、"),
        "{}",
        describe(&r)
    );
}

/// 翻訳の確保の失敗で使った予備は取り直されるので、
/// そのあとの実行の確保の失敗でも `cancel` 節が走る。
#[cfg(feature = "llvm")]
#[test]
fn 翻訳の確保の失敗のあとも予備が取り直されて_cancel_節が走る() {
    let r = run(
        "jit-args",
        LOOP,
        "jit:jit:soft:0,exec:call_args:soft:0",
        JIT_ONLY,
        &[],
    );
    let want_err = format!(
        "{}{}実行時エラー: メモリ不足です\n",
        test_line("jit"),
        test_line("call_args")
    );
    assert!(
        r.status.code() == Some(3)
            && r.stdout == CANCELLED.as_bytes()
            && r.stderr == want_err.as_bytes(),
        "{}",
        describe(&r)
    );
}

/// 実行の役割が大域の予備を使った後に、翻訳のスレッドの確保が失敗しても、プロセスは止まらない。
/// 翻訳の役割は予備を取り直してやり直し、翻訳をやめる。
/// `threshold=1` では `cancel` 節が走る間も翻訳が続く([`slow_cancel`])。
#[cfg(feature = "llvm")]
#[test]
fn 実行の役割が予備を使った後の翻訳の確保の失敗でも_cancel_節が走る() {
    let r = run(
        "jit-after-exec",
        &slow_cancel(),
        "jit:jit:soft:200,exec:call_args:soft:0",
        &["--tier", "threshold=1"],
        &[],
    );
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(
        r.status.code() == Some(3)
            && r.stdout == b"before\ncancel\n[1, 2, 3, 4, 5]\n8999997\n"
            && err.contains(&test_line("call_args"))
            && err.contains(&test_line("jit"))
            && err.ends_with("実行時エラー: メモリ不足です\n")
            && err.lines().count() == 3,
        "{}",
        describe(&r)
    );
}

/// `keleut repl` の入力を `KELEUT_TEST_ALLOC_FAIL=spec` で走らせる。
fn repl(input: &str, spec: &str) -> Output {
    use std::io::Write;
    let mut c = Command::new(env!("CARGO_BIN_EXE_keleut"))
        .arg("repl")
        .env_remove("KELEUT_REPL_STATS")
        .env_remove("KELEUT_GC_STATS")
        .env("KELEUT_TEST_ALLOC_FAIL", spec)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

/// `keleut repl` で、ヒープの確保の入口を通らない確保が予備を使った入力は、その入力のメモリの不足として報せる。
/// 印は残らないので、次の入力は関数を定義でき(表を伸ばす確保が残った印で止まらない)、セッションは続く。
/// 失敗する確保は、プレリュードの後、1 つ目の入力を実行する前に起きる。
#[test]
fn repl_で予備を使った入力だけがメモリの不足になり次の入力は続く() {
    let r = repl(
        "let a = 1\nlet f(x: Int32): Int32 = x\nf(2)\n",
        "exec:call_args:soft:0",
    );
    assert!(
        r.status.code() == Some(0)
            && r.stdout
                == "実行時エラー: メモリ不足です\nf : (Int32) => Int32 = <fn>\n_ : Int32 = 2\n"
                    .as_bytes()
            && r.stderr == test_line("call_args").as_bytes(),
        "{}",
        describe(&r)
    );
}
