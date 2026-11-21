//! cram の `.t` を読み、実行の結果から `.t` を組み立てる。
//!
//! 読み方は dune 3.21.1 の cram の字句解析器(`cram_lexer.mll`)と同じで、1 行ごとに 3 つの状態を遷移する。
//! `diktor/test/via_ir_run.py` の `parse_cram` も同じ読み方をする。
//!
//! - 初期状態では、`  $ ` で始まる行がコマンドの始まりである。
//!   直後に続く `  > ` で始まる行と `  >` だけの行が、同じコマンドの続きの行である。
//!   コマンドを読み終えると、初期状態に戻る。
//! - 初期状態で、空白 2 個で始まるそれ以外の行を読むと、その行を出力の行として、出力の状態に遷移する。
//!   `  $` だけの行も、空白 2 個で始まるので出力の行になる。
//!   ただし、コマンドに続かない `  > ` の行と `  >` だけの行は、初期状態ではコメントになる。
//!   初期状態で読んだそれ以外の行はコメントで、コメントの状態に遷移する。
//! - コメントの状態では、空白 2 個で始まらない行がすべてコメントである。
//!   空白 2 個で始まる行を読むと、初期状態に戻ってその行を読み直す。
//! - 出力の状態では、空白 2 個で始まる行が出力の行である。
//!   `  $ ` で始まる行を読むと、初期状態に戻ってその行を読み直す。
//!   コマンドの直後から続く出力の行だけが、そのコマンドの期待出力になる。
//!   0 でない終了コードは、期待出力の最後の `[N]` の行で表す。
//!   コメントの後の出力の行は、どのコマンドにも属さないので捨てる([`Item::Dropped`])。
//! - 出力の状態で読んだ空白 1 個だけの行は、扱いを決めずに保存する。
//!   出力の行が続く間は、保存したままにする。
//!   次に `  $ ` で始まる行を読むと、保存した行を捨てる。
//!   それ以外の行を読むか、ファイルが終わると、保存した行はコメントになる。
//!   空白 1 個で始まり、ほかの文字が続く行はコメントで、保存した行をコメントにしたうえでコメントの状態に遷移する。
//!
//! 捨てた行は dune の `.t.corrected` に現れないので、dune では期待出力との差分になる。
//! kconform も、捨てた行を期待出力との差分として報告する。
//!
//! 行はバイト列のまま扱う。
//! `.t` にも出力にも、UTF-8 として正しくないバイト列が現れることがあるからである。

/// `.t` の 1 つの項目である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// コメントの行である。
    Comment(Vec<u8>),
    /// コマンドとその期待出力である。
    Command(Command),
    /// どのコマンドにも属さないので、dune が捨てる行である。
    /// [`corrected`] はこの行を書かない。
    Dropped {
        /// 1 始まりの行番号である。
        line: usize,
        /// 行の内容である。
        text: Vec<u8>,
    },
}

/// cram の 1 つのコマンドである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// `  $ ` の行の、1 始まりの行番号である。
    pub line: usize,
    /// コマンドの行の並びである。
    /// 先頭の要素は `  $ ` の行、残りの要素は `  > ` で始まる続きの行で、どれも接頭辞を取り除いた内容である。
    pub text: Vec<Vec<u8>>,
    /// 期待出力の行の並びである。
    /// 各行は、先頭の空白 2 個を含めた `.t` の行そのままである。
    pub expected: Vec<Vec<u8>>,
}

impl Command {
    /// コマンドを、シェルに渡すスクリプトにする。
    /// 各行の終わりに改行を付ける。
    pub fn script(&self) -> Vec<u8> {
        let mut s = Vec::new();
        for l in &self.text {
            s.extend_from_slice(l);
            s.push(b'\n');
        }
        s
    }

