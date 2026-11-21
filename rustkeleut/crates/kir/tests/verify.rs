//! 検証(IRSpec 13.8 節)のテスト。
//!
//! Diktor が書き出した正しいプログラムのテキストを 1 か所ずつ書き換え、検証器が拒否することを確かめる。
//! 書き換えたテキストは構文が正しく、読み込みには通るので、拒否するのは検証器である。
//! 環境変数 `KIR_DIKTOR` に Diktor の実行ファイルを指定すると、同じプログラムのバイナリを `diktor --run-ir` にも渡し、Diktor も終了コード 65 で拒否することを確かめる。
//! 検証器が受理する書き換えでは、Diktor の終了コードが 65 でないことを確かめる。

mod common;

use kir::ErrorKind;

/// 検証器が拒否する書き換えの表。
///
/// 各項目は (フィクスチャの名前, 書き換える前の文字列, 書き換えた後の文字列, 壊す内容の説明) の組である。
/// 書き換える前の文字列が `*` で始まれば、`*` を除いた文字列が現れる箇所をすべて書き換える。
/// そうでなければ、最初に現れる箇所だけを書き換える。
const MUTATIONS: &[(&str, &str, &str, &str)] = &[
    (
        "ex_handle",
        r#"(op ask T "main/once$h0.op.ask")"#,
        r#"(op ask G "main/once$h0.op.ask")"#,
        "分類 T を G と書く",
    ),
    (
        "ex_handle",
        r#"(op ask G "main/twice$h0.op.ask")"#,
        r#"(op ask T "main/twice$h0.op.ask")"#,
        "分類 G を T と書く",
    ),
    (
        "ex_handle",
        r#"(op fail N "main/once$h0.op.fail")"#,
        r#"(op fail T "main/once$h0.op.fail")"#,
        "分類 N を T と書く",
    ),
    (
        "ex_handle",
        "(resume tail (lit i32 1))",
        "(resume ntail (lit i32 1))",
        "背骨の上の resume を ntail と書く",
    ),
    (
        "ex_handle",
        "(resume ntail (lit i32 2))",
        "(resume tail (lit i32 2))",
        "背骨の外の resume を tail と書く",
    ),
    (
        "ex_handle",
        r#"(handle tail "main/Ask""#,
        r#"(handle ntail "main/Ask""#,
        "末尾の handle を ntail と書く",
    ),
    (
        "ex_handle",
        "(call ntail yield (env 0)))",
        "(call tail yield (env 0)))",
        "handlebody の最も外側の call を tail と書く",
    ),
    (
        "ex_handle",
        r#"(result i32) (perform "main/Ask" ask))"#,
        r#"(result i32) (resume ntail (lit i32 0)))"#,
        "種類 lambda の関数の本体の resume",
    ),
    (
        "ex_handle",
        r#"(perform "main/Ask" fail (lit i32 7))"#,
        r#"(perform "main/Ask" fail)"#,
        "perform の引数の数",
    ),
    (
        "ex_handle",
        r#"(body "main/once$h0.body" (local 0))"#,
        r#"(body "main/once$h0.body")"#,
        "キャプチャの数",
    ),
    (
        "ex_handle",
        "  (caps any)\n  (result i32)\n  (call ntail yield (env 0)))\n(fn \"main/once$h0.op.ask\"",
        "  (caps i32)\n  (result i32)\n  (call ntail yield (env 0)))\n(fn \"main/once$h0.op.ask\"",
        "キャプチャの Rep",
    ),
    (
        "ex_handle",
        "(call ntail yield (env 0)))",
        "(call ntail yield (env 1)))",
        "env が範囲外",
    ),
    (
        "ex_handle",
        "  (pats (wild))\n  (local 0))\n(fn \"main/once$h0.return\"",
        "  (pats (wild))\n  (local 1))\n(fn \"main/once$h0.return\"",
        "local が範囲外",
    ),
    (
        "ex_handle",
        "  (pats (wild))\n  (local 0))\n(fn \"main/once$h0.return\"",
        "  (pats)\n  (local 0))\n(fn \"main/once$h0.return\"",
        "pats の数",
    ),
    (
        "ex_handle",
        "(fn \"main/r1\" thunk yield\n  (params)\n  (locals)\n  (dicts 0)\n  (caps)",
        "(fn \"main/r1\" thunk yield\n  (params)\n  (locals)\n  (dicts 0)\n  (caps any)",
        "ルート関数のキャプチャ",
    ),
    (
        "ex_handle",
        r#"(global "main/r1" value i32)"#,
        r#"(global "main/r1" value any)"#,
        "def_value の Rep",
    ),
    (
        "ex_handle",
        r#"(def_fns "main/once") (def_fns "main/twice")"#,
        r#"(def_fns "main/once") (def_fns "main/once" "main/twice")"#,
        "グローバルを 2 回定義する",
    ),
    (
        "ex_handle",
        "(op ask 0 resume user)",
        "(op ask 0 resume runtime)",
        "runtime の印",
    ),
    (
        "ex_handle",
        r#"(fn "main/r2$0" lambda"#,
        r#"(fn "main/r2$0" thunk"#,
        "関数の種類",
    ),
    (
        "ex_handle",
        r#"(fn "main/r2$0" lambda yield (params) (locals) (dicts 0)"#,
        r#"(fn "main/r2$0" lambda yield (params) (locals) (dicts 1)"#,
        "種類 lambda の関数の辞書の数",
    ),
    (
        "ex_handle",
        r#"(type "builtin/Never")"#,
        r#"(type "builtin/Nevr")"#,
        "組み込みの型の表",
    ),
    (
        "ex_handle",
        r#"(opcall "builtin/Add" add (dbuiltin "builtin/Add" "builtin/Int32") (local 0) (lit i32 100))"#,
        r#"(opcall "builtin/Add" add (dbuiltin "builtin/Mul" "builtin/Int32") (local 0) (lit i32 100))"#,
        "辞書のクラス",
    ),
    (
        "ex_handle",
        r#"(opcall "builtin/Add" add (dbuiltin "builtin/Add" "builtin/Int32") (local 0) (lit i32 100))"#,
        r#"(opcall "builtin/Add" add (dbuiltin "builtin/Add" "builtin/Boolean") (local 0) (lit i32 100))"#,
        "付録 A.3 にない組み込みのインスタンス",
    ),
    (
        "ex_handle",
        "(let 0\n    (resume ntail (lit i32 2))",
        "(let 0\n    (resume ntail (local 0))",
        "let の初期化式で自分のスロットを読む",
    ),
    (
        "ex_handle",
        "(let 0\n    (resume ntail (lit i32 2))",
        "(let 1\n    (resume ntail (lit i32 2))",
        "スロットの番号の順",
    ),
    (
        "ex_handle",
        "  (locals i32)\n  (dicts 0)\n  (caps)\n  (result i32)\n  (pats)\n  (let 0",
        "  (locals i32 i32)\n  (dicts 0)\n  (caps)\n  (result i32)\n  (pats)\n  (let 0",
        "束縛されないスロット",
    ),
    (
        "ex_handle",
        r#"(op fail 1 never user)"#,
        r#"(op fail 1 resume user)"#,
        "never の印と分類 N",
    ),
    (
        "ex_closure",
        "        (field 0 (bind 2))\n        (field 1 (bind 3)))",
        "        (field 1 (bind 2))\n        (field 0 (bind 3)))",
        "ctor のパターンのフィールドの順",
    ),
    (
        "ex_dict",
        "(construct \"main/Box\" Box\n      (field 0 (lit i32 5)))",
        "(construct \"main/Box\" Box)",
        "construct のフィールドが足りない",
    ),
    (
        "ex_dict",
        r#"(dinst "main/instance:Size[Box]" (dinst "main/instance:Size[Int32]"))"#,
        r#"(dinst "main/instance:Size[Box]")"#,
        "dinst の前提の数",
    ),
    (
        "ex_dict",
        r#"(dinst "main/instance:Size[Box]" (dinst "main/instance:Size[Int32]")))"#,
        r#"(dinst "main/instance:Size[Box]" (dinst "main/instance:Size[Int32]")) (dparam 0))"#,
        "dictapp の辞書の数",
    ),
    (
        "ex_dict",
        "      (field k (dbuiltin \"builtin/Eq\" \"builtin/Int32\"))\n      (field v (dbuiltin \"builtin/Eq\" \"builtin/Boolean\")))",
        "      (field v (dbuiltin \"builtin/Eq\" \"builtin/Boolean\"))\n      (field k (dbuiltin \"builtin/Eq\" \"builtin/Int32\")))",
        "drecord のラベルの順",
    ),
    (
        "ex_dict",
        r#"(premises 1) (method size 0"#,
        r#"(premises 0) (method size 0"#,
        "実装の関数の辞書の数",
    ),
    (
        "ex_dict",
        r#"(class "main/Size" (method size 0))"#,
        r#"(class "main/Size" (method size 0) (method other 0))"#,
        "インスタンスがすべてのメソッドを実装していない",
    ),
    (
        "ex_dict",
        r#"(def_instance "main/instance:Size[Int32]") "#,
        "",
        "インスタンスを定義しない",
    ),
    (
        "ex_handle",
        "*\"main/r3\"",
        "\"r3\"",
        "単位の鍵/名前 の形式でないシンボル",
    ),
    ("ex_handle", "*\"prelude\"", "\"prelux\"", "単位の並びの形"),
    (
        "ex_handle",
        "*\"main/r3\"",
        "\"prelude/r3\"",
        "ほかの単位のグローバルを定義する",
    ),
    (
        "ir_emit_008",
        "      (field 0 (bind 0))\n      (field 1 (bind 1))))",
        "      (field 0 (bind 1))\n      (field 1 (bind 0))))",
        "def_pattern の bind の順",
    ),
    (
        "ir_emit_008",
        "    (globals \"main/a\" \"main/b\")\n    (ctor \"main/P\" P\n      (field 0 (bind 0))\n      (field 1 (bind 1))))",
        "    (globals \"main/a\" \"main/b\")\n    (ctor \"main/P\" P\n      (field 0 (bind 0))\n      (field 1 (wild))))",
        "def_pattern がグローバルを束縛しない",
    ),
    (
        "sample",
        "    (rec 1\n      (dictabs \"prelude/instance:Show[List].show$1\" (local 1)))",
        "    (rec 1\n      (not (dictabs \"prelude/instance:Show[List].show$1\" (local 1))))",
        "letrec の項目が closure でない",
    ),
    (
        "sample",
        "(method \"builtin/Show\" show (env 0))",
        "(method \"builtin/Show\" show (env 0) (env 0))",
        "method の自身の辞書の数",
    ),
    (
        "strings",
        "          (field Big (dbuiltin \"builtin/Eq\" \"builtin/Int32\"))\n          (field Small (drecord)))",
        "          (field Small (drecord))\n          (field Big (dbuiltin \"builtin/Eq\" \"builtin/Int32\")))",
        "dvariant のラベルの順",
    ),
    (
        "strings",
        "          (field Big (dbuiltin \"builtin/Eq\" \"builtin/Int32\"))",
        "          (field Big (dbuiltin \"builtin/Show\" \"builtin/Int32\"))",
        "構造的な辞書の項目のクラス",
    ),
    (
        "ir_emit_006",
        "  (guard\n    (opcall \"builtin/Ord\" lt (dbuiltin \"builtin/Ord\" \"builtin/Int32\") (local 0) (lit i32 0)))",
        "  (guard (resume ntail (rec_empty)))",
        "操作節のガードの resume",
    ),
    (
        "ir_emit_006",
        "  (guard\n    (opcall \"builtin/Ord\" lt (dbuiltin \"builtin/Ord\" \"builtin/Int32\") (local 0) (lit i32 0)))\n  (let 1",
        "  (guard (handle ntail \"main/St\" (body \"main/st$h0.op.put$h9.body\") (op get T \"main/st$h0.op.put$h9.op.get\")))\n  (let 1",
        "操作節のガードの中のハンドラ式の本体の resume",
    ),
];

