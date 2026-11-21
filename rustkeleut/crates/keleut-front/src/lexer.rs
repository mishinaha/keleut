//! 字句解析である(Diktor の lexer.ml に対応する)。
//!
//! Diktor と同じ 3 つの層を持つ。
//!
//! | 層 | 関数 | 処理 |
//! |---|---|---|
//! | 1. 生トークン | [`Lexer::read_raw_token`] | 文字をトークンに変える。`{` は分類しない |
//! | 2. 再分類 | `pop_reclassified` | `{` をブロック、レコード、レコード型のどれかに決める |
//! | 3. ASI | `read_token` | region スタックを調べて、改行を文の区切りのトークンとして残すか、捨てるかを決める |
//!
//! 層 1 は、sedlex が作るオートマトンと同じ順に文字を読む。
//! sedlex は最長一致を探すので、受理状態に達した後も、一致を伸ばせる限り次の文字を読む。
//! 伸ばせなくなったら、最後に受理状態だった位置に戻る(`mark` と `backtrack`)。
//! この先読みで読んだ文字が不正な UTF-8 なら、その時点で字句エラーになる。
//! 本モジュールの各読み取りは、この読み方を手で移植したもので、受理状態に達したときに `mark` を、一致を伸ばせなくなったときに `backtrack` を呼ぶ。
//! こうすると、不正な UTF-8 の字句エラーが出るタイミングと位置が Diktor と一致する。
//!
//! 入力は、最初に全体を UTF-8 として検査し、正しい UTF-8 である先頭部分をコードポイントの配列にする。
//! その先頭部分より後ろを読もうとした時点で、`不正な UTF-8 バイト列です` の字句エラーを返す。
//! sedlex のデコーダも、正しい先頭部分の文字を先に渡し、不正なバイトまで読み進んだときに失敗する。

use crate::pos::{FileId, Pos};
use crate::token::{Tok, keyword_or_ident, parse_number};
use std::collections::VecDeque;

/// 文字列リテラルが閉じていないときの字句エラーのメッセージ。
/// REPL は、このメッセージの字句エラーを、入力がまだ途中であることを示す目印として使い、続きの行を待つ。
pub const UNTERMINATED_TEXT_MSG: &str = "文字列リテラルが閉じていません";
/// ブロックコメントが閉じていないときの字句エラーのメッセージ。
/// REPL は、このメッセージの字句エラーも、入力がまだ途中であることを示す目印として使う。
pub const UNTERMINATED_COMMENT_MSG: &str = "ブロックコメントが閉じていません";
/// 入力が不正な UTF-8 を含むときの字句エラーのメッセージ。
pub const MALFORMED_MSG: &str = "不正な UTF-8 バイト列です";

/// 字句エラーである(Diktor の `Lex_error` に対応する)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    /// エラーメッセージ。
    pub msg: String,
    /// エラーを報告する位置。
    pub pos: Pos,
}

/// 層 1 の内部で使う失敗の種類である。
/// 不正な UTF-8 の失敗(`Malformed`)は位置を持たず、[`Lexer::read_raw_token`] が位置を付けて [`LexError`] に変換する。
enum Fail {
    Lex(String, Pos),
    Malformed,
}

type R<T> = Result<T, Fail>;

/// sedlex の lexbuf に相当する、読み取り位置の状態である。
struct LexBuf {
    chars: Vec<char>,
    /// 正しい UTF-8 である先頭部分の後ろに、不正な UTF-8 が続いていれば真。
    malformed: bool,
    file: FileId,
    pos: usize,
    line: u32,
    bol: usize,
    start_pos: usize,
    start_line: u32,
    start_bol: usize,
    marked_pos: usize,
    marked_line: u32,
    marked_bol: usize,
    /// 1 行目の桁に加えるずれ。
    /// REPL の `:type` の後ろに書いた式を読むときに、位置の表示を入力行の中の桁にするために使う。
    col0: u32,
}

