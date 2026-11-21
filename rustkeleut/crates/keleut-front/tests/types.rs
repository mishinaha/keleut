//! 型検査の中核(`keleut_front::types`)のテストである。
//!
//! 期待する文字列は、Diktor の `--type-check` の出力(diktor/test の typecheck.t などの期待出力)をコピーしたものである。
//! 環境変数 `KELEUT_FRONT_DIKTOR` に本物の diktor のパスを設定すると、`KELEUT_FRONT_CORPUS`(既定は `conformance/corpus`)のすべての `.kel` ファイルでも Diktor と比較する。
//! 比較するのは、プレリュードありの `--type-check` の出力(標準出力の後ろに標準エラーをつないだもの)と終了コードである。

use keleut_front::types::check::{
    CheckOptions, Stop, check_files, check_limits, check_program, on_check_stack, render_line,
};
use keleut_front::types::load::{SourceFs, StdFs, normalize_display};
use std::collections::HashMap;

/// ファイル名から内容を引く表を、ファイルシステムの代わりに読む `SourceFs` の実装である。
struct MemFs(HashMap<String, Vec<u8>>);

/// 表は、`normalize_display` で正規化したパスをキーにして引く。
/// 正規化では、`.` を取り除き、`..` を直前の要素とともに取り除く。
impl SourceFs for MemFs {
    fn read(&mut self, p: &str) -> Result<Vec<u8>, String> {
        self.0
            .get(&normalize_display(p))
            .cloned()
            .ok_or_else(|| format!("{p}: No such file or directory"))
    }
    fn exists(&mut self, p: &str) -> bool {
        self.0.contains_key(&normalize_display(p))
    }
    fn realpath(&mut self, p: &str) -> Result<String, String> {
        let k = normalize_display(p);
        if self.0.contains_key(&k) {
            Ok(format!("/mem/{k}"))
        } else {
            Err(format!("{p}: No such file or directory"))
        }
    }
}

/// 相対パスを、フィールド `0` のディレクトリからのパスとして読む `SourceFs` の実装である。
/// プロセスの作業ディレクトリを変えずに、別のディレクトリで実行したのと同じ結果を得るために使う。
/// エラーメッセージの中のパスは、渡された相対パスに戻す。
struct DirFs(std::path::PathBuf);

impl DirFs {
    fn at(&self, p: &str) -> String {
        if p.starts_with('/') {
            p.to_string()
        } else {
            self.0.join(p).to_string_lossy().into_owned()
        }
    }
}

impl SourceFs for DirFs {
    fn read(&mut self, p: &str) -> Result<Vec<u8>, String> {
        let q = self.at(p);
        StdFs
            .read(&q)
            .map_err(|m| match m.strip_prefix(q.as_str()) {
                Some(r) => format!("{p}{r}"),
                None => m,
            })
    }
    fn exists(&mut self, p: &str) -> bool {
        let q = self.at(p);
        StdFs.exists(&q)
    }
    fn realpath(&mut self, p: &str) -> Result<String, String> {
        let q = self.at(p);
        StdFs
            .realpath(&q)
            .map_err(|m| match m.strip_prefix(q.as_str()) {
                Some(r) => format!("{p}{r}"),
                None => m,
            })
    }
}

/// `files` を型検査し、その結果を Diktor の `--type-check` と同じ形式の出力と終了コードにして返す。
fn run(fs: &mut dyn SourceFs, files: Vec<String>, no_prelude: bool) -> (String, u8) {
    let opts = CheckOptions {
        no_prelude,
        prelude: None,
        files,
        import_path: Vec::new(),
        max_nest: keleut_front::DEFAULT_MAX_NEST,
    };
    match check_files(&opts, fs) {
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
        Err(Stop::Lex(m) | Stop::Syntax(m)) => (format!("{m}\n"), 2),
        Err(Stop::Input(m)) => (format!("diktor: ファイルを開けません: {m}\n"), 64),
        Err(e) => (format!("{e:?}\n"), 3),
    }
}

