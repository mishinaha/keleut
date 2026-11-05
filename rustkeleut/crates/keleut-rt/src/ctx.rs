//! 実行時の文脈と、値を作り読む操作(計画 261024-1 の 6.5 節、`doc/lir.md` の各命令の意味)。
//!
//! [`Rt`] は、ヒープ、根の枠、分割値スタック、標準出力、形の表、深さと限度、
//! 大域とインスタンスの状態、文字列の定数、回収の状態を持つ。
//!
//! 箱を作る関数はどれも [`Rt::alloc`] で確保し、そこで回収が起こりうる([`crate::gc`])。
//! 引数で受け取った語は、後段の根(フレームの安全点の集合など)が持っている前提である。
//! 1 つの関数の中で 2 回以上確保するときは、先に作った箱を根に置いてから次を確保する。
//! 後段(インタプリタ、M91 の JIT)は、LIR の命令のうち実行時に任せるものをここの関数で行う。

use keleut_lir::{
    ClassId, FuncId, FuncKind, GlobalId, GlobalKind, InstanceId, LabelId, N_ALLOC_MAX, Program,
    ShapeId, TypeId,
};
use keleut_rtcore::{ShapeId as RtShape, ShapeTable};

use crate::alloc::{self, Site};
use crate::effect::{Cont, HandlerRec, Pending};
use crate::error::Stop;
use crate::gc::{GcState, Table};
use crate::heap::{Heap, Roots};
use crate::out::Out;
use crate::stack::ValueStack;
use crate::value::{self, ImmKind, Kind, Word, header};

/// 既定の限度 `L = 2^20`(IRSpec 11.5 節)。
pub const DEFAULT_MAX_DEPTH: u64 = 1 << 20;

/// 実行時の文脈。
pub struct Rt<'p> {
    /// プログラム。
    pub prog: &'p Program,
    /// ヒープ。
    pub heap: Heap,
    /// Rust の実行時の関数の根の枠。
    pub roots: Roots,
    /// 分割値スタック。
    pub stack: ValueStack,
    /// 標準出力。
    pub out: Out,
    /// レコードの形の表。
    pub shapes: ShapeTable,
    /// 文脈の深さ `ctx.depth`。
    pub depth: u64,
    /// 今の限度(ふだんは `L`。`depth_limit` の伝播中は `L + R`。IRSpec 11.5 節)。
    pub limit: u64,
    /// 限度 `L`。
    pub max_depth: u64,
    /// ハンドラ記録の表(箱 [`Kind::Handler`] の 1 番の語が添字)。
    pub handlers: Vec<HandlerRec>,
    /// 継続の表(箱 [`Kind::Cont`] の 1 番の語が添字)。
    pub conts: Vec<Cont>,
    /// 退避した状態語の表(箱 [`Kind::Pending`] の 1 番の語が添字)。
    pub saved: Vec<Pending>,
    /// 有効なハンドラの連鎖 `ctx.handlers` の先頭(無ければ Unit)。
    pub chain: Word,
    /// 状態語 `ctx.pending`。
    pub pending: Pending,
    /// 次に作るハンドラ記録の起動の番号。
    pub next_handler: u64,
    /// 大域の値。
    pub globals: Vec<Word>,
    /// 大域の初期化済みの印。
    pub global_init: Vec<bool>,
    /// インスタンスの初期化済みの印。
    pub inst_init: Vec<bool>,
    /// `inst_set_value` が覚えた値(インスタンスごと、メソッドの位置ごと)。
    pub inst_values: Vec<Vec<Word>>,
    /// 文字列の定数の箱。
    pub strings: Vec<Word>,
    /// 関数ごとの、`<prim …>` と書くときの名前(組み込みの値と `extern` の本体だけ)。
    pub func_prim_name: Vec<Option<String>>,
    /// `extern` の大域ごとの、`extern` を本体とする関数(種類 `extern` の値を見分ける)。
    pub extern_funcs: Vec<bool>,
    /// 回収の状態。
    pub gc: GcState,
}

/// 記号から単位の鍵(最後の `/` まで)と末尾の `#n` を除いた名前(IRSpec 3.1 節)。
pub fn display_of_symbol(s: &str) -> &str {
    let s = match s.rfind('/') {
        Some(i) => &s[i + 1..],
        None => s,
    };
    if let Some(i) = s.rfind('#') {
        let d = &s[i + 1..];
        if !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) {
            return &s[..i];
        }
    }
    s
}

/// ラベルやコンストラクタの名前の表示(Diktor の `Type.display`)。
///
/// `%` で始まる内部の名前は、最初の `.` の後ろを書く。
pub fn display_name(name: &str) -> &str {
    if name.len() > 1 && name.starts_with('%') {
        match name.find('.') {
            Some(i) => &name[i + 1..],
            None => name,
        }
    } else {
        name
    }
}

