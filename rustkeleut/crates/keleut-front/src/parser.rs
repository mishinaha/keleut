//! 構文解析のドライバである(Diktor の driver.ml の §16.3 と §16.3a)。
//!
//! 表(`tables.rs`)は、menhir が Diktor の parser.mly から生成した表形式のオートマトンと同じものである。
//! ドライバは、menhir の表形式のバックエンド(MenhirLib の `Engine`)と同じ手順で動く。
//!
//! - shift の後は、次のトークンを読む。ただし、`#` での既定の還元を持つ状態に入るときは読まない。
//!   `#` は menhir が入力の終わりを表すのに使う特別な終端記号で、この状態では次のトークンを見ずに還元する。
//! - トークンを読んだ後は、既定の還元を持つ状態なら先読みを調べずに還元し、そうでなければ動作表を引く。
//!
//! 同じ手順で動くので、次の 3 つが Diktor と一致する。
//! トークンを読むタイミング(字句エラーが出るタイミング)、意味動作が構文エラーを返すタイミング、
//! 文法に合わないトークンを検出する位置である。
//!
//! 予約語を識別子の位置に書いたエラーかどうかの判別(Diktor の `misused_reserved_word`)では、
//! エラーの直前の状態に別のトークンを与えて、解析を進められるかを調べる。
//! menhir の状態は永続的なので、Diktor は直前の状態をそのまま使って試す。
//! 本モジュールは、ソースを先頭から字句解析し直し、それまでに読んだ個数のトークンを新しいオートマトンに与え直して、
//! 直前の状態を作り直す。
//! 字句解析と意味動作は決定的なので、作り直した状態は元の状態と同じになる。
//! この方法では、エラーのない入力でトークンを保存しておく必要がない。
//!
//! スタックはヒープ上に確保した配列なので、入れ子の深い入力でもネイティブスタックを使わない。
//! そのため、スタックの段数には上限を設けない。
//! 入れ子のない長い並び(トップレベルの宣言、ブロックの文、引数の並び)では、規則が右再帰なのでスタックが長くなるが、
//! Diktor(menhir の表形式のバックエンド)と同じく受理する。
//! 入れ子の深さとして数えるのは、意味動作が作る木の深さである([`Ctx`] が数える)。
//! 構文解析より後の段階は木を再帰でたどるので、木の深さが上限を超えたら、
//! 構文解析は `処理系の限度` の診断を返して失敗する。

use crate::actions::{self, ActErr, Ctx, Sv};
use crate::ast::Program;
use crate::lexer::{Entry, LexError, Lexer};
use crate::pos::{FileId, Files, Pos};
use crate::tables::{
    ACTION, DEFAULT_REDUCTION, ENTRY, GOTO, N_NONTERMINALS, N_TERMINALS, NO_DISCARD, PRODS,
    TERMINALS,
};
use crate::token::{Tok, reserved_word};
use std::sync::OnceLock;

/// 構文解析が失敗した理由である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// 字句エラー(終了コード 2)。
    Lex(LexError),
    /// 構文エラー(終了コード 2)。Diktor の `Parse_error` に当たる。
    /// 内容は、位置まで整形し終えた構文エラーの行か、入れ子が構文解析の上限を超えたときの `処理系の限度` の行である。
    Syntax(String),
    /// 内部エラー(終了コード 3)。
    /// Diktor が最上位の例外ハンドラの外の例外で終了する場合に合わせる。
    Internal(String),
}

const NONE: u16 = u16::MAX;

/// トークンに対応する終端記号の番号を返す。
/// 終端記号の名前から番号を引く表は、最初の呼び出しで作る。
fn terminal_of(tok: &Tok) -> usize {
    static MAP: OnceLock<std::collections::HashMap<&'static str, usize>> = OnceLock::new();
    let map = MAP.get_or_init(|| TERMINALS.iter().enumerate().map(|(i, n)| (*n, i)).collect());
    map[tok.name()]
}

/// スタックの 1 段である(menhir のスタックの cell)。
/// `state` は、この段を積む直前にオートマトンがいた状態である。
struct Cell {
    state: u16,
    sv: Sv,
    sp: Pos,
    ep: Pos,
}

/// オートマトンにトークンを与えた結果である。
#[allow(clippy::large_enum_variant)]
enum Step {
    /// 次のトークンが必要である。
    NeedInput,
    /// 入力を受理した。
    Accepted(Sv),
    /// トークンが文法に合わない。
    Error,
}

