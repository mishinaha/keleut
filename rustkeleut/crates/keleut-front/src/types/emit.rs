//! 型検査を済ませた単位の列から KIR への書き出しである(Diktor の ir_write.ml、第18章)。
//!
//! 規範は親リポジトリの `doc/IRSpec.md` で、出力は `diktor --emit-ir` と `--emit-ir-text` とバイト単位で同じにする。
//! 名前の振り方、スロットと捕獲の番号、表の並びは、どれも Diktor の書き手と同じ規則で決める。
//!
//! 書き出しは Diktor と同じく 2 段で行う。
//! 1 段目は、すべての単位の宣言をたどって、型、エフェクト、クラス、インスタンス、大域を表に置き、
//! 単位ごとの初期化の手順と、根の関数を書く仕事の列を作る。
//! 2 段目は、仕事を順に行い、根の関数とその持ち上げた関数を関数の表に足す。
//!
//! Diktor は記号の文字列で表の項目を指す KIR を作ってから、符号化のときに添字を引く。
//! 本モジュールは [`kir::Program`] の添字を直に書く。
//! 型、エフェクト、クラス、インスタンス、大域の添字は 1 段目で決まるので、その場で書く。
//! 関数の添字は、根の関数を書き終えて持ち上げた関数を場所の番号の順に並べるまで決まらない。
//! そこで、持ち上げた関数を作る式(`closure`、`dictabs`、`handle`)の部分には枠の番号を仮に書き、
//! 根の関数を書き終えてから関数の添字と捕獲の列に書き換える。
//! 捕獲の番号も、Diktor と同じく枠をまたいで一意の仮の番号で書き、根の関数の終わりに書き換える。
//! 書き換える式と辞書は、作ったときに列に記録しておくので、木をたどり直さない。
//!
//! 式、辞書、パターンのノードを [`kir::Nodes`] に置く順は出力に現れない
//! (テキストとバイナリの書き手は、関数の本体から木をたどる)。

use super::check::{CheckOptions, Checked};
use super::exhaust::{ocaml_int32_opt, ocaml_int64_opt};
use super::fx::HashMap;
use super::pmap::PMap;
use super::{DKey, DictInfo, Evidence, GRef, HoleId, Oid, R, Resolved, Tc, Ty, TyNode, bug};
use crate::ast::{BinOp, Clause, TypeExpKind};
use crate::ast::{Decl, DeclKind, Exp, ExpKind, LetBinding, NodeId, Number, Pat, PatKind};
use kir::{
    Abi, Cap, ClassId, ClauseClass, Dict, DictId, EffectId, ExpId, FnId, FnKind, GlobalId,
    GlobalKind, HandlerOp, InstanceId, Lit, Nodes, Part, PatId, RecordTail, Rep, Step, TypeId,
};
use std::rc::Rc;

/// 書き出す単位(Diktor の `unit_src`)である。
pub struct UnitSrc<'a> {
    /// 単位の鍵。
    pub key: String,
    /// 元のファイルの綴り。
    pub paths: Vec<Vec<u8>>,
    /// 平坦化と型検査を済ませた宣言の列。
    pub decls: &'a [Decl],
    /// 対話的な実行の入力の単位の書き出し方(ファイルの単位では `None`)。
    pub repl: Option<&'a ReplUnit>,
}

/// 対話的な実行の入力の単位の書き出し方である(Diktor の driver.ml の §16.6b)。
///
/// Diktor の対話的な実行は、トップレベルの行に `Print` を足し、最も外側のハンドラが `Print.print` を出力先へ送る。
/// KIR には実行時が持つ最も外側のハンドラが無いので、この単位の初期化式と式文の `thunk` と、
/// 辞書を受け取らないメソッドの値の初期化を、書き手が作る `Print` のハンドラ式で包む。
/// ハンドラは `print(s)` を `resume(perform Console.write(s))` に翻訳するので、印字される文字列は同じである。
/// プレリュードの `with_stdout` は使わないので、`--prelude` のプレリュードが `with_stdout` を持たなくても、
/// 違う `with_stdout` を持っても、結果は同じである。
#[derive(Clone, Debug, Default)]
pub struct ReplUnit {
    /// `Print` をハンドラ式で処理する(プレリュードが、引数 1 個の操作 `print` を持つ `Print` と、
    /// 実行時が処理する `Console.write` を持つときだけ効く)。
    pub print: bool,
    /// 結果を表示する入力か。
    /// 真なら、式文の値を隠れた大域(`$it`)に束縛し、辞書を受け取る式文は辞書を受け取る大域にする。
    pub display: bool,
    /// `Show.show` で表示する値ごとの、`Show` の証拠と、証拠の中の決まっていない部分の辞書の引数。
    /// 値ごとに隠れた大域(`$show`)を作る。
    /// 辞書の引数が無ければ種類 `fn` の関数(値を受け取って文字列を返す)、あれば種類 `dictabs` の関数
    /// (辞書を受け取って、値を受け取る関数を返す)である。
    pub shows: Vec<(Evidence, Vec<DKey>)>,
}

/// 対話的な実行の入力の単位を書き出した結果である。
#[derive(Clone, Debug, Default)]
pub struct ReplOut {
    /// 束縛のノードの番号と名前から大域へ(式文はノードの番号と `$it`)。
    pub globals: Vec<((NodeId, String), GlobalId)>,
    /// [`ReplUnit::shows`] の順の、`$show` の大域。
    pub shows: Vec<GlobalId>,
}

/// 型検査を済ませたプログラムを KIR にする(Diktor の driver.ml の `ir_program`)。
///
/// `keys` は [`Checked::unit_keys`] の結果である。
/// 単位はプレリュード、import した単位(初期化の順)、起点の順に並べる。
/// 型検査が誤りなく終わった `checked` を渡す。
/// 型検査と同じ [`check::on_check_stack`](super::check::on_check_stack) のスレッドの中で呼ぶ。
pub fn program_of_checked(
    checked: &mut Checked,
    keys: &[(String, String)],
    opts: &CheckOptions,
) -> R<kir::Program> {
    let Checked {
        tc, prelude, units, ..
    } = checked;
    if units.len() != keys.len() + 1 {
        return bug("KIR の書き出し: 単位の数が鍵の数と合いません");
    }
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
    let mut srcs = vec![UnitSrc {
        key: "prelude".to_string(),
        paths: prelude_paths,
        decls: prelude,
        repl: None,
    }];
    for ((k, d), ds) in keys.iter().zip(units.iter()) {
        srcs.push(UnitSrc {
            key: k.clone(),
            paths: vec![d.clone().into_bytes()],
            decls: ds,
            repl: None,
        });
    }
    srcs.push(UnitSrc {
        key: "main".to_string(),
        paths: opts.files.iter().map(|f| f.clone().into_bytes()).collect(),
        decls: units.last().map(|v| v.as_slice()).unwrap_or(&[]),
        repl: None,
    });
    program(tc, &srcs)
}

// ---------------------------------------------------------------------------
// 表(§18.2、§18.3)
// ---------------------------------------------------------------------------

/// 記号の名前の部分。記号の最後の `/` より後ろである。
fn sym_name(s: &str) -> &str {
    match s.rfind('/') {
        Some(i) => &s[i + 1..],
        None => s,
    }
}

/// 綴りを最後の `.` で 2 つに分ける。
fn split_last_dot(s: &str) -> R<(String, String)> {
    match s.rfind('.') {
        Some(i) => Ok((s[..i].to_string(), s[i + 1..].to_string())),
        None => bug(format!("KIR の書き出し: 修飾名ではありません: {s}")),
    }
}

/// パターンの型注釈を外す。
fn strip_annot(p: &Pat) -> &Pat {
    let mut p = p;
    while let PatKind::Annot(q, _) = &p.kind {
        p = q;
    }
    p
}

/// 大域の種類(関数は記号で指し、最後に添字に直す)。
enum PendingGlobal {
    Builtin,
    Extern { abi: Abi, name: String, dicts: u32 },
    Fn(String),
    Dictabs(String),
    Value(Rep),
}

/// 大域を足すときの種類。
enum GKind {
    Fn,
    Dictabs,
    Value(Rep),
    Extern(Abi, String, u32),
}

/// 初期化の手順(関数は記号で指す)。
enum PendingStep {
    DefValue(GlobalId, String),
    DefPattern(String, Vec<GlobalId>, PatId),
    DefFns(Vec<GlobalId>),
    DefDictabs(GlobalId),
    DefInstance(InstanceId),
    RunExp(String),
}

/// インスタンス(実装の関数は記号で指す)。
struct PendingInstance {
    sym: String,
    class: ClassId,
    head: TypeId,
    premises: u32,
    methods: Vec<(u32, u32, String)>,
}

/// 書き出しの表(Diktor の `tables` と、符号化の索引)。
#[derive(Default)]
struct Tables {
    /// 型の名前の oid から記号へ。
    type_syms: HashMap<Oid, String>,
    effect_syms: HashMap<Oid, String>,
    class_syms: HashMap<Oid, String>,
    /// 名前の oid から添字へ(記号の最初の項目の添字)。
    type_ids: HashMap<Oid, TypeId>,
    effect_ids: HashMap<Oid, EffectId>,
    class_ids: HashMap<Oid, ClassId>,
    /// (持ち主の添字, 短い名前) から持ち主の中の位置へ。
    ctor_idx: HashMap<(u32, String), u32>,
    op_idx: HashMap<(u32, String), u32>,
    meth_idx: HashMap<(u32, String), u32>,
    instances: HashMap<(Oid, Oid), InstanceId>,
    globals: HashMap<(NodeId, String), GlobalId>,
    /// 組み込みの値の名前から添字へ。
    builtin_globals: HashMap<String, GlobalId>,
}

/// 記号の列から、記号ごとの最初の位置を引く表を作る(Diktor の `index_of`)。
fn first_index<'s>(syms: impl Iterator<Item = &'s str>) -> HashMap<&'s str, u32> {
    let mut m = HashMap::default();
    for (i, s) in syms.enumerate() {
        m.entry(s).or_insert(i as u32);
    }
    m
}

// ---------------------------------------------------------------------------
// 枠(§18.5)
// ---------------------------------------------------------------------------

/// 書き出しの途中の関数(枠)である。
struct Frame {
    parent: Option<usize>,
    sym: String,
    kind: FnKind,
    params: Vec<Rep>,
    locals: Vec<Rep>,
    /// 仮の番号と、持ち主の枠での読み方。足した順。
    caps: Vec<(u32, Cap)>,
    cap_index: HashMap<Cap, u32>,
    /// resume が指す操作節の枠。
    owner: Option<usize>,
    ntail_resumes: u32,
    final_caps: Vec<Cap>,
    cap_reps: Vec<Rep>,
}

/// 書き終えた関数の中身(Diktor の `pending`)。
struct Pending {
    yields: bool,
    dicts: u32,
    result: Rep,
    pats: Vec<PatId>,
    guard: Option<ExpId>,
    body: ExpId,
}

/// 根の関数ごとの状態である。
#[derive(Default)]
struct Root {
    sym: String,
    sites: u32,
    frames: Vec<Frame>,
    fns: Vec<((u32, u32), usize, Pending)>,
    /// 捕獲を読む式と辞書(仮の番号を書いたもの)。
    env_exps: Vec<ExpId>,
    env_dicts: Vec<DictId>,
    /// 部分に枠の番号を仮に書いた式。
    part_exps: Vec<ExpId>,
}

#[derive(Clone, Copy)]
struct Binding {
    frame: usize,
    var: Cap,
}

/// 書き出しの環境(Diktor の `genv`)である。
#[derive(Clone)]
struct GEnv {
    fr: usize,
    names: PMap<Rc<str>, Binding>,
    dkeys: PMap<DKey, Binding>,
    /// 末尾位置か。
    tail: bool,
    /// 操作節の背骨の上か。
    spine: bool,
}

impl GEnv {
    /// 末尾位置になく、背骨の外にある子の環境。
    fn nt(&self) -> GEnv {
        GEnv {
            tail: false,
            spine: false,
            ..self.clone()
        }
    }