/// 検証器が受理する書き換えの表。項目の形式は `MUTATIONS` と同じである。
///
/// ハンドラ式の本体の中の `resume` は、そのハンドラ式が操作節の本体の中にあれば書ける。
const ACCEPTED: &[(&str, &str, &str, &str)] = &[(
    "ir_emit_006",
    "  (let 1\n    (resume ntail (rec_empty))",
    "  (let 1\n    (handle ntail \"main/St\" (body \"main/st$h0.op.put$h9.body\") (op get T \"main/st$h0.op.put$h9.op.get\"))",
    "操作節の本体の中のハンドラ式の本体の resume",
)];

/// 書き換えで追加する関数。
/// 書き換えた後の文字列が `$h9` を含むとき、`ir_emit_006` の関数 `main/st$h0.op.put#1` の直前に挿入する。
const EXTRA_FNS: &str = "(fn \"main/st$h0.op.put$h9.body\" handlebody yield (params) (locals) (dicts 0) (caps) (result any) (resume ntail (rec_empty)))\n(fn \"main/st$h0.op.put$h9.op.get\" opclause yield (params) (locals) (dicts 0) (caps) (result i32) (pats) (lit i32 0))\n";

fn mutate(fixture: &str, old: &str, new: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(common::fixtures().join(format!("{fixture}.kirt"))).unwrap();
    let (all, old) = match old.strip_prefix('*') {
        Some(o) => (true, o),
        None => (false, old),
    };
    assert!(
        text.contains(old),
        "{fixture}: 書き換える前の文字列がありません: {old}"
    );
    let mut t = if all {
        text.replace(old, new)
    } else {
        text.replacen(old, new, 1)
    };
    if new.contains("$h9") {
        let at = t.find("(fn \"main/st$h0.op.put#1\"").unwrap();
        t.insert_str(at, EXTRA_FNS);
    }
    t.into_bytes()
}

