//! 結果の正規化である(計画 261024-1 の 8.1 節の最後)。
//!
//! 診断の本文は日本語の文面で、LangSpec §18 の比べる対象の外にある。
//! そこで、期待と結果の両方の行に同じ規則を当ててから比べる。
//!
//! - 診断の種別の語(`型エラー`、`構文エラー`、`字句エラー`、`実行時エラー`、`import エラー`、`未実装`)に `: ` が続く行は、その種別の語の後ろを切る。
//!   位置(`ファイル:行:桁`)と種別の語は残る。
//! - 警告の行(先頭が `⚠ `)は、本文を捨てて `⚠` だけを残す。
//!   行は消さないので、件数と、標準出力との前後関係が残る。
//!
//! 標準出力と標準エラーは cram と同じく 1 本にして読むので、前後関係はそのまま比べられる。

/// 種別の語である。
const KINDS: &[&str] = &[
    "型エラー",
    "構文エラー",
    "字句エラー",
    "実行時エラー",
    "import エラー",
    "未実装",
];

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// 1 行を正規化する。
/// 行は `.t` の形(先頭の 2 つの空白を含む)でも、中身だけでもよい。
pub fn line(l: &[u8]) -> Vec<u8> {
    let (indent, rest) = if l.starts_with(b"  ") {
        l.split_at(2)
    } else {
        l.split_at(0)
    };
    let warn = "⚠ ".as_bytes();
    if rest.starts_with(warn) {
        let mut v = indent.to_vec();
        v.extend_from_slice("⚠".as_bytes());
        return v;
    }
    let mut best: Option<(usize, usize)> = None;
    for k in KINDS {
        let mut pat = k.as_bytes().to_vec();
        pat.extend_from_slice(b": ");
        if let Some(i) = find(rest, &pat) {
            // 種別の語の直前は、行の先頭か区切り(空白、`:`)でなければならない。
            let ok = i == 0 || matches!(rest[i - 1], b' ' | b':');
            if ok && best.is_none_or(|(j, _)| i < j) {
                best = Some((i, k.len()));
            }
        }
    }
    match best {
        Some((i, n)) => {
            let mut v = indent.to_vec();
            v.extend_from_slice(&rest[..i + n]);
            v
        }
        None => l.to_vec(),
    }
}

/// 行の列を正規化する。
pub fn lines(ls: &[Vec<u8>]) -> Vec<Vec<u8>> {
    ls.iter().map(|l| line(l)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> String {
        String::from_utf8(line(s.as_bytes())).unwrap()
    }

    #[test]
    fn 種別の語の後ろを切る() {
        assert_eq!(
            n("  ! bad.kel:1:5: 型エラー: 注釈された型を満たしません"),
            "  ! bad.kel:1:5: 型エラー"
        );
        assert_eq!(
            n("  実行時エラー: 未実装のプリミティブ: nosuch"),
            "  実行時エラー"
        );
        assert_eq!(n("a.kel:2:1: 構文エラー: x"), "a.kel:2:1: 構文エラー");
    }

    #[test]
    fn 警告は印だけを残す() {
        assert_eq!(n("  ⚠ match が非網羅的です"), "  ⚠");
    }

    #[test]
    fn 診断でない行は変えない() {
        assert_eq!(n("  [1, 2]"), "  [1, 2]");
        assert_eq!(n("  x型エラー: y"), "  x型エラー: y");
    }
}
