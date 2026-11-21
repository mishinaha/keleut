//! テキストの読み込み(IRSpec 12 節)のテスト。

mod common;

use kir::ErrorKind;

fn base() -> String {
    std::fs::read_to_string(common::fixtures().join("ex_loop.kirt")).unwrap()
}

fn syntax(t: &str) -> String {
    let e = kir::read_kirt(t.as_bytes()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Syntax, "{e}");
    assert_eq!(e.exit_code(), 65);
    e.message().to_string()
}

#[test]
fn whitespace_is_free() {
    let t = base();
    let canon = kir::write_kir(&kir::read_kirt(t.as_bytes()).unwrap()).unwrap();
    let loose = t.replace('\n', " \t\r\n ").replace("(fn", "\n\n(fn   ");
    let p = kir::read_kirt(loose.as_bytes()).unwrap();
    assert_eq!(kir::write_kir(&p).unwrap(), canon);
    assert_eq!(kir::write_kirt(&p).unwrap(), t.as_bytes());
}

#[test]
fn non_canonical_spellings_are_rejected() {
    let t = base();
    assert!(
        syntax(&t.replace(r#"(paths "loop.kel")"#, r#"(paths "loop\x2ekel")"#)).contains("正準")
    );
    assert!(
        syntax(&t.replace(r#"(paths "loop.kel")"#, r#"(paths "loop\x0Akel")"#)).contains("16 進")
    );
    assert!(
        syntax(&t.replace("(opcall \"builtin/Eq\" eq", "(opcall \"builtin/Eq\" \"eq\""))
            .contains("文字列で書いています")
    );
    assert!(syntax(&t.replace("(lit i32 0)", "(lit i32 00)")).contains("整数"));
    assert!(syntax(&t.replace("(lit i32 0)", "(lit i32 -0)")).contains("整数"));
    syntax(&t.replace("(lit i32 0)", "(lit i32 2147483648)"));
    syntax(&t.replace("(kir 1 0 0)", "(kir 0 3 0)"));
    syntax(&t.replace("(kir 1 0 0)", "(kir 2 0 0)"));
    syntax(&t.replace("(kir 1 0 0)", "(kir 1 0 1)"));
    syntax(&t.replace("(kir 1 0 0)", "(kir 1 65536 0)"));
}

/// メジャーバージョンが同じなら、マイナーバージョンが新しいテキストも読む(IRSpec 14 節)。
/// `Program` は読んだマイナーバージョンを保持し、書き出すときもそのマイナーバージョンを書く。
#[test]
fn newer_minor_is_read() {
    let t = base();
    let newer = t.replace("(kir 1 0 0)", "(kir 1 7 0)");
    let p = kir::read_kirt(newer.as_bytes()).unwrap();
    assert_eq!(p.version, (1, 7));
    kir::verify(&p).unwrap();
    assert_eq!(kir::write_kirt(&p).unwrap(), newer.as_bytes());
}

#[test]
fn names_must_resolve() {
    let t = base();
    assert!(
        syntax(&t.replace(r#"(global "main/loop")"#, r#"(global "main/nope")"#))
            .contains("グローバルが表にありません")
    );
    assert!(
        syntax(&t.replace(
            "(class \"builtin/Sub\" (method sub 0))",
            "(class \"builtin/Sub\" (method sub 0) (method sub 0))"
        ))
        .contains("名前が重なります")
    );
    assert!(syntax(&t.replace("(init \"prelude\")", "(init \"prelud\")")).contains("init"));
    syntax(&t.replace("(init \"main\"", "(junk) (init \"main\""));
    syntax(&format!("{t}(kir 1 0 0)\n"));
    // 名前は UTF-8 でなければならない。文字列リテラルは任意のバイト列でよい。
    syntax(&t.replace(
        r#"(global "main/total" value i64)"#,
        r#"(global "main/tot\xffal" value i64)"#,
    ));
}

#[test]
fn string_literals_are_bytes() {
    let t = base().replace(

        "(call ntail noyield (global \"main/loop\") (lit i32 3) (lit i64 0))",
        "(seq (lit str \"\\x00\\xff\\\"\\\\~\") (call ntail noyield (global \"main/loop\") (lit i32 3) (lit i64 0)))",
    );
    let p = kir::read_kirt(t.as_bytes()).unwrap();
    kir::verify(&p).unwrap();
    let printed = kir::write_kirt(&p).unwrap();
    let again = kir::read_kirt(&printed).unwrap();
    assert_eq!(kir::write_kir(&again).unwrap(), kir::write_kir(&p).unwrap());
    let found = p
        .nodes
        .exps
        .iter()
        .any(|e| matches!(e, kir::Exp::Lit(kir::Lit::Str(s)) if s == b"\x00\xff\"\\~"));
    assert!(found);
}
