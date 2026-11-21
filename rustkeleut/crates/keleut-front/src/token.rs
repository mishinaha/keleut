//! トークンと、数値リテラルの表記を定義する(Diktor の lexer.ml の §2.1、§2.12、§2.13)。

use crate::escape::ocaml_escaped;

/// 数値リテラルの接尾辞の種類と幅である。
/// 幅 `N` を読めない接尾辞は、幅を -1 にする。
/// 読めないのは、`N` が 2 桁以上で先頭が 0 のときと、`N` を OCaml の `int` として読めないときである。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumSuffix {
    /// 接尾辞 `iN`。フィールドは幅 `N` である。
    Int(i64),
    /// 接尾辞 `uN`。フィールドは幅 `N` である。
    UInt(i64),
    /// 接尾辞 `fN`。フィールドは幅 `N` である。
    Float(i64),
}

/// 数値リテラルの表記である。
/// 数値には変換せず、ソースに書かれた文字列のまま持つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Number {
    /// 接尾辞を除いた本体の文字列。
    /// 前置の `-` をリテラルにまとめたときは、先頭に `-` が付く。
    pub text: String,
    /// 浮動小数点数のリテラルなら真。
    /// 接尾辞が `fN` のときと、基数の接頭辞のない本体に `.`、`e`、`E` のどれかがあるときである。
    pub is_float: bool,
    /// 接尾辞の種類と幅。接尾辞がなければ `None`。
    pub suffix: Option<NumSuffix>,
    /// 接尾辞の文字列(なければ空)。
    pub suffix_text: String,
}

impl Number {
    /// ソースに書かれた表記(本体と接尾辞をつないだ文字列)を返す。
    pub fn show(&self) -> String {
        format!("{}{}", self.text, self.suffix_text)
    }
}

/// 数値リテラルのトークンの文字列から、表記を作る(Diktor の `parse_number`)。
pub fn parse_number(text: &str) -> Number {
    let b = text.as_bytes();
    let len = b.len();
    let radix = len >= 2 && b[0] == b'0' && matches!(b[1], b'x' | b'o' | b'b');
    let is_suffix = |c: u8| c == b'i' || c == b'u' || (c == b'f' && !radix);
    let from = if radix { 2 } else { 0 };
    let suffix_at = (from..len).find(|&i| is_suffix(b[i]));
    let lexically_float =
        |body: &str| !radix && body.bytes().any(|c| matches!(c, b'.' | b'e' | b'E'));
    match suffix_at {
        None => Number {
            text: text.to_string(),
            is_float: lexically_float(text),
            suffix: None,
            suffix_text: String::new(),
        },
        Some(i) => {
            let body = &text[..i];
            let raw = &text[i..];
            let digits = &text[i + 1..];
            let width = if digits.len() > 1 && digits.as_bytes()[0] == b'0' {
                -1
            } else {
                ocaml_int_of_string_opt(digits).unwrap_or(-1)
            };
            let suffix = match b[i] {
                b'i' => NumSuffix::Int(width),
                b'u' => NumSuffix::UInt(width),
                _ => NumSuffix::Float(width),
            };
            let is_float = match suffix {
                NumSuffix::Float(_) => true,
                _ => lexically_float(body),
            };
            Number {
                text: body.to_string(),
                is_float,
                suffix: Some(suffix),
                suffix_text: raw.to_string(),
            }
        }
    }
}

/// 10 進の数字だけの文字列を、OCaml の `int`(63 ビット)として読む。
/// 空の文字列、数字以外の文字を含む文字列、範囲を超える値には `None` を返す。
pub fn ocaml_int_of_string_opt(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut v: i64 = 0;
    for c in digits.bytes() {
        v = v.checked_mul(10)?.checked_add((c - b'0') as i64)?;
        if v > (1i64 << 62) - 1 {
            return None;
        }
    }
    Some(v)
}

