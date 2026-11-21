//! IRSpec 付録 B の `<値>` の中にあるレコードのフィールドを並べ替える。
//!
//! IRSpec 付録 B は、レコードの中でラベルの異なるフィールドをどの順序で書くかを、リーダに任せている。
//! そこで、Diktor と RustKeleut の出力を比較する前に、両方のレコードのフィールドをラベルのバイト列の順に安定ソートする。
//! 安定ソートなので、同じラベルのフィールドは元の順序(最も外側のものから)のまま残る。
//!
//! 値の表記は、次のトークンに分ける。
//! 文字列(`"` で囲み、`\` で次の 1 文字をエスケープする)、括弧(`(` `)` `[` `]` `{` `}`)、`, `、`,`、` = `、それ以外の文字の並びである。
//! `{` で始まり、ラベル、` = `、値の組を `, ` で区切って並べ、`}` で閉じる部分をレコードとして扱う。
//! 入れ子は明示的なスタックでたどるので、深い入れ子でも再帰でネイティブスタックを使うことはない。
//!
//! 閉じていない文字列、対応の取れない括弧、レコードの形式でない `{` `}` の内容、ラベルの直後以外に現れる ` = ` のどれかを含む表記は、読めない表記として扱う。
//! このとき [`sort_value`] は `None` を返し、[`sort_line`] は行を変えずに返す。
//! 誤って読んで並べ替えると実際の差分を隠すので、読める形式を狭くしている。

/// 値の表記のトークンである。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// 開き括弧(`(` `[` `{`)である。
    Open(u8),
    /// 閉じ括弧(`)` `]` `}`)である。
    Close(u8),
    /// 要素を区切る `, ` である。
    Sep,
    /// 後ろに空白のない `,` である(`(1,)` の `,`)。
    Comma,
    /// ` = ` である。
    Eq,
    /// 文字列である。
    /// 範囲 `[start, end)` は引用符を含む。
    Str(usize, usize),
    /// それ以外の文字の並びである。
    Atom(usize, usize),
}

/// 値の表記をトークンに分ける。
/// 文字列が閉じていなければ `None` を返す。
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

/// グループの中の 1 要素である。
#[derive(Clone, Copy)]
enum Item {
    /// トークン 1 個である。
    /// 値は、トークンの配列 `toks` の中の添字である。
    Tok(usize),
    /// 入れ子のグループである。
    /// 値は、グループの配列 `groups` の中の番号である。
    Group(usize),
}

/// 対応する括弧で囲んだ範囲(グループ)である。
/// 番号 0 のグループは値の全体で、括弧を持たない。
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

