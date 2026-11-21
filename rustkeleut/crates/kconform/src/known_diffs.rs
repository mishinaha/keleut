//! 既知の差分の一覧 `known-diffs.toml` を読む。
//!
//! 各項目は、ファイル、コマンドの行、ペア、理由、台帳の番号を持つ。
//! 未登録の差分が 1 件でもあれば、kconform は失敗する。
//!
//! 依存する crate を増やさないように、TOML のうち次の形式だけを自前で読む。
//!
//! ```toml
//! # 注釈
//! [[diff]]
//! file = "diktor/test/run_exit.t"   # 試験のパスの末尾
//! line = 163                        # `  $ ` の行の番号
//! pairs = ["dk-ir", "ki"]           # 組の名前(`*` はすべての組)
//! reason = "限度は I(§15.5)"
//! ledger = "V213"
//! when = "unimplemented"            # 省略できる。下を見よ
//! against = "record"                # 省略できる。下を見よ
//! ```
//!
//! キー `when` は、項目が該当する条件を狭める。
//! `when = "unimplemented"` の項目は、差分のあるペアの結果が `keleut: 未実装` の行を含み、
//! その行より前の行が、比較対象の先頭から同じ行数だけ取った行と、正規化した後で一致するときだけ該当する。
//! 評価が未実装の機能に達して終了コード 4 で終了するブロックを、一時的に登録するときに使う。
//! 未実装の機能に達する前の出力の違いと、未実装の機能に達しなくなったブロックの差分は、この項目では隠れない。
//!
//! キー `against` は、項目がどの比較の差分を許すかを決める。
//! `against = "record"` の項目は `.t` に書いた期待出力との差分だけに該当し、
//! `against = "pair"` の項目は基準のペアとの差分だけに該当する。
//! 省略すると両方に該当する。
//! 期待出力が基準のペアの結果と同じテスト(`diktor/test`)では、2 つの比較の差分は同じものなので、`against` を省略してよい。
//! 期待出力が RustKeleut の結果であるテスト(`conformance/bytes/` など)では、結果が期待出力と違うペアと、結果が基準のペアと違うペアが同じとは限らないので、項目を分けて書く。
//! `kconform expect` は期待出力とだけ比較するので、`against = "pair"` の項目を使わない。
//!
//! 読んだ後に [`validate`] で、ペアの名前、台帳の番号の形式、項目の重なりを検査する。
//!
//! 文字列は二重引用符で囲み、`\"` と `\\` だけを解釈する。

use std::path::Path;

/// 既知の差分の 1 項目である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// テストのファイル `.t` のパスの末尾である。
    /// 実際のパスとは、`/` で区切った要素の単位で比較する。
    pub file: String,
    /// コマンドの `  $ ` の行の行番号である。
    pub line: usize,
    /// この項目を適用するペアの名前の並びである。
    /// `*` はすべてのペアを表す。
    pub pairs: Vec<String>,
    /// この差分を許す理由である。
    pub reason: String,
    /// 台帳の番号(`V213` の形式)である。
    pub ledger: String,
    /// 項目が該当する条件(キー `when`)である。
    pub when: When,
    /// 項目が許す比較(キー `against`)である。
    pub against: Against,
    /// 一覧のファイルの中で、この項目が始まる行の行番号である。
    /// エラーの報告に使う。
    pub source_line: usize,
    /// 一覧のファイルのパスである。
    /// エラーの報告に使う。
    /// [`parse`] は空にし、[`load`] が設定する。
    pub origin: String,
}

/// 項目が該当する条件である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum When {
    /// ファイル、行、ペアが一致すれば該当する。
    #[default]
    Always,
    /// ファイル、行、ペアが一致し、さらに、差分のあるペアの結果が `keleut: 未実装` の行を含み、
    /// その行より前が比較対象の先頭と一致するときだけ該当する。
    Unimplemented,
}

/// 項目が許す比較である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Against {
    /// 期待出力との差分と、基準のペアとの差分の両方に該当する。
    #[default]
    Both,
    /// `.t` に書いた期待出力との差分だけに該当する。
    Record,
    /// 基準のペアとの差分だけに該当する。
    Pair,
}

