//! kconform は、RustKeleut の差分試験の実行器である(計画 261024-1 の 8.1 節)。
//!
//! | 形 | 働き |
//! |---|---|
//! | `kconform [--pairs P,…] [--suite S] [--extra=FLAG…] [FILES…]` | 組ごとに走らせ、組の間で比べ、各組を `.t` に記録した期待とも比べる |
//! | `kconform expect FILES…` | 組の間で比べずに走らせ、`.t` に記録した期待とだけ比べる |
//! | `kconform kir-roundtrip` | コーパスの全件で、KIR の往復と 2 つのプロセスの書き出しの一致を確かめる |
//! | `kconform extract-corpus` | cram のヒアドキュメントから `.kel` を抜き出し、`conformance/corpus/` に置く |
//!
//! PATH の先頭に置く shim は、`kconform __shim diktor|keleut ARGS…` を呼ぶ(`shim` モジュール)。
//!
//! 終了状態は、差が無ければ 0、登録の無い差か失敗があれば 1、使い方の誤りなら 64、内部の誤りなら 70 である。

mod corpus;
mod cram;
mod known_diffs;
mod normalize;
mod pairs;
mod roundtrip;
mod runner;
mod shim;
mod util;

use pairs::{Engine, Pair};
use runner::{Case, CaseResult, Config};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
usage: kconform [OPTIONS] [--pairs P,...] [--suite all|depth] [--extra=FLAG...] [FILES...]
       kconform expect [OPTIONS] [--promote] FILES...
       kconform kir-roundtrip [OPTIONS] [--corpus DIR] [--diktor-only]
       kconform extract-corpus [--from DIR] [--corpus DIR] [--check]

options:
  --pairs P,...       the pairs to run (default: all available pairs: dk, dk-ir)
  --root DIR          the rustkeleut directory (default: the one kconform was built from)
  --diktor PATH       the real diktor (default: $KELEUT_DIKTOR, else ROOT/../diktor/_build/install/default/bin/diktor)
  --keleut PATH       the real keleut (default: $KCONFORM_KELEUT, else keleut next to kconform)
  --known-diffs FILE  the list of known differences (default: ROOT/conformance/known-diffs.toml)
  --jobs N            number of parallel jobs (default: number of CPUs)
  --timeout SECS      time limit of one .t file (default: 600)
  --check-shim        fail if a command mentions diktor but never reaches the shim
  --keep              keep the temporary directories
  --verbose           print more details
  --promote           (expect) rewrite each .t whose results differ with the actual results
  --list-invocations  print FILE:LINE [PAIR] and the shim calls of each command that reached the shim
";

/// 共通の選択肢である。
#[derive(Debug, Clone)]
struct Options {
    root: PathBuf,
    diktor: Option<PathBuf>,
    keleut: Option<PathBuf>,
    known_diffs: Option<PathBuf>,
    jobs: usize,
    timeout: Duration,
    check_shim: bool,
    keep: bool,
    verbose: bool,
    pairs: Option<Vec<String>>,
    suite: Option<String>,
    extra: Vec<String>,
    corpus: Option<PathBuf>,
    from: Option<PathBuf>,
    check: bool,
    diktor_only: bool,
    promote: bool,
    list_invocations: bool,
    files: Vec<PathBuf>,
}

/// 使い方の誤りである。
struct Usage(String);

