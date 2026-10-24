//! コーパス(cram のヒアドキュメントから抜き出した `.kel`)である(計画 261024-1 の 8.1 節)。
//!
//! `kconform extract-corpus` は、各 `.t` の `cat > NAME.kel <<'EOF'` の形のコマンドから中身を抜き出し、
//! `conformance/corpus/<試験の名前>/<番号>-<NAME>.kel` に置く。
//! 番号は `.t` の中で 1 から数え、NAME の `/` は `_` にする。
//! あわせて、各ファイルの出所と旗を `index.tsv` に書く。
//!
//! 旗は、同じ `.t` の後のコマンドのうち、そのファイルを引数に取る最初の `diktor` の呼び出しから取る
//! (`--no-prelude`、`--prelude P`、`--import-path D`)。
//! 次に同じ名前へ書くコマンドより後の呼び出しは見ない。
//! そのような呼び出しが無ければ、旗の欄を `?` にする。
//!
//! 引用符で囲まない終わりの語のヒアドキュメントは、シェルが中身を展開する。
//! 中身に `$`、`` ` ``、`\` が無ければそのまま抜き出し、あれば抜き出さずに数える。

use crate::cram::{self, Item};
use crate::runner::heredoc_terminator;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// コーパスの 1 項目である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// コーパスのディレクトリからの相対パスである。
    pub path: String,
    /// 出所(`試験.t:行`)である。
    pub source: String,
    /// 旗である。
    /// 旗を決められなかったときは `None` である。
    pub flags: Option<Vec<String>>,
}

/// 抜き出しの結果である。
#[derive(Debug, Default)]
pub struct Extracted {
    /// 項目と中身である。
    pub files: Vec<(Entry, Vec<u8>)>,
    /// 終わりの語が見つからなかったヒアドキュメントの数である。
    pub unterminated: usize,
    /// 展開を含むので抜き出さなかったヒアドキュメントの数である。
    pub expanding: usize,
}

/// `cat` のヒアドキュメントで `.kel` に書くコマンドなら、書く先のパスと、終わりの語と、引用されているかを返す。
fn kel_heredoc(first: &str) -> Option<(String, String, bool)> {
    let toks: Vec<&str> = first.split_whitespace().collect();
    if toks.first() != Some(&"cat") {
        return None;
    }
    let mut target = None;
    let mut i = 1;
    while i < toks.len() {
        let t = toks[i];
        if t == ">" {
            target = toks.get(i + 1).map(|s| s.to_string());
            i += 2;
        } else if let Some(p) = t.strip_prefix('>') {
            if p.starts_with('>') {
                return None;
            }
            target = Some(p.to_string());
            i += 1;
        } else if t.starts_with("<<") {
            if t == "<<" {
                i += 1;
            }
            i += 1;
        } else {
            return None;
        }
    }
    let target = target?;
    if !target.ends_with(".kel") {
        return None;
    }
    let term = heredoc_terminator(first)?;
    let rest = &first[first.find("<<")? + 2..];
    let quoted = rest.trim_start().starts_with(['\'', '"']);
    Some((target, term, quoted))
}