impl LexBuf {
    fn new(src: &[u8], file: FileId, line: u32, col0: u32) -> LexBuf {
        let (valid, malformed) = match std::str::from_utf8(src) {
            Ok(s) => (s, false),
            Err(e) => (
                // `valid_up_to` までは正しい UTF-8 である。
                std::str::from_utf8(&src[..e.valid_up_to()]).unwrap_or(""),
                true,
            ),
        };
        LexBuf {
            chars: valid.chars().collect(),
            malformed,
            file,
            pos: 0,
            line,
            bol: 0,
            start_pos: 0,
            start_line: line,
            start_bol: 0,
            marked_pos: 0,
            marked_line: line,
            marked_bol: 0,
            col0,
        }
    }

    /// 次の文字を読む。
    /// 入力の終わりなら `None` を返し、不正な UTF-8 まで読み進んだら `Fail::Malformed` を返す。
    fn next(&mut self) -> R<Option<char>> {
        if self.pos == self.chars.len() {
            return if self.malformed {
                Err(Fail::Malformed)
            } else {
                Ok(None)
            };
        }
        let c = self.chars[self.pos];
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.bol = self.pos;
        }
        Ok(Some(c))
    }

    fn mark(&mut self) {
        self.marked_pos = self.pos;
        self.marked_line = self.line;
        self.marked_bol = self.bol;
    }

    /// 一致を探し始める(sedlex の `start` に対応する)。
    fn start(&mut self) {
        self.start_pos = self.pos;
        self.start_line = self.line;
        self.start_bol = self.bol;
        self.mark();
    }

    fn backtrack(&mut self) {
        self.pos = self.marked_pos;
        self.line = self.marked_line;
        self.bol = self.marked_bol;
    }

    fn rollback(&mut self) {
        self.pos = self.start_pos;
        self.line = self.start_line;
        self.bol = self.start_bol;
    }

    /// 行番号、行頭の位置、読み取り位置から [`Pos`] を作る。
    /// 1 行目の桁は `col0` だけずらす(Diktor の REPL が `pos_bol` を `-col` にするのと同じ結果になる)。
    fn pos_at(&self, line: u32, bol: usize, pos: usize) -> Pos {
        Pos {
            file: self.file,
            line,
            bol: if bol == 0 { 0 } else { bol as u32 + self.col0 },
            cnum: pos as u32 + self.col0,
        }
    }

    fn start_p(&self) -> Pos {
        self.pos_at(self.start_line, self.start_bol, self.start_pos)
    }

    fn curr_p(&self) -> Pos {
        self.pos_at(self.line, self.bol, self.pos)
    }

    fn lexeme(&self) -> String {
        self.chars[self.start_pos..self.pos].iter().collect()
    }

    fn lexeme_char(&self, i: usize) -> char {
        self.chars[self.start_pos + i]
    }

    /// 文字が条件 `ok` を満たす限り読み進める。
    /// オートマトンでは、受理状態から同じ状態に戻る遷移に相当する。
    /// 呼び出す時点の位置が受理状態であることを前提にする。
    fn star(&mut self, ok: impl Fn(char) -> bool) -> R<()> {
        loop {
            self.mark();
            match self.next()? {
                Some(c) if ok(c) => {}
                _ => {
                    self.backtrack();
                    return Ok(());
                }
            }
        }
    }
}

fn is_idchar(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// トークンと、その開始位置と終了位置である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// トークンの種類と内容。
    pub tok: Tok,
    /// 開始位置。
    pub sp: Pos,
    /// 終了位置。
    pub ep: Pos,
}

/// ASI の region の種類である。
/// 層 3 は、最も内側の region が `Suppress` か `Clause` のとき、改行のトークンを捨てる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Top,
    Block,
    Suppress,
    /// `case` から、その節の `=>` までの範囲。
    /// 対応する `=>` がまだ現れていない `fn` の個数を持つ。
    Clause(u32),
}

/// 直前に渡したトークンの分類(`prev`)である。
/// ASI の判定に使うのはトークンの分類だけなので、トークンの内容は持たない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prev {
    Nl,
    /// 文を終えられるトークン。
    End,
    Import,
    /// それ以外のトークン。
    Other,
}