fn default_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parse_options(args: &[String]) -> Result<Options, Usage> {
    let mut o = Options {
        root: default_root(),
        diktor: None,
        keleut: None,
        known_diffs: None,
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
        timeout: Duration::from_secs(600),
        check_shim: false,
        keep: false,
        verbose: false,
        pairs: None,
        suite: None,
        extra: Vec::new(),
        corpus: None,
        from: None,
        check: false,
        diktor_only: false,
        promote: false,
        list_invocations: false,
        files: Vec::new(),
    };
    let mut i = 0;
    let value = |i: &mut usize, name: &str| -> Result<String, Usage> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| Usage(format!("{name} requires an argument")))
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--root" => o.root = PathBuf::from(value(&mut i, a)?),
            "--diktor" => o.diktor = Some(PathBuf::from(value(&mut i, a)?)),
            "--keleut" => o.keleut = Some(PathBuf::from(value(&mut i, a)?)),
            "--known-diffs" => o.known_diffs = Some(PathBuf::from(value(&mut i, a)?)),
            "--jobs" => {
                o.jobs = value(&mut i, a)?
                    .parse()
                    .map_err(|_| Usage("--jobs requires a positive integer".into()))?
            }
            "--timeout" => {
                o.timeout = Duration::from_secs(
                    value(&mut i, a)?
                        .parse()
                        .map_err(|_| Usage("--timeout requires seconds".into()))?,
                )
            }
            "--check-shim" => o.check_shim = true,
            "--keep" => o.keep = true,
            "--verbose" => o.verbose = true,
            "--pairs" => {
                o.pairs = Some(
                    value(&mut i, a)?
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect(),
                )
            }
            "--suite" => o.suite = Some(value(&mut i, a)?),
            "--extra" => o.extra.push(value(&mut i, a)?),
            "--corpus" => o.corpus = Some(PathBuf::from(value(&mut i, a)?)),
            "--from" => o.from = Some(PathBuf::from(value(&mut i, a)?)),
            "--check" => o.check = true,
            "--diktor-only" => o.diktor_only = true,
            "--promote" => o.promote = true,
            "--list-invocations" => o.list_invocations = true,
            _ if a.starts_with("--extra=") => o.extra.push(a["--extra=".len()..].to_string()),
            _ if a.starts_with("--") => return Err(Usage(format!("unknown option: {a}"))),
            _ => o.files.push(PathBuf::from(a)),
        }
        i += 1;
    }
    if o.jobs == 0 {
        return Err(Usage("--jobs requires a positive integer".into()));
    }
    Ok(o)
}

impl Options {
    fn diktor_path(&self) -> PathBuf {
        self.diktor
            .clone()
            .or_else(|| {
                std::env::var_os("KELEUT_DIKTOR")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| {
                self.root
                    .join("../diktor/_build/install/default/bin/diktor")
            })
    }

    fn keleut_path(&self) -> PathBuf {
        self.keleut
            .clone()
            .or_else(|| {
                std::env::var_os("KCONFORM_KELEUT")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("keleut")))
                    .unwrap_or_else(|| PathBuf::from("keleut"))
            })
    }

    fn conformance(&self) -> PathBuf {
        self.root.join("conformance")
    }

    fn corpus_dir(&self) -> PathBuf {
        self.corpus
            .clone()
            .unwrap_or_else(|| self.conformance().join("corpus"))
    }

    fn known_diffs_path(&self) -> PathBuf {
        self.known_diffs
            .clone()
            .unwrap_or_else(|| self.conformance().join("known-diffs.toml"))
    }
}

/// 絶対パスにする(shim が別のディレクトリから起動するため)。
fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    })
}

/// shim のディレクトリを作る(`diktor` と `keleut` のスクリプトを置く)。
fn make_shim_dir(keep: bool) -> std::io::Result<util::TempDir> {
    use std::os::unix::fs::PermissionsExt;
    let dir = util::TempDir::new("kconform-shim", keep)?;
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    for name in ["diktor", "keleut"] {
        let p = dir.path().join(name);
        std::fs::write(&p, shim::script(name, &exe.to_string_lossy()))?;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(dir)
}

fn t_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "t"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// `conformance/` の下の `.t` を集める(`corpus/` と、`skip` に挙げたディレクトリを除く)。
fn conformance_files(dir: &Path, skip: &[&str]) -> Vec<PathBuf> {
    let mut v = t_files_in(dir);
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    subs.sort();
    for s in subs {
        let name = s.file_name().unwrap().to_string_lossy().into_owned();
        if name == "corpus" || skip.contains(&name.as_str()) {
            continue;
        }
        v.extend(conformance_files(&s, &[]));
    }
    v
}

/// コーパスの 1 項目を、1 つのコマンド(`diktor 旗 FILE`)の試験にする。
fn corpus_case(corpus: &Path, e: &corpus::Entry) -> Case {
    let src = corpus.join(&e.path);
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let mut words = vec!["diktor".to_string()];
    words.extend(
        e.flags
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|f| util::sh_quote(f)),
    );
    words.push(util::sh_quote(&name));
    Case {
        name: format!("{} ({})", e.path, e.source),
        key: absolute(&src),
        items: vec![cram::Item::Command(cram::Command {
            line: 1,
            text: vec![words.join(" ").into_bytes()],
            expected: Vec::new(),
        })],
        fixtures: vec![(src, PathBuf::from(name))],
        has_expectations: false,
    }
}

