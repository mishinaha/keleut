//! cram の `.t` の読み取りと、結果の組み立てである。
//!
//! 読み方は dune 3.21.1 の cram の字句解析器(`cram_lexer.mll`)と同じで、行ごとに 3 つの状態を移る
//! (diktor/test/via_ir_run.py の `parse_cram` と同じ形)。
//!
//! - 初めの状態では、`  $ ` で始まる行がコマンドの始まりである。
//!   直後に続く `  > ` の行(または `  >` だけの行)が、同じコマンドの続きの行である。
//!   コマンドを読むと、初めの状態に戻る。
//! - 初めの状態で 2 つの空白で始まるほかの行を読むと、出力の状態に移る。
//!   `  $` だけの行も、2 つの空白で始まるので出力の行になる。
//!   初めの状態のそのほかの行は注釈で、注釈の状態に移る。
//! - 注釈の状態では、2 つの空白で始まらない行がすべて注釈である。
//!   2 つの空白で始まる行で、初めの状態に戻る。
//! - 出力の状態では、2 つの空白で始まる行が出力の行である。
//!   コマンドの直後から続く出力の行だけが、そのコマンドの期待の出力になる。
//!   0 でない終了状態は、期待の出力の最後の `[N]` の行で表す。
//!   注釈の後の出力の行は、どのコマンドにも付かないので捨てる([`Item::Dropped`])。
//! - 出力の状態の空白 1 つだけの行は、取っておく。
//!   次に `  $ ` の行が来れば、取っておいた行は捨てる。
//!   ファイルが終わるか、ほかの行が来れば、取っておいた行は注釈になる。
//!   空白 1 つで始まりほかの文字が続く行は注釈で、取っておいた行とともに注釈の状態に移る。
//!
//! 捨てた行は dune の `.t.corrected` に現れないので、dune では期待との差になる。
//! kconform も、捨てた行を記録との差として報告する。
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
    /// どのコマンドにも付かないので、dune が捨てる行である。
    /// [`corrected`] は書かない。
    Dropped {
        /// 行の番号(1 から数える)である。
        line: usize,
        /// 行の中身である。
        text: Vec<u8>,
    },
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

/// `.t` の中身を項目の列に分ける(読み方はモジュールの説明を見よ)。
pub fn parse(text: &[u8]) -> Vec<Item> {
    #[derive(PartialEq, Eq)]
    enum State {
        Block,
        Output,
        Comment,
    }
    let mut lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut items = Vec::new();
    let mut state = State::Block;
    // 出力の行を足す先のコマンドの、`items` の中の位置である。
    let mut cur: Option<usize> = None;
    // 出力の状態で取っておいた、空白 1 つだけの行(行の番号と中身)である。
    let mut pending: Vec<(usize, Vec<u8>)> = Vec::new();
    let add_output = |items: &mut Vec<Item>, cur: Option<usize>, i: usize, l: &[u8]| match cur
        .and_then(|k| match &mut items[k] {
            Item::Command(c) => Some(c),
            _ => None,
        }) {
        Some(c) => c.expected.push(l.to_vec()),
        None => items.push(Item::Dropped {
            line: i + 1,
            text: l.to_vec(),
        }),
    };
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        match state {
            State::Output => {
                if is_command_start(l) {
                    for (n, t) in pending.drain(..) {
                        items.push(Item::Dropped { line: n, text: t });
                    }
                    state = State::Block;
                    continue;
                }
                if l == b" " {
                    pending.push((i + 1, l.to_vec()));
                    i += 1;
                    continue;
                }
                if l.starts_with(b"  ") {
                    add_output(&mut items, cur, i, l);
                    i += 1;
                    continue;
                }
                state = if pending.is_empty() {
                    State::Block
                } else {
                    cur = None;
                    State::Comment
                };
                items.extend(pending.drain(..).map(|(_, t)| Item::Comment(t)));
                if l.starts_with(b" ") {
                    items.push(Item::Comment(l.to_vec()));
                    cur = None;
                    state = State::Comment;
                    i += 1;
                }
            }
            State::Comment => {
                if l.starts_with(b"  ") {
                    state = State::Block;
                    continue;
                }
                items.push(Item::Comment(l.to_vec()));
                i += 1;
            }
            State::Block => {
                if is_command_start(l) {
                    let line = i + 1;
                    let mut text = vec![body(l)];
                    i += 1;
                    while i < lines.len() && is_continuation(lines[i]) {
                        text.push(body(lines[i]));
                        i += 1;
                    }
                    cur = Some(items.len());
                    items.push(Item::Command(Command {
                        line,
                        text,
                        expected: Vec::new(),
                    }));
                } else if l.starts_with(b"  ") && !is_continuation(l) {
                    add_output(&mut items, cur, i, l);
                    state = State::Output;
                    i += 1;
                } else {
                    items.push(Item::Comment(l.to_vec()));
                    cur = None;
                    state = State::Comment;
                    i += 1;
                }
            }
        }
    }
    items.extend(pending.into_iter().map(|(_, t)| Item::Comment(t)));
    items
}

