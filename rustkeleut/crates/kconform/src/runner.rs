//! cram 形式のテスト 1 つを、一時ディレクトリで実行する。
//!
//! 実行の方法は dune の cram に合わせる(diktor/test/via_ir_run.py と同じ方法である)。
//!
//! - テストごとに空のディレクトリを作り、そこに入力ファイル(`sample/*.kel` と、`.t` と同じディレクトリの `.kel`、`.kir`、`.kirt`)をコピーする。
//! - すべてのコマンドを 1 つの `sh` で実行する。
//!   そのため、`cd` や `export` は後のコマンドにも影響する。
//!   各コマンドはファイルに書いて `.` で読み込み、標準出力と標準エラーを 1 つのファイルにまとめ、直後に `$?` をファイルに書く。
//! - シェルが `exit` で終了しても、`trap 'exit 0' EXIT` でシェルの終了コードを 0 にする。
//!   `$?` のファイルがないコマンドは、「到達しない」コマンド(`Outcome::Unreachable`)として扱う。
//! - 標準入力は `/dev/null` にする。
//!   `LC_ALL=C` にし、`TMPDIR` をテスト用の空のディレクトリにし、`TESTDIR` と `DIKTOR_VIA_IR` を環境から消す。
//! - 出力の後処理は、dune 3.21.1 の cram(`cram_exec.ml` の `sanitize` と `rewrite_paths`)と同じ順に行う。
//!   まず、端末の色を指定するエスケープシーケンスを取り除く。
//!   次に、dune と同じく、単語ごとにパスを置き換える([`rewrite_paths`])。
//!   作業ディレクトリで始まる単語の先頭を `$TESTCASE_ROOT` にするのは dune と同じである。
//!   `TMPDIR` で始まる単語の先頭を `$TMPDIR` にするのは、kconform だけが行う置き換えである。
//!   dune がテストのシェルに渡す `TMPDIR` は dune の作る `/tmp/build_*_dune` で、dune はこれを置き換えない。
//!   kconform の `TMPDIR` はテストごとに作るディレクトリなので、置き換えないと出力が実行のたびに変わる。
//!   最後に、シェルのエラーメッセージの先頭に付くスクリプトの位置(`<main.sh>: N: <k.sh>: ` と `<k.sh>: line N: `)を取り除く。
//!   スクリプトは作業ディレクトリと `TMPDIR` の外(`meta/`)に置くので、パスの置き換えはスクリプトの位置を変えない。
//! - 出力を行に分ける方法(改行の直前の CR を 1 つ取り除くことなど)は、`cram::render` が dune の `split_lines` に合わせる。
//! - PATH の先頭に shim のディレクトリを置き、元の PATH からは `diktor` か `keleut` を持つディレクトリを取り除く。
//!   shim を通らない呼び出しが、気づかれないまま本物の `diktor` や `keleut` を実行することを防ぐためである。

use crate::cram::{self, Item, Outcome};
use crate::pairs::Pair;
use crate::util::{TempDir, sh_quote, strip_ansi, strip_script_prefixes, wait_timeout};
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// 実行するテスト 1 つ。
#[derive(Debug, Clone)]
pub struct Case {
    /// 表示に使う名前。`.t` のテストではそのパスである。
    pub name: String,
    /// 既知の差分の照合に使うパス。シンボリックリンクを解決した絶対パスである。
    pub key: PathBuf,
    /// `.t` を読んで得た項目(コマンド、コメントなど)の並び。
    pub items: Vec<Item>,
    /// コピーする入力ファイルの、元のパスと、作業ディレクトリからの相対パスの組。
    pub fixtures: Vec<(PathBuf, PathBuf)>,
    /// `.t` に書いた期待出力と比較するなら真。コーパスのテストは期待出力を持たないので偽である。
    pub has_expectations: bool,
}

impl Case {
    /// `.t` のファイルを読んでテストにする。
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

/// `.t` のあるディレクトリ `dir` から、テストの入力ファイルを集める。
///
/// dune の cram のテスト(diktor/test)が使えるのは、test/dune が依存に挙げる `sample/*.kel` だけである。
/// `conformance/` のテストは、`.t` と同じディレクトリの `.kel`、`.kir`、`.kirt` も使う。
/// この関数は 2 つを区別せず、`dir/sample/*.kel` と、`dir` の直下の `.kel`、`.kir`、`.kirt` をすべて集める。
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

/// テストの実行の設定。
#[derive(Debug, Clone)]
pub struct Config {
    /// shim(`diktor` と `keleut`)を置いたディレクトリ。
    pub shim_dir: PathBuf,
    /// 本物の diktor のパス。
    pub real_diktor: PathBuf,
    /// 本物の keleut のパス。
    pub real_keleut: PathBuf,
    /// テストのコマンドに渡す PATH のうち、shim のディレクトリの後ろに続く部分。
    pub path_tail: OsString,
    /// テスト 1 つのタイムアウト。
    pub timeout: Duration,
    /// 真なら一時ディレクトリを残す。
    pub keep: bool,
    /// `--extra` で指定したオプション。RustKeleut のバックエンドのペアのオプションに追加する。
    pub extra: Vec<String>,
}

/// shim のログの 1 行。shim の呼び出し 1 回か、shim が書いた振り分けの結果を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// 呼ばれた shim の名前(`diktor` か `keleut`)。振り分けの結果の行では、その種類(`backend-override` など)が入る。
    pub who: String,
    /// 呼び出しの引数。振り分けの結果の行では、その内容が入る。
    pub args: Vec<String>,
}