/// 差分を見つけた比較の種類である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// `.t` に書いた期待出力との比較である。
    Record,
    /// 基準のペアとの比較である。
    Pair,
    /// RustKeleut のバックエンドのペアどうしの比較である。
    /// ペアのグループは 2 つある。
    /// Diktor が書いた KIR を実行する `ki`、`kj`、`kt`、`ka`、`km`、`kw` と、Rust のフロントエンドが書いた KIR を実行する `kr-*` である。
    /// 比較はグループの中だけで行い、`ki` と `kr-ki` のようにグループの違うペアどうしは比較しない。
    ///
    /// バックエンドどうしの結果は、既知の差分の一覧に関係なく完全に一致しなければならないので、
    /// この比較の差分はどの項目にも該当しない。
    Backend,
}

impl Against {
    /// この値を持つ項目が、比較 `c` の差分を許すかどうかを返す。
    /// バックエンドのペアどうしの比較の差分は、どの値でも許さない。
    pub fn allows(self, c: Comparison) -> bool {
        matches!(
            (self, c),
            (Against::Both, Comparison::Record | Comparison::Pair)
                | (Against::Record, Comparison::Record)
                | (Against::Pair, Comparison::Pair)
        )
    }
}

/// 未実装の機能に達したときに RustKeleut が出力する診断の先頭部分である。
const UNIMPLEMENTED: &[u8] = "keleut: 未実装".as_bytes();

impl Entry {
    /// `path` の `line` 行のコマンドでペア `pair` に比較 `cmp` の差分があるとき、この項目がその差分に該当するかどうかを返す。
    ///
    /// `want` は比較対象(期待出力か、基準のペアの結果)の行で、`got` は差分のあるペアの結果の行である。
    /// どちらも `.t` の期待出力と同じ形式で渡す。
    pub fn matches(
        &self,
        path: &Path,
        line: usize,
        pair: &str,
        cmp: Comparison,
        want: &[Vec<u8>],
        got: &[Vec<u8>],
    ) -> bool {
        self.line == line
            && self.against.allows(cmp)
            && self.pairs.iter().any(|p| p == pair || p == "*")
            && path_ends_with(path, &self.file)
            && match self.when {
                When::Always => true,
                When::Unimplemented => unimplemented_after_same_prefix(want, got),
            }
    }
}

/// `got` が `keleut: 未実装` の行を含み、その行より前の行が、`want` の先頭から同じ行数だけ取った行と、正規化した後で一致すれば真を返す。
fn unimplemented_after_same_prefix(want: &[Vec<u8>], got: &[Vec<u8>]) -> bool {
    let Some(i) = got
        .iter()
        .position(|l| l.windows(UNIMPLEMENTED.len()).any(|w| w == UNIMPLEMENTED))
    else {
        return false;
    };
    want.len() >= i && crate::normalize::lines(&got[..i]) == crate::normalize::lines(&want[..i])
}

/// `path` の末尾が、パスの要素の単位で `suffix` と一致するかどうかを返す。
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

/// 値の後ろが空かコメントだけであることを検査する。
fn end(rest: &str, n: usize) -> Result<(), String> {
    if strip_comment(rest).is_empty() {
        Ok(())
    } else {
        Err(format!("{n} 行: 値の後ろに余計なものがあります"))
    }
}

