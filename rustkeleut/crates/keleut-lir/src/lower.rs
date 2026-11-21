//! KIR から LIR への変換(パス 1〜10)。
//!
//! KIR の木を 1 回走査して、基本ブロックのグラフにする。
//! パスの番号は `rustkeleut/doc/lir.md` の 2 節の表の番号である。
//! 各パスの処理内容と実装箇所は次のとおりである。
//!
//! | パス | 処理内容 | 実装箇所 |
//! |---|---|---|
//! | 1 環境の配置 | 引数、辞書の引数、キャプチャ、スロットを、レジスタと環境の中の位置に対応させる | 本ファイルの [`Low::entry`]、`env`、`closure` |
//! | 2 パターンの照合 | パターンを、後戻りを伴う照合の命令にする | [`crate::pattern`] |
//! | 3 A 正規化 | 子の式を評価の順に命令にする | 本ファイルの、各構文を変換する関数 |
//! | 4 Rep とボックス化 | Rep が違う箇所に `box` と `unbox` を置き、関数の境界では値を `any` にする | 本ファイルの [`Low::coerce`] |
//! | 5 辞書の定数化 | `dbuiltin` と `dinst` の辞書のメソッドの呼び出しを、プリミティブと直接呼び出しにする | [`crate::dict`] |
//! | 6 レコードのシェイプ | シェイプの分かるレコードの操作を、フィールドの位置を定数で指定する命令にする | [`crate::record`] |
//! | 7 数値の検査 | プリミティブの命令の前に検査を置く | [`crate::numeric`] |
//! | 8 エフェクト | `handle`、ユーザの操作の `perform`、`resume`、操作節の関数の入口を変換する | [`crate::effect`] |
//! | 9 初期化の検査の省略 | グローバルを読む前の検査を置くかどうかを決める | [`crate::initck`] と本ファイルの [`Low::global_check`] |
//! | 10 深さの命令 | 入口の `depth_enter`、呼び出しの前後の `depth_push` と `depth_restore`、`depth_check` を置く | [`crate::depth`] |
//!
//! パス 11(yield の枝刈り)とパス 12(生存区間とスロット)は、変換した後のグラフに対して [`crate::frame`] が行う。

use std::collections::HashMap;

use kir::{Cap, Exp, ExpId, FnId, GlobalKind as KGlobalKind, Lit, Part};

use crate::builder::Builder;
use crate::initck;
use crate::ir::*;
use crate::numeric::{self, Prim};

/// 値を持つレジスタと、その Rep の組。
#[derive(Clone, Copy, Debug)]
pub(crate) struct V {
    pub reg: Reg,
    pub rep: Rep,
}

impl V {
    pub(crate) fn any(reg: Reg) -> V {
        V { reg, rep: Rep::Any }
    }
}

/// LIR への変換の間、プログラム全体で共有する状態。
///
/// ラベル、シェイプ、文字列の表と、パス 8(操作節の分類)とパス 9(初期化の検査の省略)の解析の結果と、KIR の関数から LIR の関数への添字の対応を持つ。
pub(crate) struct Prog<'k> {
    pub k: &'k kir::Program,
    pub labels: Vec<String>,
    label_ix: HashMap<String, LabelId>,
    pub shapes: Vec<Vec<LabelId>>,
    shape_ix: HashMap<Vec<LabelId>, ShapeId>,
    pub strings: Vec<Vec<u8>>,
    string_ix: HashMap<Vec<u8>, StrId>,
    pub init: initck::Info,
    /// グローバルごとの、本体の関数。
    ///
    /// 本体の関数を持つのは、組み込みの値と、実装名がプリミティブの表にある `extern` だけである。
    pub global_func: Vec<Option<FuncId>>,
    /// `Array.each` の内側のループの関数。
    pub each_loop: Option<FuncId>,
    /// KIR の関数ごとの、操作の番号と操作節の分類の組(パス 8)。
    /// 操作節でない関数は `None` である。
    pub clause_class: Vec<Option<(u32, kir::ClauseClass)>>,
    /// ハンドラのフレームになる関数 `$rt_handle`(パス 8)。
    pub rt_handle: Option<FuncId>,
    /// KIR の関数の添字から LIR の関数の添字への対応。
    ///
    /// 1 回で変換したプログラムでは、両者は同じ添字である。
    /// 単位を追加したプログラム([`crate::Extender`])では、追加した単位の KIR の関数は、それまでの LIR の関数の後ろに続く。
    pub fn_map: Vec<FuncId>,
    /// 変換の途中で見つけた内部エラーのメッセージ。
    /// 最初の 1 件だけを持つ。
    pub bug: Option<String>,
}

/// [`Prog`] のうち、KIR のプログラムを参照しない部分。
///
/// 単位を追加していく変換では、前回の変換から次回の変換へ引き継ぐ。
#[derive(Default)]
pub(crate) struct ProgParts {
    pub labels: Vec<String>,
    label_ix: HashMap<String, LabelId>,
    pub shapes: Vec<Vec<LabelId>>,
    shape_ix: HashMap<Vec<LabelId>, ShapeId>,
    /// `shape_ix` に登録済みのシェイプの個数。
    ///
    /// 呼び出し元が `shapes` の後ろに追加したシェイプは、まだ登録していない。
    shapes_indexed: usize,
    pub strings: Vec<Vec<u8>>,
    string_ix: HashMap<Vec<u8>, StrId>,
    pub init: initck::Info,
    pub global_func: Vec<Option<FuncId>>,
    pub each_loop: Option<FuncId>,
    pub clause_class: Vec<Option<(u32, kir::ClauseClass)>>,
    pub rt_handle: Option<FuncId>,
    pub fn_map: Vec<FuncId>,
}