    /// コマンドのすべての行を、空白 1 個でつないだ 1 つの文字列にする。
    /// 表示と検索に使う。
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

/// 行の先頭の 4 バイト(`  $ ` か `  > `)を除いた残りを返す。
/// `  >` だけの行では、空のバイト列を返す。
fn body(l: &[u8]) -> Vec<u8> {
    if l.len() >= 4 {
        l[4..].to_vec()
    } else {
        Vec::new()
    }
}

/// `.t` の内容を項目の並びに分ける。
/// 読み方は、このモジュールの説明を参照。
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
    // 出力の行を追加する先のコマンドの、`items` の中の添字である。
    let mut cur: Option<usize> = None;
    // 出力の状態で保存した、空白 1 個だけの行(行番号と内容)である。
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

/// 項目の並びから、コマンドだけを取り出す。
pub fn commands(items: &[Item]) -> Vec<&Command> {
    items
        .iter()
        .filter_map(|it| match it {
            Item::Command(c) => Some(c),
            Item::Comment(_) | Item::Dropped { .. } => None,
        })
        .collect()
}

/// 項目の並びから、dune が捨てる行だけを、行番号と内容の組にして取り出す。
pub fn dropped(items: &[Item]) -> Vec<(usize, &[u8])> {
    items
        .iter()
        .filter_map(|it| match it {
            Item::Dropped { line, text } => Some((*line, text.as_slice())),
            _ => None,
        })
        .collect()
}

/// 1 つのコマンドを実行した結果である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// コマンドを実行した。
    /// `output` は、標準出力と標準エラーを 1 つにまとめ、作業ディレクトリのパスを置き換えた後の出力である。
    /// `code` は終了コードである。
    Ran { output: Vec<u8>, code: i32 },
    /// 前のコマンドが `exit` などでシェルを終了させたので、実行しなかった。
    Unreachable,
}

/// 結果を、`.t` の期待出力と同じ形式の行の並びにする。
///
/// dune の cram と同じく、出力の最後の改行の有無は区別しない。
/// 改行で終わる行からは、改行の直前の CR を 1 個だけ取り除く。
/// dune が使う Stdune の `split_lines` と同じ動作である。
/// 改行で終わらない最後の部分は、CR 1 個だけなら捨て、それ以外は末尾の CR を含めてそのまま残す。
/// 行の途中の CR は残す。
/// 0 でない終了コードは `[N]` の行にする。
pub fn render(outcome: &Outcome) -> Vec<Vec<u8>> {
    match outcome {
        Outcome::Unreachable => vec![b"  ***** UNREACHABLE *****".to_vec()],
        Outcome::Ran { output, code } => {
            let mut v = Vec::new();
            if !output.is_empty() {
                let mut parts: Vec<&[u8]> = output.split(|&b| b == b'\n').collect();
                // 最後の LF の後ろの部分が空か CR 1 個だけなら捨てる。残る部分はどれも LF で終わった行である。
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

/// 項目の並びと各コマンドの結果から、期待出力を実際の結果に書き換えた `.t` の内容を作る。
/// dune の `.t.corrected` に相当する。
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
        // dune 3.21.1 が `.t.corrected` に書かない行は、`  b`、`  $ echo d` の前の ` `、`  $`、`  x`、`  > orphan` である。
        // このうち `  $`、`  x`、`  > orphan` は期待出力になり、残りは捨てる行になる。
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
        // ファイルが終わると、保存した行はコメントになる。
        assert_eq!(items.last(), Some(&Item::Comment(b" ".to_vec())));
        // 空白 1 個だけの行の後にコメントが来ると、保存した行もコメントになり、その後の出力の行は捨てる。
        let items = parse(b"  $ echo e\n  e\n \nnote\n  y\n");
        assert_eq!(commands(&items)[0].expected, vec![b"  e".to_vec()]);
        assert_eq!(dropped(&items), vec![(5, &b"  y"[..])]);
        // コマンドの直後の空白 1 個だけの行はコメントで、続く `  $ ` の行は新しいコマンドである。
        let items = parse(b"  $ true\n \n  $ false\n  [1]\n");
        assert_eq!(commands(&items).len(), 2);
        assert!(dropped(&items).is_empty());
        // コマンドに続かない `  > ` の行はコメントである。
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
        // 改行で終わらない最後の部分が CR 1 個だけなら、dune はその行を書かない。
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