/// (ファイル名, ソース)の並びをメモリ上のファイルとして型検査し、[`run`] と同じ形式の出力と終了コードを返す。
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
        let mut fs = MemFs(map);
        // 最初のファイルだけをコマンド行のファイルとして渡す。
        // 残りのファイルは import 先として読まれる。
        run(&mut fs, vec![files[0].0.clone()], no_prelude)
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
        "xs : List[Int32]\nh : (Option[Int32]) => Int32\n⚠ match が非網羅的です。たとえば None が漏れています\n_ : {}\n"
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
fn import_した名前を型検査する() {
    let lib = "pub let x: Int32 = 1\nlet hidden: Int32 = 2\npub let f(o: Option[Int32]): Int32 = o match { case Some(n) => n }\n";
    let (out, code) = check(
        &[
            ("m.kel", "from \"./lib\" import { x }\nlet y = x\n"),
            ("lib.kel", lib),
        ],
        false,
    );
    assert_eq!(
        (out.as_str(), code),
        (
            "⚠ lib.kel: match が非網羅的です。たとえば None が漏れています\ny : Int32\n",
            0
        )
    );
    let (out, code) = check(
        &[
            ("m.kel", "from \"./lib\" import hidden\n"),
            ("lib.kel", lib),
        ],
        false,
    );
    assert_eq!(
        (out.as_str(), code),
        (
            "⚠ lib.kel: match が非網羅的です。たとえば None が漏れています\n! m.kel:1:21: import エラー: ./lib の hidden は公開されていません\n",
            1
        )
    );
    let (out, code) = check(&[("m.kel", "from \"./nothere\" import x\n")], true);
    assert_eq!(
        (out.as_str(), code),
        (
            "! m.kel:1:1: import エラー: import 先が見つかりません: ./nothere(nothere.kel)\n",
            1
        )
    );
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

#[test]
fn プレリュードは下げた限度の下限で構文解析できる() {
    use keleut_front::types::check::{MIN_CHECK_NEST, PRELUDE};
    let mut files = keleut_front::Files::new();
    let id = files.add("<prelude>");
    assert!(keleut_front::parse(PRELUDE.as_bytes(), id, &files, MIN_CHECK_NEST).is_ok());
}

#[test]
fn 長い_import_の鎖の単位の鍵を小さいスタックで決める() {
    // f0 が f1 を、f1 が f2 を、というように検索パスで import する、3,000 段の連鎖を作る。
    let n = 3000;
    let mut map: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..n {
        let src = if i + 1 < n {
            format!(
                "from \"f{}\" import v{}\npub let v{i}: Int32 = v{} + 1\n",
                i + 1,
                i + 1,
                i + 1
            )
        } else {
            format!("pub let v{i}: Int32 = 0\n")
        };
        map.insert(format!("f{i}.kel"), src.into_bytes());
    }
    map.insert(
        "m.kel".to_string(),
        b"from \"f0\" import v0\nlet r = v0\n".to_vec(),
    );
    let opts = CheckOptions {
        no_prelude: true,
        prelude: None,
        files: vec!["m.kel".to_string()],
        import_path: vec![".".to_string()],
        max_nest: keleut_front::DEFAULT_MAX_NEST,
    };
    let limits = check_limits(keleut_front::DEFAULT_MAX_NEST, Some(4 << 20)).unwrap();
    let keys = on_check_stack(limits, move || {
        let mut fs = MemFs(map);
        let c = check_program(&opts, &mut fs).unwrap();
        assert_eq!(c.output.error, None);
        c.unit_keys(&mut fs, &opts).unwrap()
    })
    .unwrap();
    // キーは、依存される側の単位が先になる順に並ぶ。
    let want: Vec<(String, String)> = (0..n)
        .rev()
        .map(|i| (format!("@1/f{i}.kel"), format!("f{i}.kel")))
        .collect();
    assert_eq!(keys, want);
}

/// `KELEUT_FRONT_DIKTOR` が設定されていれば、コーパスのすべてのファイルで、`--type-check` の出力と終了コードを Diktor と比較する。
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
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let o = std::process::Command::new(&diktor)
            .arg("--type-check")
            .arg(&name)
            .current_dir(p.parent().unwrap())
            .output()
            .unwrap();
        let mut want = String::from_utf8_lossy(&o.stdout).to_string();
        want.push_str(&String::from_utf8_lossy(&o.stderr));
        let dir = p.parent().unwrap().to_path_buf();
        let limits = check_limits(keleut_front::DEFAULT_MAX_NEST, None).unwrap();
        let n2 = name.clone();
        let (got, code) =
            on_check_stack(limits, move || run(&mut DirFs(dir), vec![n2], false)).unwrap();
        if (want.as_str(), o.status.code()) != (got.as_str(), Some(code as i32)) {
            diffs.push(p.display().to_string());
        }
    }
    assert!(diffs.is_empty(), "Diktor と違う: {diffs:?}");
}