/// `.kel` のファイルを、1 つのコマンドの試験にする。
fn kel_case(p: &Path) -> Case {
    let entry = corpus::Entry {
        path: p.file_name().unwrap().to_string_lossy().into_owned(),
        source: p.display().to_string(),
        flags: None,
    };
    corpus_case(p.parent().unwrap_or(Path::new(".")), &entry)
}

/// 1 件の差である。
struct Difference {
    case: usize,
    command: usize,
    /// 差のある組の実体の名前である。
    pair: String,
    /// 比べた相手(`記録` か、基準の組の名前)である。
    against: String,
    want: Vec<Vec<u8>>,
    got: Vec<Vec<u8>>,
}

fn show(lines: &[Vec<u8>], mark: char) {
    for l in lines {
        println!("{mark}{}", String::from_utf8_lossy(l));
    }
}

/// 走らせた結果を比べ、報告し、終了状態を返す。
fn report(
    o: &Options,
    cases: &[Case],
    run_pairs: &[Pair],
    results: &[Vec<CaseResult>],
    compare_pairs: bool,
    known: &[known_diffs::Entry],
) -> u8 {
    let mut diffs = Vec::new();
    let mut failures = Vec::new();
    let mut exact_mismatch = 0usize;
    let mut compared = 0usize;
    let mut commands_total = 0usize;
    let mut invocations = 0usize;
    let mut diktor_commands = 0usize;
    let mut bypass = Vec::new();
    for (ci, case) in cases.iter().enumerate() {
        let cmds = cram::commands(&case.items);
        commands_total += cmds.len();
        for (pi, pair) in run_pairs.iter().enumerate() {
            let r = &results[ci][pi];
            if let Some(e) = &r.error {
                failures.push(format!("{} [{}]: {e}", case.name, pair.label));
                continue;
            }
            if r.timed_out {
                failures.push(format!("{} [{}]: 時間切れ", case.name, pair.label));
            }
            for (k, c) in cmds.iter().enumerate() {
                let inv = &r.invocations[k];
                invocations += inv.len();
                if o.list_invocations && !inv.is_empty() {
                    let who: Vec<&str> = inv.iter().map(|v| v.who.as_str()).collect();
                    println!(
                        "呼び出し: {}:{} [{}]\t{}",
                        case.name,
                        c.line,
                        pair.label,
                        who.join(",")
                    );
                }
                let reached = !matches!(r.outcomes[k], cram::Outcome::Unreachable);
                if reached && runner::mentions(c, "diktor") {
                    diktor_commands += 1;
                    if !inv.iter().any(|v| v.who == "diktor") {
                        bypass.push(format!(
                            "{}:{} [{}]: {}",
                            case.name,
                            c.line,
                            pair.label,
                            c.display()
                        ));
                    }
                }
                if !case.has_expectations {
                    continue;
                }
                compared += 1;
                let got = cram::render(&r.outcomes[k]);
                if got != c.expected {
                    exact_mismatch += 1;
                }
                if normalize::lines(&got) != normalize::lines(&c.expected) {
                    diffs.push(Difference {
                        case: ci,
                        command: k,
                        pair: pair.label.clone(),
                        against: "記録".to_string(),
                        want: c.expected.clone(),
                        got,
                    });
                }
            }
        }
        if compare_pairs && run_pairs.len() > 1 {
            let base = &results[ci][0];
            for (pi, pair) in run_pairs.iter().enumerate().skip(1) {
                let r = &results[ci][pi];
                if r.error.is_some() || base.error.is_some() {
                    continue;
                }
                for k in 0..cmds.len() {
                    let want = cram::render(&base.outcomes[k]);
                    let got = cram::render(&r.outcomes[k]);
                    if normalize::lines(&want) != normalize::lines(&got) {
                        diffs.push(Difference {
                            case: ci,
                            command: k,
                            pair: pair.label.clone(),
                            against: format!("組 {} ", run_pairs[0].label),
                            want,
                            got,
                        });
                    }
                }
            }
        }
    }
    let mut used = vec![false; known.len()];
    let mut unknown = 0usize;
    for d in &diffs {
        let case = &cases[d.case];
        let c = cram::commands(&case.items)[d.command];
        let pair_name = run_pairs
            .iter()
            .find(|p| p.label == d.pair)
            .map_or("", |p| p.name);
        let hit = known
            .iter()
            .position(|e| e.matches(&case.key, c.line, pair_name));
        if let Some(h) = hit {
            used[h] = true;
            if !o.verbose {
                continue;
            }
            println!(
                "既知の差: {}:{} [{}]({} {})",
                case.name, c.line, d.pair, known[h].ledger, known[h].reason
            );
        } else {
            unknown += 1;
            println!(
                "差: {}:{} [{}]: {}との差",
                case.name, c.line, d.pair, d.against
            );
        }
        println!("  $ {}", c.display());
        show(&d.want, '-');
        show(&d.got, '+');
    }
    for f in &failures {
        println!("失敗: {f}");
    }
    if o.verbose || o.check_shim {
        for b in &bypass {
            println!("shim を通らない: {b}");
        }
    }
    let run_names: Vec<&str> = run_pairs.iter().map(|p| p.name).collect();
    let unused: Vec<&known_diffs::Entry> = known
        .iter()
        .zip(&used)
        .filter(|(e, u)| {
            !**u && e
                .pairs
                .iter()
                .any(|p| p == "*" || run_names.contains(&p.as_str()))
                && cases.iter().any(|c| c.key.ends_with(Path::new(&e.file)))
        })
        .map(|(e, _)| e)
        .collect();
    if o.verbose {
        for e in &unused {
            println!(
                "使われなかった既知の差: {}:{}({})",
                e.file, e.line, e.ledger
            );
        }
    }
    let labels: Vec<&str> = run_pairs.iter().map(|p| p.label.as_str()).collect();
    println!(
        "kconform: 組 {}、{} 個の試験、{} 個のコマンド(記録と比べた {compared} 個)、差 {unknown} 件、既知の差 {} 件、失敗 {} 件",
        labels.join(","),
        cases.len(),
        commands_total,
        diffs.len() - unknown,
        failures.len()
    );
    println!(
        "kconform: 正規化の前の記録との不一致 {exact_mismatch} 件、shim の呼び出し {invocations} 回、diktor を含むコマンド {diktor_commands} 個のうち shim を通らないもの {} 個、使われなかった既知の差 {} 件",
        bypass.len(),
        unused.len()
    );
    if unknown == 0 && failures.is_empty() && !(o.check_shim && !bypass.is_empty()) {
        0
    } else {
        1
    }
}

