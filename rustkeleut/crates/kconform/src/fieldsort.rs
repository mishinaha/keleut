//! 付録 B の `<値>` の中のレコードのフィールドを並べ替える(計画 261104-1 の 5.1 節、D560)。
//!
//! IRSpec 付録 B は、`<値>` のレコードのラベルの違うフィールドどうしの順を読み手に任せる(D555)。
//! そこで、Diktor と RustKeleut の出力を比べる前に、両方のレコードのフィールドをラベルのバイト列の順に安定に並べ替える。
//! 安定なので、同じラベルのフィールドは元の順(最も外側から)のまま残る。
//!
//! 値の字句は、文字列(`"` で囲み、`\` で次の 1 文字を逃がす)、括弧(`(` `)` `[` `]` `{` `}`)、`, `、`,`、` = `、それ以外の文字の並びである。
//! `{` で始まり、ラベル、` = `、値の組を `, ` で並べて `}` で閉じる部分をレコードとする。
//! 入れ子は作業の列でたどり、再帰でネイティブのスタックを使わない。
//!
//! 読めない綴り(閉じない文字列、対応しない括弧、レコードの形をしない `{` の組、フィールドの頭の外の ` = `)は `None` とし、行を変えない。
//! 読み違えて並べ替えると本物の差を隠すので、読める形を狭く取る。

/// 字句である。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// 開き括弧(`(` `[` `{`)。
    Open(u8),
    /// 閉じ括弧(`)` `]` `}`)。
    Close(u8),
    /// `, `(列の区切り)。
    Sep,
    /// 後ろに空白の無い `,`(`(1,)` の形)。
    Comma,
    /// ` = `。
    Eq,
    /// 文字列。`[start, end)` は引用符を含む。
    Str(usize, usize),
    /// それ以外の文字の並び。
    Atom(usize, usize),
}

/// 字句に分ける。
/// 文字列が閉じなければ `None` を返す。
fn lex(s: &[u8]) -> Option<Vec<Tok>> {
    let mut v = Vec::new();
    let mut i = 0;
    let mut atom: Option<usize> = None;
    let flush = |v: &mut Vec<Tok>, atom: &mut Option<usize>, i: usize| {
        if let Some(a) = atom.take() {
            v.push(Tok::Atom(a, i));
        }
    };
    while i < s.len() {
        let b = s[i];
        let tok = match b {
            b'"' => {
                let start = i;
                i += 1;
                loop {
                    match s.get(i) {
                        None => return None,
                        Some(b'\\') => i += 2,
                        Some(b'"') => break,
                        Some(_) => i += 1,
                    }
                }
                Some((Tok::Str(start, i + 1), i + 1 - start))
            }
            b'(' | b'[' | b'{' => Some((Tok::Open(b), 1)),
            b')' | b']' | b'}' => Some((Tok::Close(b), 1)),
            b',' if s.get(i + 1) == Some(&b' ') => Some((Tok::Sep, 2)),
            b',' => Some((Tok::Comma, 1)),
            b' ' if s[i..].starts_with(b" = ") => Some((Tok::Eq, 3)),
            _ => None,
        };
        match tok {
            Some((t, n)) => {
                let start = if let Tok::Str(a, _) = t { a } else { i };
                flush(&mut v, &mut atom, start);
                v.push(t);
                i = start + n;
            }
            None => {
                atom.get_or_insert(i);
                i += 1;
            }
        }
    }
    flush(&mut v, &mut atom, s.len());
    Some(v)
}

/// 組の中の 1 項目である。
#[derive(Clone, Copy)]
enum Item {
    /// 字句の添字。
    Tok(usize),
    /// 入れ子の組の番号。
    Group(usize),
}

/// 括弧で囲んだ組である。番号 0 は値の全体(括弧なし)である。
struct Group {
    open: u8,
    items: Vec<Item>,
}

fn closer(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        _ => b'}',
    }
}

