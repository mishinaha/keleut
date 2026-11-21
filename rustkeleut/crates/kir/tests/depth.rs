//! 入れ子の上限(IRSpec 13.9 節)のテスト。
//!
//! 深さ `N_nest = 100,000` のプログラムは、読み込み、検証、書き出しのどれも受理する。
//! 深さ 100,001 のプログラムは、読み込み、検証、書き出しのそれぞれの診断で拒否する。
//! どちらの場合も、プロセスはスタックオーバーフローで異常終了しない。
//! 深いテキストはインデントのためにサイズが深さの 2 乗に比例するので、テキストの出力のテストは上限を超える側だけで行う。

mod common;

use kir::{ErrorKind, Limits, N_NEST};

/// `ex_loop` の `main/total` の本体を `deep` に置き換えたテキストを返す。
/// `deep` は 1 行で書くので、返すテキストは空白の入れ方が正準形と異なる。
fn with_body(deep: &str) -> Vec<u8> {
    let base = std::fs::read_to_string(common::fixtures().join("ex_loop.kirt")).unwrap();
    let old = r#"(call ntail noyield (global "main/loop") (lit i32 3) (lit i64 0))"#;
    assert!(base.contains(old));
    base.replace(old, deep).into_bytes()
}

/// 深さ `n` の式を返す。`not` の連鎖で深くする。
fn not_chain(n: usize) -> String {
    let mut s = "(not ".repeat(n - 1);
    s.push_str("(lit i64 0)");
    s.push_str(&")".repeat(n - 1));
    s
}

/// 深さ `n` の式を返す。`opcall` の辞書に置いた `drecord` の連鎖で深くする。
fn dict_chain(n: usize) -> String {
    let mut s = String::from(r#"(opcall "builtin/Eq" eq "#);
    s.push_str(&"(drecord (field a ".repeat(n - 2));
    s.push_str(r#"(dbuiltin "builtin/Eq" "builtin/Int32")"#);
    s.push_str(&"))".repeat(n - 2));
    s.push_str(" (lit i32 0) (lit i32 0))");
    s
}

/// 深さ `n` の式を返す。`match` の節のパターンに置いた `variant` の連鎖で深くする。
fn pat_chain(n: usize) -> String {
    let mut s = String::from("(match (lit i64 0) (clause ");
    s.push_str(&"(variant a ".repeat(n - 2));
    s.push_str("(wild)");
    s.push_str(&")".repeat(n - 2));
    s.push_str(" (lit i64 0)))");
    s
}

type Chain = fn(usize) -> String;

const CHAINS: [(&str, Chain); 3] = [("exp", not_chain), ("dict", dict_chain), ("pat", pat_chain)];

#[test]
fn accepts_n_nest() {
    for (name, chain) in CHAINS {
        let text = with_body(&chain(N_NEST));
        let p = kir::read_kirt(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        kir::verify(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
        let bin = kir::write_kir(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
        let q = kir::read_kir(&bin).unwrap_or_else(|e| panic!("{name}: {e}"));
        kir::verify(&q).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(kir::write_kir(&q).unwrap(), bin, "{name}");
        // コピー(`clone`)とドロップが再帰しないことを確かめる
        let r = q.clone();
        drop(q);
        drop(r);
    }
}

#[test]
fn rejects_n_nest_plus_one() {
    let wide = Limits::for_nest(N_NEST + 10);
    for (name, chain) in CHAINS {
        let text = with_body(&chain(N_NEST + 1));
        let e = kir::read_kirt(&text).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Limit, "{name}: {e}");
        assert!(e.message().contains(kir::READ_LIMIT_MESSAGE), "{name}: {e}");
        assert_eq!(e.exit_code(), 65);
        // 上限を引き上げれば読めるので、読んだプログラムから上限を超えたバイナリを作る
        let p = kir::read_kirt_with(&text, &wide).unwrap();
        let e = kir::verify(&p).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Limit, "{name}: {e}");
        assert!(
            e.message().contains(kir::VERIFY_LIMIT_MESSAGE),
            "{name}: {e}"
        );
        let e = kir::write_kir(&p).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::WriteLimit, "{name}: {e}");
        assert!(
            e.message().contains(kir::WRITE_LIMIT_MESSAGE),
            "{name}: {e}"
        );
        assert_eq!(e.exit_code(), 1);
        let e = kir::write_kirt(&p).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::WriteLimit, "{name}: {e}");
        let bin = kir::write_kir_with(&p, &wide).unwrap();
        let e = kir::read_kir(&bin).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Limit, "{name}: {e}");
        assert!(e.message().contains(kir::READ_LIMIT_MESSAGE), "{name}: {e}");
        let e = kir::load(&bin).unwrap_err();
        assert_eq!(e.exit_code(), 65);
    }
}

/// 深さ 1,000,000 の `not` の連鎖を持つバイナリを、リーダが上限の診断で拒否することを確かめる。
/// ライタは上限を超えるプログラムを書かないので、書き出したバイナリの関数の本体に `not` のバイト列を直接差し込んで作る。
#[test]
fn rejects_million_deep_binary() {
    let text = with_body("(not (lit i64 123456789))");
    let p = kir::read_kirt(&text).unwrap();
    let bin = kir::write_kir(&p).unwrap();
    let mut secs = common::sections(&bin);
    let marker = [0x0A, 0x00, 0x03, 0x95, 0x9A, 0xEF, 0x3A];
    let f = secs.iter_mut().find(|(id, _)| *id == 0x0A).unwrap();
    let at =
        f.1.windows(marker.len())
            .position(|w| w == marker)
            .expect("本体のバイト列が見つかりません");
    let extra = vec![0x0Au8; 1_000_000 - 2];
    f.1.splice(at..at, extra);
    let deep = common::assemble(&bin, &secs);
    let e = kir::read_kir(&deep).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Limit, "{e}");
    assert!(e.message().contains(kir::READ_LIMIT_MESSAGE), "{e}");
    let e = kir::load(&deep).unwrap_err();
    assert_eq!(e.exit_code(), 65);
}

/// 入れ子 1 段あたりのスタックの使用量を測るためのテスト。
///
/// 環境変数 `KIR_STACK_PROBE` にバイト数を指定すると、そのサイズのスタックで、深さ `N_nest` のプログラムの読み込み、検証、バイナリの書き出しを行う。
/// テキストの出力は、上限を 1 だけ下げて、上限のエラーになる場合だけを実行する。
/// スタックが足りなければプロセスが異常終了するので、値を変えながら別々のプロセスで実行する。
/// 環境変数がなければ何もしない。
#[test]
fn stack_probe() {
    let Ok(v) = std::env::var("KIR_STACK_PROBE") else {
        return;
    };
    let lim = Limits {
        max_nest: N_NEST,
        stack_bytes: v.parse().unwrap(),
    };
    for (name, chain) in CHAINS {
        let text = with_body(&chain(N_NEST));
        let p = kir::read_kirt_with(&text, &lim).unwrap_or_else(|e| panic!("{name}: {e}"));
        kir::verify_with(&p, &lim).unwrap();
        let bin = kir::write_kir_with(&p, &lim).unwrap();
        let q = kir::read_kir_with(&bin, &lim).unwrap();
        // テキストの出力はサイズが深さの 2 乗に比例するので、上限を 1 だけ下げ、上限のエラーになる場合だけを実行する
        let small = Limits {
            max_nest: N_NEST - 1,
            ..lim
        };
        assert!(kir::write_kirt_with(&q, &small).is_err());
        eprintln!("{name}: ok");
    }
}
