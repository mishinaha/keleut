//! 対話的な実行の型検査の側である(Diktor の driver.ml の §16.6b と elab.ml の §11.44)。
//!
//! [`Session`] は、プレリュードを 1 回だけ処理し、その後は入力ごとに次を行う。
//!
//! 1. 入力を構文解析する([`Session::parse`])。
//!    文法に合わなかったトークンが `EOF` なら、続きの行を待つ。
//! 2. 平坦化して型検査し、受理した入力なら、プレリュード、受理した前の入力、この入力を単位とする KIR を書く
//!    ([`Session::check`])。
//!    この入力の単位は最後の単位 `main` で、前の入力の単位の鍵は `repl/1`、`repl/2` …である。
//!    この入力の単位は、結果の表示に使う隠れた大域(式文の値と、`Show.show` を呼ぶ関数)を持つ。
//! 3. 呼び出し側が KIR を実行し、その結果で入力を受理するか捨てるかを決める([`Session::finish`])。
//!
//! 受理しなかった入力(構文エラー、型エラー、実行時エラー)は捨てる。
//! 型検査の文脈は入力の前に写しを取り、捨てるときは写しに戻す。
//! そのため、捨てた入力の宣言、束縛、インスタンスと、一般化しなかった束縛の型変数に決めた型は、後の入力から見えない。
//! 写しに戻しても、通し番号と名前の表(Diktor が戻さないもの)は捨てた入力の後のものを残す。

use super::check::{PRELUDE, Stop, fail_to_error, parse_unit, render_line};
use super::decls::{ROOT_UNIT, STD_UNIT};
use super::elab::{is_value, pat_var_names};
use super::emit::{ReplUnit, UnitSrc, program_repl};
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
/// 実行時のレコードはフィールドを正準の配置(ラベルの番号の順)で持つが、Diktor はレコードを拡張の順(外側から)に表示する。
/// 型の行も拡張の順に並ぶので、表示はレコードのフィールドを型の行の順に並べ替える。
/// 配置は再帰する型のために表の添字で指し合う。
/// 0 番は型の分からない値である。
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
const LAYOUT_BUDGET: usize = 4096;
/// 型の鍵の長さの上限。超えた型は型の分からない値として表示する。
const LAYOUT_KEY_MAX: usize = 4096;

/// [`Session::check`] の結果である。
pub enum Outcome {
    /// 受理しなかった。報告する行(警告と誤り)である。
    Rejected(Vec<String>),
    /// 型検査に通った。呼び出し側は警告の行を出し、KIR を実行してから [`Session::finish`] を呼ぶ。
    Run {
        /// 警告の行。
        lines: Vec<String>,
        /// プレリュード、受理した前の入力、この入力を単位とするプログラム。
        program: Box<kir::Program>,
        /// 結果を表示する束縛(入力の順)。
        shown: Vec<Shown>,
        /// 値の配置。
        layouts: Layouts,
    },
}

/// 型検査に通り、実行を待つ入力である。
struct PendingInput {
    snapshot: Box<Tc>,
    flat: Vec<Decl>,
    env: Env,
}

/// 対話的な実行の型検査の状態である。
pub struct Session {
    opts: SessionOptions,
    tc: Tc,
    files: Files,
    /// 入力の `<stdin>` のファイルの添字(入力ごとに足さない)。
    stdin: FileId,
    cx: Ctx,
    prelude: Vec<Decl>,
    /// プレリュードだけを処理した後の環境に、受理した入力の束縛を足した環境。
    env: Env,
    /// 受理した入力の、平坦化した宣言の列。
    units: Vec<Vec<Decl>>,
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
        Ok(Session {
            opts,
            tc,
            files,
            stdin,
            cx,
            prelude,
            env,
            units: Vec::new(),
            session_vals: HashSet::default(),
            print_owned,
            pending: None,
        })
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

    /// 型検査の文脈を写しに戻す。通し番号と名前の表は今のものを残す。
    fn restore(&mut self, snap: Tc) {
        let next_oid = self.tc.next_oid;
        let intern_map = std::mem::take(&mut self.tc.intern_map);
        let name_map = std::mem::take(&mut self.tc.name_map);
        self.tc = snap;
        self.tc.next_oid = next_oid;
        self.tc.intern_map = intern_map;
        self.tc.name_map = name_map;
    }