/// 値の綴り 1 つを受け取り、レコードのフィールドをラベルのバイト列の順に安定に並べ替えた綴りを返す。
/// 読めないときは `None` を返す。
pub fn sort_value(s: &[u8]) -> Option<Vec<u8>> {
    let toks = lex(s)?;
    // 括弧の対応を明示の積み上げでたどり、組を平らな列に置く。
    let mut groups = vec![Group {
        open: 0,
        items: Vec::new(),
    }];
    let mut stack = vec![0usize];
    for (k, t) in toks.iter().enumerate() {
        let cur = *stack.last().unwrap();
        match *t {
            Tok::Open(b) => {
                let id = groups.len();
                groups.push(Group {
                    open: b,
                    items: Vec::new(),
                });
                groups[cur].items.push(Item::Group(id));
                stack.push(id);
            }
            Tok::Close(b) => {
                if cur == 0 || closer(groups[cur].open) != b {
                    return None;
                }
                stack.pop();
            }
            _ => groups[cur].items.push(Item::Tok(k)),
        }
    }
    if stack.len() != 1 {
        return None;
    }
    let is_tok = |it: Option<&Item>, want: fn(&Tok) -> bool| match it {
        Some(Item::Tok(k)) => want(&toks[*k]),
        _ => false,
    };
    // 各組を確かめ、レコードのフィールドを並べ替える。
    for g in groups.iter_mut() {
        if g.open != b'{' {
            if g.items.iter().any(|it| is_tok(Some(it), |t| *t == Tok::Eq)) {
                return None;
            }
            continue;
        }
        if g.items.is_empty() {
            continue;
        }
        let mut fields: Vec<&[Item]> = g
            .items
            .split(|it| is_tok(Some(it), |t| *t == Tok::Sep))
            .collect();
        for f in &fields {
            let head_ok = f.len() >= 3
                && is_tok(f.first(), |t| matches!(t, Tok::Atom(..)))
                && is_tok(f.get(1), |t| *t == Tok::Eq)
                && !f[2..].iter().any(|it| is_tok(Some(it), |t| *t == Tok::Eq));
            if !head_ok {
                return None;
            }
        }
        let label = |f: &[Item]| -> &[u8] {
            match f[0] {
                Item::Tok(k) => match toks[k] {
                    Tok::Atom(a, b) => &s[a..b],
                    _ => unreachable!(),
                },
                Item::Group(_) => unreachable!(),
            }
        };
        fields.sort_by(|a, b| label(a).cmp(label(b)));
        // `Sep` の字句はどれも同じ綴りなので、最初のものを区切りに使い回す。
        let sep = g
            .items
            .iter()
            .copied()
            .find(|it| is_tok(Some(it), |t| *t == Tok::Sep));
        let mut items = Vec::with_capacity(g.items.len());
        for (i, f) in fields.into_iter().enumerate() {
            if i > 0 {
                items.push(sep.unwrap());
            }
            items.extend_from_slice(f);
        }
        g.items = items;
    }
    // 明示の積み上げで綴りに戻す。
    let mut out = Vec::with_capacity(s.len());
    let mut work: Vec<(usize, usize)> = vec![(0, 0)];
    while let Some((g, i)) = work.pop() {
        let grp = &groups[g];
        if i == grp.items.len() {
            if g != 0 {
                out.push(closer(grp.open));
            }
            continue;
        }
        work.push((g, i + 1));
        match grp.items[i] {
            Item::Tok(k) => match toks[k] {
                Tok::Sep => out.extend_from_slice(b", "),
                Tok::Comma => out.push(b','),
                Tok::Eq => out.extend_from_slice(b" = "),
                Tok::Str(a, b) | Tok::Atom(a, b) => out.extend_from_slice(&s[a..b]),
                Tok::Open(_) | Tok::Close(_) => unreachable!(),
            },
            Item::Group(c) => {
                out.push(groups[c].open);
                work.push((c, 0));
            }
        }
    }
    Some(out)
}

/// 記録の接頭辞である(付録 B の実行時エラーと、`cancel` 節の打ち切りの記録)。
const LEADS: &[&str] = &["実行時エラー: ", "cancel 節で例外が抑制されました: "];

/// `<値>` の前までの文面である(付録 B の 3 番、4 番、9 番)。
const MESSAGES: &[&str] = &[
    "パターンに値が一致しません: ",
    "match のどの節にも一致しません: ",
    "handle のどの節にも一致しません: ",
];

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// 1 行を受け取り、記録の接頭辞の直後に文面が続く連結の直後から行末までに [`sort_value`] を当てた行を返す。
///
/// 連結は行の中で最初に現れるものを使い、前の文字は問わない(cram の字下げや、改行の無い標準出力の後ろの診断のため)。
/// 連結が無いときと、値が読めないときは、行を変えずに返す。
/// `panic: <文字列>` のように接頭辞と文面の間に別の語が入る行には当てない。
/// 接頭辞の直後に `panic: ` が続くとき、その後ろは利用者の文字列なので、そこに現れる連結にも当てない。
pub fn sort_line(l: &[u8]) -> Vec<u8> {
    // 利用者の文字列が始まる位置(最初の `<接頭辞>panic: ` の位置)。
    let panic_at = LEADS
        .iter()
        .filter_map(|lead| find(l, &[lead.as_bytes(), b"panic: "].concat()))
        .min();
    // (連結の位置、連結の終わり)。位置が先のものを採る。
    let mut best: Option<(usize, usize)> = None;
    for lead in LEADS {
        for msg in MESSAGES {
            let pat = [lead.as_bytes(), msg.as_bytes()].concat();
            if let Some(i) = find(l, &pat)
                && best.is_none_or(|(j, _)| i < j)
            {
                best = Some((i, i + pat.len()));
            }
        }
    }
    let Some((_, end)) = best.filter(|&(i, _)| panic_at.is_none_or(|p| i < p)) else {
        return l.to_vec();
    };
    match sort_value(&l[end..]) {
        Some(v) => [&l[..end], &v[..]].concat(),
        None => l.to_vec(),
    }
}

