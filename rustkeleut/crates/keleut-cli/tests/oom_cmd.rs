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

/// 関数 `name` の中で、ラベル `a` と `b0`〜`b6` を 2 つずつ重ねる拡張を 300 段続ける鎖。
///
/// 段 i のレコードは 2i + 1 個のフィールドを持ち、形はどれも新しい。
/// 形の表のラベルの合計は段の数の 2 乗で増えるので、鎖の途中で表を何度も広げ直す。
fn shape_chain(name: &str, a: &str, b: &str) -> String {
    let mut s = format!("let {name}(): Int32 = {{\n  let s0 = {{{a} = 0}}\n");
    for i in 1..=300 {
        s.push_str(&format!(
            "  let s{i} = {{{a} = {i}, {b}{} = 1 extends s{}}}\n",
            i % 7,
            i - 1
        ));
    }
    s.push_str(&format!("  s300.{a}\n}}\n"));
    s
}

/// 処理器の中で [`shape_chain`] の鎖を走らせるプログラム。
///
/// `cancel_builds` が真なら、`cancel` 節でもラベルの違う同じ長さの鎖を走らせ、形の表を広げ直させる。
fn shape_prog(cancel_builds: bool) -> String {
    let mut s = String::from("effect Res = { use: () => Unit }\n");
    s.push_str(&shape_chain("f", "a", "b"));
    let clause = if cancel_builds {
        s.push_str(&shape_chain("g", "x", "y"));
        r#"{ echoln("cancel"); echoln(show(g())) }"#
    } else {
        r#"echoln("cancel")"#
    };
    s.push_str(&format!(
        "let main(): Int32 = run h {{ f() }} handle {{ case use() => resume(()) case cancel => {clause} }}\n"
    ));
    s.push_str("echoln(\"before\")\necholn(show(main()))\n");
    s
}

/// 形の表の確保を `exec:shape:hard:SKIP` で失敗させ、起動の後の最初の広げ直しで失敗する実行を返す。
///
/// 起動のときの登録(プレリュードとプログラムの形)も場所 `shape` で表を取り広げるので、その確保の数を飛ばす。
/// 数はプレリュードとプログラムの形の数で変わるので、`SKIP` を 0 から増やし、`before` を書く最初の値を選ぶ。
/// それより小さい `SKIP` はどれも起動のときの登録で失敗する。
/// `SKIP` 0 では、起動のときの登録の失敗が、初期化を始めるときの実行時エラー(メモリ不足、3)で終わることも確かめる。
fn shape_first_runtime_failure(tag: &str, src: &str, backend: &[&str]) -> Output {
    let start_err = format!("{}実行時エラー: メモリ不足です\n", test_line("shape"));
    for skip in 0..200 {
        let spec = format!("exec:shape:hard:{skip}");
        let r = run(tag, src, &spec, backend, &[]);
        if !r.stdout.is_empty() {
            assert!(skip > 0, "{spec} {backend:?}: {}", describe(&r));
            return r;
        }
        assert!(
            r.status.code() == Some(3) && r.stderr == start_err.as_bytes(),
            "{spec} {backend:?}: {}",
            describe(&r)
        );
    }
    panic!("{backend:?}: 起動のときの登録の後に形の表を広げる確保に当たらない");
}

/// 形の表を広げられなくても `cancel` 節が走り、メモリ不足の実行時エラーの 3 で終わる。
fn expect_shape_light(tag: &str, backend: &[&str]) {
    let r = shape_first_runtime_failure(tag, &shape_prog(false), backend);
    let want_err = format!("{}実行時エラー: メモリ不足です\n", test_line("shape"));
    assert!(
        r.status.code() == Some(3)
            && r.stdout == CANCELLED_LIGHT.as_bytes()
            && r.stderr == want_err.as_bytes(),
        "{backend:?}: {}",
        describe(&r)
    );
}

