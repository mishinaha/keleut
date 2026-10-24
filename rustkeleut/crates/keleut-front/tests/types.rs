//! 型検査の核(`keleut_front::types`)の試験である。
//!
//! 期待の文字列は Diktor の `--type-check` の出力(diktor/test の typecheck.t などの記録)を写したものである。
//! 環境変数 `KELEUT_FRONT_DIKTOR` に本物の diktor を与えると、
//! `KELEUT_FRONT_CORPUS`(既定は `conformance/corpus`)の全件でも、
//! プレリュードありの `--type-check` の出力(標準出力と標準エラーを 1 本にしたもの)と終了状態を Diktor と比べる。

use keleut_front::types::check::{
    CheckOptions, Stop, check_files, check_limits, on_check_stack, render_line,
};
use std::collections::HashMap;

/// 型検査の結果を、Diktor の `--type-check` の標準出力と終了状態の形にする。
fn check(files: &[(&str, &str)], no_prelude: bool) -> (String, u8) {
    let files: Vec<(String, String)> = files
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let limits = check_limits(keleut_front::DEFAULT_MAX_NEST, None).unwrap();
    on_check_stack(limits, move || {
        let map: HashMap<String, Vec<u8>> = files
            .iter()
            .map(|(n, s)| (n.clone(), s.as_bytes().to_vec()))
            .collect();
        let opts = CheckOptions {
            no_prelude,
            prelude: None,
            files: files.iter().map(|(n, _)| n.clone()).collect(),
            max_nest: keleut_front::DEFAULT_MAX_NEST,
        };
        let mut read = |p: &str| {
            map.get(p)
                .cloned()
                .ok_or_else(|| format!("{p}: No such file or directory"))
        };
        match check_files(&opts, &mut read) {
            Ok(out) => {
                let mut s = String::new();
                for l in &out.lines {
                    s.push_str(&render_line(l));
                    s.push('\n');
                }
                let code = match &out.error {
                    Some(e) => {
                        s.push_str(&e.render());
                        s.push('\n');
                        e.exit
                    }
                    None => 0,
                };
                (s, code)
            }
            Err(Stop::Unsupported(w)) => (format!("unsupported: {w}\n"), 4),
            Err(Stop::Lex(m) | Stop::Syntax(m)) => (format!("{m}\n"), 2),
            Err(e) => (format!("{e:?}\n"), 3),
        }
    })
    .unwrap()
}

#[test]
fn 基本の型と別名() {
    let src = "type MyInt = Int32\ntype Point = {x: Float64, y: Float64}\ntype Pair = (Int32, String)\nlet n: MyInt = 42\nlet d = 1 + 2\nlet f = 1.5\nlet s = \"a\" + \"b\"\nlet big = 1_000_000i64\nlet point(x: Float64, y: Float64): Point = {x = x, y = y}\nlet dist2(p: Point): Float64 = p.x * p.x + p.y * p.y\nlet pair: Pair = (1, \"one\")\nlet first = pair._0\nlet second = pair._1\n";
    let (out, code) = check(&[("basics.kel", src)], true);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "n : Int32\nd : Int32\nf : Float64\ns : String\nbig : Int64\npoint : (Float64, Float64) => {x: Float64, y: Float64}\ndist2 : ({x: Float64, y: Float64}) => Float64\npair : (Int32, String)\nfirst : Int32\nsecond : String\n"
    );
}

#[test]
fn 行多相と型変数の名前() {
    let src = "let getx = fn(r) => r.x + 1\nlet fst2[A, R](t: {_item: A extends R}): A = t._0\nlet swap[A, B](p: (A, B)): (B, A) = (p._1, p._0)\nlet update = fn(p) => {p with x = 1.5}\nlet fma[A: Add + Mul](x: A, y: A, z: A): A = x * y + z\nlet weak = (fn(x) => x)(fn(y) => y)\n";
    let (out, code) = check(&[("rows.kel", src)], true);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "getx : ({x: Int32 extends R1}) => Int32\nfst2 : ({_item: A extends R1}) => A\nswap : ((A, B)) => (B, A)\nupdate : ({x: A extends R1}) => {x: Float64 extends R1}\nfma : [A: Add + Mul] (A, A, A) => A\nweak : (_A) => _A\n"
    );
}

