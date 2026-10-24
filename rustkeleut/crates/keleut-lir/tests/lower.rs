//! 下ろしの形の試験。
//!
//! 小さな KIR のテキストを下ろし、各パスが出す命令の形を確かめる。
//! 土台は `conformance/kir-bad/deep-base.kirt` の組み込みの表の部分である。

use keleut_lir::{FuncKind, Inst, Program, RtError, Term};
use std::path::PathBuf;

/// 組み込みの表(型、クラス、組み込みの値)までの部分。
fn prelude() -> String {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/kir-bad/deep-base.kirt");
    let text = std::fs::read_to_string(p).unwrap();
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("(global \"main/") {
            break;
        }
        out += line;
        out.push('\n');
    }
    out
}

/// 土台に `body` を足し、単位 `main` の手順を `init` にして下ろす。
fn lower(body: &str, init: &str) -> Program {
    let text = format!(
        "{}{body}\n(init \"builtin\")\n(init \"prelude\")\n(init \"main\" {init})\n",
        prelude()
    );
    let p = kir::load(text.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{text}"));
    keleut_lir::lower(&p).unwrap_or_else(|e| panic!("{e}"))
}

fn func<'a>(p: &'a Program, sym: &str) -> &'a keleut_lir::Func {
    p.funcs.iter().find(|f| f.sym == sym).unwrap()
}

fn insts(f: &keleut_lir::Func) -> Vec<&Inst> {
    f.blocks.iter().flat_map(|b| b.insts.iter()).collect()
}

fn terms(f: &keleut_lir::Func) -> Vec<&Term> {
    f.blocks.iter().map(|b| &b.term).collect()
}

fn has_error(f: &keleut_lir::Func, pred: impl Fn(&RtError) -> bool) -> bool {
    terms(f)
        .iter()
        .any(|t| matches!(t, Term::Error { kind, .. } if pred(kind)))
}

fn count(f: &keleut_lir::Func, pred: impl Fn(&Inst) -> bool) -> usize {
    insts(f).into_iter().filter(|i| pred(i)).count()
}

const THUNK: &str = "(params) (locals) (dicts 0) (caps)";

#[test]
fn 整数の除算はゼロの検査を伴い深さを検査する() {
    let p = lower(
        &format!(
            "(global \"main/x\" value i32)\n(fn \"main/x\" thunk yield {THUNK} (result i32)\n  (opcall \"builtin/Div\" div (dbuiltin \"builtin/Div\" \"builtin/Int32\") (lit i32 7) (lit i32 0)))"
        ),
        "(def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/x");
    assert!(has_error(f, |k| *k == RtError::DivZero));
    assert_eq!(count(f, |i| matches!(i, Inst::DepthCheck)), 1);
    assert_eq!(count(f, |i| matches!(i, Inst::IntArith { .. })), 1);
    assert_eq!(count(f, |i| matches!(i, Inst::CallMethod { .. })), 0);
    // 入口の先頭は depth_enter である。
    assert!(matches!(f.blocks[0].insts[0], Inst::DepthEnter));
}

#[test]
fn 浮動小数点数から整数への変換は_nan_と範囲を検査する() {
    let p = lower(
        &format!(
            "(global \"main/f2i\" extern prim \"__f64_to_i32\" 0)\n(global \"main/x\" value i32)\n(fn \"main/x\" thunk yield {THUNK} (result i32)\n  (call ntail noyield (global \"main/f2i\") (lit f64 0x7ff8000000000000)))"
        ),
        "(def_fns \"main/f2i\") (def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/x");
    assert!(has_error(f, |k| matches!(k, RtError::F64ToIntNan(_))));
    assert!(has_error(f, |k| matches!(k, RtError::F64ToIntRange(_))));
    assert_eq!(count(f, |i| matches!(i, Inst::Conv { .. })), 1);
    // extern の大域の値の本体も作る。
    assert!(p.funcs.iter().any(|f| f.kind == FuncKind::Extern));
}

