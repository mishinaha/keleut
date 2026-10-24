//! 構文解析の駆動である(Diktor の driver.ml の §16.3 と §16.3a)。
//!
//! 表(`tables.rs`)は、Diktor の parser.mly を menhir が表の形にした自動機械と同じものである。
//! 駆動は menhir の表の後段(MenhirLib の `Engine`)と同じ手順で動く。
//!
//! - shift の後は、次のトークンを必ず読む(`#` での既定の還元を持つ状態に入るときだけ読まない)。
//! - トークンを読んだ後は、既定の還元を持つ状態なら先読みを見ずに還元し、そうでなければ動作の表を引く。
//!
//! この手順を写すので、トークンを読む時機(字句エラーが出る時機)、
//! 意味の動作が構文エラーを投げる時機、文法に合わないトークンに気づく位置が Diktor と一致する。
//!
//! 予約語を識別子の位置に書いた誤りの見分け(Diktor の `misused_reserved_word`)は、
//! 誤りの直前の状態から別のトークンを与えて進められるかを調べる。
//! menhir の状態は永続的なので Diktor はそのまま試すが、本モジュールは、
//! ソースを先頭から字句解析し直し、それまでに読んだ数のトークンを新しい自動機械に与え直して、
//! 直前の状態を作り直す。
//! 字句解析と意味の動作は決定的なので、作り直した状態は元と同じである。
//! こうすると、誤りの無い入力ではトークンを控えておく仕事が要らない。
//!
//! スタックはヒープの上の明示的な配列なので、深い入力でもネイティブのスタックを使わない。
//! そのためスタックの段の数には限度を設けない。
//! 平らな列(トップレベルの宣言、ブロックの文、引数の列)は右再帰の規則でスタックを長くするが、
//! Diktor(menhir の表の後段)と同じく受理する。
//! 入れ子の計数器は、意味の動作が作る木の深さ([`Ctx`])である。
//! 後の段は木をたどって再帰するので、木の深さが限度を超えたら「処理系の限度」で終わる
//! (計画 261024-1 の 6.11 節)。

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

/// 構文解析の失敗である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// 字句エラー(終了状態 2)。
    Lex(LexError),
    /// 位置まで整形し終えた構文エラーか、構文解析の限度の 1 行(終了状態 2。Diktor の `Parse_error`)。
    Syntax(String),
    /// 内部エラー(終了状態 3)。Diktor が受け皿の外の例外で終わる場合に合わせる。
    Internal(String),
}

const NONE: u16 = u16::MAX;

/// トークンの名前から終端記号の番号を引く表である。
fn terminal_of(tok: &Tok) -> usize {
    static MAP: OnceLock<std::collections::HashMap<&'static str, usize>> = OnceLock::new();
    let map = MAP.get_or_init(|| TERMINALS.iter().enumerate().map(|(i, n)| (*n, i)).collect());
    map[tok.name()]
}

/// スタックの 1 段である(menhir の stack の cell)。
/// `state` は、この段を積んだときに離れた状態である。
struct Cell {
    state: u16,
    sv: Sv,
    sp: Pos,
    ep: Pos,
}

/// トークンを与えた結果である。
#[allow(clippy::large_enum_variant)]
enum Step {
    /// 次のトークンを求める。
    NeedInput,
    /// 受理した。
    Accepted(Sv),
    /// 文法に合わない。
    Error,
}

/// 自動機械である。
struct Machine {
    stack: Vec<Cell>,
    current: u16,
    /// 還元のたびに右辺を移す列(割り付けを使い回す)。
    buf: Vec<(Sv, Pos, Pos)>,
}

/// 自動機械の失敗(意味の動作の誤りか、木の深さの限度)である。
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

    /// トークンを与えて、次のトークンを求めるか、受理か、誤りまで進める。
    /// `stop_at_shift` のときは、最初の shift の直前で止まる(menhir の `acceptable`)。
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

/// 構文解析の限度の 1 行である。
pub fn limit_message(file: &str, limit: usize) -> String {
    format!("{file}: 処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 {limit} を超えました)")
}

/// 限度でない意味の動作の誤りを診断にする(限度は [`Driver::merr`] が先に受ける)。
fn act_err(files: &Files, e: ActErr) -> ParseError {
    match e {
        ActErr::Syntax(pos, msg) => {
            ParseError::Syntax(format!("{}: 構文エラー: {msg}", files.show(pos)))
        }
        ActErr::Internal(msg) => ParseError::Internal(msg),
        ActErr::Limit => unreachable!("限度は Driver::merr が受ける"),
    }
}

/// 構文解析の状態である。
struct Driver<'a> {
    lexer: &'a mut Lexer,
    files: &'a Files,
    cx: &'a mut Ctx,
    limit: usize,
    /// 字句解析し直すためのソースとファイル(誤りの直前の状態を作り直すために使う)。
    src: &'a [u8],
    file: FileId,
    /// これまでに与えたトークンの数。
    count: usize,
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

    /// 最後のトークンを与える直前の自動機械を作り直す。
    fn before(&mut self) -> Result<Machine, ParseError> {
        let mut m = Machine::new();
        let mut lx = Lexer::new(self.src, self.file);
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

    /// 直前の状態から `first` を与えて次の入力を求める状態へ進め、`then` を受け入れられるかを調べる。
    /// `first` を与えた段の構文エラーは「受け入れない」とし、`then` を調べる段の誤りはそのまま返す
    /// (Diktor の `misused_reserved_word` の `continues` と `operand` が例外を受ける範囲と同じ)。
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

    /// 予約語を識別子の位置に書いた誤りを見分ける(Diktor の `misused_reserved_word`)。
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
                "{}: 構文エラー: 数値リテラル {} の直後に {next} が続いています(数値リテラルの綴りを確認してください)",
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
                        None => ParseError::Syntax(self.parse_error_message()),
                    });
                }
                Err(err) => return Err(self.merr(err)),
            }
        }
    }
}

/// 構文解析の限度(作る木の深さ)の既定値である。
///
/// run_exit.t の `l=8M` のブロックの、2 万段の `1 + (...)` の式が通る大きさにしてある(計画 261024-1 の 6.11 節)。
/// 12 万文のブロックは、文を `let` の鎖に脱糖するので深さ 12 万の木になり、限度の内側にある。
pub const DEFAULT_MAX_NEST: usize = 200_000;

/// 1 つのファイルのソースを構文解析する(Diktor の `parse_unit_with`)。
///
/// 作る木の深さの限度は `cx` の限度([`Ctx::limit`])である。
/// ノードの番号は `cx` が続きから振る。
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
    };
    d.run()
}
