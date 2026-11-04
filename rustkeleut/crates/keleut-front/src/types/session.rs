//! 対話的な実行の型検査の側である(Diktor の driver.ml の §16.6b と elab.ml の §11.44)。
//!
//! [`Session`] は、プレリュードを 1 回だけ処理し、その後は入力ごとに次を行う。
//!
//! 1. 入力を構文解析する([`Session::parse`])。
//!    文法に合わなかったトークンが `EOF` なら、続きの行を待つ。
//! 2. 平坦化して型検査し、型検査に通った入力なら、その入力を 1 つの単位として KIR に書き、
//!    セッションのプログラムの後ろに足す([`Session::check`]。[`ReplWriter`])。
//!    プログラムは、組み込みの単位、プレリュードの単位、足した入力の単位(鍵は `repl/1`、`repl/2` …)からなる。
//!    入力の単位は、結果の表示に使う隠れた大域(式文の値と、`Show.show` を呼ぶ関数)を持つ。
//! 3. 呼び出し側が足した単位だけを実行し、その結果で入力を受理するか捨てるかを決める([`Session::finish`])。
//!
//! 受理しなかった入力(構文エラー、型エラー、実行時エラー)は捨てる。
//! 型検査の文脈は入力の前に印を取り(`Tc::snapshot`)、捨てるときは印の時点に戻す。
//! 入力の数とともに伸びる表(型、セル、穴、木の注釈、宣言表、名前の表など)は、印の後の書き換えの記録で戻し、
//! 入力ごとに初めに戻す小さな欄だけを写しから戻す。
//! そのため、捨てた入力の宣言、束縛、インスタンスと、一般化しなかった束縛の型変数に決めた型は、後の入力から見えない。
//! 印に戻しても、通し番号と名前の表(Diktor が戻さないもの)は捨てた入力の後のものを残す(`Session::restore`)。
//! KIR の書き手の名前の表も同じく印で入力の前に戻すが、実行した単位はプログラムに残す
//! (関数と大域の番号を詰め直さないためである。実行時エラーで止まった入力の大域の値は、実行する側が捨てる)。

use super::check::{PRELUDE, Stop, fail_to_error, parse_unit, render_line};
use super::decls::{ROOT_UNIT, STD_UNIT};
use super::elab::{is_value, pat_var_names};
use super::emit::{ProgramMark, ReplUnit, ReplWriter, UnitSrc, WriterMark};
use super::fx::{HashMap, HashSet};
use super::{DKey, Emit, Env, Evidence, Fail, GRef, OutLine, R, Tc, Ty, TyNode};
use crate::ParseError;
use crate::actions::Ctx;
use crate::ast::{Decl, DeclKind, PatKind};
use crate::parser::{InputParse, parse_input};
use crate::pos::{FileId, Files};

/// 対話的な実行の選択肢である。
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// `--no-prelude`。
    pub no_prelude: bool,
    /// `--prelude PATH`。
    pub prelude: Option<String>,
    /// 構文解析の限度。
    pub max_nest: usize,
}

/// 入力を構文解析した結果である(Diktor の `parse_result`)。
#[derive(Debug)]
pub enum Parsed {
    /// 入力が終わった。宣言の列である。
    Complete(Vec<Decl>),
    /// 続きの行を待つ。
    Incomplete,
    /// 誤り。報告する 1 行である。
    Failed(String),
}

/// セッションを始められなかった理由である([`Session::new`])。
#[derive(Debug)]
pub enum StartError {
    /// プレリュードを読めない、字句と構文の誤り、内部の誤り(終了状態は `keleut check` の [`Stop`] と同じ)。
    Stop(Stop),
    /// プレリュードの型エラー(終了状態 1)。報告する 1 行である。
    Type(String),
    /// プレリュードを KIR に書き出せない(内部の誤り。終了状態 70)。報告する 1 行である。
    Emit(String),
}

impl From<Stop> for StartError {
    fn from(s: Stop) -> StartError {
        StartError::Stop(s)
    }
}

/// 結果を表示する束縛(Diktor の `exec_input` の `shown` の項目)である。
#[derive(Clone, Debug)]
pub struct Shown {
    /// `名前 : 型` の部分。
    pub label: String,
    /// 値を持つ大域(KIR の添字)。
    pub global: kir::GlobalId,
    /// 型が `{}` の式文か(何も表示しない)。
    pub unit_exp: bool,
    /// 値を `Show.show` で表示するときの、`$show` の大域。
    pub show: Option<kir::GlobalId>,
    /// 値の型の配置([`Layouts`] の添字)。
    pub layout: usize,
}