fn diktor_run_ir(bin: &[u8], tag: usize) -> Option<i32> {
    let d = std::env::var("KIR_DIKTOR").ok()?;
    let dir = std::env::temp_dir().join(format!("kir-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(format!("m{tag}.kir"));
    std::fs::write(&f, bin).unwrap();
    let out = std::process::Command::new(d)
        .arg("--run-ir")
        .arg(&f)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    // 並行して動くテストがまだ使っていれば、ディレクトリは空でないので削除できない。最後に終わるテストが削除する。
    let _ = std::fs::remove_dir(&dir);
    out.status.code()
}

#[test]
fn originals_are_valid() {
    for (name, kir, kirt) in common::pairs(&common::fixtures()) {
        let p = kir::read_kirt(&kirt).unwrap();
        kir::verify(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
        let _ = kir;
    }
}

#[test]
fn mutations_are_rejected() {
    let mut bad = Vec::new();
    for (i, (fixture, old, new, what)) in MUTATIONS.iter().enumerate() {
        let text = mutate(fixture, old, new);
        let p = match kir::read_kirt(&text) {
            Ok(p) => p,
            Err(e) => {
                bad.push(format!(
                    "{what}: 読み込みで拒否されました(書き換えの誤り): {e}"
                ));
                continue;
            }
        };
        match kir::verify(&p) {
            Ok(()) => bad.push(format!("{what}: 検証が受理しました")),
            Err(e) => {
                assert_eq!(e.kind(), ErrorKind::Invalid, "{what}: {e}");
                assert_eq!(e.exit_code(), 65);
                eprintln!("{what}: {e}");
            }
        }
        if let Ok(bin) = kir::write_kir(&p)
            && let Some(code) = diktor_run_ir(&bin, i)
            && code != 65
        {
            bad.push(format!("{what}: Diktor の終了コードが {code} です"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn accepted_rewrites() {
    for (i, (fixture, old, new, what)) in ACCEPTED.iter().enumerate() {
        let text = mutate(fixture, old, new);
        let p = kir::read_kirt(&text).unwrap_or_else(|e| panic!("{what}: {e}"));
        kir::verify(&p).unwrap_or_else(|e| panic!("{what}: {e}"));
        let bin = kir::write_kir(&p).unwrap();
        if let Some(code) = diktor_run_ir(&bin, 1000 + i) {
            assert_ne!(code, 65, "{what}: Diktor が拒否しました");
        }
    }
}

/// 生成箇所を順にたどると循環する `handlebody` の関数には持ち主がないので、検証器はその中の `resume` を拒否する。
/// このテストは RustKeleut の検証器だけを確かめ、プログラムを Diktor には渡さない。
#[test]
fn cyclic_handlebody_has_no_owner() {
    let t = std::fs::read_to_string(common::fixtures().join("ex_handle.kirt")).unwrap();
    let extra = concat!(
        "(fn \"main/cyc$h0.body\" handlebody yield (params) (locals) (dicts 0) (caps) (result any) ",
        "(seq (handle ntail \"main/Ask\" (body \"main/cyc$h1.body\") (op ask T \"main/cyc$h1.op.ask\")) (resume ntail (rec_empty))))\n",
        "(fn \"main/cyc$h0.op.ask\" opclause yield (params) (locals) (dicts 0) (caps) (result i32) (pats) (lit i32 0))\n",
        "(fn \"main/cyc$h1.body\" handlebody yield (params) (locals) (dicts 0) (caps) (result any) ",
        "(handle ntail \"main/Ask\" (body \"main/cyc$h0.body\") (op ask T \"main/cyc$h0.op.ask\")))\n",
        "(fn \"main/cyc$h1.op.ask\" opclause yield (params) (locals) (dicts 0) (caps) (result i32) (pats) (lit i32 0))\n",
    );
    let i = t.find("(init \"builtin\")").unwrap();
    let text = format!("{}{extra}{}", &t[..i], &t[i..]);
    let p = kir::read_kirt(text.as_bytes()).unwrap();
    let e = kir::verify(&p).unwrap_err();
    assert!(
        e.message()
            .contains("resume を書けない関数に resume があります"),
        "{e}"
    );
}

/// `handlebody` の関数が長く連なっていても、持ち主を求める時間は連鎖の長さに比例する。
///
/// 操作節 `main/twice$h0.op.ask` の中の `handle` が `main/c0` を作り、`main/c{i}` の中の `handle` が `main/c{i+1}` を作る。
/// どの `main/c{i}` も `resume` を含むので、関数ごとに連鎖をたどり直すと、連鎖の長さの 2 乗に比例する時間がかかる。
/// 正しいプログラムなので、検証器は受理する。
/// 連鎖の長さを 200,000 にして、検証が 30 秒未満で終わることを確かめる。
#[test]
fn long_handlebody_chain_is_linear() {
    const N: usize = 200_000;
    let t = std::fs::read_to_string(common::fixtures().join("ex_handle.kirt")).unwrap();
    let mut out = String::with_capacity(t.len() + N * 300);
    let mut lines = t.split('\n').peekable();
    while let Some(l) = lines.next() {
        if l.starts_with("(fn \"main/twice$h0.op.ask\"") {
            out.push_str(concat!(
                "(fn \"main/twice$h0.op.ask\" opclause yield (params) (locals) (dicts 0) (caps) (result i32) (pats) ",
                "(seq (handle ntail \"main/Ask\" (body \"main/c0\") (op ask T \"main/k0\")) (resume tail (lit i32 2))))\n",
            ));
            while lines.peek().is_some_and(|l| !l.starts_with("(fn ")) {
                lines.next();
            }
            continue;
        }
        if l.starts_with("(init \"builtin\")") {
            for j in 0..N {
                if j + 1 < N {
                    out.push_str(&format!(
                        "(fn \"main/c{j}\" handlebody yield (params) (locals) (dicts 0) (caps) (result any) (seq (handle ntail \"main/Ask\" (body \"main/c{}\") (op ask T \"main/k{}\")) (resume ntail (rec_empty))))\n",
                        j + 1,
                        j + 1
                    ));
                } else {
                    out.push_str(&format!(
                        "(fn \"main/c{j}\" handlebody yield (params) (locals) (dicts 0) (caps) (result any) (resume ntail (rec_empty)))\n"
                    ));
                }
                out.push_str(&format!(
                    "(fn \"main/k{j}\" opclause yield (params) (locals) (dicts 0) (caps) (result i32) (pats) (lit i32 0))\n"
                ));
            }
        }
        out.push_str(l);
        out.push('\n');
    }
    let p = kir::read_kirt(out.as_bytes()).unwrap();
    let start = std::time::Instant::now();
    kir::verify(&p).unwrap();
    let took = start.elapsed();
    assert!(took.as_secs() < 30, "検証に {took:?} かかりました");
}

/// プログラム `p` から、先頭の `k` 個の単位と、その単位に属する表の項目だけを残したプログラムを返す。
/// 表の項目は単位の順に並ぶので、残る項目は各表の先頭の部分である。
fn prefix(p: &kir::Program, k: usize) -> kir::Program {
    let keys: Vec<&str> = p.units[..k].iter().map(|u| u.key.as_str()).collect();
    let mine = |s: &str| keys.contains(&kir::unit_of_symbol(s));
    let mut q = p.clone();
    q.units.truncate(k);
    q.types.retain(|t| mine(&t.sym));
    q.effects.retain(|t| mine(&t.sym));
    q.classes.retain(|t| mine(&t.sym));
    q.instances.retain(|t| mine(&t.sym));
    q.globals.retain(|t| mine(&t.sym));
    q.functions.retain(|t| mine(&t.sym));
    q
}

/// 単位を追加していく検証([`kir::verify::Extension`])は、全体を 1 回で渡しても、単位を 1 つずつ追加しても、正しいプログラムを受理する。
/// 検証済みの単位と同じキーの単位を追加すると拒否し、拒否した後も検証の状態は変わらない。
/// `MUTATIONS` で書き換えたプログラムは拒否する。
/// ただし、単位の並びが `main` で終わらないことのほかに誤りがないプログラムは受理する。
#[test]
fn extension_agrees_with_verify() {
    let lim = kir::Limits::default();
    for (name, _, kirt) in common::pairs(&common::fixtures()) {
        let p = kir::read_kirt(&kirt).unwrap();
        let mut e = kir::verify::Extension::new();
        kir::verify_extension(&mut e, &p, &lim).unwrap_or_else(|x| panic!("{name}: {x}"));
        let mut e = kir::verify::Extension::new();
        for k in 2..=p.units.len() {
            let q = prefix(&p, k);
            kir::verify_extension(&mut e, &q, &lim)
                .unwrap_or_else(|x| panic!("{name} の {k} 個の単位: {x}"));
        }
        assert_eq!(e.done(), kir::verify::Marks::of(&p), "{name}");
        // 同じキーの単位をもう 1 つ追加する。
        let mut q = p.clone();
        let last = q.units.last().unwrap().clone();
        q.units.push(last);
        let done = e.done();
        assert!(kir::verify_extension(&mut e, &q, &lim).is_err(), "{name}");
        assert_eq!(e.done(), done, "{name}");
        kir::verify_extension(&mut e, &p, &lim).unwrap_or_else(|x| panic!("{name}: {x}"));
    }
    for (fixture, old, new, what) in MUTATIONS {
        let text = mutate(fixture, old, new);
        let Ok(p) = kir::read_kirt(&text) else {
            continue;
        };
        let mut e = kir::verify::Extension::new();
        if let Err(x) = kir::verify_extension(&mut e, &p, &lim) {
            assert_eq!(x.kind(), ErrorKind::Invalid, "{what}: {x}");
        } else {
            // 単位の並びが `main` で終わることだけは、単位を追加していく検証では検査しない。
            assert!(
                kir::verify(&p).unwrap_err().to_string().contains("main"),
                "{what}"
            );
        }
    }
}
