//! 対話的な実行(REPL)のうち、型検査を行う部分である(Diktor の driver.ml の `repl` と、elab.ml の `check_input`、`type_of_expr` に対応する)。
//!
//! [`Session`] は、プレリュードを 1 回だけ処理し、その後は入力ごとに次の処理を行う。
//!
//! 1. 入力を構文解析する([`Session::parse`])。
//!    文法に合わなかったトークンが `EOF` なら、続きの行を待つ。
//! 2. 入力を平坦化して型検査する([`Session::check`])。
//!    平坦化は、module の中の宣言を、修飾名を付けたトップレベルの宣言に展開する処理である。
//!    型検査に通った入力は、1 つの単位として KIR に書き、セッションのプログラムの末尾に追加する([`ReplWriter`])。
//!    プログラムは、組み込みの単位、プレリュードの単位、追加した入力の単位(キーは `repl/1`、`repl/2` …)からなる。
//!    入力の単位は、結果の表示に使う、ソースにはないグローバルを持つ。
//!    式文の値を入れるグローバルと、`Show.show` を呼ぶ関数を入れるグローバルである。
//! 3. 呼び出し側が、追加した単位だけを実行する。
//!    その結果を受け取って、入力を受理するか捨てるかを決める([`Session::finish`])。
//!
//! 受理しなかった入力(構文エラー、型エラー、実行時エラーになった入力)は捨てる。
//! 型検査のコンテキストは、入力の前にマークを取り(`Tc::snapshot`)、入力を捨てるときにマークの時点に戻す。
//! 入力の数とともに大きくなる表(型、セル、穴、木の注釈、宣言表、名前の表など)は、マークの後の書き換えの記録を使って戻す。
//! 入力ごとに初期状態に戻す小さなフィールドだけは、マークを取るときにコピーしておき、そのコピーから戻す。
//! そのため、捨てた入力の宣言、束縛、インスタンスと、一般化しなかった束縛の型変数に決めた型は、後の入力から見えない。
//! ただし、通し番号と名前の表は Diktor が戻さないので、マークの時点に戻しても、捨てた入力の後の状態を残す(`Session::restore`)。
//! KIR のライタの名前の表も、同じようにマークを取って入力の前に戻す。
//! 一方、実行した単位は、入力を捨てるときもプログラムに残す。
//! 関数とグローバルの番号を詰め直さずに済ませるためである。
//! 実行時エラーになった入力のグローバルの値は、呼び出し側が捨てる。

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

/// 対話的な実行のオプションである。
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// `--no-prelude` を指定したら真。プレリュードを読まない。
    pub no_prelude: bool,
    /// `--prelude PATH` で指定した、プレリュードのファイルのパス。指定がなければ、組み込みのプレリュードを使う。
    pub prelude: Option<String>,
    /// 構文解析で作る木の深さの上限。
    pub max_nest: usize,
}

/// 入力を構文解析した結果である(Diktor の `parse_result`)。
#[derive(Debug)]
pub enum Parsed {
    /// 入力が完結している。値は宣言の並びである。
    Complete(Vec<Decl>),
    /// 入力が途中で終わっているので、続きの行を待つ。
    Incomplete,
    /// エラー。値は報告する 1 行である。
    Failed(String),
}

/// セッションを始められなかった理由である([`Session::new`])。
#[derive(Debug)]
pub enum StartError {
    /// プレリュードの読み込みエラー、字句エラー、構文エラー、内部エラー。終了コードは `keleut check` の [`Stop`] と同じである。
    Stop(Stop),
    /// プレリュードの型エラー(終了コード 1)。値は報告する 1 行である。
    Type(String),
    /// プレリュードを KIR に書き出せない。内部エラーで、終了コードは 70 である。値は報告する 1 行である。
    Emit(String),
}

impl From<Stop> for StartError {
    fn from(s: Stop) -> StartError {
        StartError::Stop(s)
    }
}

/// 結果を表示する束縛である(Diktor の `exec_input` の `shown` の項目)。
#[derive(Clone, Debug)]
pub struct Shown {
    /// 表示する行のうち、`名前 : 型` の部分。
    pub label: String,
    /// 値を持つグローバル(KIR の添字)。
    pub global: kir::GlobalId,
    /// 型が `{}` の式文なら真。このときは何も表示しない。
    pub unit_exp: bool,
    /// 値を `Show.show` で表示するときに使う、`$show` のグローバル。
    pub show: Option<kir::GlobalId>,
    /// 値の型のレイアウト([`Layouts`] の添字)。
    pub layout: usize,
}