#[test]
fn 後で宣言する大域を読む無名関数は初期化の検査を残す() {
    let p = lower(
        &format!(
            "(global \"main/p\" value any)\n(global \"main/h\" fn \"main/h\")\n\
             (fn \"main/p\" thunk yield {THUNK} (result any) (closure \"main/p$0\"))\n\
             (fn \"main/p$0\" lambda yield (params any) (locals) (dicts 0) (caps) (result any)\n  (call tail yield (global \"main/h\") (local 0)))\n\
             (fn \"main/h\" lambda yield (params any) (locals) (dicts 0) (caps) (result any) (local 0))"
        ),
        "(def_value \"main/p\" \"main/p\") (def_fns \"main/h\")",
    );
    let f = func(&p, "main/p$0");
    assert_eq!(count(f, |i| matches!(i, Inst::GlobalIsInit { .. })), 1);
    assert!(has_error(f, |k| matches!(k, RtError::Unbound(_))));
}

#[test]
fn 前に定義した大域と同じ群の大域は初期化の検査を省く() {
    let p = lower(
        &format!(
            "(global \"main/f\" fn \"main/f\")\n(global \"main/g\" fn \"main/g\")\n(global \"main/x\" value any)\n\
             (fn \"main/f\" lambda yield (params any) (locals) (dicts 0) (caps) (result any)\n  (call tail yield (global \"main/g\") (local 0)))\n\
             (fn \"main/g\" lambda yield (params any) (locals) (dicts 0) (caps) (result any)\n  (call tail yield (global \"main/f\") (local 0)))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any) (call ntail yield (global \"main/f\") (rec_empty)))"
        ),
        "(def_fns \"main/f\" \"main/g\") (def_value \"main/x\" \"main/x\")",
    );
    for s in ["main/f", "main/g", "main/x"] {
        assert_eq!(
            count(func(&p, s), |i| matches!(i, Inst::GlobalIsInit { .. })),
            0,
            "{s}"
        );
    }
    // 自分の手順の中で自分を読む値束縛は検査を残す。
    let p = lower(
        &format!(
            "(global \"main/x\" value any)\n(fn \"main/x\" thunk yield {THUNK} (result any) (global \"main/x\"))"
        ),
        "(def_value \"main/x\" \"main/x\")",
    );
    assert_eq!(
        count(func(&p, "main/x"), |i| matches!(
            i,
            Inst::GlobalIsInit { .. }
        )),
        1
    );
}

#[test]
fn 末尾の呼び出しは深さの命令を持たない() {
    let p = lower(
        &format!(
            "(global \"main/f\" fn \"main/f\")\n(global \"main/x\" value any)\n\
             (fn \"main/f\" lambda yield (params any) (locals) (dicts 0) (caps) (result any)\n  (call tail yield (global \"main/f\") (local 0)))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any) (call ntail yield (global \"main/f\") (rec_empty)))"
        ),
        "(def_fns \"main/f\") (def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/f");
    assert_eq!(count(f, |i| matches!(i, Inst::DepthPush)), 0);
    assert!(
        terms(f)
            .iter()
            .any(|t| matches!(t, Term::TailCallDirect { .. }))
    );
    let x = func(&p, "main/x");
    assert_eq!(count(x, |i| matches!(i, Inst::DepthPush)), 1);
    assert_eq!(count(x, |i| matches!(i, Inst::DepthRestore)), 1);
    let fr = x.frame.as_ref().unwrap();
    assert_eq!(fr.resume_points.len(), 1);
    // 初期化の関数は thunk を直に、深さを増やさずに呼ぶ。
    let init = &p.funcs[p.init[2].index()];
    assert_eq!(count(init, |i| matches!(i, Inst::DepthPush)), 0);
    assert_eq!(count(init, |i| matches!(i, Inst::CallDirect { .. })), 1);
}

#[test]
fn 形の分かるレコードは位置で読む() {
    let p = lower(
        &format!(
            "(global \"main/x\" value any)\n(fn \"main/x\" thunk yield {THUNK} (result any)\n  \
             (rec_select y (rec_extend x (lit i32 1) (rec_extend y (lit i32 2) (rec_empty)))))"
        ),
        "(def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/x");
    assert_eq!(count(f, |i| matches!(i, Inst::RecNew { .. })), 1);
    assert_eq!(
        count(f, |i| matches!(
            i,
            Inst::RecField { .. } | Inst::RecExtend { .. }
        )),
        0
    );
    // 形の分からないレコード(大域)は実行時に引く。
    let p = lower(
        &format!(
            "(global \"main/r\" value any)\n(global \"main/x\" value any)\n\
             (fn \"main/r\" thunk yield {THUNK} (result any) (rec_extend x (lit i32 1) (rec_empty)))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any) (rec_update x (global \"main/r\") (lit i32 2)))"
        ),
        "(def_value \"main/r\" \"main/r\") (def_value \"main/x\" \"main/x\")",
    );
    assert_eq!(
        count(func(&p, "main/x"), |i| matches!(i, Inst::RecUpdate { .. })),
        1
    );
}

