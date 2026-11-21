//! `kconform kir-roundtrip` の実装である。
//!
//! コーパスの各 `.kel` について、次のことを確かめる。
//!
//! 1. `diktor --emit-ir` が終了コード 0 で終了する。
//!    0 以外の終了コードで終了するもの(型エラーになるプログラムなど)は対象外とし、終了コードごとに数える。
//!    タイムアウトしたものは、対象外にせず失敗として数える。
//! 2. `diktor --emit-ir` を 2 回実行して書き出したバイナリが、バイト単位で一致する。
//! 3. `diktor --emit-ir-text` が終了コード 0 で終了する。
//! 4. `keleut kir verify` が、バイナリでもテキストでも終了コード 0 で終了する。
//! 5. `keleut kir dis` でバイナリから出力したテキストが、Diktor のテキストと一致する。
//! 6. `keleut kir asm` でテキストから作ったバイナリが、Diktor のバイナリと一致する。
//! 7. 6 のバイナリから `keleut kir dis` で出力したテキストが、Diktor のテキストと一致する。
//!    テキストをバイナリにしてテキストに戻すと元に戻ること(ラウンドトリップ)の確認である。
//! 8. 同じオプションの `keleut emit-kir` と `keleut emit-kir --text` が終了コード 0 で終了し、書き出したバイナリとテキストが Diktor のものと一致する。
//!
//! 続いて、`conformance/kir-fixtures/` にある各バージョンのフィクスチャ(IRSpec 14 節)について、次のことを確かめる。
//!
//! 9. `.kir` のフィクスチャを `diktor --run-ir` と `keleut run --frontend diktor --backend interp --tier interp-only` で実行する。
//!    `diktor --run-ir` がプログラムの結果を表す終了コード(0 か 3)で終了し、`keleut run` の終了コード、標準出力、標準エラーがそれと一致することを確かめる。
//!    3 は、実行時エラーで終了するフィクスチャの終了コードである。
//!    読み込みと検証の失敗は終了コード 65 になるので、実行時エラーと区別できる。
//!    標準エラーは、`<値>` のレコードのフィールドを並べ替えてから比較する。
//! 10. `.kir` と `.kirt` のどのフィクスチャでも、`keleut kir verify` が終了コード 0 で終了する。
//!
//! Diktor と `keleut emit-kir` に渡すオプションは、コーパスの一覧(`index.tsv`)のものを使う。
//! 一覧のオプションが `?` の項目は、オプションなしで書き出し、0 以外の終了コードで終了すれば `--no-prelude` を付けてやり直す。
//! タイムアウトしたときはやり直さない。
//! `--diktor-only` では `keleut` を実行しない。
//! 4〜8 と 10 を行わず、9 は `.kir` のフィクスチャで `diktor --run-ir` が 0 か 3 で終了することだけを確かめる。

use crate::corpus::{self, Entry};
use crate::util::{TempDir, par_map, wait_timeout};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// `kir-roundtrip` の実行の設定。
#[derive(Debug, Clone)]
pub struct Config {
    /// コーパスのディレクトリ。一覧(`index.tsv`)と `.kel` がある。
    pub corpus: PathBuf,
    /// フィクスチャのディレクトリ(`conformance/kir-fixtures`)。
    /// ディレクトリがないか、`.kir` も `.kirt` もなければ、9 と 10 で確かめるものはない。
    pub fixtures: PathBuf,
    /// 実行する diktor のパス。
    pub diktor: PathBuf,
    /// 実行する keleut のパス。
    pub keleut: PathBuf,
    /// 並列に確かめる項目の数。
    pub jobs: usize,
    /// コマンド 1 回の実行のタイムアウト。
    pub timeout: Duration,
    /// 真なら `keleut` を実行せず、Diktor だけを確かめる(`--diktor-only`)。
    pub diktor_only: bool,
    /// 真なら、対象外にした項目も 1 件ずつ表示する。
    pub verbose: bool,
}

/// 1 項目を確かめた結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// すべての確認に通った。
    Pass,
    /// `diktor --emit-ir` が 0 で終了しなかったので対象外にした。値はその終了コードである。
    Skipped(i32),
    /// 確認に失敗した。値は、失敗した確認の名前と説明である。
    Fail(&'static str, String),
}

struct Run {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(
    prog: &Path,
    arg0: &str,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> std::io::Result<Run> {
    let out = TempDir::new("kconform-rt-out", false)?;
    let so = out.path().join("o");
    let se = out.path().join("e");
    let mut child = Command::new(prog)
        .arg0(arg0)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&so)?)
        .stderr(std::fs::File::create(&se)?)
        .process_group(0)
        .spawn()?;
    let st = wait_timeout(&mut child, timeout)?;
    Ok(Run {
        code: st.map(|s| s.code().unwrap_or(128)),
        stdout: std::fs::read(&so)?,
        stderr: std::fs::read(&se)?,
    })
}

