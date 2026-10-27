//! 1 つの cram の試験を、一時のディレクトリで走らせる。
//!
//! 走らせ方は dune の cram と同じにする(diktor/test/via_ir_run.py と同じ形)。
//!
//! - 試験ごとに空のディレクトリを作り、そこに試料(`sample/*.kel` と、`.t` の横の `.kel`、`.kir`、`.kirt`)を写す。
//! - すべてのコマンドを 1 本の `sh` で走らせる。
//!   `cd` や `export` は後のコマンドに残る。
//!   各コマンドはファイルに書いて `.` で読み込み、標準出力と標準エラーを 1 本のファイルにし、直後に `$?` を記録する。
//! - シェルが `exit` で終わっても `trap 'exit 0' EXIT` で 0 にし、記録の無いコマンドは「到達しない」とする。
//! - 標準入力は `/dev/null` にする。
//!   `LC_ALL=C` にし、`TMPDIR` を試験の中の空のディレクトリにし、`TESTDIR` と `DIKTOR_VIA_IR` を消す。
//! - 出力は、dune 3.21.1 の cram(`cram_exec.ml` の `sanitize` と `rewrite_paths`)と同じ順に整える。
//!   まず端末の色の指定を除く。
//!   次に、パスの置き換えを dune と同じく語ごとに行う([`rewrite_paths`])。
//!   作業ディレクトリで始まる語の頭を `$TESTCASE_ROOT` にするのは dune と同じである。
//!   `TMPDIR` で始まる語の頭を `$TMPDIR` にするのは kconform だけの正規化である。
//!   dune の試験のシェルの `TMPDIR` は dune の作る `/tmp/build_*_dune` で、dune はこれを置き換えない。
//!   kconform の `TMPDIR` は試験ごとに作るディレクトリなので、置き換えないと出力が走らせるたびに変わる。
//!   最後に、シェルの誤りの文面の頭に付くスクリプトの位置(`<main.sh>: N: <k.sh>: ` と `<k.sh>: line N: `)を除く。
//!   スクリプトは作業ディレクトリと `TMPDIR` の外(`meta/`)に置くので、パスの置き換えはスクリプトの位置に当たらない。
//! - 行への切り方(改行の直前の CR を 1 つ落とすことなど)は、`cram::render` が dune の `split_lines` と同じにする。
//! - PATH の先頭に shim のディレクトリを置き、元の PATH からは `diktor` と `keleut` を持つディレクトリを除く。
//!   shim を通らない呼び出しが、本物に黙って届かないようにするためである。

use crate::cram::{self, Item, Outcome};
use crate::pairs::Pair;
use crate::util::{TempDir, sh_quote, strip_ansi, strip_script_prefixes, wait_timeout};
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// 走らせる 1 つの試験である。
#[derive(Debug, Clone)]
pub struct Case {
    /// 表示に使う名前(`.t` のパス)である。
    pub name: String,
    /// 既知の差の照合に使うパス(正規化した絶対パス)である。
    pub key: PathBuf,
    /// 試験の項目である。
    pub items: Vec<Item>,
    /// 写す試料の、元のパスと作業ディレクトリの中の相対パスである。
    pub fixtures: Vec<(PathBuf, PathBuf)>,
    /// `.t` に記録した期待と比べるかどうかである(コーパスの試験は比べない)。
    pub has_expectations: bool,
}

impl Case {
    /// `.t` のファイルを読んで試験にする。
    pub fn from_file(path: &Path) -> std::io::Result<Case> {
        let text = std::fs::read(path)?;
        let key = std::fs::canonicalize(path)?;
        let dir = key.parent().unwrap_or(Path::new("/")).to_path_buf();
        Ok(Case {
            name: path.display().to_string(),
            key,
            items: cram::parse(&text),
            fixtures: fixtures_of(&dir)?,
            has_expectations: true,
        })
    }
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.contains(&e))
}