#[test]
fn ハンドラと_resume_と利用者の操作は未実装に下ろす() {
    let src = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kir/tests/fixtures/ex_handle.kir"),
    )
    .unwrap();
    let k = kir::load(&src).unwrap();
    let p = keleut_lir::lower(&k).unwrap();
    let mut whats: std::collections::BTreeSet<String> = p
        .funcs
        .iter()
        .flat_map(|f| f.blocks.iter())
        .filter_map(|b| match &b.term {
            Term::Unimplemented { what } => Some(what.clone()),
            _ => None,
        })
        .collect();
    whats.extend(p.funcs.iter().flat_map(insts).filter_map(|i| match i {
        Inst::UnimplementedValue { what, .. } => Some(what.clone()),
        _ => None,
    }));
    assert!(whats.contains("handle"), "{whats:?}");
    for w in &whats {
        assert!(
            ["handle", "perform", "resume", "操作節の選択"].contains(&w.as_str()),
            "{w}"
        );
    }
}

/// 検証に通る小さなプログラム(`main/f` は引数の符号で分ける)。
fn sample() -> Program {
    lower(
        &format!(
            "(global \"main/f\" fn \"main/f\")\n(global \"main/x\" value any)\n\
             (fn \"main/f\" lambda yield (params i32) (locals) (dicts 0) (caps) (result any)\n  \
             (match (opcall \"builtin/Ord\" lt (dbuiltin \"builtin/Ord\" \"builtin/Int32\") (local 0) (lit i32 0))\n    \
             (clause (lit bool true) (seq (call ntail yield (global \"main/f\") (lit i32 1)) (rec_empty)))\n    \
             (clause (wild) (rec_empty))))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any) (call ntail yield (global \"main/f\") (lit i32 -1)))"
        ),
        "(def_fns \"main/f\") (def_value \"main/x\" \"main/x\")",
    )
}

fn fid(p: &Program, sym: &str) -> usize {
    p.funcs.iter().position(|f| f.sym == sym).unwrap()
}

#[test]
fn 検証器は壊した_lir_を拒否する() {
    let ok = sample();
    keleut_lir::verify(&ok).unwrap();
    let f = fid(&ok, "main/f");

    // 後ろ向きの枝。
    let mut p = ok.clone();
    let last = p.funcs[f].blocks.len() - 1;
    p.funcs[f].blocks[last].term = Term::Jump(keleut_lir::BlockId(0));
    assert!(keleut_lir::verify(&p).is_err());

    // Rep の食い違い(bool のレジスタを返す)。
    let mut p = ok.clone();
    let b = p.funcs[f]
        .regs
        .iter()
        .position(|r| *r == keleut_lir::Rep::Bool)
        .unwrap();
    let blk = p.funcs[f]
        .blocks
        .iter()
        .position(|bl| matches!(bl.term, Term::Return(_)))
        .unwrap();
    p.funcs[f].blocks[blk].term = Term::Return(keleut_lir::Reg(b as u32));
    assert!(keleut_lir::verify(&p).is_err());

    // 書いていないかもしれないレジスタを読む(引数でない、後で書くレジスタ)。
    let mut p = ok.clone();
    let late = p.funcs[f].regs.len() - 1;
    p.funcs[f].blocks[0].insts.insert(
        1,
        Inst::Move {
            dst: keleut_lir::Reg(late as u32),
            src: keleut_lir::Reg(late as u32),
        },
    );
    assert!(keleut_lir::verify(&p).is_err());

    // depth_enter が入口の先頭にない。
    let mut p = ok.clone();
    p.funcs[f].blocks[0].insts.remove(0);
    assert!(keleut_lir::verify(&p).is_err());

    // 再開点の生きているレジスタの集合が生存解析と合わない。
    let mut p = ok.clone();
    let fr = p.funcs[f].frame.as_mut().unwrap();
    assert!(!fr.resume_points.is_empty());
    let mut sets: Vec<_> = fr.resume_points.iter().collect();
    sets[0].1.push(keleut_lir::Reg(0));
    sets[0].1.sort();
    sets[0].1.dedup();
    sets[0].1.push(keleut_lir::Reg(u32::MAX));
    fr.resume_points = keleut_lir::Points::from_sets(sets);
    assert!(keleut_lir::verify(&p).is_err());

    // 生のレジスタを any の区画に置く。
    let mut p = ok.clone();
    let raw = p.funcs[f]
        .regs
        .iter()
        .position(|r| *r == keleut_lir::Rep::I32)
        .unwrap();
    p.funcs[f].frame.as_mut().unwrap().slot[raw] = keleut_lir::Slot::Any(0);
    assert!(keleut_lir::verify(&p).is_err());

    // 区間の重なる 2 つのレジスタ(引数と、入口の unbox の結果)を同じ生のスロットに置く。
    let mut p = ok.clone();
    let raws: Vec<usize> = p.funcs[f]
        .regs
        .iter()
        .enumerate()
        .filter(|(_, r)| **r == keleut_lir::Rep::I32)
        .map(|(i, _)| i)
        .collect();
    assert!(raws.len() >= 2);
    // 入口の unbox の結果は本体の比較と再帰の呼び出しの引数の計算まで生きる。
    let fr = p.funcs[f].frame.as_mut().unwrap();
    for &r in &raws {
        fr.slot[r] = keleut_lir::Slot::Raw(0);
    }
    assert!(keleut_lir::verify(&p).is_err());
}