fn describe(r: &Run) -> String {
    let msg = String::from_utf8_lossy(&r.stderr);
    let first = msg.lines().next().unwrap_or("");
    match r.code {
        None => "タイムアウト".to_string(),
        Some(c) => format!("終了コード {c}: {first}"),
    }
}

fn check_one(e: &Entry, cfg: &Config) -> Verdict {
    match check_inner(e, cfg) {
        Ok(v) => v,
        Err(err) => Verdict::Fail("io", err.to_string()),
    }
}

fn check_inner(e: &Entry, cfg: &Config) -> std::io::Result<Verdict> {
    let src = cfg.corpus.join(&e.path);
    let work = TempDir::new("kconform-rt", false)?;
    let dir = work.path();
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::copy(&src, dir.join(&name))?;
    let d = |args: &[&str]| run(&cfg.diktor, "diktor", args, dir, cfg.timeout);
    let candidates: Vec<Vec<String>> = match &e.flags {
        Some(f) => vec![f.clone()],
        None => vec![vec![], vec!["--no-prelude".to_string()]],
    };
    let mut chosen = None;
    let mut last_code = 0;
    for flags in &candidates {
        let mut args: Vec<&str> = flags.iter().map(String::as_str).collect();
        args.extend(["--emit-ir", "a.kir", name.as_str()]);
        let r = d(&args)?;
        match r.code {
            Some(0) => {
                chosen = Some(flags.clone());
                break;
            }
            Some(c) => last_code = c,
            None => return Ok(Verdict::Fail("emit", "タイムアウト".to_string())),
        }
    }
    let Some(flags) = chosen else {
        return Ok(Verdict::Skipped(last_code));
    };
    let base: Vec<&str> = flags.iter().map(String::as_str).collect();
    let with = |extra: &[&'static str]| -> Vec<&str> {
        let mut v = base.clone();
        v.extend_from_slice(extra);
        v.push(name.as_str());
        v
    };
    let r = d(&with(&["--emit-ir", "b.kir"]))?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("emit-2", describe(&r)));
    }
    let a = std::fs::read(dir.join("a.kir"))?;
    if a != std::fs::read(dir.join("b.kir"))? {
        return Ok(Verdict::Fail(
            "cmp",
            "2 つのプロセスの書き出しが一致しない".to_string(),
        ));
    }
    let r = d(&with(&["--emit-ir-text", "a.kirt"]))?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("emit-text", describe(&r)));
    }
    if cfg.diktor_only {
        return Ok(Verdict::Pass);
    }
    let text = std::fs::read(dir.join("a.kirt"))?;
    let k = |args: &[&str]| run(&cfg.keleut, "keleut", args, dir, cfg.timeout);
    for f in ["a.kir", "a.kirt"] {
        let r = k(&["kir", "verify", f])?;
        if r.code != Some(0) {
            return Ok(Verdict::Fail("verify", format!("{f}: {}", describe(&r))));
        }
    }
    let r = k(&["kir", "dis", "a.kir"])?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("dis", describe(&r)));
    }
    if r.stdout != text {
        return Ok(Verdict::Fail(
            "dis",
            "テキストが diktor --emit-ir-text と一致しない".to_string(),
        ));
    }
    let r = k(&["kir", "asm", "a.kirt", "-o", "c.kir"])?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("asm", describe(&r)));
    }
    if std::fs::read(dir.join("c.kir"))? != a {
        return Ok(Verdict::Fail(
            "asm",
            "バイナリが diktor --emit-ir と一致しない".to_string(),
        ));
    }
    let r = k(&["kir", "dis", "c.kir"])?;
    if r.code != Some(0) || r.stdout != text {
        return Ok(Verdict::Fail(
            "roundtrip",
            "テキスト → バイナリ → テキストが一致しない".to_string(),
        ));
    }
    let emit = |extra: &[&'static str]| -> Vec<&str> {
        let mut v = vec!["emit-kir"];
        v.extend_from_slice(extra);
        v.extend(base.iter().copied());
        v.push(name.as_str());
        v
    };
    let r = k(&emit(&["-o", "k.kir"]))?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("emit-kir", describe(&r)));
    }
    if std::fs::read(dir.join("k.kir"))? != a {
        return Ok(Verdict::Fail(
            "emit-kir",
            "keleut emit-kir のバイナリが diktor --emit-ir と一致しない".to_string(),
        ));
    }
    let r = k(&emit(&["--text", "-o", "k.kirt"]))?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("emit-kir-text", describe(&r)));
    }
    if std::fs::read(dir.join("k.kirt"))? != text {
        return Ok(Verdict::Fail(
            "emit-kir-text",
            "keleut emit-kir --text のテキストが diktor --emit-ir-text と一致しない".to_string(),
        ));
    }
    Ok(Verdict::Pass)
}

