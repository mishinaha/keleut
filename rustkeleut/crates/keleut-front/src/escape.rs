//! OCaml の `String.escaped` と同じ書き方である。
//!
//! Diktor は文字列のトークンと AST の文字列を、この形で引用符の中に書く。
//! 0x20〜0x7E のうち `"` と `\` を除くバイトはそのまま、`\n`、`\t`、`\r`、`\b` は 2 文字の綴り、
//! ほかのバイト(UTF-8 の多バイト文字のバイトを含む)は `\` と 10 進 3 桁にする。

/// バイト列を `String.escaped` の形にする。
pub fn ocaml_escaped(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s {
        match c {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            8 => out.push_str("\\b"),
            b' '..=b'~' => out.push(c as char),
            _ => {
                out.push('\\');
                out.push((b'0' + c / 100) as char);
                out.push((b'0' + c / 10 % 10) as char);
                out.push((b'0' + c % 10) as char);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocaml_と同じ() {
        assert_eq!(
            ocaml_escaped(b"a\"b\\c\n\t\r\x08"),
            "a\\\"b\\\\c\\n\\t\\r\\b"
        );
        assert_eq!(ocaml_escaped("あ".as_bytes()), "\\227\\129\\130");
        assert_eq!(ocaml_escaped(b"\x00\x7f"), "\\000\\127");
    }
}