#[test]
fn 開いたレコードのパターンはラベルで読む() {
    // main/g が先にラベル y を表に入れるので、x の番号は y より大きい。
    // 開いたパターン {x} を位置で読むと、{x, y} の形の 0 番目(y)を取り違える。
    let p = lower(
        &format!(
            "(global \"main/g\" fn \"main/g\")\n(global \"main/f\" fn \"main/f\")\n(global \"main/x\" value any)\n\
             (fn \"main/g\" lambda yield (params any) (locals) (dicts 0) (caps) (result any) (rec_select y (local 0)))\n\
             (fn \"main/f\" lambda yield (params any) (locals any) (dicts 0) (caps) (result any)\n  \
             (match (local 0) (clause (record open (field x (bind 1))) (local 1))))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any)\n  \
             (call ntail yield (global \"main/f\") (rec_extend x (lit i32 1) (rec_extend y (lit i32 2) (rec_empty)))))"
        ),
        "(def_fns \"main/g\" \"main/f\") (def_value \"main/x\" \"main/x\")",
    );
    let x = p.labels.iter().position(|l| l == "x").unwrap() as u32;
    let f = func(&p, "main/f");
    assert_eq!(count(f, |i| matches!(i, Inst::RecGet { .. })), 0);
    assert_eq!(
        count(f, |i| matches!(
            i,
            Inst::RecField { label, nth: 0, .. } if label.0 == x
        )),
        1
    );
    // 書いたラベルの有無は型検査が保証するので調べない。
    assert_eq!(count(f, |i| matches!(i, Inst::RecHas { .. })), 0);

    // 閉じたパターン(タプル)は形が決まるので位置で読む。
    let p = lower(
        &format!(
            "(global \"main/f\" fn \"main/f\")\n(global \"main/x\" value any)\n\
             (fn \"main/f\" lambda yield (params any) (locals any) (dicts 0) (caps) (result any)\n  \
             (match (local 0) (clause (record closed (field x (bind 1)) (field y (wild))) (local 1))))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any)\n  \
             (call ntail yield (global \"main/f\") (rec_extend x (lit i32 1) (rec_extend y (lit i32 2) (rec_empty)))))"
        ),
        "(def_fns \"main/f\") (def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/f");
    assert_eq!(count(f, |i| matches!(i, Inst::RecGet { .. })), 1);
    assert_eq!(count(f, |i| matches!(i, Inst::RecField { .. })), 0);
}

#[test]
fn 実装の無いプリミティブは表示名と実装名で_17_と_18_を分ける() {
    for (sym, want) in [("main/__no_such", 17), ("main/M.__no_such", 18)] {
        let p = lower(
            &format!(
                "(global \"{sym}\" extern prim \"__no_such\" 0)\n(global \"main/x\" value any)\n\
                 (fn \"main/x\" thunk yield {THUNK} (result any) (call ntail noyield (global \"{sym}\") (lit i32 1)))"
            ),
            &format!("(def_fns \"{sym}\") (def_value \"main/x\" \"main/x\")"),
        );
        let f = func(&p, "main/x");
        let kind = terms(f)
            .into_iter()
            .find_map(|t| match t {
                Term::Error { kind, .. } if matches!(kind, RtError::UnimplPrim(_)) => Some(kind),
                _ => None,
            })
            .unwrap();
        assert_eq!(p.error_number(kind), want, "{sym}");
        assert!(keleut_lir::print(&p).contains(&format!("error {want} \"{sym}\"")));
    }
}