/// コマンドの行を、シェルの区切りで粗く語に分ける。
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// `cmd` が `path` を引数に取る `diktor` の呼び出しを含むなら、その旗を返す。
fn diktor_flags(cmd: &str, path: &str) -> Option<Vec<String>> {
    let ws = words(cmd);
    let mut i = 0;
    while i < ws.len() {
        if ws[i] == "diktor" || ws[i].ends_with("/diktor") {
            let mut flags = Vec::new();
            let mut j = i + 1;
            while j < ws.len() && ws[j] != "diktor" {
                match ws[j] {
                    "--no-prelude" => flags.push(ws[j].to_string()),
                    "--prelude" | "--import-path" if j + 1 < ws.len() => {
                        flags.push(ws[j].to_string());
                        flags.push(ws[j + 1].to_string());
                        j += 1;
                    }
                    w if w == path => return Some(flags),
                    _ => {}
                }
                j += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

fn sanitize(name: &str) -> String {
    name.replace('/', "_")
}

/// 1 つの `.t` から抜き出す。
fn extract_one(stem: &str, file_name: &str, items: &[Item], out: &mut Extracted) {
    let cmds = cram::commands(items);
    let mut seq = 0;
    for (k, c) in cmds.iter().enumerate() {
        let first = String::from_utf8_lossy(&c.text[0]).into_owned();
        let Some((target, term, quoted)) = kel_heredoc(&first) else {
            continue;
        };
        let body_lines = &c.text[1..];
        let Some(end) = body_lines.iter().position(|l| l == term.as_bytes()) else {
            out.unterminated += 1;
            continue;
        };
        let body = &body_lines[..end];
        if !quoted
            && body
                .iter()
                .any(|l| l.iter().any(|b| matches!(b, b'$' | b'`' | b'\\')))
        {
            out.expanding += 1;
            continue;
        }
        let mut content = Vec::new();
        for l in body {
            content.extend_from_slice(l);
            content.push(b'\n');
        }
        let mut flags = None;
        for later in &cmds[k + 1..] {
            let first = String::from_utf8_lossy(&later.text[0]).into_owned();
            if kel_heredoc(&first).is_some_and(|(t, _, _)| t == target) {
                break;
            }
            if let Some(f) = diktor_flags(&later.display(), &target) {
                flags = Some(f);
                break;
            }
        }
        seq += 1;
        let entry = Entry {
            path: format!("{stem}/{seq:04}-{}", sanitize(&target)),
            source: format!("{file_name}:{}", c.line),
            flags,
        };
        out.files.push((entry, content));
    }
}

/// `dir` の `*.t` のすべてから抜き出す(ファイルの名前の順)。
pub fn extract(dir: &Path) -> std::io::Result<Extracted> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "t"))
        .collect();
    names.sort();
    let mut out = Extracted::default();
    for p in names {
        let text = std::fs::read(&p)?;
        let file_name = p.file_name().unwrap().to_string_lossy().into_owned();
        let stem = file_name.trim_end_matches(".t").to_string();
        extract_one(&stem, &file_name, &cram::parse(&text), &mut out);
    }
    Ok(out)
}

/// 索引の 1 行目である。
pub const INDEX_HEADER: &str = "# path\tsource\tflags";

/// 索引の名前である。
pub const INDEX: &str = "index.tsv";

/// 索引の中身を作る。
pub fn index_text(entries: &[&Entry]) -> String {
    let mut s = String::from(INDEX_HEADER);
    s.push('\n');
    for e in entries {
        let flags = match &e.flags {
            Some(f) => f.join(" "),
            None => "?".to_string(),
        };
        s.push_str(&format!("{}\t{}\t{flags}\n", e.path, e.source));
    }
    s
}

/// 索引を読む。
pub fn read_index(corpus: &Path) -> Result<Vec<Entry>, String> {
    let p = corpus.join(INDEX);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut v = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        let parts: Vec<&str> = l.split('\t').collect();
        if parts.len() != 3 {
            return Err(format!(
                "{}:{}: 欄が 3 つではありません",
                p.display(),
                i + 1
            ));
        }
        let flags = if parts[2] == "?" {
            None
        } else {
            Some(parts[2].split_whitespace().map(str::to_string).collect())
        };
        v.push(Entry {
            path: parts[0].to_string(),
            source: parts[1].to_string(),
            flags,
        });
    }
    Ok(v)
}

/// コーパスのディレクトリにある、kconform が書くファイル(`*.kel` と索引)を集める。
fn existing(corpus: &Path) -> std::io::Result<BTreeMap<String, Vec<u8>>> {
    let mut m = BTreeMap::new();
    if !corpus.is_dir() {
        return Ok(m);
    }
    if corpus.join(INDEX).is_file() {
        m.insert(INDEX.to_string(), std::fs::read(corpus.join(INDEX))?);
    }
    for d in std::fs::read_dir(corpus)? {
        let d = d?.path();
        if !d.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&d)? {
            let f = f?.path();
            if f.is_file() && f.extension().is_some_and(|e| e == "kel") {
                let rel = f
                    .strip_prefix(corpus)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                m.insert(rel, std::fs::read(&f)?);
            }
        }
    }
    Ok(m)
}