/// 走らせる試験を集める。
fn collect_cases(o: &Options, include_corpus: bool) -> Result<Vec<Case>, String> {
    let mut paths = Vec::new();
    let mut kels = Vec::new();
    let mut with_corpus = false;
    if o.files.is_empty() {
        match o.suite.as_deref().unwrap_or("all") {
            "all" => {
                paths.extend(t_files_in(&o.root.join("../diktor/test")));
                paths.extend(conformance_files(
                    &o.conformance(),
                    &["depth", "frontend-depth"],
                ));
                with_corpus = true;
            }
            "depth" => paths.extend(t_files_in(&o.conformance().join("depth"))),
            s => return Err(format!("unknown suite: {s}")),
        }
    } else {
        for f in &o.files {
            if f.is_dir() {
                paths.extend(t_files_in(f));
            } else if f.extension().is_some_and(|e| e == "kel") {
                kels.push(f.clone());
            } else {
                paths.push(f.clone());
            }
        }
    }
    let mut cases = Vec::new();
    for p in &paths {
        cases.push(Case::from_file(p).map_err(|e| format!("{}: {e}", p.display()))?);
    }
    for k in &kels {
        cases.push(kel_case(k));
    }
    if with_corpus && include_corpus {
        let corpus = o.corpus_dir();
        if corpus.join(corpus::INDEX).is_file() {
            for e in corpus::read_index(&corpus)? {
                cases.push(corpus_case(&corpus, &e));
            }
        }
    }
    Ok(cases)
}

