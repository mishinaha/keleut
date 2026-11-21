//! 深い入れ子の KIR を LIR に変換するテスト。
//!
//! `conformance/kir-bad/deep-base.kirt` の `main/total` の本体を、深さ `N_nest` の連鎖に置き換えて LIR に変換する。
//! 変換器の再帰が、[`keleut_lir::STACK_BYTES_PER_LEVEL`] から見積もったスタックに収まることと、生存解析と検証が深い入力でも現実的な時間とメモリで終わることを確かめる。
//! 値を保持したまま確保や呼び出しを重ねる連鎖では、再開点と安全点の集合として保持するレジスタの個数と、パス 12 の生存解析の処理量と、検証器が行う定義の検査の処理量([`keleut_lir::dataflow_work`])が、関数のサイズに比例することも確かめる。
//! レコードの連鎖では、`rec_new` のオペランドの個数とシェイプ表のサイズがプログラムのサイズに比例することも確かめる。
//! 浅い連鎖では、再開点と安全点の集合を、ブロックごとに集合の全体を持つ単純な生存解析の結果と比べる。

use std::path::PathBuf;

use keleut_lir::{Func, Inst, KNOWN_RECORD_FIELDS, Point, Reg};

fn base() -> String {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/kir-bad/deep-base.kirt");
    std::fs::read_to_string(p).expect("deep-base.kirt")
}

/// match を引数に取る非末尾呼び出し。
///
/// 連鎖の中では、この呼び出しの結果を保持したまま次の段を評価する。
const CALL_MATCH: &str = "(call ntail yield (global \"main/loop\") (lit i32 3) \
     (match (lit bool true) (clause (lit bool true) (lit i64 1)) (clause (wild) (lit i64 2))))";

/// 種類 `kind` の連鎖を深さ `n` で作り、KIR のテキストを返す。
pub fn chain(n: usize, kind: &str) -> String {
    // 1 段が 2 つのノード(式と、フィールドか節)からなる連鎖の段数。
    let m = n / 2 - 2;
    // コンストラクタ P のフィールドの個数。
    let mut arity = 2;
    // `main/total` のローカル変数の Rep の並び。
    // 空なら元のテキストのままにする。
    let mut locals = String::new();
    let mut body = String::from("  ");
    match kind {
        "exp" => {
            body += &"(not ".repeat(n - 1);
            body += "(lit bool true)";
            body += &")".repeat(n - 1);
        }
        "pat" => {
            body += "(match (lit i64 0) (clause ";
            body += &"(variant a ".repeat(n - 2);
            body += "(wild)";
            body += &")".repeat(n - 2);
            body += " (lit i64 0)))";
        }
        "div" => {
            body += &"(opcall \"builtin/Div\" div (dbuiltin \"builtin/Div\" \"builtin/Int64\") (lit i64 7) "
                .repeat(n - 1);
            body += "(lit i64 1)";
            body += &")".repeat(n - 1);
        }
        "dict" => {
            body += "(opcall \"builtin/Eq\" eq ";
            body += &"(drecord (field a ".repeat(n - 2);
            body += "(dbuiltin \"builtin/Eq\" \"builtin/Int32\")";
            body += &"))".repeat(n - 2);
            body += " (lit i32 0) (lit i32 0))";
        }
        // 値を保持したまま確保を重ねる連鎖。
        // 長いリストのリテラルと同じ形である。
        "construct" => {
            body += &"(construct \"main/P\" P (field 0 (lit i32 1)) (field 1 ".repeat(m);
            body += "(lit i32 0)";
            body += &"))".repeat(m);
        }
        "rec_extend" => {
            body += &"(rec_extend a (lit i64 1) ".repeat(n - 2);
            body += "(rec_empty)";
            body += &")".repeat(n - 2);
        }
        // 同じラベルのフィールドを重ねたレコードを、更新し続ける連鎖。
        "rec_update" => {
            let m = n / 2 - 4;
            body += &"(rec_update a ".repeat(m);
            body += &"(rec_extend a (lit i64 1) ".repeat(m);
            body += "(rec_empty)";
            body += &")".repeat(m);
            body += &" (lit i64 2))".repeat(m);
        }
        // 同じラベルのフィールドを重ねたレコードから、フィールドを取り除き続ける連鎖。
        "rec_restrict" => {
            let m = n / 2 - 4;
            body += &"(rec_restrict a ".repeat(m);
            body += &"(rec_extend a (lit i64 1) ".repeat(m);
            body += "(rec_empty)";
            body += &")".repeat(2 * m);
        }
        // `let` で束縛したレコードを、別のラベルで拡張し続ける連鎖。
        "rec_let" => {
            let m = n / 2 - 4;
            locals = vec!["any"; m].join(" ");
            for i in 0..m {
                let prev = if i == 0 {
                    "(rec_empty)".to_string()
                } else {
                    format!("(local {})", i - 1)
                };
                body += &format!("(let {i} (rec_extend l{i} (lit i64 1) {prev}) ");
            }
            body += &format!("(local {})", m - 1);
            body += &")".repeat(m);
        }
        // ガードの中に match を入れ子にした連鎖。
        "guard" => {
            body += &"(match (lit i64 0) (clause (wild) (guard ".repeat(m);
            body += "(lit bool true)";
            body += &") (lit bool true)))".repeat(m);
        }
        // 引数の中に非末尾呼び出しを入れ子にした連鎖。
        // 外側の呼び出しの引数が、内側の呼び出しをまたいで生存する。
        "args" => {
            body += &"(call ntail yield (global \"main/loop\") (lit i32 3) ".repeat(m);
            body += "(lit i64 0)";
            body += &")".repeat(m);
        }
        "and" => {
            body += &"(and (lit bool true) ".repeat(n - 2);
            body += "(lit bool true)";
            body += &")".repeat(n - 2);
        }
        // 文字列リテラルのパターンの照合に失敗して進む節の中に、match を入れ子にした連鎖。
        "strpat" => {
            body += &"(match (lit str \"a\") (clause (lit str \"b\") (lit i64 0)) (clause (wild) "
                .repeat(m);
            body += "(lit i64 1)";
            body += &"))".repeat(m);
        }
        // 呼び出しの結果を保持したまま、match を引数に取る呼び出しを重ねる連鎖。
        "callmatch" => {
            body += &format!("(construct \"main/P\" P (field 0 {CALL_MATCH}) (field 1 ").repeat(m);
            body += "(lit i32 0)";
            body += &"))".repeat(m);
        }
        // 1 つのコンストラクタに、match を引数に取る呼び出しのフィールドを n 個並べる。
        "wide" => {
            body += "(construct \"main/P\" P ";
            for i in 0..n {
                body += &format!("(field {i} {CALL_MATCH})");
            }
            body += ")";
            arity = n;
        }
        // 呼び出しの結果を保持したまま、match の最初の節の中に次の段を入れる連鎖。
        // 最初の節の後ろに置いた節の入口の集合は、変更ログが長く伸びた後で使われる。
        "armnest" => {
            let m = n / 4 - 2;
            body += &format!(
                "(construct \"main/P\" P (field 0 {CALL_MATCH}) (field 1 \
                 (match (lit bool true) (clause (lit bool true) "
            )
            .repeat(m);
            body += "(lit i32 0)";
            body += &") (clause (wild) (lit i32 1)))))".repeat(m);
        }
        _ => unreachable!(),
    }
    body += ")";
    let mut out = String::new();
    for line in base().lines() {
        if line.contains("(call ntail noyield (global \"main/loop\")") {
            out += &body;
        } else {
            out += line;
        }
        out.push('\n');
        if line == "(type \"builtin/MutableArray\")" {
            out += &format!("(type \"main/P\" (ctor P {arity}))\n");
        }
    }
    if !locals.is_empty() {
        // 最後の `(locals)` が `main/total` のものである。
        let at = out.rfind("(locals)").expect("locals");
        out.replace_range(at..at + "(locals)".len(), &format!("(locals {locals})"));
    }
    out
}