/// 抜き出した結果を、コーパスのディレクトリの望ましい中身(相対パスから中身への表)にする。
pub fn desired(ex: &Extracted) -> BTreeMap<String, Vec<u8>> {
    let mut m = BTreeMap::new();
    let entries: Vec<&Entry> = ex.files.iter().map(|(e, _)| e).collect();
    m.insert(INDEX.to_string(), index_text(&entries).into_bytes());
    for (e, c) in &ex.files {
        m.insert(e.path.clone(), c.clone());
    }
    m
}

/// コーパスのディレクトリと望ましい中身の差(足りない、余る、中身が違う、の相対パス)を返す。
pub fn diff(corpus: &Path, want: &BTreeMap<String, Vec<u8>>) -> std::io::Result<Vec<String>> {
    let have = existing(corpus)?;
    let mut v = Vec::new();
    for (k, c) in want {
        match have.get(k) {
            None => v.push(format!("足りない: {k}")),
            Some(h) if h != c => v.push(format!("中身が違う: {k}")),
            _ => {}
        }
    }
    for k in have.keys() {
        if !want.contains_key(k) {
            v.push(format!("余る: {k}"));
        }
    }
    Ok(v)
}

/// コーパスのディレクトリを望ましい中身に書き換える。
/// kconform が書くファイル(`*.kel` と索引)のうち、望ましい中身に無いものは消す。
pub fn write(corpus: &Path, want: &BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    let have = existing(corpus)?;
    for k in have.keys() {
        if !want.contains_key(k) {
            std::fs::remove_file(corpus.join(k))?;
        }
    }
    for (k, c) in want {
        let p = corpus.join(k);
        if have.get(k) == Some(c) {
            continue;
        }
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, c)?;
    }
    // 空になったディレクトリを消す。
    for d in std::fs::read_dir(corpus)? {
        let d = d?.path();
        if d.is_dir() && std::fs::read_dir(&d)?.next().is_none() {
            std::fs::remove_dir(&d)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ヒアドキュメントの形() {
        assert_eq!(
            kel_heredoc("cat > a.kel <<'EOF'"),
            Some(("a.kel".into(), "EOF".into(), true))
        );
        assert_eq!(
            kel_heredoc("cat <<EOF > lib/M.kel"),
            Some(("lib/M.kel".into(), "EOF".into(), false))
        );
        assert_eq!(kel_heredoc("cat >> a.kel <<'EOF'"), None);
        assert_eq!(kel_heredoc("cat > a.txt <<'EOF'"), None);
        assert_eq!(kel_heredoc("cat a.kel"), None);
    }

    #[test]
    fn 後の呼び出しから旗を取る() {
        assert_eq!(
            diktor_flags("diktor --no-prelude --type-check a.kel", "a.kel"),
            Some(vec!["--no-prelude".to_string()])
        );
        assert_eq!(
            diktor_flags("diktor --import-path lib main.kel; echo $?", "main.kel"),
            Some(vec!["--import-path".to_string(), "lib".to_string()])
        );
        assert_eq!(diktor_flags("cat a.kel", "a.kel"), None);
        assert_eq!(diktor_flags("diktor b.kel", "a.kel"), None);
    }

    #[test]
    fn 抜き出しと索引() {
        let t = b"  $ cat > a.kel <<'EOF'\n  > let x = 1\n  > EOF\n  $ diktor --no-prelude a.kel\n  $ cat > b.kel <<EOF\n  > $x\n  > EOF\n  $ cat > c.kel <<EOF\n  > let y = 2\n  > EOF\n";
        let mut ex = Extracted::default();
        extract_one("t", "t.t", &cram::parse(t), &mut ex);
        assert_eq!(ex.files.len(), 2);
        assert_eq!(ex.expanding, 1);
        assert_eq!(ex.files[0].0.path, "t/0001-a.kel");
        assert_eq!(ex.files[0].0.source, "t.t:1");
        assert_eq!(ex.files[0].1, b"let x = 1\n");
        assert_eq!(ex.files[1].0.flags, None);
        let idx = index_text(&ex.files.iter().map(|(e, _)| e).collect::<Vec<_>>());
        assert!(idx.contains("t/0001-a.kel\tt.t:1\t--no-prelude\n"));
        assert!(idx.contains("t/0002-c.kel\tt.t:8\t?\n"));
    }
}