/// `cancel` 節の中の拡張も形の表を広げられないときは、`cancel` 節からの脱出として打ち切りの記録を書き、
/// 巻き戻しを続けてメモリ不足の実行時エラーの 3 で終わる。
fn expect_shape_in_cancel(tag: &str, backend: &[&str]) {
    let r = shape_first_runtime_failure(tag, &shape_prog(true), backend);
    let want_err = format!(
        "{}cancel 節で例外が抑制されました: メモリ不足です\n実行時エラー: メモリ不足です\n",
        test_line("shape")
    );
    assert!(
        r.status.code() == Some(3)
            && r.stdout == CANCELLED_LIGHT.as_bytes()
            && r.stderr == want_err.as_bytes(),
        "{backend:?}: {}",
        describe(&r)
    );
}

#[test]
fn 形の表を広げられなくても_cancel_節が走る_interp() {
    expect_shape_light("shape-i", INTERP);
}

#[cfg(feature = "llvm")]
#[test]
fn 形の表を広げられなくても_cancel_節が走る_jit() {
    expect_shape_light("shape-j", JIT_ONLY);
}

#[test]
fn cancel_節の中でも形の表を広げられなければ打ち切りを記録して巻き戻しを続ける_interp() {
    expect_shape_in_cancel("shape-cancel-i", INTERP);
}

#[cfg(feature = "llvm")]
#[test]
fn cancel_節の中でも形の表を広げられなければ打ち切りを記録して巻き戻しを続ける_jit() {
    expect_shape_in_cancel("shape-cancel-j", JIT_ONLY);
}

/// `keleut repl` の起動のときと `:reset` の後の形の登録がメモリの不足になれば、
/// 初期化を始めるときの実行時エラー(メモリ不足、3)で終わる(`keleut run` と同じ)。
#[test]
fn repl_の起動と_reset_の形の登録の失敗はメモリ不足の_3_で終わる() {
    let want_err = format!("{}実行時エラー: メモリ不足です\n", test_line("shape"));
    // SKIP 0 は起動のときの最初の表、1 は `:reset` の後の最初の表で失敗する。
    for (skip, want_out) in [(0, ""), (1, "_ : Int32 = 2\n")] {
        let r = repl("1 + 1\n:reset\n2 + 2\n", &format!("exec:shape:hard:{skip}"));
        assert!(
            r.status.code() == Some(3)
                && r.stdout == want_out.as_bytes()
                && r.stderr == want_err.as_bytes(),
            "SKIP {skip}: {}",
            describe(&r)
        );
    }
}

/// `keleut repl` で入力の形を形の表に入れられなければ、その入力だけがメモリの不足になり、セッションは続く。
/// 形の表はほかの表より先に伸ばすので、失敗した入力の大域は伸ばしていない。
/// 起動のときに形の表が使う確保の数はプレリュードで変わるので、`SKIP` を 0 から増やし、
/// 1 つ目の入力が失敗する最初の値を選ぶ。
#[test]
fn repl_で形の表を広げられない入力だけがメモリの不足になり次の入力は続く() {
    let input = "let x = 3\nlet r = {qq = x, rr = x}\nx + 1\n{qq = 5, rr = 6}\n";
    let want_out = "x : Int32 = 3\n実行時エラー: メモリ不足です\n_ : Int32 = 4\n_ : {qq: Int32, rr: Int32} = {qq = 5, rr = 6}\n";
    for skip in 0..50 {
        let r = repl(input, &format!("exec:shape:soft:{skip}"));
        if r.stdout.starts_with(b"x : Int32 = 3\n") {
            assert!(
                r.status.code() == Some(0)
                    && r.stdout == want_out.as_bytes()
                    && r.stderr == test_line("shape").as_bytes(),
                "SKIP {skip}: {}",
                describe(&r)
            );
            return;
        }
    }
    panic!("入力の形を入れる確保に当たらない");
}

// ---- 値スタックの塊の回収と注入(計画 261104-3 の 4.5 節、M111) ----

