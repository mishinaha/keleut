//! cram の `.t` の読み取りと、結果の組み立てである。
//!
//! 読み方は dune の cram と同じにする。
//!
//! - `  $ ` で始まる行がコマンドの始まりである。
//!   `  $` だけの行はコマンドでなく、出力の行である(dune と同じ)。
//! - 直後に続く `  > ` の行(または `  >` だけの行)が、同じコマンドの続きの行である。
//! - その後に続く、`  ` で始まり `  $ ` で始まらない行が、期待の出力である。
//!   0 でない終了状態は、期待の出力の最後の `[N]` の行で表す。
//! - それ以外の行は注釈である。
//!
//! 行はバイト列のまま扱う。
//! `.t` にも出力にも、正しくない UTF-8 が現れうるからである。

/// `.t` の 1 つの項目である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// 注釈の行である。
    Comment(Vec<u8>),
    /// コマンドとその期待の出力である。
    Command(Command),
}

/// cram の 1 つのコマンドである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// `  $ ` の行の番号(1 から数える)である。
    pub line: usize,
    /// コマンドの行の列である。
    /// 先頭が `  $ ` の行の中身で、残りが `  > ` の続きの行の中身である。
    pub text: Vec<Vec<u8>>,
    /// 期待の出力の行の列である。
    /// 先頭の 2 つの空白を含めた、`.t` の行そのままである。
    pub expected: Vec<Vec<u8>>,
}

impl Command {
    /// コマンドを、シェルに渡す形(各行の終わりに改行)にする。
    pub fn script(&self) -> Vec<u8> {
        let mut s = Vec::new();
        for l in &self.text {
            s.extend_from_slice(l);
            s.push(b'\n');
        }
        s
    }

    /// コマンドの行の全体を、行の間に空白を入れた 1 つの文字列にする(表示と検索に使う)。
    pub fn display(&self) -> String {
        let parts: Vec<String> = self
            .text
            .iter()
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect();
        parts.join(" ")
    }
}

fn is_command_start(l: &[u8]) -> bool {
    l.starts_with(b"  $ ")
}

fn is_continuation(l: &[u8]) -> bool {
    l.starts_with(b"  > ") || l == b"  >"
}

/// `prefix` の後ろの中身を取り出す(`  >` だけの行は空の中身になる)。
fn body(l: &[u8]) -> Vec<u8> {
    if l.len() >= 4 {
        l[4..].to_vec()
    } else {
        Vec::new()
    }
}

/// `.t` の中身を項目の列に分ける。
pub fn parse(text: &[u8]) -> Vec<Item> {
    let mut lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if is_command_start(l) {
            let line = i + 1;
            let mut text = vec![body(l)];
            i += 1;
            while i < lines.len() && is_continuation(lines[i]) {
                text.push(body(lines[i]));
                i += 1;
            }
            let mut expected = Vec::new();
            while i < lines.len() && lines[i].starts_with(b"  ") && !is_command_start(lines[i]) {
                expected.push(lines[i].to_vec());
                i += 1;
            }
            items.push(Item::Command(Command {
                line,
                text,
                expected,
            }));
        } else {
            items.push(Item::Comment(l.to_vec()));
            i += 1;
        }
    }
    items
}

/// コマンドの列だけを取り出す。
pub fn commands(items: &[Item]) -> Vec<&Command> {
    items
        .iter()
        .filter_map(|it| match it {
            Item::Command(c) => Some(c),
            Item::Comment(_) => None,
        })
        .collect()
}

/// 1 つのコマンドを走らせた結果である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 走った。
    /// 出力は、標準出力と標準エラーを 1 本にしたもので、作業ディレクトリのパスを置き換えた後のものである。
    Ran { output: Vec<u8>, code: i32 },
    /// 前のコマンドが `exit` などでシェルを終えたので、走らなかった。
    Unreachable,
}

