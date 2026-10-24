//! 既知の差の一覧 `known-diffs.toml` である(計画 261024-1 の 8.1 節)。
//!
//! 各項目は、ファイル、コマンドの行、組、理由、台帳の番号を持つ。
//! 登録の無い差が 1 件でもあれば、kconform は失敗する。
//!
//! 読むのは TOML の次の部分だけである(依存の crate を増やさないため)。
//!
//! ```toml
//! # 注釈
//! [[diff]]
//! file = "diktor/test/run_exit.t"   # 試験のパスの末尾
//! line = 163                        # `  $ ` の行の番号
//! pairs = ["dk-ir", "ki"]           # 組の名前(`*` はすべての組)
//! reason = "限度は I(§15.5)"
//! ledger = "V213"
//! ```
//!
//! 文字列は二重引用符で囲み、`\"` と `\\` だけを解釈する。

use std::path::Path;

/// 既知の差の 1 項目である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 試験のパスの末尾(`/` 区切りの部分の列として照合する)である。
    pub file: String,
    /// コマンドの行の番号である。
    pub line: usize,
    /// 組の名前の列である。
    pub pairs: Vec<String>,
    /// 理由である。
    pub reason: String,
    /// 台帳の番号である。
    pub ledger: String,
    /// 一覧のファイルの中の行の番号(誤りの報告に使う)である。
    pub source_line: usize,
}

impl Entry {
    /// この項目が、`path` の `line` 行のコマンドの、組 `pair` の差に当たるかどうかを返す。
    pub fn matches(&self, path: &Path, line: usize, pair: &str) -> bool {
        self.line == line
            && self.pairs.iter().any(|p| p == "*" || p == pair)
            && path_ends_with(path, &self.file)
    }
}

/// `path` が `suffix` を、パスの部分の境目で終わるかどうかを返す。
fn path_ends_with(path: &Path, suffix: &str) -> bool {
    path.ends_with(Path::new(suffix))
}

fn parse_string(s: &str, n: usize) -> Result<(String, &str), String> {
    let s = s
        .strip_prefix('"')
        .ok_or_else(|| format!("{n} 行: 文字列は \" で始めてください"))?;
    let mut out = String::new();
    let mut it = s.char_indices();
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Ok((out, &s[i + 1..])),
            '\\' => match it.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                _ => return Err(format!("{n} 行: 扱えない逆斜線の書き方です")),
            },
            c => out.push(c),
        }
    }
    Err(format!("{n} 行: 文字列が閉じていません"))
}

fn strip_comment(rest: &str) -> &str {
    let t = rest.trim_start();
    if t.is_empty() || t.starts_with('#') {
        ""
    } else {
        t
    }
}

/// 値の後ろが空か注釈だけであることを確かめる。
fn end(rest: &str, n: usize) -> Result<(), String> {
    if strip_comment(rest).is_empty() {
        Ok(())
    } else {
        Err(format!("{n} 行: 値の後ろに余計なものがあります"))
    }
}

/// 一覧の中身を読む。
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    #[derive(Default)]
    struct Partial {
        file: Option<String>,
        line: Option<usize>,
        pairs: Option<Vec<String>>,
        reason: Option<String>,
        ledger: Option<String>,
        start: usize,
    }
    fn finish(p: Partial) -> Result<Entry, String> {
        let miss = |k: &str| format!("{} 行からの項目に {k} がありません", p.start);
        Ok(Entry {
            file: p.file.clone().ok_or_else(|| miss("file"))?,
            line: p.line.ok_or_else(|| miss("line"))?,
            pairs: p.pairs.clone().ok_or_else(|| miss("pairs"))?,
            reason: p.reason.clone().ok_or_else(|| miss("reason"))?,
            ledger: p.ledger.clone().ok_or_else(|| miss("ledger"))?,
            source_line: p.start,
        })
    }
    let mut entries = Vec::new();
    let mut cur: Option<Partial> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let l = raw.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l == "[[diff]]" {
            if let Some(p) = cur.take() {
                entries.push(finish(p)?);
            }
            cur = Some(Partial {
                start: n,
                ..Partial::default()
            });
            continue;
        }
        let p = cur
            .as_mut()
            .ok_or_else(|| format!("{n} 行: [[diff]] の前に値があります"))?;
        let (key, value) = l
            .split_once('=')
            .ok_or_else(|| format!("{n} 行: 「鍵 = 値」の形ではありません"))?;
        let key = key.trim();
        let value = value.trim_start();
        match key {
            "file" | "reason" | "ledger" => {
                let (s, rest) = parse_string(value, n)?;
                end(rest, n)?;
                let slot = match key {
                    "file" => &mut p.file,
                    "reason" => &mut p.reason,
                    _ => &mut p.ledger,
                };
                *slot = Some(s);
            }
            "line" => {
                let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
                end(&value[digits.len()..], n)?;
                p.line = Some(
                    digits
                        .parse()
                        .map_err(|_| format!("{n} 行: line は正の整数です"))?,
                );
            }
            "pairs" => {
                let mut rest = value
                    .strip_prefix('[')
                    .ok_or_else(|| format!("{n} 行: pairs は [ で始めてください"))?
                    .trim_start();
                let mut v = Vec::new();
                loop {
                    if let Some(r) = rest.strip_prefix(']') {
                        end(r, n)?;
                        break;
                    }
                    let (s, r) = parse_string(rest, n)?;
                    v.push(s);
                    rest = r.trim_start();
                    if let Some(r) = rest.strip_prefix(',') {
                        rest = r.trim_start();
                    } else if !rest.starts_with(']') {
                        return Err(format!("{n} 行: pairs の要素は , で区切ってください"));
                    }
                }
                p.pairs = Some(v);
            }
            _ => return Err(format!("{n} 行: 知らない鍵です: {key}")),
        }
    }
    if let Some(p) = cur.take() {
        entries.push(finish(p)?);
    }
    Ok(entries)
}

/// 一覧のファイルを読む。
/// ファイルが無ければ空の一覧である。
pub fn load(path: &Path) -> Result<Vec<Entry>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => parse(&t).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = r#"
# 注釈
[[diff]]
file = "diktor/test/run_exit.t"  # 末尾の注釈
line = 163
pairs = ["dk-ir", "ki"]
reason = "限度は \"I\""
ledger = "V213"

[[diff]]
file = "a.t"
line = 1
pairs = ["*"]
reason = "r"
ledger = "V1"
"#;

    #[test]
    fn 項目を読む() {
        let es = parse(T).unwrap();
        assert_eq!(es.len(), 2);
        assert_eq!(es[0].pairs, vec!["dk-ir", "ki"]);
        assert_eq!(es[0].reason, "限度は \"I\"");
        assert_eq!(es[0].source_line, 3);
        assert!(es[0].matches(Path::new("/r/diktor/test/run_exit.t"), 163, "ki"));
        assert!(!es[0].matches(Path::new("/r/diktor/test/run_exit.t"), 163, "dk"));
        assert!(!es[0].matches(Path::new("/r/xdiktor/test/run_exit.t"), 163, "ki"));
        assert!(es[1].matches(Path::new("/x/a.t"), 1, "dk"));
    }

    #[test]
    fn 欠けた鍵と知らない鍵は誤り() {
        assert!(parse("[[diff]]\nfile = \"a\"\n").is_err());
        assert!(parse("[[diff]]\nzz = 1\n").is_err());
        assert!(parse("file = \"a\"\n").is_err());
    }
}
