//! kconform は、RustKeleut の差分テストを実行するツールである。
//!
//! | 書式 | 動作 |
//! |---|---|
//! | `kconform [--pairs P,…] [--suite S] [--extra=FLAG…] [FILES…]` | テストをペアごとに実行し、ペアの間で結果を比較する。各ペアの結果は `.t` に書いた期待出力とも比較する |
//! | `kconform expect FILES…` | ペアの間の比較をせずに実行し、`.t` に書いた期待出力とだけ比較する |
//! | `kconform kir-roundtrip` | コーパスの全件で、KIR のテキストとバイナリを相互に変換して元に戻ること(ラウンドトリップ)と、2 つのプロセスが書き出した KIR が一致することを確かめる。`conformance/kir-fixtures/` のフィクスチャを Diktor と RustKeleut の両方のリーダで読むことも確かめる |
//! | `kconform extract-corpus` | cram のヒアドキュメントから `.kel` を抜き出し、`conformance/corpus/` に置く |
//!
//! PATH の先頭に置く shim は、`kconform __shim diktor|keleut ARGS…` を呼ぶ(`shim` モジュール)。
//!
//! 終了コードは、差分がなければ 0、未登録の差分か失敗があれば 1、使い方のエラーなら 64、内部エラーなら 70 である。
//! kconform は、出力ではペアを「組」、バックエンドを「後段」、テストを「試験」、既知の差分を「既知の差」、期待出力を主に「記録」と書く(「期待の出力」と書く行もある)。

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
  --pairs P,...       the pairs to run (default: all pairs; the LLVM pairs kj, kt, ka, km and
                      kr-kj, kr-kt, kr-ka, kr-km are left out, with a note, when keleut was built
                      without the llvm feature, and naming them is then an error;
                      --suite depth leaves out dk and kf and rejects them)
  --root DIR          the rustkeleut directory (default: the one kconform was built from)
  --diktor PATH       the real diktor (default: $KELEUT_DIKTOR, else ROOT/../diktor/_build/install/default/bin/diktor)
  --keleut PATH       the real keleut (default: $KCONFORM_KELEUT, else keleut next to kconform)
  --known-diffs FILE  the list of known differences (default: ROOT/conformance/known-diffs.toml and known-diffs-*.toml)
  --jobs N            number of parallel jobs (default: number of CPUs)
  --timeout SECS      time limit of one .t file (default: 600)
  --check-shim        fail if a command mentions diktor but never reaches the shim
  --keep              keep the temporary directories
  --verbose           print more details
  --promote           (expect) rewrite each .t whose results differ with the actual results
  --list-invocations  print FILE:LINE [PAIR] and the shim calls of each command that reached the shim
";

/// コマンド行のオプションを読んだ結果。
///
/// どのサブコマンドも同じ構造体を使い、自分に関係するフィールドだけを読む。
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

/// 使い方のエラー。標準エラーに書くメッセージを持つ。
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

    /// 読み込む既知の差分の一覧のパスを返す。
    ///
    /// `--known-diffs` を指定しなければ、`conformance/known-diffs.toml` と、
    /// 同じディレクトリの `known-diffs-*.toml` を名前の順に読む。
    /// `known-diffs-*.toml` は、一時的な登録を分けて置くためのファイルである。
    fn known_diffs_paths(&self) -> Vec<PathBuf> {
        if let Some(p) = &self.known_diffs {
            return vec![p.clone()];
        }
        let dir = self.conformance();
        let mut extra: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.is_file()
                            && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                                n.starts_with("known-diffs-") && n.ends_with(".toml")
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        extra.sort();
        let mut v = vec![dir.join("known-diffs.toml")];
        v.extend(extra);
        v
    }
}

/// パスを絶対パスにする。
/// shim はテストの作業ディレクトリで起動されるので、相対パスのままでは使えない。
fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    })
}

/// shim のディレクトリを作り、`diktor` と `keleut` のスクリプトを置く。
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

/// `conformance/` の下の `.t` を集める。
/// `corpus/` と、`skip` に挙げた直下のディレクトリは除く。
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

/// コーパスの 1 項目を、コマンド 1 つ(`diktor [オプション] FILE`)のテストにする。
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

/// `.kel` のファイルを、コマンド 1 つのテストにする。
fn kel_case(p: &Path) -> Case {
    let entry = corpus::Entry {
        path: p.file_name().unwrap().to_string_lossy().into_owned(),
        source: p.display().to_string(),
        flags: None,
    };
    corpus_case(p.parent().unwrap_or(Path::new(".")), &entry)
}