#[test]
fn 型エラーは位置と文面を出して_1_で終わる() {
    let (out, code) = check(
        &[("ml1b.kel", "let bad = fn(f) => {a = f(1), b = f(true)}\n")],
        true,
    );
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "! ml1b.kel:1:37: 型エラー: Boolean は Integral のインスタンスではありません\n"
    );
    let (out, code) = check(
        &[("ml5b.kel", "let ok = 1\nlet bad[A](x: A): A = x + 1\n")],
        true,
    );
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "ok : Int32\n! ml5b.kel:2:23: 型エラー: 型パラメータ ς1 は Add のインスタンスではありません。[A: Add] のように制約を書いてください\n"
    );
}

#[test]
fn プレリュードの型と網羅性の警告() {
    let src = "let xs = Cons(1, Nil)\nlet h(o: Option[Int32]): Int32 = o match { case Some(x) => x }\necholn(show(xs))\n";
    let (out, code) = check(&[("p.kel", src)], false);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "xs : List[Int32]\nh : (Option[Int32]) => Int32\n⚠ match が非網羅的です。例えば None が漏れています\n_ : {}\n"
    );
}

#[test]
fn 循環したカインドは処理系の限度で止まる() {
    let src = "newtype Callback[E] = Callback(() => {} @ E)\nnewtype N2[X] = MkN2(Callback[X[X]])\nlet f2(n: N2[{}]): Int32 = 0\n";
    let (out, code) = check(&[("k.kel", src)], true);
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "! 処理系の限度: 型検査で再帰が深すぎます(スタックオーバーフロー)\n"
    );
}

#[test]
fn import_は移していない段として断る() {
    let (out, code) = check(
        &[("m.kel", "from \"./lib\" import { x }\nlet y = x\n")],
        true,
    );
    assert_eq!(code, 4);
    assert!(out.starts_with("unsupported: "), "{out}");
}

#[test]
fn 深い式も型検査する() {
    let mut s = "1".to_string();
    for _ in 0..20000 {
        s = format!("1 + ({s})");
    }
    let src = format!("let x = {s}\n");
    let (out, code) = check(&[("deep.kel", &src)], true);
    assert_eq!((out.as_str(), code), ("x : Int32\n", 0));
}

/// 本物の diktor が与えられたら、コーパスの全件で `--type-check` の出力を比べる。
#[test]
fn コーパスで_diktor_と同じ() {
    let Some(diktor) = std::env::var_os("KELEUT_FRONT_DIKTOR") else {
        return;
    };
    let corpus = std::env::var_os("KELEUT_FRONT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/corpus")
        });
    let mut files = Vec::new();
    let mut stack = vec![corpus];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "kel") {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut diffs = Vec::new();
    for p in &files {
        let src = std::fs::read_to_string(p).unwrap_or_default();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let o = std::process::Command::new(&diktor)
            .arg("--type-check")
            .arg(&name)
            .current_dir(p.parent().unwrap())
            .output()
            .unwrap();
        let mut want = String::from_utf8_lossy(&o.stdout).to_string();
        want.push_str(&String::from_utf8_lossy(&o.stderr));
        let (got, code) = check(&[(&name, &src)], false);
        if got.starts_with("unsupported: ") {
            continue;
        }
        if (want.as_str(), o.status.code()) != (got.as_str(), Some(code as i32)) {
            diffs.push(p.display().to_string());
        }
    }
    assert!(diffs.is_empty(), "Diktor と違う: {diffs:?}");
}