    fn bind_name(&self, x: &str, var: Cap) -> GEnv {
        GEnv {
            names: self.names.add(
                Rc::from(x),
                Binding {
                    frame: self.fr,
                    var,
                },
            ),
            ..self.clone()
        }
    }

    fn bind_dkeys(&self, ks: &[DKey], first: u32) -> GEnv {
        let mut m = self.dkeys.clone();
        for (i, k) in ks.iter().enumerate() {
            m = m.add(
                *k,
                Binding {
                    frame: self.fr,
                    var: Cap::DParam(first + i as u32),
                },
            );
        }
        GEnv {
            dkeys: m,
            ..self.clone()
        }
    }
}

/// 関数の本体の書き方である(Diktor が閉包で渡すもの)。
enum Body<'b> {
    /// 式をそのまま書く(`nt` なら末尾位置の外で)。
    Exp(&'b Exp, bool),
    /// ラムダの場所にする。
    Lambda(&'b [Pat], &'b Exp, NodeId),
    /// 仮引数のパターンを写して本体を書く(根の関数の lambda)。
    Params(&'b [Pat], &'b Exp),
    /// 辞書を受け取る関数の場所にして、その中で式を書く。
    DictabsExp(Vec<DKey>, Rep, &'b Exp),
    /// 束縛の値(Diktor の `binding_value`)。
    Binding(&'b LetBinding),
    /// let rec の群を書き、その中の名前 `x` を読む(インスタンスのメソッド)。
    RecGroup(&'b [LetBinding], String),
    /// 対話的な実行の表示:引数のスロット 0 の値に、証拠の辞書の `Show.show` を末尾で呼ぶ(種類 `fn` の根の関数)。
    ReplShowCall(Evidence),
    /// 対話的な実行の表示:[`Body::ReplShowCall`] を本体とするラムダを作る(種類 `dictabs` の根の関数)。
    ReplShowLambda(Evidence),
}

/// 根の関数を書く仕事である。
struct Job<'b> {
    sym: String,
    kind: FnKind,
    params: Vec<Rep>,
    dkeys: Vec<DKey>,
    tail: bool,
    yields: bool,
    result: Rep,
    body: Body<'b>,
    /// 書いた本体に辞書を適用する(インスタンスのメソッドの適合器)。
    adapter: Vec<HoleId>,
    /// 本体を `Print` のハンドラ式で包む([`ReplUnit::print`])。
    print: bool,
}

/// 書き手の状態である。
struct W<'t> {
    tc: &'t mut Tc,
    tb: Tables,
    nodes: Nodes,
    root: Root,
    /// 捕獲の仮の番号の数え上げ。
    provisional: u32,
    /// 仮の番号から確定した番号へ。
    final_cap: HashMap<u32, u32>,
    functions: Vec<(kir::Function, String)>,
    /// 関数の記号から、その記号の最初の関数の添字へ(Diktor の符号化の索引と同じく、最初の項目を指す)。
    fn_first: HashMap<String, FnId>,
    /// 対話的な実行で `Print` を処理するハンドラ式が使う、エフェクトと操作([`ReplUnit::print`])。
    repl_print: Option<ReplPrint>,
    /// 対話的な実行の入力の単位を書き出した結果。
    repl_out: ReplOut,
}

/// 対話的な実行で `Print` を処理するハンドラ式が使う、エフェクトと操作の位置である。
#[derive(Clone, Copy, Debug)]
struct ReplPrint {
    /// エフェクト `Print` と、その操作 `print`。
    print: (EffectId, u32),
    /// エフェクト `Console` と、その操作 `write`。
    write: (EffectId, u32),
}

/// 型検査を済ませた単位の列を KIR にする(Diktor の `program`)。
pub fn program(tc: &mut Tc, units: &[UnitSrc<'_>]) -> R<kir::Program> {
    program_repl(tc, units).map(|(p, _)| p)
}

/// [`program`] と同じで、対話的な実行の入力の単位([`UnitSrc::repl`])を書き出した結果も返す。
pub fn program_repl(tc: &mut Tc, units: &[UnitSrc<'_>]) -> R<(kir::Program, ReplOut)> {
    let mut w = W {
        tc,
        tb: Tables::default(),
        nodes: Nodes::default(),
        root: Root::default(),
        provisional: 0,
        final_cap: HashMap::default(),
        functions: Vec::new(),
        fn_first: HashMap::default(),
        repl_print: None,
        repl_out: ReplOut::default(),
    };
    let p = w.program(units)?;
    Ok((p, w.repl_out))
}

impl<'t> W<'t> {
    // ---- 記号 ----

    fn type_id(&self, c: Oid) -> R<TypeId> {
        match self.tb.type_ids.get(&c) {
            Some(i) => Ok(*i),
            None => bug(format!(
                "KIR の書き出し: 記号がありません: 型 {}",
                self.tc.name_of(c)
            )),
        }
    }

    fn effect_id(&self, e: Oid) -> R<EffectId> {
        match self.tb.effect_ids.get(&e) {
            Some(i) => Ok(*i),
            None => bug(format!(
                "KIR の書き出し: 記号がありません: エフェクト {}",
                self.tc.name_of(e)
            )),
        }
    }

    fn class_id(&self, c: Oid) -> R<ClassId> {
        match self.tb.class_ids.get(&c) {
            Some(i) => Ok(*i),
            None => bug(format!(
                "KIR の書き出し: 記号がありません: クラス {}",
                self.tc.name_of(c)
            )),
        }
    }

    fn inner(tbl: &HashMap<(u32, String), u32>, owner: u32, name: &str, what: &str) -> R<u32> {
        match tbl.get(&(owner, name.to_string())) {
            Some(i) => Ok(*i),
            None => bug(format!("KIR の書き出し: {what}が表にありません: {name}")),
        }
    }

    fn ctor_index(&self, ty: TypeId, name: &str) -> R<u32> {
        Self::inner(&self.tb.ctor_idx, ty.0, name, "コンストラクタ")
    }

    fn op_index(&self, e: EffectId, name: &str) -> R<u32> {
        Self::inner(&self.tb.op_idx, e.0, name, "操作")
    }

    fn method_index(&self, c: ClassId, name: &str) -> R<u32> {
        Self::inner(&self.tb.meth_idx, c.0, name, "メソッド")
    }

    fn builtin_class(&self, name: &str) -> R<ClassId> {
        match kir::builtins::CLASSES.iter().position(|(c, _)| *c == name) {
            Some(i) => Ok(ClassId(i as u32)),
            None => bug(format!(
                "KIR の書き出し: 組み込みのクラスがありません: {name}"
            )),
        }
    }

    // ---- Rep と yield の印(§18.4) ----

    fn rep_of_ty(&mut self, t: Ty) -> Rep {
        let t = self.tc.repr(t);
        match self.tc.node_ref(t) {
            TyNode::Con(c, args) if args.is_empty() => {
                let c = *c;
                match &*self.tc.name_of(c) {
                    "Boolean" => Rep::Bool,
                    "Int32" => Rep::I32,
                    "Int64" => Rep::I64,
                    "Float64" => Rep::F64,
                    "String" => Rep::Str,
                    _ => Rep::Any,
                }
            }
            _ => Rep::Any,
        }
    }

    fn ty_opt(&self, id: NodeId) -> Option<Ty> {
        self.tc.node_ty.get(&id).copied()
    }

    fn rep_of_node(&mut self, id: NodeId) -> Rep {
        match self.ty_opt(id) {
            Some(t) => self.rep_of_ty(t),
            None => Rep::Any,
        }
    }

    /// 束縛の名前のパターンの Rep。パターンに型が無ければ束縛のノードの型を使う。
    fn rep_of_name(&mut self, name: &Pat, b: &LetBinding) -> Rep {
        match self.ty_opt(name.meta.id) {
            Some(t) => self.rep_of_ty(t),
            None => self.rep_of_node(b.meta.id),
        }
    }

    fn noyield(&mut self, t: Ty) -> bool {
        let t = self.tc.repr(t);
        let TyNode::Arrow(_, _, eff) = *self.tc.node_ref(t) else {
            return false;
        };
        let mut r = eff;
        loop {
            let rr = self.tc.repr(r);
            match *self.tc.node_ref(rr) {
                TyNode::RowEmpty => return true,
                TyNode::RowExtend(l, _, rest) => {
                    let allowed = l == self.tc.eff_heap
                        || l == self.tc.eff_blocking
                        || self.tb.effect_syms.get(&l).map(|s| s.as_str())
                            == Some("prelude/Console");
                    if !allowed {
                        return false;
                    }
                    r = rest;
                }
                _ => return false,
            }
        }
    }

    fn yield_of_node(&mut self, id: NodeId) -> bool {
        match self.ty_opt(id) {
            Some(t) => !self.noyield(t),
            None => true,
        }
    }

    /// 辞書パラメータの鍵。インスタンスのメソッドでは実装の型の辞書パラメータを使う。
    fn dict_params_of(&self, id: NodeId) -> Vec<DKey> {
        match self.tc.dict.get(&id) {
            Some(DictInfo::Abs(ks)) | Some(DictInfo::Method(ks, _, _)) => ks.clone(),
            _ => Vec::new(),
        }
    }

    // ---- 枠 ----

    fn new_frame(
        &mut self,
        parent: Option<usize>,
        sym: String,
        kind: FnKind,
        params: Vec<Rep>,
        owner: Option<usize>,
    ) -> usize {
        self.root.frames.push(Frame {
            parent,
            sym,
            kind,
            params,
            locals: Vec::new(),
            caps: Vec::new(),
            cap_index: HashMap::default(),
            owner,
            ntail_resumes: 0,
            final_caps: Vec::new(),
            cap_reps: Vec::new(),
        });
        self.root.frames.len() - 1
    }

    fn new_slot(&mut self, fr: usize, rep: Rep) -> u32 {
        let f = &mut self.root.frames[fr];
        let i = f.params.len() + f.locals.len();
        f.locals.push(rep);
        i as u32
    }

    /// 枠 fr から束縛 b を読む方法。b の枠が fr の外にあれば、間の枠に捕獲を足す。
    fn access(&mut self, fr: usize, b: Binding) -> R<Cap> {
        if fr == b.frame {
            return Ok(b.var);
        }
        let Some(p) = self.root.frames[fr].parent else {
            return bug(format!(
                "KIR の書き出し: 根の関数の外の変数を読もうとしました: {}",
                self.root.frames[fr].sym
            ));
        };
        let pv = self.access(p, b)?;
        if let Some(id) = self.root.frames[fr].cap_index.get(&pv) {
            return Ok(Cap::Env(*id));
        }
        self.provisional += 1;
        let id = self.provisional;
        let f = &mut self.root.frames[fr];
        f.caps.push((id, pv));
        f.cap_index.insert(pv, id);
        Ok(Cap::Env(id))
    }

    fn next_site(&mut self) -> u32 {
        let n = self.root.sites;
        self.root.sites += 1;
        n
    }

    fn finish(&mut self, fr: usize, order: (u32, u32), pd: Pending) {
        self.root.fns.push((order, fr, pd));
    }

    // ---- ノード ----

    fn push(&mut self, e: kir::Exp) -> ExpId {
        self.nodes.push_exp(e)
    }

    /// 捕獲を読む式(仮の番号)。
    fn push_var(&mut self, c: Cap) -> R<ExpId> {
        Ok(match c {
            Cap::Local(i) => self.push(kir::Exp::Local(i)),
            Cap::Env(t) => {
                let id = self.push(kir::Exp::Env(t));
                self.root.env_exps.push(id);
                id
            }
            Cap::DParam(_) => return bug("KIR の書き出し: 名前が辞書を指しています"),
        })
    }

    fn push_part_exp(&mut self, e: kir::Exp) -> ExpId {
        let id = self.push(e);
        self.root.part_exps.push(id);
        id
    }

    fn part(fr: usize) -> Part {
        Part {
            func: FnId(fr as u32),
            caps: Vec::new(),
        }
    }

    // ---- リテラル(§18.6) ----

    fn number_head(&mut self, id: NodeId) -> String {
        match self.ty_opt(id) {
            Some(t) => {
                let t = self.tc.repr(t);
                match self.tc.node_ref(t) {
                    TyNode::Con(c, _) => {
                        let c = *c;
                        self.tc.name_of(c).to_string()
                    }
                    _ => String::new(),
                }
            }
            None => String::new(),
        }
    }

    fn number_lit(&mut self, id: NodeId, n: &Number) -> R<kir::Exp> {
        let h = self.number_head(id);
        let err = || kir::Exp::LitError(n.show());
        Ok(match h.as_str() {
            "Int32" => ocaml_int32_opt(&n.text).map_or_else(err, |v| kir::Exp::Lit(Lit::I32(v))),
            "Int64" => ocaml_int64_opt(&n.text).map_or_else(err, |v| kir::Exp::Lit(Lit::I64(v))),
            "Float64" => ocaml_float_of_string(&n.text)
                .map_or_else(err, |v| kir::Exp::Lit(Lit::F64(v.to_bits()))),
            _ => return bug(format!("KIR の書き出し: 数値リテラルの型が不正です: {h}")),
        })
    }

    fn pattern_number(&mut self, id: NodeId, n: &Number) -> R<Lit> {
        match self.number_lit(id, n)? {
            kir::Exp::Lit(l) => Ok(l),
            _ => bug(format!(
                "KIR の書き出し: パターンの数値が範囲外です: {}",
                n.show()
            )),
        }
    }

    // ---- パターン(§18.7) ----

    fn pat(&mut self, p: &Pat, bind: &mut dyn FnMut(&mut Self, &str, &Pat) -> R<u32>) -> R<PatId> {
        super::guard()?;
        let q = match &p.kind {
            PatKind::Wildcard => kir::Pat::Wild,
            PatKind::Var(x) => kir::Pat::Bind(bind(self, x, p)?),
            PatKind::Annot(q, _) => return self.pat(q, bind),
            PatKind::Bool(b) => kir::Pat::Lit(Lit::Bool(*b)),
            PatKind::Text(s) => kir::Pat::Lit(Lit::Str(s.as_bytes().to_vec())),
            PatKind::Number(n) => kir::Pat::Lit(self.pattern_number(p.meta.id, n)?),
            PatKind::Variant(l, q) => {
                let q = self.pat(q, bind)?;
                kir::Pat::Variant {
                    label: l.clone(),
                    pat: q,
                }
            }
            PatKind::Record(fs, tl) => {
                let mut fields = Vec::with_capacity(fs.len());
                for (l, q) in fs {
                    fields.push((l.clone(), self.pat(q, bind)?));
                }
                let tail = match tl {
                    None => RecordTail::Closed,
                    Some(q) if matches!(q.kind, PatKind::Wildcard) => RecordTail::Open,
                    Some(q) => RecordTail::Rest(self.pat(q, bind)?),
                };
                kir::Pat::Record { fields, tail }
            }
            PatKind::Ctor(_, args) => match self.tc.resolved.get(&p.meta.id) {
                Some(Resolved::CtorPat(data, ctor, f2a)) => {
                    let (data, ctor, f2a) = (*data, *ctor, f2a.clone());
                    let mut fields = Vec::new();
                    for (fi, a) in f2a.iter().enumerate() {
                        if let Some(ai) = a {
                            let Some(arg) = args.get(*ai) else {
                                return bug("KIR の書き出し: PCtor の引数の位置が範囲の外です");
                            };
                            fields.push((fi as u32, self.pat(&arg.pat, bind)?));
                        }
                    }
                    let ty = self.type_id(data)?;
                    let name = super::display(&self.tc.name_of(ctor)).to_string();
                    let ctor = self.ctor_index(ty, &name)?;
                    kir::Pat::Ctor { ty, ctor, fields }
                }
                _ => return bug("KIR の書き出し: PCtor が解決されていません"),
            },
        };
        Ok(self.nodes.push_pat(q))
    }

    /// 関数の中のパターン。変数ごとにスロットを振り、名前を環境に足す。
    fn local_pat(&mut self, env: &GEnv, p: &Pat) -> R<(PatId, GEnv)> {
        let fr = env.fr;
        let mut binds: Vec<(String, u32)> = Vec::new();
        let ip = self.pat(p, &mut |w, x, node| {
            let rep = w.rep_of_node(node.meta.id);
            let i = w.new_slot(fr, rep);
            binds.push((x.to_string(), i));
            Ok(i)
        })?;
        let mut env = env.clone();
        for (x, i) in binds {
            env = env.bind_name(&x, Cap::Local(i));
        }
        Ok((ip, env))
    }

    // ---- 辞書(§18.8) ----

    fn dict(&mut self, env: &GEnv, ev: &Evidence) -> R<DictId> {
        super::guard()?;
        let d = match ev {
            Evidence::Param(k) => {
                let Some(b) = env.dkeys.get(k).copied() else {
                    return bug("KIR の書き出し: 辞書パラメータが束縛されていません");
                };
                match self.access(env.fr, b)? {
                    Cap::DParam(i) => Dict::DParam(i),
                    Cap::Env(t) => {
                        let id = self.nodes.push_dict(Dict::Env(t));
                        self.root.env_dicts.push(id);
                        return Ok(id);
                    }
                    Cap::Local(_) => {
                        return bug("KIR の書き出し: 辞書がスロットを指しています");
                    }
                }
            }
            Evidence::Inst(c, n, ps) => match self.tb.instances.get(&(*c, *n)).copied() {
                Some(i) => {
                    let mut premises = Vec::with_capacity(ps.len());
                    for p in ps {
                        premises.push(self.dict(env, p)?);
                    }
                    Dict::Inst {
                        instance: i,
                        premises,
                    }
                }
                None => Dict::Builtin {
                    class: self.class_id(*c)?,
                    ty: self.type_id(*n)?,
                },
            },
            Evidence::Record(fs) | Evidence::Variant(fs) => {
                let mut v = Vec::with_capacity(fs.len());
                for (l, e) in fs {
                    let l = self.tc.name_of(*l).to_string();
                    v.push((l, self.dict(env, e)?));
                }
                if matches!(ev, Evidence::Record(_)) {
                    Dict::Record(v)
                } else {
                    Dict::Variant(v)
                }
            }
            Evidence::Hole(h) => match self.tc.hole(*h).h_sol.clone() {
                Some(e) => return self.dict(env, &e),
                None => return bug("KIR の書き出し: 解かれていない辞書の穴があります"),
            },
        };
        Ok(self.nodes.push_dict(d))
    }

    fn use_dicts(&mut self, env: &GEnv, id: NodeId) -> R<Vec<DictId>> {
        let hs = match self.tc.dict.get(&id) {
            Some(DictInfo::Use(hs)) => hs.clone(),
            _ => return Ok(Vec::new()),
        };
        let mut v = Vec::with_capacity(hs.len());
        for h in hs {
            v.push(self.dict(env, &Evidence::Hole(h))?);
        }
        Ok(v)
    }

    fn with_dicts(&mut self, env: &GEnv, id: NodeId, e: ExpId) -> R<ExpId> {
        let ds = self.use_dicts(env, id)?;
        Ok(if ds.is_empty() {
            e
        } else {
            self.push(kir::Exp::Dictapp { func: e, dicts: ds })
        })
    }

    fn op_dict(&mut self, env: &GEnv, id: NodeId) -> R<DictId> {
        match self.tc.dict.get(&id) {
            Some(DictInfo::Use(hs)) if hs.len() == 1 => {
                let h = hs[0];
                self.dict(env, &Evidence::Hole(h))
            }
            _ => bug("KIR の書き出し: 演算子に証拠がありません"),
        }
    }

    /// クラスのメソッドの参照。
    fn method_ref(&mut self, env: &GEnv, id: NodeId, q: &str) -> R<ExpId> {
        let (cls_name, m) = split_last_dot(q)?;
        let co = self.tc.intern(&cls_name);
        let Some(ci) = self.tc.find_class(co) else {
            return bug(format!("KIR の書き出し: クラスがありません: {cls_name}"));
        };
        let Some(scheme) = ci.ci_methods.iter().find(|(n, _)| *n == m).map(|(_, s)| *s) else {
            return bug(format!("KIR の書き出し: メソッドがありません: {q}"));
        };
        let cs = self.tc.constraints_of(scheme)?;
        let Some(idx) = cs
            .iter()
            .position(|(i, c)| i.vid == ci.ci_param.vid && *c == ci.ci_name)
        else {
            return bug(format!(
                "KIR の書き出し: メソッドの型にクラスの引数がありません: {q}"
            ));
        };
        let ds = self.use_dicts(env, id)?;
        if ds.len() != cs.len() {
            return bug(format!(
                "KIR の書き出し: メソッドの辞書の数が合いません: {q}"
            ));
        }
        let d = ds[idx];
        let own: Vec<DictId> = ds
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != idx)
            .map(|(_, d)| *d)
            .collect();
        let class = self.class_id(ci.ci_name)?;
        let method = self.method_index(class, &m)?;
        Ok(self.push(kir::Exp::Method {
            class,
            method,
            dict: d,
            own,
        }))
    }

    // ---- 式(§18.9) ----

    fn exps(&mut self, env: &GEnv, es: &[&Exp]) -> R<Vec<ExpId>> {
        let mut v = Vec::with_capacity(es.len());
        for e in es {
            v.push(self.exp(env, e)?);
        }
        Ok(v)
    }

    fn exp(&mut self, env: &GEnv, node: &Exp) -> R<ExpId> {
        super::guard()?;
        let id = node.meta.id;
        let e = match &node.kind {
            ExpKind::Bool(b) => kir::Exp::Lit(Lit::Bool(*b)),
            ExpKind::Text(s) => kir::Exp::Lit(Lit::Str(s.as_bytes().to_vec())),
            ExpKind::Number(n) => self.number_lit(id, n)?,
            ExpKind::Hole => kir::Exp::Hole,
            ExpKind::Ident(li) => {
                let name = li.show();
                if let Some(b) = env.names.get(name.as_str()).copied() {
                    let v = self.access(env.fr, b)?;
                    let v = self.push_var(v)?;
                    return self.with_dicts(env, id, v);
                }
                match self.tc.resolved.get(&id).cloned() {
                    Some(Resolved::Var(GRef::Decl(oid, x))) => {
                        let Some(g) = self.tb.globals.get(&(oid, x.clone())).copied() else {
                            return bug(format!("KIR の書き出し: 記号がありません: 大域 {x}"));
                        };
                        let v = self.push(kir::Exp::Global(g));
                        return self.with_dicts(env, id, v);
                    }
                    Some(Resolved::Var(GRef::Builtin(x))) => {
                        let Some(g) = self.tb.builtin_globals.get(&x).copied() else {
                            return bug(format!(
                                "KIR の書き出し: 大域が表にありません: builtin/{x}"
                            ));
                        };
                        let v = self.push(kir::Exp::Global(g));
                        return self.with_dicts(env, id, v);
                    }
                    Some(Resolved::Var(GRef::Method(q))) => return self.method_ref(env, id, &q),
                    Some(Resolved::Ctor(data, c, _)) => {
                        let ty = self.type_id(data)?;
                        let name = super::display(&self.tc.name_of(c)).to_string();
                        kir::Exp::Construct {
                            ty,
                            ctor: self.ctor_index(ty, &name)?,
                            fields: Vec::new(),
                        }
                    }
                    _ => {
                        return bug(format!(
                            "KIR の書き出し: Ident が解決されていません: {name}"
                        ));
                    }
                }
            }
            ExpKind::Apply(f, arg) => {
                let fe = self.exp(&env.nt(), f)?;
                let args = args_of(arg)?;
                let args = self.exps(&env.nt(), &args)?;
                kir::Exp::Call {
                    tail: env.tail,
                    yields: self.yield_of_node(f.meta.id),
                    func: fe,
                    args,
                }
            }
            ExpKind::Construct(_, args) => match self.tc.resolved.get(&id).cloned() {
                Some(Resolved::Ctor(data, c, a2f)) => {
                    let mut fields = Vec::with_capacity(args.len());
                    for (ai, a) in args.iter().enumerate() {
                        let Some(fi) = a2f.get(ai) else {
                            return bug("KIR の書き出し: Construct の引数の位置が範囲の外です");
                        };
                        fields.push((*fi as u32, self.exp(&env.nt(), &a.exp)?));
                    }
                    let ty = self.type_id(data)?;
                    let name = super::display(&self.tc.name_of(c)).to_string();
                    kir::Exp::Construct {
                        ty,
                        ctor: self.ctor_index(ty, &name)?,
                        fields,
                    }
                }
                _ => return bug("KIR の書き出し: Construct が解決されていません"),
            },
            ExpKind::Variant(l, v) => kir::Exp::Variant {
                label: l.clone(),
                value: self.exp(&env.nt(), v)?,
            },
            ExpKind::BinOp(l, op, r) => match op {
                BinOp::And | BinOp::Or => {
                    let le = self.exp(&env.nt(), l)?;
                    let re = self.exp(env, r)?;
                    if *op == BinOp::And {
                        kir::Exp::And(le, re)
                    } else {
                        kir::Exp::Or(le, re)
                    }
                }
                _ => {
                    let (cls, m) = super::elab::bin_op_sem(*op)
                        .ok_or_else(|| super::Fail::Bug("[BUG] 演算子".into()))?;
                    let d = self.op_dict(env, id)?;
                    let le = self.exp(&env.nt(), l)?;
                    let re = self.exp(&env.nt(), r)?;
                    let class = self.builtin_class(cls)?;
                    let method = self.method_index(class, m)?;
                    let call = kir::Exp::Opcall {
                        class,
                        method,
                        dict: d,
                        args: vec![le, re],
                    };
                    if *op == BinOp::Ne {
                        let c = self.push(call);
                        kir::Exp::Not(c)
                    } else {
                        call
                    }
                }
            },
            ExpKind::Not(v) => kir::Exp::Not(self.exp(&env.nt(), v)?),
            ExpKind::Neg(v) => {
                let d = self.op_dict(env, id)?;
                let a = self.exp(&env.nt(), v)?;
                let class = self.builtin_class("Neg")?;
                let method = self.method_index(class, "neg")?;
                kir::Exp::Opcall {
                    class,
                    method,
                    dict: d,
                    args: vec![a],
                }
            }
            ExpKind::Lambda(l) => return self.lambda_site(env, &l.params, &l.body, id),
            ExpKind::Let(b, rest) => return self.let_exp(env, b, rest),
            ExpKind::LetRec(bs, rest) => {
                let (env2, items) = self.rec_group(env, bs)?;
                let body = self.exp(&env2, rest)?;
                kir::Exp::Letrec { items, body }
            }
            ExpKind::Seq(es) => match es.as_slice() {
                [] => kir::Exp::RecEmpty,
                [x] => return self.exp(env, x),
                _ => {
                    let mut v = Vec::with_capacity(es.len());
                    for (i, x) in es.iter().enumerate() {
                        if i + 1 == es.len() {
                            v.push(self.exp(env, x)?);
                        } else {
                            v.push(self.exp(&env.nt(), x)?);
                        }
                    }
                    kir::Exp::Seq(v)
                }
            },
            ExpKind::Match(scrut, clauses) => {
                let s = self.exp(&env.nt(), scrut)?;
                let mut cls = Vec::with_capacity(clauses.len());
                for c in clauses {
                    let (p, env2) = self.local_pat(env, &c.pat)?;
                    let g = match &c.guard {
                        Some(g) => Some(self.exp(&env2.nt(), g)?),
                        None => None,
                    };
                    let body = self.exp(&env2, &c.body)?;
                    cls.push(kir::Clause {
                        pat: p,
                        guard: g,
                        body,
                    });
                }
                kir::Exp::Match {
                    scrutinee: s,
                    clauses: cls,
                }
            }
            ExpKind::RecordEmpty => kir::Exp::RecEmpty,
            ExpKind::RecordExtend(rest, l, v) => {
                let ve = self.exp(&env.nt(), v)?;
                let re = self.exp(&env.nt(), rest)?;
                kir::Exp::RecExtend {
                    label: l.clone(),
                    value: ve,
                    record: re,
                }
            }
            ExpKind::RecordUpdate(r, l, v) => {
                let re = self.exp(&env.nt(), r)?;
                let ve = self.exp(&env.nt(), v)?;
                kir::Exp::RecUpdate {
                    label: l.clone(),
                    record: re,
                    value: ve,
                }
            }
            ExpKind::RecordRestriction(r, l) => kir::Exp::RecRestrict {
                label: l.clone(),
                record: self.exp(&env.nt(), r)?,
            },
            ExpKind::RecordSelection(r, l) => kir::Exp::RecSelect {
                label: l.clone(),
                record: self.exp(&env.nt(), r)?,
            },
            ExpKind::Perform(_, arg) => match self.tc.resolved.get(&id).cloned() {
                Some(Resolved::Op(op)) => {
                    let (eff, o) = split_last_dot(&self.tc.name_of(op))?;
                    let eo = self.tc.intern(&eff);
                    let effect = self.effect_id(eo)?;
                    let op = self.op_index(effect, &o)?;
                    let args = args_of(arg)?;
                    let args = self.exps(&env.nt(), &args)?;
                    kir::Exp::Perform { effect, op, args }
                }
                _ => return bug("KIR の書き出し: Perform が解決されていません"),
            },
            ExpKind::Handle(body, clauses) => return self.handle_site(env, body, clauses),
            ExpKind::Resume(arg) => {
                let v = match arg {
                    Some(a) => self.exp(&env.nt(), a)?,
                    None => self.push(kir::Exp::RecEmpty),
                };
                if !env.spine {
                    match self.root.frames[env.fr].owner {
                        Some(o) => self.root.frames[o].ntail_resumes += 1,
                        None => return bug("KIR の書き出し: 操作節の外に resume があります"),
                    }
                }
                kir::Exp::Resume {
                    tail: env.spine,
                    value: v,
                }
            }
            ExpKind::Run(_, b) => return self.exp(env, b),
        };
        Ok(self.push(e))
    }

    fn body(&mut self, env: &GEnv, b: &Body<'_>) -> R<ExpId> {
        match b {
            Body::Exp(e, nt) => {
                if *nt {
                    self.exp(&env.nt(), e)
                } else {
                    self.exp(env, e)
                }
            }
            Body::Lambda(ps, body, fn_node) => self.lambda_site(env, ps, body, *fn_node),
            Body::Params(ps, body) => self.lambda_body(env, ps, body),
            Body::DictabsExp(ks, result, e) => {
                self.dictabs_site(env, ks, *result, &Body::Exp(e, false))
            }
            Body::Binding(b) => self.binding_value(env, b),
            Body::RecGroup(bs, x) => {
                let (env2, items) = self.rec_group(env, bs)?;
                let slot = match env2.names.get(x.as_str()) {
                    Some(Binding {
                        var: Cap::Local(n), ..
                    }) => *n,
                    _ => return bug("KIR の書き出し: let rec の名前"),
                };
                let body = self.push(kir::Exp::Local(slot));
                Ok(self.push(kir::Exp::Letrec { items, body }))
            }
            Body::ReplShowCall(ev) => {
                let d = self.dict(env, ev)?;
                let show = self.tc.intern("Show");
                let class = self.class_id(show)?;
                let method = self.method_index(class, "show")?;
                let m = self.push(kir::Exp::Method {
                    class,
                    method,
                    dict: d,
                    own: Vec::new(),
                });
                let x = self.push(kir::Exp::Local(0));
                Ok(self.push(kir::Exp::Call {
                    tail: env.tail,
                    yields: true,
                    func: m,
                    args: vec![x],
                }))
            }
            Body::ReplShowLambda(ev) => {
                let n = self.next_site();
                let sym = format!("{}${n}", self.root.sym);
                let fr = self.new_frame(Some(env.fr), sym, FnKind::Lambda, vec![Rep::Any], None);
                let env0 = GEnv {
                    fr,
                    tail: true,
                    spine: false,
                    ..env.clone()
                };
                let b = self.body(&env0, &Body::ReplShowCall(ev.clone()))?;
                let pd = Pending {
                    yields: true,
                    dicts: 0,
                    result: Rep::Str,
                    pats: Vec::new(),
                    guard: None,
                    body: b,
                };
                self.finish(fr, (n, 0), pd);
                Ok(self.push_part_exp(kir::Exp::Closure(Self::part(fr))))
            }
        }
    }

    /// 本体を、`Print` のハンドラ式で包む([`ReplUnit::print`])。
    ///
    /// プレリュードの `with_stdout` と同じく、`print(s)` の操作節は `resume(perform Console.write(s))` である
    /// (操作節の `resume` は末尾にあるので、分類は T である)。
    /// ハンドラ式は、本体を書く前に場所の番号を取り、本体、操作節の順に関数を作る。
    fn print_wrapped(&mut self, env: &GEnv, rp: ReplPrint, b: &Body<'_>, result: Rep) -> R<ExpId> {
        let n = self.next_site();
        let base = format!("{}$h{n}", self.root.sym);
        let owner = self.root.frames[env.fr].owner;
        let body_fr = self.new_frame(
            Some(env.fr),
            format!("{base}.body"),
            FnKind::Handlebody,
            Vec::new(),
            owner,
        );
        let env0 = GEnv {
            fr: body_fr,
            tail: false,
            spine: false,
            ..env.clone()
        };
        // メソッドの値の束縛は、`binding_value` と同じく末尾位置の外で書く。
        let inner = match b {
            Body::Binding(lb) if lb.params.is_none() => {
                self.body(&env0, &Body::Exp(&lb.body, false))?
            }
            b => self.body(&env0, b)?,
        };
        let pd = Pending {
            yields: true,
            dicts: 0,
            result,
            pats: Vec::new(),
            guard: None,
            body: inner,
        };
        self.finish(body_fr, (n, 0), pd);
        let op_fr = self.new_frame(
            Some(env.fr),
            format!("{base}.op.print"),
            FnKind::Opclause,
            vec![Rep::Any],
            None,
        );
        self.root.frames[op_fr].owner = Some(op_fr);
        let msg = self.push(kir::Exp::Local(0));
        let write = self.push(kir::Exp::Perform {
            effect: rp.write.0,
            op: rp.write.1,
            args: vec![msg],
        });
        let resume = self.push(kir::Exp::Resume {
            tail: true,
            value: write,
        });
        let pat = self.nodes.push_pat(kir::Pat::Wild);
        let pd = Pending {
            yields: true,
            dicts: 0,
            result,
            pats: vec![pat],
            guard: None,
            body: resume,
        };
        self.finish(op_fr, (n, 1), pd);
        Ok(self.push_part_exp(kir::Exp::Handle(Box::new(kir::Handle {
            tail: env.tail,
            effect: rp.print.0,
            body: Self::part(body_fr),
            ops: vec![HandlerOp {
                op: rp.print.1,
                class: ClauseClass::T,
                part: Self::part(op_fr),
            }],
            ret: None,
            cancel: None,
        }))))
    }

    /// 束縛の値(Diktor の `binding_value`)。
    fn binding_value(&mut self, env: &GEnv, b: &LetBinding) -> R<ExpId> {
        let plain = match &b.params {
            Some(ps) => Body::Lambda(ps, &b.body, b.meta.id),
            None => Body::Exp(&b.body, true),
        };
        let ks = self.dict_params_of(b.meta.id);
        if ks.is_empty() {
            self.body(env, &plain)
        } else {
            let result = match b.params {
                Some(_) => Rep::Any,
                None => self.rep_of_node(b.body.meta.id),
            };
            self.dictabs_site(env, &ks, result, &plain)
        }
    }

    fn is_closure_binding(&self, b: &LetBinding) -> bool {
        b.params.is_some() || !self.dict_params_of(b.meta.id).is_empty()
    }

    fn let_exp(&mut self, env: &GEnv, b: &LetBinding, rest: &Exp) -> R<ExpId> {
        let name = strip_annot(&b.name);
        let e = match &name.kind {
            PatKind::Var(x) => {
                let rep = if self.is_closure_binding(b) {
                    Rep::Any
                } else {
                    self.rep_of_name(name, b)
                };
                let n = self.new_slot(env.fr, rep);
                let v = self.binding_value(env, b)?;
                let body = self.exp(&env.bind_name(x, Cap::Local(n)), rest)?;
                kir::Exp::Let {
                    slot: n,
                    value: v,
                    body,
                }
            }
            _ => {
                let (p, env2) = self.local_pat(env, &b.name)?;
                let v = self.binding_value(env, b)?;
                let body = self.exp(&env2, rest)?;
                kir::Exp::Letpat {
                    pat: p,
                    value: v,
                    body,
                }
            }
        };
        Ok(self.push(e))
    }

    /// let rec の群。群の名前にスロットを先に振ってから、各項目を書く。
    fn rec_group(&mut self, env: &GEnv, bs: &[LetBinding]) -> R<(GEnv, Vec<(u32, ExpId)>)> {
        let mut names = Vec::with_capacity(bs.len());
        for b in bs {
            match &strip_annot(&b.name).kind {
                PatKind::Var(x) => names.push(x.clone()),
                _ => return bug("KIR の書き出し: let rec の名前が変数ではありません"),
            }
        }
        let mut slots = Vec::with_capacity(bs.len());
        for _ in &names {
            slots.push(self.new_slot(env.fr, Rep::Any));
        }
        let mut env2 = env.clone();
        for (x, n) in names.iter().zip(&slots) {
            env2 = env2.bind_name(x, Cap::Local(*n));
        }
        let mut items = Vec::with_capacity(bs.len());
        for (n, b) in slots.iter().zip(bs) {
            let site = match (&b.params, &b.body.kind) {
                (Some(ps), _) => Body::Lambda(ps, &b.body, b.meta.id),
                (None, ExpKind::Lambda(l)) => Body::Lambda(&l.params, &l.body, b.body.meta.id),
                _ => return bug("KIR の書き出し: let rec の右辺が関数ではありません"),
            };
            let ks = self.dict_params_of(b.meta.id);
            let e = if ks.is_empty() {
                self.body(&env2, &site)?
            } else {
                self.dictabs_site(&env2, &ks, Rep::Any, &site)?
            };
            items.push((*n, e));
        }
        Ok((env2, items))
    }

    // ---- 持ち上げた関数の場所(§18.10) ----

    fn lambda_site(&mut self, env: &GEnv, params: &[Pat], body: &Exp, fn_node: NodeId) -> R<ExpId> {
        let n = self.next_site();
        let sym = format!("{}${n}", self.root.sym);
        let reps: Vec<Rep> = params.iter().map(|p| self.rep_of_node(p.meta.id)).collect();
        let fr = self.new_frame(Some(env.fr), sym, FnKind::Lambda, reps, None);
        let env0 = GEnv {
            fr,
            tail: true,
            spine: false,
            ..env.clone()
        };
        let b = self.lambda_body(&env0, params, body)?;
        let pd = Pending {
            yields: self.yield_of_node(fn_node),
            dicts: 0,
            result: self.rep_of_node(body.meta.id),
            pats: Vec::new(),
            guard: None,
            body: b,
        };
        self.finish(fr, (n, 0), pd);
        Ok(self.push_part_exp(kir::Exp::Closure(Self::part(fr))))
    }

    /// 仮引数のパターンを写し、本体を書く。
    fn lambda_body(&mut self, env: &GEnv, params: &[Pat], body: &Exp) -> R<ExpId> {
        let mut env = env.clone();
        let mut wraps: Vec<(PatId, u32)> = Vec::new();
        for (j, p) in params.iter().enumerate() {
            let q = strip_annot(p);
            match &q.kind {
                PatKind::Var(x) => env = env.bind_name(x, Cap::Local(j as u32)),
                PatKind::Wildcard => {}
                _ => {
                    let (ip, env2) = self.local_pat(&env, p)?;
                    env = env2;
                    wraps.push((ip, j as u32));
                }
            }
        }
        let mut b = self.exp(&env, body)?;
        for (ip, j) in wraps.into_iter().rev() {
            let v = self.push(kir::Exp::Local(j));
            b = self.push(kir::Exp::Letpat {
                pat: ip,
                value: v,
                body: b,
            });
        }
        Ok(b)
    }

    /// 辞書を受け取る関数の場所。
    fn dictabs_site(&mut self, env: &GEnv, ks: &[DKey], result: Rep, body: &Body<'_>) -> R<ExpId> {
        let n = self.next_site();
        let sym = format!("{}${n}", self.root.sym);
        let fr = self.new_frame(Some(env.fr), sym, FnKind::Dictabs, Vec::new(), None);
        let env0 = GEnv {
            fr,
            tail: false,
            spine: false,
            ..env.clone()
        }
        .bind_dkeys(ks, 0);
        let b = self.body(&env0, body)?;
        self.finish(
            fr,
            (n, 0),
            Pending {
                yields: true,
                dicts: ks.len() as u32,
                result,
                pats: Vec::new(),
                guard: None,
                body: b,
            },
        );
        Ok(self.push_part_exp(kir::Exp::Dictabs(Self::part(fr))))
    }

    /// ハンドラ式の場所。本体、操作節(書いた順)、return 節、cancel 節の順に関数を作る。
    fn handle_site(&mut self, env: &GEnv, body: &Exp, clauses: &[Clause]) -> R<ExpId> {
        let n = self.next_site();
        let base = format!("{}$h{n}", self.root.sym);
        let mut order = 0u32;
        let mut ops: Vec<(&Clause, Oid, bool)> = Vec::new();
        for c in clauses {
            match self.tc.resolved.get(&c.meta.id) {
                Some(Resolved::Op(op)) => ops.push((c, *op, false)),
                Some(Resolved::NeverOp(op)) => ops.push((c, *op, true)),
                _ => {}
            }
        }
        let Some((_, op0, _)) = ops.first() else {
            return bug("KIR の書き出し: 操作節の無い handle です");
        };
        let (eff_name, _) = split_last_dot(&self.tc.name_of(*op0))?;
        let eo = self.tc.intern(&eff_name);
        let effect = self.effect_id(eo)?;
        let arg_pats = |c: &Clause| -> Vec<Pat> {
            match &c.pat.kind {
                PatKind::Ctor(_, aps) => aps.iter().map(|a| a.pat.clone()).collect(),
                _ => Vec::new(),
            }
        };
        // 本体
        let body_fr = {
            let owner = self.root.frames[env.fr].owner;
            let fr = self.new_frame(
                Some(env.fr),
                format!("{base}.body"),
                FnKind::Handlebody,
                Vec::new(),
                owner,
            );
            let b = self.exp(
                &GEnv {
                    fr,
                    tail: false,
                    spine: false,
                    ..env.clone()
                },
                body,
            )?;
            let pd = Pending {
                yields: true,
                dicts: 0,
                result: self.rep_of_node(body.meta.id),
                pats: Vec::new(),
                guard: None,
                body: b,
            };
            self.finish(fr, (n, order), pd);
            order += 1;
            fr
        };
        // 操作節
        let mut seen: HashMap<String, u32> = HashMap::default();
        let mut op_parts = Vec::with_capacity(ops.len());
        for (c, op, never) in ops {
            let (_, o) = split_last_dot(&self.tc.name_of(op))?;
            let k = *seen.get(&o).unwrap_or(&0);
            seen.insert(o.clone(), k + 1);
            let sym = if k == 0 {
                format!("{base}.op.{o}")
            } else {
                format!("{base}.op.{o}#{k}")
            };
            let ps = arg_pats(c);
            let reps: Vec<Rep> = ps.iter().map(|p| self.rep_of_node(p.meta.id)).collect();
            let fr = self.new_frame(Some(env.fr), sym, FnKind::Opclause, reps, None);
            if !never {
                self.root.frames[fr].owner = Some(fr);
            }
            let mut env1 = GEnv {
                fr,
                tail: false,
                spine: false,
                ..env.clone()
            };
            let mut pats = Vec::with_capacity(ps.len());
            for (j, p) in ps.iter().enumerate() {
                let q = strip_annot(p);
                match &q.kind {
                    PatKind::Var(x) => {
                        env1 = env1.bind_name(x, Cap::Local(j as u32));
                        pats.push(self.nodes.push_pat(kir::Pat::Wild));
                    }
                    PatKind::Wildcard => pats.push(self.nodes.push_pat(kir::Pat::Wild)),
                    _ => {
                        let (ip, env2) = self.local_pat(&env1, p)?;
                        env1 = env2;
                        pats.push(ip);
                    }
                }
            }
            let guard = match &c.guard {
                Some(g) => Some(self.exp(&env1.nt(), g)?),
                None => None,
            };
            let b = self.exp(
                &GEnv {
                    tail: never && env.tail,
                    spine: !never,
                    ..env1.clone()
                },
                &c.body,
            )?;
            let class = if never {
                ClauseClass::N
            } else if self.root.frames[fr].ntail_resumes == 0 {
                ClauseClass::T
            } else {
                ClauseClass::G
            };
            let pd = Pending {
                yields: true,
                dicts: 0,
                result: self.rep_of_node(c.body.meta.id),
                pats,
                guard,
                body: b,
            };
            self.finish(fr, (n, order), pd);
            order += 1;
            op_parts.push(HandlerOp {
                op: self.op_index(effect, &o)?,
                class,
                part: Self::part(fr),
            });
        }
        let find = |w: &Self, ret: bool| -> Option<&Clause> {
            clauses.iter().find(|c| {
                matches!(
                    (w.tc.resolved.get(&c.meta.id), ret),
                    (Some(Resolved::ReturnClause), true) | (Some(Resolved::CancelClause), false)
                )
            })
        };
        let ret_part = match find(self, true) {
            None => None,
            Some(c) => {
                let ps = arg_pats(c);
                let reps: Vec<Rep> = ps.iter().map(|p| self.rep_of_node(p.meta.id)).collect();
                let fr = self.new_frame(
                    Some(env.fr),
                    format!("{base}.return"),
                    FnKind::Retclause,
                    reps,
                    None,
                );
                let b = self.lambda_body(
                    &GEnv {
                        fr,
                        tail: env.tail,
                        spine: false,
                        ..env.clone()
                    },
                    &ps,
                    &c.body,
                )?;
                let pd = Pending {
                    yields: true,
                    dicts: 0,
                    result: self.rep_of_node(c.body.meta.id),
                    pats: Vec::new(),
                    guard: None,
                    body: b,
                };
                self.finish(fr, (n, order), pd);
                order += 1;
                Some(Self::part(fr))
            }
        };
        let cancel_part = match find(self, false) {
            None => None,
            Some(c) => {
                let fr = self.new_frame(
                    Some(env.fr),
                    format!("{base}.cancel"),
                    FnKind::Cancelclause,
                    Vec::new(),
                    None,
                );
                let b = self.exp(
                    &GEnv {
                        fr,
                        tail: false,
                        spine: false,
                        ..env.clone()
                    },
                    &c.body,
                )?;
                let pd = Pending {
                    yields: true,
                    dicts: 0,
                    result: self.rep_of_node(c.body.meta.id),
                    pats: Vec::new(),
                    guard: None,
                    body: b,
                };
                self.finish(fr, (n, order), pd);
                Some(Self::part(fr))
            }
        };
        Ok(self.push_part_exp(kir::Exp::Handle(Box::new(kir::Handle {
            tail: env.tail,
            effect,
            body: Self::part(body_fr),
            ops: op_parts,
            ret: ret_part,
            cancel: cancel_part,
        }))))
    }

    // ---- 捕獲の番号の確定と根の関数(§18.11) ----

    fn var_key(v: &Cap) -> (u8, u32) {
        match v {
            Cap::DParam(i) => (0, *i),
            Cap::Env(i) => (1, *i),
            Cap::Local(i) => (2, *i),
        }
    }

    fn fix_caps(&mut self, fr: usize) -> R<()> {
        let Some(p) = self.root.frames[fr].parent else {
            if !self.root.frames[fr].caps.is_empty() {
                return bug("KIR の書き出し: 根の関数が捕獲を持っています");
            }
            return Ok(());
        };
        let mut resolved = Vec::with_capacity(self.root.frames[fr].caps.len());
        for (id, v) in &self.root.frames[fr].caps {
            let v = match v {
                Cap::Env(t) => match self.final_cap.get(t) {
                    Some(i) => Cap::Env(*i),
                    None => return bug("KIR の書き出し: 捕獲の仮の番号がありません"),
                },
                v => *v,
            };
            resolved.push((*id, v));
        }
        resolved.sort_by_key(|(_, v)| Self::var_key(v));
        let mut reps = Vec::with_capacity(resolved.len());
        for (i, (id, v)) in resolved.iter().enumerate() {
            self.final_cap.insert(*id, i as u32);
            let pf = &self.root.frames[p];
            reps.push(match v {
                Cap::Local(i) => {
                    let i = *i as usize;
                    if i < pf.params.len() {
                        pf.params[i]
                    } else {
                        match pf.locals.get(i - pf.params.len()) {
                            Some(r) => *r,
                            None => return bug("KIR の書き出し: スロットの番号が範囲の外です"),
                        }
                    }
                }
                Cap::DParam(_) => Rep::Any,
                Cap::Env(j) => match pf.cap_reps.get(*j as usize) {
                    Some(r) => *r,
                    None => return bug("KIR の書き出し: 捕獲の番号が範囲の外です"),
                },
            });
        }
        let f = &mut self.root.frames[fr];
        f.final_caps = resolved.into_iter().map(|(_, v)| v).collect();
        f.cap_reps = reps;
        Ok(())
    }

    /// 根の関数を 1 つ書き出し、持ち上げた関数とともに関数の表に足す。
    fn root_fn(&mut self, job: Job<'_>) -> R<()> {
        self.root = Root {
            sym: job.sym.clone(),
            ..Root::default()
        };
        let fr = self.new_frame(None, job.sym.clone(), job.kind, job.params, None);
        let env = GEnv {
            fr,
            names: PMap::new(),
            dkeys: PMap::new(),
            tail: job.tail,
            spine: false,
        }
        .bind_dkeys(&job.dkeys, 0);
        let mut body = match (job.print, self.repl_print) {
            (true, Some(rp)) => self.print_wrapped(&env, rp, &job.body, job.result)?,
            _ => self.body(&env, &job.body)?,
        };
        if !job.adapter.is_empty() {
            let mut ds = Vec::with_capacity(job.adapter.len());
            for h in &job.adapter {
                ds.push(self.dict(&env, &Evidence::Hole(*h))?);
            }
            body = self.push(kir::Exp::Dictapp {
                func: body,
                dicts: ds,
            });
        }
        // 捕獲の並びを、作った順(持ち主が先)に決める。
        for i in 0..self.root.frames.len() {
            self.fix_caps(i)?;
        }
        let mut root = std::mem::take(&mut self.root);
        for e in &root.env_exps {
            if let kir::Exp::Env(t) = &mut self.nodes.exps[e.index()] {
                *t = self.final_cap[t];
            }
        }
        for d in &root.env_dicts {
            if let Dict::Env(t) = &mut self.nodes.dicts[d.index()] {
                *t = self.final_cap[t];
            }
        }
        // 根の関数に続けて、持ち上げた関数を場所の番号の順に並べる。
        let mut lifted = std::mem::take(&mut root.fns);
        lifted.sort_by_key(|(o, _, _)| *o);
        let mk = |f: &Frame, pd: Pending| kir::Function {
            sym: String::new(),
            kind: f.kind,
            yields: pd.yields,
            params: f.params.clone(),
            locals: f.locals.clone(),
            dicts: pd.dicts,
            caps: f.cap_reps.clone(),
            result: pd.result,
            pats: pd.pats,
            guard: pd.guard,
            body: pd.body,
        };
        let rootf = mk(
            &root.frames[fr],
            Pending {
                yields: job.yields,
                dicts: job.dkeys.len() as u32,
                result: job.result,
                pats: Vec::new(),
                guard: None,
                body,
            },
        );
        let push_fn = |w: &mut Self, f: kir::Function, sym: String| {
            let id = FnId(w.functions.len() as u32);
            w.fn_first.entry(sym.clone()).or_insert(id);
            w.functions.push((f, sym));
        };
        push_fn(self, rootf, job.sym);
        for (_, f, pd) in lifted {
            let sym = root.frames[f].sym.clone();
            push_fn(self, mk(&root.frames[f], pd), sym);
        }
        // 部分の関数は記号で引く(同じ記号の関数が前にあれば、Diktor の符号化と同じくそれを指す)。
        let fn_first = &self.fn_first;
        let fix = |p: &mut Part| -> R<()> {
            let f = &root.frames[p.func.index()];
            match fn_first.get(&f.sym) {
                Some(id) => p.func = *id,
                None => {
                    return bug(format!("KIR の書き出し: 場所の関数がありません: {}", f.sym));
                }
            }
            p.caps = f.final_caps.clone();
            Ok(())
        };
        for e in &root.part_exps {
            match &mut self.nodes.exps[e.index()] {
                kir::Exp::Closure(p) | kir::Exp::Dictabs(p) => fix(p)?,
                kir::Exp::Handle(h) => {
                    fix(&mut h.body)?;
                    for o in &mut h.ops {
                        fix(&mut o.part)?;
                    }
                    if let Some(p) = &mut h.ret {
                        fix(p)?;
                    }
                    if let Some(p) = &mut h.cancel {
                        fix(p)?;
                    }
                }
                _ => return bug("KIR の書き出し: 場所の式ではありません"),
            }
        }
        Ok(())
    }

    // ---- 宣言(§18.12) ----

    /// インスタンスの頭(宣言の出身の module と単位で、クラスと型構成子の名前を解く)。
    fn instance_head(&mut self, d: &Decl, i: &crate::ast::InstanceDecl) -> R<(Oid, Oid)> {
        self.tc.with_decl_scope(d.meta.id, |tc| {
            let co = tc.intern(&i.class);
            let cls = tc.resolve_class(co);
            let comps = match i.args.as_slice() {
                [t] => match &t.kind {
                    TypeExpKind::Ident(li) => li.0.clone(),
                    TypeExpKind::Apply(h, _) => match &h.kind {
                        TypeExpKind::Ident(li) => li.0.clone(),
                        _ => return bug("KIR の書き出し: インスタンスの頭を解決できません"),
                    },
                    _ => return bug("KIR の書き出し: インスタンスの頭を解決できません"),
                },
                _ => return bug("KIR の書き出し: インスタンスの頭を解決できません"),
            };
            let o = tc.intern(&comps.join("."));
            Ok((cls, tc.resolve_con(o)))
        })
    }

    fn program(&mut self, units: &[UnitSrc<'_>]) -> R<kir::Program> {
        use kir::builtins;
        let mut types: Vec<kir::Type> = Vec::new();
        let mut effects: Vec<kir::Effect> = Vec::new();
        let mut classes: Vec<kir::Class> = Vec::new();
        for t in builtins::TYPES {
            let o = self.tc.intern(t);
            let s = format!("builtin/{t}");
            self.tb.type_syms.insert(o, s.clone());
            types.push(kir::Type {
                sym: s,
                ctors: Vec::new(),
            });
        }
        for (c, ms) in builtins::CLASSES {
            let o = self.tc.intern(c);
            let s = format!("builtin/{c}");
            self.tb.class_syms.insert(o, s.clone());
            classes.push(kir::Class {
                sym: s,
                methods: ms
                    .iter()
                    .map(|m| kir::ClassMethod {
                        name: m.to_string(),
                        dicts: 0,
                    })
                    .collect(),
            });
        }
        // 1 段目の前半:型、エフェクト、クラス。
        for u in units {
            self.collect_types(u, &mut types, &mut effects, &mut classes)?;
        }
        {
            let ti = first_index(types.iter().map(|t| t.sym.as_str()));
            for (o, s) in &self.tb.type_syms {
                self.tb.type_ids.insert(*o, TypeId(ti[s.as_str()]));
            }
            for t in &types {
                let id = ti[t.sym.as_str()];
                for (j, c) in t.ctors.iter().enumerate() {
                    self.tb
                        .ctor_idx
                        .entry((id, c.name.clone()))
                        .or_insert(j as u32);
                }
            }
            let ei = first_index(effects.iter().map(|t| t.sym.as_str()));
            for (o, s) in &self.tb.effect_syms {
                self.tb.effect_ids.insert(*o, EffectId(ei[s.as_str()]));
            }
            for t in &effects {
                let id = ei[t.sym.as_str()];
                for (j, c) in t.ops.iter().enumerate() {
                    self.tb
                        .op_idx
                        .entry((id, c.name.clone()))
                        .or_insert(j as u32);
                }
            }
            let ci = first_index(classes.iter().map(|t| t.sym.as_str()));
            for (o, s) in &self.tb.class_syms {
                self.tb.class_ids.insert(*o, ClassId(ci[s.as_str()]));
            }
            for t in &classes {
                let id = ci[t.sym.as_str()];
                for (j, c) in t.methods.iter().enumerate() {
                    self.tb
                        .meth_idx
                        .entry((id, c.name.clone()))
                        .or_insert(j as u32);
                }
            }
        }
        // 1 段目の後半:大域、インスタンス、手順。
        let mut globals: Vec<(String, PendingGlobal)> = Vec::new();
        for v in builtins::VALUES {
            self.tb
                .builtin_globals
                .insert(v.to_string(), GlobalId(globals.len() as u32));
            globals.push((format!("builtin/{v}"), PendingGlobal::Builtin));
        }
        let mut instances: Vec<PendingInstance> = Vec::new();
        let mut unit_steps: Vec<Vec<PendingStep>> = Vec::new();
        let mut jobs: Vec<Job<'_>> = Vec::new();
        for u in units {
            let steps = self.collect_unit(u, &mut globals, &mut instances, &mut jobs)?;
            unit_steps.push(steps);
        }
        if units.iter().any(|u| u.repl.is_some_and(|r| r.print)) {
            // 引数 1 個の操作 `print` を持つ `Print` と、実行時が処理する `Console.write` があるときだけ包む。
            let find = |sym: &str, op: &str| -> Option<(EffectId, u32)> {
                let e = effects.iter().position(|e| e.sym == sym)?;
                let o = effects[e].ops.iter().position(|o| {
                    o.name == op && o.arity == 1 && !o.never && (op != "write" || o.runtime)
                })?;
                Some((EffectId(e as u32), o as u32))
            };
            self.repl_print = match (
                find("prelude/Print", "print"),
                find("prelude/Console", "write"),
            ) {
                (Some(print), Some(write)) => Some(ReplPrint { print, write }),
                _ => None,
            };
        }
        // 2 段目:根の関数。
        for job in jobs {
            self.root_fn(job)?;
        }
        // 関数を記号で引く。
        let functions = std::mem::take(&mut self.functions);
        let fi = std::mem::take(&mut self.fn_first);
        let fn_id = |s: &str| -> R<FnId> {
            match fi.get(s) {
                Some(i) => Ok(*i),
                None => bug(format!("KIR の書き出し: 関数が表にありません: {s}")),
            }
        };
        let mut gs = Vec::with_capacity(globals.len());
        for (sym, k) in &globals {
            let kind = match k {
                PendingGlobal::Builtin => GlobalKind::Builtin,
                PendingGlobal::Extern { abi, name, dicts } => GlobalKind::Extern {
                    abi: *abi,
                    name: name.clone(),
                    dicts: *dicts,
                },
                PendingGlobal::Fn(s) => GlobalKind::Fn(fn_id(s)?),
                PendingGlobal::Dictabs(s) => GlobalKind::Dictabs(fn_id(s)?),
                PendingGlobal::Value(r) => GlobalKind::Value(*r),
            };
            gs.push(kir::Global {
                sym: sym.clone(),
                kind,
            });
        }
        let mut is = Vec::with_capacity(instances.len());
        for i in &instances {
            let mut methods = Vec::with_capacity(i.methods.len());
            for (x, k, f) in &i.methods {
                methods.push(kir::Impl {
                    method: *x,
                    dicts: *k,
                    func: fn_id(f)?,
                });
            }
            is.push(kir::Instance {
                sym: i.sym.clone(),
                class: i.class,
                head: i.head,
                premises: i.premises,
                methods,
            });
        }
        let mut us = vec![kir::Unit {
            key: "builtin".to_string(),
            paths: Vec::new(),
            init: Vec::new(),
        }];
        for (u, steps) in units.iter().zip(unit_steps) {
            let mut init = Vec::with_capacity(steps.len());
            for s in steps {
                init.push(match s {
                    PendingStep::DefValue(g, f) => Step::DefValue {
                        global: g,
                        func: fn_id(&f)?,
                    },
                    PendingStep::DefPattern(f, globals, pat) => Step::DefPattern {
                        func: fn_id(&f)?,
                        globals,
                        pat,
                    },
                    PendingStep::DefFns(g) => Step::DefFns(g),
                    PendingStep::DefDictabs(g) => Step::DefDictabs(g),
                    PendingStep::DefInstance(i) => Step::DefInstance(i),
                    PendingStep::RunExp(f) => Step::RunExp(fn_id(&f)?),
                });
            }
            us.push(kir::Unit {
                key: u.key.clone(),
                paths: u.paths.clone(),
                init,
            });
        }
        let functions = functions
            .into_iter()
            .map(|(mut f, s)| {
                f.sym = s;
                f
            })
            .collect();
        Ok(kir::Program {
            version: kir::VERSION,
            units: us,
            types,
            effects,
            classes,
            instances: is,
            globals: gs,
            functions,
            nodes: std::mem::take(&mut self.nodes),
        })
    }

    /// 単位の型、エフェクト、クラスの表を作る(1 段目の前半)。
    fn collect_types(
        &mut self,
        u: &UnitSrc<'_>,
        types: &mut Vec<kir::Type>,
        effects: &mut Vec<kir::Effect>,
        classes: &mut Vec<kir::Class>,
    ) -> R<()> {
        let sym = |name: &str| format!("{}/{}", u.key, super::display(name));
        for d in u.decls {
            match &d.kind {
                DeclKind::Newtype(n) => {
                    let s = sym(&n.name);
                    let o = self.tc.intern(&n.name);
                    self.tb.type_syms.insert(o, s.clone());
                    types.push(kir::Type {
                        sym: s,
                        ctors: n
                            .ctors
                            .iter()
                            .map(|c| kir::Ctor {
                                name: super::display(&c.name).to_string(),
                                arity: c.fields.len() as u32,
                            })
                            .collect(),
                    });
                }
                DeclKind::Effect(e) => {
                    let s = sym(&e.name);
                    let eoid = self.tc.intern(&e.name);
                    self.tb.effect_syms.insert(eoid, s.clone());
                    let Some(info) = self.tc.find_effect(eoid) else {
                        return bug(format!(
                            "KIR の書き出し: エフェクトがありません: {}",
                            e.name
                        ));
                    };
                    let never_o = self.tc.intern("Never");
                    let mut ops = Vec::with_capacity(e.ops.len());
                    for (o, _) in &e.ops {
                        let oo = self.tc.intern(o);
                        let Some(scheme) =
                            info.ef_ops.iter().find(|(x, _)| *x == oo).map(|(_, t)| *t)
                        else {
                            return bug(format!("KIR の書き出し: 操作がありません: {o}"));
                        };
                        let st = self.tc.repr(scheme);
                        let TyNode::Arrow(a, r, _) = *self.tc.node_ref(st) else {
                            return bug("KIR の書き出し: 操作の型が関数の型ではありません");
                        };
                        let ar = self.tc.repr(a);
                        let arity = match *self.tc.node_ref(ar) {
                            TyNode::Record(row) => self.tc.row_fields(row).0.len(),
                            _ => 0,
                        } as u32;
                        let rr = self.tc.repr(r);
                        let never = matches!(self.tc.node_ref(rr), TyNode::Con(c, args) if args.is_empty() && *c == never_o);
                        ops.push(kir::Op {
                            name: o.clone(),
                            arity,
                            never,
                            runtime: s == "prelude/Console" && o == "write" && arity == 1,
                        });
                    }
                    effects.push(kir::Effect { sym: s, ops });
                }
                DeclKind::Class(c) => {
                    let s = sym(&c.name);
                    let coid = self.tc.intern(&c.name);
                    self.tb.class_syms.insert(coid, s.clone());
                    let Some(ci) = self.tc.find_class(coid) else {
                        return bug(format!("KIR の書き出し: クラスがありません: {}", c.name));
                    };
                    let mut methods = Vec::with_capacity(c.vals.len());
                    for v in &c.vals {
                        let Some(scheme) = ci
                            .ci_methods
                            .iter()
                            .find(|(n, _)| *n == v.name)
                            .map(|(_, t)| *t)
                        else {
                            return bug("KIR の書き出し: メソッドがありません");
                        };
                        let n = self.tc.constraints_of(scheme)?.len();
                        let Some(k) = n.checked_sub(1) else {
                            return bug("KIR の書き出し: メソッドの型に制約がありません");
                        };
                        methods.push(kir::ClassMethod {
                            name: v.name.clone(),
                            dicts: k as u32,
                        });
                    }
                    classes.push(kir::Class { sym: s, methods });
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// 単位の大域、インスタンス、手順を決める(1 段目の後半)。
    fn collect_unit<'b>(
        &mut self,
        u: &UnitSrc<'b>,
        globals: &mut Vec<(String, PendingGlobal)>,
        instances: &mut Vec<PendingInstance>,
        jobs: &mut Vec<Job<'b>>,
    ) -> R<Vec<PendingStep>> {
        let key = &u.key;
        let print = u.repl.is_some_and(|r| r.print);
        let display = u.repl.is_some_and(|r| r.display);
        let thunk = |sym: String, result: Rep, body: Body<'b>| -> Job<'b> {
            let mut j = thunk_job(sym, result, body);
            j.print = print;
            j
        };
        let mut counts: HashMap<String, u32> = HashMap::default();
        let mut inst_counts: HashMap<String, u32> = HashMap::default();
        let numbered = |tbl: &mut HashMap<String, u32>, q: &str| -> String {
            let e = tbl.entry(q.to_string()).or_insert(0);
            let n = *e;
            *e += 1;
            if n == 0 {
                q.to_string()
            } else {
                format!("{q}#{n}")
            }
        };
        let mut steps: Vec<PendingStep> = Vec::new();
        let new_global = |w: &mut Self,
                          globals: &mut Vec<(String, PendingGlobal)>,
                          counts: &mut HashMap<String, u32>,
                          oid: NodeId,
                          x: &str,
                          kind: GKind|
         -> GlobalId {
            let g = format!("{key}/{}", numbered(counts, super::display(x)));
            let id = GlobalId(globals.len() as u32);
            w.tb.globals.insert((oid, x.to_string()), id);
            if display {
                w.repl_out.globals.push(((oid, x.to_string()), id));
            }
            let k = match kind {
                GKind::Fn => PendingGlobal::Fn(g.clone()),
                GKind::Dictabs => PendingGlobal::Dictabs(g.clone()),
                GKind::Value(r) => PendingGlobal::Value(r),
                GKind::Extern(abi, name, dicts) => PendingGlobal::Extern { abi, name, dicts },
            };
            globals.push((g, k));
            id
        };
        let gsym =
            |globals: &Vec<(String, PendingGlobal)>, g: GlobalId| globals[g.index()].0.clone();
        for d in u.decls {
            match &d.kind {
                DeclKind::Let(b) => {
                    let name = strip_annot(&b.name);
                    let ks = self.dict_params_of(b.meta.id);
                    match (&b.params, &name.kind) {
                        (Some(ps), PatKind::Var(x)) => {
                            let dk = ks.is_empty();
                            let g = new_global(
                                self,
                                globals,
                                &mut counts,
                                b.meta.id,
                                x,
                                if dk { GKind::Fn } else { GKind::Dictabs },
                            );
                            steps.push(PendingStep::DefFns(vec![g]));
                            let s = gsym(globals, g);
                            jobs.push(self.fn_def_job(s, b, ps, &b.body, b.meta.id));
                        }
                        (Some(_), _) => {
                            return bug("KIR の書き出し: 関数の定義の名前が変数ではありません");
                        }
                        (None, PatKind::Var(x)) if !ks.is_empty() => {
                            let g = new_global(
                                self,
                                globals,
                                &mut counts,
                                b.meta.id,
                                x,
                                GKind::Dictabs,
                            );
                            steps.push(PendingStep::DefDictabs(g));
                            let result = self.rep_of_node(b.body.meta.id);
                            jobs.push(Job {
                                sym: gsym(globals, g),
                                kind: FnKind::Dictabs,
                                params: Vec::new(),
                                dkeys: ks,
                                tail: false,
                                yields: true,
                                result,
                                body: Body::Exp(&b.body, false),
                                adapter: Vec::new(),
                                print: false,
                            });
                        }
                        (None, PatKind::Var(x)) => {
                            let rep = self.rep_of_name(name, b);
                            let g = new_global(
                                self,
                                globals,
                                &mut counts,
                                b.meta.id,
                                x,
                                GKind::Value(rep),
                            );
                            let s = gsym(globals, g);
                            steps.push(PendingStep::DefValue(g, s.clone()));
                            let result = self.rep_of_node(b.body.meta.id);
                            jobs.push(thunk(s, result, Body::Exp(&b.body, false)));
                        }
                        (None, _) => {
                            let f = format!("{key}/$init{}", steps.len());
                            let mut gs: Vec<GlobalId> = Vec::new();
                            let bid = b.meta.id;
                            let p = {
                                let counts = &mut counts;
                                let gs = &mut gs;
                                let globals = &mut *globals;
                                self.pat(&b.name, &mut |w, x, pnode| {
                                    let rep = w.rep_of_node(pnode.meta.id);
                                    let g =
                                        new_global(w, globals, counts, bid, x, GKind::Value(rep));
                                    gs.push(g);
                                    Ok((gs.len() - 1) as u32)
                                })?
                            };
                            steps.push(PendingStep::DefPattern(f.clone(), gs, p));
                            let result = self.rep_of_node(b.body.meta.id);
                            let job = if ks.is_empty() {
                                thunk(f, result, Body::Exp(&b.body, false))
                            } else {
                                thunk(f, Rep::Any, Body::DictabsExp(ks, result, &b.body))
                            };
                            jobs.push(job);
                        }
                    }
                }
                DeclKind::LetRec(bs) => {
                    let mut gs = Vec::with_capacity(bs.len());
                    for b in bs {
                        match &strip_annot(&b.name).kind {
                            PatKind::Var(x) => {
                                let dk = self.dict_params_of(b.meta.id).is_empty();
                                gs.push(new_global(
                                    self,
                                    globals,
                                    &mut counts,
                                    b.meta.id,
                                    x,
                                    if dk { GKind::Fn } else { GKind::Dictabs },
                                ));
                            }
                            _ => return bug("KIR の書き出し: let rec の名前が変数ではありません"),
                        }
                    }
                    steps.push(PendingStep::DefFns(gs.clone()));
                    for (g, b) in gs.iter().zip(bs) {
                        let s = gsym(globals, *g);
                        let job = match (&b.params, &b.body.kind) {
                            (Some(ps), _) => self.fn_def_job(s, b, ps, &b.body, b.meta.id),
                            (None, ExpKind::Lambda(l)) => {
                                self.fn_def_job(s, b, &l.params, &l.body, b.body.meta.id)
                            }
                            _ => return bug("KIR の書き出し: let rec の右辺が関数ではありません"),
                        };
                        jobs.push(job);
                    }
                }
                DeclKind::Extern(ex) => {
                    let dicts = match self.tc.dict.get(&d.meta.id) {
                        Some(DictInfo::Abs(ks)) => ks.len() as u32,
                        _ => 0,
                    };
                    let abi = if ex.abi == "C" { Abi::C } else { Abi::Prim };
                    let kind = GKind::Extern(abi, ex.prim.clone(), dicts);
                    let g = new_global(self, globals, &mut counts, d.meta.id, &ex.name, kind);
                    steps.push(PendingStep::DefFns(vec![g]));
                }
                DeclKind::Exp(e) if display => {
                    // 対話的な実行の表示のため、式文の値を隠れた大域に束縛する。
                    let result = self.rep_of_node(e.meta.id);
                    match self.tc.dict.get(&d.meta.id) {
                        Some(DictInfo::Abs(ks)) => {
                            let ks = ks.clone();
                            let g = new_global(
                                self,
                                globals,
                                &mut counts,
                                d.meta.id,
                                "$it",
                                GKind::Dictabs,
                            );
                            steps.push(PendingStep::DefDictabs(g));
                            jobs.push(Job {
                                sym: gsym(globals, g),
                                kind: FnKind::Dictabs,
                                params: Vec::new(),
                                dkeys: ks,
                                tail: false,
                                yields: true,
                                result,
                                body: Body::Exp(e, false),
                                adapter: Vec::new(),
                                print: false,
                            });
                        }
                        _ => {
                            let g = new_global(
                                self,
                                globals,
                                &mut counts,
                                d.meta.id,
                                "$it",
                                GKind::Value(result),
                            );
                            let s = gsym(globals, g);
                            steps.push(PendingStep::DefValue(g, s.clone()));
                            jobs.push(thunk(s, result, Body::Exp(e, false)));
                        }
                    }
                }
                DeclKind::Exp(e) => {
                    if !matches!(self.tc.dict.get(&d.meta.id), Some(DictInfo::Abs(_))) {
                        let f = format!("{key}/$init{}", steps.len());
                        steps.push(PendingStep::RunExp(f.clone()));
                        let result = self.rep_of_node(e.meta.id);
                        jobs.push(thunk(f, result, Body::Exp(e, false)));
                    }
                }
                DeclKind::Instance(i) => {
                    let (cls, con) = self.instance_head(d, i)?;
                    let class = self.class_id(cls)?;
                    let head = self.type_id(con)?;
                    let csym = self.tb.class_syms[&cls].clone();
                    let hsym = self.tb.type_syms[&con].clone();
                    let isym = format!(
                        "{key}/{}",
                        numbered(
                            &mut inst_counts,
                            &format!("instance:{}[{}]", sym_name(&csym), sym_name(&hsym))
                        )
                    );
                    let iid = InstanceId(instances.len() as u32);
                    self.tb.instances.insert((cls, con), iid);
                    let pk = match self.tc.dict.get(&d.meta.id) {
                        Some(DictInfo::Instance(ks)) => ks.clone(),
                        _ => Vec::new(),
                    };
                    let mut methods = Vec::new();
                    let mut method_job =
                        |w: &mut Self, x: &str, b: &LetBinding, body: Body<'b>| -> R<()> {
                            let (mk, adapter) = match w.tc.dict.get(&b.meta.id) {
                                Some(DictInfo::Method(_, mk, ad)) => (mk.clone(), ad.clone()),
                                _ => (Vec::new(), Vec::new()),
                            };
                            let fsym = format!("{isym}.{x}");
                            let m = w.method_index(class, x)?;
                            methods.push((m, mk.len() as u32, fsym.clone()));
                            let mut dkeys = pk.clone();
                            dkeys.extend(mk);
                            let value_init = matches!(body, Body::Binding(b) if b.params.is_none());
                            let print = print && value_init && dkeys.is_empty();
                            jobs.push(Job {
                                sym: fsym,
                                kind: FnKind::Dictabs,
                                params: Vec::new(),
                                dkeys,
                                tail: false,
                                yields: true,
                                result: Rep::Any,
                                body,
                                adapter,
                                print,
                            });
                            Ok(())
                        };
                    for d2 in &i.body {
                        match &d2.kind {
                            DeclKind::Let(b) => {
                                if let PatKind::Var(x) = &strip_annot(&b.name).kind {
                                    method_job(self, x, b, Body::Binding(b))?;
                                }
                            }
                            DeclKind::LetRec(bs) => {
                                for b in bs {
                                    if let PatKind::Var(x) = &strip_annot(&b.name).kind {
                                        method_job(self, x, b, Body::RecGroup(bs, x.clone()))?;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    instances.push(PendingInstance {
                        sym: isym,
                        class,
                        head,
                        premises: pk.len() as u32,
                        methods,
                    });
                    steps.push(PendingStep::DefInstance(iid));
                }
                DeclKind::Type(_)
                | DeclKind::Newtype(_)
                | DeclKind::Effect(_)
                | DeclKind::Class(_) => {}
                DeclKind::Module(..) => {
                    return bug("KIR の書き出し: module が平坦化されていません");
                }
            }
        }
        if let Some(r) = u.repl.filter(|r| r.display) {
            // 対話的な実行の表示に使う、Show.show を呼ぶ関数。
            for (ev, ks) in &r.shows {
                let (kind, gk, body) = if ks.is_empty() {
                    (FnKind::Lambda, GKind::Fn, Body::ReplShowCall(ev.clone()))
                } else {
                    (
                        FnKind::Dictabs,
                        GKind::Dictabs,
                        Body::ReplShowLambda(ev.clone()),
                    )
                };
                let g = new_global(self, globals, &mut counts, 0, "$show", gk);
                steps.push(if ks.is_empty() {
                    PendingStep::DefFns(vec![g])
                } else {
                    PendingStep::DefDictabs(g)
                });
                self.repl_out.shows.push(g);
                jobs.push(Job {
                    sym: gsym(globals, g),
                    kind,
                    params: if ks.is_empty() {
                        vec![Rep::Any]
                    } else {
                        Vec::new()
                    },
                    dkeys: ks.clone(),
                    tail: ks.is_empty(),
                    yields: true,
                    result: if ks.is_empty() { Rep::Str } else { Rep::Any },
                    body,
                    adapter: Vec::new(),
                    print: false,
                });
            }
        }
        Ok(steps)
    }

    /// 関数の定義の根の関数の仕事(Diktor の `fn_def_root`)。
    fn fn_def_job<'b>(
        &mut self,
        sym: String,
        b: &LetBinding,
        params: &'b [Pat],
        body: &'b Exp,
        fn_node: NodeId,
    ) -> Job<'b> {
        let ks = self.dict_params_of(b.meta.id);
        if ks.is_empty() {
            let reps = params.iter().map(|p| self.rep_of_node(p.meta.id)).collect();
            Job {
                sym,
                kind: FnKind::Lambda,
                params: reps,
                dkeys: Vec::new(),
                tail: true,
                yields: self.yield_of_node(fn_node),
                result: self.rep_of_node(body.meta.id),
                body: Body::Params(params, body),
                adapter: Vec::new(),
                print: false,
            }
        } else {
            Job {
                sym,
                kind: FnKind::Dictabs,
                params: Vec::new(),
                dkeys: ks,
                tail: false,
                yields: true,
                result: Rep::Any,
                body: Body::Lambda(params, body, fn_node),
                adapter: Vec::new(),
                print: false,
            }
        }
    }
}

/// 種類が thunk の根の関数の仕事。
fn thunk_job(sym: String, result: Rep, body: Body<'_>) -> Job<'_> {
    Job {
        sym,
        kind: FnKind::Thunk,
        params: Vec::new(),
        dkeys: Vec::new(),
        tail: false,
        yields: true,
        result,
        body,
        adapter: Vec::new(),
        print: false,
    }
}

/// 呼び出しと perform の引数。引数は `_item` のレコードの式で、外側の要素が先頭の引数である。
fn args_of(e: &Exp) -> R<Vec<&Exp>> {
    let mut v = Vec::new();
    let mut cur = e;
    loop {
        match &cur.kind {
            ExpKind::RecordExtend(rest, _, x) => {
                v.push(&**x);
                cur = rest;
            }
            ExpKind::RecordEmpty => return Ok(v),
            _ => return bug("KIR の書き出し: 引数がレコードの式ではありません"),
        }
    }
}

/// OCaml の `float_of_string` である。
///
/// 桁区切りの `_` を除き、符号の後ろが `0x` か `0X` なら 16 進(OCaml の実行時の `caml_float_of_hex`)で、
/// ほかは C の `strtod` で読み、全体を読み切れなければ失敗する。
/// `strtod` は正しく丸めるので、10 進は Rust の `parse` と同じ値になる。
/// 字句の上で数値リテラルになる綴り(数字、`.`、`e`、`E`、`+`、`-`、`x`、`o`、`b` と 16 進の数字)だけを考える。
pub(crate) fn ocaml_float_of_string(s: &str) -> Option<f64> {
    let t: String = s.chars().filter(|c| *c != '_').collect();
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    let v = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        float_of_hex(hex)?
    } else {
        if body.is_empty()
            || !body
                .bytes()
                .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
            || !body.as_bytes()[0].is_ascii_digit()
        {
            return None;
        }
        body.parse::<f64>().ok()?
    };
    Some(if neg { -v } else { v })
}

/// OCaml の実行時の `caml_float_of_hex`(指数の `p` を除く)。
fn float_of_hex(s: &str) -> Option<f64> {
    let mut m: u64 = 0;
    let mut n_bits: i64 = 0;
    let mut m_bits = 0;
    let mut x_bits: i64 = 0;
    let mut dec_point: i64 = -1;
    for c in s.bytes() {
        if c == b'.' {
            if dec_point >= 0 {
                return None;
            }
            dec_point = n_bits;
            continue;
        }
        let d = (c as char).to_digit(16)? as u64;
        n_bits += 4;
        if d == 0 && m == 0 {
            continue;
        }
        if m_bits < 60 {
            m = (m << 4) + d;
            m_bits += 4;
        } else {
            if d != 0 {
                m |= 1;
            }
            x_bits += 4;
        }
    }
    if n_bits == 0 {
        return None;
    }
    let mut f = m as i64 as f64;
    let mut exp = x_bits;
    if dec_point >= 0 {
        exp += dec_point - n_bits;
    }
    // ldexp。2 の冪を掛けるのは、桁あふれと桁落ちのとき以外は正確である。
    while exp > 1000 {
        f *= 2f64.powi(1000);
        exp -= 1000;
    }
    while exp < -1000 {
        // 正規化数のうちに掛け終えるよう、まず仮数を正規化数の範囲で下げる。
        f *= 2f64.powi(-1000);
        exp += 1000;
    }
    Some(f * 2f64.powi(exp as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocaml_の_float_of_string() {
        assert_eq!(ocaml_float_of_string("1_000.5"), Some(1000.5));
        assert_eq!(ocaml_float_of_string("-0x10"), Some(-16.0));
        assert_eq!(ocaml_float_of_string("1."), Some(1.0));
        assert_eq!(ocaml_float_of_string("1e400"), Some(f64::INFINITY));
        assert_eq!(ocaml_float_of_string("0o17"), None);
        assert_eq!(ocaml_float_of_string("0b1"), None);
        assert_eq!(ocaml_float_of_string("0x"), None);
        // 60 ビットを超える 16 進は奇数に丸めてから double にする。
        assert_eq!(
            ocaml_float_of_string("0x1000000000000001"),
            Some(1152921504606846976.0)
        );
    }
}
