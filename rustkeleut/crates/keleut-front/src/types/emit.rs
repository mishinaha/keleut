//! 型検査を済ませた単位の並びを KIR として書き出す(Diktor の ir_write.ml、第18章に当たる)。
//! 単位はコンパイル単位で、プレリュード、import したファイル、起点、REPL の入力のそれぞれが 1 つの単位になる。
//!
//! 仕様はリポジトリの根の `doc/IRSpec.md` が定める。
//! 出力は、`diktor --emit-ir` と `--emit-ir-text` の出力にバイト単位で一致させる。
//! 名前の付け方、スロットとキャプチャの番号、表の順序は、どれも Diktor のライタ(KIR を書き出す側)と同じ規則で決める。
//! 本ファイルの区切りのコメントにある §18.N は、ir_write.ml の節の番号である。
//!
//! 書き出しは Diktor と同じく 2 段階で行う。
//! 第 1 段階では、すべての単位の宣言をたどって、型、エフェクト、クラス、インスタンス、グローバルを表に置き、単位ごとの初期化手順と、ルート関数を書き出すジョブの並びを作る。
//! 第 2 段階では、ジョブを順に実行し、ルート関数と、そこから持ち上げた関数(ラムダリフティングで取り出した関数)を関数表に追加する。
//!
//! Diktor は、表の項目をシンボルの文字列で指す KIR を作ってから、符号化のときにシンボルを添字に直す。
//! 本モジュールは [`kir::Program`] の添字を直接書く。
//! 型、エフェクト、クラス、インスタンス、グローバルの添字は第 1 段階で決まるので、その場で書く。
//! 関数の添字は、ルート関数を書き終えて、持ち上げた関数を生成箇所の番号の順に並べるまで決まらない。
//! そこで、持ち上げた関数を作る式(`closure`、`dictabs`、`handle`)の `Part` には、書き出し中の関数(`Frame`)の番号を仮に書いておく。
//! ルート関数を書き終えてから、この番号を関数の添字とキャプチャの並びに書き換える。
//! キャプチャの番号も、Diktor と同じく、書き出し中の関数をまたいで一意な仮の番号で書いておき、ルート関数を書き終えたときに確定した番号に書き換える。
//! 書き換える式と辞書は、作ったときに配列に記録しておくので、木をたどり直す必要はない。
//!
//! 式、辞書、パターンのノードを [`kir::Nodes`] に置く順は、出力に影響しない。
//! テキストとバイナリのライタは、関数の本体から木をたどって書くからである。

use super::check::{CheckOptions, Checked};
use super::exhaust::{ocaml_int32_opt, ocaml_int64_opt};
use super::fx::{HashMap, JMap};
use super::pmap::PMap;
use super::{DKey, DictInfo, Evidence, GRef, HoleId, Oid, R, Resolved, Tc, Ty, TyNode, bug};
use crate::ast::{BinOp, Clause, TypeExpKind};
use crate::ast::{Decl, DeclKind, Exp, ExpKind, LetBinding, NodeId, Number, Pat, PatKind};
use kir::{
    Abi, Cap, ClassId, ClauseClass, Dict, DictId, EffectId, ExpId, FnId, FnKind, GlobalId,
    GlobalKind, HandlerOp, InstanceId, Lit, Nodes, Part, PatId, RecordTail, Rep, Step, TypeId,
};
use std::rc::Rc;

/// 書き出す単位である(Diktor の `unit_src`)。
pub struct UnitSrc<'a> {
    /// 単位のキー。
    pub key: String,
    /// 元のファイルのパスの並び。
    pub paths: Vec<Vec<u8>>,
    /// 平坦化と型検査を済ませた宣言の並び。
    pub decls: &'a [Decl],
    /// REPL の入力の単位を書き出すときの設定。ファイルの単位では `None` である。
    pub repl: Option<&'a ReplUnit>,
}

/// REPL の入力の単位を書き出すときの設定である(Diktor の driver.ml の §16.6b に当たる)。
///
/// Diktor の REPL は、トップレベルのエフェクト行に `Print` を追加し、最も外側のハンドラが `Print.print` を出力先へ送る。
/// KIR には、ランタイムが持つ最も外側のハンドラがない。
/// そこで、この単位の初期化式と式文の `thunk`、および辞書を受け取らないメソッドの値の初期化を、ライタが作る `Print` のハンドラ式で囲む。
/// このハンドラは `print(s)` を `resume(perform Console.write(s))` に置き換えるので、出力される文字列は同じである。
/// プレリュードの `with_stdout` は使わない。
/// そのため、`--prelude` で渡したプレリュードに `with_stdout` がなくても、別の定義の `with_stdout` があっても、結果は変わらない。
#[derive(Clone, Debug, Default)]
pub struct ReplUnit {
    /// 真なら、`Print` をハンドラ式で処理する。
    /// プレリュードに、引数 1 個の操作 `print` を持つ `Print` と、ランタイムが処理する `Console.write` があるときだけ有効である。
    pub print: bool,
    /// 結果を表示する入力なら真である。
    /// 真のとき、式文の値を内部用のグローバル(`$it`)に束縛する。
    /// 辞書を受け取る式文は、辞書を受け取るグローバル(種類 `dictabs`)にする。
    pub display: bool,
    /// `Show.show` で表示する値ごとの、`Show` の証拠(どのインスタンスの辞書を使うかを表す項)と、証拠のうちまだ決まっていない部分に渡す辞書の引数。
    /// 値ごとに内部用のグローバル(`$show`)を作る。
    /// 辞書の引数がなければ、種類 `fn` のグローバルにする。その関数は、値を受け取って文字列を返す。
    /// 辞書の引数があれば、種類 `dictabs` のグローバルにする。その関数は、辞書を受け取って、値を受け取る関数を返す。
    pub shows: Vec<(Evidence, Vec<DKey>)>,
}