/// 値の表示に使う、型から読んだ配置である。
///
/// 実行時のレコードはフィールドを正準の配置(ラベルの番号の順)で持つ。
/// 表示は、値の表示の順を型の表示(`名前 : 型 = 値` の型)の順にそろえるため、レコードのフィールドを型の行の順に並べ替える。
/// ラベルの番号は受理しなかった入力を含むセッションの履歴で決まるので、表示に正準の配置の順を使わない。
/// 配置は再帰する型のために表の添字で指し合う。
/// 0 番は型の分からない値である。
/// 型の分からない値のレコードは、フィールドをラベルの綴りの順で書く(`keleut repl` の `show_value`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    /// 型の分からない値(型変数、関数など)。子も型が分からないものとして表示する。
    Any,
    /// レコードとタプル。ラベル(KIR の綴り)と子の配置の、型の行の順の列。
    Record(Vec<(String, usize)>),
    /// ヴァリアント。ラベルと積載値の配置。
    Variant(Vec<(String, usize)>),
    /// データ。コンストラクタの名前(表示の綴り)と、フィールドの配置の列。
    Data(Vec<(String, Vec<usize>)>),
    /// `Array` と `MutableArray`。要素の配置。
    Array(usize),
}

/// [`Layout`] の表である。
#[derive(Clone, Debug, Default)]
pub struct Layouts {
    /// 配置。0 番は [`Layout::Any`] である。
    pub nodes: Vec<Layout>,
}

/// 配置の表に置く項目の数の上限(正則でない再帰型で表が限りなく伸びないようにする)。
/// 届いた後の型は型の分からない値として表示する。
const LAYOUT_BUDGET: usize = 4096;
/// 配置を作る型の大きさの上限(`Session::layout_key` の数え方で、この値を含む)。
///
/// 超えた型は型の分からない値として表示する。
/// 上限は表示する値全体の型に掛かり、外側の型の大きさが上限を超えると、内側のレコードにも配置を作らない。
/// `Int32` のフィールド N 個のレコードの大きさは 2N + 1 なので、表示する値全体がレコードのとき、配置を使うのはフィールド 409 個までである。
/// 大きさは型の構造だけで決まり、通し番号の桁数に依らないので、配置を使うかどうかは受理しなかった入力に依らない。
const LAYOUT_SIZE_MAX: usize = 820;

/// [`Session::check`] の結果である。
pub enum Outcome {
    /// 受理しなかった。報告する行(警告と誤り)である。
    Rejected(Vec<String>),
    /// 型検査に通った。呼び出し側は警告の行を出し、足した単位を実行してから [`Session::finish`] を呼ぶ。
    Run {
        /// 警告の行。
        lines: Vec<String>,
        /// セッションのプログラム。最後の単位がこの入力である。
        /// [`Session::finish`] で返す。
        program: Box<kir::Program>,
        /// 結果を表示する束縛(入力の順)。
        shown: Vec<Shown>,
        /// 値の配置。
        layouts: Layouts,
    },
}

/// 型検査に通り、実行を待つ入力である。
struct PendingInput {
    snapshot: Snapshot,
    flat: Vec<Decl>,
    env: Env,
    /// 書き手の名前の表の、入力の前の写し。
    writer: WriterMark,
    /// 入力の単位を足す前のプログラムの大きさ。
    program: ProgramMark,
}

/// 実行した結果([`Session::finish`])。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// 最後まで実行した。入力を受理する。
    Accepted,
    /// 実行時エラーで止まった。入力を捨てるが、実行した単位はプログラムに残す。
    Failed,
    /// 実行しなかった(KIR の限度に当たった)。入力を捨て、足した単位をプログラムから除く。
    Unrun,
}

/// 対話的な実行の型検査の状態である。
pub struct Session {
    opts: SessionOptions,
    tc: Tc,
    files: Files,
    /// 入力の `<stdin>` のファイルの添字(入力ごとに足さない)。
    stdin: FileId,
    cx: Ctx,
    /// プレリュードだけを処理した後の環境に、受理した入力の束縛を足した環境。
    env: Env,
    /// KIR の書き手。
    writer: ReplWriter,
    /// セッションのプログラム(入力の実行の間は呼び出し側が持つ)。
    program: Option<Box<kir::Program>>,
    /// 次に足す単位の番号(受理しなかった入力の単位の鍵も使い直さない)。
    next_unit: u32,
    /// 受理した入力のトップレベルの値の名前(Diktor の `session_vals`)。
    session_vals: HashSet<String>,
    /// `Print` をプレリュードが所有するか。
    print_owned: bool,
    pending: Option<PendingInput>,
}

/// 型検査の誤りの 1 行。内部の誤りは `[BUG]` の文面のまま返す。
fn render_fail(files: &Files, f: Fail) -> String {
    match fail_to_error(files, f) {
        Ok(e) => e.render(),
        Err(m) => m,
    }
}