fn can_end_statement(t: &Tok) -> bool {
    use Tok::*;
    matches!(
        t,
        RPAREN
            | RBRACKET
            | RBRACE
            | LOWER_IDENTIFIER(_)
            | UPPER_IDENTIFIER(_)
            | HASH_IDENT(_)
            | NUMBER(_)
            | TEXT(_)
            | BOOL(_)
            | HOLE
    )
}

fn can_begin_statement(t: &Tok) -> bool {
    use Tok::*;
    matches!(
        t,
        LET | TYPE
            | NEWTYPE
            | EFFECT
            | MODULE
            | PUB
            | EXTERN
            | VAL
            | WITH
            | PERFORM
            | RESUME
            | RUN
            | FN
            | FROM
            | IF
            | LOWER_IDENTIFIER(_)
            | UPPER_IDENTIFIER(_)
            | HASH_IDENT(_)
            | NUMBER(_)
            | TEXT(_)
            | BOOL(_)
            | HOLE
            | EXCLAMATION
            | LBRACE_BLOCK
            | LBRACE_RECORD
            | LBRACE_TYPE
            | LPAREN
    )
}

fn prev_of(t: &Tok) -> Prev {
    match t {
        Tok::NL => Prev::Nl,
        Tok::IMPORT => Prev::Import,
        t if can_end_statement(t) => Prev::End,
        _ => Prev::Other,
    }
}

/// 字句解析器の状態である(Diktor の `Lexer.Make(...).t` に対応する)。
pub struct Lexer {
    lb: LexBuf,
    pending: VecDeque<Entry>,
    regions: Vec<Region>,
    pub(crate) prev: Option<Prev>,
    /// 直前に渡したトークンの開始位置。
    pub last_sp: Pos,
    pub(crate) last_ep: Pos,
    pub(crate) before_ep: Pos,
    pub(crate) before_cont: bool,
    /// 直前に渡したトークンからさかのぼって、最大 3 個のトークン。新しいものが先頭である。
    pub(crate) recent: Vec<Entry>,
}

/// `Lexing.dummy_pos` に相当する位置である。行番号は 0 で、診断に表示されることはない。
const DUMMY: Pos = Pos {
    file: 0,
    line: 0,
    bol: 0,
    cnum: 0,
};

impl Lexer {
    /// ソースのバイト列から字句解析器を作る。
    pub fn new(src: &[u8], file: FileId) -> Lexer {
        Lexer::with_start(src, file, 1, 0)
    }

    /// ソースの 1 行目を `line` 行目として扱い、1 行目の桁を `col0` だけずらす字句解析器を作る。
    /// Diktor の REPL の `try_parse` が呼ぶ `Sedlexing.set_position` に対応する。
    pub fn with_start(src: &[u8], file: FileId, line: u32, col0: u32) -> Lexer {
        Lexer {
            lb: LexBuf::new(src, file, line, col0),
            pending: VecDeque::new(),
            regions: vec![Region::Top],
            prev: None,
            last_sp: DUMMY,
            last_ep: DUMMY,
            before_ep: DUMMY,
            before_cont: false,
            recent: Vec::new(),
        }
    }

    fn here(&self, tok: Tok) -> Entry {
        Entry {
            tok,
            sp: self.lb.start_p(),
            ep: self.lb.curr_p(),
        }
    }

    fn lex_err<T>(&self, msg: &str) -> R<T> {
        Err(Fail::Lex(msg.to_string(), self.lb.start_p()))
    }

    // ---- 生トークン層 ----

    /// 入れ子にできるブロックコメントを終わりまで読み、改行を含んでいたら真を返す。
    /// 呼び出す時点で、先頭の `/*` を読み終えている。
    fn read_block_comment(&mut self) -> R<bool> {
        let mut saw_nl = false;
        let mut depth = 1u64;
        while depth > 0 {
            self.lb.start();
            match self.lb.next()? {
                Some('/') => {
                    self.lb.mark();
                    if self.lb.next()? == Some('*') {
                        depth += 1;
                    } else {
                        self.lb.backtrack();
                    }
                }
                Some('*') => {
                    self.lb.mark();
                    if self.lb.next()? == Some('/') {
                        depth -= 1;
                    } else {
                        self.lb.backtrack();
                    }
                }
                Some('\n') => saw_nl = true,
                None => return self.lex_err(UNTERMINATED_COMMENT_MSG),
                Some(_) => {}
            }
        }
        Ok(saw_nl)
    }