/// 一覧の内容を読み、項目の並びを返す。
/// 書式のエラーがあれば、行番号を付けたメッセージを返す。
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    #[derive(Default)]
    struct Partial {
        file: Option<String>,
        line: Option<usize>,
        pairs: Option<Vec<String>>,
        reason: Option<String>,
        ledger: Option<String>,
        when: When,
        against: Against,
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
            when: p.when,
            against: p.against,
            source_line: p.start,
            origin: String::new(),
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
            "when" => {
                let (s, rest) = parse_string(value, n)?;
                end(rest, n)?;
                p.when = match s.as_str() {
                    "unimplemented" => When::Unimplemented,
                    _ => return Err(format!("{n} 行: when の値は \"unimplemented\" だけです")),
                };
            }
            "against" => {
                let (s, rest) = parse_string(value, n)?;
                end(rest, n)?;
                p.against = match s.as_str() {
                    "record" => Against::Record,
                    "pair" => Against::Pair,
                    _ => {
                        return Err(format!(
                            "{n} 行: against の値は \"record\" か \"pair\" です"
                        ));
                    }
                };
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
/// ファイルがなければ、空の一覧を返す。
pub fn load(path: &Path) -> Result<Vec<Entry>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => {
            let mut es = parse(&t).map_err(|e| format!("{}: {e}", path.display()))?;
            for e in &mut es {
                e.origin = path.display().to_string();
            }
            Ok(es)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// `name` が、項目の `pairs` に書けるペアの名前かどうかを返す。
///
/// `*`、[`crate::pairs::NAMES`] の名前、`kr-` の後に 1 文字以上が続く名前を受理する。
pub fn is_pair_name(name: &str) -> bool {
    name == "*"
        || crate::pairs::NAMES.contains(&name)
        || name.strip_prefix("kr-").is_some_and(|r| !r.is_empty())
}

/// 台帳の番号の形式(`V` の後に 1 桁以上の 10 進の数字)かどうかを返す。
fn is_ledger(s: &str) -> bool {
    s.strip_prefix('V')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// 読んだすべての一覧の項目を検査し、エラーメッセージの並びを返す。
/// 空ならエラーはない。
///
/// 次の項目をエラーにする。
///
/// - ペアの名前が [`is_pair_name`] を満たさない項目。
/// - `pairs` が空の項目。
/// - 台帳の番号が `V` と数字の形式でない項目。
/// - ファイル、行、ペア、`when` が同じで、許す比較が重なる 2 つ以上の項目。別の一覧のファイルにある項目どうしも調べる。
///
/// 許す比較が重ならないのは、`against = "record"` と `against = "pair"` の組み合わせだけである。
pub fn validate(entries: &[Entry]) -> Vec<String> {
    let mut errs = Vec::new();
    type Key<'a> = (&'a str, usize, &'a str, When);
    let mut seen: std::collections::HashMap<Key, Vec<&Entry>> = std::collections::HashMap::new();
    let overlap = |a: Against, b: Against| a == Against::Both || b == Against::Both || a == b;
    for e in entries {
        let at = format!(
            "{} の {} 行の項目 {}:{}",
            e.origin, e.source_line, e.file, e.line
        );
        if !is_ledger(&e.ledger) {
            errs.push(format!(
                "{at}: 台帳の番号の形ではありません: {:?}",
                e.ledger
            ));
        }
        if e.pairs.is_empty() {
            errs.push(format!("{at}: pairs が空です"));
        }
        for p in &e.pairs {
            if !is_pair_name(p) {
                errs.push(format!("{at}: 知らない組です: {p}"));
                continue;
            }
            let prevs = seen.entry((&e.file, e.line, p, e.when)).or_default();
            if let Some(prev) = prevs.iter().find(|q| overlap(q.against, e.against)) {
                errs.push(format!(
                    "{at}: 組 {p} が {} の {} 行の項目と重なっています",
                    prev.origin, prev.source_line
                ));
            }
            prevs.push(e);
        }
    }
    errs
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
        let p = Path::new("/r/diktor/test/run_exit.t");
        let r = Comparison::Record;
        assert!(es[0].matches(p, 163, "ki", r, &[], &[]));
        assert!(es[0].matches(p, 163, "ki", Comparison::Pair, &[], &[]));
        assert!(!es[0].matches(p, 163, "dk", r, &[], &[]));
        assert!(!es[0].matches(
            Path::new("/r/xdiktor/test/run_exit.t"),
            163,
            "ki",
            r,
            &[],
            &[]
        ));
        assert!(es[1].matches(Path::new("/x/a.t"), 1, "dk", r, &[], &[]));
        assert_eq!(es[0].when, When::Always);
        assert_eq!(es[0].against, Against::Both);
    }

    #[test]
    fn 未実装の条件は結果の行を見る() {
        let es = parse(
            "[[diff]]\nfile = \"a.t\"\nline = 3\npairs = [\"ki\"]\nreason = \"r\"\nledger = \"V1\"\nwhen = \"unimplemented\"\n",
        )
        .unwrap();
        let p = Path::new("/x/a.t");
        let l = |s: &str| s.as_bytes().to_vec();
        let want = vec![l("  x"), l("  y")];
        let hit = vec![l("  x"), l("  keleut: 未実装: handle"), l("  [4]")];
        let r = Comparison::Record;
        assert!(es[0].matches(p, 3, "ki", r, &want, &hit));
        // 未実装の機能に達する前の出力が違えば該当しない。
        let early = vec![l("  z"), l("  keleut: 未実装: handle"), l("  [4]")];
        assert!(!es[0].matches(p, 3, "ki", r, &want, &early));
        // 未実装の機能に達しなければ該当しない。
        assert!(!es[0].matches(p, 3, "ki", r, &want, &[l("  x"), l("  [4]")]));
        assert!(parse("[[diff]]\nwhen = \"zz\"\n").is_err());
    }

    #[test]
    fn 後段の間の差はどの項目にも当たらない() {
        let one = |pairs: &str, extra: &str| {
            parse(&format!(
                "[[diff]]\nfile = \"a.t\"\nline = 3\npairs = [{pairs}]\nreason = \"r\"\nledger = \"V1\"\n{extra}"
            ))
            .unwrap()
            .remove(0)
        };
        let p = Path::new("/x/a.t");
        let l = |s: &str| s.as_bytes().to_vec();
        let b = Comparison::Backend;
        let base = vec![l("  x"), l("  y")];
        let hit = vec![l("  x"), l("  keleut: 未実装: G"), l("  [4]")];
        for e in [
            one("\"kw\"", "when = \"unimplemented\"\n"),
            one("\"kw\"", "when = \"unimplemented\"\nagainst = \"pair\"\n"),
            one("\"kw\"", ""),
            one("\"*\"", ""),
        ] {
            assert!(!e.matches(p, 3, "kw", b, &base, &hit));
            assert!(!e.matches(p, 3, "kw", b, &base, &base));
        }
        // 同じ項目は、期待出力との差分と基準のペアとの差分には該当する。
        let kw = one("\"kw\"", "when = \"unimplemented\"\n");
        assert!(kw.matches(p, 3, "kw", Comparison::Record, &base, &hit));
        assert!(kw.matches(p, 3, "kw", Comparison::Pair, &base, &hit));
    }

    #[test]
    fn 許す比較を分ける() {
        let one = |against: &str| {
            parse(&format!(
                "[[diff]]\nfile = \"a.t\"\nline = 3\npairs = [\"ki\"]\nreason = \"r\"\nledger = \"V1\"\nagainst = \"{against}\"\n"
            ))
        };
        let p = Path::new("/x/a.t");
        let rec = one("record").unwrap();
        assert!(rec[0].matches(p, 3, "ki", Comparison::Record, &[], &[]));
        assert!(!rec[0].matches(p, 3, "ki", Comparison::Pair, &[], &[]));
        let pair = one("pair").unwrap();
        assert!(!pair[0].matches(p, 3, "ki", Comparison::Record, &[], &[]));
        assert!(pair[0].matches(p, 3, "ki", Comparison::Pair, &[], &[]));
        // バックエンドのペアどうしの差分は、`when` のない項目にも該当しない。
        let both = parse(
            "[[diff]]\nfile = \"a.t\"\nline = 3\npairs = [\"*\"]\nreason = \"r\"\nledger = \"V1\"\n",
        )
        .unwrap();
        assert!(both[0].matches(p, 3, "ki", Comparison::Pair, &[], &[]));
        for e in [&rec[0], &pair[0], &both[0]] {
            assert!(!e.matches(p, 3, "ki", Comparison::Backend, &[], &[]));
        }
        assert!(one("zz").is_err());
    }

    #[test]
    fn 組の名前と台帳の番号と重なりを確かめる() {
        let e = |pairs: &str, ledger: &str, against: &str| {
            let mut es = parse(&format!(
                "[[diff]]\nfile = \"a.t\"\nline = 3\npairs = [{pairs}]\nreason = \"r\"\nledger = \"{ledger}\"\n{against}"
            ))
            .unwrap();
            es[0].origin = "k.toml".to_string();
            es.remove(0)
        };
        assert!(validate(&[e("\"ki\", \"*\", \"kr-c\"", "V12", "")]).is_empty());
        assert_eq!(validate(&[e("\"kii\"", "V1", "")]).len(), 1);
        assert_eq!(validate(&[e("\"kr-\"", "V1", "")]).len(), 1);
        assert_eq!(validate(&[e("\"ki\"", "", "")]).len(), 1);
        assert_eq!(validate(&[e("\"ki\"", "zz", "")]).len(), 1);
        assert_eq!(validate(&[e("", "V1", "")]).len(), 1);
        // ファイル、行、ペアが同じ項目は、許す比較が重なればエラーである。
        let both = e("\"ki\", \"dk\"", "V1", "");
        let rec = e("\"ki\"", "V2", "against = \"record\"\n");
        let pair = e("\"ki\"", "V2", "against = \"pair\"\n");
        assert_eq!(validate(&[both.clone(), both.clone()]).len(), 2);
        assert_eq!(validate(&[both, pair.clone()]).len(), 1);
        assert!(validate(&[rec, pair]).is_empty());
    }

    #[test]
    fn 欠けた鍵と知らない鍵は誤り() {
        assert!(parse("[[diff]]\nfile = \"a\"\n").is_err());
        assert!(parse("[[diff]]\nzz = 1\n").is_err());
        assert!(parse("file = \"a\"\n").is_err());
    }
}