fn run_suite(o: &Options, run_pairs: Vec<Pair>, compare_pairs: bool) -> u8 {
    let include_corpus = compare_pairs && run_pairs.len() > 1;
    let cases = match collect_cases(o, include_corpus) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kconform: {e}");
            return 64;
        }
    };
    let known = match known_diffs::load(&o.known_diffs_path()) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("kconform: {e}");
            return 64;
        }
    };
    // 一覧の項目が、走らせる試験のコマンドの行を指していることを確かめる(試験の行がずれたら気づく)。
    let mut stale = 0;
    for e in &known {
        for c in cases.iter().filter(|c| c.key.ends_with(Path::new(&e.file))) {
            if !cram::commands(&c.items)
                .iter()
                .any(|cmd| cmd.line == e.line)
            {
                eprintln!(
                    "kconform: 既知の差の {} 行の項目 {}:{} はコマンドの行ではありません",
                    e.source_line, e.file, e.line
                );
                stale += 1;
            }
        }
    }
    if stale > 0 {
        return 64;
    }
    let shim = match make_shim_dir(o.keep) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kconform: shim を作れません: {e}");
            return 70;
        }
    };
    let (path_tail, removed) = runner::filter_path(&std::env::var_os("PATH").unwrap_or_default());
    if o.verbose {
        for r in &removed {
            println!("PATH から除いた: {}", r.display());
        }
    }
    let real_diktor = absolute(&o.diktor_path());
    if !real_diktor.is_file() {
        eprintln!(
            "kconform: diktor が見つかりません: {}",
            real_diktor.display()
        );
        return 64;
    }
    let cfg = Config {
        shim_dir: shim.path().to_path_buf(),
        real_diktor,
        real_keleut: absolute(&o.keleut_path()),
        path_tail,
        timeout: o.timeout,
        keep: o.keep,
        extra: o.extra.clone(),
    };
    let jobs: Vec<(usize, usize)> = (0..cases.len())
        .flat_map(|c| (0..run_pairs.len()).map(move |p| (c, p)))
        .collect();
    let flat = util::par_map(&jobs, o.jobs, |&(c, p)| {
        runner::run(&cases[c], &run_pairs[p], &cfg)
    });
    let mut results: Vec<Vec<CaseResult>> = (0..cases.len()).map(|_| Vec::new()).collect();
    for (&(c, _), r) in jobs.iter().zip(flat) {
        results[c].push(r);
    }
    if o.promote && !compare_pairs {
        for (case, rs) in cases.iter().zip(&results) {
            let r = &rs[0];
            if r.error.is_some() || !case.has_expectations {
                continue;
            }
            let text = cram::corrected(&case.items, &r.outcomes);
            let changed = std::fs::read(&case.key).map(|t| t != text).unwrap_or(true);
            if changed {
                if let Err(e) = std::fs::write(&case.key, &text) {
                    eprintln!("kconform: {}: {e}", case.key.display());
                    return 70;
                }
                println!("書き直した: {}", case.name);
            }
        }
    }
    report(o, &cases, &run_pairs, &results, compare_pairs, &known)
}

fn main_default(o: &Options) -> u8 {
    let names: Vec<String> = o
        .pairs
        .clone()
        .unwrap_or_else(|| pairs::DEFAULT.iter().map(|s| s.to_string()).collect());
    let mut run_pairs = Vec::new();
    for n in &names {
        let Some(ps) = pairs::expand(n) else {
            eprintln!(
                "kconform: unknown pair: {n} (pairs: {})",
                pairs::NAMES.join(",")
            );
            return 64;
        };
        if !pairs::AVAILABLE.contains(&n.as_str()) {
            eprintln!(
                "kconform: 組 {n} はまだ使えません(使える組: {})",
                pairs::AVAILABLE.join(",")
            );
            return 64;
        }
        run_pairs.extend(ps);
    }
    if run_pairs.is_empty() {
        eprintln!("kconform: --pairs is empty");
        return 64;
    }
    run_suite(o, run_pairs, true)
}