impl Session {
    /// プレリュードを処理したセッションを作る(Diktor の `repl` の `start`)。
    ///
    /// 型検査と同じ [`on_check_stack`](super::check::on_check_stack) のスレッドの中で呼ぶ。
    /// プレリュードの型エラーは [`StartError::Type`] で返す(Diktor は標準エラーに書いて 1 で終わる)。
    pub fn new(opts: SessionOptions) -> Result<Session, StartError> {
        let mut cx = Ctx::new(opts.max_nest);
        let mut files = Files::new();
        let mut tc = Tc::new();
        let decls = if opts.no_prelude {
            Vec::new()
        } else {
            let (name, src) = match &opts.prelude {
                Some(p) => (
                    p.clone(),
                    super::load::SourceFs::read_channel(&mut super::load::StdFs, p)
                        .map_err(Stop::Input)?,
                ),
                None => ("<prelude>".to_string(), PRELUDE.as_bytes().to_vec()),
            };
            let prog = parse_unit(&mut cx, &mut files, &name, &src)?;
            if let Some(im) = prog.imports.first() {
                return Err(StartError::Stop(Stop::Syntax(format!(
                    "{}: 構文エラー: import はコマンド行で渡したファイルにだけ書けます",
                    files.show(im.span.start)
                ))));
            }
            prog.decls
        };
        let check_err = |files: &Files, f: Fail| match fail_to_error(files, f) {
            Ok(e) => StartError::Type(e.render()),
            Err(m) => StartError::Stop(Stop::Internal(m)),
        };
        let prelude = tc
            .flatten_modules(&[], &[], STD_UNIT, decls)
            .map_err(|f| check_err(&files, f))?;
        tc.start_program();
        let env = tc
            .check_prelude(&prelude)
            .map_err(|f| check_err(&files, f))?;
        let print = tc.intern("Print");
        let print_owned = tc.prelude_owned("effect", print);
        let stdin = files.add("<stdin>");
        let prelude_paths: Vec<Vec<u8>> = if opts.no_prelude {
            Vec::new()
        } else {
            vec![
                opts.prelude
                    .clone()
                    .unwrap_or_else(|| "<prelude>".to_string())
                    .into_bytes(),
            ]
        };
        let base = [UnitSrc {
            key: "prelude".to_string(),
            paths: prelude_paths,
            decls: &prelude,
            repl: None,
        }];
        let (writer, program) = ReplWriter::start(&mut tc, &base)
            .map_err(|f| StartError::Emit(render_fail(&files, f)))?;
        Ok(Session {
            opts,
            tc,
            files,
            stdin,
            cx,
            env,
            writer,
            program: Some(Box::new(program)),
            next_unit: 1,
            session_vals: HashSet::default(),
            print_owned,
            pending: None,
        })
    }

    /// プレリュードまでを書いたプログラムを取り出す(始めるときと `:reset` の後の、プレリュードの初期化)。
    ///
    /// 実行した後で [`Session::put_program`] で返す。
    pub fn take_program(&mut self) -> Option<Box<kir::Program>> {
        self.program.take()
    }

    /// [`Session::take_program`] で取り出したプログラムを返す。
    pub fn put_program(&mut self, p: Box<kir::Program>) {
        self.program = Some(p);
    }

    /// プレリュードだけを処理した状態に戻す(`:reset`)。
    pub fn reset(&mut self) -> Result<(), StartError> {
        *self = Session::new(self.opts.clone())?;
        Ok(())
    }

    /// 入力を構文解析する(Diktor の `try_parse`)。
    ///
    /// `src` の 1 行目を `line` 行目とし、1 行目の桁を `col0` だけずらす。
    pub fn parse(&mut self, src: &[u8], line: u32, col0: u32) -> Parsed {
        match parse_input(src, self.stdin, &self.files, &mut self.cx, line, col0) {
            InputParse::Complete(p) => match p.imports.first() {
                Some(im) => Parsed::Failed(format!(
                    "{}: 構文エラー: import は対話的な実行では書けません",
                    self.files.show(im.span.start)
                )),
                None => Parsed::Complete(p.decls),
            },
            InputParse::Incomplete => Parsed::Incomplete,
            InputParse::Failed(ParseError::Lex(e)) => {
                Parsed::Failed(format!("{}: 字句エラー: {}", self.files.show(e.pos), e.msg))
            }
            InputParse::Failed(ParseError::Syntax(m)) => Parsed::Failed(m),
            InputParse::Failed(ParseError::Internal(m)) => {
                Parsed::Failed(format!("内部エラー: 予期しない例外です: {m}"))
            }
        }
    }

    /// 型検査の文脈を印の時点に戻す。通し番号と名前の表は今のものを残す。
    fn restore(&mut self, snap: Snapshot) {
        self.tc.rollback_with(snap, true);
    }

