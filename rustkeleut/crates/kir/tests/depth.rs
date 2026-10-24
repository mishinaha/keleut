//! 入れ子の限度(IRSpec 13.9 節)の試験。
//!
//! 深さ `N_nest = 100,000` のプログラムは、読み込み、検証、書き出しのどれも受け付け、
//! 100,001 のものは、それぞれの診断で拒否する。
//! どの場合も、プロセスをスタックあふれで落とさない。
//! 深いテキストは字下げのため大きさが深さの 2 乗に比例するので、印字の試験は限度を超える側だけで行う。

mod common;

use kir::{ErrorKind, Limits, N_NEST};

/// `ex_loop` の `main/total` の本体を `deep` に置き換えたテキスト(空白は正準でない 1 行の形)。
fn with_body(deep: &str) -> Vec<u8> {
    let base = std::fs::read_to_string(common::fixtures().join("ex_loop.kirt")).unwrap();
    let old = r#"(call ntail noyield (global "main/loop") (lit i32 3) (lit i64 0))"#;
    assert!(base.contains(old));
    base.replace(old, deep).into_bytes()
}

/// 深さ `n` の式(`not` の鎖)。
fn not_chain(n: usize) -> String {
    let mut s = "(not ".repeat(n - 1);
    s.push_str("(lit i64 0)");
    s.push_str(&")".repeat(n - 1));
    s
}

/// 深さ `n` の式(`opcall` の下の `drecord` の鎖)。
fn dict_chain(n: usize) -> String {
    let mut s = String::from(r#"(opcall "builtin/Eq" eq "#);
    s.push_str(&"(drecord (field a ".repeat(n - 2));
    s.push_str(r#"(dbuiltin "builtin/Eq" "builtin/Int32")"#);
    s.push_str(&"))".repeat(n - 2));
    s.push_str(" (lit i32 0) (lit i32 0))");
    s
}

/// 深さ `n` の式(`match` の節のパターンの `variant` の鎖)。
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
        // 写しと捨てで再帰しない
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
        // 限度を広げれば読めるので、それを使って限度を超えたバイナリを作る
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

/// 深さ 1,000,000 の `not` の鎖を、バイナリに直に書いて読む。
/// 書き手は限度を超えるものを書かないので、関数の本体のバイト列を差し込んで作る。
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

/// 1 段あたりのスタックの量を測るための試験。
///
/// 環境変数 `KIR_STACK_PROBE` にバイト数を与えると、そのスタックで深さ `N_nest` の読み込み、検証、
/// 書き出し、印字を行う。足りなければプロセスが落ちるので、別のプロセスで値を変えて走らせる。
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
        // 印字は大きさが 2 乗になるので、限度を 1 だけ下げて限度の誤りの側だけを走らせる
        let small = Limits {
            max_nest: N_NEST - 1,
            ..lim
        };
        assert!(kir::write_kirt_with(&q, &small).is_err());
        eprintln!("{name}: ok");
    }
}
