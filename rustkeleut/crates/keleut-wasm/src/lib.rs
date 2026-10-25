//! WasmGC の後段(計画 261024-1 の 6.10 節、D541〜D545)。
//!
//! LIR のプログラム(パス 12 を済ませたもの)から、WasmGC のモジュール `program.wasm` を作る。
//! モジュールは、rtcore を組んだ `rt.wasm`(線形メモリ、`show(Float64)`、libm、形の表)と、
//! Node 側の実行時 `host/keleut-host.mjs`(起動、入出力、診断の文面)を import する。
//!
//! | モジュール | 中身 |
//! | --- | --- |
//! | [`emit`] | モジュールの組み立て(型の再帰群、関数、大域、データ、名前の節) |
//! | `rt` | 実行時の関数(trap しうる命令を置く補助関数、エフェクト、メソッドの選択) |
//! | `func` | LIR の関数の翻訳(平らなブロック、状態語の伝播、大きなフレームの形) |
//! | [`meta`] | ホストに渡す表(診断の型紙と名前) |
//! | [`check`] | wasmparser による検証と trap の検査器 |
//! | [`launch`] | `keleut run --backend wasm` と `keleut build` の出力と Node の起動 |
//!
//! 値の表現と制御の形は `rt` と `func` の冒頭に書く。
//! M94 は G の節を持つハンドラの取り出しと再開を扱わない。
//! G の節が選ばれたら、標準出力を書き終えてから `keleut: 未実装` で 4 で終わる(M95 で入れる)。

pub mod check;
pub mod emit;
mod func;
pub mod launch;
pub mod meta;
mod rt;

use std::collections::{BTreeSet, HashMap, VecDeque};

use keleut_lir::{FuncId, FuncKind, GlobalKind, Inst, Program};
use wasm_encoder::{ConstExpr, ExportKind, Instruction, ValType};

use emit::{EQ, I32, I64, Mb, rv};
use meta::Templates;

/// 1 段に見込むバイト数 `B_wasm`(M79 の測定。`doc/log/261024-2-m79.md` の 1.5 節)。
pub const B_WASM: u64 = 1536;

/// 1 段のバイト数の推定 `8 × K + 24 × P + 64`(K は局所の数、P は引数の数。M79 の 1.2 節)。
pub fn frame_estimate(locals: u64, params: u64) -> u64 {
    8 * locals + 24 * params + 64
}

/// 呼び出しで Wasm の引数として直に渡す LIR の引数の数の上限。
///
/// 引数が `P_DIRECT` 個を超える呼び出しは、先頭の `P_DIRECT` 個を直に渡し、残りを 1 つの配列(`$anyarr`)にまとめて渡す。
/// 呼ばれた側は入口で配列を読み、残りの引数をふつうのレジスタに置く。
/// 引数の多い関数でも 1 段が [`B_WASM`] に収まるようにするためである(推定で `24 × (32 + 3) + 64 = 904` バイト)。
pub const P_DIRECT: u32 = 32;

/// 1 つの呼び出しでまとめて渡せる引数の数の上限(`array.new_fixed` の要素の数。V8 の上限は 10,000)。
pub const PACKED_MAX: u32 = 1000;

/// LIR の引数が `n` 個の呼び出しで、Wasm の引数として渡す値の数。
pub fn wire_arity(n: u32) -> u32 {
    if n > P_DIRECT { P_DIRECT + 1 } else { n }
}

/// `--stack-size` に渡す KiB(`(L + R) × B_wasm + 32 MiB`。M79 の 1.5 節)。
pub fn stack_size_kib(max_depth: u64) -> u64 {
    let r = (max_depth / 64).max(1024);
    (max_depth.saturating_add(r))
        .saturating_mul(B_WASM)
        .saturating_add(32 << 20)
        .div_ceil(1024)
}

/// V8 の `--stack-size` が受け付ける最大の KiB(M79 の 1.3 節)。
pub const STACK_SIZE_MAX_KIB: u64 = 2_097_151;