/// REPL の入力の単位を書き出した結果である。
#[derive(Clone, Debug, Default)]
pub struct ReplOut {
    /// (束縛のノードの番号, 名前)の組から、グローバルへの対応。
    /// 式文は、式文のノードの番号と `$it` の組で登録する。
    /// [`ReplUnit::display`] が真のときだけ記録する。
    pub globals: Vec<((NodeId, String), GlobalId)>,
    /// `$show` のグローバル。[`ReplUnit::shows`] と同じ順に並ぶ。
    pub shows: Vec<GlobalId>,
}

/// 型検査を済ませたプログラムを KIR に変換する(Diktor の driver.ml の `ir_program`)。
///
/// `keys` は [`Checked::unit_keys`] の結果である。
/// 単位は、プレリュード、import した単位(初期化の順)、起点の順に並べる。
/// `checked` には、型検査がエラーなく終わったものを渡す。
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
        return bug("KIR の書き出し: 単位の数がキーの数と合いません");
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

/// シンボルのうち、最後の `/` より後ろの名前の部分を返す。
fn sym_name(s: &str) -> &str {
    match s.rfind('/') {
        Some(i) => &s[i + 1..],
        None => s,
    }
}

/// 修飾名を最後の `.` で 2 つに分ける。
fn split_last_dot(s: &str) -> R<(String, String)> {
    match s.rfind('.') {
        Some(i) => Ok((s[..i].to_string(), s[i + 1..].to_string())),
        None => bug(format!("KIR の書き出し: 修飾名ではありません: {s}")),
    }
}

/// パターンの型注釈を取り除く。
fn strip_annot(p: &Pat) -> &Pat {
    let mut p = p;
    while let PatKind::Annot(q, _) = &p.kind {
        p = q;
    }
    p
}

/// グローバルの種類である。関数はシンボルで指しておき、最後に添字に直す。
enum PendingGlobal {
    Extern { abi: Abi, name: String, dicts: u32 },
    Fn(String),
    Dictabs(String),
    Value(Rep),
}

/// グローバルを追加するときに指定する種類である。
enum GKind {
    Fn,
    Dictabs,
    Value(Rep),
    Extern(Abi, String, u32),
}

/// 初期化手順である。関数はシンボルで指す。
enum PendingStep {
    DefValue(GlobalId, String),
    DefPattern(String, Vec<GlobalId>, PatId),
    DefFns(Vec<GlobalId>),
    DefDictabs(GlobalId),
    DefInstance(InstanceId),
    RunExp(String),
}

/// インスタンスである。メソッドを実装する関数はシンボルで指す。
struct PendingInstance {
    sym: String,
    class: ClassId,
    head: TypeId,
    premises: u32,
    methods: Vec<(u32, u32, String)>,
}

/// 書き出しに使う表である(Diktor の `tables` と、符号化のときにシンボルから添字を求める表に当たる)。
#[derive(Default)]
struct Tables {
    /// 名前の番号(`Oid`)からシンボルへの表(型、エフェクト、クラス)。
    type_syms: JMap<Oid, String>,
    effect_syms: JMap<Oid, String>,
    class_syms: JMap<Oid, String>,
    /// シンボルを置いたが、まだ添字を決めていない名前。[`W::add_units`] が添字を決めて空にする。
    new_syms: [Vec<Oid>; 3],
    /// 名前の番号(`Oid`)から添字への表(型、エフェクト、クラス)。
    /// 同じシンボルの項目が複数あれば、最初の項目の添字である。
    type_ids: JMap<Oid, TypeId>,
    effect_ids: JMap<Oid, EffectId>,
    class_ids: JMap<Oid, ClassId>,
    /// (属する型、エフェクト、クラスの添字, 修飾しない名前)から、その中での位置への表(コンストラクタ、操作、メソッド)。
    ctor_idx: HashMap<(u32, String), u32>,
    op_idx: HashMap<(u32, String), u32>,
    meth_idx: HashMap<(u32, String), u32>,
    instances: JMap<(Oid, Oid), InstanceId>,
    globals: HashMap<(NodeId, String), GlobalId>,
    /// 組み込みの値の名前から添字への表。
    builtin_globals: HashMap<String, GlobalId>,
}

/// [`Tables`] の表のうち、マークの時点に戻せるものすべてに同じ操作をする([`ReplWriter::mark`])。
macro_rules! each_marked {
    ($tb:expr, $op:ident) => {
        $tb.type_syms.$op();
        $tb.effect_syms.$op();
        $tb.class_syms.$op();
        $tb.type_ids.$op();
        $tb.effect_ids.$op();
        $tb.class_ids.$op();
        $tb.instances.$op();
    };
}