impl<'k> Prog<'k> {
    /// 引き継いだ部分 `parts` を使って、KIR のプログラム `k` の変換を始める。
    ///
    /// シェイプ表が空なら、空のシェイプを 0 番に登録する。
    /// 呼び出し元が `parts.shapes` の後ろに追加したシェイプ(ランタイムが作ったシェイプ)は、ここで `shape_ix` に登録する。
    pub(crate) fn resume(k: &'k kir::Program, parts: ProgParts) -> Prog<'k> {
        let ProgParts {
            labels,
            label_ix,
            shapes,
            mut shape_ix,
            shapes_indexed,
            strings,
            string_ix,
            init,
            global_func,
            each_loop,
            clause_class,
            rt_handle,
            fn_map,
        } = parts;
        for (i, s) in shapes.iter().enumerate().skip(shapes_indexed) {
            shape_ix.entry(s.clone()).or_insert(ShapeId(i as u32));
        }
        let mut p = Prog {
            k,
            labels,
            label_ix,
            shapes,
            shape_ix,
            strings,
            string_ix,
            init,
            global_func,
            each_loop,
            clause_class,
            rt_handle,
            fn_map,
            bug: None,
        };
        if p.shapes.is_empty() {
            p.shape(Vec::new());
        }
        p
    }

    /// 次回の変換へ引き継ぐ部分を取り出す。
    pub(crate) fn into_parts(self) -> ProgParts {
        ProgParts {
            shapes_indexed: self.shapes.len(),
            labels: self.labels,
            label_ix: self.label_ix,
            shapes: self.shapes,
            shape_ix: self.shape_ix,
            strings: self.strings,
            string_ix: self.string_ix,
            init: self.init,
            global_func: self.global_func,
            each_loop: self.each_loop,
            clause_class: self.clause_class,
            rt_handle: self.rt_handle,
            fn_map: self.fn_map,
        }
    }

    /// KIR の関数 `f` に対応する LIR の関数の添字を返す。
    /// `fn_map` に対応がなければ、KIR と同じ添字を返す。
    pub(crate) fn func(&self, f: FnId) -> FuncId {
        match self.fn_map.get(f.index()) {
            Some(&g) => g,
            None => FuncId(f.0),
        }
    }

    /// ラベルを表に登録して、その番号を返す。
    /// 登録済みなら同じ番号を返す。
    pub(crate) fn label(&mut self, l: &str) -> LabelId {
        if let Some(&id) = self.label_ix.get(l) {
            return id;
        }
        let id = LabelId(self.labels.len() as u32);
        self.labels.push(l.to_string());
        self.label_ix.insert(l.to_string(), id);
        id
    }

    /// シェイプ(正準順に並べたラベルの並び)を表に登録して、その番号を返す。
    /// 登録済みなら同じ番号を返す。
    pub(crate) fn shape(&mut self, labels: Vec<LabelId>) -> ShapeId {
        if let Some(&id) = self.shape_ix.get(&labels) {
            return id;
        }
        let id = ShapeId(self.shapes.len() as u32);
        self.shapes.push(labels.clone());
        self.shape_ix.insert(labels, id);
        id
    }

    /// 文字列定数を表に登録して、その番号を返す。
    /// 登録済みなら同じ番号を返す。
    pub(crate) fn string(&mut self, s: &[u8]) -> StrId {
        if let Some(&id) = self.string_ix.get(s) {
            return id;
        }
        let id = StrId(self.strings.len() as u32);
        self.strings.push(s.to_vec());
        self.string_ix.insert(s.to_vec(), id);
        id
    }

    /// 内部エラーのメッセージを記録する。
    /// 記録するのは最初の 1 件だけである。
    pub(crate) fn note_bug(&mut self, msg: String) {
        if self.bug.is_none() {
            self.bug = Some(msg);
        }
    }
}

/// シェイプの分かるレコードのレイアウト。
///
/// (ラベル, フィールドの値のレジスタ)の組を正準順に並べたものである。
pub(crate) type Layout = Vec<(LabelId, Reg)>;

/// 1 つの関数を LIR に変換している間の状態。
pub(crate) struct Low<'p, 'k> {
    pub p: &'p mut Prog<'k>,
    /// 変換している KIR の関数。
    /// 単位の初期化関数を作るときは `None` である。
    pub f: Option<&'k kir::Function>,
    pub fid: Option<FnId>,
    pub b: Builder,
    /// KIR のスロットごとの、値を持つレジスタ。
    /// まだ束縛していないスロットは `None` である。
    pub slots: Vec<Option<Reg>>,
    /// KIR のスロットごとの Rep。
    pub slot_rep: Vec<Rep>,
    /// 辞書の引数のレジスタ。
    pub dparams: Vec<Reg>,
    /// 入口で読んだキャプチャのレジスタ(`any`)。
    pub envs: Vec<Reg>,
    /// シェイプの分かるレコードのレジスタと、そのレイアウト(パス 6)。
    pub known: HashMap<Reg, Layout>,
    /// 操作節の関数を変換しているときの、その節の分類(パス 8)。
    ///
    /// 末尾の値と末尾 `resume` を何に変換するかを決める。
    pub clause: Option<kir::ClauseClass>,
    /// この関数の中の `resume` が再開する継続のレジスタ(パス 8)。
    ///
    /// 分類 G の節の本体では、暗黙の引数として受け取った継続である。
    /// ハンドラ式の本体では、暗黙の引数として受け取った、引き継ぐ継続である。
    pub resumption: Option<Reg>,
}

impl<'p, 'k> Low<'p, 'k> {
    pub(crate) fn new(p: &'p mut Prog<'k>, f: Option<(FnId, &'k kir::Function)>) -> Low<'p, 'k> {
        let (fid, f) = match f {
            Some((i, f)) => (Some(i), Some(f)),
            None => (None, None),
        };
        let slot_rep: Vec<Rep> = match f {
            Some(f) => f.params.iter().chain(f.locals.iter()).copied().collect(),
            None => Vec::new(),
        };
        Low {
            p,
            f,
            fid,
            b: Builder::new(),
            slots: vec![None; slot_rep.len()],
            slot_rep,
            dparams: Vec::new(),
            envs: Vec::new(),
            known: HashMap::new(),
            clause: None,
            resumption: None,
        }
    }

    fn k(&self) -> &'k kir::Program {
        self.p.k
    }