/// `.t` の横の試料を集める。
///
/// dune の cram の試験(diktor/test)は、test/dune が依存に挙げる `sample/*.kel` だけを見る。
/// `conformance/` の試験は、`.t` の横の `.kel`、`.kir`、`.kirt` も見る。
fn fixtures_of(dir: &Path) -> std::io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut v = Vec::new();
    let sample = dir.join("sample");
    if sample.is_dir() {
        for e in std::fs::read_dir(&sample)? {
            let p = e?.path();
            if p.is_file() && has_ext(&p, &["kel"]) {
                let rel = Path::new("sample").join(p.file_name().unwrap());
                v.push((p, rel));
            }
        }
    }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_file() && has_ext(&p, &["kel", "kir", "kirt"]) {
            let rel = PathBuf::from(p.file_name().unwrap());
            v.push((p, rel));
        }
    }
    v.sort();
    Ok(v)
}

/// 走らせ方の設定である。
#[derive(Debug, Clone)]
pub struct Config {
    /// shim(`diktor` と `keleut`)を置いたディレクトリである。
    pub shim_dir: PathBuf,
    /// 本物の diktor である。
    pub real_diktor: PathBuf,
    /// 本物の keleut である。
    pub real_keleut: PathBuf,
    /// 試験のコマンドの PATH の、shim の後ろに続く部分である。
    pub path_tail: OsString,
    /// 1 つの試験の時間の上限である。
    pub timeout: Duration,
    /// 一時のディレクトリを残すかどうかである。
    pub keep: bool,
    /// `--extra` の旗である。
    pub extra: Vec<String>,
}

/// shim の 1 回の呼び出しである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// 呼ばれた shim の名前(`diktor` か `keleut`)である。
    pub who: String,
    /// 引数である。
    pub args: Vec<String>,
}

/// 1 つの試験を 1 つの組で走らせた結果である。
#[derive(Debug, Clone)]
pub struct CaseResult {
    /// コマンドごとの結果である。
    pub outcomes: Vec<Outcome>,
    /// コマンドごとの shim の呼び出しである。
    pub invocations: Vec<Vec<Invocation>>,
    /// 時間切れになったかどうかである。
    pub timed_out: bool,
    /// 走らせる前の誤り(試料を写せないなど)である。
    pub error: Option<String>,
}

/// 元の PATH から、`diktor` か `keleut` を持つディレクトリを除く。
/// 戻り値は、残した PATH と除いたディレクトリの列である。
pub fn filter_path(path: &OsString) -> (OsString, Vec<PathBuf>) {
    let mut keep = Vec::new();
    let mut removed = Vec::new();
    for d in std::env::split_paths(path) {
        if d.join("diktor").exists() || d.join("keleut").exists() {
            removed.push(d);
        } else {
            keep.push(d);
        }
    }
    (std::env::join_paths(keep).unwrap_or_default(), removed)
}

/// 試験を走らせる。
pub fn run(case: &Case, pair: &Pair, cfg: &Config) -> CaseResult {
    match run_inner(case, pair, cfg) {
        Ok(r) => r,
        Err(e) => CaseResult {
            outcomes: Vec::new(),
            invocations: Vec::new(),
            timed_out: false,
            error: Some(e.to_string()),
        },
    }
}