/// LR オートマトンの実行状態である。
/// スタックと現在の状態を持つ。
struct Machine {
    stack: Vec<Cell>,
    current: u16,
    /// 還元のたびに、右辺の要素をスタックから移すバッファ。
    /// 確保済みの領域を再利用する。
    buf: Vec<(Sv, Pos, Pos)>,
}

/// オートマトンの失敗である。
/// 意味動作のエラーか、木の深さが上限を超えたことを表す。
enum MErr {
    Act(ActErr),
    Limit,
}

impl From<ActErr> for MErr {
    fn from(e: ActErr) -> MErr {
        MErr::Act(e)
    }
}

impl Machine {
    fn new() -> Machine {
        Machine {
            stack: Vec::new(),
            current: ENTRY,
            buf: Vec::new(),
        }
    }

    fn reduce(&mut self, cx: &mut Ctx, p: usize) -> Result<(), MErr> {
        let prod = &PRODS[p];
        let n = prod.len as usize;
        let at = self.stack.len() - n;
        let (base, sp, ep) = if n > 0 {
            (
                self.stack[at].state,
                self.stack[at].sp,
                self.stack[n - 1 + at].ep,
            )
        } else {
            let e = self.stack.last().map_or(Pos::START, |c| c.ep);
            (self.current, e, e)
        };
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();
        buf.extend(self.stack.drain(at..).map(|c| (c.sv, c.sp, c.ep)));
        let mut args = actions::Args::new(buf, (sp, ep));
        let sv = actions::run(p, cx, &mut args);
        self.buf = args.into_vec();
        let sv = sv?;
        if cx.over {
            sv.dismantle();
            return Err(MErr::Limit);
        }
        let target = GOTO[base as usize * N_NONTERMINALS + prod.lhs as usize];
        debug_assert_ne!(target, NONE);
        self.stack.push(Cell {
            state: base,
            sv,
            sp,
            ep,
        });
        self.current = target;
        Ok(())
    }

    /// トークンを与え、次のトークンが必要になるか、入力を受理するか、エラーになるまで進める。
    /// `stop_at_shift` が真のときは、最初の shift の直前で止まる(menhir の `acceptable`)。
    fn offer(&mut self, cx: &mut Ctx, e: &Entry, stop_at_shift: bool) -> Result<Step, MErr> {
        let term = terminal_of(&e.tok);
        loop {
            let d = DEFAULT_REDUCTION[self.current as usize];
            let act = if d != NONE {
                0x8000 | d
            } else {
                ACTION[self.current as usize * N_TERMINALS + term]
            };
            if act == 0 {
                return Ok(Step::Error);
            }
            if act & 0x8000 != 0 {
                let p = (act & 0x7fff) as usize;
                if PRODS[p].start {
                    let v = self.stack.pop().map_or(Sv::None, |c| c.sv);
                    return Ok(Step::Accepted(v));
                }
                self.reduce(cx, p)?;
                continue;
            }
            if stop_at_shift {
                return Ok(Step::NeedInput);
            }
            let target = act - 1;
            let state = self.current;
            self.stack.push(Cell {
                state,
                sv: Sv::Tok(e.tok.clone()),
                sp: e.sp,
                ep: e.ep,
            });
            self.current = target;
            if !NO_DISCARD[target as usize] {
                return Ok(Step::NeedInput);
            }
        }
    }
}

/// 入れ子が構文解析の上限を超えたときに出す、`処理系の限度` の行を作る。
pub fn limit_message(file: &str, limit: usize) -> String {
    format!("{file}: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の上限 {limit} を超えました)")
}

/// 意味動作のエラー(`ActErr`)を、構文解析のエラー([`ParseError`])に変換する。
/// 構文エラーには、位置と `構文エラー:` を付けた行を作る。内部エラーはメッセージをそのまま渡す。
/// 上限を超えたエラー(`ActErr::Limit`)は [`Driver::merr`] が先に処理するので、ここには渡されない。
fn act_err(files: &Files, e: ActErr) -> ParseError {
    match e {
        ActErr::Syntax(pos, msg) => {
            ParseError::Syntax(format!("{}: 構文エラー: {msg}", files.show(pos)))
        }
        ActErr::Internal(msg) => ParseError::Internal(msg),
        ActErr::Limit => unreachable!("上限は Driver::merr が受ける"),
    }
}

