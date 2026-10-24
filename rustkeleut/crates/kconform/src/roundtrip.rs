//! `kconform kir-roundtrip` である(計画 261024-1 の 8.1 節と 8.3 節)。
//!
//! コーパスの各 `.kel` について、次のことを確かめる。
//!
//! 1. `diktor --emit-ir` が 0 で終わる。
//!    0 で終わらないもの(型検査の誤りなど)は対象の外とし、終了状態ごとに数える。
//! 2. 2 つのプロセスで書き出したバイナリが、バイト単位で一致する。
//! 3. `diktor --emit-ir-text` が 0 で終わる。
//! 4. `keleut kir verify` が、バイナリにもテキストにも 0 で終わる。
//! 5. `keleut kir dis` でバイナリから印字したテキストが、Diktor のテキストと一致する。
//! 6. `keleut kir asm` でテキストから組んだバイナリが、Diktor のバイナリと一致する。
//! 7. 6 のバイナリを `keleut kir dis` で印字したテキストが、Diktor のテキストと一致する(テキスト → バイナリ → テキストの往復)。
//! 8. 同じ旗の `keleut emit-kir` と `keleut emit-kir --text` が 0 で終わり、書いたバイナリとテキストが Diktor のものと一致する。
//!
//! 旗は索引のものを使う。
//! 索引の旗が `?` のときは、旗なしで書き出し、型検査の誤り(1)なら `--no-prelude` で試し直す。
//! `--diktor-only` では 4〜8 を行わない(RustKeleut の側がまだ無いときの確認に使う)。

use crate::corpus::{self, Entry};
use crate::util::{TempDir, par_map, wait_timeout};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// 設定である。
#[derive(Debug, Clone)]
pub struct Config {
    pub corpus: PathBuf,
    pub diktor: PathBuf,
    pub keleut: PathBuf,
    pub jobs: usize,
    pub timeout: Duration,
    pub diktor_only: bool,
    pub verbose: bool,
}

/// 1 項目の結果である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// すべての確認に通った。
    Pass,
    /// `diktor --emit-ir` が 0 で終わらなかったので対象の外にした(その終了状態)。
    Skipped(i32),
    /// 確認に落ちた(段の名前と説明)。
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
        None => "時間切れ".to_string(),
        Some(c) => format!("終了状態 {c}: {first}"),
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
            None => return Ok(Verdict::Fail("emit", "時間切れ".to_string())),
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

/// コーパスの全件を確かめ、終了状態を返す(0 は全件が通ったこと)。
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
    if fails == 0 { 0 } else { 1 }
}
