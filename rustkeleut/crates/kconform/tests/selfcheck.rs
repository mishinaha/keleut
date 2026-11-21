//! kconform の自己検査である。
//!
//! 本物の diktor の代わりに小さな `sh` のスクリプトを置き、cram の読み取り、shim を通る呼び出し、期待出力との比較を確かめる。

use std::path::{Path, PathBuf};
use std::process::Command;

/// テストごとに作る一時ディレクトリである。
/// ドロップすると内容ごと削除する。
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p =
            std::env::temp_dir().join(format!("kconform-selfcheck-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 偽の diktor である。
/// 引数を標準出力に書き、引数に `fail.kel` があれば型エラーの行を標準エラーに書いて終了コード 3 で終了する。
/// 標準出力が閉じていれば 74 で終了する。
const FAKE: &str = "#!/bin/sh\n\
printf 'fake %s\\n' \"$*\" 2>/dev/null || exit 74\n\
case \"$*\" in *fail.kel*) echo 'x.kel:1:1: 型エラー: 偽の誤り' >&2; exit 3 ;; esac\n";

fn setup(d: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let fake = d.join("bin/diktor");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(&fake, FAKE).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    fake
}

fn kconform(d: &Path, fake: &Path, args: &[&str]) -> (i32, String) {
    // サブコマンドは、ここで追加するオプション(`--root` など)より前に置く。
    let (sub, rest) = match args.first() {
        Some(&s) if !s.starts_with('-') && !s.contains('/') => (vec![s], &args[1..]),
        _ => (vec![], args),
    };
    let out = Command::new(env!("CARGO_BIN_EXE_kconform"))
        .args(sub)
        .arg("--root")
        .arg(d)
        .arg("--diktor")
        .arg(fake)
        .arg("--known-diffs")
        .arg(d.join("known-diffs.toml"))
        .args(rest)
        .env("TMPDIR", d)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

const GOOD: &str = "注釈の行。

  $ diktor a.kel
  fake a.kel
  $ diktor fail.kel
  fake fail.kel
  x.kel:1:1: 型エラー: 偽の誤り
  [3]
  $ x=1
  $ echo $x; pwd
  1
  $TESTCASE_ROOT
  $ cat > b.kel <<'EOF'
  > diktor
  > EOF
  $ diktor b.kel >&-; echo \"exit: $?\"
  exit: 74
  $ printf 'a\\n' | diktor --repl | cat
  fake --repl
  $ printf 'x\\n$\\ny\\n'
  x
  $
  y
  $ exit 5
  ***** UNREACHABLE *****
  $ echo never
  ***** UNREACHABLE *****
";

#[test]
fn 記録どおりなら差は無く_shim_を通る() {
    let d = Dir::new("good");
    let fake = setup(&d.0);
    std::fs::write(d.0.join("good.t"), GOOD).unwrap();
    let t = d.0.join("good.t");
    let (code, out) = kconform(
        &d.0,
        &fake,
        &["--pairs", "dk", "--check-shim", t.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("差分 0 件"), "{out}");
    assert!(out.contains("shim の呼び出し 4 回"), "{out}");
    assert!(
        out.contains("diktor を含むコマンド 4 個のうち shim を通らないもの 0 個"),
        "{out}"
    );
}

/// dune 3.21.1 の cram が promote した内容である。
/// dune の cram は、改行の直前の CR を 1 つだけ取り除き、行の途中の CR と、出力の最後にあって改行が続かない CR は残す。
const CR: &str = "  $ printf \"a\\rb\\n\"\n  a\rb\n  $ printf \"x\\r\\r\\n\"\n  x\r\n  $ printf \"y\\r\"\n  y\r\n  $ printf \"\\r\\n\\r\\n\"\n  \n  \n";

#[test]
fn 出力の_cr_を_dune_と同じに扱う() {
    let d = Dir::new("cr");
    let fake = setup(&d.0);
    let t = d.0.join("cr.t");
    std::fs::write(&t, CR).unwrap();
    let (code, out) = kconform(&d.0, &fake, &["expect", t.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("差分 0 件"), "{out}");
}

/// dune 3.21.1 の cram が promote した内容である。
/// dune の cram は、シェルのエラーメッセージの先頭に付くスクリプトの位置と、端末の色の指定を、出力から取り除く。
const SHELL_MESSAGES: &str = "シェルの誤りと色。

  $ nosuchcmd_xyz
  nosuchcmd_xyz: command not found
  [127]
  $ echo hi > /nonexistent/dir/f
  /nonexistent/dir/f: No such file or directory
  [1]
  $ printf '\\033[31mred\\033[0m \\033[1;32mbold\\033[0m\\n'
  red bold
  $ cd /nonexistent_dir_xyz
  cd: /nonexistent_dir_xyz: No such file or directory
  [1]
  $ echo ok
  ok
";

#[test]
fn シェルの誤りの位置と色の指定を_dune_と同じに除く() {
    let d = Dir::new("shellmsg");
    let fake = setup(&d.0);
    let t = d.0.join("shellmsg.t");
    std::fs::write(&t, SHELL_MESSAGES).unwrap();
    let (code, out) = kconform(&d.0, &fake, &["expect", t.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("差分 0 件"), "{out}");
}

#[test]
fn 診断の本文の違いは正規化で消え_終了状態の違いは差になる() {
    let d = Dir::new("diff");
    let fake = setup(&d.0);
    let t = d.0.join("diff.t");
    std::fs::write(
        &t,
        "  $ diktor fail.kel\n  fake fail.kel\n  x.kel:1:1: 型エラー: 別の本文\n  [3]\n  $ diktor a.kel\n  fake a.kel\n  [1]\n",
    )
    .unwrap();
    let (code, out) = kconform(&d.0, &fake, &["--pairs", "dk", t.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("差分 1 件"), "{out}");
    assert!(out.contains("正規化の前の期待出力との不一致 2 件"), "{out}");
    assert!(out.contains("diff.t:5 [dk]: 期待出力との差分"), "{out}");
}

#[test]
fn 既知の差に載せた差は失敗にしない() {
    let d = Dir::new("known");
    let fake = setup(&d.0);
    let t = d.0.join("k.t");
    std::fs::write(&t, "  $ diktor a.kel\n  wrong\n").unwrap();
    std::fs::write(
        d.0.join("known-diffs.toml"),
        "[[diff]]\nfile = \"k.t\"\nline = 1\npairs = [\"dk\"]\nreason = \"試験\"\nledger = \"V0\"\n",
    )
    .unwrap();
    let (code, out) = kconform(&d.0, &fake, &["--pairs", "dk", t.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("既知の差分 1 件"), "{out}");
    std::fs::write(
        d.0.join("known-diffs.toml"),
        "[[diff]]\nfile = \"k.t\"\nline = 2\npairs = [\"dk\"]\nreason = \"試験\"\nledger = \"V0\"\n",
    )
    .unwrap();
    let (code, _) = kconform(&d.0, &fake, &["--pairs", "dk", t.to_str().unwrap()]);
    assert_eq!(code, 64, "コマンドの行を指さない項目は使い方のエラー");
}

#[test]
fn ヒアドキュメントからコーパスを抜き出す() {
    let d = Dir::new("corpus");
    let fake = setup(&d.0);
    let tests = d.0.join("tests");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(tests.join("x.t"), GOOD).unwrap();
    let corpus = d.0.join("corpus");
    let args = [
        "--from",
        tests.to_str().unwrap(),
        "--corpus",
        corpus.to_str().unwrap(),
    ];
    let mut a = vec!["extract-corpus"];
    a.extend(args);
    let (code, out) = kconform(&d.0, &fake, &a);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        std::fs::read_to_string(corpus.join("x/0001-b.kel")).unwrap(),
        "diktor\n"
    );
    let index = std::fs::read_to_string(corpus.join("index.tsv")).unwrap();
    assert!(index.contains("x/0001-b.kel\tx.t:13\t\n"), "{index}");
    a.push("--check");
    let (code, out) = kconform(&d.0, &fake, &a);
    assert_eq!(code, 0, "{out}");
    std::fs::write(corpus.join("x/0001-b.kel"), "changed\n").unwrap();
    let (code, out) = kconform(&d.0, &fake, &a);
    assert_eq!(code, 1, "{out}");
}

#[test]
fn 深さの試験群は_dk_と_kf_を走らせず_試験が無ければ失敗にする() {
    let d = Dir::new("depth");
    let fake = setup(&d.0);
    for p in ["dk", "kf", "dk-ir,dk"] {
        let (code, _) = kconform(&d.0, &fake, &["--pairs", p, "--suite", "depth"]);
        assert_eq!(code, 64, "{p}");
    }
    // conformance/depth/ がなければ、実行するテストがないので失敗にする。
    let (code, out) = kconform(&d.0, &fake, &["--pairs", "dk-ir", "--suite", "depth"]);
    assert_eq!(code, 1, "{out}");
}

/// 偽の keleut を `d` の下に置き、そのパスを返す。
/// 偽の keleut は、`--version` には `version` の内容を出力し、ほかの引数では `fake` に続けてその引数を出力する。
fn fake_keleut(d: &Path, name: &str, version: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let k = d.join(format!("kbin-{name}/keleut"));
    std::fs::create_dir_all(k.parent().unwrap()).unwrap();
    std::fs::write(
        &k,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '{version}'; exit 0; fi\nprintf 'fake %s\\n' \"$*\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&k, std::fs::Permissions::from_mode(0o755)).unwrap();
    k
}

/// 偽の keleut を使って kconform を実行し、終了コード、標準出力、標準エラーを返す。
fn kconform_with_keleut(
    d: &Path,
    fake: &Path,
    keleut: &Path,
    args: &[&str],
) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_kconform"))
        .arg("--root")
        .arg(d)
        .arg("--diktor")
        .arg(fake)
        .arg("--keleut")
        .arg(keleut)
        .arg("--known-diffs")
        .arg(d.join("known-diffs.toml"))
        .args(args)
        .env("TMPDIR", d)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// LLVM ペアの名前をコンマで区切って並べたものである。
/// kconform が LLVM ペアを除いたときに標準エラーに書く並びと照合する。
const JIT_PAIRS: &str = "kj,kt,ka,km,kr-kj,kr-kt,kr-ka,kr-km";

#[test]
fn feature_llvm_を外した_keleut_では_llvm_の後段の組を除く() {
    let d = Dir::new("nojit");
    let fake = setup(&d.0);
    let t = d.0.join("echo.t");
    std::fs::write(&t, "  $ echo hi\n  hi\n").unwrap();
    let t = t.to_str().unwrap();
    let nojit = fake_keleut(&d.0, "nojit", "keleut 0.1.0\\nbackends: interp wasm\\n");

    // --pairs に LLVM ペアを書けば、使い方のエラーとして終了コード 64 で終了する。
    let (code, out, err) = kconform_with_keleut(&d.0, &fake, &nojit, &["--pairs", "ki,kj", t]);
    assert_eq!(code, 64, "{out}{err}");
    assert!(err.contains("kj"), "{err}");
    assert!(
        err.contains("feature llvm を外してビルドされています"),
        "{err}"
    );

    // --pairs を省けば、LLVM ペアを除いたことを標準エラーに書いて、残りのペアを実行する。
    let (_, out, err) = kconform_with_keleut(&d.0, &fake, &nojit, &[t]);
    assert!(
        err.contains(&format!("LLVM ペア {JIT_PAIRS} を実行しません")),
        "{err}"
    );
    let ran = ran_pairs(&out);
    assert_eq!(
        ran,
        ["dk", "dk-ir", "kf", "ki", "kw", "x-ir", "kr-ki", "kr-kw"],
        "{out}"
    );

    // `--version` の `backends:` の行に jit がある keleut と、その行を出力しない keleut では、ペアを除かない。
    for (name, version) in [
        ("jit", "keleut 0.1.0\\nbackends: interp jit wasm\\n"),
        ("noline", "keleut 0.1.0\\n"),
    ] {
        let k = fake_keleut(&d.0, name, version);
        let (_, out, err) = kconform_with_keleut(&d.0, &fake, &k, &[t]);
        assert!(!err.contains("を実行しません"), "{name}: {err}");
        assert!(!err.contains("feature llvm"), "{name}: {out}{err}");
        let ran = ran_pairs(&out);
        for p in [
            "kj", "kt", "ka=1", "ka=8", "km=4", "kr-kj", "kr-ka=8", "kr-km=1",
        ] {
            assert!(ran.contains(&p.to_string()), "{name}, {p}: {out}");
        }
    }
}

/// 要約の行(`kconform: ペア ` で始まる行)から、実行したペアの名前を取り出す。
/// シードのあるペアは、`ka=1` のようにシードごとの名前になる。
fn ran_pairs(out: &str) -> Vec<String> {
    let line = out
        .lines()
        .find(|l| l.starts_with("kconform: ペア "))
        .unwrap_or_else(|| panic!("要約の行がない: {out}"));
    let rest = &line["kconform: ペア ".len()..];
    let list = &rest[..rest.find('(').unwrap()];
    list.split(',').map(str::to_string).collect()
}
