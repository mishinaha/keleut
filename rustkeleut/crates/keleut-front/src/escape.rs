//! OCaml の `String.escaped` と同じ規則で、バイト列をエスケープする。
//!
//! Diktor は、文字列のトークンと AST の中の文字列を、この規則でエスケープして引用符の中に書く。
//! 規則は次のとおりである。
//!
//! - `"` と `\` は、前に `\` を付けて `\"` と `\\` にする。
//! - 改行、タブ、復帰(CR)、バックスペース(0x08)は、2 文字の `\n`、`\t`、`\r`、`\b` にする。
//! - それ以外の 0x20〜0x7E のバイトは、そのまま書く。
//! - 残りのバイトは、`\` と 10 進 3 桁にする。UTF-8 のマルチバイト文字も、1 バイトずつこの形になる。

/// バイト列を `String.escaped` と同じ規則でエスケープした文字列を返す。
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