#[test]
fn 安全点の集合は確保の被演算子を含む() {
    // 文字列の連結の被演算子は、連結が確保している間も根でなければならない。
    let p = lower(
        &format!(
            "(global \"main/cat\" fn \"main/cat\")\n(global \"main/x\" value any)\n\
             (fn \"main/cat\" lambda yield (params str str) (locals) (dicts 0) (caps) (result str)\n  \
             (opcall \"builtin/Add\" add (dbuiltin \"builtin/Add\" \"builtin/String\") (local 0) (local 1)))\n\
             (fn \"main/x\" thunk yield {THUNK} (result any) (call ntail yield (global \"main/cat\") (lit str \"a\") (lit str \"b\")))"
        ),
        "(def_fns \"main/cat\") (def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/cat");
    let fr = f.frame.as_ref().unwrap();
    let mut seen = 0;
    for (pt, live) in fr.safepoints.iter() {
        let i = &f.blocks[pt.block.index()].insts[pt.index as usize];
        if let Inst::StrConcat { dst, a, b } = i {
            assert!(live.contains(a) && live.contains(b), "{live:?}");
            assert!(!live.contains(dst));
            seen += 1;
        }
    }
    assert_eq!(seen, 1);
}

#[test]
fn 捕獲は入口で読み再開点の集合に入る() {
    // 捕獲を持つ関数は、入口で捕獲をレジスタに読み、呼び出しの後で使う捕獲はその再開点で生きている。
    let p = lower(
        "(global \"main/h\" fn \"main/h\")\n(global \"main/x\" value any)\n\
             (fn \"main/h\" lambda yield (params any) (locals) (dicts 0) (caps) (result any) (local 0))\n\
             (fn \"main/x$0\" lambda yield (params any) (locals) (dicts 0) (caps any) (result any)\n  \
             (seq (call ntail yield (global \"main/h\") (local 0)) (env 0)))\n\
             (fn \"main/x\" thunk yield (params) (locals any) (dicts 0) (caps) (result any)\n  \
             (let 0 (rec_empty) (closure \"main/x$0\" (local 0))))",
        "(def_fns \"main/h\") (def_value \"main/x\" \"main/x\")",
    );
    let f = func(&p, "main/x$0");
    let b0 = &f.blocks[0].insts;
    assert!(matches!(b0[0], Inst::DepthEnter));
    let Inst::EnvGet { dst: env, index: 0 } = b0[1] else {
        panic!("{:?}", b0[1]);
    };
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, i) in b.insts.iter().enumerate() {
            if matches!(i, Inst::EnvGet { .. }) {
                assert_eq!((bi, ii), (0, 1));
            }
        }
    }
    let fr = f.frame.as_ref().unwrap();
    assert_eq!(fr.resume_points.len(), 1);
    assert!(fr.resume_points.live(0).contains(&env));
}

#[test]
fn ハンドラの後の式も下ろす() {
    let src = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kir/tests/fixtures/ex_handle.kir"),
    )
    .unwrap();
    let k = kir::load(&src).unwrap();
    let p = keleut_lir::lower(&k).unwrap();
    // unimplemented_value はブロックを終えないので、その値を読む続きの命令も下ろしてある。
    let (mut n, mut used) = (0, 0);
    for f in &p.funcs {
        for b in &f.blocks {
            for (ii, i) in b.insts.iter().enumerate() {
                if let Inst::UnimplementedValue { dst, .. } = i {
                    n += 1;
                    let later = b.insts[ii + 1..].iter().any(|x| x.uses().contains(dst))
                        || b.term.uses().contains(dst);
                    if later {
                        used += 1;
                    }
                }
            }
        }
    }
    assert!(n > 0 && used > 0, "{n} {used}");
}

/// 検証器が `p` を、`what` を含む文面で拒否することを確かめる。
fn rejects(p: &Program, what: &str) {
    let e = keleut_lir::verify(p).unwrap_err().to_string();
    assert!(e.contains(what), "{e}");
}