/// 比較で見つけた差分 1 件。どのテストのどのコマンドで、どのペアの結果が何と違ったかを持つ。
struct Difference {
    /// テストの番号(`cases` の添字)。
    case: usize,
    /// テストの中のコマンドの番号。
    command: usize,
    /// 差分のあるペアの、個々の構成の名前(`ka=3` など)。
    pair: String,
    /// 比較対象の表示。`記録`(期待出力)か、基準のペアの名前を含む文字列である。
    against: String,
    /// 比較の種類。既知の差分の `against` との照合に使う。
    cmp: known_diffs::Comparison,
    want: Vec<Vec<u8>>,
    got: Vec<Vec<u8>>,
}

fn show(lines: &[Vec<u8>], mark: char) {
    for l in lines {
        println!("{mark}{}", String::from_utf8_lossy(l));
    }
}

/// ペアの間の比較で基準にするペアの、`run_pairs` の中の位置を返す。
///
/// `dk` があれば `dk` を基準にする。
/// `dk` がなければ `dk-ir`(`--suite depth` など)、それもなければ最初のペアを基準にする。
/// こうすると、`--pairs` に書いた順序によらず、既知の差分の同じ項目が該当する。
fn base_index(run_pairs: &[Pair]) -> usize {
    ["dk", "dk-ir"]
        .iter()
        .find_map(|n| run_pairs.iter().position(|p| p.name == *n))
        .unwrap_or(0)
}

/// 深さのテストスイート(`--suite depth`)で実行しないペア。
///
/// 深さのテストは `dk-ir` と RustKeleut のペアで比較する。
/// `dk` と `kf` は Diktor の構文木の評価器で実行するので、`keleut run --max-depth` の上限が適用されない。
const NOT_IN_DEPTH: &[&str] = &["dk", "kf"];

/// 深さのテストスイートを実行するなら真を返す。
fn depth_suite(o: &Options) -> bool {
    o.suite.as_deref() == Some("depth")
}

/// RustKeleut のバックエンドのペアの間の差分を集める。
///
/// KIR のライタが同じペアを 1 つのグループにする。
/// グループは 2 つある。
/// 処理系が [`Engine::Keleut`] のペアは、Diktor が書いた KIR を実行する(`ka` と `km` はシードごとの個々の構成を含む)。
/// 処理系が [`Engine::RustKeleut`] のペアは、Rust のフロントエンドが書いた KIR を実行する(`kr-ka` と `kr-km` はシードごとの個々の構成を含む)。
/// グループごとに、エラーのない最初のペアを基準にし、ほかのペアの各コマンドの結果を、正規化せずに基準のペアと比較する。
/// バックエンドどうしは完全に一致しなければならないからである。
/// ライタの違うグループの間(`ki` と `kr-ki` など)は、診断メッセージが違ってよいので、ここでは比較しない。
/// エラーのあるペアは [`report`] が失敗として数えるので、ここでは比較しない。
/// 返す差分は比較の種類が [`known_diffs::Comparison::Backend`] で、既知の差分のどの項目にも該当しない。
fn backend_diffs(
    case: usize,
    run_pairs: &[Pair],
    rs: &[CaseResult],
    commands: usize,
) -> Vec<Difference> {
    let mut v = Vec::new();
    for engine in [Engine::Keleut, Engine::RustKeleut] {
        v.extend(backend_group_diffs(case, run_pairs, rs, commands, engine));
    }
    v
}

/// [`backend_diffs`] のグループ 1 つ(処理系が `engine` のペア)の差分を集める。
fn backend_group_diffs(
    case: usize,
    run_pairs: &[Pair],
    rs: &[CaseResult],
    commands: usize,
    engine: Engine,
) -> Vec<Difference> {
    let ks: Vec<usize> = (0..run_pairs.len())
        .filter(|&pi| run_pairs[pi].engine == engine && rs[pi].error.is_none())
        .collect();
    let Some((&r0, rest)) = ks.split_first() else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for &pi in rest {
        for k in 0..commands {
            let want = cram::render(&rs[r0].outcomes[k]);
            let got = cram::render(&rs[pi].outcomes[k]);
            if want != got {
                v.push(Difference {
                    case,
                    command: k,
                    pair: run_pairs[pi].label.clone(),
                    against: format!("後段の組 {} ", run_pairs[r0].label),
                    cmp: known_diffs::Comparison::Backend,
                    want,
                    got,
                });
            }
        }
    }
    v
}