/// トークンである。
/// バリアントの名前は Diktor の parser.mly の `%token` と同じである。
/// [`Tok::name`] が返す名前を、LR の表から終端記号を引くキーに使う。
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms, non_camel_case_types)]
pub enum Tok {
    AND,
    AT,
    ASTERISK,
    BACKSLASH,
    BIG_AMPERSAND,
    BIG_EQ,
    BIG_VERTICAL,
    BOOL(bool),
    CASE,
    CLASS,
    COLON,
    COMMA,
    DOT,
    DOTDOTDOT,
    EFFECT,
    ELSE,
    EOF,
    EQ,
    EQ_GREATER,
    EXCLAMATION,
    EXCLAMATION_EQ,
    EXTENDS,
    EXTERN,
    FN,
    FROM,
    GREATER,
    GREATER_EQ,
    HANDLE,
    HASH_IDENT(String),
    HOLE,
    HYPHEN,
    IF,
    IMPORT,
    INSTANCE,
    LBRACE_BLOCK,
    LBRACE_RECORD,
    LBRACE_TYPE,
    LBRACKET,
    LESS,
    LESS_EQ,
    LET,
    LOWER_IDENTIFIER(String),
    LOWLINE,
    LPAREN,
    MATCH,
    MODULE,
    NEWTYPE,
    NL,
    NUMBER(Number),
    PERFORM,
    PERCENT,
    PLUS,
    PUB,
    RBRACE,
    RBRACKET,
    REC,
    RESUME,
    RPAREN,
    RUN,
    SEMI,
    SOLIDUS,
    TEXT(String),
    THEN,
    TYPE,
    UPPER_IDENTIFIER(String),
    VAL,
    VERTICAL,
    WITH,
}

impl Tok {
    /// parser.mly の終端記号の名前を返す。
    pub fn name(&self) -> &'static str {
        use Tok::*;
        match self {
            AND => "AND",
            AT => "AT",
            ASTERISK => "ASTERISK",
            BACKSLASH => "BACKSLASH",
            BIG_AMPERSAND => "BIG_AMPERSAND",
            BIG_EQ => "BIG_EQ",
            BIG_VERTICAL => "BIG_VERTICAL",
            BOOL(_) => "BOOL",
            CASE => "CASE",
            CLASS => "CLASS",
            COLON => "COLON",
            COMMA => "COMMA",
            DOT => "DOT",
            DOTDOTDOT => "DOTDOTDOT",
            EFFECT => "EFFECT",
            ELSE => "ELSE",
            EOF => "EOF",
            EQ => "EQ",
            EQ_GREATER => "EQ_GREATER",
            EXCLAMATION => "EXCLAMATION",
            EXCLAMATION_EQ => "EXCLAMATION_EQ",
            EXTENDS => "EXTENDS",
            EXTERN => "EXTERN",
            FN => "FN",
            FROM => "FROM",
            GREATER => "GREATER",
            GREATER_EQ => "GREATER_EQ",
            HANDLE => "HANDLE",
            HASH_IDENT(_) => "HASH_IDENT",
            HOLE => "HOLE",
            HYPHEN => "HYPHEN",
            IF => "IF",
            IMPORT => "IMPORT",
            INSTANCE => "INSTANCE",
            LBRACE_BLOCK => "LBRACE_BLOCK",
            LBRACE_RECORD => "LBRACE_RECORD",
            LBRACE_TYPE => "LBRACE_TYPE",
            LBRACKET => "LBRACKET",
            LESS => "LESS",
            LESS_EQ => "LESS_EQ",
            LET => "LET",
            LOWER_IDENTIFIER(_) => "LOWER_IDENTIFIER",
            LOWLINE => "LOWLINE",
            LPAREN => "LPAREN",
            MATCH => "MATCH",
            MODULE => "MODULE",
            NEWTYPE => "NEWTYPE",
            NL => "NL",
            NUMBER(_) => "NUMBER",
            PERFORM => "PERFORM",
            PERCENT => "PERCENT",
            PLUS => "PLUS",
            PUB => "PUB",
            RBRACE => "RBRACE",
            RBRACKET => "RBRACKET",
            REC => "REC",
            RESUME => "RESUME",
            RPAREN => "RPAREN",
            RUN => "RUN",
            SEMI => "SEMI",
            SOLIDUS => "SOLIDUS",
            TEXT(_) => "TEXT",
            THEN => "THEN",
            TYPE => "TYPE",
            UPPER_IDENTIFIER(_) => "UPPER_IDENTIFIER",
            VAL => "VAL",
            VERTICAL => "VERTICAL",
            WITH => "WITH",
        }
    }

    /// Diktor の `--dump-tokens` と同じ表記を返す(Diktor の `show_token`)。
    pub fn show(&self) -> String {
        use Tok::*;
        let s = match self {
            AND => "and",
            AT => "@",
            ASTERISK => "*",
            BACKSLASH => "\\",
            BIG_AMPERSAND => "&&",
            BIG_EQ => "==",
            BIG_VERTICAL => "||",
            BOOL(b) => {
                if *b {
                    "true"
                } else {
                    "false"
                }
            }
            CASE => "case",
            CLASS => "class",
            COLON => ":",
            COMMA => ",",
            DOT => ".",
            DOTDOTDOT => "...",
            EFFECT => "effect",
            ELSE => "else",
            EOF => "<EOF>",
            EQ => "=",
            EQ_GREATER => "=>",
            EXCLAMATION => "!",
            EXCLAMATION_EQ => "!=",
            EXTENDS => "extends",
            EXTERN => "extern",
            FN => "fn",
            FROM => "from",
            GREATER => ">",
            GREATER_EQ => ">=",
            HANDLE => "handle",
            HASH_IDENT(s) => return format!("#{s}"),
            HOLE => "???",
            HYPHEN => "-",
            IF => "if",
            IMPORT => "import",
            INSTANCE => "instance",
            LBRACE_BLOCK => "{blk",
            LBRACE_RECORD => "{rec",
            LBRACE_TYPE => "{ty",
            LBRACKET => "[",
            LESS => "<",
            LESS_EQ => "<=",
            LET => "let",
            LOWER_IDENTIFIER(s) => return s.clone(),
            LOWLINE => "_",
            LPAREN => "(",
            MATCH => "match",
            MODULE => "module",
            NEWTYPE => "newtype",
            NL => "<NL>",
            NUMBER(n) => return n.show(),
            PERFORM => "perform",
            PERCENT => "%",
            PLUS => "+",
            PUB => "pub",
            RBRACE => "}",
            RBRACKET => "]",
            REC => "rec",
            RESUME => "resume",
            RPAREN => ")",
            RUN => "run",
            SEMI => ";",
            SOLIDUS => "/",
            TEXT(s) => return format!("\"{}\"", ocaml_escaped(s.as_bytes())),
            THEN => "then",
            TYPE => "type",
            UPPER_IDENTIFIER(s) => return s.clone(),
            VAL => "val",
            VERTICAL => "|",
            WITH => "with",
        };
        s.to_string()
    }
}

