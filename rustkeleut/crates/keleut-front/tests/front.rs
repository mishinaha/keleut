//! keleut-front の字句解析と構文解析のテストである。
//!
//! 期待する文字列は、Diktor の `--dump-tokens` と `--dump-ast` の出力をコピーしたものである。
//! 環境変数 `KELEUT_FRONT_DIKTOR` に本物の diktor のパスを設定すると、`KELEUT_FRONT_CORPUS`(既定は `conformance/corpus`)のすべての `.kel` ファイルでも、Diktor の `--dump-ast` の出力と比較する。

use keleut_front::{Files, Limits, ParseError, dump, oformat::Formatter, on_stack};

/// `keleut dump ast` と同じ出力を作る。
/// 字句解析か構文解析に失敗したときは、診断の 1 行を返す。
fn ast(src: &str) -> Result<String, String> {
    ast_with(src.as_bytes(), keleut_front::DEFAULT_MAX_NEST)
}

fn ast_with(src: &[u8], max_nest: usize) -> Result<String, String> {
    let src = src.to_vec();
    on_stack(Limits::for_nest(max_nest).stack_bytes, move || {
        let mut files = Files::new();
        let id = files.add("t.kel");
        match keleut_front::parse(&src, id, &files, max_nest) {
            Ok(p) => {
                let mut out = dump::import_lines(&p.imports);
                let mut f = Formatter::new();
                for d in &p.decls {
                    out.extend(dump::decl_text(&mut f, d));
                }
                Ok(String::from_utf8(out).unwrap())
            }
            Err(ParseError::Lex(e)) => Err(format!("{}: 字句エラー: {}", files.show(e.pos), e.msg)),
            Err(ParseError::Syntax(m) | ParseError::Internal(m)) => Err(m),
        }
    })
    .unwrap()
}

#[test]
fn トークンの列() {
    let src = "let f(x: Int32): Int32 = x + 1\nf(2) { y => y }\nlet t = (1, \"あ\\n\")\n";
    let toks = keleut_front::tokens(src.as_bytes(), 1).unwrap();
    let text = String::from_utf8(dump::tokens_text(&toks)).unwrap();
    let shown: Vec<&str> = text.lines().map(|l| l.trim_start()).collect();
    assert_eq!(shown[0], "1  let");
    assert!(shown.contains(&"2  {blk"));
    assert!(shown.contains(&"3  \"\\227\\129\\130\\n\""));
    assert_eq!(shown.last(), Some(&"4  <EOF>"));
    assert_eq!(shown.iter().filter(|l| l.ends_with("<NL>")).count(), 2);
}

#[test]
fn ast_の印字() {
    let src = "let f(x: Int32): Int32 = x + 1\nf(2) { y => y }\nlet t = (1, \"あ\\n\")\n";
    assert_eq!(
        ast(src).unwrap(),
        "(dlet (binding f (params (pannot x Int32)) : Int32 = (+ x 1)))\n\
         (exp (apply f (extend _item 2 (extend _item (fn (y) y) {}))))\n\
         (dlet (binding t = (extend _item 1 (extend _item \"\\227\\129\\130\\n\" {}))))\n"
    );
}

#[test]
fn 構文エラーの文面() {
    assert_eq!(
        ast("let then = 1\n").unwrap_err(),
        "t.kel:1:5: 構文エラー: 予約語 then はここには書けません(予約語は識別子に使えません)"
    );
    assert_eq!(
        ast("let x = 0b102\n").unwrap_err(),
        "t.kel:1:9: 構文エラー: 数値リテラル 0b10 の直後に 2 が続いています(数値リテラルの表記を確認してください)"
    );
    assert_eq!(
        ast("let x = \"abc").unwrap_err(),
        "t.kel:1:13: 字句エラー: 文字列リテラルが閉じていません"
    );
}