    /// 内部エラーのメッセージを記録し、現在のブロックがあれば `unimplemented` で終える。
    /// 常に `None` を返す。
    pub(crate) fn bug(&mut self, msg: impl Into<String>) -> Option<V> {
        let msg = msg.into();
        self.p.note_bug(msg.clone());
        if self.b.current().is_some() {
            self.b.terminate(Term::Unimplemented {
                what: format!("内部の誤り: {msg}"),
            });
        }
        None
    }

    // ---- パス 4: Rep とボックス化 ----

    /// 値 `v` を Rep `to` のレジスタに変換する。
    /// Rep が違えば `box` と `unbox` のどちらか、または両方を生成する。
    pub(crate) fn coerce(&mut self, v: V, to: Rep) -> Reg {
        if v.rep == to {
            return v.reg;
        }
        if to == Rep::Any {
            let dst = self.b.reg(Rep::Any);
            self.b.emit(Inst::Box { dst, src: v.reg });
            return dst;
        }
        let src = if v.rep == Rep::Any {
            v.reg
        } else {
            // `any` 以外の Rep どうしが食い違うことは、型が健全なら起きない。
            // 起きたときは、一様表現の値を経由して変換する。
            let a = self.b.reg(Rep::Any);
            self.b.emit(Inst::Box { dst: a, src: v.reg });
            a
        };
        let dst = self.b.reg(to);
        self.b.emit(Inst::Unbox { dst, src });
        dst
    }

    /// 値を一様表現の値(Rep `any`)に変換する。
    pub(crate) fn boxed(&mut self, v: V) -> Reg {
        self.coerce(v, Rep::Any)
    }

    /// 値を返して、現在のブロックを終える。
    ///
    /// 分類 T の節の本体では、末尾の値は `resume` しない節の値なので、`return` ではなく `clause_value` で終える(パス 8)。
    pub(crate) fn ret(&mut self, v: V) {
        let r = self.boxed(v);
        if self.clause == Some(kir::ClauseClass::T) {
            self.b.terminate(Term::Fx {
                op: FxTermOp::ClauseValue,
                args: vec![r],
            });
        } else {
            self.b.terminate(Term::Return(r));
        }
    }

    /// `tail` が真なら、[`Low::ret`] で値 `v` を返す終端命令を置いて `None` を返す。
    /// 偽なら、`v` をそのまま返す。
    pub(crate) fn finish(&mut self, v: Option<V>, tail: bool) -> Option<V> {
        match (v, tail) {
            (Some(v), true) => {
                self.ret(v);
                None
            }
            (v, false) => v,
            (None, true) => None,
        }
    }

    /// 分岐したそれぞれの経路の値を、1 つのレジスタに合流させる。
    ///
    /// `results` は(経路の最後のブロック, 値)の組の並びで、どのブロックにもまだ終端命令を置いていない。
    /// 値の Rep がすべて同じならその Rep で、そうでなければ `any` で合流させる。
    pub(crate) fn join(&mut self, results: Vec<(BlockId, V)>) -> Option<V> {
        if results.is_empty() {
            return None;
        }
        let rep0 = results[0].1.rep;
        let rep = if results.iter().all(|(_, v)| v.rep == rep0) {
            rep0
        } else {
            Rep::Any
        };
        let join = self.b.block();
        let dst = self.b.reg(rep);
        for (blk, v) in results {
            self.b.switch_to(blk);
            let src = self.coerce(v, rep);
            self.b.emit(Inst::Move { dst, src });
            self.b.terminate(Term::Jump(join));
        }
        self.b.switch_to(join);
        Some(V { reg: dst, rep })
    }

    // ---- パス 1: 環境の配置 ----