/// コマンドの列だけを取り出す。
pub fn commands(items: &[Item]) -> Vec<&Command> {
    items
        .iter()
        .filter_map(|it| match it {
            Item::Command(c) => Some(c),
            Item::Comment(_) | Item::Dropped { .. } => None,
        })
        .collect()
}

/// dune が捨てる行(行の番号と中身)の列を取り出す。
pub fn dropped(items: &[Item]) -> Vec<(usize, &[u8])> {
    items
        .iter()
        .filter_map(|it| match it {
            Item::Dropped { line, text } => Some((*line, text.as_slice())),
            _ => None,
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
/// 改行で終わる行は、改行の直前の CR を 1 つだけ落とす(dune が使う Stdune の `split_lines` と同じ)。
/// 改行の無い最後の部分は、CR 1 つだけなら捨て、ほかは末尾の CR を含めてそのまま残す。
/// 行の途中の CR は残す。
/// 0 でない終了状態は `[N]` の行にする。
pub fn render(outcome: &Outcome) -> Vec<Vec<u8>> {
    match outcome {
        Outcome::Unreachable => vec![b"  ***** UNREACHABLE *****".to_vec()],
        Outcome::Ran { output, code } => {
            let mut v = Vec::new();
            if !output.is_empty() {
                let mut parts: Vec<&[u8]> = output.split(|&b| b == b'\n').collect();
                // LF の後ろの部分が空か CR 1 つだけなら捨てる。残る部分はどれも LF で終わった行である。
                let last_ended = matches!(parts.last(), Some(&b"") | Some(&b"\r"));
                if last_ended {
                    parts.pop();
                }
                let n = parts.len();
                for (k, p) in parts.into_iter().enumerate() {
                    // 最後の部分は、捨てた部分がその後ろにあったときだけ LF で終わった行である。
                    let ended_by_lf = k + 1 < n || last_ended;
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
            Item::Dropped { .. } => {}
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
    fn dune_と同じく注釈の後の出力の行と取っておいた行を捨てる() {
        // dune 3.21.1 が `.t.corrected` から落とす行は、`  b`、`  $ echo d` の前の ` `、`  $`、`  x`、`  > orphan` である。
        // `  $` と `  x`、`  > orphan` は期待の出力になり、ほかは捨てる行になる。
        let t = b"  $ echo a\n  a\ncomment\n  b\n  $ echo c\n  c\n \n  $ echo d\n  d\n  $\n  $ echo e\n  e\n \n  x\n  > orphan\n";
        let items = parse(t);
        let cs = commands(&items);
        assert_eq!(cs.len(), 4);
        assert_eq!(cs[0].expected, vec![b"  a".to_vec()]);
        assert_eq!(cs[1].expected, vec![b"  c".to_vec()]);
        assert_eq!(cs[2].expected, vec![b"  d".to_vec(), b"  $".to_vec()]);
        assert_eq!(
            cs[3].expected,
            vec![b"  e".to_vec(), b"  x".to_vec(), b"  > orphan".to_vec()]
        );
        assert_eq!(dropped(&items), vec![(4, &b"  b"[..]), (7, &b" "[..])]);
        // ファイルの終わりで取っておいた行は注釈になる。
        assert_eq!(items.last(), Some(&Item::Comment(b" ".to_vec())));
        // 空白 1 つの行の後の注釈は、取っておいた行とともに注釈になり、その後の出力の行は捨てる。
        let items = parse(b"  $ echo e\n  e\n \nnote\n  y\n");
        assert_eq!(commands(&items)[0].expected, vec![b"  e".to_vec()]);
        assert_eq!(dropped(&items), vec![(5, &b"  y"[..])]);
        // コマンドの直後の空白 1 つの行は注釈で、続く `  $ ` の行は新しいコマンドである。
        let items = parse(b"  $ true\n \n  $ false\n  [1]\n");
        assert_eq!(commands(&items).len(), 2);
        assert!(dropped(&items).is_empty());
        // コマンドに続かない `  > ` の行は注釈である。
        let items = parse(b"x\n  > y\n");
        assert_eq!(items[1], Item::Comment(b"  > y".to_vec()));
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
        // 改行の無い最後の部分が CR 1 つだけなら、dune はその行を書かない。
        let r = render(&Outcome::Ran {
            output: b"a\r\n\r".to_vec(),
            code: 1,
        });
        assert_eq!(r, vec![b"  a".to_vec(), b"  [1]".to_vec()]);
        let r = render(&Outcome::Ran {
            output: b"\r".to_vec(),
            code: 0,
        });
        assert!(r.is_empty());
    }
}