/// 状態語の種類(大域 `$p_kind`)。
pub mod pk {
    pub const NONE: i32 = 0;
    pub const YIELD: i32 = 1;
    pub const DISCARD_VALUE: i32 = 2;
    pub const DISCARD_NEVER: i32 = 3;
    pub const ERROR: i32 = 4;
    pub const MATCHED: i32 = 5;
    pub const CLAUSE_VALUE: i32 = 6;
    pub const TAIL_RESUME: i32 = 7;
}

/// 操作節の分類の番号(ハンドラ記録の `classes`)。
pub mod cls {
    pub const T: i32 = 0;
    pub const N: i32 = 1;
    pub const G: i32 = 2;
}

/// 関数の鍵。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FKey {
    /// LIR の関数。
    Lir(u32),
    /// 固定の実行時の関数。
    H(rt::H),
    /// 値を `n` 個の引数で呼ぶ。
    CallV(u32),
    /// 辞書からメソッドを選んで呼ぶ(クラス、メソッド、メソッド自身の辞書の数、引数の数)。
    CallM(u32, u32, u32, u32),
    /// 辞書の抽象に `k` 個の辞書を渡す。
    DictApp(u32),
    /// 退避した `DISCARD(NeverBody)` の節の本体を末尾で呼ぶ。
    TailNever,
    /// 構造的な `Eq` の比べ方(クラス)。
    EqStruct(u32),
}

/// 固定の大域の番号。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gl {
    pub depth: u32,
    pub limit: u32,
    pub lmax: u32,
    pub lr: u32,
    pub next_hid: u32,
    pub chain: u32,
    pub p_kind: u32,
    pub p_hr: u32,
    pub p_val: u32,
    pub p_clause: u32,
    pub p_args: u32,
    pub p_msg: u32,
    pub unit: u32,
    pub true_: u32,
    pub false_: u32,
    pub str_true: u32,
    pub str_false: u32,
    pub empty: u32,
}

/// 翻訳の文脈。
pub(crate) struct Cg<'p> {
    pub prog: &'p Program,
    pub m: Mb,
    fmap: HashMap<FKey, u32>,
    pending: VecDeque<(FKey, u32)>,
    pub g: Gl,
    /// 大域ごとの(値、初期化済みの印)。
    pub pglobals: Vec<(u32, u32)>,
    /// インスタンスごとの初期化済みの印。
    pub inst_init: Vec<u32>,
    /// インスタンスとメソッドごとの値。
    pub inst_vals: Vec<Vec<u32>>,
    /// 文字列の定数(大域、データの位置、長さ)。
    pub strs: Vec<(u32, u32, u32)>,
    ctor0: HashMap<(u32, u32), u32>,
    dictb: HashMap<(u32, u32), u32>,
    dicti0: HashMap<u32, u32>,
    /// 診断の型紙。
    pub tpl: Templates,
    /// メソッドの値を作る箇所の(クラス、メソッド、メソッド自身の辞書の数)。
    pub method_vals: BTreeSet<(u32, u32, u32)>,
    /// `extern` の本体の関数。
    pub extern_funcs: Vec<u32>,
    /// 操作節の関数の引数の数。
    pub clause_arities: BTreeSet<u32>,
    /// 実装の無い `extern` の大域。
    pub extern_unimpl: Vec<u32>,
    /// 統計。
    pub stats: Stats,
}

/// 翻訳の統計(記録に書く値)。
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// LIR の関数の数。
    pub funcs: usize,
    /// 大きなフレームの形にした関数の数。
    pub big_frames: usize,
    /// 1 段のバイト数の推定の最大(大きなフレームの形にした後)。
    pub max_estimate: u64,
    /// 引数の数の最大。
    pub max_params: usize,
    /// 1 つの関数のブロックの数の最大。
    pub max_blocks: usize,
    /// 呼び出しの引数の数の最大。
    pub max_args: u32,
    /// いくつかの Wasm の関数に分けた LIR の関数の数。
    pub split_funcs: usize,
    /// 1 つの LIR の関数を分けた部分の数の最大。
    pub max_parts: usize,
}

fn cexpr(insns: Vec<Instruction<'static>>) -> ConstExpr {
    ConstExpr::extended(insns)
}