/// 構文解析のドライバの状態である。
struct Driver<'a> {
    lexer: &'a mut Lexer,
    files: &'a Files,
    cx: &'a mut Ctx,
    limit: usize,
    /// 字句解析し直すためのソースとファイル。
    /// エラーの直前の状態を作り直すために使う。
    src: &'a [u8],
    file: FileId,
    /// これまでにオートマトンに与えたトークンの個数。
    count: usize,
    /// 1 行目の行番号と、1 行目の桁のずれ([`Lexer::with_start`] に渡す値)。
    line: u32,
    col0: u32,
    /// 文法に合わなかったトークンが `EOF` なら真。
    /// 予約語の誤用と判定しなかったときだけ設定する。
    eof_error: bool,
}

impl Driver<'_> {
    fn limit_err(&self) -> ParseError {
        ParseError::Syntax(limit_message(
            self.files.name(self.lexer.last_sp.file),
            self.limit,
        ))
    }

    fn merr(&self, e: MErr) -> ParseError {
        match e {
            MErr::Act(ActErr::Limit) | MErr::Limit => self.limit_err(),
            MErr::Act(a) => act_err(self.files, a),
        }
    }

    /// 最後のトークンを与える直前の状態のオートマトンを作り直す。
    fn before(&mut self) -> Result<Machine, ParseError> {
        let mut m = Machine::new();
        let mut lx = Lexer::with_start(self.src, self.file, self.line, self.col0);
        for _ in 1..self.count {
            let Ok(e) = lx.read() else {
                return Err(ParseError::Internal(
                    "字句解析の作り直しが元と食い違いました".into(),
                ));
            };
            match m.offer(self.cx, &e, false) {
                Ok(Step::NeedInput) => {}
                Ok(_) => {
                    return Err(ParseError::Internal(
                        "構文解析の作り直しが元と食い違いました".into(),
                    ));
                }
                Err(e) => return Err(self.merr(e)),
            }
        }
        Ok(m)
    }

    /// 直前の状態に `first` を与えて次のトークンが必要になる状態まで進め、続けて `then` を受理できるかを調べる。
    /// `first` を与える段階で起きた構文エラーは「受理できない」として扱い、`then` を調べる段階のエラーはそのまま返す
    /// (Diktor の `misused_reserved_word` の `continues` と `operand` が例外を捕捉する範囲と同じ)。
    fn probe(&mut self, first: &Entry, then: &Entry) -> Result<bool, ParseError> {
        let mut m = self.before()?;
        match m.offer(self.cx, first, false) {
            Ok(Step::NeedInput) => {}
            Ok(_) => return Ok(false),
            Err(MErr::Act(ActErr::Syntax(..))) => return Ok(false),
            Err(e) => return Err(self.merr(e)),
        }
        match m.offer(self.cx, then, true) {
            Ok(Step::NeedInput) => Ok(true),
            Ok(_) => Ok(false),
            Err(e) => Err(self.merr(e)),
        }
    }

    /// 文法に合わなかったトークン `bad` が、識別子の位置に書いた予約語かどうかを判別する
    /// (Diktor の `misused_reserved_word`)。
    /// そうであれば、予約語の表記と位置を返す。
    fn misused_reserved_word(
        &mut self,
        after_brace: bool,
        bad: &Entry,
    ) -> Result<Option<(String, Pos)>, ParseError> {
        let Some(w) = reserved_word(&bad.tok) else {
            return Ok(None);
        };
        let saved = self.lexer.save();
        let next = self.lexer.read().ok().map(|e| (e.tok, e.sp));
        self.lexer.restore(saved);
        let Some((ntok, nsp)) = next else {
            return Ok(None);
        };
        let ident = Entry {
            tok: Tok::LOWER_IDENTIFIER(w.clone()),
            sp: bad.sp,
            ep: bad.ep,
        };
        let next_e = Entry {
            tok: ntok.clone(),
            sp: nsp,
            ep: nsp,
        };
        let continues = self.probe(&ident, &next_e)?;
        let record_label = matches!(ntok, Tok::EQ | Tok::COMMA) && after_brace;
        if !(continues || record_label) {
            return Ok(None);
        }
        let literal = matches!(bad.tok, Tok::BOOL(_));
        if literal {
            return Ok(Some((w, bad.sp)));
        }
        let lparen = Entry {
            tok: Tok::LPAREN,
            sp: bad.sp,
            ep: bad.sp,
        };
        let again = Entry {
            tok: bad.tok.clone(),
            sp: bad.sp,
            ep: bad.sp,
        };
        let operand = self.probe(&lparen, &again)?;
        Ok(if operand { None } else { Some((w, bad.sp)) })
    }

    fn parse_error_message(&self) -> String {
        let f = self.files;
        if let Some((n, sp, next)) = self.lexer.glued_number() {
            return format!(
                "{}: 構文エラー: 数値リテラル {} の直後に {next} が続いています(数値リテラルの表記を確認してください)",
                f.show(sp),
                n.show()
            );
        }
        match self.lexer.error_positions() {
            (pos, None) => format!(
                "{}: 構文エラー: 付近のトークンを確認してください",
                f.show(pos)
            ),
            (pos, Some(next)) => format!(
                "{}: 構文エラー: この位置の後ろ、{}:{} のトークンが文法に合いません",
                f.show(pos),
                next.line,
                next.col()
            ),
        }
    }

    fn run(&mut self) -> Result<Program, ParseError> {
        let mut m = Machine::new();
        let mut last_brace = false;
        loop {
            let e = self.lexer.read().map_err(ParseError::Lex)?;
            let after_brace = std::mem::replace(&mut last_brace, e.tok == Tok::LBRACE_BLOCK);
            self.count += 1;
            match m.offer(self.cx, &e, false) {
                Ok(Step::NeedInput) => {}
                Ok(Step::Accepted(v)) => return Ok(v.into_program()),
                Ok(Step::Error) => {
                    return Err(match self.misused_reserved_word(after_brace, &e)? {
                        Some((w, sp)) => ParseError::Syntax(format!(
                            "{}: 構文エラー: 予約語 {w} はここには書けません(予約語は識別子に使えません)",
                            self.files.show(sp)
                        )),
                        None => {
                            self.eof_error = e.tok == Tok::EOF;
                            ParseError::Syntax(self.parse_error_message())
                        }
                    });
                }
                Err(err) => return Err(self.merr(err)),
            }
        }
    }
}