fn main_expect(o: &Options) -> u8 {
    if o.files.is_empty() {
        eprint!("kconform: expect requires FILES\n{USAGE}");
        return 64;
    }
    let pair = Pair {
        name: "ki",
        label: "expect".to_string(),
        engine: Engine::Keleut,
        args: pairs::EXPECT_ARGS.iter().map(|s| s.to_string()).collect(),
    };
    run_suite(o, vec![pair], false)
}

fn main_extract(o: &Options) -> u8 {
    let from = o
        .from
        .clone()
        .unwrap_or_else(|| o.root.join("../diktor/test"));
    let corpus = o.corpus_dir();
    let ex = match corpus::extract(&from) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("kconform: {}: {e}", from.display());
            return 70;
        }
    };
    let want = corpus::desired(&ex);
    let unknown_flags = ex.files.iter().filter(|(e, _)| e.flags.is_none()).count();
    let summary = format!(
        "extract-corpus: {} 本の .kel(旗の分からないもの {unknown_flags} 本)、展開を含むので除いたもの {}、終わりの語の無いもの {}",
        ex.files.len(),
        ex.expanding,
        ex.unterminated
    );
    if o.check {
        match corpus::diff(&corpus, &want) {
            Ok(d) if d.is_empty() => {
                println!("{summary}。コーパスは最新である");
                0
            }
            Ok(d) => {
                for l in &d {
                    println!("{l}");
                }
                println!("{summary}。コーパスとの差 {} 件", d.len());
                1
            }
            Err(e) => {
                eprintln!("kconform: {e}");
                70
            }
        }
    } else {
        if let Err(e) =
            std::fs::create_dir_all(&corpus).and_then(|()| corpus::write(&corpus, &want))
        {
            eprintln!("kconform: {}: {e}", corpus.display());
            return 70;
        }
        println!("{summary}。{} に書いた", corpus.display());
        0
    }
}

fn main_roundtrip(o: &Options) -> u8 {
    let cfg = roundtrip::Config {
        corpus: o.corpus_dir(),
        diktor: absolute(&o.diktor_path()),
        keleut: absolute(&o.keleut_path()),
        jobs: o.jobs,
        timeout: Duration::from_secs(60),
        diktor_only: o.diktor_only,
        verbose: o.verbose,
    };
    roundtrip::main(&cfg)
}

fn main() -> ExitCode {
    // shim として呼ばれたときの引数は、UTF-8 でないことがある(試験が故意に渡す)。
    let mut argv = std::env::args_os();
    let _ = argv.next();
    let raw: Vec<std::ffi::OsString> = argv.collect();
    if raw.first().is_some_and(|a| a == "__shim") && raw.len() >= 2 {
        return ExitCode::from(if raw[1] == "diktor" {
            shim::diktor(&raw[2..])
        } else {
            shim::keleut(&raw[2..])
        });
    }
    let args: Vec<String> = match raw.into_iter().map(|a| a.into_string()).collect() {
        Ok(v) => v,
        Err(a) => {
            eprintln!("kconform: UTF-8 でない引数です: {}", a.to_string_lossy());
            return ExitCode::from(64);
        }
    };
    if matches!(
        args.first().map(String::as_str),
        Some("--help" | "-h" | "help")
    ) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let (sub, rest) = match args.first().map(String::as_str) {
        Some(s @ ("expect" | "kir-roundtrip" | "extract-corpus")) => (s.to_string(), &args[1..]),
        _ => (String::new(), &args[..]),
    };
    let o = match parse_options(rest) {
        Ok(o) => o,
        Err(Usage(m)) => {
            eprint!("kconform: {m}\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    ExitCode::from(match sub.as_str() {
        "expect" => main_expect(&o),
        "kir-roundtrip" => main_roundtrip(&o),
        "extract-corpus" => main_extract(&o),
        _ => main_default(&o),
    })
}
