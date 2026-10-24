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
//! when = "unimplemented"            # 省略できる。下を見よ
//! against = "record"                # 省略できる。下を見よ
//! ```
//!
//! `when` は項目が当たる条件を狭める(計画 8.1 節の項目の形に足した鍵である)。
//! `when = "unimplemented"` の項目は、差のある組の結果が `keleut: 未実装` の行を含み、
//! その行より前の行が比べる相手の同じ数の先頭の行と(正規化の後で)一致するときだけ当たる。
//! 評価が未実装の機能に届いて 4 で終わるブロックを一時的に登録するときに使う。
//! 未実装に届く前の出力の違いと、未実装に届かなくなったブロックの差は隠さない。
//!
//! `against` は、項目が許す比較を決める(これも計画 8.1 節の項目の形に足した鍵である)。
//! `against = "record"` の項目は `.t` に記録した期待との差だけに当たり、
//! `against = "pair"` の項目は基準の組との差だけに当たる。
//! 省くと両方に当たる。
//! 記録が基準の組の結果と同じ試験(diktor/test)では、2 つの比較の差は同じものなので省いてよい。
//! 記録が RustKeleut の結果である試験(`conformance/bytes/` など)では、記録と違う組と基準の組と違う組が別なので、分けて書く。
//! `kconform expect` は記録とだけ比べるので、`against = "pair"` の項目を使わない。
//!
//! 読んだ後に [`validate`] で、組の名前、台帳の番号の形、項目の重なりを確かめる。
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
    /// 当たる条件である(`when` の鍵)。
    pub when: When,
    /// 許す比較である(`against` の鍵)。
    pub against: Against,
    /// 一覧のファイルの中の行の番号(誤りの報告に使う)である。
    pub source_line: usize,
    /// 一覧のファイルのパス(誤りの報告に使う。`parse` では空)である。
    pub origin: String,
}

/// 項目が当たる条件である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum When {
    /// ファイル、行、組が合えば当たる。
    #[default]
    Always,
    /// そのうえ、差のある組の結果が `keleut: 未実装` の行を含み、
    /// その行より前が比べる相手の先頭と一致するときだけ当たる。
    Unimplemented,
}

/// 項目が許す比較である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Against {
    /// 記録との差と、基準の組との差の両方に当たる。
    #[default]
    Both,
    /// `.t` に記録した期待との差だけに当たる。
    Record,
    /// 基準の組との差だけに当たる。
    Pair,
}

/// 差を見つけた比較の種類である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// `.t` に記録した期待との比較である。
    Record,
    /// 基準の組との比較である。
    Pair,
    /// RustKeleut の後段の組(`ki kj kt ka km kw`)の間の比較である。
    ///
    /// 後段の間は既知の差の一覧に関係なく完全に一致しなければならない(計画 8.2 節、D499)ので、
    /// この比較の差を許すのは、次の一時の例外だけである。
    /// `when = "unimplemented"` の項目が差のある組を名前で挙げ(`*` では当たらない)、
    /// `against` が `"record"` でなく、差のある組の結果が `keleut: 未実装` の行を含み、
    /// その行より前が基準の後段の組の結果の先頭と正規化をせずに一致するときである。
    /// 計画の M94 で、後段 `kw` が G の節を持つプログラムを 4 で止める一時の登録のための例外であり、M95 で消す。
    Backend,
}

impl Against {
    /// この値の項目が、比較 `c` の差を許すかどうかを返す。
    pub fn allows(self, c: Comparison) -> bool {
        matches!(
            (self, c),
            (
                Against::Both,
                Comparison::Record | Comparison::Pair | Comparison::Backend
            ) | (Against::Record, Comparison::Record)
                | (Against::Pair, Comparison::Pair | Comparison::Backend)
        )
    }
}

/// 未実装の機能に届いたときの、RustKeleut の診断の頭である。
const UNIMPLEMENTED: &[u8] = "keleut: 未実装".as_bytes();

impl Entry {
    /// この項目が、`path` の `line` 行のコマンドの、組 `pair` の比較 `cmp` の差に当たるかどうかを返す。
    ///
    /// `want` は比べる相手(記録か基準の組)の行、`got` は差のある組の結果の行(どちらも `.t` の形)である。
    pub fn matches(
        &self,
        path: &Path,
        line: usize,
        pair: &str,
        cmp: Comparison,
        want: &[Vec<u8>],
        got: &[Vec<u8>],
    ) -> bool {
        let backend = cmp == Comparison::Backend;
        self.line == line
            && self.against.allows(cmp)
            && (!backend || self.when == When::Unimplemented)
            && self
                .pairs
                .iter()
                .any(|p| p == pair || (p == "*" && !backend))
            && path_ends_with(path, &self.file)
            && match self.when {
                When::Always => true,
                When::Unimplemented => unimplemented_after_same_prefix(want, got, !backend),
            }
    }
}