/// 値の表示に使うレイアウトである。値の型から作る。
///
/// 実行時のレコードは、フィールドを正準なレイアウト(ラベルの番号の順)で持つ。
/// 値を表示するときは、フィールドの順序を型の表示(`名前 : 型 = 値` の型)にそろえるために、レコードのフィールドをレコード型の行(フィールドの並び)の順に並べ替える。
/// ラベルの番号は、受理しなかった入力を含むセッションの履歴で決まるので、正準なレイアウトの順序は表示に使わない。
/// 再帰する型を表せるように、レイアウトは子のレイアウトを表の添字で参照する。
/// 0 番は、型の分からない値のレイアウトである。
/// 型の分からない値のレコードは、フィールドをラベル名の順に表示する(`keleut repl` の `show_value`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    /// 型の分からない値(型変数、関数など)。子も型の分からない値として表示する。
    Any,
    /// レコードとタプル。ラベル(KIR での名前)と子のレイアウトの組を、レコード型の行の順に並べたもの。
    Record(Vec<(String, usize)>),
    /// ヴァリアント。ラベルとペイロードのレイアウトの組の並び。
    Variant(Vec<(String, usize)>),
    /// データ。コンストラクタの表示名と、各フィールドのレイアウトの並びの組を並べたもの。
    Data(Vec<(String, Vec<usize>)>),
    /// `Array` と `MutableArray`。値は要素のレイアウトである。
    Array(usize),
}

/// [`Layout`] の表である。
#[derive(Clone, Debug, Default)]
pub struct Layouts {
    /// レイアウトの並び。0 番は [`Layout::Any`] である。
    pub nodes: Vec<Layout>,
}

/// レイアウトの表に置く項目数の上限である。正則でない再帰型で、表が際限なく大きくならないようにする。
/// 項目数が上限に達した後の型は、型の分からない値として表示する。
const LAYOUT_BUDGET: usize = 4096;
/// レイアウトを作る型のサイズの上限である。サイズは `Session::layout_key` が数え、サイズがこの値以下の型にレイアウトを作る。
///
/// 上限を超えた型は、型の分からない値として表示する。
/// 上限は表示する値全体の型に適用するので、外側の型のサイズが上限を超えると、内側のレコードにもレイアウトを作らない。
/// `Int32` のフィールドが N 個のレコードのサイズは 2N + 1 なので、表示する値全体がそのレコードのとき、レイアウトを使うのはフィールドが 409 個までである。
/// サイズは型の構造だけで決まり、通し番号の桁数によらないので、レイアウトを使うかどうかは、受理しなかった入力に影響されない。
const LAYOUT_SIZE_MAX: usize = 820;

/// [`Session::check`] の結果である。
pub enum Outcome {
    /// 受理しなかった。値は報告する行(警告とエラー)である。
    Rejected(Vec<String>),
    /// 型検査に通った。呼び出し側は警告の行を出力し、追加した単位を実行してから [`Session::finish`] を呼ぶ。
    Run {
        /// 警告の行。
        lines: Vec<String>,
        /// セッションのプログラム。最後の単位がこの入力である。
        /// 実行した後、[`Session::finish`] に渡してセッションに返す。
        program: Box<kir::Program>,
        /// 結果を表示する束縛。入力に書いた順に並ぶ。
        shown: Vec<Shown>,
        /// 値のレイアウトの表。
        layouts: Layouts,
    },
}

/// 型検査に通り、実行を待っている入力である。
struct PendingInput {
    snapshot: Snapshot,
    flat: Vec<Decl>,
    env: Env,
    /// 入力の前に取った、ライタの名前の表のマーク。
    writer: WriterMark,
    /// 入力の単位を追加する前のプログラムのサイズ。
    program: ProgramMark,
}

/// 入力の単位を実行した結果である([`Session::finish`] に渡す)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// 最後まで実行した。入力を受理する。
    Accepted,
    /// 実行時エラーなどで、実行が最後まで進まなかった。入力を捨てるが、実行した単位はプログラムに残す。
    Failed,
    /// 実行しなかった。入力を捨て、追加した単位をプログラムから取り除く。
    /// 入力の単位が処理系の限度に当たったときと、実行の前に標準出力へ書けなかったときである。
    /// 処理系の限度に当たるのは、入れ子が KIR の上限を超えたときと、KIR を処理するスレッドを作れなかったときである。
    Unrun,
}

