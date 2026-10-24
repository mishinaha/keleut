//! x86_64 の静的ライブラリが libc に依存せず、外部の記号がすべて `krt_` で始まることを確かめる。
//!
//! `nm` で、ライブラリの中で定義されずに参照される記号と、定義される大域の記号を調べる。

use std::collections::BTreeSet;
use std::process::Command;

fn nm(args: &[&str]) -> Vec<String> {
    let out = Command::new("nm")
        .args(args)
        .arg(keleut_rtcore::STATIC_LIB_PATH)
        .output()
        .expect("nm を起動できない");
    assert!(out.status.success(), "nm が失敗した");
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_whitespace().last().map(str::to_string))
        .filter(|s| !s.ends_with(':') && !s.ends_with(".o"))
        .collect()
}

#[test]
fn no_libc_and_krt_prefix() {
    let defined: BTreeSet<String> = nm(&["-g", "--defined-only"]).into_iter().collect();
    let undefined: BTreeSet<String> = nm(&["-u"]).into_iter().collect();
    let missing: Vec<&String> = undefined.difference(&defined).collect();
    assert!(
        missing.is_empty(),
        "rtcore の外の記号を参照している: {missing:?}"
    );
    let bad: Vec<&String> = defined.iter().filter(|s| !s.starts_with("krt_")).collect();
    assert!(bad.is_empty(), "krt_ で始まらない大域の記号: {bad:?}");
    assert!(defined.contains("krt_f64_show") && defined.contains("krt_sin"));
}