#[test]
fn 深い入力は限度で止まる() {
    let limit = 1000;
    let deep = format!("let x = {}1{}", "1 + (".repeat(2000), ")".repeat(2000));
    assert!(
        ast_with(deep.as_bytes(), limit)
            .unwrap_err()
            .contains("処理系の限度: 構文解析で再帰が深すぎます")
    );
    let sum = format!("let x = 1{}", "+1".repeat(2000));
    assert!(
        ast_with(sum.as_bytes(), limit)
            .unwrap_err()
            .contains("処理系の限度")
    );
    let ok = format!("let x = 1{}", "+1".repeat(900));
    assert!(ast_with(ok.as_bytes(), limit).is_ok());
}

#[test]
fn 平らな長い列は限度に数えない() {
    let limit = 1000;
    let parens = format!("let x = {}1{}", "(".repeat(5000), ")".repeat(5000));
    assert!(ast_with(parens.as_bytes(), limit).is_ok());
    let tops: String = (0..5000).map(|i| format!("let a{i} = {i}\n")).collect();
    assert!(ast_with(tops.as_bytes(), limit).is_ok());
    // 引数の並びは `extend` の連鎖に脱糖されるので、木の深さは引数の個数になる。
    let args = format!("let x = f({}1)", "1, ".repeat(900));
    assert!(ast_with(args.as_bytes(), limit).is_ok());
    let block = format!("let x = {{\n{}  1\n}}\n", "  let a = 1\n".repeat(5000));
    assert!(ast_with(block.as_bytes(), limit).is_err());
}

#[test]
fn 既定の限度の深さを既定のスタックで印字できる() {
    let n = keleut_front::DEFAULT_MAX_NEST - 10;
    let sum = format!("let x = 1{}", "+1".repeat(n - 1));
    let out = ast_with(sum.as_bytes(), keleut_front::DEFAULT_MAX_NEST).unwrap();
    assert!(out.starts_with("(dlet"));
}

#[test]
fn 文脈は限度の失敗の後も使えて番号を続けて振る() {
    let mut files = Files::new();
    let a = files.add("a.kel");
    let b = files.add("b.kel");
    let c = files.add("c.kel");
    let mut cx = keleut_front::actions::Ctx::new(10);
    let deep = format!("let x = {}1", "-".repeat(20));
    match keleut_front::parse_with(&mut cx, deep.as_bytes(), a, &files) {
        Err(ParseError::Syntax(m)) => assert_eq!(
            m,
            "a.kel: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の上限 10 を超えました)"
        ),
        r => panic!("上限で失敗するはずです: {r:?}"),
    }
    let p = keleut_front::parse_with(&mut cx, b"let y = 1", b, &files).unwrap();
    let q = keleut_front::parse_with(&mut cx, b"let z = 2", c, &files).unwrap();
    let (i, j) = (p.decls[0].meta.id, q.decls[0].meta.id);
    assert!(i < j, "番号が重なっています: {i} と {j}");
    assert!(j < cx.next_id());
    assert_eq!(cx.limit(), 10);
}

#[test]
fn 不正な_utf8_は位置を付けて拒否する() {
    assert_eq!(
        ast_with(b"let ab\xff = 1", 100).unwrap_err(),
        "t.kel:1:7: 字句エラー: 不正な UTF-8 バイト列です"
    );
}

#[test]
fn diktor_と全件で比べる() {
    let Some(dk) = std::env::var_os("KELEUT_FRONT_DIKTOR") else {
        return;
    };
    let dir = std::env::var_os("KELEUT_FRONT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/corpus")
        });
    let mut stack = vec![dir];
    let mut n = 0;
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "kel") {
                continue;
            }
            let o = std::process::Command::new(&dk)
                .arg("--dump-ast")
                .arg(&p)
                .output()
                .unwrap();
            let src = std::fs::read(&p).unwrap();
            let got = ast_with(&src, keleut_front::DEFAULT_MAX_NEST);
            if o.status.code() == Some(0) {
                assert_eq!(
                    got.as_deref(),
                    Ok(String::from_utf8_lossy(&o.stdout).as_ref()),
                    "{}",
                    p.display()
                );
            } else {
                assert!(got.is_err(), "{}", p.display());
            }
            n += 1;
        }
    }
    assert!(n > 0);
}