impl<'p> Cg<'p> {
    fn new(prog: &'p Program) -> Cg<'p> {
        let mut m = Mb::new();
        let t = m.t;
        let i64g = |m: &mut Mb, n: &str| m.global(I64, true, ConstExpr::i64_const(0), n);
        let i32g = |m: &mut Mb, n: &str| m.global(I32, true, ConstExpr::i32_const(0), n);
        let eqg = |m: &mut Mb, n: &str| m.global(EQ, true, ConstExpr::ref_null(emit::HEQ), n);
        let depth = i64g(&mut m, "depth");
        let limit = i64g(&mut m, "limit");
        let lmax = i64g(&mut m, "lmax");
        let lr = i64g(&mut m, "lr");
        let next_hid = m.global(I64, true, ConstExpr::i64_const(1), "next_hid");
        let chain = eqg(&mut m, "chain");
        let p_kind = i32g(&mut m, "p_kind");
        let p_hr = eqg(&mut m, "p_hr");
        let p_val = eqg(&mut m, "p_val");
        let p_clause = i32g(&mut m, "p_clause");
        let p_args = eqg(&mut m, "p_args");
        let p_msg = i32g(&mut m, "p_msg");
        let unit = m.global(
            rv(false, t.unit),
            false,
            cexpr(vec![Instruction::StructNew(t.unit)]),
            "unit",
        );
        let true_ = m.global(
            rv(false, t.bool_),
            false,
            cexpr(vec![
                Instruction::I32Const(1),
                Instruction::StructNew(t.bool_),
            ]),
            "true",
        );
        let false_ = m.global(
            rv(false, t.bool_),
            false,
            cexpr(vec![
                Instruction::I32Const(0),
                Instruction::StructNew(t.bool_),
            ]),
            "false",
        );
        let empty = m.global(
            rv(false, t.anyarr),
            false,
            cexpr(vec![Instruction::ArrayNewFixed {
                array_type_index: t.anyarr,
                array_size: 0,
            }]),
            "empty",
        );
        let strg = |m: &mut Mb, n: &str| {
            m.global(
                rv(true, t.str_),
                true,
                ConstExpr::ref_null(wasm_encoder::HeapType::Concrete(t.str_)),
                n,
            )
        };
        let str_true = strg(&mut m, "str_true");
        let str_false = strg(&mut m, "str_false");
        let g = Gl {
            depth,
            limit,
            lmax,
            lr,
            next_hid,
            chain,
            p_kind,
            p_hr,
            p_val,
            p_clause,
            p_args,
            p_msg,
            unit,
            true_,
            false_,
            str_true,
            str_false,
            empty,
        };
        let mut pglobals = Vec::new();
        for (i, gl) in prog.globals.iter().enumerate() {
            let v = eqg(&mut m, &format!("g{i}"));
            let f = i32g(&mut m, &format!("g{i}.init"));
            let _ = gl;
            pglobals.push((v, f));
        }
        let mut inst_init = Vec::new();
        let mut inst_vals = Vec::new();
        for (i, inst) in prog.instances.iter().enumerate() {
            inst_init.push(i32g(&mut m, &format!("inst{i}.init")));
            inst_vals.push(
                (0..inst.methods.len())
                    .map(|k| eqg(&mut m, &format!("inst{i}.m{k}")))
                    .collect(),
            );
        }
        let mut strs = Vec::new();
        for (i, s) in prog.strings.iter().enumerate() {
            let gi = strg(&mut m, &format!("str{i}"));
            let off = m.add_data(s);
            strs.push((gi, off, s.len() as u32));
        }
        let mut extern_funcs = Vec::new();
        let mut extern_unimpl = Vec::new();
        for (i, gl) in prog.globals.iter().enumerate() {
            match &gl.kind {
                GlobalKind::Extern { func: Some(f), .. } => extern_funcs.push(f.0),
                GlobalKind::Extern { func: None, .. } => extern_unimpl.push(i as u32),
                _ => {}
            }
        }
        let mut clause_arities = BTreeSet::new();
        let mut method_vals = BTreeSet::new();
        for f in &prog.funcs {
            if f.kind == FuncKind::Kir(keleut_lir::FnKind::Opclause) {
                clause_arities.insert(f.params.len() as u32);
            }
            for b in &f.blocks {
                for i in &b.insts {
                    if let Inst::MethodNew {
                        class, method, own, ..
                    } = i
                    {
                        method_vals.insert((class.0, *method, own.len() as u32));
                    }
                }
            }
        }
        Cg {
            prog,
            m,
            fmap: HashMap::new(),
            pending: VecDeque::new(),
            g,
            pglobals,
            inst_init,
            inst_vals,
            strs,
            ctor0: HashMap::new(),
            dictb: HashMap::new(),
            dicti0: HashMap::new(),
            tpl: Templates::default(),
            method_vals,
            extern_funcs,
            clause_arities,
            extern_unimpl,
            stats: Stats::default(),
        }
    }

    /// 関数の番号(無ければ型を決めて確保し、本体を後で作る)。
    pub fn func(&mut self, k: FKey) -> u32 {
        if let Some(&i) = self.fmap.get(&k) {
            return i;
        }
        let (params, results, name, safe) = self.sig_of(&k);
        let ty = match &k {
            FKey::Lir(f) => {
                let n = self.prog.funcs[*f as usize].params.len() as u32;
                self.m.fn_types(n).0
            }
            _ => self.m.sig(&params, &results),
        };
        let idx = self.m.declare_func(ty, name, safe);
        self.fmap.insert(k.clone(), idx);
        self.pending.push_back((k, idx));
        idx
    }

    /// 積んだ `n` 個の引数のうち、[`P_DIRECT`] 個を超える分を 1 つの配列にまとめる。
    pub fn pack_args(&mut self, fb: &mut emit::Fb, n: u32) {
        self.stats.max_args = self.stats.max_args.max(n);
        if n > P_DIRECT {
            fb.i()
                .array_new_fixed(self.m.t.anyarr, (n - P_DIRECT).min(PACKED_MAX));
        }
    }

    /// LIR の関数の番号。
    pub fn lir(&mut self, f: FuncId) -> u32 {
        self.func(FKey::Lir(f.0))
    }

    /// LIR の関数の引数の数。
    pub fn arity(&self, f: FuncId) -> u32 {
        self.prog.funcs[f.index()].params.len() as u32
    }

    fn sig_of(&mut self, k: &FKey) -> (Vec<ValType>, Vec<ValType>, String, bool) {
        match k {
            FKey::Lir(f) => {
                let func = &self.prog.funcs[*f as usize];
                (Vec::new(), Vec::new(), format!("f{f}:{}", func.sym), false)
            }
            FKey::H(h) => {
                let (p, r, safe) = rt::sig(self, *h);
                (p, r, format!("{h:?}"), safe)
            }
            FKey::CallV(n) => {
                let mut p = vec![EQ];
                p.extend(self.m.arg_types(*n));
                (p, vec![EQ], format!("callv{n}"), false)
            }
            FKey::CallM(c, m, no, n) => {
                let mut p = vec![EQ];
                p.extend(std::iter::repeat_n(EQ, *no as usize));
                p.extend(self.m.arg_types(*n));
                (p, vec![EQ], format!("callm{c}.{m}.{no}.{n}"), false)
            }
            FKey::DictApp(n) => {
                let mut p = vec![EQ];
                p.extend(self.m.arg_types(*n));
                (p, vec![EQ], format!("dictapp{n}"), false)
            }
            FKey::TailNever => (vec![EQ], vec![EQ], "tail_never".to_string(), false),
            FKey::EqStruct(c) => (vec![EQ, EQ, EQ], vec![I32], format!("eq_struct{c}"), false),
        }
    }

    /// 引数の無いコンストラクタの値の大域。
    pub fn ctor0(&mut self, ty: u32, c: u32) -> u32 {
        if let Some(&g) = self.ctor0.get(&(ty, c)) {
            return g;
        }
        let t = self.m.t;
        let g = self.m.global(
            rv(false, t.data),
            false,
            cexpr(vec![
                Instruction::I32Const(ty as i32),
                Instruction::I32Const(c as i32),
                Instruction::ArrayNewFixed {
                    array_type_index: t.anyarr,
                    array_size: 0,
                },
                Instruction::StructNew(t.data),
            ]),
            &format!("ctor0.{ty}.{c}"),
        );
        self.ctor0.insert((ty, c), g);
        g
    }

    /// 組み込みのインスタンスの辞書の大域。
    pub fn dictb(&mut self, class: u32, ty: u32) -> u32 {
        if let Some(&g) = self.dictb.get(&(class, ty)) {
            return g;
        }
        let t = self.m.t;
        let g = self.m.global(
            rv(false, t.dictb),
            false,
            cexpr(vec![
                Instruction::I32Const(class as i32),
                Instruction::I32Const(ty as i32),
                Instruction::StructNew(t.dictb),
            ]),
            &format!("dictb.{class}.{ty}"),
        );
        self.dictb.insert((class, ty), g);
        g
    }

    /// 前提を持たない利用者のインスタンスの辞書の大域。
    pub fn dicti0(&mut self, inst: u32) -> u32 {
        if let Some(&g) = self.dicti0.get(&inst) {
            return g;
        }
        let t = self.m.t;
        let g = self.m.global(
            rv(false, t.dicti),
            false,
            cexpr(vec![
                Instruction::I32Const(inst as i32),
                Instruction::ArrayNewFixed {
                    array_type_index: t.anyarr,
                    array_size: 0,
                },
                Instruction::StructNew(t.dicti),
            ]),
            &format!("dicti0.{inst}"),
        );
        self.dicti0.insert(inst, g);
        g
    }
}

/// 翻訳の選択肢。
#[derive(Clone, Debug)]
pub struct Options {
    /// 論理的な深さの限度 `L`(`keleut build` の出力が単独で走るときの既定)。
    pub max_depth: u64,
}

/// 翻訳の結果。
pub struct Output {
    /// `program.wasm`。
    pub program: Vec<u8>,
    /// ホストに渡す表(JSON)。
    pub meta: String,
    /// 統計。
    pub stats: Stats,
}

/// LIR のプログラムを WasmGC のモジュールにする。
pub fn compile(prog: &Program, opts: &Options) -> Result<Output, String> {
    if prog.funcs.iter().any(|f| f.frame.is_none()) {
        return Err("パス 12 を済ませていない関数があります".into());
    }
    let mut cg = Cg::new(prog);
    for i in 0..prog.funcs.len() {
        cg.lir(FuncId(i as u32));
    }
    let main = cg.func(FKey::H(rt::H::Main));
    cg.m.export("main", ExportKind::Func, main);
    for (name, h) in rt::EXPORTED {
        let f = cg.func(FKey::H(*h));
        cg.m.export(name, ExportKind::Func, f);
    }
    cg.m.export("p_kind", ExportKind::Global, cg.g.p_kind);
    cg.m.export("p_msg", ExportKind::Global, cg.g.p_msg);
    while let Some((k, idx)) = cg.pending.pop_front() {
        let body = match &k {
            FKey::Lir(f) => func::generate(&mut cg, FuncId(*f))?,
            _ => rt::generate(&mut cg, &k),
        };
        cg.m.set_body(idx, body);
    }
    cg.stats.funcs = prog.funcs.len();
    if cg.stats.max_args > P_DIRECT + PACKED_MAX {
        return Err(format!(
            "引数が {} 個の呼び出しは扱えません(上限は {} 個)",
            cg.stats.max_args,
            P_DIRECT + PACKED_MAX
        ));
    }
    let meta = meta::meta_json(prog, &cg.tpl, opts.max_depth);
    let stats = cg.stats.clone();
    let program = cg.m.finish();
    Ok(Output {
        program,
        meta,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn スタックの大きさは_m79_の記録の値() {
        // M79 の記録の 1.5 節: L = 2^20 で --stack-size = 1,630,208 KiB。
        assert_eq!(stack_size_kib(1 << 20), 1_630_208);
        assert!(stack_size_kib(1 << 20) <= STACK_SIZE_MAX_KIB);
        assert!(stack_size_kib(2_000_000) > STACK_SIZE_MAX_KIB);
        // 局所 178 個、引数 2 個の関数がちょうど B_wasm に収まる(M79 の 1.5 節)。
        assert_eq!(frame_estimate(178, 2), 1536);
        assert!(frame_estimate(179, 2) > B_WASM);
    }
}