impl Tables {
    /// 名前 `o` のシンボルを置く。`k` は種類で、型が 0、エフェクトが 1、クラスが 2 である。
    fn set_sym(&mut self, k: usize, o: Oid, s: String) {
        match k {
            0 => self.type_syms.insert(o, s),
            1 => self.effect_syms.insert(o, s),
            _ => self.class_syms.insert(o, s),
        };
        self.new_syms[k].push(o);
    }
}

// ---------------------------------------------------------------------------
// 書き出し中の関数(§18.5)
// ---------------------------------------------------------------------------

/// 書き出している途中の関数である(Diktor の `frame`)。以下では「書き出し中の関数」と呼ぶ。
struct Frame {
    parent: Option<usize>,
    sym: String,
    kind: FnKind,
    params: Vec<Rep>,
    locals: Vec<Rep>,
    /// キャプチャの(仮の番号, 親の関数での読み方)の並び。追加した順である。
    caps: Vec<(u32, Cap)>,
    cap_index: HashMap<Cap, u32>,
    /// `resume` の持ち主(その `resume` が再開する継続を受け取る操作節)である、書き出し中の関数の番号。
    owner: Option<usize>,
    ntail_resumes: u32,
    final_caps: Vec<Cap>,
    cap_reps: Vec<Rep>,
}

/// 書き終えた関数の内容である(Diktor の `pending`)。
struct Pending {
    yields: bool,
    dicts: u32,
    result: Rep,
    pats: Vec<PatId>,
    guard: Option<ExpId>,
    body: ExpId,
}

/// ルート関数ごとの状態である。
#[derive(Default)]
struct Root {
    sym: String,
    sites: u32,
    frames: Vec<Frame>,
    fns: Vec<((u32, u32), usize, Pending)>,
    /// キャプチャを読む式と辞書。仮の番号を書いてあり、後で確定した番号に書き換える。
    env_exps: Vec<ExpId>,
    env_dicts: Vec<DictId>,
    /// `Part` に書き出し中の関数の番号を仮に書いた式。
    part_exps: Vec<ExpId>,
}

#[derive(Clone, Copy)]
struct Binding {
    frame: usize,
    var: Cap,
}

/// 書き出しの環境である(Diktor の `genv`)。
#[derive(Clone)]
struct GEnv {
    fr: usize,
    names: PMap<Rc<str>, Binding>,
    dkeys: PMap<DKey, Binding>,
    /// 末尾位置にあれば真である。
    tail: bool,
    /// 操作節の背骨(操作節の本体から末尾位置だけをたどった経路)の上にあれば真である。
    spine: bool,
}