/// 値の表記を 1 つ受け取り、レコードのフィールドをラベルのバイト列の順に安定ソートした表記を返す。
/// 読めない表記のときは `None` を返す。
pub fn sort_value(s: &[u8]) -> Option<Vec<u8>> {
    let toks = lex(s)?;
    // 括弧の対応を明示的なスタックでたどり、グループを 1 本の配列 `groups` に並べる。
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
    // 各グループの形式を検査し、レコードのフィールドを並べ替える。
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
        // `Sep` のトークンはどれも同じ表記なので、最初のものを区切りとして再利用する。
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
    // 明示的なスタックでグループをたどり、表記に戻す。
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

/// 行の接頭辞である。
/// 実行時エラーの行と、`cancel` 節の抑制の記録の行に付く(IRSpec 付録 B)。
const LEADS: &[&str] = &["実行時エラー: ", "cancel 節で例外が抑制されました: "];

/// `<値>`(9 番では `<操作><値>`)の前までのメッセージである(IRSpec 付録 B の 3 番、4 番、9 番)。
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

/// 1 行を受け取り、接頭辞とメッセージの後ろの部分に [`sort_value`] を適用した行を返す。
///
/// 接頭辞(`LEADS`)の直後にメッセージ(`MESSAGES`)が続く部分を、ここでは連結と呼ぶ。
/// 連結の直後から行末まで(IRSpec 付録 B の 3 番と 4 番では `<値>`、9 番では `<操作><値>`)に [`sort_value`] を適用する。
/// 連結は行の中で最初に現れるものを使い、その前にどんな文字があってもよい。
/// cram のインデントや、改行で終わらない標準出力に続けて書かれた診断があるからである。
/// 連結がないときと、値が読めないときは、行を変えずに返す。
/// `panic: <文字列>` のように、接頭辞とメッセージの間に別の文字列が入る行には適用しない。
/// 接頭辞の直後に `panic: ` が続くとき、その後ろはユーザの文字列なので、そこに現れる連結にも適用しない。
pub fn sort_line(l: &[u8]) -> Vec<u8> {
    // ユーザの文字列が始まる位置(最初の `<接頭辞>panic: ` の位置)。
    let panic_at = LEADS
        .iter()
        .filter_map(|lead| find(l, &[lead.as_bytes(), b"panic: "].concat()))
        .min();
    // (連結の位置、連結の終わり)。位置が最も前のものを選ぶ。
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

/// 2 つの出力が、各行に [`sort_line`] を適用した後で一致すれば真を返す。
///
/// Diktor の出力と keleut の出力を比較するテスト(keleut-cli)とツール(`kconform kir-roundtrip`)が使う。
pub fn same_output(a: &[u8], b: &[u8]) -> bool {
    a == b || sort_text(a) == sort_text(b)
}

/// 出力の各行に [`sort_line`] を適用する。
/// 改行の位置と個数は変えない。
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
        // ラベルはバイト列として比較する。`_` は小文字より前、大文字より後ろである。
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
        // IRSpec 付録 B の 9 番の `<操作><値>`。`<値>` は引数の組の表記である。
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
        // cram のインデントがある行。
        assert_eq!(
            l(&format!("  {CX}{{b = 1, a = 2}}")),
            format!("  {CX}{{a = 2, b = 1}}")
        );
        // 改行で終わらない標準出力の後ろに続く診断。
        assert_eq!(
            l(&format!("partial{RT}{{b = 1, a = 2}}")),
            format!("partial{RT}{{a = 2, b = 1}}")
        );
        // 3 つのメッセージのどれでもよい。
        assert_eq!(
            l("cancel 節で例外が抑制されました: パターンに値が一致しません: {b = 1, a = 2}"),
            "cancel 節で例外が抑制されました: パターンに値が一致しません: {a = 2, b = 1}"
        );
        // 連結が 2 つ現れれば、最初の連結の直後から行末までを 1 つの表記として読む。
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
            // 14 番の panic はユーザの文字列をそのまま書くので、並べ替えない。
            "実行時エラー: panic: match のどの節にも一致しません: {b = 1, a = 2}",
            "  cancel 節で例外が抑制されました: panic: match のどの節にも一致しません: {b = 1, a = 2}",
            // ユーザの文字列の中に接頭辞とメッセージの連結があっても、並べ替えない。
            "実行時エラー: panic: 実行時エラー: パターンに値が一致しません: {b = 1, a = 2}",
            "cancel 節で例外が抑制されました: panic: 実行時エラー: パターンに値が一致しません: {b = 1, a = 2}",
            // 連結の直後から行末までが値として読めなければ、並べ替えない。
            "実行時エラー: パターンに値が一致しません: {b = 1, a = 2",
            "実行時エラー: パターンに値が一致しません: {b = 1, a = 2} = x",
            // 接頭辞がなくメッセージだけの行には適用しない。
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
        // 値が違えば一致しない。
        let k2 = format!("a\n{CX}{{a = 1, b = 2, c = 4}}\n");
        assert!(!same_output(d.as_bytes(), k2.as_bytes()));
        // 同じラベルのフィールドの順序が違えば一致しない。
        assert!(!same_output(
            format!("{RT}{{x = 1, x = 2}}").as_bytes(),
            format!("{RT}{{x = 2, x = 1}}").as_bytes()
        ));
        // 並べ替えを適用しない行では、フィールドの順序の違いも不一致になる。
        assert!(!same_output(b"{b = 1, a = 2}\n", b"{a = 2, b = 1}\n"));
        // 改行の個数が違っても一致しない。
        assert!(!same_output(b"x\n", b"x"));
        assert!(same_output(b"", b""));
        assert_eq!(sort_text(b"x\n\ny"), b"x\n\ny");
    }
}