    /// 1 つの入力を平坦化して型検査し、受理できれば KIR を書く(Diktor の `repl` の `process` の前半)。
    ///
    /// 受理しなかったときは、型検査の文脈を入力の前に戻してから返す。
    /// [`Outcome::Run`] を返したときは、呼び出し側が [`Session::finish`] を呼ぶ。
    pub fn check(&mut self, decls: Vec<Decl>) -> Outcome {
        let snap = self.tc.snapshot();
        let flat = match self.tc.flatten_input(&self.session_vals, decls) {
            Ok(d) => d,
            Err(f) => {
                let line = render_fail(&self.files, f);
                self.restore(snap);
                return Outcome::Rejected(vec![line]);
            }
        };
        let r = self.tc.check_input(&self.env, &flat);
        let mut lines: Vec<String> = std::mem::take(&mut self.tc.current_out)
            .iter()
            .filter(|l| matches!(l, OutLine::Warning(_)))
            .map(render_line)
            .collect();
        let env = match r {
            Ok(env) => env,
            Err(f) => {
                lines.push(render_fail(&self.files, f));
                self.restore(snap);
                return Outcome::Rejected(lines);
            }
        };
        let Some(mut program) = self.program.take() else {
            lines.push("[BUG] 対話的な実行のプログラムがありません".to_string());
            self.restore(snap);
            return Outcome::Rejected(lines);
        };
        let writer = self.writer.mark();
        let pmark = ProgramMark::of(&program);
        let r = self.prepare(&flat, &mut program);
        self.program = Some(program);
        match r {
            Ok((shown, layouts)) => {
                self.pending = Some(PendingInput {
                    snapshot: snap,
                    flat,
                    env,
                    writer,
                    program: pmark,
                });
                Outcome::Run {
                    lines,
                    program: self.program.take().expect("プログラム"),
                    shown,
                    layouts,
                }
            }
            Err(f) => {
                lines.push(match f {
                    Fail::Limit => {
                        format!("keleut: KIR を書き出せません: {}", kir::WRITE_LIMIT_MESSAGE)
                    }
                    f => render_fail(&self.files, f),
                });
                self.writer.rollback(writer);
                self.restore(snap);
                Outcome::Rejected(lines)
            }
        }
    }

