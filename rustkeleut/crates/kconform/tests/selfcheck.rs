//! kconform の自己検査である。
//!
//! 本物の diktor の代わりに小さな `sh` のスクリプトを置き、cram の読み方、shim の通り方、記録との比べ方を確かめる。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 試験ごとの一時のディレクトリである。
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
/// 引数を印字し、`fail.kel` なら 3 で終わる。
/// 標準出力が閉じていれば 74 で終わる。
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
    // サブコマンドは先頭に置く。
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
    assert!(out.contains("差 0 件"), "{out}");
    assert!(out.contains("shim の呼び出し 4 回"), "{out}");
    assert!(
        out.contains("diktor を含むコマンド 4 個のうち shim を通らないもの 0 個"),
        "{out}"
    );
}

/// dune 3.21.1 の cram が promote した中身である。
/// 改行の直前の CR は 1 つだけ落ち、行の途中の CR と最後の改行の無い CR は残る。
const CR: &str = "  $ printf \"a\\rb\\n\"\n  a\rb\n  $ printf \"x\\r\\r\\n\"\n  x\r\n  $ printf \"y\\r\"\n  y\r\n  $ printf \"\\r\\n\\r\\n\"\n  \n  \n";

#[test]
fn 出力の_cr_を_dune_と同じに扱う() {
    let d = Dir::new("cr");
    let fake = setup(&d.0);
    let t = d.0.join("cr.t");
    std::fs::write(&t, CR).unwrap();
    let (code, out) = kconform(&d.0, &fake, &["expect", t.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("差 0 件"), "{out}");
}

/// dune 3.21.1 の cram が promote した中身である。
/// シェルの誤りの文面の頭のスクリプトの位置と、端末の色の指定は、出力から除かれる。
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
    assert!(out.contains("差 0 件"), "{out}");
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
    assert!(out.contains("差 1 件"), "{out}");
    assert!(out.contains("正規化の前の記録との不一致 2 件"), "{out}");
    assert!(out.contains("diff.t:5 [dk]: 記録との差"), "{out}");
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
    assert!(out.contains("既知の差 1 件"), "{out}");
    std::fs::write(
        d.0.join("known-diffs.toml"),
        "[[diff]]\nfile = \"k.t\"\nline = 2\npairs = [\"dk\"]\nreason = \"試験\"\nledger = \"V0\"\n",
    )
    .unwrap();
    let (code, _) = kconform(&d.0, &fake, &["--pairs", "dk", t.to_str().unwrap()]);
    assert_eq!(code, 64, "コマンドの行を指さない項目は使い方の誤り");
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