/// 点と、その点で生存しているレジスタの集合の組を、位置の順に並べたもの。
type Sets = Vec<(Point, Vec<Reg>)>;

/// 再開点と安全点の集合を、ブロックごとに集合の全体を持つ単純な生存解析で求める。
fn naive_points(f: &Func) -> (Sets, Sets) {
    let nr = f.regs.len();
    let mut live_in: Vec<Vec<bool>> = vec![Vec::new(); f.blocks.len()];
    let (mut rp, mut sp) = (Vec::new(), Vec::new());
    for bi in (0..f.blocks.len()).rev() {
        let b = &f.blocks[bi];
        let mut live = vec![false; nr];
        for s in b.term.succs() {
            for (r, &on) in live_in[s.index()].iter().enumerate() {
                live[r] |= on;
            }
        }
        for u in b.term.uses() {
            live[u.index()] = true;
        }
        for (ii, i) in b.insts.iter().enumerate().rev() {
            let call = i.is_call();
            let uses = i.uses();
            if call || i.allocates(&f.regs) {
                let mut set: Vec<Reg> = (0..nr as u32)
                    .map(Reg)
                    .filter(|&r| live[r.index()] && i.def() != Some(r))
                    .collect();
                if !call {
                    set.extend(&uses);
                    set.sort_unstable();
                    set.dedup();
                }
                let pt = Point {
                    block: keleut_lir::BlockId(bi as u32),
                    index: ii as u32,
                };
                if call { &mut rp } else { &mut sp }.push((pt, set));
            }
            if let Some(d) = i.def() {
                live[d.index()] = false;
            }
            for u in uses {
                live[u.index()] = true;
            }
        }
        live_in[bi] = live;
    }
    rp.reverse();
    sp.reverse();
    (rp, sp)
}