/// 2 つの出力を、行ごとに [`sort_line`] を当ててから比べる(計画 261104-1 の 5.2 節、D561)。
///
/// Diktor の出力と keleut の出力をバイト列で比べていた試験と道具が使う。
pub fn same_output(a: &[u8], b: &[u8]) -> bool {
    a == b || sort_text(a) == sort_text(b)
}

/// 出力の各行に [`sort_line`] を当てる。改行の位置と数は変えない。
pub fn sort_text(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for (i, l) in s.split(|&b| b == b'\n').enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(&sort_line(l));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Option<String> {
        sort_value(s.as_bytes()).map(|v| String::from_utf8(v).unwrap())
    }

    fn l(s: &str) -> String {
        String::from_utf8(sort_line(s.as_bytes())).unwrap()
    }

    const RT: &str = "実行時エラー: パターンに値が一致しません: ";
    const CX: &str = "cancel 節で例外が抑制されました: match のどの節にも一致しません: ";

    #[test]
    fn レコードのフィールドをラベルの順に並べる() {
        assert_eq!(v("{c = 3, a = 1, b = 2}").unwrap(), "{a = 1, b = 2, c = 3}");
        assert_eq!(v("{}").unwrap(), "{}");
        assert_eq!(v("{x = 1}").unwrap(), "{x = 1}");
        // ラベルはバイト列で比べる。`_` は小文字より前、大文字より後ろである。
        assert_eq!(
            v("{b = 1, _item = 2, a = 3, B = 4}").unwrap(),
            "{B = 4, _item = 2, a = 3, b = 1}"
        );
    }

    #[test]
    fn 同じラベルは元の順を保つ() {
        assert_eq!(
            v("{y = \"t\", x = true, x = 1}").unwrap(),
            "{x = true, x = 1, y = \"t\"}"
        );
        assert_eq!(v("{x = 2, y = 0, x = 1}").unwrap(), "{x = 2, x = 1, y = 0}");
    }

    #[test]
    fn 入れ子とほかの値の形() {
        assert_eq!(
            v("{b = {d = 1, c = 2}, a = [{z = 1, y = 2}]}").unwrap(),
            "{a = [{y = 2, z = 1}], b = {c = 2, d = 1}}"
        );
        assert_eq!(v("#P({q = 1, p = 2})").unwrap(), "#P({p = 2, q = 1})");
        assert_eq!(v("Box({q = 1, p = 2})").unwrap(), "Box({p = 2, q = 1})");
        assert_eq!(
            v("({b = 1, a = 2}, {d = 3, c = 4})").unwrap(),
            "({a = 2, b = 1}, {c = 4, d = 3})"
        );
        assert_eq!(v("({b = 1, a = 2},)").unwrap(), "({a = 2, b = 1},)");
        assert_eq!(
            v("{f = <fn>, e = <prim Foo.bar>, d = <mutable [{b = 1, a = 2}]>, c = #N}").unwrap(),
            "{c = #N, d = <mutable [{a = 2, b = 1}]>, e = <prim Foo.bar>, f = <fn>}"
        );
        assert_eq!(v("[1, 2, 3]").unwrap(), "[1, 2, 3]");
        assert_eq!(v("-1.5e+10").unwrap(), "-1.5e+10");
    }

    #[test]
    fn 文字列の中の括弧と逃がした引用符は数えない() {
        assert_eq!(
            v(r#"{b = "}{ = ,", a = "\"{", c = "\\"}"#).unwrap(),
            r#"{a = "\"{", b = "}{ = ,", c = "\\"}"#
        );
    }

    #[test]
    fn 操作と引数の組() {
        // 付録 B の 9 番の `<操作><値>`(引数の組の綴り)。
        assert_eq!(
            v("Two.b2({c = 3, a = 1, b = 2},)").unwrap(),
            "Two.b2({a = 1, b = 2, c = 3},)"
        );
        assert_eq!(
            l("  実行時エラー: handle のどの節にも一致しません: E.op({b = 1, a = 2}, 3)"),
            "  実行時エラー: handle のどの節にも一致しません: E.op({a = 2, b = 1}, 3)"
        );
    }

    #[test]
    fn 読めない綴りは_none() {
        for s in [
            "{b = 1, a = 2",
            "{b = 1, a = 2})",
            "(b = 1]",
            "\"abc",
            "\"abc\\",
            "{b, a}",
            "{b = 1, 2}",
            "{ = 1}",
            "{a = }",
            "{a = 1 = 2}",
            "(a = 1)",
            "a = 1",
            "{\"a\" = 1}",
        ] {
            assert_eq!(v(s), None, "{s}");
        }
    }

    #[test]
    fn 行の中の連結の直後から並べる() {
        assert_eq!(
            l(&format!("{RT}{{b = 1, a = 2}}")),
            format!("{RT}{{a = 2, b = 1}}")
        );
        // cram の字下げ。
        assert_eq!(
            l(&format!("  {CX}{{b = 1, a = 2}}")),
            format!("  {CX}{{a = 2, b = 1}}")
        );
        // 改行の無い標準出力の後ろに続く診断。
        assert_eq!(
            l(&format!("partial{RT}{{b = 1, a = 2}}")),
            format!("partial{RT}{{a = 2, b = 1}}")
        );
        // 3 つの文面のどれでもよい。
        assert_eq!(
            l("cancel 節で例外が抑制されました: パターンに値が一致しません: {b = 1, a = 2}"),
            "cancel 節で例外が抑制されました: パターンに値が一致しません: {a = 2, b = 1}"
        );
        // 2 つ現れれば、先のものの直後から行末までを 1 つの綴りとして読む。
        let two = format!("{RT}{{b = 1, a = 2}} {RT}{{d = 1, c = 2}}");
        assert_eq!(
            l(&two),
            format!("{RT}{{a = 2, b = 1}} {RT}{{c = 2, d = 1}}")
        );
    }

    #[test]
    fn 当てない行は変えない() {
        for s in [
            "  [1, 2]",
            "  {b = 1, a = 2}",
            "  実行時エラー: ゼロ除算です",
            // 14 番の panic は利用者の文字列をそのまま書くので、並べ替えない。
            "実行時エラー: panic: match のどの節にも一致しません: {b = 1, a = 2}",
            "  cancel 節で例外が抑制されました: panic: match のどの節にも一致しません: {b = 1, a = 2}",
            // 利用者の文字列の中に接頭辞と文面の連結があっても、並べ替えない。
            "実行時エラー: panic: 実行時エラー: パターンに値が一致しません: {b = 1, a = 2}",
            "cancel 節で例外が抑制されました: panic: 実行時エラー: パターンに値が一致しません: {b = 1, a = 2}",
            // 連結の直後に余計な文字があれば読めない。
            "実行時エラー: パターンに値が一致しません: {b = 1, a = 2",
            "実行時エラー: パターンに値が一致しません: {b = 1, a = 2} = x",
            // 種別の語だけでは当てない。
            "パターンに値が一致しません: {b = 1, a = 2}",
        ] {
            assert_eq!(l(s), s);
        }
    }

    #[test]
    fn 深さ_100000_の入れ子() {
        const N: usize = 100_000;
        let mut src = String::new();
        let mut want = String::new();
        for _ in 0..N {
            src.push_str("{b = 1, a = ");
            want.push_str("{a = ");
        }
        src.push('0');
        want.push('0');
        for _ in 0..N {
            src.push('}');
            want.push_str(", b = 1}");
        }
        assert_eq!(sort_value(src.as_bytes()).unwrap(), want.as_bytes());
        let deep_list = format!("{}{}", "[".repeat(N), "]".repeat(N));
        assert_eq!(
            sort_value(deep_list.as_bytes()).unwrap(),
            deep_list.as_bytes()
        );
        let unclosed = "(".repeat(N);
        assert_eq!(sort_value(unclosed.as_bytes()), None);
    }

    #[test]
    fn 出力の比べ方() {
        let d = format!("a\n{CX}{{c = 3, a = 1, b = 2}}\n");
        let k = format!("a\n{CX}{{a = 1, b = 2, c = 3}}\n");
        assert!(same_output(d.as_bytes(), k.as_bytes()));
        // 値が違えば違う。
        let k2 = format!("a\n{CX}{{a = 1, b = 2, c = 4}}\n");
        assert!(!same_output(d.as_bytes(), k2.as_bytes()));
        // 同じラベルの順が違えば違う。
        assert!(!same_output(
            format!("{RT}{{x = 1, x = 2}}").as_bytes(),
            format!("{RT}{{x = 2, x = 1}}").as_bytes()
        ));
        // 当てない行の順の差は違いのまま残る。
        assert!(!same_output(b"{b = 1, a = 2}\n", b"{a = 2, b = 1}\n"));
        // 改行の数の差も違いである。
        assert!(!same_output(b"x\n", b"x"));
        assert!(same_output(b"", b""));
        assert_eq!(sort_text(b"x\n\ny"), b"x\n\ny");
    }
}
