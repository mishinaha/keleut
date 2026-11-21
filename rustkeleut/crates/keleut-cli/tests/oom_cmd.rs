//! `KELEUT_TEST_ALLOC_FAIL` で確保を失敗させ、`keleut run` がどう終了するかを確かめるテスト。
//!
//! 多くのテストは、`cancel` 節を持つハンドラの中で確保を失敗させる。
//! まず、標準エラーの `keleut: 試験: SITE の確保を失敗させました` の行で、狙った確保箇所で失敗したことを確かめる。
//! 続いて、`before` の行、`cancel` 節の出力(節の中で作ったリストの表示を含む)、
//! `実行時エラー: メモリ不足です` の行、終了コード 3 を確かめる。
//! これと違う結果を確かめるテスト(コンパイル中の確保の失敗、`keleut repl`、起動時のシェイプの登録、値スタックのチャンクの解放など)は、
//! それぞれの関数の説明に、確かめる結果を書く。
//! `--tier` を使うテストは、LLVM バックエンドを含むビルドでだけ実行する。

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// ハンドラの中で、末尾再帰の処理を続けるプログラム。
/// GC と、呼び出しの引数の配列の確保が、この処理の中で起きる。
/// フィールドを追加するレコードを行多相の関数に渡さないので、確保箇所 `rec_fields` の確保は起きない。
/// `cancel` 節はリストを作って表示する。
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

/// ハンドラの中で、非末尾の再帰を深く続けるプログラム。
/// 値スタックのチャンクの確保と、行多相の関数がレコードのフィールドを置く領域の確保が、この再帰の中で起きる。
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

/// ハンドラの中で、`cancel` 節を持つ内側のハンドラを脱出で抜けることを繰り返すプログラム。
/// 内側のハンドラのフレームが脱出の状態ワードを退避するので、GC と GC の間に、退避した状態ワードの表が拡張される。
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

/// `DEEP` の `cancel` 節に、16,000 段の非末尾の再帰を追加したプログラムを返す。
///
/// `cancel` 節のフレームは、ハンドラのフレームの次から始まり、1 つ目のチャンクの終わりを越えて、2 つ目のチャンクに入る。
/// `exec:chunk:hard:2` では、このとき 2 つ目のチャンクの確保がすでに失敗しているので、2 つ目のチャンクには値スタックの予備チャンクを使うしかない。
/// 段数は、予備チャンクを使わなければ失敗する 11,000 段と、予備チャンクを使えば実行できる 21,796 段の間から選んだ。
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

/// `LOOP` の `cancel` 節に、それまで呼んでいない関数を長く実行する処理を追加したプログラムを返す。
/// `--tier threshold=N` では、`cancel` 節を実行している間もコンパイルスレッドがコンパイルを続ける。
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

/// `LOOP` の `cancel` 節を、確保の少ない節に置き換えたプログラムを返す。
fn light() -> String {
    let s = LOOP.replace(
        r#"case cancel => { echoln("cancel"); echoln(show(build(5, Nil))) }"#,
        r#"case cancel => echoln("cancel")"#,
    );
    assert_ne!(s, LOOP);
    s
}