/// 構文解析の上限(作る木の深さの上限)の既定値である。
///
/// Diktor のテスト run_exit.t の `l=8M` のブロックにある、2 万段の `1 + (...)` の式を解析できる値にしてある。
/// 12 万文のブロックは、文を `let` の連鎖に脱糖するので深さ 12 万の木になるが、この上限に収まる。
pub const DEFAULT_MAX_NEST: usize = 200_000;

/// 1 つのファイルのソースを構文解析する(Diktor の `parse_unit_with`)。
///
/// 作る木の深さの上限は、`cx` が持つ上限([`Ctx::limit`])である。
/// ノードの番号は、`cx` がそれまでに振った番号の続きから振る。
pub fn parse_program(
    src: &[u8],
    file: FileId,
    files: &Files,
    cx: &mut Ctx,
) -> Result<Program, ParseError> {
    cx.over = false;
    let limit = cx.limit();
    let mut lexer = Lexer::new(src, file);
    let mut d = Driver {
        lexer: &mut lexer,
        files,
        cx,
        limit,
        src,
        file,
        count: 0,
        line: 1,
        col0: 0,
        eof_error: false,
    };
    d.run()
}

/// 対話的な実行の入力 1 つを構文解析した結果である(Diktor の driver.ml の `try_parse` が返す 3 通り)。
#[derive(Debug)]
pub enum InputParse {
    /// 入力が完結していて、構文解析できた。
    Complete(Program),
    /// 入力が途中なので、続きの行を待つ。
    /// 文法に合わなかったトークンが `EOF` のときと、文字列リテラルかブロックコメントが閉じていない字句エラーのときである。
    Incomplete,
    /// エラーになった。
    Failed(ParseError),
}

/// 対話的な実行の入力 1 つを構文解析する。
///
/// `src` の 1 行目を `line` 行目として扱い、1 行目の桁を `col0` だけずらす。
/// `:type` の後ろの式を解析するときは、`col0` に `:type ` の長さを渡す。
/// ノードの番号は、`cx` がそれまでに振った番号の続きから振る。
pub fn parse_input(
    src: &[u8],
    file: FileId,
    files: &Files,
    cx: &mut Ctx,
    line: u32,
    col0: u32,
) -> InputParse {
    cx.over = false;
    let limit = cx.limit();
    let mut lexer = Lexer::with_start(src, file, line, col0);
    let mut d = Driver {
        lexer: &mut lexer,
        files,
        cx,
        limit,
        src,
        file,
        count: 0,
        line,
        col0,
        eof_error: false,
    };
    match d.run() {
        Ok(p) => InputParse::Complete(p),
        Err(ParseError::Syntax(_)) if d.eof_error => InputParse::Incomplete,
        Err(ParseError::Lex(e))
            if e.msg == crate::lexer::UNTERMINATED_TEXT_MSG
                || e.msg == crate::lexer::UNTERMINATED_COMMENT_MSG =>
        {
            InputParse::Incomplete
        }
        Err(e) => InputParse::Failed(e),
    }
}