/// 3 段のハンドラ(`inner`、`mid`、`outer`)の中で、`mid` の G の節が継続を捕まえるプログラム。
///
/// `mid` の G の節は、長さ 1,000,000 のリストを作って回収を起こしてから、値を作らずに `resume` する。
/// 回収は、捕まえた継続の分の値スタックの塊を返す。
/// 継続は `deep` の `depth` 段のフレーム(1 段 12 語)と `inner` のハンドラからなり、`depth` が 300,000 なら約 360 万語、
/// `CHUNK_WORDS`(131,072 語)の 27 塊ほどで、4 塊(524,288 語)を十分に超える。
/// 塊の回収だけを入れた構築では、21,900 段ほどから(約 2 塊)ふつうの `resume` が塊を取れなくなる。
///
/// `inner` の `cancel` 節は 48 個の値を生かしたまま `echoln` を呼ぶ。
/// 節のフレーム(55 語)は、継続の写しの塊の上端に空く `inner` の本体のフレームの分(8 語)に収まらず、次の塊に置かれる。
fn inj(depth: u32) -> String {
    let lets: Vec<String> = (1..=48).map(|i| format!("let x{i} = k({i})")).collect();
    let sum: Vec<String> = (1..=48).map(|i| format!("x{i}")).collect();
    format!(
        r#"effect Op = {{ op: () => Int32 }}
effect Res = {{ use: () => Unit }}
let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
let rec len(l: List[Int32], k: Int32): Int32 = l match {{ case Nil => k case Cons(_, t) => len(t, k + 1) }}
let k(i: Int32): Int32 = i * 1
let inner(): Int32 = run h {{ perform op() }} handle {{ case use() => resume(()) case cancel => {{ {}; echoln("inner cancel"); echoln(show({})) }} }}
let rec deep(n: Int32): Int32 = if n == 0 then inner() else 1 + deep(n - 1)
let mid(): Int32 = deep({depth}) handle {{ case op() => 1 + resume(0 * len(build(1000000, Nil), 0)) case cancel => echoln("own cancel") }}
let outer(): Int32 = run h {{ mid() }} handle {{ case use() => resume(()) case cancel => echoln("outer cancel") }}
echoln("before")
echoln(show(outer()))
"#,
        lets.join("; "),
        sum.join(" + ")
    )
}

/// [`inj`] の 3 つの `cancel` 節がすべて走ったときの標準出力(1176 は 1 から 48 の和)。
const INJ_CANCELLED: &str = "before\ninner cancel\n1176\nown cancel\nouter cancel\n";

/// ふつうの `resume` が継続を置き直す塊(場所 `resume`)をすべて取れなくても、
/// 注入は継続の写しを塊としてつなぐので新しい塊を取らず、継続の中の `cancel` 節から順に走る。
/// 塊の回収だけを入れて注入を直す前の構築では、注入も塊を取れず、`inner cancel` と `own cancel` が失われる。
fn expect_inj_resume(tag: &str, backend: &[&str]) {
    expect_out(
        tag,
        &inj(300_000),
        "exec:resume:hard:0",
        "resume",
        backend,
        INJ_CANCELLED,
    );
}

#[test]
fn 継続を置き直す塊を取れなくても注入で継続の中の_cancel_節が走る_interp() {
    expect_inj_resume("inj-i", INTERP);
}

#[cfg(feature = "llvm")]
#[test]
fn 継続を置き直す塊を取れなくても注入で継続の中の_cancel_節が走る_jit() {
    expect_inj_resume("inj-j", JIT_ONLY);
}

#[test]
fn 継続を置き直す塊を取れなくても注入で継続の中の_cancel_節が走る_gc_stress() {
    expect_inj_resume("inj-g", &["--backend", "interp", "--gc-stress"]);
}