    /// 関数の入口を作る。
    /// 深さの命令を置き、キャプチャ、引数、辞書の引数をレジスタに読む。
    ///
    /// キャプチャはすべて、`depth_enter` の直後に `env_get` でレジスタに読む。
    /// その後は、キャプチャもほかの値と同じようにスロットに置かれ、再開点と安全点の集合に入るので、フレームはクロージャ(環境)を保持し続けなくてよい。
    /// 使わなかったキャプチャの `env_get` は [`drop_unused_env`] が取り除く。
    pub(crate) fn entry(&mut self) {
        crate::depth::enter(&mut self.b);
        let f = self.f.expect("KIR の関数ではない");
        let n = f.params.len() + f.dicts as usize;
        let params: Vec<Reg> = (0..n).map(|_| self.b.param()).collect();
        for i in 0..f.caps.len() {
            let dst = self.b.reg(Rep::Any);
            self.b.emit(Inst::EnvGet {
                dst,
                index: i as u32,
            });
            self.envs.push(dst);
        }
        if f.dicts > 0 {
            self.dparams = params;
        } else {
            for (i, &p) in params.iter().enumerate() {
                let r = self.coerce(V::any(p), f.params[i]);
                self.slots[i] = Some(r);
            }
        }
    }

    /// 入口で読んだ `i` 番目のキャプチャのレジスタを返す。
    pub(crate) fn env_reg(&mut self, i: u32) -> Reg {
        match self.envs.get(i as usize) {
            Some(&r) => r,
            None => {
                self.p.note_bug(format!("範囲の外の捕獲 {i}"));
                self.b.konst(Const::Unit)
            }
        }
    }

    /// `i` 番目のキャプチャを、KIR の関数が宣言している Rep の値として読む。
    fn env(&mut self, i: u32) -> V {
        let dst = self.env_reg(i);
        let rep = self
            .f
            .and_then(|f| f.caps.get(i as usize).copied())
            .unwrap_or(Rep::Any);
        let reg = self.coerce(V::any(dst), rep);
        V { reg, rep }
    }

    /// `i` 番のスロットの値を読む。
    fn local(&mut self, i: u32) -> Option<V> {
        match self.slots.get(i as usize).copied().flatten() {
            Some(reg) => Some(V {
                reg,
                rep: self.slot_rep[i as usize],
            }),
            None => self.bug(format!("束縛の前のスロット {i} を読んだ")),
        }
    }

    /// `i` 番のスロットに値を置く。
    pub(crate) fn bind_slot(&mut self, i: u32, v: V) {
        let Some(&rep) = self.slot_rep.get(i as usize) else {
            self.bug(format!("範囲の外のスロット {i}"));
            return;
        };
        let r = self.coerce(v, rep);
        self.slots[i as usize] = Some(r);
    }

    /// キャプチャの並びの要素 `c` を、一様表現の値として読む。
    fn cap_any(&mut self, c: Cap) -> Option<Reg> {
        Some(match c {
            Cap::Local(i) => {
                let v = self.local(i)?;
                self.boxed(v)
            }
            Cap::Env(i) => self.env_reg(i),
            Cap::DParam(i) => match self.dparams.get(i as usize) {
                Some(&r) => r,
                None => {
                    self.bug(format!("範囲の外の辞書の引数 {i}"));
                    return None;
                }
            },
        })
    }

    fn closure(&mut self, part: &Part) -> Option<V> {
        let mut caps = Vec::with_capacity(part.caps.len());
        for &c in &part.caps {
            caps.push(self.cap_any(c)?);
        }
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::ClosureNew {
            dst,
            func: self.p.func(part.func),
            caps,
        });
        Some(V::any(dst))
    }

    // ---- パス 9: グローバルを読む ----

    /// グローバル `g` が初期化済みかどうかの検査を置く。
    /// 省いてよい箇所では何も置かない。
    pub(crate) fn global_check(&mut self, g: kir::GlobalId) {
        if self.p.init.may_elide(self.k(), self.fid, g) {
            return;
        }
        let ok = self.b.reg(Rep::Bool);
        self.b.emit(Inst::GlobalIsInit { dst: ok, global: g });
        let bad = self.b.reg(Rep::Bool);
        self.b.emit(Inst::BoolNot { dst: bad, a: ok });
        self.b.fail_if(bad, RtError::Unbound(g), None);
    }