fn lower_chain(kind: &str, n: usize, max_nest: usize) {
    let text = chain(n, kind);
    let p = kir::load(text.as_bytes()).unwrap_or_else(|e| panic!("{kind}: {e}"));
    let lp = keleut_lir::lower_with(&p, max_nest).unwrap_or_else(|e| panic!("{kind}: {e}"));
    let mut total = 0;
    for f in &lp.funcs {
        let fr = f.frame.as_ref().expect("frame");
        let size: usize = f.regs.len() + f.blocks.iter().map(|b| b.insts.len() + 1).sum::<usize>();
        total += size;
        // シェイプの分かるレコードのレイアウトをコピーする `rec_new` が 1 つで持つオペランドは、`KNOWN_RECORD_FIELDS` 個以下である。
        // そのため、オペランドの個数の合計は、関数のサイズに比例する量と、`KNOWN_RECORD_FIELDS` と `rec_new` の個数の積との和を超えない。
        let (news, fields) = f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter_map(|i| match i {
                Inst::RecNew { fields, .. } => Some(fields.len()),
                _ => None,
            })
            .fold((0, 0), |(n, s), k| (n + 1, s + k));
        assert!(
            fields <= 2 * size + KNOWN_RECORD_FIELDS * news,
            "{kind}: {} の rec_new の {news} 個のオペランドが {fields} 個(関数の大きさ {size})",
            f.sym
        );
        // 再開点と安全点の集合は差分で持つので、保持するレジスタの個数は関数のサイズに比例する。
        let stored = fr.resume_points.stored_regs() + fr.safepoints.stored_regs();
        assert!(
            stored <= 8 * size + 64,
            "{kind}: {} の集合が {stored} 個(関数の大きさ {size})",
            f.sym
        );
        // 生存解析と定義の検査の処理量も、関数のサイズに比例する。
        // ただし、入口の集合を離れた位置で使うブロック(armnest の、最初の節の後ろに置いた節)では、1 つあたりの処理量が集合の幅(ワード数)に比例する。
        let (live, defs) = keleut_lir::dataflow_work(f);
        let words = (f.regs.len() / 64 + 1) as u64;
        let bound = 32 * size as u64
            + 4096
            + if kind == "armnest" {
                16 * f.blocks.len() as u64 * words
            } else {
                0
            };
        assert!(
            live <= bound && defs <= bound,
            "{kind}: {} の処理量が生存解析で {live}、定義の検査で {defs}(関数の大きさ {size})",
            f.sym
        );
        if n <= 1000 {
            let (rp, sp) = naive_points(f);
            assert_eq!(
                fr.resume_points.iter().collect::<Sets>(),
                rp,
                "{kind}: {}",
                f.sym
            );
            assert_eq!(
                fr.safepoints.iter().collect::<Sets>(),
                sp,
                "{kind}: {}",
                f.sym
            );
        }
    }
    // シェイプ表のサイズも、プログラムのサイズに比例する。
    let shapes: usize = lp.shapes.iter().map(Vec::len).sum();
    assert!(
        shapes <= 2 * total + KNOWN_RECORD_FIELDS * lp.shapes.len(),
        "{kind}: シェイプ表のラベルが {shapes} 個(関数の大きさの和 {total})"
    );
}

/// 連鎖の種類ごとに、深さ `N_nest` の連鎖を LIR に変換するテストを作る。
///
/// 種類ごとに別のテストにするのは、並列に実行するためである。
macro_rules! deep {
    ($($name:ident: $kind:literal,)*) => {
        $(
            #[test]
            fn $name() {
                lower_chain($kind, kir::N_NEST, kir::N_NEST);
            }
        )*
    };
}

deep! {
    深さ_n_nest_の_not_の鎖: "exp",
    深さ_n_nest_のヴァリアントのパターンの鎖: "pat",
    深さ_n_nest_の除算の鎖: "div",
    深さ_n_nest_の構造的な辞書の鎖: "dict",
    深さ_n_nest_のコンストラクタの鎖: "construct",
    深さ_n_nest_のレコードの拡張の鎖: "rec_extend",
    深さ_n_nest_のレコードの更新の鎖: "rec_update",
    深さ_n_nest_のレコードの制限の鎖: "rec_restrict",
    深さ_n_nest_の_let_で束ねたレコードの拡張の鎖: "rec_let",
    深さ_n_nest_のガードの鎖: "guard",
    深さ_n_nest_の引数の中の呼び出しの鎖: "args",
    深さ_n_nest_の_and_の鎖: "and",
    深さ_n_nest_の文字列のパターンの鎖: "strpat",
    深さ_n_nest_の_match_を引数に取る呼び出しの鎖: "callmatch",
    深さ_n_nest_の最初の節の中の呼び出しの鎖: "armnest",
}

#[test]
fn match_を引数に取る呼び出しを_n_nest_個並べたコンストラクタ() {
    lower_chain("wide", kir::N_NEST, kir::N_NEST);
}

/// 浅い連鎖で、再開点と安全点の集合を単純な生存解析の結果と比べる。
#[test]
fn 浅い鎖の再開点と安全点は素朴な生存解析と一致する() {
    for kind in [
        "exp",
        "pat",
        "div",
        "dict",
        "construct",
        "rec_extend",
        "rec_update",
        "rec_restrict",
        "rec_let",
        "guard",
        "args",
        "and",
        "strpat",
        "callmatch",
        "wide",
        "armnest",
    ] {
        for n in [12, 101, 400] {
            lower_chain(kind, n, kir::N_NEST);
        }
    }
}