/// 識別子として読んだ文字列がキーワードならそのトークンを、そうでなければ識別子のトークンを返す
/// (Diktor の `keyword_or_ident`)。
pub fn keyword_or_ident(id: &str) -> Tok {
    use Tok::*;
    match id {
        "_" => LOWLINE,
        "and" => AND,
        "case" => CASE,
        "class" => CLASS,
        "effect" => EFFECT,
        "else" => ELSE,
        "extends" => EXTENDS,
        "extern" => EXTERN,
        "false" => BOOL(false),
        "fn" => FN,
        "from" => FROM,
        "handle" => HANDLE,
        "if" => IF,
        "import" => IMPORT,
        "instance" => INSTANCE,
        "let" => LET,
        "match" => MATCH,
        "module" => MODULE,
        "newtype" => NEWTYPE,
        "perform" => PERFORM,
        "pub" => PUB,
        "rec" => REC,
        "resume" => RESUME,
        "run" => RUN,
        "then" => THEN,
        "true" => BOOL(true),
        "type" => TYPE,
        "val" => VAL,
        "with" => WITH,
        id => LOWER_IDENTIFIER(id.to_string()),
    }
}

/// トークンが予約語なら、その表記を返す(Diktor の `reserved_word`)。
/// `_` は予約語として扱わない。
pub fn reserved_word(tok: &Tok) -> Option<String> {
    let s = tok.show();
    match keyword_or_ident(&s) {
        Tok::LOWER_IDENTIFIER(_) | Tok::LOWLINE => None,
        t if &t == tok => Some(s),
        _ => None,
    }
}