impl GEnv {
    /// 末尾位置になく、背骨の外にある子を書くための環境を返す。
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

/// 関数の本体の書き出し方である(Diktor はクロージャで渡す)。
enum Body<'b> {
    /// 式をそのまま書く。2 番目の値が真なら、末尾位置の外で書く。
    Exp(&'b Exp, bool),
    /// 無名関数の生成箇所として書く。
    Lambda(&'b [Pat], &'b Exp, NodeId),
    /// 仮引数のパターンを変換してから本体を書く(種類 `lambda` のルート関数)。
    Params(&'b [Pat], &'b Exp),
    /// 辞書を受け取る関数の生成箇所として書き、その中で式を書く。
    DictabsExp(Vec<DKey>, Rep, &'b Exp),
    /// 束縛の値を書く(Diktor の `binding_value`)。
    Binding(&'b LetBinding),
    /// `let rec` のグループを書き、その中の名前 `x` を読む(インスタンスのメソッド)。
    RecGroup(&'b [LetBinding], String),
    /// REPL が値を表示するときに使う。スロット 0 の引数の値を、証拠の辞書の `Show.show` に末尾呼び出しで渡す(グローバルの種類が `fn` のルート関数)。
    ReplShowCall(Evidence),
    /// REPL が値を表示するときに使う。[`Body::ReplShowCall`] を本体とする無名関数を作る(グローバルの種類が `dictabs` のルート関数)。
    ReplShowLambda(Evidence),
}

/// ルート関数を 1 つ書き出すジョブである。
struct Job<'b> {
    sym: String,
    kind: FnKind,
    params: Vec<Rep>,
    dkeys: Vec<DKey>,
    tail: bool,
    yields: bool,
    result: Rep,
    body: Body<'b>,
    /// 書いた本体に適用する辞書。インスタンスのメソッドのアダプタに使う。
    adapter: Vec<HoleId>,
    /// 真なら、本体を `Print` のハンドラ式で囲む([`ReplUnit::print`])。
    print: bool,
}

/// ライタの状態である。
struct W<'t> {
    tc: &'t mut Tc,
    tb: Tables,
    nodes: Nodes,
    root: Root,
    /// キャプチャの仮の番号のカウンタ。
    provisional: u32,
    /// キャプチャの仮の番号から確定した番号への表。
    final_cap: HashMap<u32, u32>,
    functions: Vec<(kir::Function, String)>,
    /// 関数のシンボルから、そのシンボルを持つ最初の関数の添字への表。
    /// Diktor が符号化のときに使う表と同じく、最初の項目を指す。
    fn_first: HashMap<String, FnId>,
    /// REPL で `Print` を処理するハンドラ式が使う、エフェクトと操作([`ReplUnit::print`])。
    repl_print: Option<ReplPrint>,
    /// REPL の入力の単位を書き出した結果。
    repl_out: ReplOut,
    /// この書き出しで追加するグローバル、インスタンス、関数の最初の添字。
    /// それまでの書き出しが置いた項目の個数に等しい。
    gbase: u32,
    ibase: u32,
    fbase: u32,
    /// 型、エフェクト、クラスのシンボルから添字への表。
    /// [`ReplWriter`] の `type_index`、`effect_index`、`class_index` を、書き出しの間だけ移してきたものである。
    index: [HashMap<String, u32>; 3],
}

/// REPL で `Print` を処理するハンドラ式が使う、エフェクトの添字と、そのエフェクトの中での操作の位置である。
#[derive(Clone, Copy, Debug)]
struct ReplPrint {
    /// エフェクト `Print` と、その操作 `print`。
    print: (EffectId, u32),
    /// エフェクト `Console` と、その操作 `write`。
    write: (EffectId, u32),
}

/// 型検査を済ませた単位の並びを KIR に変換する(Diktor の `program`)。
pub fn program(tc: &mut Tc, units: &[UnitSrc<'_>]) -> R<kir::Program> {
    program_repl(tc, units).map(|(p, _)| p)
}

/// [`program`] と同じ変換を行い、REPL の入力の単位([`UnitSrc::repl`])を書き出した結果も返す。
pub fn program_repl(tc: &mut Tc, units: &[UnitSrc<'_>]) -> R<(kir::Program, ReplOut)> {
    let mut rw = ReplWriter::default();
    let mut kp = empty_program();
    let mut w = rw.writer(tc, &mut kp);
    w.builtins(&mut kp);
    let r = w.add_units(&mut kp, units);
    let out = rw.restore(w, &mut kp);
    r?;
    Ok((kp, out))
}

/// どの表も空である KIR のプログラムを作る。
fn empty_program() -> kir::Program {
    kir::Program {
        version: kir::VERSION,
        units: Vec::new(),
        types: Vec::new(),
        effects: Vec::new(),
        classes: Vec::new(),
        instances: Vec::new(),
        globals: Vec::new(),
        functions: Vec::new(),
        nodes: Nodes::default(),
    }
}

/// 単位を後ろに追加していく KIR のライタである。REPL が使う(Diktor の driver.ml の §16.6b に当たる)。
///
/// REPL は、受理した入力ごとに単位を 1 つプログラムの後ろに追加し、それまでの単位はランタイムに残したまま使う。
/// [`ReplWriter::add`] は、[`program`] と同じ規則で単位を 1 つ書き出し、プログラムの各表の後ろに項目を追加する。
/// それまでの項目の添字は変わらない。
/// 追加した単位の式は、それまでの単位のグローバルと型を同じ添字で指す。
/// 書き出しに使う表(名前から型、エフェクト、クラス、インスタンスを引く表)は、追加した単位の宣言が以後の単位から見えるように書き換える。
/// 処理時間は、追加した単位のサイズに比例する。
///
/// 入力を受理しなかったときは、表の書き換えを取り消せる。
/// [`ReplWriter::mark`] で置いたマークの時点に、[`ReplWriter::rollback`] で戻す。
/// 戻しても、プログラムに追加した項目は残す。関数とグローバルの番号を詰め直さずに済ませるためである。
/// 単位には名前の衝突しないキーを使うので、残した項目が後の単位の項目と重なることはない。
#[derive(Default)]
pub struct ReplWriter {
    tb: Tables,
    fn_first: HashMap<String, FnId>,
    repl_print: Option<ReplPrint>,
    /// 型、エフェクト、クラスのシンボルから添字への表。
    /// 同じシンボルの項目が複数あれば、最初の項目を指す。
    type_index: HashMap<String, u32>,
    effect_index: HashMap<String, u32>,
    class_index: HashMap<String, u32>,
}

/// [`ReplWriter::mark`] が返す、名前で引く表のマークである。
///
/// [`ReplWriter::commit`] で外すか、[`ReplWriter::rollback`] でマークの時点に戻す。
#[must_use]
pub struct WriterMark(());

/// プログラムの各表のサイズを記録したマークである。
/// 書き出しが途中で失敗したときと、追加した単位を実行せずに捨てるときに、追加した項目を取り除くために使う([`ProgramMark::truncate`])。
pub struct ProgramMark {
    types: usize,
    effects: usize,
    classes: usize,
    instances: usize,
    globals: usize,
    functions: usize,
    units: usize,
    exps: usize,
    dicts: usize,
    pats: usize,
}

impl ProgramMark {
    /// プログラム `kp` の現在のサイズを記録する。
    pub fn of(kp: &kir::Program) -> ProgramMark {
        ProgramMark {
            types: kp.types.len(),
            effects: kp.effects.len(),
            classes: kp.classes.len(),
            instances: kp.instances.len(),
            globals: kp.globals.len(),
            functions: kp.functions.len(),
            units: kp.units.len(),
            exps: kp.nodes.exps.len(),
            dicts: kp.nodes.dicts.len(),
            pats: kp.nodes.pats.len(),
        }
    }

    /// `kp` を、マークを取ったときのサイズに戻す。実行した単位には使わない。
    pub fn truncate(&self, kp: &mut kir::Program) {
        kp.types.truncate(self.types);
        kp.effects.truncate(self.effects);
        kp.classes.truncate(self.classes);
        kp.instances.truncate(self.instances);
        kp.globals.truncate(self.globals);
        kp.functions.truncate(self.functions);
        kp.units.truncate(self.units);
        kp.nodes.exps.truncate(self.exps);
        kp.nodes.dicts.truncate(self.dicts);
        kp.nodes.pats.truncate(self.pats);
    }
}

impl ReplWriter {
    /// 組み込みの単位と `units`(プレリュード)を書き出したプログラムを作る。
    ///
    /// 型検査と同じ [`check::on_check_stack`](super::check::on_check_stack) のスレッドの中で呼ぶ。
    pub fn start(tc: &mut Tc, units: &[UnitSrc<'_>]) -> R<(ReplWriter, kir::Program)> {
        let mut rw = ReplWriter::default();
        let mut kp = empty_program();
        let mut w = rw.writer(tc, &mut kp);
        w.builtins(&mut kp);
        let r = w.add_units(&mut kp, units);
        rw.restore(w, &mut kp);
        r?;
        Ok((rw, kp))
    }

    /// 単位 `u` を書き出して `kp` の後ろに追加する。
    ///
    /// `kp` は、このライタがそれまでに書き出したプログラムである。
    /// エラーのときは、追加しかけた項目を取り除き、表をこの呼び出しの前の状態に戻す。
    pub fn add(&mut self, tc: &mut Tc, kp: &mut kir::Program, u: UnitSrc<'_>) -> R<ReplOut> {
        let lens = ProgramMark::of(kp);
        let mark = self.mark();
        let mut w = self.writer(tc, kp);
        let r = w.add_units(kp, std::slice::from_ref(&u));
        let out = self.restore(w, kp);
        match r {
            Ok(()) => {
                self.commit(mark);
                Ok(out)
            }
            Err(e) => {
                lens.truncate(kp);
                self.rollback(mark);
                Err(e)
            }
        }
    }

    /// 名前で引く表にマークを置く。
    ///
    /// 表はコピーせず、マークの後の書き換えを記録する。そのため、処理時間は表のサイズによらない。
    /// マークは入れ子にできる。
    pub fn mark(&mut self) -> WriterMark {
        each_marked!(self.tb, begin);
        WriterMark(())
    }

    /// マーク `m` を外し、マークの後の書き換えを残す。入力を受理したときに呼ぶ。
    pub fn commit(&mut self, m: WriterMark) {
        let WriterMark(()) = m;
        each_marked!(self.tb, commit);
    }

    /// 名前で引く表をマーク `m` の時点に戻す。
    /// 受理しなかった入力の型、エフェクト、クラス、インスタンスは、以後の入力から見えなくなる。
    pub fn rollback(&mut self, m: WriterMark) {
        let WriterMark(()) = m;
        each_marked!(self.tb, rollback);
        for v in &mut self.tb.new_syms {
            v.clear();
        }
    }

    fn writer<'t>(&mut self, tc: &'t mut Tc, kp: &mut kir::Program) -> W<'t> {
        W {
            tc,
            tb: std::mem::take(&mut self.tb),
            nodes: std::mem::take(&mut kp.nodes),
            root: Root::default(),
            provisional: 0,
            final_cap: HashMap::default(),
            functions: Vec::new(),
            fn_first: std::mem::take(&mut self.fn_first),
            repl_print: self.repl_print,
            repl_out: ReplOut::default(),
            gbase: 0,
            ibase: 0,
            fbase: 0,
            index: [
                std::mem::take(&mut self.type_index),
                std::mem::take(&mut self.effect_index),
                std::mem::take(&mut self.class_index),
            ],
        }
    }

    fn restore(&mut self, w: W<'_>, kp: &mut kir::Program) -> ReplOut {
        let W {
            tb,
            nodes,
            fn_first,
            repl_print,
            repl_out,
            index: [ti, ei, ci],
            ..
        } = w;
        self.tb = tb;
        kp.nodes = nodes;
        self.fn_first = fn_first;
        self.repl_print = repl_print;
        self.type_index = ti;
        self.effect_index = ei;
        self.class_index = ci;
        repl_out
    }
}

impl<'t> W<'t> {
    // ---- シンボル ----

    fn type_id(&self, c: Oid) -> R<TypeId> {
        match self.tb.type_ids.get(&c) {
            Some(i) => Ok(*i),
            None => bug(format!(
                "KIR の書き出し: シンボルがありません: 型 {}",
                self.tc.name_of(c)
            )),
        }
    }

    fn effect_id(&self, e: Oid) -> R<EffectId> {
        match self.tb.effect_ids.get(&e) {
            Some(i) => Ok(*i),
            None => bug(format!(
                "KIR の書き出し: シンボルがありません: エフェクト {}",
                self.tc.name_of(e)
            )),
        }
    }

    fn class_id(&self, c: Oid) -> R<ClassId> {
        match self.tb.class_ids.get(&c) {
            Some(i) => Ok(*i),
            None => bug(format!(
                "KIR の書き出し: シンボルがありません: クラス {}",
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

    /// 束縛の名前のパターンの Rep を返す。パターンに型がなければ、束縛のノードの型を使う。
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

    /// 辞書パラメータのキーを返す。インスタンスのメソッドでは、実装の型の辞書パラメータを使う。
    fn dict_params_of(&self, id: NodeId) -> Vec<DKey> {
        match self.tc.dict.get(&id) {
            Some(DictInfo::Abs(ks)) | Some(DictInfo::Method(ks, _, _)) => ks.clone(),
            _ => Vec::new(),
        }
    }

    // ---- 書き出し中の関数 ----

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

    /// 書き出し中の関数 `fr` から束縛 `b` を読む方法を返す。
    /// `b` が `fr` より外側の関数の束縛なら、間にある関数にキャプチャを追加する。
    fn access(&mut self, fr: usize, b: Binding) -> R<Cap> {
        if fr == b.frame {
            return Ok(b.var);
        }
        let Some(p) = self.root.frames[fr].parent else {
            return bug(format!(
                "KIR の書き出し: ルート関数の外の変数を読もうとしました: {}",
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

    /// ローカルかキャプチャを読む式を作る。キャプチャには仮の番号を書く。
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
                                return bug("KIR の書き出し: PCtor の引数の位置が範囲外です");
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

    /// 関数の中のパターンを変換する。変数ごとにスロットを振り、名前を環境に追加する。
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

    /// クラスのメソッドを参照する式を作る。
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
                            return bug(format!(
                                "KIR の書き出し: シンボルがありません: グローバル {x}"
                            ));
                        };
                        let v = self.push(kir::Exp::Global(g));
                        return self.with_dicts(env, id, v);
                    }
                    Some(Resolved::Var(GRef::Builtin(x))) => {
                        let Some(g) = self.tb.builtin_globals.get(&x).copied() else {
                            return bug(format!(
                                "KIR の書き出し: グローバルが表にありません: builtin/{x}"
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
                            return bug("KIR の書き出し: Construct の引数の位置が範囲外です");
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

    /// 本体を `Print` のハンドラ式で囲む([`ReplUnit::print`])。
    ///
    /// `print(s)` の操作節は、プレリュードの `with_stdout` と同じく `resume(perform Console.write(s))` である。
    /// この `resume` は末尾位置にあるので、操作節の分類は T になる。
    /// ハンドラ式は、本体を書く前に生成箇所の番号を取り、本体、操作節の順に関数を作る。
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

    /// 束縛の値を書く(Diktor の `binding_value`)。
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

    /// `let rec` のグループを書く。グループのすべての名前に先にスロットを振ってから、各束縛を書く。
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

    // ---- 持ち上げた関数の生成箇所(§18.10) ----

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

    /// 仮引数のパターンを変換し、本体を書く。
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

    /// 辞書を受け取る関数の生成箇所を書く。
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

    /// ハンドラ式の生成箇所を書く。
    /// 本体、操作節(ソースに書いた順)、`return` 節、`cancel` 節の順に関数を作る。
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
            return bug("KIR の書き出し: 操作節のない handle です");
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

    // ---- キャプチャの番号の確定とルート関数(§18.11) ----

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
                return bug("KIR の書き出し: ルート関数がキャプチャを持っています");
            }
            return Ok(());
        };
        let mut resolved = Vec::with_capacity(self.root.frames[fr].caps.len());
        for (id, v) in &self.root.frames[fr].caps {
            let v = match v {
                Cap::Env(t) => match self.final_cap.get(t) {
                    Some(i) => Cap::Env(*i),
                    None => return bug("KIR の書き出し: キャプチャの仮の番号がありません"),
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
                            None => return bug("KIR の書き出し: スロットの番号が範囲外です"),
                        }
                    }
                }
                Cap::DParam(_) => Rep::Any,
                Cap::Env(j) => match pf.cap_reps.get(*j as usize) {
                    Some(r) => *r,
                    None => return bug("KIR の書き出し: キャプチャの番号が範囲外です"),
                },
            });
        }
        let f = &mut self.root.frames[fr];
        f.final_caps = resolved.into_iter().map(|(_, v)| v).collect();
        f.cap_reps = reps;
        Ok(())
    }

    /// ルート関数を 1 つ書き出し、そこから持ち上げた関数とともに関数表に追加する。
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
        // キャプチャの並びを、関数を作った順(親の関数が先)に決める。
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
        // ルート関数に続けて、持ち上げた関数を生成箇所の番号の順に並べる。
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
            let id = FnId(w.fbase + w.functions.len() as u32);
            w.fn_first.entry(sym.clone()).or_insert(id);
            w.functions.push((f, sym));
        };
        push_fn(self, rootf, job.sym);
        for (_, f, pd) in lifted {
            let sym = root.frames[f].sym.clone();
            push_fn(self, mk(&root.frames[f], pd), sym);
        }
        // `Part` の関数はシンボルで引く。同じシンボルの関数が前にあれば、Diktor の符号化と同じく、その関数を指す。
        let fn_first = &self.fn_first;
        let fix = |p: &mut Part| -> R<()> {
            let f = &root.frames[p.func.index()];
            match fn_first.get(&f.sym) {
                Some(id) => p.func = *id,
                None => {
                    return bug(format!(
                        "KIR の書き出し: 生成箇所の関数がありません: {}",
                        f.sym
                    ));
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
                _ => return bug("KIR の書き出し: 生成箇所の式ではありません"),
            }
        }
        Ok(())
    }

    // ---- 宣言(§18.12) ----

    /// インスタンスのクラスと、対象の型の型構成子を返す。
    /// 名前は、宣言を書いた module と単位のスコープで解決する。
    fn instance_head(&mut self, d: &Decl, i: &crate::ast::InstanceDecl) -> R<(Oid, Oid)> {
        self.tc.with_decl_scope(d.meta.id, |tc| {
            let co = tc.intern(&i.class);
            let cls = tc.resolve_class(co);
            let comps = match i.args.as_slice() {
                [t] => match &t.kind {
                    TypeExpKind::Ident(li) => li.0.clone(),
                    TypeExpKind::Apply(h, _) => match &h.kind {
                        TypeExpKind::Ident(li) => li.0.clone(),
                        _ => return bug("KIR の書き出し: インスタンスの対象の型を解決できません"),
                    },
                    _ => return bug("KIR の書き出し: インスタンスの対象の型を解決できません"),
                },
                _ => return bug("KIR の書き出し: インスタンスの対象の型を解決できません"),
            };
            let o = tc.intern(&comps.join("."));
            Ok((cls, tc.resolve_con(o)))
        })
    }

    /// 組み込みの型、クラス、値と、組み込みの単位を `kp` に置く。空のプログラムに 1 回だけ行う。
    fn builtins(&mut self, kp: &mut kir::Program) {
        use kir::builtins;
        for t in builtins::TYPES {
            let o = self.tc.intern(t);
            let s = format!("builtin/{t}");
            self.tb.set_sym(0, o, s.clone());
            self.index[0].insert(s.clone(), kp.types.len() as u32);
            kp.types.push(kir::Type {
                sym: s,
                ctors: Vec::new(),
            });
        }
        for (c, ms) in builtins::CLASSES {
            let o = self.tc.intern(c);
            let s = format!("builtin/{c}");
            self.tb.set_sym(2, o, s.clone());
            let id = kp.classes.len() as u32;
            self.index[2].insert(s.clone(), id);
            for (j, m) in ms.iter().enumerate() {
                self.tb
                    .meth_idx
                    .entry((id, m.to_string()))
                    .or_insert(j as u32);
            }
            kp.classes.push(kir::Class {
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
        for v in builtins::VALUES {
            self.tb
                .builtin_globals
                .insert(v.to_string(), GlobalId(kp.globals.len() as u32));
            kp.globals.push(kir::Global {
                sym: format!("builtin/{v}"),
                kind: GlobalKind::Builtin,
            });
        }
        kp.units.push(kir::Unit {
            key: "builtin".to_string(),
            paths: Vec::new(),
            init: Vec::new(),
        });
    }

    /// 単位の並び `units` を書き出し、`kp` の各表の後ろに追加する(Diktor の `program`)。
    ///
    /// [`program`] が 1 回で書き出すプログラムは、空のプログラムに組み込みの項目を置き([`W::builtins`])、すべての単位を 1 回の呼び出しで追加したものである。
    fn add_units(&mut self, kp: &mut kir::Program, units: &[UnitSrc<'_>]) -> R<()> {
        let (t0, e0, c0) = (kp.types.len(), kp.effects.len(), kp.classes.len());
        // 第 1 段階の前半:型、エフェクト、クラス。
        for u in units {
            self.collect_types(u, &mut kp.types, &mut kp.effects, &mut kp.classes)?;
        }
        {
            let [ti, ei, ci] = &mut self.index;
            for (i, t) in kp.types.iter().enumerate().skip(t0) {
                ti.entry(t.sym.clone()).or_insert(i as u32);
            }
            for (i, t) in kp.effects.iter().enumerate().skip(e0) {
                ei.entry(t.sym.clone()).or_insert(i as u32);
            }
            for (i, t) in kp.classes.iter().enumerate().skip(c0) {
                ci.entry(t.sym.clone()).or_insert(i as u32);
            }
            // 添字を決めるのは、この呼び出しまでにシンボルを置いて、まだ添字を決めていない名前だけである。
            // それまでの単位の名前の添字は変わらない。
            let [nt, ne, nc] = std::mem::take(&mut self.tb.new_syms);
            for o in nt {
                if let Some(s) = self.tb.type_syms.get(&o) {
                    let id = TypeId(ti[s.as_str()]);
                    self.tb.type_ids.insert(o, id);
                }
            }
            for t in &kp.types[t0..] {
                let id = ti[t.sym.as_str()];
                for (j, c) in t.ctors.iter().enumerate() {
                    self.tb
                        .ctor_idx
                        .entry((id, c.name.clone()))
                        .or_insert(j as u32);
                }
            }
            for o in ne {
                if let Some(s) = self.tb.effect_syms.get(&o) {
                    let id = EffectId(ei[s.as_str()]);
                    self.tb.effect_ids.insert(o, id);
                }
            }
            for t in &kp.effects[e0..] {
                let id = ei[t.sym.as_str()];
                for (j, c) in t.ops.iter().enumerate() {
                    self.tb
                        .op_idx
                        .entry((id, c.name.clone()))
                        .or_insert(j as u32);
                }
            }
            for o in nc {
                if let Some(s) = self.tb.class_syms.get(&o) {
                    let id = ClassId(ci[s.as_str()]);
                    self.tb.class_ids.insert(o, id);
                }
            }
            for t in &kp.classes[c0..] {
                let id = ci[t.sym.as_str()];
                for (j, c) in t.methods.iter().enumerate() {
                    self.tb
                        .meth_idx
                        .entry((id, c.name.clone()))
                        .or_insert(j as u32);
                }
            }
        }
        // 第 1 段階の後半:グローバル、インスタンス、初期化手順。
        self.gbase = kp.globals.len() as u32;
        self.ibase = kp.instances.len() as u32;
        let mut globals: Vec<(String, PendingGlobal)> = Vec::new();
        let mut instances: Vec<PendingInstance> = Vec::new();
        let mut unit_steps: Vec<Vec<PendingStep>> = Vec::new();
        let mut jobs: Vec<Job<'_>> = Vec::new();
        for u in units {
            let steps = self.collect_unit(u, &mut globals, &mut instances, &mut jobs)?;
            unit_steps.push(steps);
        }
        if self.repl_print.is_none() && units.iter().any(|u| u.repl.is_some_and(|r| r.print)) {
            // 引数 1 個の操作 `print` を持つ `Print` と、ランタイムが処理する `Console.write` があるときだけ囲む。
            let effects = &kp.effects;
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
        // 第 2 段階:ルート関数。
        self.fbase = kp.functions.len() as u32;
        for job in jobs {
            self.root_fn(job)?;
        }
        // 関数をシンボルで引いて、添字に直す。
        let functions = std::mem::take(&mut self.functions);
        let fi = &self.fn_first;
        let fn_id = |s: &str| -> R<FnId> {
            match fi.get(s) {
                Some(i) => Ok(*i),
                None => bug(format!("KIR の書き出し: 関数が表にありません: {s}")),
            }
        };
        let mut gs = Vec::with_capacity(globals.len());
        for (sym, k) in &globals {
            let kind = match k {
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
        let mut us = Vec::with_capacity(units.len());
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
        kp.globals.extend(gs);
        kp.instances.extend(is);
        kp.units.extend(us);
        kp.functions.extend(functions.into_iter().map(|(mut f, s)| {
            f.sym = s;
            f
        }));
        Ok(())
    }

    /// 単位の型、エフェクト、クラスの表を作る(第 1 段階の前半)。
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
                    self.tb.set_sym(0, o, s.clone());
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
                    self.tb.set_sym(1, eoid, s.clone());
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
                    self.tb.set_sym(2, coid, s.clone());
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

    /// 単位のグローバル、インスタンス、初期化手順を決める(第 1 段階の後半)。
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
            let id = GlobalId(w.gbase + globals.len() as u32);
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
        let gbase = self.gbase as usize;
        let gsym = |globals: &Vec<(String, PendingGlobal)>, g: GlobalId| {
            globals[g.index() - gbase].0.clone()
        };
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
                    // REPL で結果を表示するため、式文の値を内部用のグローバルに束縛する。
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
                    let iid = InstanceId(self.ibase + instances.len() as u32);
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
            // REPL の表示に使う、`Show.show` を呼ぶ関数。
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

    /// 関数定義のルート関数を書き出すジョブを作る(Diktor の `fn_def_root`)。
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

/// 種類 `thunk` のルート関数を書き出すジョブを作る。
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

/// 呼び出しと `perform` の引数を取り出す。
/// 引数は `_item` のレコードの式になっていて、最も外側の要素が先頭の引数である。
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

/// OCaml の `float_of_string` と同じ値を返す。
///
/// OCaml は、桁区切りの `_` を取り除いたうえで、符号の後ろが `0x` か `0X` なら 16 進数として読む(OCaml のランタイムの `caml_float_of_hex`)。
/// それ以外は C の `strtod` で読む。
/// どちらの場合も、全体を読み切れなければ失敗する。
/// 本関数は、10 進数を Rust の `parse` で読む。
/// `strtod` も `parse` も正しく丸めるので、同じ値になる。
/// 入力は、字句解析で数値リテラルになる表記(数字、`.`、`e`、`E`、`+`、`-`、`x`、`o`、`b` と 16 進の数字)だけを想定する。
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

/// OCaml のランタイムの `caml_float_of_hex` と同じ計算をする。指数部(`p`)は扱わない。
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
    // `ldexp` に当たる。2 のべき乗を掛ける演算は、オーバーフローとアンダーフローのとき以外は正確である。
    while exp > 1000 {
        f *= 2f64.powi(1000);
        exp -= 1000;
    }
    while exp < -1000 {
        // 最後に掛ける `2f64.powi(exp)` が正規化数になるよう、先に 2 の -1000 乗を繰り返し掛けて `f` を小さくし、`exp` を -1000 以上にする。
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
        // 60 ビットを超える 16 進数は、あふれた桁に 0 でない桁があれば最下位ビットを 1 にしてから(奇数への丸め)、double に変換する。
        assert_eq!(
            ocaml_float_of_string("0x1000000000000001"),
            Some(1152921504606846976.0)
        );
    }
}