/// 対話的な実行のうち、型検査を行う部分の状態である。
pub struct Session {
    opts: SessionOptions,
    tc: Tc,
    files: Files,
    /// 入力を表すファイル `<stdin>` の添字。入力ごとに新しいファイルを追加せず、同じ添字を使う。
    stdin: FileId,
    cx: Ctx,
    /// プレリュードだけを処理した後の環境に、受理した入力の束縛を追加した環境。
    env: Env,
    /// KIR のライタ。
    writer: ReplWriter,
    /// セッションのプログラム。入力を実行している間は呼び出し側が持つ。
    program: Option<Box<kir::Program>>,
    /// 次に追加する単位の番号。受理しなかった入力の単位のキーも再利用しない。
    next_unit: u32,
    /// 受理した入力のトップレベルの値の名前(Diktor の `session_vals`)。
    session_vals: HashSet<String>,
    /// エフェクト `Print` がプレリュードの宣言なら真。
    print_owned: bool,
    pending: Option<PendingInput>,
}

/// 型検査のエラーを、報告する 1 行にする。内部エラーは `[BUG]` の付いたメッセージのまま返す。
fn render_fail(files: &Files, f: Fail) -> String {
    match fail_to_error(files, f) {
        Ok(e) => e.render(),
        Err(m) => m,
    }
}

impl Session {
    /// プレリュードを処理したセッションを作る(Diktor の `repl` の `start`)。
    ///
    /// 型検査と同じく、[`on_check_stack`](super::check::on_check_stack) が作るスレッドの中で呼ぶ。
    /// プレリュードの型エラーは [`StartError::Type`] で返す。
    /// Diktor は、同じエラーを標準エラーに書いて終了コード 1 で終了する。
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

    /// プレリュードまでを書いたプログラムを取り出す。
    ///
    /// セッションの開始時と `:reset` の後に、プレリュードの初期化を実行するために使う。
    ///
    /// 実行した後、[`Session::put_program`] でセッションに返す。
    pub fn take_program(&mut self) -> Option<Box<kir::Program>> {
        self.program.take()
    }

    /// [`Session::take_program`] で取り出したプログラムをセッションに返す。
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
    /// `src` の 1 行目を `line` 行目として扱い、1 行目の桁を `col0` だけずらす。
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

    /// 型検査のコンテキストをマークの時点に戻す。通し番号と名前の表は、現在のものを残す。
    fn restore(&mut self, snap: Snapshot) {
        self.tc.rollback_with(snap, true);
    }

    /// 1 つの入力を平坦化して型検査し、受理できれば KIR を書く(Diktor の `repl` の `process` の前半)。
    ///
    /// 受理しなかったときは、型検査のコンテキストを入力の前に戻してから返す。
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