    /// 連続する改行と空白とコメントを読み飛ばす。
    fn skip_newlines(&mut self) -> R<()> {
        loop {
            self.lb.start();
            match self.lb.next()? {
                Some('\n') => {}
                Some(' ' | '\t' | '\r') => self.lb.star(|c| matches!(c, ' ' | '\t' | '\r'))?,
                Some('/') => match self.lb.next()? {
                    Some('/') => self.lb.star(|c| c != '\n')?,
                    Some('*') => {
                        self.read_block_comment()?;
                    }
                    _ => {
                        self.lb.backtrack();
                        self.lb.rollback();
                        return Ok(());
                    }
                },
                _ => {
                    self.lb.backtrack();
                    self.lb.rollback();
                    return Ok(());
                }
            }
        }
    }

    fn skip_rest_of_line(&mut self) -> R<()> {
        self.lb.start();
        self.lb.star(|c| c != '\n')
    }

    fn read_unicode_escape(&mut self, limit: u32) -> R<char> {
        let mut acc: u32 = 0;
        for _ in 0..limit {
            self.lb.start();
            let v = match self.lb.next()? {
                Some(c @ '0'..='9') => c as u32 - '0' as u32,
                Some(c @ 'a'..='f') => c as u32 - 'a' as u32 + 10,
                Some(c @ 'A'..='F') => c as u32 - 'A' as u32 + 10,
                _ => {
                    self.lb.backtrack();
                    return self.lex_err("Unicode エスケープの 16 進の桁が足りません");
                }
            };
            acc = acc * 16 + v;
        }
        match char::from_u32(acc) {
            Some(c) => Ok(c),
            None => self.lex_err("Unicode エスケープの値がサロゲートか、U+10FFFF を超えています"),
        }
    }

    fn read_escape(&mut self) -> R<char> {
        self.lb.start();
        match self.lb.next()? {
            Some('u') => self.read_unicode_escape(4),
            Some('U') => self.read_unicode_escape(8),
            Some('\'') => Ok('\''),
            Some('"') => Ok('"'),
            Some('\\') => Ok('\\'),
            Some('a') => Ok('\x07'),
            Some('b') => Ok('\x08'),
            Some('f') => Ok('\x0c'),
            Some('n') => Ok('\n'),
            Some('r') => Ok('\r'),
            Some('t') => Ok('\t'),
            Some('v') => Ok('\x0b'),
            _ => {
                self.lb.backtrack();
                self.lex_err("不正なエスケープです")
            }
        }
    }

    fn read_text(&mut self) -> R<String> {
        let mut buf = String::new();
        loop {
            self.lb.start();
            match self.lb.next()? {
                Some('"') => return Ok(buf),
                None => return self.lex_err(UNTERMINATED_TEXT_MSG),
                Some('\\') => {
                    let c = self.read_escape()?;
                    buf.push(c);
                }
                Some(c) => buf.push(c),
            }
        }
    }