/// `cancel` 節まで実行したときの標準出力。
const CANCELLED: &str = "before\ncancel\n[1, 2, 3, 4, 5]\n";
/// `light()` のプログラムを `cancel` 節まで実行したときの標準出力。
const CANCELLED_LIGHT: &str = "before\ncancel\n";

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("keleut-oom-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `src` を書いた `.kel` を、`KELEUT_TEST_ALLOC_FAIL=spec` を設定して `keleut run` で実行する。
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

/// 狙った確保箇所 `site` で確保が失敗した後に `cancel` 節が実行され、メモリ不足の実行時エラーとして終了コード 3 で終了することを確かめる。
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

/// `SKIP` を 6 にして、GC の中の 7 回目の確保を失敗させる。
/// 最初の 6 回の確保は失敗しても GC を続けられるが、7 回目の確保が失敗すると、GC はマークを中断する。
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

/// マークを中断し、付けたマークを消し、スイープをせずにメモリ不足を返す経路を通す。
/// `cancel` 節には確保の少ない節を使う。
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

/// 2 つ目以降のチャンクの確保がすべて失敗しても、`cancel` 節は値スタックの予備チャンクにフレームを置いて実行される。
/// `hard` はグローバルアロケータの予備領域を使わないので、`cancel` 節が 2 つ目のチャンクとして使えるのは予備チャンクだけである([`deep_spare`])。
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

/// 退避した状態ワードの表を拡張する確保がすべて失敗しても、メモリ不足の状態ワードは空けておいた項目に退避され、`cancel` 節が実行される。
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

/// コンパイル中の確保が失敗すると、その関数のコンパイル結果を捨てて以後のコンパイルをやめ、プログラムはインタプリタで実行を続ける。
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

/// コンパイル中の確保が失敗して予備領域を使っても、予備領域は確保し直されるので、
/// その後に実行スレッドの確保が失敗しても `cancel` 節が実行される。
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

/// 実行スレッドがグローバルアロケータの予備領域を使った後に、コンパイルスレッドの確保が失敗しても、プロセスは異常終了しない。
/// コンパイルスレッドは、予備領域を確保し直してから確保をやり直し、以後のコンパイルをやめる。
/// `--tier threshold=1` では、`cancel` 節を実行している間もコンパイルが続く([`slow_cancel`])。
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

/// `KELEUT_TEST_ALLOC_FAIL=spec` を設定した `keleut repl` に、`input` を標準入力として読ませる。
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

/// ヒープの確保の関数(`Rt::alloc`)を通らない確保が予備領域を使うと、実行スレッドのフラグが立つ。
/// `keleut repl` は、入力の単位を実行する前にこのフラグを下ろし、その入力をメモリ不足の実行時エラーとして報告する。
/// フラグは次の入力まで残らないので、次の入力は関数を定義でき、セッションは続く。
/// フラグが残っていれば、次の入力で表を拡張する確保が失敗する。
/// 失敗させる確保は、プレリュードの後、1 つ目の入力を実行する前に起きる。
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

/// 関数 `name` の定義を返す。この関数は、ラベル `a` と `b0`〜`b6` のフィールドを 2 つずつ重ねる拡張を 300 段続ける。
///
/// 段 i のレコードは 2i + 1 個のフィールドを持ち、どの段のシェイプも新しい。
/// シェイプ表に入るラベルの合計は段数の 2 乗に比例して増えるので、連鎖の途中でシェイプ表を何度も拡張する。
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

/// ハンドラの中で [`shape_chain`] の連鎖を実行するプログラムを返す。
///
/// `cancel_builds` が真なら、`cancel` 節でも、ラベルだけが違う同じ長さの連鎖を実行し、シェイプ表をさらに拡張させる。
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

/// シェイプ表の確保を `exec:shape:hard:SKIP` で失敗させ、起動後の最初の拡張で失敗した実行の結果を返す。
///
/// 起動時の登録(プレリュードとプログラムのシェイプの登録)も、確保箇所 `shape` でシェイプ表を確保し、拡張する。
/// そこで、`SKIP` で起動時の確保の回数だけ飛ばす。
/// この回数はプレリュードとプログラムのシェイプの個数で変わるので、`SKIP` を 0 から増やし、プログラムが `before` を書く最初の値を選ぶ。
/// それより小さい `SKIP` では、どれも起動時の登録で失敗する。
/// 起動時の登録で失敗した実行が、標準出力に何も書かず、初期化の開始時の実行時エラー(メモリ不足)として終了コード 3 で終了することも確かめる。
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

/// シェイプ表を拡張できなくても `cancel` 節が実行され、メモリ不足の実行時エラーとして終了コード 3 で終了することを確かめる。
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

/// `cancel` 節の中でレコードにフィールドを追加するときにもシェイプ表を拡張できなければ、`cancel` 節からの脱出として抑制の記録を書き、
/// 巻き戻しを続けて、メモリ不足の実行時エラーとして終了コード 3 で終了することを確かめる。
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

/// `keleut repl` の起動時と `:reset` の後のシェイプの登録がメモリ不足になれば、
/// `keleut run` と同じく、初期化の開始時の実行時エラー(メモリ不足)として終了コード 3 で終了する。
#[test]
fn repl_の起動と_reset_の形の登録の失敗はメモリ不足の_3_で終わる() {
    let want_err = format!("{}実行時エラー: メモリ不足です\n", test_line("shape"));
    // `SKIP` が 0 なら起動時の最初のシェイプ表の確保、1 なら `:reset` の後の最初のシェイプ表の確保が失敗する。
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

/// `keleut repl` で入力のシェイプをシェイプ表に登録できなければ、その入力だけがメモリ不足になり、セッションは続く。
/// シェイプ表はほかの表より先に拡張するので、失敗した入力のためにグローバルの表は拡張していない。
/// 起動時にシェイプ表が行う確保の回数はプレリュードで変わるので、`SKIP` を 0 から増やし、
/// 起動に成功して 1 つ目の入力 `let x = 3` が受理される最初の値を選ぶ。
/// その値では、新しいシェイプを使う 2 つ目の入力で確保が失敗する。
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

// ---- 値スタックのチャンクの解放と注入 ----

/// 3 段のハンドラ(`inner`、`mid`、`outer`)の中で、`mid` の分類 G の操作節が継続をキャプチャするプログラムを返す。
///
/// `mid` の分類 G の操作節は、長さ 1,000,000 のリストを作って GC を起こしてから、値を作らずに `resume` する。
/// GC は、キャプチャした継続のフレームが使っていた値スタックのチャンクを解放する。
/// 継続は `deep` の `depth` 段のフレーム(1 段 12 ワード)と `inner` のハンドラからなる。
/// `depth` が 300,000 なら約 360 万ワードで、`CHUNK_WORDS`(131,072 ワード)のチャンク 27 個ほどになり、4 チャンク(524,288 ワード)を十分に超える。
/// GC は、生きているフレームのうち最も上のものを含むチャンクと、その次の 1 つだけを残す。
/// そのため、通常の `resume` は、継続を配置し直すチャンクのうち、足りない分を新しく確保しなければならない。
/// `depth` が約 21,900 段(約 2 チャンク)以上のときに確保箇所 `resume` の確保を失敗させると、通常の `resume` は必要なチャンクを確保できない。
///
/// `inner` の `cancel` 節は、48 個の値を生存させたまま `echoln` を呼ぶ。
/// 節のフレーム(55 ワード)は、コピーチャンク(継続が持つフレームのコピーの領域を、そのままつないだチャンク)の上端の空きに収まらず、次のチャンクに置かれる。
/// 上端の空きは 8 ワードで、`inner` の本体のフレームの分である。
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

/// [`inj`] の 3 つの `cancel` 節がすべて実行されたときの標準出力。1176 は 1 から 48 までの和である。
const INJ_CANCELLED: &str = "before\ninner cancel\n1176\nown cancel\nouter cancel\n";

/// 通常の `resume` が継続を配置し直すチャンク(確保箇所 `resume`)を 1 つも確保できなくても、
/// 注入は継続のコピーをチャンクのまま値スタックにつなぐので、新しいチャンクを確保しない。
/// そのため、継続の中の `cancel` 節から順に、3 つの `cancel` 節がすべて実行されることを確かめる。
/// 注入が新しいチャンクを確保する実装では、注入も失敗し、`inner cancel` と `own cancel` の行が出力されない。
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

/// `cont_make` が継続のコピーを確保できなければ、キャプチャする区間の最上段から、メモリ不足で巻き戻す。
/// `--tier jit-only` では、JIT のフレームのエフェクト命令が、区間の最下段の結果をフレームヘッダの再開点に返し直す経路を通る。
/// `soft` では、予備領域を解放してやり直した確保が成功するので、`hard` でコピーの確保をすべて失敗させる。
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

/// 深さ 3,000,000 の再帰から戻った後に、長さ 7,000,000 のリストを 2 回作るプログラム。
///
/// 再帰で確保したチャンクを解放すれば、メモリの最大使用量は、再帰の分とリストの分の大きいほうに近くなる。
/// 解放しなければ、2 つの和に近くなる。
/// リストの長さは、2 つの分が同じ程度(どちらも約 300 MB)になるように選んだ。
const CHUNKS: &str = r#"let rec deep(n: Int32): Int32 = if n == 0 then 0 else 1 + deep(n - 1)
let rec build(n: Int32, acc: List[Int32]): List[Int32] = if n == 0 then acc else build(n - 1, Cons(n, acc))
let rec len(l: List[Int32], k: Int32): Int32 = l match { case Nil => k case Cons(_, t) => len(t, k + 1) }
let rec rounds(r: Int32, k: Int32): Int32 = if r == 0 then k else rounds(r - 1, k + len(build(7000000, Nil), 0))
let a = deep(3000000)
let b = rounds(2, 0)
echoln(show(a + b))
"#;

/// 深い再帰で確保した値スタックのチャンクを GC の後に解放するので、その後にリストを作っても `ulimit -v` の上限に収まる。
///
/// 上限は、LLVM バックエンドを含むビルドでは 720,000 KiB、含まないビルド(`--no-default-features`)では 500,000 KiB にする。
/// LLVM バックエンドを含むビルドは、LLVM の共有ライブラリを読み込む分だけ仮想メモリを多く使うからである。
/// 実測では、このプログラムを実行できる上限の最小値は次の表のとおりである。
/// どちらの上限も、チャンクを解放する実装の最小値と、解放しない実装の最小値の間にある。
///
/// | ビルド | チャンクを解放する実装 | チャンクを解放しない実装 |
/// | --- | --- | --- |
/// | LLVM バックエンドを含むリリースビルド | 約 583,000 KiB | 約 863,000 KiB |
/// | LLVM バックエンドを含むデバッグビルド(テストで使うビルド) | 約 592,000 KiB | 測っていない |
/// | LLVM バックエンドを含まないリリースビルド | 約 372,000 KiB | 約 653,000 KiB |
/// | LLVM バックエンドを含まないデバッグビルド(テストで使うビルド) | 約 379,000 KiB | 約 661,000 KiB |
///
/// これらの値はマシンとアロケータで変動するので、cram のテストにはしない。
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