fn fixture(name: &str) -> Program {
    let src = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../kir/tests/fixtures")
            .join(name),
    )
    .unwrap();
    keleut_lir::lower(&kir::load(&src).unwrap()).unwrap()
}

#[test]
fn 検証器は表と辞書と捕獲の食い違いを拒否する() {
    // closure_set_cap の添字が捕獲の数を超える。
    let ok = fixture("strings.kir");
    keleut_lir::verify(&ok).unwrap();
    let mut p = ok.clone();
    let mut hit = false;
    'outer: for f in &mut p.funcs {
        for b in &mut f.blocks {
            for i in &mut b.insts {
                if let Inst::ClosureSetCap { index, .. } = i {
                    *index = 7;
                    hit = true;
                    f.frame = None;
                    break 'outer;
                }
            }
        }
    }
    assert!(hit);
    rejects(&p, "範囲の外の捕獲 7");

    let ok = sample();
    let f = fid(&ok, "main/f");
    let g = ok.globals.iter().position(|g| g.sym == "main/f").unwrap();

    // 種類 fn の大域の関数が初期化の関数である。
    let mut p = ok.clone();
    p.globals[g].kind = keleut_lir::GlobalKind::Fn(p.init[0]);
    rejects(&p, "捕獲を持たない種類");

    // 初期化の関数が引数を持つ。
    let mut p = ok.clone();
    let i0 = p.init[0].index();
    let r = keleut_lir::Reg(p.funcs[i0].regs.len() as u32);
    p.funcs[i0].regs.push(keleut_lir::Rep::Any);
    p.funcs[i0].params.push(r);
    p.funcs[i0].frame = None;
    rejects(&p, "初期化の関数");

    // 初期化の関数を直に呼ぶ。
    let mut p = ok.clone();
    for b in &mut p.funcs[f].blocks {
        for i in &mut b.insts {
            if let Inst::CallDirect { func, args, .. } = i {
                *func = ok.init[0];
                args.clear();
            }
        }
    }
    rejects(&p, "初期化の関数");

    // 19 番は error に使わない。
    let mut p = ok.clone();
    let last = p.funcs[f].blocks.len() - 1;
    p.funcs[f].blocks[last].term = Term::Error {
        kind: RtError::OutOfMemory,
        operand: None,
    };
    p.funcs[f].frame = None;
    rejects(&p, "19 番");

    // 組み込みのメソッドの表に無い組の辞書。
    let mut p = ok.clone();
    let pairs: std::collections::HashSet<_> =
        p.builtin_methods.iter().map(|m| (m.class, m.ty)).collect();
    let (c, t) = (0..p.classes.len())
        .flat_map(|c| (0..p.types.len()).map(move |t| (c, t)))
        .map(|(c, t)| (keleut_lir::ClassId(c as u32), keleut_lir::TypeId(t as u32)))
        .find(|x| !pairs.contains(x))
        .unwrap();
    let r = keleut_lir::Reg(p.funcs[f].regs.len() as u32);
    p.funcs[f].regs.push(keleut_lir::Rep::Any);
    p.funcs[f].blocks[0].insts.insert(
        1,
        Inst::Const {
            dst: r,
            value: keleut_lir::Const::DictBuiltin { class: c, ty: t },
        },
    );
    p.funcs[f].frame = None;
    rejects(&p, "組み込みのメソッドの表に無い組");

    // インスタンスの実装の数がクラスのメソッドの数と合わない。
    let ok = fixture("ex_dict.kir");
    keleut_lir::verify(&ok).unwrap();
    assert!(!ok.instances.is_empty());
    let mut p = ok.clone();
    p.instances[0].methods.clear();
    rejects(&p, "実装の数");
}