    /// 1 つの入力を平坦化して型検査し、受理できれば KIR を書く(Diktor の `repl` の `process` の前半)。
    ///
    /// 受理しなかったときは、型検査の文脈を入力の前に戻してから返す。
    /// [`Outcome::Run`] を返したときは、呼び出し側が [`Session::finish`] を呼ぶ。
    pub fn check(&mut self, decls: Vec<Decl>) -> Outcome {
        let snap = self.tc.clone();
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
        match self.prepare(&flat) {
            Ok((program, shown, layouts)) => {
                self.pending = Some(PendingInput {
                    snapshot: Box::new(snap),
                    flat,
                    env,
                });
                Outcome::Run {
                    lines,
                    program: Box::new(program),
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
                self.restore(snap);
                Outcome::Rejected(lines)
            }
        }
    }

    /// 結果を表示する束縛を集め、KIR を書く。
    fn prepare(&mut self, flat: &[Decl]) -> R<(kir::Program, Vec<Shown>, Layouts)> {
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
        let mut probe: Option<Box<Tc>> = None;
        for (x, t, key, is_exp) in items {
            let label = format!("{} : {}", super::display(&x), self.tc.show(t)?);
            let unit_exp = is_exp && {
                let r = self.tc.repr(t);
                match self.tc.node(r) {
                    TyNode::Record(row) => self.tc.repr_is_empty(row),
                    _ => false,
                }
            };
            let show = if !unit_exp && self.shows_newtype(t, &mut probe)? {
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
        let quiet = ReplUnit {
            print: self.print_owned,
            display: false,
            shows: Vec::new(),
        };
        let cur = ReplUnit {
            print: self.print_owned,
            display: true,
            shows,
        };
        let prelude_paths: Vec<Vec<u8>> = if self.opts.no_prelude {
            Vec::new()
        } else {
            vec![
                self.opts
                    .prelude
                    .clone()
                    .unwrap_or_else(|| "<prelude>".to_string())
                    .into_bytes(),
            ]
        };
        let mut srcs = vec![UnitSrc {
            key: "prelude".to_string(),
            paths: prelude_paths,
            decls: &self.prelude,
            repl: None,
        }];
        for (i, u) in self.units.iter().enumerate() {
            srcs.push(UnitSrc {
                key: format!("repl/{}", i + 1),
                paths: vec![b"<stdin>".to_vec()],
                decls: u,
                repl: Some(&quiet),
            });
        }
        srcs.push(UnitSrc {
            key: "main".to_string(),
            paths: vec![b"<stdin>".to_vec()],
            decls: flat,
            repl: Some(&cur),
        });
        let (program, out) = program_repl(&mut self.tc, &srcs)?;
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
        Ok((program, shown, layouts))
    }

    /// 型の構造の鍵。型変数は `?` にする(型変数の位置の値は型の分からない値として表示する)。
    fn layout_key(&mut self, t: Ty, out: &mut String) -> R<()> {
        super::guard()?;
        if out.len() > LAYOUT_KEY_MAX {
            return Ok(());
        }
        let t = self.tc.repr(t);
        match self.tc.node(t) {
            TyNode::Con(n, args) => {
                out.push_str(&format!("C{n}["));
                for a in args.iter() {
                    self.layout_key(*a, out)?;
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
                for (l, f) in fs {
                    out.push_str(&format!("{l}:"));
                    self.layout_key(f, out)?;
                    out.push(',');
                }
                out.push('}');
            }
            _ => out.push('?'),
        }
        Ok(())
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
        self.layout_key(t, &mut key)?;
        if key.len() > LAYOUT_KEY_MAX || ls.nodes.len() >= LAYOUT_BUDGET {
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
    /// 満たすかどうかは、写した型検査の文脈 `probe` に `Show` の制約を足して確かめる。
    /// 写しは 1 つの入力の中で使い回し、制約を足すのに失敗したときと、型に決まっていない型変数があるときに捨てる
    /// (足した制約が、後の束縛の確かめに効かないようにする)。
    fn shows_newtype(&mut self, t: Ty, probe: &mut Option<Box<Tc>>) -> R<bool> {
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
        let mut vars = Vec::new();
        self.tc.unbound_vars(&mut vars, r)?;
        let p = probe.get_or_insert_with(|| Box::new(self.tc.clone()));
        let ok = p.add_class(None, r, show).is_ok();
        if !ok || !vars.is_empty() {
            *probe = None;
        }
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
    /// `accepted` が偽なら、型検査の文脈を入力の前に戻す。
    pub fn finish(&mut self, accepted: bool) {
        let Some(p) = self.pending.take() else {
            return;
        };
        if accepted {
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
            self.units.push(p.flat);
            self.env = p.env;
        } else {
            self.restore(*p.snapshot);
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
        let snap = self.tc.clone();
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
        let mut prev: Vec<String> = session_vals.iter().cloned().collect();
        prev.sort();
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