/// 実行の結果を比較して報告し、終了コードを返す。
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
    // 正規化すれば一致するが、正規化の前は違うもの(`--verbose` のときに表示する)。
    let mut exact_only = Vec::new();
    let mut compared = 0usize;
    let mut commands_total = 0usize;
    let mut invocations = 0usize;
    let mut diktor_commands = 0usize;
    let mut bypass = Vec::new();
    // コマンドの `--backend` がペアのバックエンドを上書きしたもの(shim のログの `backend-override` の行)。
    let mut overridden = Vec::new();
    let base_i = base_index(run_pairs);
    let depth = depth_suite(o);
    // バックエンドのペアの間の差分(既知の差分の一覧と照合しない)。
    let mut backend = Vec::new();
    // どのコマンドにも属さない期待出力の行の数(dune はこの行を捨てるので、期待出力との差分として数える)。
    let mut dropped = 0usize;
    for case in cases.iter().filter(|c| c.has_expectations) {
        for (line, text) in cram::dropped(&case.items) {
            dropped += 1;
            println!(
                "差: {}:{line}: どのコマンドの期待の出力にも付かない行(dune はこの行を捨てる)",
                case.name
            );
            println!("-{}", String::from_utf8_lossy(text));
        }
    }
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
                for v in inv.iter().filter(|v| v.who == "backend-override") {
                    overridden.push(format!(
                        "{}:{} [{}]: {}",
                        case.name,
                        c.line,
                        pair.label,
                        v.args.join(" ")
                    ));
                }
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
                // 深さのテストでは、スタック残量の検査(出力では「予備の検査」)による終了コード 70 を失敗として扱う。
                if depth && matches!(r.outcomes[k], cram::Outcome::Ran { code: 70, .. }) {
                    failures.push(format!(
                        "{}:{} [{}]: 終了状態 70(深さの試験では予備の検査を失敗にする)",
                        case.name, c.line, pair.label
                    ));
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
                let same = normalize::lines(&got) == normalize::lines(&c.expected);
                if same && got != c.expected && o.verbose {
                    exact_only.push(Difference {
                        case: ci,
                        command: k,
                        pair: pair.label.clone(),
                        against: "記録".to_string(),
                        cmp: known_diffs::Comparison::Record,
                        want: c.expected.clone(),
                        got: got.clone(),
                    });
                }
                if !same {
                    diffs.push(Difference {
                        case: ci,
                        command: k,
                        pair: pair.label.clone(),
                        against: "記録".to_string(),
                        cmp: known_diffs::Comparison::Record,
                        want: c.expected.clone(),
                        got,
                    });
                }
            }
        }
        if compare_pairs && run_pairs.len() > 1 {
            let base = &results[ci][base_i];
            for (pi, pair) in run_pairs.iter().enumerate() {
                if pi == base_i {
                    continue;
                }
                let r = &results[ci][pi];
                if r.error.is_some() || base.error.is_some() {
                    continue;
                }
                for k in 0..cmds.len() {
                    let want = cram::render(&base.outcomes[k]);
                    let got = cram::render(&r.outcomes[k]);
                    let same = normalize::lines(&want) == normalize::lines(&got);
                    if same && want != got && o.verbose {
                        exact_only.push(Difference {
                            case: ci,
                            command: k,
                            pair: pair.label.clone(),
                            against: format!("組 {} ", run_pairs[base_i].label),
                            cmp: known_diffs::Comparison::Pair,
                            want: want.clone(),
                            got: got.clone(),
                        });
                    }
                    if !same {
                        diffs.push(Difference {
                            case: ci,
                            command: k,
                            pair: pair.label.clone(),
                            against: format!("組 {} ", run_pairs[base_i].label),
                            cmp: known_diffs::Comparison::Pair,
                            want,
                            got,
                        });
                    }
                }
            }
        }
        if compare_pairs {
            backend.extend(backend_diffs(ci, run_pairs, &results[ci], cmds.len()));
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
            .position(|e| e.matches(&case.key, c.line, pair_name, d.cmp, &d.want, &d.got));
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
    // バックエンドのペアの間の差分は、すべて未登録の差分として数える。
    let mut backend_unknown = 0usize;
    for d in &backend {
        let case = &cases[d.case];
        let c = cram::commands(&case.items)[d.command];
        backend_unknown += 1;
        println!(
            "差: {}:{} [{}]: {}との差(後段の間は既知の差に関係なく一致しなければならない)",
            case.name, c.line, d.pair, d.against
        );
        println!("  $ {}", c.display());
        show(&d.want, '-');
        show(&d.got, '+');
    }
    for d in &exact_only {
        let case = &cases[d.case];
        let c = cram::commands(&case.items)[d.command];
        println!(
            "正規化の前の不一致: {}:{} [{}]: {}との不一致",
            case.name, c.line, d.pair, d.against
        );
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
    // ペアのバックエンドで実行されなかったコマンドは、そのペアの比較から漏れているので、`--verbose` でなくても表示する。
    for b in &overridden {
        println!("組の後段の上書き: {b}");
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
    let diffs_known = diffs.len() - unknown + backend.len() - backend_unknown;
    unknown += dropped + backend_unknown;
    let base_note = if compare_pairs && run_pairs.len() > 1 {
        format!("(基準 {})", run_pairs[base_i].label)
    } else {
        String::new()
    };
    println!(
        "kconform: 組 {}{base_note}、{} 個の試験、{} 個のコマンド(記録と比べた {compared} 個)、差 {unknown} 件、既知の差 {diffs_known} 件、失敗 {} 件",
        labels.join(","),
        cases.len(),
        commands_total,
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

/// 実行するテストを集める。
///
/// ファイルを指定しなければ、`--suite` のテストスイートを集める。
/// `all` のコーパスは、`include_corpus` が真のときだけ加える。
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

/// 既知の差分の項目が指すことのある、すべてのテストのファイルのパスを返す。
///
/// diktor/test の `.t`、`conformance/` の下のすべての `.t`(`depth/` と `frontend-depth/` を含む)、
/// コーパスの `.kel` である。
fn all_test_keys(o: &Options) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = t_files_in(&o.root.join("../diktor/test"))
        .into_iter()
        .chain(conformance_files(&o.conformance(), &[]))
        .map(|p| absolute(&p))
        .collect();
    let corpus = o.corpus_dir();
    if let Ok(es) = corpus::read_index(&corpus) {
        v.extend(es.iter().map(|e| absolute(&corpus.join(&e.path))));
    }
    v
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
    // テストが 1 つもなければ、何も確かめていないので失敗として終了する(`conformance/depth/` がないときなど)。
    if cases.is_empty() {
        eprintln!("kconform: 走らせる試験がありません");
        return 1;
    }
    let mut known = Vec::new();
    for p in o.known_diffs_paths() {
        match known_diffs::load(&p) {
            Ok(k) => known.extend(k),
            Err(e) => {
                eprintln!("kconform: {e}");
                return 64;
            }
        }
    }
    let errs = known_diffs::validate(&known);
    for e in &errs {
        eprintln!("kconform: {e}");
    }
    if !errs.is_empty() {
        return 64;
    }
    // テストスイートを実行するときは、既知の差分の一覧に、どのテストのファイルにも該当しない項目があれば使い方のエラーにする。
    // ファイル名の誤りを検出するためである。
    // 実行しないペアの項目や、このテストスイートで集めないコーパスと `depth` のテストの項目も確かめる。
    if o.files.is_empty() {
        let universe = all_test_keys(o);
        let mut orphan = 0;
        for e in &known {
            if !universe.iter().any(|k| k.ends_with(Path::new(&e.file))) {
                eprintln!(
                    "kconform: {} の {} 行の項目 {}:{} は、どの試験のファイルにも当たりません",
                    e.origin, e.source_line, e.file, e.line
                );
                orphan += 1;
            }
        }
        if orphan > 0 {
            return 64;
        }
    }
    // `kconform expect` は期待出力とだけ比較するので、基準のペアとの差分だけを許す項目を使わない。
    if !compare_pairs {
        known.retain(|e| e.against != known_diffs::Against::Pair);
    }
    // 一覧の項目が、実行するテストのコマンドの行を指していることを確かめる。
    // テストを編集して行番号がずれたことを検出するためである。
    let mut stale = 0;
    for e in &known {
        for c in cases.iter().filter(|c| c.key.ends_with(Path::new(&e.file))) {
            if !cram::commands(&c.items)
                .iter()
                .any(|cmd| cmd.line == e.line)
            {
                eprintln!(
                    "kconform: {} の {} 行の項目 {}:{} はコマンドの行ではありません",
                    e.origin, e.source_line, e.file, e.line
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
    let depth = depth_suite(o);
    let names: Vec<String> = o.pairs.clone().unwrap_or_else(|| {
        pairs::DEFAULT
            .iter()
            .filter(|n| !depth || !NOT_IN_DEPTH.contains(n))
            .map(|s| s.to_string())
            .collect()
    });
    let mut run_pairs = Vec::new();
    for n in &names {
        if depth && NOT_IN_DEPTH.contains(&n.as_str()) {
            eprintln!(
                "kconform: 組 {n} は深さの試験では走らせません(深さの試験は dk-ir と RustKeleut の組で比べる)"
            );
            return 64;
        }
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
    let jit_names: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| pairs::needs_jit(n))
        .collect();
    if !jit_names.is_empty() && !keleut_has_jit(o) {
        if o.pairs.is_some() {
            eprintln!(
                "kconform: 組 {} は LLVM の後段を使いますが、{} は feature llvm を外して組まれています(cargo build --release で組み直してください)",
                jit_names.join(","),
                o.keleut_path().display()
            );
            return 64;
        }
        eprintln!(
            "kconform: {} は feature llvm を外して組まれているので、LLVM の後段の組 {} を走らせません",
            o.keleut_path().display(),
            jit_names.join(",")
        );
        run_pairs.retain(|p| !pairs::needs_jit(p.name));
    }
    if run_pairs.is_empty() {
        eprintln!("kconform: --pairs is empty");
        return 64;
    }
    run_suite(o, run_pairs, true)
}

/// 使う `keleut` がバックエンド `jit`(feature `llvm`)を持つかどうかを、`keleut --version` の出力で調べる。
///
/// `keleut` を起動できないか、出力に `backends:` の行がないときは、持つとみなす。
/// そのときはペアの実行そのものが失敗として報告されるので、ここではペアを除かない。
fn keleut_has_jit(o: &Options) -> bool {
    let out = std::process::Command::new(o.keleut_path())
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(out) => pairs::version_has_jit(&String::from_utf8_lossy(&out.stdout)).unwrap_or(true),
        Err(_) => true,
    }
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
        fixtures: o.conformance().join("kir-fixtures"),
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
    // shim として呼ばれたときの引数は、UTF-8 でないことがある(テストが意図して渡す)。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(text: &str, code: i32) -> cram::Outcome {
        cram::Outcome::Ran {
            output: text.as_bytes().to_vec(),
            code,
        }
    }

    fn result(outcomes: Vec<cram::Outcome>) -> CaseResult {
        CaseResult {
            invocations: outcomes.iter().map(|_| Vec::new()).collect(),
            outcomes,
            timed_out: false,
            error: None,
        }
    }

    #[test]
    fn 後段の組の間の差は既知の差の項目でも許さない() {
        let mut ps = pairs::expand("dk").unwrap();
        ps.extend(pairs::expand("ki").unwrap());
        ps.extend(pairs::expand("km").unwrap());
        let mut rs = vec![result(vec![ran("a\n", 0)])];
        rs.push(result(vec![ran("x\n", 0)]));
        for out in ["x\n", "y\n", "x\n", "x\n"] {
            rs.push(result(vec![ran(out, 0)]));
        }
        // ペア `dk` との差分は、バックエンドの間の差分ではない。
        let d = backend_diffs(0, &ps, &rs, 1);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].pair, "km=2");
        assert_eq!(d[0].cmp, known_diffs::Comparison::Backend);
        // 両方のペアを挙げた項目も、すべてのペアを指す項目も該当しない。
        let es = known_diffs::parse(
            "[[diff]]\nfile = \"a.t\"\nline = 1\npairs = [\"ki\", \"km\"]\nreason = \"r\"\nledger = \"V1\"\n\n[[diff]]\nfile = \"a.t\"\nline = 1\npairs = [\"*\"]\nreason = \"r\"\nledger = \"V1\"\nagainst = \"pair\"\n",
        )
        .unwrap();
        for e in &es {
            assert!(!e.matches(
                Path::new("/x/a.t"),
                1,
                "km",
                d[0].cmp,
                &d[0].want,
                &d[0].got
            ));
            assert!(e.matches(
                Path::new("/x/a.t"),
                1,
                "km",
                known_diffs::Comparison::Pair,
                &d[0].want,
                &d[0].got
            ));
        }
        // 正規化で消える違いも差分として数える。
        let ps2 = [pairs::expand("ki").unwrap(), pairs::expand("km").unwrap()].concat();
        let rs2: Vec<CaseResult> = ["x.kel:1:1: 型エラー: 甲\n", "x.kel:1:1: 型エラー: 乙\n"]
            .iter()
            .chain(["x.kel:1:1: 型エラー: 甲\n"; 3].iter())
            .map(|t| result(vec![ran(t, 1)]))
            .collect();
        assert_eq!(backend_diffs(0, &ps2, &rs2, 1).len(), 1);
        // エラーのあるペアは比較しない。
        let mut rs3 = rs2;
        rs3[1].error = Some("e".to_string());
        assert!(backend_diffs(0, &ps2, &rs3, 1).is_empty());
        // 基準のペアにエラーがあれば、エラーのない次のペアを基準にする。
        rs3[1].error = None;
        rs3[0].error = Some("e".to_string());
        assert_eq!(backend_diffs(0, &ps2, &rs3, 1).len(), 3);
    }
}