#[test]
fn 辞書の引数を通るメソッドの呼び出しは引数を再開点の集合に入れない() {
    // 前提を持つ利用者のインスタンスの辞書は、実行時に辞書の引数を通って `call_method` に届く。
    // `call_method` は引数を渡す前に実装の関数(確保する)を評価するが、引数は再開点の集合に入らない。
    // この間、引数と辞書は呼び出し元のフレームのスロットに残り、`doc/lir.md` の 1.8 節のとおり、印 PHASE の付いたフレームでは GC が呼び出しの命令の被演算子のスロットを根として走査する。
    let mut text = String::new();
    for line in prelude().lines() {
        text += line;
        text.push('\n');
        if line == "(type \"builtin/MutableArray\")" {
            text += "(type \"main/Box\" (ctor Box 1))\n";
        }
        if line.starts_with("(class \"builtin/Show\"") {
            text += "(class \"main/Sz\" (method sz 0))\n\
                (instance \"main/instance:Sz[Int32]\" \"main/Sz\" \"builtin/Int32\" (premises 0) \
                (method sz 0 \"main/instance:Sz[Int32].sz\"))\n\
                (instance \"main/instance:Sz[Box]\" \"main/Sz\" \"main/Box\" (premises 1) \
                (method sz 0 \"main/instance:Sz[Box].sz\"))\n";
        }
    }
    text += "(global \"main/f\" dictabs \"main/f\")\n\
        (global \"main/r\" value i32)\n\
        (fn \"main/instance:Sz[Int32].sz\" dictabs yield (params) (locals) (dicts 0) (caps) (result any) \
        (closure \"main/instance:Sz[Int32].sz$0\"))\n\
        (fn \"main/instance:Sz[Int32].sz$0\" lambda yield (params i32) (locals) (dicts 0) (caps) (result i32) (local 0))\n\
        (fn \"main/instance:Sz[Box].sz\" dictabs yield (params) (locals) (dicts 1) (caps) (result any) \
        (closure \"main/instance:Sz[Box].sz$0\" (dparam 0)))\n\
        (fn \"main/instance:Sz[Box].sz$0\" lambda yield (params any) (locals any) (dicts 0) (caps any) (result i32) \
        (match (local 0) (clause (ctor \"main/Box\" Box (field 0 (bind 1))) \
        (opcall \"builtin/Add\" add (dbuiltin \"builtin/Add\" \"builtin/Int32\") \
        (call ntail yield (method \"main/Sz\" sz (env 0)) (local 1)) (lit i32 1)))))\n\
        (fn \"main/f\" dictabs yield (params) (locals) (dicts 1) (caps) (result any) \
        (closure \"main/f$0\" (dparam 0)))\n\
        (fn \"main/f$0\" lambda yield (params any) (locals) (dicts 0) (caps any) (result i32) \
        (opcall \"builtin/Add\" add (dbuiltin \"builtin/Add\" \"builtin/Int32\") \
        (call ntail yield (method \"main/Sz\" sz (env 0)) (local 0)) (lit i32 1)))\n\
        (fn \"main/r\" thunk yield (params) (locals) (dicts 0) (caps) (result i32) \
        (call ntail noyield (dictapp (global \"main/f\") \
        (dinst \"main/instance:Sz[Box]\" (dinst \"main/instance:Sz[Box]\" (dinst \"main/instance:Sz[Int32]\")))) \
        (construct \"main/Box\" Box (field 0 (construct \"main/Box\" Box (field 0 (lit i32 3)))))))\n\
        (init \"builtin\")\n(init \"prelude\")\n\
        (init \"main\" (def_instance \"main/instance:Sz[Int32]\") (def_instance \"main/instance:Sz[Box]\") \
        (def_fns \"main/f\") (def_value \"main/r\" \"main/r\"))\n";
    let k = kir::load(text.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let p = keleut_lir::lower(&k).unwrap_or_else(|e| panic!("{e}"));
    // 実装の関数の本体はクロージャを作る確保で、安全点を持つ。
    let imp = func(&p, "main/instance:Sz[Box].sz");
    assert_eq!(count(imp, |i| matches!(i, Inst::ClosureNew { .. })), 1);
    assert_eq!(imp.frame.as_ref().unwrap().safepoints.len(), 1);
    for sym in ["main/f$0", "main/instance:Sz[Box].sz$0"] {
        let f = func(&p, sym);
        let fr = f.frame.as_ref().unwrap();
        let mut seen = 0;
        for (pt, live) in fr.resume_points.iter() {
            if let Inst::CallMethod {
                dict, own, args, ..
            } = &f.blocks[pt.block.index()].insts[pt.index as usize]
            {
                assert!(own.is_empty());
                assert_eq!(args.len(), 1);
                assert!(!live.contains(&args[0]), "{sym}: {live:?}");
                assert!(!live.contains(dict), "{sym}: {live:?}");
                seen += 1;
            }
        }
        assert_eq!(seen, 1, "{sym}");
    }
}