fn run_inner(case: &Case, pair: &Pair, cfg: &Config) -> std::io::Result<CaseResult> {
    let work = TempDir::new("kconform", cfg.keep)?;
    let root = work.path().join("root");
    let meta = work.path().join("meta");
    let tmp = work.path().join("tmp");
    for d in [&root, &meta, &tmp] {
        std::fs::create_dir(d)?;
    }
    for (src, rel) in &case.fixtures {
        let dst = root.join(rel);
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(src, dst)?;
    }
    let commands = cram::commands(&case.items);
    let q = |p: &Path| sh_quote(&p.to_string_lossy());
    let mut script = String::from("trap 'exit 0' EXIT\n");
    for (k, c) in commands.iter().enumerate() {
        let cmd_file = meta.join(format!("{k}.sh"));
        std::fs::write(&cmd_file, c.script())?;
        let out_file = meta.join(format!("{k}.output"));
        let code_file = meta.join(format!("{k}.code"));
        script.push_str(&format!(
            "KCONFORM_CMD={k}; export KCONFORM_CMD\n. {} > {} 2>&1\nprintf '%d' $? > {}\n",
            q(&cmd_file),
            q(&out_file),
            q(&code_file)
        ));
    }
    let main = meta.join("main.sh");
    std::fs::write(&main, script)?;
    let log = meta.join("shim.log");
    let mut path = OsString::from(&cfg.shim_dir);
    if !cfg.path_tail.is_empty() {
        path.push(":");
        path.push(&cfg.path_tail);
    }
    let mut pair_args = pair.args.clone();
    if matches!(
        pair.engine,
        crate::pairs::Engine::Keleut | crate::pairs::Engine::RustKeleut
    ) {
        pair_args.extend(cfg.extra.iter().cloned());
    }
    let mut child = Command::new("sh")
        .arg(&main)
        .current_dir(&root)
        .env_remove("TESTDIR")
        .env_remove("DIKTOR_VIA_IR")
        .env("PATH", path)
        .env("LC_ALL", "C")
        .env("TMPDIR", &tmp)
        .env("KCONFORM_REAL_DIKTOR", &cfg.real_diktor)
        .env("KCONFORM_REAL_KELEUT", &cfg.real_keleut)
        .env("KCONFORM_PAIR", pair.name)
        .env("KCONFORM_PAIR_ARGS", pair_args.join(" "))
        .env("KCONFORM_LOG", &log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let timed_out = wait_timeout(&mut child, cfg.timeout)?.is_none();
    let root_s = root.to_string_lossy().into_owned().into_bytes();
    let tmp_s = tmp.to_string_lossy().into_owned().into_bytes();
    let main_s = main.to_string_lossy().into_owned();
    let mut outcomes = Vec::new();
    for k in 0..commands.len() {
        let code_file = meta.join(format!("{k}.code"));
        match std::fs::read_to_string(&code_file) {
            Ok(code) => {
                let out = std::fs::read(meta.join(format!("{k}.output"))).unwrap_or_default();
                let cmd_s = meta.join(format!("{k}.sh")).to_string_lossy().into_owned();
                let out = sanitize(&out, &root_s, &tmp_s, main_s.as_bytes(), cmd_s.as_bytes());
                outcomes.push(Outcome::Ran {
                    output: out,
                    code: code.trim().parse().unwrap_or(0),
                });
            }
            Err(_) => outcomes.push(Outcome::Unreachable),
        }
    }
    let mut invocations = vec![Vec::new(); commands.len()];
    if let Ok(text) = std::fs::read(&log) {
        for line in text.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            let line = String::from_utf8_lossy(line);
            let mut parts = line.splitn(3, '\t');
            let k: Option<usize> = parts.next().and_then(|s| s.parse().ok());
            let who = parts.next().unwrap_or("").to_string();
            let args = parts
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.split('\x1f').map(str::to_string).collect())
                .unwrap_or_default();
            if let Some(k) = k.filter(|&k| k < invocations.len()) {
                invocations[k].push(Invocation { who, args });
            }
        }
    }
    Ok(CaseResult {
        outcomes,
        invocations,
        timed_out,
        error: None,
    })
}

/// 1 つのコマンドの出力を、dune 3.21.1 の cram と同じ順に整える。
///
/// 端末の色の指定を除き、パスを語ごとに置き換え([`rewrite_paths`])、
/// 行の頭のスクリプトの位置(親のスクリプト `main` とコマンドのスクリプト `cmd` のもの)を除く。
pub fn sanitize(out: &[u8], root: &[u8], tmp: &[u8], main: &[u8], cmd: &[u8]) -> Vec<u8> {
    let out = strip_ansi(out);
    let out = rewrite_paths(&out, root, tmp);
    strip_script_prefixes(&out, main, cmd)
}