/// フィクスチャのディレクトリの下にある `.kir` と `.kirt` を、パスの順に集める。
fn fixture_files(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "kir" || x == "kirt") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

/// フィクスチャ 1 つを確かめる(モジュールの説明の 9 と 10)。
fn check_fixture(path: &Path, cfg: &Config) -> std::io::Result<Verdict> {
    let work = TempDir::new("kconform-fx", false)?;
    let dir = work.path();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::copy(path, dir.join(&name))?;
    let binary = name.ends_with(".kir");
    let d = if binary {
        let r = run(
            &cfg.diktor,
            "diktor",
            &["--run-ir", &name],
            dir,
            cfg.timeout,
        )?;
        if !matches!(r.code, Some(0 | 3)) {
            return Ok(Verdict::Fail("fixture-diktor", describe(&r)));
        }
        Some(r)
    } else {
        None
    };
    if cfg.diktor_only {
        return Ok(Verdict::Pass);
    }
    let k = |args: &[&str]| run(&cfg.keleut, "keleut", args, dir, cfg.timeout);
    let r = k(&["kir", "verify", &name])?;
    if r.code != Some(0) {
        return Ok(Verdict::Fail("fixture-verify", describe(&r)));
    }
    if let Some(d) = d {
        let r = k(&[
            "run",
            "--frontend",
            "diktor",
            "--backend",
            "interp",
            "--tier",
            "interp-only",
            &name,
        ])?;
        if r.code != d.code {
            return Ok(Verdict::Fail("fixture-run", describe(&r)));
        }
        // IRSpec 付録 B は、`<値>` のレコードの、ラベルの異なるフィールドどうしの順序をリーダに任せている。
        // そこで、標準エラーはフィールドを並べ替えてから比較する。
        if r.stdout != d.stdout || !kconform::fieldsort::same_output(&r.stderr, &d.stderr) {
            return Ok(Verdict::Fail(
                "fixture-run",
                "keleut run の出力が diktor --run-ir と一致しない".to_string(),
            ));
        }
    }
    Ok(Verdict::Pass)
}

/// すべてのフィクスチャを確かめ、(フィクスチャの数、失敗の数)を返す。
fn check_fixtures(cfg: &Config) -> (usize, usize) {
    let files = fixture_files(&cfg.fixtures);
    let verdicts = par_map(&files, cfg.jobs, |p| {
        check_fixture(p, cfg).unwrap_or_else(|e| Verdict::Fail("io", e.to_string()))
    });
    let mut fails = 0;
    for (p, v) in files.iter().zip(&verdicts) {
        if let Verdict::Fail(step, msg) = v {
            fails += 1;
            println!("失敗 {} [{step}]: {msg}", p.display());
        }
    }
    (files.len(), fails)
}

/// コーパスの全件とすべてのフィクスチャを確かめ、終了コードを返す。
///
/// すべて通れば 0、失敗が 1 件でもあれば 1、コーパスの一覧を読めなければ 64 を返す。
pub fn main(cfg: &Config) -> u8 {
    let entries = match corpus::read_index(&cfg.corpus) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("kconform: {e}");
            return 64;
        }
    };
    let verdicts = par_map(&entries, cfg.jobs, |e| check_one(e, cfg));
    let mut pass = 0;
    let mut skipped = std::collections::BTreeMap::new();
    let mut fails = 0;
    for (e, v) in entries.iter().zip(&verdicts) {
        match v {
            Verdict::Pass => pass += 1,
            Verdict::Skipped(c) => {
                *skipped.entry(*c).or_insert(0usize) += 1;
                if cfg.verbose {
                    println!("対象外 {} ({}): diktor --emit-ir が {c}", e.path, e.source);
                }
            }
            Verdict::Fail(step, msg) => {
                fails += 1;
                println!("失敗 {} ({}) [{step}]: {msg}", e.path, e.source);
            }
        }
    }
    let skipped_s: Vec<String> = skipped
        .iter()
        .map(|(c, n)| format!("{c} が {n} 件"))
        .collect();
    println!(
        "kir-roundtrip: {} 件のうち、通った {pass} 件、失敗 {fails} 件、対象外 {} 件({}){}",
        entries.len(),
        entries.len() - pass - fails,
        if skipped_s.is_empty() {
            "なし".to_string()
        } else {
            skipped_s.join("、")
        },
        if cfg.diktor_only {
            "。--diktor-only"
        } else {
            ""
        }
    );
    let (nfx, fx_fails) = check_fixtures(cfg);
    println!(
        "kir-roundtrip: フィクスチャ {nfx} 個({})のうち、通った {} 個、失敗 {fx_fails} 個",
        cfg.fixtures.display(),
        nfx - fx_fails
    );
    if fails == 0 && fx_fails == 0 { 0 } else { 1 }
}