/// 結果を、`.t` の期待の出力の形の行の列にする。
///
/// dune の cram と同じく、出力の最後の改行の有無は区別しない。
/// 改行で終わる行は、改行の直前の CR を 1 つだけ落とす(dune が使う `split_lines` と同じ)。
/// 行の途中の CR と、改行の無い最後の部分の末尾の CR は残す。
/// 0 でない終了状態は `[N]` の行にする。
pub fn render(outcome: &Outcome) -> Vec<Vec<u8>> {
    match outcome {
        Outcome::Unreachable => vec![b"  ***** UNREACHABLE *****".to_vec()],
        Outcome::Ran { output, code } => {
            let mut v = Vec::new();
            if !output.is_empty() {
                let mut parts: Vec<&[u8]> = output.split(|&b| b == b'\n').collect();
                if output.ends_with(b"\n") {
                    parts.pop();
                }
                let n = parts.len();
                for (k, p) in parts.into_iter().enumerate() {
                    // 最後の部分は、出力が改行で終わるときだけ改行で終わった行である。
                    let ended_by_lf = k + 1 < n || output.ends_with(b"\n");
                    let p = match p.strip_suffix(b"\r") {
                        Some(q) if ended_by_lf => q,
                        _ => p,
                    };
                    let mut l = b"  ".to_vec();
                    l.extend_from_slice(p);
                    v.push(l);
                }
            }
            if *code != 0 {
                v.push(format!("  [{code}]").into_bytes());
            }
            v
        }
    }
}

/// 項目の列と各コマンドの結果から、期待を書き直した `.t` の中身を作る(dune の `.t.corrected` に当たる)。
pub fn corrected(items: &[Item], outcomes: &[Outcome]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut k = 0;
    for it in items {
        match it {
            Item::Comment(l) => {
                out.extend_from_slice(l);
                out.push(b'\n');
            }
            Item::Command(c) => {
                for (j, l) in c.text.iter().enumerate() {
                    let mark: &[u8] = match (j == 0, l.is_empty()) {
                        (true, _) => b"  $ ",
                        (false, true) => b"  >",
                        (false, false) => b"  > ",
                    };
                    out.extend_from_slice(mark);
                    out.extend_from_slice(l);
                    out.push(b'\n');
                }
                for l in render(&outcomes[k]) {
                    out.extend_from_slice(&l);
                    out.push(b'\n');
                }
                k += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"comment\n  $ cat > a.kel <<'EOF'\n  > let x = 1\n  > EOF\n  $ diktor a.kel\n  out\n  \n  [1]\nnext\n  $ \n  $\n";

    #[test]
    fn コマンドと続きの行と期待を分ける() {
        let items = parse(SAMPLE);
        let cs = commands(&items);
        assert_eq!(cs.len(), 3);
        assert_eq!(cs[0].line, 2);
        assert_eq!(cs[0].text.len(), 3);
        assert!(cs[0].expected.is_empty());
        assert_eq!(cs[1].line, 5);
        assert_eq!(
            cs[1].expected,
            vec![b"  out".to_vec(), b"  ".to_vec(), b"  [1]".to_vec()]
        );
        assert_eq!(cs[2].text, vec![Vec::<u8>::new()]);
        // `  $` だけの行は出力の行である。
        assert_eq!(cs[2].expected, vec![b"  $".to_vec()]);
    }

    #[test]
    fn 結果をすべて期待どおりにすると元の中身に戻る() {
        let items = parse(SAMPLE);
        let outcomes = vec![
            Outcome::Ran {
                output: vec![],
                code: 0,
            },
            Outcome::Ran {
                output: b"out\n\n".to_vec(),
                code: 1,
            },
            Outcome::Ran {
                output: b"$\n".to_vec(),
                code: 0,
            },
        ];
        assert_eq!(corrected(&items, &outcomes), SAMPLE);
    }

    #[test]
    fn 最後の改行の有無は区別しない() {
        let a = render(&Outcome::Ran {
            output: b"x".to_vec(),
            code: 0,
        });
        let b = render(&Outcome::Ran {
            output: b"x\n".to_vec(),
            code: 0,
        });
        assert_eq!(a, b);
    }

    #[test]
    fn 改行の直前の_cr_を_1_つだけ落とす() {
        // dune 3.21.1 が promote した結果と同じ行になる。
        let r = render(&Outcome::Ran {
            output: b"a\rb\nx\r\r\n\r\n\r\ny\r".to_vec(),
            code: 0,
        });
        assert_eq!(
            r,
            vec![
                b"  a\rb".to_vec(),
                b"  x\r".to_vec(),
                b"  ".to_vec(),
                b"  ".to_vec(),
                b"  y\r".to_vec(),
            ]
        );
    }
}