/// パスを語ごとに置き換える(dune 3.21.1 の `cram_exec.ml` の `rewrite_paths` と同じ切り方)。
///
/// 語は、`/` で始まり、空白、改行、CR、タブ、`:` のどれでもないバイトが続く最長の並びである(正規表現 `/[^ \n\r\t:]+`)。
/// 語の頭が `tmp` なら `$TMPDIR` に、そうでなく `root` なら `$TESTCASE_ROOT` に、1 つの語につき 1 度だけ置き換える。
/// dune の `Build_path_prefix_map.rewrite` と同じく、頭の比べ方は文字列の接頭辞で、パスの部分の境目を見ない。
/// 新しく足した対応から先に見るので、後に足す `tmp` を先に比べる。
/// 語の途中に現れるパスは置き換えない。
/// `$TMPDIR` への置き換えは kconform だけの正規化である(モジュールの説明を見よ)。
pub fn rewrite_paths(out: &[u8], root: &[u8], tmp: &[u8]) -> Vec<u8> {
    let stop = |b: u8| matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b':');
    let mut v = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        if out[i] != b'/' {
            v.push(out[i]);
            i += 1;
            continue;
        }
        let j = out[i + 1..]
            .iter()
            .position(|&b| stop(b))
            .map_or(out.len(), |k| i + 1 + k);
        let word = &out[i..j];
        if j == i + 1 {
            v.push(b'/');
        } else if let Some(r) = (!tmp.is_empty()).then(|| word.strip_prefix(tmp)).flatten() {
            v.extend_from_slice(b"$TMPDIR");
            v.extend_from_slice(r);
        } else if let Some(r) = (!root.is_empty())
            .then(|| word.strip_prefix(root))
            .flatten()
        {
            v.extend_from_slice(b"$TESTCASE_ROOT");
            v.extend_from_slice(r);
        } else {
            v.extend_from_slice(word);
        }
        i = j;
    }
    v
}

/// コマンドの行(ヒアドキュメントの中身を除く)が、`diktor` をコマンドの語として含むかどうかを返す。
///
/// shim を通らずに本物へ届く呼び出しを探すために使う。
/// 語の境目だけを見る粗い判定で、引用符の中の `diktor` も数える。
pub fn mentions(command: &cram::Command, word: &str) -> bool {
    if defines_function(command) {
        return false;
    }
    let mut terminator: Option<String> = None;
    for line in &command.text {
        let line = String::from_utf8_lossy(line);
        if let Some(t) = &terminator {
            if line.trim_start_matches('\t') == t {
                terminator = None;
            }
            continue;
        }
        if has_word(&line, word) {
            return true;
        }
        terminator = heredoc_terminator(&line);
    }
    false
}

/// コマンドが、シェルの関数を定義するだけのもの(`name() { … }`)かどうかを返す。
/// 関数の本体の `diktor` は、関数を呼んだコマンドで走るので、定義のコマンドでは数えない。
fn defines_function(command: &cram::Command) -> bool {
    let first = String::from_utf8_lossy(&command.text[0]);
    let t = first.trim_start();
    let name: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    !name.is_empty() && t[name.len()..].trim_start().starts_with("()")
}

fn has_word(line: &str, word: &str) -> bool {
    let b = line.as_bytes();
    // `/` は境目として扱う。`$DIR/diktor` のようにパスで呼ぶ(shim を通らない)呼び出しも見つけるためである。
    let is_word = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.');
    let mut start = 0;
    while let Some(i) = line[start..].find(word) {
        let i = start + i;
        let j = i + word.len();
        let before_ok = i == 0 || !is_word(b[i - 1]);
        let after_ok = j == b.len() || !is_word(b[j]);
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
    }
    false
}