    /// 数値のトークンを読む。
    /// 4 つの正規表現(10 進、16 進、8 進、2 進)のオートマトンを同時に進め、どれかが受理した最も長い一致を採る。
    /// 呼び出す時点で、先頭の数字 `first` を読み終えている。
    fn read_number(&mut self, first: char) -> R<()> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum D {
            Int,
            Dot,
            Frac,
            E,
            ESign,
            Exp,
            Suf,
            SufDigits,
        }
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum P {
            /// `0` を読んだ状態。次に基数の文字を待つ。
            Zero,
            /// 基数の文字を読んだ状態。次に最初の桁を待つ。
            Prefix,
            /// 桁の並びを読んでいる状態。受理状態である。
            Digits,
            /// 接尾辞の文字を読んだ状態。
            Suf,
            /// 接尾辞の桁を読んでいる状態。受理状態である。
            SufD,
        }
        let mut dec = Some(D::Int);
        // 16 進、8 進、2 進の順に並べた状態と、基数ごとの桁の判定。
        let mut radix: [Option<P>; 3] = if first == '0' {
            [Some(P::Zero); 3]
        } else {
            [None; 3]
        };
        let letters = ['x', 'o', 'b'];
        let digit_ok = [
            is_hex as fn(char) -> bool,
            |c| ('0'..='7').contains(&c),
            |c| c == '0' || c == '1',
        ];
        let dec_final =
            |d: Option<D>| matches!(d, Some(D::Int | D::Dot | D::Frac | D::Exp | D::SufDigits));
        let p_final = |p: Option<P>| matches!(p, Some(P::Digits | P::SufD));
        loop {
            if dec_final(dec) || radix.iter().any(|&p| p_final(p)) {
                self.lb.mark();
            }
            let c = self.lb.next()?;
            let nd = match (dec, c) {
                (Some(D::Int), Some(c)) if c.is_ascii_digit() || c == '_' => Some(D::Int),
                (Some(D::Int), Some('.')) => Some(D::Dot),
                (Some(D::Int | D::Dot | D::Frac), Some('e' | 'E')) => Some(D::E),
                (Some(D::Int | D::Dot | D::Frac | D::Exp), Some('i' | 'u' | 'f')) => Some(D::Suf),
                (Some(D::Dot), Some(c)) if c.is_ascii_digit() => Some(D::Frac),
                (Some(D::Frac), Some(c)) if c.is_ascii_digit() || c == '_' => Some(D::Frac),
                (Some(D::E), Some('+' | '-')) => Some(D::ESign),
                (Some(D::E | D::ESign | D::Exp), Some(c)) if c.is_ascii_digit() => Some(D::Exp),
                (Some(D::Suf | D::SufDigits), Some(c)) if c.is_ascii_digit() => Some(D::SufDigits),
                _ => None,
            };
            let mut nr = [None; 3];
            for k in 0..3 {
                nr[k] = match (radix[k], c) {
                    (Some(P::Zero), Some(c)) if c == letters[k] => Some(P::Prefix),
                    (Some(P::Prefix), Some(c)) if digit_ok[k](c) => Some(P::Digits),
                    (Some(P::Digits), Some(c)) if digit_ok[k](c) || c == '_' => Some(P::Digits),
                    (Some(P::Digits), Some('i' | 'u')) => Some(P::Suf),
                    (Some(P::Suf | P::SufD), Some(c)) if c.is_ascii_digit() => Some(P::SufD),
                    _ => None,
                };
            }
            dec = nd;
            radix = nr;
            if dec.is_none() && radix.iter().all(|p| p.is_none()) {
                self.lb.backtrack();
                return Ok(());
            }
        }
    }

    fn read_raw_token_inner(&mut self) -> R<Entry> {
        use Tok::*;
        loop {
            self.lb.start();
            let Some(c) = self.lb.next()? else {
                return Ok(self.here(EOF));
            };
            // 1 文字で決まり、それ以上は伸ばせないトークン。
            let single = match c {
                '(' => Some(LPAREN),
                ')' => Some(RPAREN),
                '[' => Some(LBRACKET),
                ']' => Some(RBRACKET),
                '{' => Some(LBRACE_BLOCK),
                '}' => Some(RBRACE),
                ':' => Some(COLON),
                ',' => Some(COMMA),
                ';' => Some(SEMI),
                '@' => Some(AT),
                '+' => Some(PLUS),
                '-' => Some(HYPHEN),
                '*' => Some(ASTERISK),
                '%' => Some(PERCENT),
                '\\' => Some(BACKSLASH),
                _ => None,
            };
            if let Some(t) = single {
                return Ok(self.here(t));
            }
            match c {
                ' ' | '\t' | '\r' => {
                    self.lb.star(|c| matches!(c, ' ' | '\t' | '\r'))?;
                    continue;
                }
                '\n' => {
                    let e = self.here(NL);
                    self.skip_newlines()?;
                    return Ok(e);
                }
                '/' => {
                    self.lb.mark();
                    match self.lb.next()? {
                        Some('/') => {
                            self.lb.star(|c| c != '\n')?;
                            continue;
                        }
                        Some('*') => {
                            if self.read_block_comment()? {
                                let e = self.here(NL);
                                self.skip_newlines()?;
                                return Ok(e);
                            }
                            continue;
                        }
                        _ => {
                            self.lb.backtrack();
                            return Ok(self.here(SOLIDUS));
                        }
                    }
                }
                '#' => {
                    self.lb.mark();
                    match self.lb.next()? {
                        Some('!') => {
                            if self.lb.start_pos == 0 {
                                self.skip_rest_of_line()?;
                                continue;
                            }
                            return self.lex_err("#! はファイルの先頭にだけ書けます");
                        }
                        Some('A'..='Z') => {
                            self.lb.star(is_idchar)?;
                            let s = self.lb.lexeme();
                            return Ok(self.here(HASH_IDENT(s[1..].to_string())));
                        }
                        _ => {
                            self.lb.backtrack();
                            return self.unexpected();
                        }
                    }
                }
                '?' => {
                    self.lb.mark();
                    if self.lb.next()? == Some('?') && self.lb.next()? == Some('?') {
                        return Ok(self.here(HOLE));
                    }
                    self.lb.backtrack();
                    return self.unexpected();
                }
                '.' => {
                    self.lb.mark();
                    if self.lb.next()? == Some('.') && self.lb.next()? == Some('.') {
                        return Ok(self.here(DOTDOTDOT));
                    }
                    self.lb.backtrack();
                    return Ok(self.here(DOT));
                }
                '&' => {
                    self.lb.mark();
                    if self.lb.next()? == Some('&') {
                        return Ok(self.here(BIG_AMPERSAND));
                    }
                    self.lb.backtrack();
                    return self.unexpected();
                }
                '=' | '!' | '<' | '>' | '|' => {
                    self.lb.mark();
                    let n = self.lb.next()?;
                    let t = match (c, n) {
                        ('=', Some('>')) => Some(EQ_GREATER),
                        ('=', Some('=')) => Some(BIG_EQ),
                        ('!', Some('=')) => Some(EXCLAMATION_EQ),
                        ('<', Some('=')) => Some(LESS_EQ),
                        ('>', Some('=')) => Some(GREATER_EQ),
                        ('|', Some('|')) => Some(BIG_VERTICAL),
                        _ => None,
                    };
                    if let Some(t) = t {
                        return Ok(self.here(t));
                    }
                    self.lb.backtrack();
                    let t = match c {
                        '=' => EQ,
                        '!' => EXCLAMATION,
                        '<' => LESS,
                        '>' => GREATER,
                        _ => VERTICAL,
                    };
                    return Ok(self.here(t));
                }
                'A'..='Z' => {
                    self.lb.star(is_idchar)?;
                    let s = self.lb.lexeme();
                    return Ok(self.here(UPPER_IDENTIFIER(s)));
                }
                'a'..='z' | '_' => {
                    self.lb.star(is_idchar)?;
                    let s = self.lb.lexeme();
                    return Ok(self.here(keyword_or_ident(&s)));
                }
                '0'..='9' => {
                    self.read_number(c)?;
                    let s = self.lb.lexeme();
                    return Ok(self.here(NUMBER(parse_number(&s))));
                }
                '"' => {
                    let sp = self.lb.start_p();
                    let s = self.read_text()?;
                    let ep = self.lb.curr_p();
                    return Ok(Entry {
                        tok: TEXT(s),
                        sp,
                        ep,
                    });
                }
                _ => return self.unexpected(),
            }
        }
    }

    /// どの規則にも一致しない文字を読んだときの字句エラーを作る(sedlex の `any` の分岐に相当する)。
    /// 目に見えない文字は、`U+XXXX` の形式で表示する。
    fn unexpected<T>(&self) -> R<T> {
        let cp = self.lb.lexeme_char(0) as u32;
        let invisible = cp == 0xFEFF
            || cp < 0x20
            || (0x7F..=0xA0).contains(&cp)
            || cp == 0xAD
            || (0x200B..=0x200F).contains(&cp)
            || (0x2028..=0x202E).contains(&cp)
            || (0x2060..=0x2064).contains(&cp);
        let shown = if invisible {
            format!("U+{cp:04X}")
        } else {
            self.lb.lexeme()
        };
        self.lex_err(&format!("予期しない文字です: {shown}"))
    }

    /// 生トークンを 1 つ読む。
    /// 不正な UTF-8 まで読み進んだときは、その時点の読み取り位置を付けた字句エラーを返す。
    pub fn read_raw_token(&mut self) -> Result<Entry, LexError> {
        self.read_raw_token_inner().map_err(|f| match f {
            Fail::Lex(msg, pos) => LexError { msg, pos },
            Fail::Malformed => LexError {
                msg: MALFORMED_MSG.to_string(),
                pos: self.lb.curr_p(),
            },
        })
    }

    // ---- 再分類層 + ASI 層 ----

    fn peek(&mut self, i: usize) -> Result<&Entry, LexError> {
        while self.pending.len() <= i {
            let e = self.read_raw_token()?;
            self.pending.push_back(e);
        }
        Ok(&self.pending[i])
    }

    /// `NL` を読み飛ばして数えた、0 始まりで `k` 番目のトークンを先読みする。
    fn peek_sig(&mut self, k: usize) -> Result<Tok, LexError> {
        let mut i = 0;
        let mut k = k;
        loop {
            let t = &self.peek(i)?.tok;
            if *t != Tok::NL {
                if k == 0 {
                    return Ok(t.clone());
                }
                k -= 1;
            }
            i += 1;
        }
    }

    fn classify_brace(&mut self) -> Result<Tok, LexError> {
        use Tok::*;
        let t1 = self.peek_sig(0)?;
        if self.prev == Some(Prev::Import) {
            return Ok(LBRACE_RECORD);
        }
        Ok(match t1 {
            RBRACE | EXTENDS => LBRACE_RECORD,
            LOWER_IDENTIFIER(_) | UPPER_IDENTIFIER(_) => match self.peek_sig(1)? {
                COLON => LBRACE_TYPE,
                EQ | COMMA | WITH | EXTENDS => LBRACE_RECORD,
                _ => LBRACE_BLOCK,
            },
            _ => LBRACE_BLOCK,
        })
    }

    fn pop_reclassified(&mut self) -> Result<Entry, LexError> {
        let e = match self.pending.pop_front() {
            Some(e) => e,
            None => self.read_raw_token()?,
        };
        if e.tok == Tok::LBRACE_BLOCK {
            let tok = self.classify_brace()?;
            return Ok(Entry { tok, ..e });
        }
        Ok(e)
    }

    fn read_token(&mut self) -> Result<Entry, LexError> {
        use Tok::*;
        loop {
            let e = self.pop_reclassified()?;
            if e.tok == NL {
                match self.regions.last() {
                    Some(Region::Suppress | Region::Clause(_)) => continue,
                    _ => {
                        let keep = match self.prev {
                            Some(p) => p == Prev::End && can_begin_statement(&self.peek_sig(0)?),
                            None => false,
                        };
                        if keep {
                            self.prev = Some(Prev::Nl);
                            return Ok(e);
                        }
                        continue;
                    }
                }
            }
            let n = self.regions.len();
            match &e.tok {
                LPAREN | LBRACKET | LBRACE_RECORD | LBRACE_TYPE => {
                    self.regions.push(Region::Suppress)
                }
                LBRACE_BLOCK => self.regions.push(Region::Block),
                RPAREN | RBRACKET | RBRACE => {
                    if n >= 2 && matches!(self.regions[n - 1], Region::Clause(_)) {
                        self.regions.pop();
                    }
                    if self.regions.len() >= 2 {
                        self.regions.pop();
                    }
                }
                CASE => self.regions.push(Region::Clause(0)),
                FN => {
                    if let Some(Region::Clause(k)) = self.regions.last_mut() {
                        *k += 1;
                    }
                }
                EQ_GREATER => {
                    if n >= 2
                        && let Region::Clause(k) = self.regions[n - 1]
                    {
                        if k > 0 {
                            self.regions[n - 1] = Region::Clause(k - 1);
                        } else {
                            self.regions.pop();
                        }
                    }
                }
                _ => {}
            }
            self.prev = Some(prev_of(&e.tok));
            return Ok(e);
        }
    }

    // ---- パーサ向けの API ----

    /// パーサに渡す次のトークンを読む(Diktor の `read` に対応する)。
    pub fn read(&mut self) -> Result<Entry, LexError> {
        self.before_ep = self.last_ep;
        self.before_cont = matches!(
            self.regions.last(),
            Some(Region::Suppress | Region::Clause(_))
        ) || matches!(self.prev, Some(Prev::Import | Prev::Other));
        let e = self.read_token()?;
        self.last_sp = e.sp;
        self.last_ep = e.ep;
        if self.recent.len() >= 3 {
            self.recent.truncate(2);
        }
        self.recent.insert(0, e.clone());
        Ok(e)
    }

    /// 文法に合わなかったトークンについて、構文エラーを報告する位置を返す(Diktor の `error_positions` に対応する)。
    /// 直前のトークンの後の改行が文の区切りにならない状態(`before_cont`)で、行が変わってからそのトークンが現れたときは、直前のトークンの終了位置と、そのトークンの開始位置の両方を返す。
    /// それ以外は、そのトークンの開始位置だけを返す。
    pub fn error_positions(&self) -> (Pos, Option<Pos>) {
        if self.before_cont && self.last_sp.line > self.before_ep.line {
            (self.before_ep, Some(self.last_sp))
        } else {
            (self.last_sp, None)
        }
    }

    /// ASI を適用した後のトークン列を、EOF まで集めて返す。
    /// `keleut dump tokens`(Diktor の `--dump-tokens`)の出力に使う。
    pub fn all_tokens(&mut self) -> Result<Vec<Entry>, LexError> {
        let mut v = Vec::new();
        loop {
            let e = self.read_token()?;
            let eof = e.tok == Tok::EOF;
            v.push(e);
            if eof {
                return Ok(v);
            }
        }
    }

    /// 数値リテラルの表記の誤りによって、数値とその直後のトークンに分かれた入力を検出する(Diktor の `glued_number` に対応する)。
    /// 検出したときは、数値、その開始位置、数値の直後に続く文字列を返す。
    pub fn glued_number(&self) -> Option<(crate::token::Number, Pos, String)> {
        let touches = |a: &Entry, b: &Entry| a.ep.cnum == b.sp.cnum;
        let wordlike = |t: &Tok| {
            let s = t.show();
            s.bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
        };
        let r = &self.recent;
        if r.len() >= 3
            && let (Tok::NUMBER(m), Tok::DOT, Tok::NUMBER(n)) = (&r[0].tok, &r[1].tok, &r[2].tok)
            && touches(&r[2], &r[1])
            && touches(&r[1], &r[0])
        {
            return Some((n.clone(), r[2].sp, format!(".{}", m.show())));
        }
        if r.len() >= 2
            && let Tok::NUMBER(n) = &r[1].tok
            && touches(&r[1], &r[0])
            && wordlike(&r[0].tok)
        {
            return Some((n.clone(), r[1].sp, r[0].tok.show()));
        }
        None
    }

    /// `read` が更新する状態のうち、直前のトークンと位置に関わるものを保存する。
    pub(crate) fn save(&self) -> Saved {
        Saved {
            prev: self.prev,
            last_sp: self.last_sp,
            last_ep: self.last_ep,
            before_ep: self.before_ep,
            before_cont: self.before_cont,
            recent: self.recent.clone(),
        }
    }

    /// [`Lexer::save`] で保存した状態に戻す。
    pub(crate) fn restore(&mut self, s: Saved) {
        self.prev = s.prev;
        self.last_sp = s.last_sp;
        self.last_ep = s.last_ep;
        self.before_ep = s.before_ep;
        self.before_cont = s.before_cont;
        self.recent = s.recent;
    }
}

/// [`Lexer::save`] が保存する状態である。
pub(crate) struct Saved {
    prev: Option<Prev>,
    last_sp: Pos,
    last_ep: Pos,
    before_ep: Pos,
    before_cont: bool,
    recent: Vec<Entry>,
}