impl<'p> Rt<'p> {
    /// プログラム `prog` の文脈を作る。
    ///
    /// 形の表にプログラムの形を順に入れ、番号が表の添字と一致することを確かめる(`doc/lir.md` の 1.5 節)。
    /// 文字列の定数と、関数を値とする大域のクロージャを先に確保する。
    /// 作った箱はすぐに表(根)に入れるので、その間の回収で失われない。
    pub fn new(prog: &'p Program, limit: u64) -> Result<Rt<'p>, Stop> {
        Self::new_with(prog, limit, false)
    }

    /// [`Rt::new`] と同じで、`stress` が真なら `--gc-stress` の規則で回収する([`crate::gc::STRESS_RATIO`])。
    pub fn new_with(prog: &'p Program, limit: u64, stress: bool) -> Result<Rt<'p>, Stop> {
        let mut shapes = ShapeTable::new();
        let mut pos = Vec::new();
        for (i, s) in prog.shapes.iter().enumerate() {
            let labels: Vec<u32> = s.iter().map(|l| l.0).collect();
            let id = {
                let _site = alloc::site(Site::Shape);
                shapes.intern(&labels, &mut pos)
            };
            if id.0 as usize != i {
                return Err(Stop::Internal(format!(
                    "形の表の番号が合いません(LIR の {i} 番が実行時の {} 番)",
                    id.0
                )));
            }
        }
        let mut func_prim_name = vec![None; prog.funcs.len()];
        let mut extern_funcs = vec![false; prog.funcs.len()];
        for g in &prog.globals {
            match &g.kind {
                GlobalKind::Builtin { func } => {
                    func_prim_name[func.index()] = Some(g.display.clone());
                }
                GlobalKind::Extern { func: Some(f), .. } => {
                    func_prim_name[f.index()] = Some(g.display.clone());
                    extern_funcs[f.index()] = true;
                }
                _ => {}
            }
        }
        let mut rt = Rt {
            prog,
            heap: Heap::new(),
            roots: Roots::default(),
            stack: ValueStack::new(),
            out: Out::default(),
            shapes,
            depth: 0,
            limit,
            max_depth: limit,
            handlers: Vec::new(),
            conts: Vec::new(),
            saved: Vec::new(),
            chain: Word::UNIT,
            pending: Pending::None,
            next_handler: 1,
            globals: vec![Word::UNIT; prog.globals.len()],
            global_init: vec![false; prog.globals.len()],
            inst_init: vec![false; prog.instances.len()],
            inst_values: prog
                .instances
                .iter()
                .map(|i| vec![Word::UNIT; i.methods.len()])
                .collect(),
            strings: Vec::with_capacity(prog.strings.len()),
            func_prim_name,
            extern_funcs,
            gc: GcState::new(prog.funcs.len()),
        };
        rt.gc.stress = stress;
        rt.keep_pending_spare()?;
        for s in &prog.strings {
            let w = rt.new_str(s)?;
            rt.strings.push(w);
        }
        for (i, g) in prog.globals.iter().enumerate() {
            let w = match &g.kind {
                GlobalKind::Builtin { func } => {
                    rt.global_init[i] = true;
                    rt.closure_new(*func, &[])?
                }
                GlobalKind::Extern { func: Some(f), .. } => rt.closure_new(*f, &[])?,
                GlobalKind::Extern { func: None, .. } => {
                    let p = rt.alloc(2)?;
                    // SAFETY: p は 2 語の新しい領域である。
                    unsafe {
                        *p = header(Kind::ExternUnimpl, 0);
                        *p.add(1) = i as u64;
                    }
                    Word::from_ptr(p)
                }
                GlobalKind::Fn(f) | GlobalKind::Dictabs(f) => rt.closure_new(*f, &[])?,
                GlobalKind::Value(_) => Word::UNIT,
            };
            rt.globals[i] = w;
        }
        Ok(rt)
    }

    /// 見出しを書いた箱を確保する。残りの語は呼び出し側が、次の確保の前に書く。
    fn alloc_obj(&mut self, kind: Kind, n: u32, words: usize) -> Result<*mut u64, Stop> {
        let p = self.alloc(words)?;
        // SAFETY: p は words ≧ 1 語の新しい領域である。
        unsafe { *p = header(kind, n) };
        Ok(p)
    }

    // ---- 数 ----

    /// `Int64` を箱に入れる。
    pub fn box_i64(&mut self, n: i64) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::I64, 0, 2)?;
        // SAFETY: p は 2 語の新しい箱である。
        unsafe { *p.add(1) = n as u64 };
        Ok(Word::from_ptr(p))
    }