    /// 結果を表示する束縛を集め、入力の単位を KIR に書いて `program` の末尾に追加する。
    ///
    /// エラーを返すときは、ライタが途中まで追加した項目を取り除き、ライタの表を元に戻してある。
    fn prepare(&mut self, flat: &[Decl], program: &mut kir::Program) -> R<(Vec<Shown>, Layouts)> {
        // 束縛ごとの (表示名, 型, グローバルを引くキー, 式文なら真)。
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
        // (表示名と型, グローバルを引くキー, 型が {} の式文なら真, shows の添字, レイアウト)。
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

    /// 型の構造を表すキーを `out` に書く。
    ///
    /// 型変数は `?` にする。型変数の位置の値は、型の分からない値として表示するからである。
    ///
    /// `size` に型のサイズを加算していき、[`LAYOUT_SIZE_MAX`] を超えたら、たどるのをやめて `false` を返す。
    /// サイズは、たどった型のノード(型構成子、レコード、ヴァリアント、`?`)の個数と、レコードとヴァリアントのフィールドの個数の和である。
    /// 通し番号の表記の長さは含めない。
    /// サイズが上限以下の型ではキーを最後まで作るので、別の型が同じキーになることはない。
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

    /// 型 `t` のレイアウトを表に追加し、その添字を返す。レイアウトを作らない型には 0 を返す。
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

    /// 型の先頭の型構成子が newtype で、その型が `Show` を満たせば真を返す(Diktor の `show_value` の `shows`)。
    ///
    /// 満たすかどうかは、型検査のコンテキストに `Show` の制約を追加して調べる。
    /// 調べた後で、コンテキストを制約を追加する前の状態に戻す。
    /// 追加した制約が、後の束縛の検査と表示に影響しないようにするためである。
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
            // インスタンスがなければ、制約を追加しても失敗するだけなので、追加せずに偽を返す。
            return Ok(false);
        }
        let snap = self.tc.snapshot();
        let ok = self.tc.add_class(None, r, show).is_ok();
        self.tc.rollback(snap);
        Ok(ok)
    }

    /// 結果の表示に使う証拠を作る(Diktor の `Unify.display_evidence`)。
    ///
    /// 決まっていない部分は辞書の引数にして、そのキーを `pend` に追加する。
    /// これは Diktor の解けていない穴に相当する。
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

    /// 実行の結果に従って、入力を受理するか捨てるかを決める(Diktor の `repl` の `process` の後半)。
    ///
    /// `program` は [`Outcome::Run`] で渡したプログラムである。
    /// 受理しないときは、型検査のコンテキストとライタの名前の表を入力の前に戻す。
    /// [`Finish::Unrun`] のときは、追加した単位もプログラムから取り除く。
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

    /// `:type` に書いた式の型を表示する 1 行を返す(Diktor の `repl` の `type_of_expr`)。
    ///
    /// 型検査が行った書き換えは、成功しても失敗しても元に戻す。
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
    /// 入力ごとの状態を初期状態に戻す(Diktor の `start_input`)。
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
    /// 入力のトップレベルの値が、前の入力の module の中の値と同名でないことを検査する。
    /// 前の入力のトップレベルの値の名前は、この入力の module の中の値と照合する。
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
        // 前の入力のトップレベルの値の名前は、この入力の module の中の値と照合するためだけに使う。
        // module のない入力では渡さない。入力のたびに、前の入力の数に比例する時間がかからないようにするためである。
        let prev: Vec<String> = if decls.iter().any(|d| matches!(d.kind, DeclKind::Module(..))) {
            let mut v: Vec<String> = session_vals.iter().cloned().collect();
            v.sort();
            v
        } else {
            Vec::new()
        };
        self.flatten_modules(&prev, &[], ROOT_UNIT, decls)
    }

    /// 1 つの入力の宣言を型検査する(Diktor の `check_input`)。
    ///
    /// `Print` がプレリュードの宣言であるときだけ、トップレベルのエフェクト行に `Print` を追加する。
    /// 型を表示する行と警告は、[`Tc::current_out`] に追加する。
    pub(crate) fn check_input(&mut self, env: &Env, decls: &[Decl]) -> R<Env> {
        self.start_input();
        let saved = self.d.current_unit;
        self.d.current_unit = ROOT_UNIT;
        let r = self.process_decls(&["Print".to_string()], env, decls, Emit::All);
        self.d.current_unit = saved;
        r
    }

    /// 式を `let` の右辺と同じく 1 段深いレベルで推論し、`let` と同じ値制限で一般化した型を返す。
    ///
    /// Diktor の `type_of_expr` に対応し、対話的な実行の `:type` が使う。
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

/// 型検査のコンテキストのマークである([`Tc::snapshot`] が返す)。
pub(crate) struct Snapshot {
    /// 書き換えを記録する表を除いた、コンテキストのコピー。入力ごとに初期状態に戻す小さなフィールドだけが入っている。
    rest: Box<Tc>,
    /// 型の表の長さ。
    tys: usize,
    /// 宣言表のうち、表でないフィールドのマーク。
    decls: super::decls::DeclsMark,
}

/// 型検査のコンテキストのうち、マークを取るときにコピーせず、マークの後の書き換えの記録を使って戻すフィールドである。
///
/// 入力の数とともに大きくなる表(型、セル、カインド、穴、木の注釈、宣言表、名前の表など)をすべて含む。
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

    /// コンテキストのマークを取る。対話的な実行が、入力を取り消すために使う。
    ///
    /// 入力の数とともに大きくなる表([`Big`])はコピーせず、マークの後の書き換えを記録する。
    /// 型の表は末尾に追加するだけなので長さを記録し、それ以外の表は書き換える前の値を記録する。
    /// 残りのフィールドはコピーするが、どれも入力ごとに初期状態に戻す小さなものである。
    /// そのため、マークを取るのにかかる時間は、それまでの入力の数とサイズによらない。
    /// マークは入れ子にできる。
    /// 取ったマークは、[`Tc::rollback`] でその時点に戻すか、[`Tc::commit`] で外す。
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

    /// マーク `s` を外し、マークの後の書き換えを残す。
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

    /// コンテキストをマーク `s` の時点に戻す。通し番号と名前の表も戻す。
    pub(crate) fn rollback(&mut self, s: Snapshot) {
        self.rollback_with(s, false);
    }

    /// コンテキストをマーク `s` の時点に戻す。`keep_names` が真なら、通し番号と名前の表は現在のものを残す。
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
            // 外側のマークのために、書き換えの記録は残す。
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