    /// 結果を表示する束縛を集め、入力の単位を KIR に書いて `program` の後ろに足す。
    ///
    /// 誤りのときは、書き手が足しかけた項目を除き、書き手の表を戻してある。
    fn prepare(&mut self, flat: &[Decl], program: &mut kir::Program) -> R<(Vec<Shown>, Layouts)> {
        // 束縛ごとの (表示の名前, 型, 大域を引く鍵)。
        let mut items: Vec<(String, Ty, (crate::ast::NodeId, String), bool)> = Vec::new();
        for d in flat {
            match &d.kind {
                DeclKind::Exp(e) => {
                    let t = self.tc.get_ty(e.meta.id)?;
                    items.push(("_".to_string(), t, (d.meta.id, "$it".to_string()), true));
                }
                DeclKind::Let(b) => {
                    for x in pat_var_names(&b.name) {
                        let g = GRef::Decl(b.meta.id, x.clone());
                        if let Some(t) = self.tc.top_types.get(&g).copied() {
                            items.push((x.clone(), t, (b.meta.id, x), false));
                        }
                    }
                }
                DeclKind::LetRec(bs) => {
                    for b in bs {
                        if let PatKind::Var(x) = &b.name.kind {
                            let g = GRef::Decl(b.meta.id, x.clone());
                            if let Some(t) = self.tc.top_types.get(&g).copied() {
                                items.push((x.clone(), t, (b.meta.id, x.clone()), false));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let mut shows: Vec<(Evidence, Vec<DKey>)> = Vec::new();
        // (表示の名前と型, 大域を引く鍵, 型が {} の式文か, shows の添字, 配置)。
        type Pre = (
            String,
            (crate::ast::NodeId, String),
            bool,
            Option<usize>,
            usize,
        );
        let mut pre: Vec<Pre> = Vec::new();
        let mut layouts = Layouts {
            nodes: vec![Layout::Any],
        };
        let mut memo: HashMap<String, usize> = HashMap::default();
        for (x, t, key, is_exp) in items {
            let label = format!("{} : {}", super::display(&x), self.tc.show(t)?);
            let unit_exp = is_exp && {
                let r = self.tc.repr(t);
                match self.tc.node(r) {
                    TyNode::Record(row) => self.tc.repr_is_empty(row),
                    _ => false,
                }
            };
            let show = if !unit_exp && self.shows_newtype(t)? {
                let show = self.tc.intern("Show");
                let mut pend = Vec::new();
                let ev = self.display_evidence(show, t, &mut pend)?;
                shows.push((ev, pend));
                Some(shows.len() - 1)
            } else {
                None
            };
            let layout = self.layout(t, &mut layouts, &mut memo)?;
            pre.push((label, key, unit_exp, show, layout));
        }
        let cur = ReplUnit {
            print: self.print_owned,
            display: true,
            shows,
        };
        let key = format!("repl/{}", self.next_unit);
        self.next_unit += 1;
        let src = UnitSrc {
            key,
            paths: vec![b"<stdin>".to_vec()],
            decls: flat,
            repl: Some(&cur),
        };
        let out = self.writer.add(&mut self.tc, program, src)?;
        let globals: HashMap<(crate::ast::NodeId, String), kir::GlobalId> =
            out.globals.into_iter().collect();
        let mut shown = Vec::with_capacity(pre.len());
        for (label, key, unit_exp, show, layout) in pre {
            let Some(g) = globals.get(&key).copied() else {
                return super::bug(format!("REPL の束縛の大域が見つかりません: {}", key.1));
            };
            shown.push(Shown {
                label,
                global: g,
                unit_exp,
                show: show.map(|i| out.shows[i]),
                layout,
            });
        }
        Ok((shown, layouts))
    }

    /// 型の構造の鍵。型変数は `?` にする(型変数の位置の値は型の分からない値として表示する)。
    ///
    /// `size` に型の大きさを足していき、[`LAYOUT_SIZE_MAX`] を超えたところでたどるのをやめて `false` を返す。
    /// 大きさは、たどった型の節(型構成子、レコード、ヴァリアント、`?`)の数とレコードとヴァリアントのフィールドの数の和で、
    /// 通し番号の綴りの長さを含めない。
    /// 上限の内側の型では鍵を最後まで作るので、別の型が同じ鍵になることはない。
    fn layout_key(&mut self, t: Ty, out: &mut String, size: &mut usize) -> R<bool> {
        super::guard()?;
        *size += 1;
        if *size > LAYOUT_SIZE_MAX {
            return Ok(false);
        }
        let t = self.tc.repr(t);
        match self.tc.node(t) {
            TyNode::Con(n, args) => {
                out.push_str(&format!("C{n}["));
                for a in args.iter() {
                    if !self.layout_key(*a, out, size)? {
                        return Ok(false);
                    }
                    out.push(',');
                }
                out.push(']');
            }
            TyNode::Record(row) | TyNode::Variant(row) => {
                out.push(if matches!(self.tc.node(t), TyNode::Record(_)) {
                    '{'
                } else {
                    '<'
                });
                let (fs, _) = self.tc.row_fields(row);
                *size += fs.len();
                if *size > LAYOUT_SIZE_MAX {
                    return Ok(false);
                }
                for (l, f) in fs {
                    out.push_str(&format!("{l}:"));
                    if !self.layout_key(f, out, size)? {
                        return Ok(false);
                    }
                    out.push(',');
                }
                out.push('}');
            }
            _ => out.push('?'),
        }
        Ok(true)
    }

    /// 型 `t` の配置を表に置き、その添字を返す。
    fn layout(&mut self, t: Ty, ls: &mut Layouts, memo: &mut HashMap<String, usize>) -> R<usize> {
        super::guard()?;
        let t = self.tc.repr(t);
        let node = self.tc.node(t);
        if !matches!(
            node,
            TyNode::Con(..) | TyNode::Record(_) | TyNode::Variant(_)
        ) {
            return Ok(0);
        }
        let mut key = String::new();
        if !self.layout_key(t, &mut key, &mut 0)? || ls.nodes.len() >= LAYOUT_BUDGET {
            return Ok(0);
        }
        if let Some(&i) = memo.get(&key) {
            return Ok(i);
        }
        let id = ls.nodes.len();
        ls.nodes.push(Layout::Any);
        memo.insert(key, id);
        let l = match node {
            TyNode::Record(row) | TyNode::Variant(row) => {
                let (fs, _) = self.tc.row_fields(row);
                let mut v = Vec::with_capacity(fs.len());
                for (l, f) in fs {
                    let c = self.layout(f, ls, memo)?;
                    v.push((self.tc.name_of(l).to_string(), c));
                }
                if matches!(node, TyNode::Record(_)) {
                    Layout::Record(v)
                } else {
                    Layout::Variant(v)
                }
            }
            TyNode::Con(n, args) => {
                if (n == self.tc.intern("Array") || n == self.tc.ty_mutable_array)
                    && args.len() == 1
                {
                    Layout::Array(self.layout(args[0], ls, memo)?)
                } else if let Some(dd) = self.tc.d.datas.get(&n).cloned() {
                    if dd.dd_params.len() != args.len() {
                        Layout::Any
                    } else {
                        let subst: Vec<(super::Oid, Ty)> = dd
                            .dd_params
                            .iter()
                            .zip(args.iter())
                            .map(|(p, a)| (p.vid, *a))
                            .collect();
                        let mut cs = Vec::with_capacity(dd.dd_ctors.len());
                        for c in &dd.dd_ctors {
                            let mut fs = Vec::with_capacity(c.ct_fields.len());
                            for f in &c.ct_fields {
                                let ft = self.tc.subst_params(0, &subst, f.fi_ty)?;
                                fs.push(self.layout(ft, ls, memo)?);
                            }
                            cs.push((super::display(&self.tc.name_of(c.ct_name)).to_string(), fs));
                        }
                        Layout::Data(cs)
                    }
                } else {
                    Layout::Any
                }
            }
            _ => Layout::Any,
        };
        ls.nodes[id] = l;
        Ok(id)
    }

    /// 型の頭が newtype で、その型が `Show` を満たすか(Diktor の `show_value` の `shows`)。
    ///
    /// 満たすかどうかは、型検査の文脈に `Show` の制約を足して確かめ、文脈を足す前に戻す
    /// (足した制約が、後の束縛の確かめと表示に効かないようにする)。
    fn shows_newtype(&mut self, t: Ty) -> R<bool> {
        let r = self.tc.repr(t);
        let TyNode::Con(c, _) = self.tc.node(r) else {
            return Ok(false);
        };
        if !self.tc.d.datas.contains_key(&c) {
            return Ok(false);
        }
        let show = self.tc.intern("Show");
        if self.tc.find_instance(show, c).is_none() {
            // インスタンスが無ければ、制約を足すと必ず失敗する。
            return Ok(false);
        }
        let snap = self.tc.snapshot();
        let ok = self.tc.add_class(None, r, show).is_ok();
        self.tc.rollback(snap);
        Ok(ok)
    }

    /// 結果の表示に使う証拠(Diktor の `Unify.display_evidence`)。
    ///
    /// 決まっていない部分は、辞書の引数にして `pend` に足す(Diktor の解けていない穴)。
    fn display_evidence(&mut self, c: super::Oid, t: Ty, pend: &mut Vec<DKey>) -> R<Evidence> {
        super::guard()?;
        let t = self.tc.repr(t);
        match self.tc.node(t) {
            TyNode::Con(n, args) => match self.tc.find_instance(c, n) {
                Some(ii) => {
                    let mut ps = Vec::with_capacity(ii.ii_premises.len());
                    for (i, c2) in &ii.ii_premises {
                        let a = match args.get(*i) {
                            Some(a) => *a,
                            None => return super::bug("display_evidence: 型の引数が足りません"),
                        };
                        ps.push(self.display_evidence(*c2, a, pend)?);
                    }
                    Ok(Evidence::Inst(c, n, ps))
                }
                None => Ok(self.pending_param(c, pend)),
            },
            TyNode::Record(row) | TyNode::Variant(row) => {
                let is_record = matches!(self.tc.node(t), TyNode::Record(_));
                let (mut fs, _) = self.tc.row_fields(row);
                self.tc.sort_row_fields(&mut fs);
                let mut out = Vec::with_capacity(fs.len());
                for (l, f) in fs {
                    out.push((l, self.display_evidence(c, f, pend)?));
                }
                Ok(if is_record {
                    Evidence::Record(out)
                } else {
                    Evidence::Variant(out)
                })
            }
            _ => Ok(self.pending_param(c, pend)),
        }
    }

    fn pending_param(&mut self, c: super::Oid, pend: &mut Vec<DKey>) -> Evidence {
        let k: DKey = (self.tc.new_oid(), c);
        pend.push(k);
        Evidence::Param(k)
    }

    /// 実行の結果で、入力を受理するか捨てるかを決める(Diktor の `repl` の `process` の後半)。
    ///
    /// `program` は [`Outcome::Run`] で渡したプログラムである。
    /// 受理しなければ、型検査の文脈と書き手の名前の表を入力の前に戻す。
    /// [`Finish::Unrun`] なら、足した単位もプログラムから除く。
    pub fn finish(&mut self, mut program: Box<kir::Program>, r: Finish) {
        let Some(p) = self.pending.take() else {
            self.program = Some(program);
            return;
        };
        if r == Finish::Unrun {
            p.program.truncate(&mut program);
        }
        self.program = Some(program);
        if r == Finish::Accepted {
            self.tc.commit(p.snapshot);
            self.writer.commit(p.writer);
            for d in &p.flat {
                match &d.kind {
                    DeclKind::Let(b) => {
                        self.session_vals.extend(pat_var_names(&b.name));
                    }
                    DeclKind::LetRec(bs) => {
                        for b in bs {
                            self.session_vals.extend(pat_var_names(&b.name));
                        }
                    }
                    DeclKind::Extern(ex) => {
                        self.session_vals.insert(ex.name.clone());
                    }
                    _ => {}
                }
            }
            self.env = p.env;
        } else {
            self.writer.rollback(p.writer);
            self.restore(p.snapshot);
        }
    }

    /// `:type` の式の型を表示する 1 行(Diktor の `repl` の `type_of_expr`)。
    ///
    /// 型検査の書き換えは必ず戻す。
    pub fn type_of(&mut self, src: &[u8], line: u32, col0: u32) -> String {
        let decls = match self.parse(src, line, col0) {
            Parsed::Complete(ds) => ds,
            Parsed::Incomplete => return ":type の式が途中で終わっています".to_string(),
            Parsed::Failed(m) => return m,
        };
        let [d] = decls.as_slice() else {
            return ":type には式を 1 つ書いてください".to_string();
        };
        let DeclKind::Exp(e) = &d.kind else {
            return ":type には式を 1 つ書いてください".to_string();
        };
        let snap = self.tc.snapshot();
        let env = self.env.clone();
        let r = self.tc.type_of_expr(&env, e).and_then(|t| self.tc.show(t));
        let line = match r {
            Ok(s) => s,
            Err(f) => render_fail(&self.files, f),
        };
        self.restore(snap);
        line
    }
}

impl Tc {
    /// 入力ごとの状態を初めに戻す(Diktor の `start_input`)。
    pub(crate) fn start_input(&mut self) {
        self.current_out.clear();
        self.warnings.clear();
        self.top_types.clear();
        self.unify_reset();
        self.pending.clear();
        self.pattern_numbers.clear();
        self.group_stack.clear();
        self.opened_rows.clear();
    }

    /// 対話的な実行の入力を平坦化する(Diktor の `flatten_modules ~session_vals`)。
    ///
    /// 入力のトップレベルの値が、前の入力の module の中の値と同名でないかを確かめ、
    /// 前の入力のトップレベルの値の名前を、この入力の module の中の値と突き合わせる。
    pub(crate) fn flatten_input(
        &mut self,
        session_vals: &HashSet<String>,
        decls: Vec<Decl>,
    ) -> R<Vec<Decl>> {
        for node in &decls {
            let names: Vec<String> = match &node.kind {
                DeclKind::Let(b) => pat_var_names(&b.name),
                DeclKind::LetRec(bs) => bs.iter().flat_map(|b| pat_var_names(&b.name)).collect(),
                DeclKind::Extern(ex) => vec![ex.name.clone()],
                _ => Vec::new(),
            };
            for x in names {
                let hit = self
                    .d
                    .module_val_synonyms
                    .keys()
                    .filter(|(_, y)| &*self.name_of(*y) == x.as_str())
                    .map(|(m, _)| m.clone())
                    .min();
                if let Some(m) = hit {
                    let r: R<()> = super::terr(format!(
                        "トップレベルの {x} は、前の入力の module {} の {x} と同名です(module 内の名前とトップレベル名は同名にできません)",
                        super::display(&m)
                    ));
                    super::at(node.meta.span, r)?;
                }
            }
        }
        // 前の入力のトップレベルの値の名前は、この入力の module の中の値と突き合わせるだけに使う。
        // module の無い入力では渡さない(入力ごとに前の入力の数だけ手間がかからないようにする)。
        let prev: Vec<String> = if decls.iter().any(|d| matches!(d.kind, DeclKind::Module(..))) {
            let mut v: Vec<String> = session_vals.iter().cloned().collect();
            v.sort();
            v
        } else {
            Vec::new()
        };
        self.flatten_modules(&prev, &[], ROOT_UNIT, decls)
    }

    /// 1 入力分の宣言を型検査する(Diktor の `check_input`)。
    ///
    /// トップレベルの行に `Print` を足す(プレリュードが所有するときだけ効く)。
    /// 型の行と警告は [`Tc::current_out`] に積む。
    pub(crate) fn check_input(&mut self, env: &Env, decls: &[Decl]) -> R<Env> {
        self.start_input();
        let saved = self.d.current_unit;
        self.d.current_unit = ROOT_UNIT;
        let r = self.process_decls(&["Print".to_string()], env, decls, Emit::All);
        self.d.current_unit = saved;
        r
    }

    /// 式を let の右辺と同じく 1 段深いレベルで推論し、let と同じ値制限で一般化した型を返す
    /// (Diktor の `type_of_expr`。対話的な実行の `:type`)。
    pub(crate) fn type_of_expr(&mut self, env: &Env, e: &crate::ast::Exp) -> R<Ty> {
        self.start_input();
        let eff0 = self.toplevel_eff(&["Print".to_string()]);
        let mark = self.class_mark();
        let t = self.elab_exp(env, 1, eff0, e)?;
        for w in self.ex_drain(0, &[t], true)? {
            self.warn(w);
        }
        if is_value(e) {
            self.check_ambiguity(false, 0, &[t])?;
            self.generalize(0, t)?;
        } else {
            self.lower_levels(mark, 0, t)?;
            self.check_ambiguity(true, 0, &[t])?;
            self.default_numerics()?;
        }
        self.check_pattern_numbers()?;
        Ok(t)
    }
}

/// 型検査の文脈の印([`Tc::snapshot`])。
pub(crate) struct Snapshot {
    /// 書き換えを記録する表を除いた文脈の写し(入力ごとに初めに戻す、小さな欄だけが残る)。
    rest: Box<Tc>,
    /// 型の表の長さ。
    tys: usize,
    /// 宣言表の、表でない欄。
    decls: super::decls::DeclsMark,
}

/// 型検査の文脈のうち、印を取るときに写さず、印の後の書き換えの記録で戻す欄である。
///
/// 入力の数とともに伸びる表(型、セル、カインド、穴、木の注釈、宣言表、名前の表など)をすべて含む。
struct Big {
    tys: Vec<super::TyNode>,
    cells: Vec<super::TVar>,
    kinds: Vec<super::KNode>,
    holes_arena: Vec<super::Hole>,
    node_ty: super::fx::NodeMap<Ty>,
    resolved: super::fx::NodeMap<super::Resolved>,
    dict: super::fx::NodeMap<super::DictInfo>,
    cell_trail: super::fx::Trail<super::TVar>,
    hole_trail: super::fx::Trail<super::Hole>,
    kind_trail: super::fx::Trail<super::KNode>,
    d: super::decls::Decls,
    rigid_tparams: super::fx::JMap<super::Oid, String>,
    intern_map: super::fx::JMap<std::rc::Rc<str>, super::Oid>,
    name_map: super::fx::JMap<super::Oid, std::rc::Rc<str>>,
}

impl Tc {
    fn take_big(&mut self) -> Big {
        use std::mem::take;
        Big {
            tys: take(&mut self.tys),
            cells: take(&mut self.cells),
            kinds: take(&mut self.kinds),
            holes_arena: take(&mut self.holes_arena),
            node_ty: take(&mut self.node_ty),
            resolved: take(&mut self.resolved),
            dict: take(&mut self.dict),
            cell_trail: take(&mut self.cell_trail),
            hole_trail: take(&mut self.hole_trail),
            kind_trail: take(&mut self.kind_trail),
            d: take(&mut self.d),
            rigid_tparams: take(&mut self.rigid_tparams),
            intern_map: take(&mut self.intern_map),
            name_map: take(&mut self.name_map),
        }
    }

    fn put_big(&mut self, b: Big) {
        self.tys = b.tys;
        self.cells = b.cells;
        self.kinds = b.kinds;
        self.holes_arena = b.holes_arena;
        self.node_ty = b.node_ty;
        self.resolved = b.resolved;
        self.dict = b.dict;
        self.cell_trail = b.cell_trail;
        self.hole_trail = b.hole_trail;
        self.kind_trail = b.kind_trail;
        self.d = b.d;
        self.rigid_tparams = b.rigid_tparams;
        self.intern_map = b.intern_map;
        self.name_map = b.name_map;
    }

    /// 文脈の印を取る(対話的な実行の入力の取り消し。計画 7 節の snapshot)。
    ///
    /// 入力の数とともに伸びる表([`Big`])は写さず、印の後の書き換えを記録する。
    /// 型の表は足すだけなので長さを覚え、ほかの表は書き換える前の値を覚える。
    /// 残りの欄は写すが、どれも入力ごとに初めに戻す小さなものである。
    /// そのため、印を取る手間は、それまでの入力の数と大きさに依らない。
    /// 印は入れ子にできる。
    /// 取った印は、[`Tc::rollback`] で戻すか [`Tc::commit`] で外す。
    pub(crate) fn snapshot(&mut self) -> Snapshot {
        let big = self.take_big();
        let rest = Box::new(self.clone());
        self.put_big(big);
        self.cell_trail.begin(self.cells.len());
        self.hole_trail.begin(self.holes_arena.len());
        self.kind_trail.begin(self.kinds.len());
        self.node_ty.begin();
        self.resolved.begin();
        self.dict.begin();
        self.rigid_tparams.begin();
        self.intern_map.begin();
        self.name_map.begin();
        let decls = self.d.begin();
        Snapshot {
            rest,
            tys: self.tys.len(),
            decls,
        }
    }

    /// 印 `s` を外し、印の後の書き換えを残す。
    pub(crate) fn commit(&mut self, s: Snapshot) {
        drop(s);
        self.cell_trail.commit();
        self.hole_trail.commit();
        self.kind_trail.commit();
        self.node_ty.commit();
        self.resolved.commit();
        self.dict.commit();
        self.rigid_tparams.commit();
        self.intern_map.commit();
        self.name_map.commit();
        self.d.commit();
    }

    /// 文脈を印 `s` の時点に戻す(通し番号と名前の表も戻す)。
    pub(crate) fn rollback(&mut self, s: Snapshot) {
        self.rollback_with(s, false);
    }

    /// 文脈を印 `s` の時点に戻す。`keep_names` なら、通し番号と名前の表は今のものを残す。
    pub(crate) fn rollback_with(&mut self, s: Snapshot, keep_names: bool) {
        let next_oid = self.next_oid;
        self.cell_trail.rollback(&mut self.cells);
        self.hole_trail.rollback(&mut self.holes_arena);
        self.kind_trail.rollback(&mut self.kinds);
        self.node_ty.rollback();
        self.resolved.rollback();
        self.dict.rollback();
        self.rigid_tparams.rollback();
        if keep_names {
            // 外側の印のために記録は残す。
            self.intern_map.commit();
            self.name_map.commit();
        } else {
            self.intern_map.rollback();
            self.name_map.rollback();
        }
        self.d.rollback(s.decls);
        self.tys.truncate(s.tys);
        let big = self.take_big();
        *self = *s.rest;
        self.put_big(big);
        if keep_names {
            self.next_oid = next_oid;
        }
    }
}