/// テスト 1 つをペア 1 つで実行した結果。
#[derive(Debug, Clone)]
pub struct CaseResult {
    /// コマンドごとの結果。
    pub outcomes: Vec<Outcome>,
    /// コマンドごとの、shim の呼び出しの並び。
    pub invocations: Vec<Vec<Invocation>>,
    /// タイムアウトになったら真。
    pub timed_out: bool,
    /// コマンドを実行できなかったときのエラー(入力ファイルをコピーできないなど)。
    pub error: Option<String>,
}

/// 元の PATH から、`diktor` か `keleut` を持つディレクトリを取り除く。
/// 残した PATH と、取り除いたディレクトリの並びを返す。
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

/// テスト `case` をペア `pair` で実行する。
/// 一時ディレクトリの作成やファイルのコピーに失敗したときは、`error` にエラーを入れた結果を返す。
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
        // `keleut run --frontend diktor` が PATH の diktor(shim)を呼ぶように、
        // `$KELEUT_DIKTOR` を子プロセスに渡さない(本物の diktor のパスは `KCONFORM_REAL_DIKTOR` で渡す)。
        .env_remove("KELEUT_DIKTOR")
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

/// コマンド 1 つの出力に、dune 3.21.1 の cram と同じ順で後処理を行う。
///
/// 端末の色を指定するエスケープシーケンスを取り除き、単語ごとにパスを置き換え([`rewrite_paths`])、
/// 行の先頭に付くスクリプトの位置(親のスクリプト `main` とコマンドのスクリプト `cmd` のもの)を取り除く。
pub fn sanitize(out: &[u8], root: &[u8], tmp: &[u8], main: &[u8], cmd: &[u8]) -> Vec<u8> {
    let out = strip_ansi(out);
    let out = rewrite_paths(&out, root, tmp);
    strip_script_prefixes(&out, main, cmd)
}

/// 単語ごとにパスを置き換える。単語の区切り方は、dune 3.21.1 の `cram_exec.ml` の `rewrite_paths` と同じである。
///
/// 単語は、`/` で始まり、空白、改行、CR、タブ、`:` のどれでもないバイトが続く最長の並びである(正規表現 `/[^ \n\r\t:]+`)。
/// 単語の先頭が `tmp` なら `$TMPDIR` に、そうでなく `root` なら `$TESTCASE_ROOT` に置き換える。
/// 置き換えは、単語 1 つにつき 1 度だけ行う。
/// dune の `Build_path_prefix_map.rewrite` と同じく、単語の先頭は文字列の接頭辞として比較し、パスの要素の境界は考慮しない。
/// dune は、接頭辞と置き換え後の文字列の対応を、後から追加したものから先に調べる。
/// `tmp` の対応は `root` の対応より後に追加するものなので、`tmp` を先に比較する。
/// 単語の途中に現れるパスは置き換えない。
/// `$TMPDIR` への置き換えは、kconform だけが行う(モジュールの説明を参照)。
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

/// コマンドの行(ヒアドキュメントの内容を除く)が、`word` を単語として含むなら真を返す。
///
/// `word` に `diktor` を渡して、shim を通らずに本物の diktor を実行する呼び出しを探すために使う。
/// 単語の境界だけを調べる粗い判定で、引用符の中の `diktor` も数える。
/// シェルの関数を定義するだけのコマンドでは偽を返す。
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

/// コマンドが、シェルの関数を定義するだけのもの(`name() { … }`)なら真を返す。
/// 関数の本体の `diktor` は、関数を呼んだコマンドで実行されるので、定義のコマンドでは数えない。
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
    // `/` は単語の境界として扱う。`$DIR/diktor` のようにパスで呼ぶ(shim を通らない)呼び出しも見つけるためである。
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

/// 行がヒアドキュメントを始めるなら、その終端の語を返す。
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
        // エスケープシーケンスを先に取り除くので、その直後にあるスクリプトの位置も取り除ける。
        assert_eq!(s(b"\x1b[1m/w/meta/0.sh: line 2: y\n"), b"y\n");
    }

    #[test]
    fn パスは語の頭だけを_1_度置き換える() {
        let r = |o: &[u8]| rewrite_paths(o, b"/w/root", b"/w/tmp");
        // dune 3.21.1 と同じく、単語の途中に現れるパスは置き換えない。
        assert_eq!(r(b"/x/w/root/a"), b"/x/w/root/a");
        assert_eq!(r(b"/w/root/a/w/root"), b"$TESTCASE_ROOT/a/w/root");
        assert_eq!(r(b"/w/root/a /w/tmp/b"), b"$TESTCASE_ROOT/a $TMPDIR/b");
        // 単語は `/` から始まる。`x/w/root/a` のように、ほかの文字の直後にある `/` からも単語が始まるので、そこから置き換える。
        assert_eq!(r(b"x/w/root/a"), b"x$TESTCASE_ROOT/a");
        // `:` は単語を区切る。
        assert_eq!(r(b"/w/root/a:/w/tmp:x"), b"$TESTCASE_ROOT/a:$TMPDIR:x");
        // 単語の先頭は文字列の接頭辞として比較する。
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