/// `KELEUT_FRONT_DIKTOR` が設定されていれば、KIR の単位のキーを、`diktor --emit-ir-text` が出力する単位の行(`(unit ...)`)と比較する。
#[test]
fn 単位の鍵が_diktor_と同じ() {
    let Some(diktor) = std::env::var_os("KELEUT_FRONT_DIKTOR") else {
        return;
    };
    let d = std::env::temp_dir().join(format!("keleut-front-keys-{}", std::process::id()));
    let files: &[(&str, &str)] = &[
        ("p/lib/leaf.kel", "pub let base: Int32 = 10\n"),
        (
            "p/lib/left.kel",
            "from \"./leaf\" import base\npub let l: Int32 = base + 1\n",
        ),
        (
            "p/lib/right.kel",
            "from \"../lib/leaf\" import base\nfrom \"std/s\" import s\npub let r: Int32 = base + s\n",
        ),
        (
            "p/sp/std/s.kel",
            "from \"../t\" import t\npub let s: Int32 = t\n",
        ),
        ("p/sp/t.kel", "pub let t: Int32 = 2\n"),
        (
            "p/main.kel",
            "from \"./lib/left\" import l\nfrom \"./lib/right\" import r\nlet x = l + r\n",
        ),
        (
            "p/q/other.kel",
            "from \"../lib/leaf\" import base\nlet z = base\n",
        ),
    ];
    for (n, s) in files {
        let p = d.join(n);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("p", &["sp"], &["main.kel"]),
        ("", &["p/sp"], &["p/main.kel"]),
        ("p", &["./sp", "sp"], &["main.kel", "q/other.kel"]),
        ("p/q", &["../sp"], &["other.kel", "../main.kel"]),
    ];
    for (cwd, ipath, roots) in cases {
        let dir = d.join(cwd);
        let mut cmd = std::process::Command::new(&diktor);
        cmd.current_dir(&dir)
            .arg("--emit-ir-text")
            .arg("/dev/stdout");
        for i in *ipath {
            cmd.arg("--import-path").arg(i);
        }
        let o = cmd.args(*roots).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        let want: Vec<String> = text
            .lines()
            .filter(|l| l.starts_with("(unit \""))
            .map(|l| l.to_string())
            .collect();
        assert!(want.len() > 3, "{cwd} {roots:?}: {text}");
        let opts = CheckOptions {
            no_prelude: false,
            prelude: None,
            files: roots.iter().map(|s| s.to_string()).collect(),
            import_path: ipath.iter().map(|s| s.to_string()).collect(),
            max_nest: keleut_front::DEFAULT_MAX_NEST,
        };
        let limits = check_limits(keleut_front::DEFAULT_MAX_NEST, None).unwrap();
        let got = on_check_stack(limits, move || {
            let mut fs = DirFs(dir);
            let c = check_program(&opts, &mut fs).unwrap();
            assert!(c.output.error.is_none());
            c.unit_keys(&mut fs, &opts).unwrap()
        })
        .unwrap();
        // Diktor の単位の行は、builtin、prelude、依存する単位、main の順に並ぶ。
        // 先頭の 2 行と最後の 1 行を除いた部分が、`unit_keys` の返すキーに対応する。
        let got: Vec<String> = got
            .iter()
            .map(|(k, p)| format!("(unit \"{k}\" (paths \"{p}\"))"))
            .collect();
        assert_eq!(&want[2..want.len() - 1], &got[..], "{cwd} {roots:?}");
    }
    std::fs::remove_dir_all(&d).unwrap();
}