    /// `Float64` を箱に入れる。
    pub fn box_f64(&mut self, bits: u64) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::F64, 0, 2)?;
        // SAFETY: p は 2 語の新しい箱である。
        unsafe { *p.add(1) = bits };
        Ok(Word::from_ptr(p))
    }

    /// 箱の中の 1 番の語(`Int64` と `Float64` の値)。
    ///
    /// 前提:`w` は `Int64` か `Float64` の箱である(`doc/lir.md` の `unbox`)。
    #[inline]
    pub fn unbox_word(&self, w: Word) -> u64 {
        // SAFETY: 前提により w は 2 語の箱である。
        unsafe { value::get(w, 1) }
    }

    // ---- 文字列 ----

    /// バイト列の文字列を作る。長さが `N_alloc_max` を超えればメモリの不足にする。
    pub fn new_str(&mut self, s: &[u8]) -> Result<Word, Stop> {
        let p = self.alloc_str(s.len())?;
        // SAFETY: p は長さ s.len() の新しい文字列の箱である。
        unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), p.add(1).cast::<u8>(), s.len()) };
        Ok(Word::from_ptr(p))
    }

    fn alloc_str(&mut self, len: usize) -> Result<*mut u64, Stop> {
        if len > N_ALLOC_MAX as usize {
            return Err(Stop::OutOfMemory);
        }
        let words = 1 + len.div_ceil(8);
        self.alloc_obj(Kind::Str, len as u32, words)
    }

    /// 文字列のバイト列。
    ///
    /// 前提:`w` は文字列である。
    #[inline]
    pub fn str_bytes(&self, w: Word) -> &[u8] {
        // SAFETY: 前提により w は生きている文字列の箱で、箱は動かない。
        // 借用の間は self を変えられないので確保も回収も起きない。
        unsafe { value::str_bytes(w) }
    }

    /// 2 つの文字列の連結。
    pub fn str_concat(&mut self, a: Word, b: Word) -> Result<Word, Stop> {
        let (la, lb) = (self.str_bytes(a).len(), self.str_bytes(b).len());
        let p = self.alloc_str(la.checked_add(lb).ok_or(Stop::OutOfMemory)?)?;
        // SAFETY: p は la + lb バイトの新しい文字列の箱で、a と b は生きている文字列である。
        unsafe {
            let d = p.add(1).cast::<u8>();
            std::ptr::copy_nonoverlapping(a.ptr().add(1).cast::<u8>(), d, la);
            std::ptr::copy_nonoverlapping(b.ptr().add(1).cast::<u8>(), d.add(la), lb);
        }
        Ok(Word::from_ptr(p))
    }

    /// 部分の文字列(全域。範囲の外は切り詰める)。
    pub fn str_sub(&mut self, s: Word, pos: i32, len: i32) -> Result<Word, Stop> {
        let n = self.str_bytes(s).len() as i64;
        let start = i64::from(pos).clamp(0, n);
        let end = (start + i64::from(len).clamp(0, n)).min(n);
        let (start, end) = (start as usize, end as usize);
        let p = self.alloc_str(end - start)?;
        // SAFETY: 範囲は s の中で、p は新しい箱である。
        unsafe {
            std::ptr::copy_nonoverlapping(
                s.ptr().add(1).cast::<u8>().add(start),
                p.add(1).cast::<u8>(),
                end - start,
            )
        };
        Ok(Word::from_ptr(p))
    }

    // ---- データとヴァリアント ----

    /// 名目的な型の値を作る。`fields` が空なら即値にする。
    pub fn ctor_new(&mut self, ty: TypeId, ctor: u32, fields: &[Word]) -> Result<Word, Stop> {
        if fields.is_empty() {
            return Ok(Word::ctor0(ty.0, ctor));
        }
        let n = fields.len();
        let p = self.alloc_obj(Kind::Data, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n 語の新しい箱である。
        unsafe {
            *p.add(1) = (u64::from(ty.0) << 32) | u64::from(ctor);
            for (i, f) in fields.iter().enumerate() {
                *p.add(2 + i) = f.0;
            }
        }
        Ok(Word::from_ptr(p))
    }

    /// コンストラクタの番号。前提:`v` は名目的な型の値である。
    #[inline]
    pub fn ctor_tag(&self, v: Word) -> i32 {
        if v.is_imm() {
            v.payload_pair().1 as i32
        } else {
            // SAFETY: 前提により v はデータの箱である。
            unsafe { value::get(v, 1) as u32 as i32 }
        }
    }

    /// フィールドの `i` 番目(全域。無ければ Unit)。
    #[inline]
    pub fn ctor_field(&self, v: Word, i: u32) -> Word {
        if v.is_imm() {
            return Word::UNIT;
        }
        // SAFETY: 前提により v はデータの箱で、i は要素の数と比べてから読む。
        unsafe {
            if i < value::count_of(v) {
                Word(value::get(v, 2 + i as usize))
            } else {
                Word::UNIT
            }
        }
    }

    /// ヴァリアントを作る。
    pub fn variant_new(&mut self, label: LabelId, payload: Word) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::Variant, 0, 3)?;
        // SAFETY: p は 3 語の新しい箱である。
        unsafe {
            *p.add(1) = u64::from(label.0);
            *p.add(2) = payload.0;
        }
        Ok(Word::from_ptr(p))
    }

    /// ヴァリアントのラベルの番号。
    #[inline]
    pub fn variant_tag(&self, v: Word) -> i32 {
        // SAFETY: 前提により v はヴァリアントの箱である。
        unsafe { value::get(v, 1) as i32 }
    }

    /// ヴァリアントの積載値。
    #[inline]
    pub fn variant_payload(&self, v: Word) -> Word {
        // SAFETY: 前提により v はヴァリアントの箱である。
        Word(unsafe { value::get(v, 2) })
    }

    // ---- レコード ----

    /// 形とフィールドからレコードを作る。フィールドが無ければ Unit にする。
    pub fn rec_new(&mut self, shape: ShapeId, fields: &[Word]) -> Result<Word, Stop> {
        if fields.is_empty() {
            return Ok(Word::UNIT);
        }
        let n = fields.len();
        let p = self.alloc_obj(Kind::Record, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n 語の新しい箱である。
        unsafe {
            *p.add(1) = u64::from(shape.0);
            std::ptr::copy_nonoverlapping(fields.as_ptr().cast::<u64>(), p.add(2), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// レコードの形とフィールドの数。Unit は空の形である。
    ///
    /// 前提:`v` はレコードか Unit である。
    #[inline]
    pub fn rec_shape(&self, v: Word) -> (u32, u32) {
        if v.is_imm() {
            return (0, 0);
        }
        // SAFETY: 前提により v はレコードの箱である。
        unsafe { (value::get(v, 1) as u32, value::count_of(v)) }
    }

    /// レコードの `i` 番目のフィールド(全域。無ければ Unit)。
    #[inline]
    pub fn rec_get(&self, v: Word, i: u32) -> Word {
        let (_, n) = self.rec_shape(v);
        if i < n {
            // SAFETY: v はレコードの箱で、i は要素の数より小さい。
            Word(unsafe { value::get(v, 2 + i as usize) })
        } else {
            Word::UNIT
        }
    }

    /// 外側から `nth` 番目の `label` のフィールドの位置。
    fn rec_pos(&self, v: Word, label: LabelId, nth: u32) -> Option<u32> {
        let (shape, _) = self.rec_shape(v);
        let first = self.shapes.find(RtShape(shape), label.0)?;
        let labels = self.shapes.labels(RtShape(shape));
        let p = first.checked_add(nth)?;
        (labels.get(p as usize) == Some(&label.0)).then_some(p)
    }

    /// `rec_field`。
    pub fn rec_field(&self, v: Word, label: LabelId, nth: u32) -> Word {
        match self.rec_pos(v, label, nth) {
            Some(p) => self.rec_get(v, p),
            None => Word::UNIT,
        }
    }

    /// `rec_has`。
    pub fn rec_has(&self, v: Word, label: LabelId, nth: u32) -> bool {
        self.rec_pos(v, label, nth).is_some()
    }

    /// レコードのフィールドの写し。1 つ足せる容量で取る。
    ///
    /// 写しの領域は失敗を自分で扱う範囲で取り(`crate::alloc`)、取れなければメモリの不足にする。
    fn rec_fields(&self, v: Word) -> Result<Vec<Word>, Stop> {
        let (_, n) = self.rec_shape(v);
        let _site = alloc::site(Site::RecFields);
        let mut fs = Vec::new();
        alloc::handled(|| fs.try_reserve_exact(n as usize + 1)).map_err(|_| Stop::OutOfMemory)?;
        fs.extend((0..n).map(|i| self.rec_get(v, i)));
        Ok(fs)
    }

    /// `rec_extend`。
    pub fn rec_extend(&mut self, v: Word, label: LabelId, x: Word) -> Result<Word, Stop> {
        let (shape, _) = self.rec_shape(v);
        let (ns, pos) = {
            let _site = alloc::site(Site::Shape);
            self.shapes.extend(RtShape(shape), label.0)
        };
        let mut fs = self.rec_fields(v)?;
        fs.insert(pos as usize, x);
        self.rec_new(ShapeId(ns.0), &fs)
    }

    /// `rec_restrict`(全域。ラベルが無ければ同じフィールドのレコード)。
    pub fn rec_restrict(&mut self, v: Word, label: LabelId) -> Result<Word, Stop> {
        let (shape, _) = self.rec_shape(v);
        let mut fs = self.rec_fields(v)?;
        let restricted = {
            let _site = alloc::site(Site::Shape);
            self.shapes.restrict(RtShape(shape), label.0)
        };
        let ns = match restricted {
            Some((ns, pos)) => {
                fs.remove(pos as usize);
                ns.0
            }
            None => shape,
        };
        self.rec_new(ShapeId(ns), &fs)
    }

    /// `rec_update`(全域。ラベルが無ければ同じフィールドのレコード)。
    pub fn rec_update(&mut self, v: Word, label: LabelId, x: Word) -> Result<Word, Stop> {
        let (shape, _) = self.rec_shape(v);
        let mut fs = self.rec_fields(v)?;
        if let Some(p) = self.shapes.find(RtShape(shape), label.0) {
            fs[p as usize] = x;
        }
        self.rec_new(ShapeId(shape), &fs)
    }

    // ---- 関数の値と辞書 ----

    /// クロージャを作る。
    pub fn closure_new(&mut self, func: FuncId, caps: &[Word]) -> Result<Word, Stop> {
        let n = caps.len();
        let p = self.alloc_obj(Kind::Closure, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n 語の新しい箱である。
        unsafe {
            *p.add(1) = u64::from(func.0);
            std::ptr::copy_nonoverlapping(caps.as_ptr().cast::<u64>(), p.add(2), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// クロージャの関数。前提:`c` はクロージャである。
    #[inline]
    pub fn closure_func(&self, c: Word) -> FuncId {
        // SAFETY: 前提により c はクロージャの箱である。
        FuncId(unsafe { value::get(c, 1) } as u32)
    }

    /// クロージャの捕獲の `i` 番目。
    #[inline]
    pub fn closure_cap(&self, c: Word, i: u32) -> Word {
        // SAFETY: 前提により c はクロージャの箱で、i は捕獲の数より小さい(検証器が確かめる)。
        Word(unsafe { value::get(c, 2 + i as usize) })
    }

    /// クロージャの捕獲の `i` 番目を書く。
    #[inline]
    pub fn closure_set_cap(&mut self, c: Word, i: u32, x: Word) {
        // SAFETY: closure_cap と同じ。
        unsafe { value::set(c, 2 + i as usize, x.0) }
    }

    /// メソッドの値を作る。
    pub fn method_new(
        &mut self,
        class: ClassId,
        method: u32,
        dict: Word,
        own: &[Word],
    ) -> Result<Word, Stop> {
        let n = own.len();
        let p = self.alloc_obj(Kind::Method, n as u32, 3 + n)?;
        // SAFETY: p は 3 + n 語の新しい箱である。
        unsafe {
            *p.add(1) = (u64::from(class.0) << 32) | u64::from(method);
            *p.add(2) = dict.0;
            std::ptr::copy_nonoverlapping(own.as_ptr().cast::<u64>(), p.add(3), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// メソッドの値の中身(クラス、メソッド、辞書、メソッド自身の辞書)。
    pub fn method_parts(&self, m: Word) -> (ClassId, u32, Word, Vec<Word>) {
        // SAFETY: 前提により m はメソッドの値の箱である。
        unsafe {
            let cm = value::get(m, 1);
            let n = value::count_of(m) as usize;
            let own = (0..n).map(|i| Word(value::get(m, 3 + i))).collect();
            (
                ClassId((cm >> 32) as u32),
                cm as u32,
                Word(value::get(m, 2)),
                own,
            )
        }
    }

    /// 前提を持つ利用者のインスタンスの辞書を作る。
    pub fn dict_inst(&mut self, inst: InstanceId, premises: &[Word]) -> Result<Word, Stop> {
        let n = premises.len();
        let p = self.alloc_obj(Kind::DictInst, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n 語の新しい箱である。
        unsafe {
            *p.add(1) = u64::from(inst.0);
            std::ptr::copy_nonoverlapping(premises.as_ptr().cast::<u64>(), p.add(2), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// 構造的な `Eq` の辞書を作る。
    pub fn dict_struct(&mut self, kind: Kind, fields: &[(LabelId, Word)]) -> Result<Word, Stop> {
        let n = fields.len();
        let p = self.alloc_obj(kind, n as u32, 1 + 2 * n)?;
        // SAFETY: p は 1 + 2n 語の新しい箱である。
        unsafe {
            for (i, (l, d)) in fields.iter().enumerate() {
                *p.add(1 + 2 * i) = u64::from(l.0);
                *p.add(2 + 2 * i) = d.0;
            }
        }
        Ok(Word::from_ptr(p))
    }

    /// 辞書の種類を読む。
    pub fn dict_view(&self, d: Word) -> DictView {
        match d.imm_kind() {
            Some(ImmKind::DictBuiltin) => {
                let (c, t) = d.payload_pair();
                return DictView::Builtin(ClassId(c), TypeId(t));
            }
            Some(ImmKind::DictInst0) => {
                return DictView::Inst(InstanceId(d.payload() as u32), Vec::new());
            }
            _ => {}
        }
        // SAFETY: 前提により d は辞書の箱である。
        unsafe {
            let n = value::count_of(d) as usize;
            match value::kind_of(d) {
                Some(Kind::DictInst) => DictView::Inst(
                    InstanceId(value::get(d, 1) as u32),
                    (0..n).map(|i| Word(value::get(d, 2 + i))).collect(),
                ),
                Some(k @ (Kind::DictRecord | Kind::DictVariant)) => {
                    let fs = (0..n)
                        .map(|i| {
                            (
                                LabelId(value::get(d, 1 + 2 * i) as u32),
                                Word(value::get(d, 2 + 2 * i)),
                            )
                        })
                        .collect();
                    if k == Kind::DictRecord {
                        DictView::Record(fs)
                    } else {
                        DictView::Variant(fs)
                    }
                }
                _ => DictView::Other,
            }
        }
    }

    /// 呼べる値の種類を読む。
    pub fn callee_view(&self, f: Word) -> CalleeView {
        if !f.is_ptr() {
            return CalleeView::Other;
        }
        // SAFETY: f は箱へのポインタである。
        match unsafe { value::kind_of(f) } {
            Some(Kind::Closure) => CalleeView::Closure(self.closure_func(f)),
            Some(Kind::Method) => CalleeView::Method,
            // SAFETY: 実装の無い extern の箱の 1 番は大域の番号である。
            Some(Kind::ExternUnimpl) => {
                CalleeView::ExternUnimpl(GlobalId(unsafe { value::get(f, 1) } as u32))
            }
            _ => CalleeView::Other,
        }
    }

    /// 値が `extern` の大域の値か(`dict_app` が辞書を捨てて値そのものを返す相手)。
    pub fn is_extern_value(&self, f: Word) -> bool {
        match self.callee_view(f) {
            CalleeView::ExternUnimpl(_) => true,
            CalleeView::Closure(func) => self.extern_funcs[func.index()],
            _ => false,
        }
    }

    // ---- 組み込みの値 ----

    /// `Ref.new`。
    pub fn ref_new(&mut self, x: Word) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::Ref, 0, 2)?;
        // SAFETY: p は 2 語の新しい箱である。
        unsafe { *p.add(1) = x.0 };
        Ok(Word::from_ptr(p))
    }

    /// 長さ `n`(0 以上)の配列を作り、要素を `x` で埋める。
    pub fn array_new(&mut self, kind: Kind, n: usize, x: Word) -> Result<Word, Stop> {
        if n > N_ALLOC_MAX as usize {
            return Err(Stop::OutOfMemory);
        }
        let p = self.alloc_obj(kind, n as u32, 1 + n)?;
        // SAFETY: p は 1 + n 語の新しい箱である。
        unsafe {
            for i in 0..n {
                *p.add(1 + i) = x.0;
            }
        }
        Ok(Word::from_ptr(p))
    }

    /// 配列の長さ。
    #[inline]
    pub fn array_len(&self, a: Word) -> i32 {
        // SAFETY: 前提により a は配列の箱である。
        unsafe { value::count_of(a) as i32 }
    }

    /// 配列の `i` 番目(全域。範囲の外は Unit)。
    #[inline]
    pub fn array_get(&self, a: Word, i: i32) -> Word {
        if i < 0 || i >= self.array_len(a) {
            return Word::UNIT;
        }
        // SAFETY: i は範囲の中である。
        Word(unsafe { value::get(a, 1 + i as usize) })
    }

    /// 可変の配列の `i` 番目を書く(全域。範囲の外は何もしない)。
    #[inline]
    pub fn array_set(&mut self, a: Word, i: i32, x: Word) {
        if i >= 0 && i < self.array_len(a) {
            // SAFETY: i は範囲の中である。
            unsafe { value::set(a, 1 + i as usize, x.0) }
        }
    }

    /// 可変の配列の写しの `Array`。
    pub fn array_freeze(&mut self, a: Word) -> Result<Word, Stop> {
        let n = self.array_len(a) as usize;
        let p = self.alloc_obj(Kind::Array, n as u32, 1 + n)?;
        // SAFETY: p は 1 + n 語の新しい箱で、a は n 個の要素を持つ。
        unsafe { std::ptr::copy_nonoverlapping(a.ptr().add(1), p.add(1), n) };
        Ok(Word::from_ptr(p))
    }

    // ---- エフェクト ----

    fn index_box(&mut self, kind: Kind, i: usize) -> Result<Word, Stop> {
        let p = self.alloc_obj(kind, 0, 2)?;
        // SAFETY: p は 2 語の新しい箱である。
        unsafe { *p.add(1) = i as u64 };
        Ok(Word::from_ptr(p))
    }

    /// 語が種類 `kind` の箱か。
    pub fn is_kind(&self, w: Word, kind: Kind) -> bool {
        // SAFETY: ポインタの語は生きている箱を指す。
        w.is_ptr() && unsafe { value::kind_of(w) } == Some(kind)
    }

    fn index_of(w: Word) -> usize {
        // SAFETY: 呼び出し側が箱の種類を確かめている。
        unsafe { value::get(w, 1) as usize }
    }

    /// 表に項目を足してから、その添字の箱を作る。箱を作れなければ項目を外して失敗を返す。
    ///
    /// 回収で空いた添字があれば使い回す。
    /// 箱の確保(安全点)の間は、まだ箱の無い項目として根になる([`crate::gc`])。
    /// 表を広げられないときも、プロセスを止めずにメモリの不足を返す。
    /// `ahead` は、新しい項目の後に空けておく項目の数である([`TableGc::keep_spare`](crate::gc::TableGc::keep_spare))。
    /// `urgent` なら、箱の確保で実行の役割の印を見ずに下ろす([`Rt::pending_box`])。
    #[allow(clippy::too_many_arguments)]
    fn table_push<T>(
        &mut self,
        table: fn(&mut Self) -> &mut Vec<T>,
        which: Table,
        kind: Kind,
        dead: fn() -> T,
        x: T,
        ahead: usize,
        urgent: bool,
    ) -> Result<Word, Stop> {
        let len = table(self).len();
        let i = {
            let _site = (which == Table::Pending).then(|| alloc::site(Site::Pending));
            if self.gc.tables[which as usize].state_len() == len {
                alloc::handled(|| table(self).try_reserve(1 + ahead))
                    .map_err(|_| Stop::OutOfMemory)?;
            }
            self.gc.tables[which as usize].next_index(len, ahead)?
        };
        if i == len {
            table(self).push(x);
        } else {
            table(self)[i] = x;
        }
        self.gc.unboxed = Some((which, i));
        let r = if urgent {
            let _ = alloc::take_flag(alloc::Role::Exec);
            self.alloc_after_flag(2).map(|p| {
                // SAFETY: p は 2 語の新しい領域である。
                unsafe {
                    *p = header(kind, 0);
                    *p.add(1) = i as u64;
                }
                Word::from_ptr(p)
            })
        } else {
            self.index_box(kind, i)
        };
        self.gc.unboxed = None;
        match r {
            Ok(w) => Ok(w),
            Err(e) => {
                table(self)[i] = dead();
                self.gc.tables[which as usize].release(i);
                Err(e)
            }
        }
    }

    /// ハンドラ記録を作る。起動の番号を振る。
    pub fn handler_new(&mut self, mut rec: HandlerRec) -> Result<Word, Stop> {
        rec.id = self.next_handler;
        let w = self.table_push(
            |r| &mut r.handlers,
            Table::Handler,
            Kind::Handler,
            HandlerRec::dead,
            rec,
            0,
            false,
        )?;
        self.next_handler += 1;
        Ok(w)
    }

    /// ハンドラ記録の箱の添字。ハンドラ記録でなければ内部の誤り。
    pub fn hr_index(&self, w: Word) -> Result<usize, Stop> {
        if self.is_kind(w, Kind::Handler) {
            Ok(Self::index_of(w))
        } else {
            Err(Stop::Internal(format!(
                "ハンドラ記録でない値: {}",
                String::from_utf8_lossy(&self.show(w))
            )))
        }
    }

    /// ハンドラ記録。
    pub fn hr(&self, w: Word) -> Result<&HandlerRec, Stop> {
        let i = self.hr_index(w)?;
        Ok(&self.handlers[i])
    }

    /// 書き換えるハンドラ記録。
    pub fn hr_mut(&mut self, w: Word) -> Result<&mut HandlerRec, Stop> {
        let i = self.hr_index(w)?;
        Ok(&mut self.handlers[i])
    }

    /// 継続を作る。
    pub fn cont_new(&mut self, c: Cont) -> Result<Word, Stop> {
        self.table_push(
            |r| &mut r.conts,
            Table::Cont,
            Kind::Cont,
            Cont::default,
            c,
            0,
            false,
        )
    }

    /// 継続の箱の添字。継続でなければ内部の誤り。
    pub fn cont_index(&self, w: Word) -> Result<usize, Stop> {
        if self.is_kind(w, Kind::Cont) {
            Ok(Self::index_of(w))
        } else {
            Err(Stop::Internal("継続でない値".into()))
        }
    }

    /// 状態語を退避した値にする。
    ///
    /// 表はいつも 1 個分の項目を空けておく(`Rt::new` で取り、退避のたびに取り直す)。
    /// メモリの不足の状態語(`ERROR(OutOfMemory)`)は、空けておいた項目に置き、表を広げない。
    /// 箱の確保でも実行の役割の印を見ずに下ろす。
    /// 印はいま巻き戻しているメモリの不足と同じ不足を表し、ここで失敗すると状態語が失われて `cancel` 節が走らないからである。
    /// 置いた後で空きを取り直せなくても、この退避は失敗にしない。
    /// ほかの状態語は、置いた後の空きまで先に取り、取れなければメモリの不足を返す。
    /// 表を広げる確保と空きを取り直す確保は、場所 `pending` で行う([`Site::Pending`]。`Rt::new` の最初の空きは除く)。
    pub fn pending_box(&mut self, p: Pending) -> Result<Word, Stop> {
        let urgent = matches!(p, Pending::Error(Stop::OutOfMemory));
        let w = self.table_push(
            |r| &mut r.saved,
            Table::Pending,
            Kind::Pending,
            Pending::default,
            p,
            usize::from(!urgent),
            urgent,
        )?;
        if urgent {
            let _site = alloc::site(Site::Pending);
            let _ = self.keep_pending_spare();
        }
        Ok(w)
    }

    /// 退避した状態語の表に 1 個分の空きを取る(失敗を自分で扱う範囲で)。
    fn keep_pending_spare(&mut self) -> Result<(), Stop> {
        alloc::handled(|| self.saved.try_reserve(1)).map_err(|_| Stop::OutOfMemory)?;
        self.gc.tables[Table::Pending as usize]
            .keep_spare(1)
            .map_err(|_| Stop::OutOfMemory)
    }

    /// 退避した値から状態語を取り出す(表の項目は空にする)。
    pub fn pending_unbox(&mut self, w: Word) -> Result<Pending, Stop> {
        if !self.is_kind(w, Kind::Pending) {
            return Err(Stop::Internal("退避した状態語でない値".into()));
        }
        Ok(std::mem::take(&mut self.saved[Self::index_of(w)]))
    }

    /// `depth_limit` の誤りを作り、限度を予備の分だけ広げる(IRSpec 11.5 節)。
    pub fn depth_error(&mut self) -> Stop {
        self.limit = self.max_depth + crate::reserve(self.max_depth);
        Stop::DepthLimit(self.max_depth)
    }

    /// 実行時エラーを状態語にする。巻き戻さずに止まる誤りはそのまま返す。
    pub fn raise(&mut self, s: Stop) -> Result<(), Stop> {
        match s {
            Stop::Pending => Ok(()),
            s if s.is_fatal() => Err(s),
            s => {
                self.pending = Pending::Error(s);
                Ok(())
            }
        }
    }

    // ---- 診断 ----

    /// 付録 B の 2 番の文面。
    pub fn unbound(&self, g: GlobalId) -> Stop {
        Stop::runtime(format!(
            "未束縛の変数: {}",
            self.prog.globals[g.index()].display
        ))
    }

    /// 付録 B の `<値>`(Diktor の第 12 章の `show`)。
    ///
    /// 深い値でもネイティブのスタックを使わないよう、明示の作業の列でたどる。
    /// レコードのフィールドは正準の配置(ラベルの番号の昇順、同名は外側から)の順に書く。
    /// 付録 B は、ラベルの違うフィールドどうしの順を読み手に任せ、同じラベルのフィールドの順(外側から)だけを決める。
    /// そのため、Diktor(値を作った順に書く)と綴りが違っても仕様に合う。
    /// ただし、順は後段の間で同じでなければならないので、host の `show`(`host/keleut-host.mjs`)も同じ順で書く。
    /// 文面は LangSpec §18 の比べる対象の外である。
    pub fn show(&self, w: Word) -> Vec<u8> {
        enum Item {
            V(Word),
            T(&'static str),
            S(String),
        }
        let p = self.prog;
        let mut out = Vec::new();
        let mut work = vec![Item::V(w)];
        // 列の部品を逆順に積む。
        fn push_seq(work: &mut Vec<Item>, open: Item, xs: Vec<Word>, close: &'static str) {
            work.push(Item::T(close));
            for (i, x) in xs.into_iter().enumerate().rev() {
                work.push(Item::V(x));
                if i > 0 {
                    work.push(Item::T(", "));
                }
            }
            work.push(open);
        }
        while let Some(it) = work.pop() {
            let w = match it {
                Item::T(s) => {
                    out.extend_from_slice(s.as_bytes());
                    continue;
                }
                Item::S(s) => {
                    out.extend_from_slice(s.as_bytes());
                    continue;
                }
                Item::V(w) => w,
            };
            if w.is_i32() {
                out.extend_from_slice(w.as_i32().to_string().as_bytes());
                continue;
            }
            if let Some(k) = w.imm_kind() {
                match k {
                    ImmKind::Unit => out.extend_from_slice(b"()"),
                    ImmKind::Bool => {
                        out.extend_from_slice(if w.as_bool() { b"true" } else { b"false" })
                    }
                    ImmKind::Ctor0 => {
                        let (t, c) = w.payload_pair();
                        out.extend_from_slice(self.ctor_name(t, c).as_bytes());
                    }
                    ImmKind::DictBuiltin | ImmKind::DictInst0 => out.extend_from_slice(b"<dict>"),
                }
                continue;
            }
            if !w.is_ptr() {
                out.extend_from_slice(b"<?>");
                continue;
            }
            // SAFETY: w は生きている箱を指す。
            let (kind, n) = unsafe { (value::kind_of(w), value::count_of(w) as usize) };
            // SAFETY: 各分岐は箱の種類の配置のとおりに読む。
            let at = |i: usize| Word(unsafe { value::get(w, i) });
            match kind {
                Some(Kind::I64) => {
                    out.extend_from_slice((self.unbox_word(w) as i64).to_string().as_bytes())
                }
                Some(Kind::F64) => out.extend_from_slice(
                    keleut_rtcore::show_f64(f64::from_bits(self.unbox_word(w))).as_bytes(),
                ),
                Some(Kind::Str) => {
                    out.push(b'"');
                    escape_into(&mut out, self.str_bytes(w));
                    out.push(b'"');
                }
                Some(Kind::Record) => {
                    let shape = at(1).0 as u32;
                    let labels = self.shapes.labels(RtShape(shape));
                    let fields: Vec<Word> = (0..n).map(|i| at(2 + i)).collect();
                    let tuple = labels.iter().all(|&l| p.labels[l as usize] == "_item");
                    if tuple {
                        let close = if n == 1 { ",)" } else { ")" };
                        push_seq(&mut work, Item::T("("), fields, close);
                    } else {
                        work.push(Item::T("}"));
                        for i in (0..n).rev() {
                            work.push(Item::V(fields[i]));
                            let l = display_name(&p.labels[labels[i] as usize]);
                            let sep = if i > 0 { ", " } else { "" };
                            work.push(Item::S(format!("{sep}{l} = ")));
                        }
                        work.push(Item::T("{"));
                    }
                }
                Some(Kind::Variant) => {
                    let l = display_name(&p.labels[at(1).0 as usize]);
                    let payload = at(2);
                    if payload == Word::UNIT {
                        out.extend_from_slice(format!("#{l}").as_bytes());
                    } else {
                        push_seq(&mut work, Item::S(format!("#{l}(")), vec![payload], ")");
                    }
                }
                Some(Kind::Data) => {
                    let tc = at(1).0;
                    let name = self.ctor_name((tc >> 32) as u32, tc as u32).to_string();
                    let fields = (0..n).map(|i| at(2 + i)).collect();
                    push_seq(&mut work, Item::S(format!("{name}(")), fields, ")");
                }
                Some(Kind::Closure) => {
                    let f = self.closure_func(w);
                    match &self.func_prim_name[f.index()] {
                        Some(name) => out.extend_from_slice(format!("<prim {name}>").as_bytes()),
                        None => out.extend_from_slice(b"<fn>"),
                    }
                }
                Some(Kind::Method) => {
                    let (c, m, _, _) = self.method_parts(w);
                    let ci = &p.classes[c.index()];
                    out.extend_from_slice(
                        format!(
                            "<prim {}.{}>",
                            display_of_symbol(&ci.sym),
                            ci.methods[m as usize]
                        )
                        .as_bytes(),
                    );
                }
                Some(Kind::ExternUnimpl) => {
                    let g = at(1).0 as usize;
                    out.extend_from_slice(format!("<prim {}>", p.globals[g].display).as_bytes());
                }
                Some(Kind::Ref) => out.extend_from_slice(b"<ref>"),
                Some(Kind::Array) => {
                    let xs = (0..n).map(|i| at(1 + i)).collect();
                    push_seq(&mut work, Item::T("["), xs, "]");
                }
                Some(Kind::MArray) => {
                    let xs = (0..n).map(|i| at(1 + i)).collect();
                    push_seq(&mut work, Item::T("<mutable ["), xs, "]>");
                }
                Some(Kind::DictInst | Kind::DictRecord | Kind::DictVariant) => {
                    out.extend_from_slice(b"<dict>")
                }
                Some(Kind::Handler | Kind::Cont | Kind::Pending) => out.extend_from_slice(b"<?>"),
                None => out.extend_from_slice(b"<?>"),
            }
        }
        out
    }

    fn ctor_name(&self, t: u32, c: u32) -> &str {
        display_name(&self.prog.types[t as usize].ctors[c as usize].name)
    }

    /// 関数の種類。
    pub fn func_kind(&self, f: FuncId) -> FuncKind {
        self.prog.funcs[f.index()].kind
    }
}

/// OCaml の `String.escaped` と同じ規則で、バイト列を書く。
pub fn escape_into(out: &mut Vec<u8>, s: &[u8]) {
    for &b in s {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\r' => out.extend_from_slice(b"\\r"),
            8 => out.extend_from_slice(b"\\b"),
            0x20..=0x7e => out.push(b),
            _ => out.extend_from_slice(format!("\\{b:03}").as_bytes()),
        }
    }
}

/// 辞書の種類。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DictView {
    /// 組み込みのインスタンス。
    Builtin(ClassId, TypeId),
    /// 利用者のインスタンスと前提の辞書。
    Inst(InstanceId, Vec<Word>),
    /// 構造的な `Eq` のレコードの辞書。
    Record(Vec<(LabelId, Word)>),
    /// 構造的な `Eq` のヴァリアントの辞書。
    Variant(Vec<(LabelId, Word)>),
    /// 辞書でない値(前提が破られた)。
    Other,
}

/// 呼べる値の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalleeView {
    /// クロージャ(組み込みの値と `extern` の本体を含む)。
    Closure(FuncId),
    /// メソッドの値。
    Method,
    /// 実装の無い `extern` の値。
    ExternUnimpl(GlobalId),
    /// 呼べない値(前提が破られた)。
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名前の表示() {
        assert_eq!(display_of_symbol("main/M.__no_such"), "M.__no_such");
        assert_eq!(display_of_symbol("main/__no_such#1"), "__no_such");
        assert_eq!(display_of_symbol("lib/x.kel/f#x"), "f#x");
        assert_eq!(display_name("%3.Foo"), "Foo");
        assert_eq!(display_name("Foo"), "Foo");
    }

    #[test]
    fn 文字列の印字は_ocaml_の規則で書く() {
        let mut v = Vec::new();
        escape_into(&mut v, "a\"\\\n\t\r\x08\x00'é".as_bytes());
        assert_eq!(
            String::from_utf8(v).unwrap(),
            "a\\\"\\\\\\n\\t\\r\\b\\000'\\195\\169"
        );
    }
}