/// `cont_make` が継続の写しを取れなければ、取り出した区間の最上段からメモリの不足で巻き戻す。
/// `--tier jit-only` では、JIT のフレームのエフェクトの命令が、区間の最下段の結果を見出しの再開点に返し直す道を通る。
/// `soft` では予備の道でやり直しが通るので、`hard` で写しの確保をすべて失敗させる。
#[cfg(feature = "llvm")]
#[test]
fn 継続の写しを取れなければ取り出した区間から巻き戻す_jit() {
    expect_out(
        "cont-j",
        &inj(1000),
        "exec:cont:hard:0",
        "cont",
        JIT_ONLY,
        INJ_CANCELLED,
    );
}

#[test]
fn 継続の写しを取れなければ取り出した区間から巻き戻す_interp() {
    expect_out(
        "cont-i",
        &inj(1000),
        "exec:cont:hard:0",
        "cont",
        INTERP,
        INJ_CANCELLED,
    );
}

/// 深さ 3,000,000 の再帰から戻った後に、長さ 7,000,000 のリストを 2 回作り直すプログラム(`probe/alloc/chunks.kel` の MODE 3 の変種)。
///
/// 塊を返せば、最大の使用量は再帰の分とリストの分の大きいほうに近く、返さなければ和に近い。
/// リストの長さは、2 つの分が同じ程度(どちらも約 300 MB)になるように選んだ。
const CHUNKS: &str = r#"let rec deep(n: Int32): Int32 = if n == 0 then 0 else 1 + deep(n - 1)
let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
let rec len(l: List[Int32], k: Int32): Int32 = l match { case Nil => k case Cons(_, t) => len(t, k + 1) }
let rec rounds(r: Int32, k: Int32): Int32 = if r == 0 then k else rounds(r - 1, k + len(build(7000000, Nil), 0))
let a = deep(3000000)
let b = rounds(2, 0)
echoln(show(a + b))
"#;

/// 深い再帰で取った値スタックの塊を回収の後に返すので、その後にリストを作り直しても `ulimit -v` の上限に収まる。
///
/// 上限は、LLVM の後段を持つ構築で 720,000 KiB、持たない構築(`--no-default-features`)で 500,000 KiB である。
/// LLVM の共有ライブラリを読み込む分だけ、持つ構築のほうが仮想メモリを多く使うからである。
/// M111 で測った通る上限の最小は次のとおりで、どちらの上限も、直した構築と塊を返さない直す前の構築(M111 の前の 6aa7ba3)の境目の間にある。
///
/// | 構築 | 直した構築 | 直す前の構築 |
/// | --- | --- | --- |
/// | LLVM を持つ release | 約 583,000 KiB | 約 863,000 KiB |
/// | LLVM を持つ debug(試験の構築) | 約 592,000 KiB | 測っていない |
/// | LLVM を持たない release | 約 372,000 KiB | 約 653,000 KiB |
/// | LLVM を持たない debug(試験の構築) | 約 379,000 KiB | 約 661,000 KiB |
///
/// 機械と確保器で揺れるので、cram にはしない。
#[test]
fn 深い再帰の後の回収で塊を返し仮想メモリの上限に収まる() {
    let d = scratch("chunks-ulimit");
    std::fs::write(d.join("p.kel"), CHUNKS).unwrap();
    let r = Command::new("sh")
        .arg("-c")
        .arg("ulimit -v \"$1\"; exec \"$0\" run --backend interp --native-stack 64m --max-depth 20000000 p.kel")
        .arg(env!("CARGO_BIN_EXE_keleut"))
        .arg(if cfg!(feature = "llvm") { "720000" } else { "500000" })
        .current_dir(&d)
        .env_remove("KELEUT_JIT_STATS")
        .env_remove("KELEUT_GC_STATS")
        .env_remove("KELEUT_TEST_ALLOC_FAIL")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    std::fs::remove_dir_all(&d).unwrap();
    assert!(
        r.status.code() == Some(0) && r.stdout == b"17000000\n" && r.stderr.is_empty(),
        "{}",
        describe(&r)
    );
}