/// 行がヒアドキュメントを始めるなら、その終わりの語を返す。
pub fn heredoc_terminator(line: &str) -> Option<String> {
    let i = line.find("<<")?;
    let rest = line[i + 2..].trim_start_matches('-').trim_start();
    let word: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | ')' | '>'))
        .collect();
    let word = word.trim_matches(|c| c == '\'' || c == '"').to_string();
    if word.is_empty() { None } else { Some(word) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(lines: &[&str]) -> cram::Command {
        cram::Command {
            line: 1,
            text: lines.iter().map(|l| l.as_bytes().to_vec()).collect(),
            expected: vec![],
        }
    }

    #[test]
    fn コマンドの語としてのdiktorを見つける() {
        assert!(mentions(&cmd(&["diktor a.kel"]), "diktor"));
        assert!(mentions(
            &cmd(&["printf 'x' | diktor --repl; echo $?"]),
            "diktor"
        ));
        assert!(mentions(&cmd(&["(cd d && diktor x.kel)"]), "diktor"));
        assert!(!mentions(&cmd(&["cat a.diktor"]), "diktor"));
        assert!(mentions(&cmd(&["$D/diktor a.kel"]), "diktor"));
        assert!(!mentions(&cmd(&["inv() { diktor \"$1\"; }"]), "diktor"));
        assert!(!mentions(
            &cmd(&["cat > a.kel <<'EOF'", "diktor", "EOF"]),
            "diktor"
        ));
        assert!(mentions(
            &cmd(&["cat > a.kel <<'EOF'", "x", "EOF", "diktor a.kel"]),
            "diktor"
        ));
    }

    #[test]
    fn 色の指定とパスとスクリプトの位置の順に整える() {
        let s = |o: &[u8]| {
            sanitize(
                o,
                b"/w/root",
                b"/w/tmp",
                b"/w/meta/main.sh",
                b"/w/meta/0.sh",
            )
        };
        assert_eq!(
            s(b"\x1b[31m/w/meta/0.sh: line 1: /w/root/x: Permission denied\x1b[0m\n"),
            b"$TESTCASE_ROOT/x: Permission denied\n"
        );
        assert_eq!(
            s(b"/w/meta/main.sh: 3: /w/meta/0.sh: cannot create /w/tmp/a: x\n"),
            b"cannot create $TMPDIR/a: x\n"
        );
        // 色の指定を先に除くので、色の指定に挟まれたスクリプトの位置も除く。
        assert_eq!(s(b"\x1b[1m/w/meta/0.sh: line 2: y\n"), b"y\n");
    }

    #[test]
    fn パスは語の頭だけを_1_度置き換える() {
        let r = |o: &[u8]| rewrite_paths(o, b"/w/root", b"/w/tmp");
        // dune 3.21.1 と同じく、語の途中のパスと、別の語に続くパスは置き換えない。
        assert_eq!(r(b"/x/w/root/a"), b"/x/w/root/a");
        assert_eq!(r(b"/w/root/a/w/root"), b"$TESTCASE_ROOT/a/w/root");
        assert_eq!(r(b"/w/root/a /w/tmp/b"), b"$TESTCASE_ROOT/a $TMPDIR/b");
        // `/` は語の始まりなので、語の文字に続く `/` からも語が始まる。
        assert_eq!(r(b"x/w/root/a"), b"x$TESTCASE_ROOT/a");
        // `:` は語を切る。
        assert_eq!(r(b"/w/root/a:/w/tmp:x"), b"$TESTCASE_ROOT/a:$TMPDIR:x");
        // 頭の比べ方は文字列の接頭辞である。
        assert_eq!(r(b"/w/rootx"), b"$TESTCASE_ROOTx");
        assert_eq!(r(b"a / b //"), b"a / b //");
    }

    #[test]
    fn ヒアドキュメントの終わりの語() {
        assert_eq!(
            heredoc_terminator("cat > a.kel <<'EOF'").as_deref(),
            Some("EOF")
        );
        assert_eq!(
            heredoc_terminator("cat <<EOF > a.kel").as_deref(),
            Some("EOF")
        );
        assert_eq!(heredoc_terminator("echo hi"), None);
    }
}