/// `got` が `keleut: 未実装` の行を含み、その行より前の行が `want` の先頭と一致するかを返す。
///
/// `normalize` が真なら正規化の後で比べ、偽なら(後段の間の比較)そのまま比べる。
fn unimplemented_after_same_prefix(want: &[Vec<u8>], got: &[Vec<u8>], normalize: bool) -> bool {
    let Some(i) = got
        .iter()
        .position(|l| l.windows(UNIMPLEMENTED.len()).any(|w| w == UNIMPLEMENTED))
    else {
        return false;
    };
    want.len() >= i
        && if normalize {
            crate::normalize::lines(&got[..i]) == crate::normalize::lines(&want[..i])
        } else {
            got[..i] == want[..i]
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
/// ファイルが無ければ空の一覧である。
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

/// 組の名前として書けるものかどうかを返す。
///
/// `*`、[`crate::pairs::NAMES`] の名前、`kr-` で始まる名前(計画 8.2 節の組 `kr-*`)を受け入れる。
pub fn is_pair_name(name: &str) -> bool {
    name == "*"
        || crate::pairs::NAMES.contains(&name)
        || name.strip_prefix("kr-").is_some_and(|r| !r.is_empty())
}

/// 台帳の番号の形(`V` に続く 10 進の数字)かどうかを返す。
fn is_ledger(s: &str) -> bool {
    s.strip_prefix('V')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// 読んだすべての一覧の項目を確かめ、誤りの文の列を返す(空なら誤りは無い)。
///
/// 組の名前が [`is_pair_name`] でないもの、台帳の番号が `V` と数字でないもの、
/// ファイル、行、組、`when` が同じで、許す比較が重なる項目が 2 つ以上あるもの(ファイルをまたいでも)を誤りとする。
/// 許す比較が重ならないのは、`against = "record"` と `against = "pair"` の 2 つだけである。
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
        // 未実装に届く前の出力が違えば当たらない。
        let early = vec![l("  z"), l("  keleut: 未実装: handle"), l("  [4]")];
        assert!(!es[0].matches(p, 3, "ki", r, &want, &early));
        // 未実装に届かなければ当たらない。
        assert!(!es[0].matches(p, 3, "ki", r, &want, &[l("  x"), l("  [4]")]));
        assert!(parse("[[diff]]\nwhen = \"zz\"\n").is_err());
    }

    #[test]
    fn 後段の間の差を許すのは未実装の一時の項目だけである() {
        let one = |pairs: &str, extra: &str| {
            parse(&format!(
                "[[diff]]\nfile = \"a.t\"\nline = 3\npairs = [{pairs}]\nreason = \"r\"\nledger = \"V1\"\nwhen = \"unimplemented\"\n{extra}"
            ))
            .unwrap()
            .remove(0)
        };
        let p = Path::new("/x/a.t");
        let l = |s: &str| s.as_bytes().to_vec();
        let b = Comparison::Backend;
        let base = vec![l("  x"), l("  y")];
        let hit = vec![l("  x"), l("  keleut: 未実装: G"), l("  [4]")];
        let kw = one("\"kw\"", "");
        assert!(kw.matches(p, 3, "kw", b, &base, &hit));
        assert!(one("\"kw\"", "against = \"pair\"\n").matches(p, 3, "kw", b, &base, &hit));
        // 記録とだけ比べる項目、`*` の項目、ほかの組の項目は当たらない。
        assert!(!one("\"kw\"", "against = \"record\"\n").matches(p, 3, "kw", b, &base, &hit));
        assert!(!one("\"*\"", "").matches(p, 3, "kw", b, &base, &hit));
        assert!(!kw.matches(p, 3, "kj", b, &base, &hit));
        // 未実装より前は、基準の後段の組と正規化をせずに一致しなければならない。
        let spaced = vec![l("  x "), l("  keleut: 未実装: G"), l("  [4]")];
        assert!(!kw.matches(p, 3, "kw", b, &base, &spaced));
        assert!(!kw.matches(p, 3, "kw", b, &base, &[l("  x"), l("  [4]")]));
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
        // 後段の間の差は、`when = "unimplemented"` でない項目には当たらない(D499)。
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
        // 同じファイル、行、組の項目は、許す比較が重なれば誤りである。
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