    /// グローバルを値として読む。
    fn global(&mut self, g: kir::GlobalId) -> V {
        self.global_check(g);
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::GlobalGet { dst, global: g });
        match self.k().globals[g.index()].kind {
            KGlobalKind::Value(rep) if rep != Rep::Any => {
                let reg = self.coerce(V::any(dst), rep);
                V { reg, rep }
            }
            _ => V::any(dst),
        }
    }

    // ---- パス 3: 式 ----

    /// 式を LIR に変換する。
    ///
    /// `tail` が真なら、式の値が関数の返り値になる。
    /// このときは、`return` か末尾呼び出しで現在のブロックを終え、`None` を返す。
    /// 偽なら、値を返す。
    /// 必ず実行時エラーになる式でも、ブロックを終えて `None` を返す。
    pub(crate) fn exp(&mut self, e: ExpId, tail: bool) -> Option<V> {
        let k = self.k();
        let Some(x) = k.nodes.exp(e) else {
            return self.bug(format!("範囲の外の式 {}", e.0));
        };
        match x {
            Exp::Lit(l) => {
                let v = self.lit(l);
                self.finish(Some(v), tail)
            }
            Exp::LitError(text) => {
                self.b.terminate(Term::Error {
                    kind: RtError::LitRange(text.clone()),
                    operand: None,
                });
                None
            }
            Exp::Local(i) => {
                let v = self.local(*i);
                self.finish(v, tail)
            }
            Exp::Env(i) => {
                let v = self.env(*i);
                self.finish(Some(v), tail)
            }
            Exp::Global(g) => {
                let v = self.global(*g);
                self.finish(Some(v), tail)
            }
            Exp::Construct { ty, ctor, fields } => {
                let v = self.construct(*ty, *ctor, fields);
                self.finish(v, tail)
            }
            Exp::Variant { label, value } => {
                let v = self.variant(label, *value);
                self.finish(v, tail)
            }
            Exp::Call {
                tail: t,
                yields,
                func,
                args,
            } => self.call(*t && tail, *yields, *func, args, tail),
            Exp::Method {
                class,
                method,
                dict,
                own,
            } => {
                let v = self.method_value(*class, *method, *dict, own);
                self.finish(Some(v), tail)
            }
            Exp::Opcall {
                class,
                method,
                dict,
                args,
            } => {
                let v = self.opcall(*class, *method, *dict, args);
                self.finish(v, tail)
            }
            Exp::Not(a) => {
                let v = self.not(*a);
                self.finish(v, tail)
            }
            Exp::And(a, b2) => self.and_or(true, *a, *b2, tail),
            Exp::Or(a, b2) => self.and_or(false, *a, *b2, tail),
            Exp::Closure(part) | Exp::Dictabs(part) => {
                let v = self.closure(part);
                self.finish(v, tail)
            }
            Exp::Dictapp { func, dicts } => {
                let v = self.dictapp(*func, dicts);
                self.finish(v, tail)
            }
            Exp::Let { slot, value, body } => {
                let v = self.exp(*value, false)?;
                self.bind_slot(*slot, v);
                self.exp(*body, tail)
            }
            Exp::Letpat { pat, value, body } => {
                let v = self.exp(*value, false)?;
                self.letpat(*pat, v)?;
                self.exp(*body, tail)
            }
            Exp::Letrec { items, body } => {
                self.letrec(items)?;
                self.exp(*body, tail)
            }
            Exp::Seq(es) => {
                let Some((last, init)) = es.split_last() else {
                    return self.bug("空の seq");
                };
                for &e in init {
                    self.exp(e, false)?;
                }
                self.exp(*last, tail)
            }
            Exp::Match { scrutinee, clauses } => self.match_(*scrutinee, clauses, tail),
            Exp::RecEmpty => {
                let r = self.b.konst(Const::Unit);
                self.known.insert(r, Vec::new());
                self.finish(Some(V::any(r)), tail)
            }
            Exp::RecExtend { .. } => {
                let v = self.rec_extend(e);
                self.finish(v, tail)
            }
            Exp::RecSelect { label, record } => {
                let v = self.rec_select(label, *record);
                self.finish(v, tail)
            }
            Exp::RecRestrict { label, record } => {
                let v = self.rec_restrict(label, *record);
                self.finish(v, tail)
            }
            Exp::RecUpdate {
                label,
                record,
                value,
            } => {
                let v = self.rec_update(label, *record, *value);
                self.finish(v, tail)
            }
            Exp::Perform { effect, op, args } => {
                let v = self.perform(*effect, *op, args);
                self.finish(v, tail)
            }
            Exp::Handle(h) => {
                let v = self.handle(h, tail)?;
                self.finish(Some(v), tail)
            }
            Exp::Resume { tail: t, value } => self.resume(*t && tail, *value, tail),
            Exp::Hole => {
                self.b.terminate(Term::Error {
                    kind: RtError::Hole,
                    operand: None,
                });
                None
            }
        }
    }

    fn lit(&mut self, l: &Lit) -> V {
        let c = match l {
            Lit::Bool(b) => Const::Bool(*b),
            Lit::I32(n) => Const::I32(*n),
            Lit::I64(n) => Const::I64(*n),
            Lit::F64(bits) => Const::F64(*bits),
            Lit::Str(s) => Const::Str(self.p.string(s)),
        };
        let rep = c.rep();
        V {
            reg: self.b.konst(c),
            rep,
        }
    }

    /// 引数を左から順に評価し、それぞれを一様表現の値にする。
    pub(crate) fn args_any(&mut self, args: &[ExpId]) -> Option<Vec<Reg>> {
        let vs = self.args(args)?;
        Some(vs.into_iter().map(|v| self.boxed(v)).collect())
    }

    /// 引数を左から順に評価する。
    pub(crate) fn args(&mut self, args: &[ExpId]) -> Option<Vec<V>> {
        let mut out = Vec::with_capacity(args.len());
        for &a in args {
            out.push(self.exp(a, false)?);
        }
        Some(out)
    }

    #[inline(never)]
    fn construct(&mut self, ty: kir::TypeId, ctor: u32, fields: &[(u32, ExpId)]) -> Option<V> {
        let arity = self
            .k()
            .types
            .get(ty.index())
            .and_then(|t| t.ctors.get(ctor as usize))
            .map(|c| c.arity as usize);
        let Some(arity) = arity else {
            return self.bug("範囲の外のコンストラクタ");
        };
        let mut vals: Vec<Option<Reg>> = vec![None; arity];
        for &(pos, e) in fields {
            let v = self.exp(e, false)?;
            let r = self.boxed(v);
            match vals.get_mut(pos as usize) {
                Some(slot) => *slot = Some(r),
                None => return self.bug("範囲の外のフィールド"),
            }
        }
        if arity == 0 {
            let r = self.b.konst(Const::Ctor0 { ty, ctor });
            return Some(V::any(r));
        }
        let mut regs = Vec::with_capacity(arity);
        for v in vals {
            regs.push(match v {
                Some(r) => r,
                // 書かなかったフィールドは Unit にする(Diktor の ir_eval と同じ)。
                None => self.b.konst(Const::Unit),
            });
        }
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::CtorNew {
            dst,
            ty,
            ctor,
            fields: regs,
        });
        Some(V::any(dst))
    }

    #[inline(never)]
    fn variant(&mut self, label: &str, value: ExpId) -> Option<V> {
        let v = self.exp(value, false)?;
        let payload = self.boxed(v);
        let label = self.p.label(label);
        let dst = self.b.reg(Rep::Any);
        self.b.emit(Inst::VariantNew {
            dst,
            label,
            payload,
        });
        Some(V::any(dst))
    }

    #[inline(never)]
    fn not(&mut self, a: ExpId) -> Option<V> {
        let v = self.exp(a, false)?;
        let x = self.coerce(v, Rep::Bool);
        let dst = self.b.reg(Rep::Bool);
        self.b.emit(Inst::BoolNot { dst, a: x });
        Some(V {
            reg: dst,
            rep: Rep::Bool,
        })
    }

    /// `and`(`is_and` が真のとき)と `or` を、短絡評価の分岐に変換する。
    #[inline(never)]
    fn and_or(&mut self, is_and: bool, a: ExpId, rhs: ExpId, tail: bool) -> Option<V> {
        let v = self.exp(a, false)?;
        let c = self.coerce(v, Rep::Bool);
        let eval_b = self.b.block();
        let short = self.b.block();
        let (then, else_) = if is_and {
            (eval_b, short)
        } else {
            (short, eval_b)
        };
        self.b.terminate(Term::Branch {
            cond: c,
            then,
            else_,
        });
        let mut results = Vec::new();
        self.b.switch_to(eval_b);
        if let Some(y) = self.exp(rhs, tail) {
            let r = self.coerce(y, Rep::Bool);
            results.push((
                self.b.current().unwrap(),
                V {
                    reg: r,
                    rep: Rep::Bool,
                },
            ));
        }
        self.b.switch_to(short);
        let k = self.b.konst(Const::Bool(!is_and));
        let kv = V {
            reg: k,
            rep: Rep::Bool,
        };
        if tail {
            self.ret(kv);
            return None;
        }
        results.push((self.b.current().unwrap(), kv));
        self.join(results)
    }

    #[inline(never)]
    fn letrec(&mut self, items: &[(u32, ExpId)]) -> Option<()> {
        let k = self.k();
        let mut parts = Vec::with_capacity(items.len());
        for &(slot, e) in items {
            let part = match k.nodes.exp(e) {
                Some(Exp::Closure(p)) | Some(Exp::Dictabs(p)) => p,
                _ => {
                    self.bug("letrec の項目が closure でも dictabs でもない");
                    return None;
                }
            };
            let unit = self.b.konst(Const::Unit);
            let dst = self.b.reg(Rep::Any);
            self.b.emit(Inst::ClosureNew {
                dst,
                func: self.p.func(part.func),
                caps: vec![unit; part.caps.len()],
            });
            self.bind_slot(slot, V::any(dst));
            parts.push((dst, part));
        }
        for (clo, part) in parts {
            for (i, &c) in part.caps.iter().enumerate() {
                let value = self.cap_any(c)?;
                self.b.emit(Inst::ClosureSetCap {
                    clo,
                    index: i as u32,
                    value,
                });
            }
        }
        Some(())
    }

    #[inline(never)]
    fn letpat(&mut self, pat: kir::PatId, v: V) -> Option<()> {
        let fail = self.b.block();
        self.pattern(pat, v, fail, &crate::pattern::Target::Slots);
        let cont = self.b.current();
        self.b.switch_to(fail);
        let op = self.boxed(v);
        self.b.terminate(Term::Error {
            kind: RtError::PatternFail,
            operand: Some(op),
        });
        match cont {
            Some(c) => {
                self.b.switch_to(c);
                Some(())
            }
            None => None,
        }
    }

    #[inline(never)]
    fn match_(&mut self, scrutinee: ExpId, clauses: &[kir::Clause], tail: bool) -> Option<V> {
        let v = self.exp(scrutinee, false)?;
        let mut results = Vec::new();
        for c in clauses {
            let next = self.b.block();
            self.pattern(c.pat, v, next, &crate::pattern::Target::Slots);
            if self.b.current().is_some()
                && let Some(g) = c.guard
                && let Some(gv) = self.exp(g, false)
            {
                let cond = self.coerce(gv, Rep::Bool);
                let body = self.b.block();
                self.b.terminate(Term::Branch {
                    cond,
                    then: body,
                    else_: next,
                });
                self.b.switch_to(body);
            }
            if self.b.current().is_some()
                && let Some(r) = self.exp(c.body, tail)
            {
                results.push((self.b.current().unwrap(), r));
            }
            self.b.switch_to(next);
        }
        let op = self.boxed(v);
        self.b.terminate(Term::Error {
            kind: RtError::MatchFail,
            operand: Some(op),
        });
        if tail {
            return None;
        }
        self.join(results)
    }

    // ---- 呼び出し ----

    /// `call` を変換する。
    ///
    /// `tail` が真なら末尾呼び出しにする。
    /// `tail` が偽で `ret` が真なら、呼び出しの結果を返してブロックを終える。
    #[inline(never)]
    fn call(
        &mut self,
        tail: bool,
        yields: bool,
        func: ExpId,
        args: &[ExpId],
        ret: bool,
    ) -> Option<V> {
        let k = self.k();
        match k.nodes.exp(func) {
            Some(Exp::Global(g)) => {
                let v = self.call_global(*g, tail, yields, args);
                if tail {
                    return None;
                }
                self.finish(v, ret)
            }
            Some(Exp::Method {
                class,
                method,
                dict,
                own,
            }) => {
                let d = self.dict(*dict);
                let own: Vec<Reg> = own
                    .iter()
                    .map(|&o| {
                        let dv = self.dict(o);
                        self.dict_reg(dv)
                    })
                    .collect();
                let args = self.args(args)?;
                let v = self.method_call(*class, *method, d, own, args, tail, yields);
                if tail {
                    return None;
                }
                self.finish(v, ret)
            }
            _ => {
                let f = self.exp(func, false)?;
                let callee = self.boxed(f);
                let args = self.args_any(args)?;
                let v = self.call_value(callee, args, tail, yields);
                if tail {
                    return None;
                }
                self.finish(v, ret)
            }
        }
    }

    /// 関数値の呼び出しを生成する。
    /// `tail` が真なら末尾呼び出しにする。
    pub(crate) fn call_value(
        &mut self,
        callee: Reg,
        args: Vec<Reg>,
        tail: bool,
        yields: bool,
    ) -> Option<V> {
        if tail {
            self.b.terminate(Term::TailCallValue {
                callee,
                args,
                yields,
            });
            return None;
        }
        let dst = self.b.reg(Rep::Any);
        crate::depth::around_call(&mut self.b, |b| {
            b.emit(Inst::CallValue {
                dst,
                callee,
                args,
                yields,
            })
        });
        Some(V::any(dst))
    }

    /// キャプチャを持たない関数の直接呼び出しを生成する。
    /// `tail` が真なら末尾呼び出しにする。
    pub(crate) fn call_direct(
        &mut self,
        func: FuncId,
        args: Vec<Reg>,
        tail: bool,
        yields: bool,
    ) -> Option<V> {
        if tail {
            self.b
                .terminate(Term::TailCallDirect { func, args, yields });
            return None;
        }
        let dst = self.b.reg(Rep::Any);
        crate::depth::around_call(&mut self.b, |b| {
            b.emit(Inst::CallDirect {
                dst,
                func,
                args,
                yields,
            })
        });
        Some(V::any(dst))
    }

    /// 呼び出し先がグローバルである `call` を変換する。
    fn call_global(
        &mut self,
        g: kir::GlobalId,
        tail: bool,
        yields: bool,
        args: &[ExpId],
    ) -> Option<V> {
        let k = self.k();
        let gl = &k.globals[g.index()];
        match &gl.kind {
            KGlobalKind::Fn(f) => {
                self.global_check(g);
                let args = self.args_any(args)?;
                self.call_direct(self.p.func(*f), args, tail, yields)
            }
            KGlobalKind::Extern { abi, name, .. } => {
                self.global_check(g);
                let vs = self.args(args)?;
                match numeric::by_name(*abi, name) {
                    Some(p) if numeric::sig(p).0.len() == vs.len() => self.prim_call(p, &vs, tail),
                    Some(_) => {
                        let callee = self.global(g).reg;
                        let args = vs.into_iter().map(|v| self.boxed(v)).collect();
                        self.call_value(callee, args, tail, yields)
                    }
                    None => {
                        if !tail {
                            crate::depth::check(&mut self.b);
                        }
                        self.b.terminate(Term::Error {
                            kind: RtError::UnimplPrim(g),
                            operand: None,
                        });
                        None
                    }
                }
            }
            KGlobalKind::Builtin => {
                let name = gl.sym.strip_prefix("builtin/").unwrap_or(&gl.sym);
                let vs = self.args(args)?;
                let args: Vec<Reg> = vs.into_iter().map(|v| self.boxed(v)).collect();
                if let Some(arity) = crate::wrappers::inline_arity(name)
                    && arity == args.len()
                {
                    if !tail {
                        crate::depth::check(&mut self.b);
                    }
                    let r = crate::wrappers::builtin_inline(&mut self.b, name, &args)?;
                    let v = V::any(r);
                    if tail {
                        self.ret(v);
                        return None;
                    }
                    return Some(v);
                }
                match self.p.global_func[g.index()] {
                    Some(f) if crate::wrappers::builtin_arity(name) == Some(args.len()) => {
                        self.call_direct(f, args, tail, yields)
                    }
                    _ => {
                        let callee = self.b.reg(Rep::Any);
                        self.b.emit(Inst::GlobalGet {
                            dst: callee,
                            global: g,
                        });
                        self.call_value(callee, args, tail, yields)
                    }
                }
            }
            KGlobalKind::Value(_) | KGlobalKind::Dictabs(_) => {
                let f = self.global(g);
                let callee = self.boxed(f);
                let args = self.args_any(args)?;
                self.call_value(callee, args, tail, yields)
            }
        }
    }

    /// プリミティブの呼び出しを生成する。
    /// 非末尾呼び出しなら、その前に深さの検査(`depth_check`)を置く。
    pub(crate) fn prim_call(&mut self, p: Prim, vs: &[V], tail: bool) -> Option<V> {
        if !tail {
            crate::depth::check(&mut self.b);
        }
        let (reps, res) = numeric::sig(p);
        let regs: Vec<Reg> = vs
            .iter()
            .zip(reps.iter())
            .map(|(&v, &r)| self.coerce(v, r))
            .collect();
        let r = numeric::emit(&mut self.b, p, &regs)?;
        let v = V { reg: r, rep: res };
        self.finish(Some(v), tail)
    }

    #[inline(never)]
    fn dictapp(&mut self, func: ExpId, dicts: &[kir::DictId]) -> Option<V> {
        let k = self.k();
        if let Some(Exp::Global(g)) = k.nodes.exp(func) {
            match k.globals[g.index()].kind {
                KGlobalKind::Dictabs(f) => {
                    self.global_check(*g);
                    let ds = self.dict_regs(dicts);
                    return self.call_direct(self.p.func(f), ds, false, false);
                }
                KGlobalKind::Extern { .. } => {
                    self.global_check(*g);
                    // 辞書を捨てて、グローバルの値そのものを返す。
                    // 深さは非末尾呼び出しと同じように検査する。
                    crate::depth::check(&mut self.b);
                    let dst = self.b.reg(Rep::Any);
                    self.b.emit(Inst::GlobalGet { dst, global: *g });
                    return Some(V::any(dst));
                }
                _ => {}
            }
        }
        let fv = self.exp(func, false)?;
        let f = self.boxed(fv);
        let ds = self.dict_regs(dicts);
        let dst = self.b.reg(Rep::Any);
        crate::depth::around_call(&mut self.b, |b| b.emit(Inst::DictApp { dst, f, dicts: ds }));
        Some(V::any(dst))
    }

    /// 辞書の式の並びを、それぞれ一様表現の値のレジスタにする。
    pub(crate) fn dict_regs(&mut self, ds: &[kir::DictId]) -> Vec<Reg> {
        ds.iter()
            .map(|&d| {
                let dv = self.dict(d);
                self.dict_reg(dv)
            })
            .collect()
    }

    #[inline(never)]
    fn perform(&mut self, effect: kir::EffectId, op: u32, args: &[ExpId]) -> Option<V> {
        let k = self.k();
        let runtime = k
            .effects
            .get(effect.index())
            .and_then(|e| e.ops.get(op as usize))
            .map(|o| o.runtime)
            .unwrap_or(false);
        let vs = self.args(args)?;
        if runtime && vs.len() == 1 {
            let s = self.coerce(vs[0], Rep::Str);
            let dst = self.b.reg(Rep::Any);
            self.b.emit(Inst::ConsoleWrite { dst, s });
            return Some(V::any(dst));
        }
        let args: Vec<Reg> = vs.into_iter().map(|v| self.boxed(v)).collect();
        Some(crate::effect::perform(self, effect.0, op, args))
    }

    /// ハンドラ式を変換する(パス 8)。
    #[inline(never)]
    fn handle(&mut self, h: &kir::Handle, tail: bool) -> Option<V> {
        let body = self.closure(&h.body)?;
        let mut clauses = Vec::with_capacity(h.ops.len());
        for o in &h.ops {
            clauses.push(self.closure(&o.part)?.reg);
        }
        let ret = match &h.ret {
            Some(p) => Some(self.closure(p)?.reg),
            None => None,
        };
        let cancel = match &h.cancel {
            Some(p) => Some(self.closure(p)?.reg),
            None => None,
        };
        let res = match self.resumption {
            Some(r) => r,
            None => self.b.konst(Const::Unit),
        };
        let ops = h.ops.iter().map(|o| (o.op, o.class)).collect();
        crate::effect::handle(
            self,
            h.effect.0,
            body.reg,
            res,
            ops,
            clauses,
            ret,
            cancel,
            h.tail && tail,
        )
    }

    /// `resume` を変換する(パス 8)。
    ///
    /// `tail` が真なら、`resume` の背骨の上にある末尾 `resume` として変換する。
    /// `tail` が偽で `ret` が真なら、非末尾の `resume` の結果を返してブロックを終える。
    #[inline(never)]
    fn resume(&mut self, tail: bool, value: ExpId, ret: bool) -> Option<V> {
        let v = self.exp(value, false)?;
        let v = self.boxed(v);
        if tail {
            match self.clause {
                Some(kir::ClauseClass::T) => self.b.terminate(Term::Return(v)),
                _ => self.b.terminate(Term::Fx {
                    op: FxTermOp::TailResume,
                    args: vec![v],
                }),
            }
            return None;
        }
        let Some(k) = self.resumption else {
            return self.bug("継続を持たない関数の中の resume");
        };
        let r = crate::effect::resume(self, k, v)?;
        self.finish(Some(r), ret)
    }
}

/// 入口の `env_get` のうち、結果がどこでも使われないものを取り除く。
pub(crate) fn drop_unused_env(blocks: &mut [Block], nregs: usize) {
    let mut used = vec![false; nregs];
    for b in blocks.iter() {
        for i in &b.insts {
            for u in i.uses() {
                used[u.index()] = true;
            }
        }
        for u in b.term.uses() {
            used[u.index()] = true;
        }
    }
    if let Some(b0) = blocks.first_mut() {
        b0.insts
            .retain(|i| !matches!(i, Inst::EnvGet { dst, .. } if !used[dst.index()]));
    }
}
