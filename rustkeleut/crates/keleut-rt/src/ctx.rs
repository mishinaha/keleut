//! ランタイムコンテキスト [`Rt`] と、値を作る操作、値を読む操作を定義する。
//! 各操作の意味は、`doc/lir.md` の命令の説明が定める。
//!
//! [`Rt`] は、ヒープ、ルートスタック、分割値スタック、標準出力、シェイプ表、深さとその上限、グローバルとインスタンスの状態、文字列定数、GC の状態を持つ。
//!
//! オブジェクトを作る関数はどれも [`Rt::alloc`] で確保するので、その中で GC が起きることがある([`crate::gc`])。
//! 引数で受け取ったワードは、バックエンドのルート(フレームの安全点の集合など)が保持しているものとして扱う。
//! 1 つの関数の中で 2 回以上確保するときは、先に作ったオブジェクトをルートに置いてから次を確保する。
//! バックエンド(インタプリタと LLVM の JIT)は、LIR の命令のうちランタイムに任せるものを、このモジュールの関数で実行する。

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

/// 論理的な深さの上限 `L` の既定値 `2^20`(IRSpec 11.5 節)。
pub const DEFAULT_MAX_DEPTH: u64 = 1 << 20;

/// ランタイムコンテキスト。実行中のプログラムの状態をまとめて持つ。
pub struct Rt<'p> {
    /// 実行する LIR のプログラム。
    pub prog: &'p Program,
    /// オブジェクトを置くヒープ。
    pub heap: Heap,
    /// Rust で書いたランタイム関数が値を保持するルートスタック。
    pub roots: Roots,
    /// 分割値スタック。
    pub stack: ValueStack,
    /// 標準出力。
    pub out: Out,
    /// レコードのシェイプ表。
    pub shapes: ShapeTable,
    /// コンテキストの深さ `ctx.depth`。
    pub depth: u64,
    /// 現在の深さの上限。通常は `L` で、`depth_limit` の伝播中は `L + R` になる(IRSpec 11.5 節)。
    pub limit: u64,
    /// 深さの上限 `L`。
    pub max_depth: u64,
    /// ハンドラ記録の表。オブジェクト [`Kind::Handler`] の 1 番のワードが添字である。
    pub handlers: Vec<HandlerRec>,
    /// 継続の表。オブジェクト [`Kind::Cont`] の 1 番のワードが添字である。
    pub conts: Vec<Cont>,
    /// 退避した状態ワードの表。オブジェクト [`Kind::Pending`] の 1 番のワードが添字である。
    pub saved: Vec<Pending>,
    /// 有効なハンドラの連鎖 `ctx.handlers` の先頭。なければ Unit である。
    pub chain: Word,
    /// 状態ワード `ctx.pending`。
    pub pending: Pending,
    /// 次に作るハンドラ記録に振る、ハンドラの起動(activation)の番号。
    pub next_handler: u64,
    /// グローバルの値。
    pub globals: Vec<Word>,
    /// グローバルごとの初期化済みフラグ。
    pub global_init: Vec<bool>,
    /// インスタンスごとの初期化済みフラグ。
    pub inst_init: Vec<bool>,
    /// `inst_set_value` が記録した値。インスタンスごと、メソッドの位置ごとに持つ。
    pub inst_values: Vec<Vec<Word>>,
    /// 文字列定数のオブジェクト。
    pub strings: Vec<Word>,
    /// 関数ごとの、`<prim …>` と表示するときの名前。組み込みの値の関数と `extern` の本体の関数だけが持つ。
    pub func_prim_name: Vec<Option<String>>,
    /// 関数ごとのフラグ。`extern` のグローバルの本体である関数なら真である。値が `extern` のグローバルの値かどうかの判別に使う([`Rt::is_extern_value`])。
    pub extern_funcs: Vec<bool>,
    /// GC の状態。
    pub gc: GcState,
}

/// シンボルから、単位のキー(最後の `/` まで)と末尾の `#n`(`n` は 10 進の数字の並び)を取り除いた名前を返す(IRSpec 3.1 節)。
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

/// ラベルやコンストラクタの名前から、表示に使う名前を返す(Diktor の `Type.display` と同じ規則)。
///
/// `%` で始まる内部名は、最初の `.` より後ろの部分を返す。
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
    /// プログラム `prog` のコンテキストを作る。`limit` は深さの上限 `L` である。
    ///
    /// シェイプ表にプログラムのシェイプを順に登録し、番号が LIR のシェイプ表の添字と一致することを確かめる(`doc/lir.md` の 1.5 節)。
    /// 一致しなければ内部エラーを返す。
    /// シェイプ表を確保できないか拡張できなければ、評価に必要なメモリを用意できないので [`Stop::OutOfMemory`] を返す。
    /// `keleut run` の呼び出し元(`keleut-interp` の `run_mutator`)は、これを初期化の開始時の実行時エラーとして書き、終了コード 3 で終了する(LangSpec §15.5)。
    /// 文字列定数と、関数を値とするグローバルのクロージャは、ここで先に確保する。
    /// 作ったオブジェクトはすぐに表に入れ、表が GC ルートになるので、その間の GC で失われない。
    pub fn new(prog: &'p Program, limit: u64) -> Result<Rt<'p>, Stop> {
        Self::new_with(prog, limit, false)
    }

    /// [`Rt::new`] と同じだが、`stress` が真なら `--gc-stress` の規則で GC を実行する([`crate::gc::STRESS_RATIO`])。
    pub fn new_with(prog: &'p Program, limit: u64, stress: bool) -> Result<Rt<'p>, Stop> {
        let mut shapes = {
            let _site = alloc::site(Site::Shape);
            alloc::handled(ShapeTable::new)?
        };
        let mut pos = Vec::new();
        for (i, s) in prog.shapes.iter().enumerate() {
            let labels: Vec<u32> = s.iter().map(|l| l.0).collect();
            let id = {
                let _site = alloc::site(Site::Shape);
                alloc::handled(|| shapes.intern(&labels, &mut pos))?
            };
            if id.0 as usize != i {
                return Err(Stop::Internal(format!(
                    "シェイプ表の番号が合いません(LIR の {i} 番がランタイムの {} 番)",
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
                    // SAFETY: p は 2 ワードの新しい領域である。
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

    /// ヘッダを書いたオブジェクトを確保する。残りのワードは、呼び出し元が次の確保の前に書く。
    fn alloc_obj(&mut self, kind: Kind, n: u32, words: usize) -> Result<*mut u64, Stop> {
        let p = self.alloc(words)?;
        // SAFETY: p は words ワード(1 以上)の新しい領域である。
        unsafe { *p = header(kind, n) };
        Ok(p)
    }

    // ---- 数 ----

    /// `Int64` の値 `n` をボックス化する。
    pub fn box_i64(&mut self, n: i64) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::I64, 0, 2)?;
        // SAFETY: p は 2 ワードの新しいオブジェクトである。
        unsafe { *p.add(1) = n as u64 };
        Ok(Word::from_ptr(p))
    }

    /// `Float64` の値(ビット表現 `bits`)をボックス化する。
    pub fn box_f64(&mut self, bits: u64) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::F64, 0, 2)?;
        // SAFETY: p は 2 ワードの新しいオブジェクトである。
        unsafe { *p.add(1) = bits };
        Ok(Word::from_ptr(p))
    }

    /// ボックスの 1 番のワード(`Int64` か `Float64` の値)を返す。
    ///
    /// 事前条件:`w` は `Int64` か `Float64` のボックスである(`doc/lir.md` の `unbox`)。
    #[inline]
    pub fn unbox_word(&self, w: Word) -> u64 {
        // SAFETY: 事前条件により w は 2 ワードのオブジェクトである。
        unsafe { value::get(w, 1) }
    }

    // ---- 文字列 ----

    /// バイト列 `s` を内容とする文字列を作る。長さが `N_alloc_max` を超えれば、メモリ不足のエラーを返す。
    pub fn new_str(&mut self, s: &[u8]) -> Result<Word, Stop> {
        let p = self.alloc_str(s.len())?;
        // SAFETY: p は長さ s.len() の新しい文字列オブジェクトである。
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

    /// 文字列のバイト列を返す。
    ///
    /// 事前条件:`w` は文字列である。
    #[inline]
    pub fn str_bytes(&self, w: Word) -> &[u8] {
        // SAFETY: 事前条件により w は生きている文字列オブジェクトで、オブジェクトは移動しない。
        // 借用の間は self を変更できないので、確保も GC も起きない。
        unsafe { value::str_bytes(w) }
    }

    /// 2 つの文字列を連結した文字列を作る。
    pub fn str_concat(&mut self, a: Word, b: Word) -> Result<Word, Stop> {
        let (la, lb) = (self.str_bytes(a).len(), self.str_bytes(b).len());
        let p = self.alloc_str(la.checked_add(lb).ok_or(Stop::OutOfMemory)?)?;
        // SAFETY: p は la + lb バイトの新しい文字列オブジェクトで、a と b は生きている文字列である。
        unsafe {
            let d = p.add(1).cast::<u8>();
            std::ptr::copy_nonoverlapping(a.ptr().add(1).cast::<u8>(), d, la);
            std::ptr::copy_nonoverlapping(b.ptr().add(1).cast::<u8>(), d.add(la), lb);
        }
        Ok(Word::from_ptr(p))
    }

    /// バイト位置 `pos` から `len` バイトの部分文字列を作る。どの入力にも結果を定めていて、範囲外の部分は切り詰める。
    pub fn str_sub(&mut self, s: Word, pos: i32, len: i32) -> Result<Word, Stop> {
        let n = self.str_bytes(s).len() as i64;
        let start = i64::from(pos).clamp(0, n);
        let end = (start + i64::from(len).clamp(0, n)).min(n);
        let (start, end) = (start as usize, end as usize);
        let p = self.alloc_str(end - start)?;
        // SAFETY: 範囲は s の中で、p は新しいオブジェクトである。
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

    /// 名目的な型の値を作る。`fields` が空なら即値を返す。
    pub fn ctor_new(&mut self, ty: TypeId, ctor: u32, fields: &[Word]) -> Result<Word, Stop> {
        if fields.is_empty() {
            return Ok(Word::ctor0(ty.0, ctor));
        }
        let n = fields.len();
        let p = self.alloc_obj(Kind::Data, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n ワードの新しいオブジェクトである。
        unsafe {
            *p.add(1) = (u64::from(ty.0) << 32) | u64::from(ctor);
            for (i, f) in fields.iter().enumerate() {
                *p.add(2 + i) = f.0;
            }
        }
        Ok(Word::from_ptr(p))
    }

    /// コンストラクタの番号を返す。事前条件:`v` は名目的な型の値である。
    #[inline]
    pub fn ctor_tag(&self, v: Word) -> i32 {
        if v.is_imm() {
            v.payload_pair().1 as i32
        } else {
            // SAFETY: 事前条件により v はデータのオブジェクトである。
            unsafe { value::get(v, 1) as u32 as i32 }
        }
    }

    /// `i` 番目のフィールドを返す。どの `i` にも結果を定めていて、フィールドがなければ Unit を返す。
    ///
    /// 事前条件:`v` は名目的な型の値である。
    #[inline]
    pub fn ctor_field(&self, v: Word, i: u32) -> Word {
        if v.is_imm() {
            return Word::UNIT;
        }
        // SAFETY: 事前条件により v はデータのオブジェクトである。i が要素数より小さいことを確かめてから読む。
        unsafe {
            if i < value::count_of(v) {
                Word(value::get(v, 2 + i as usize))
            } else {
                Word::UNIT
            }
        }
    }

    /// ラベル `label` とペイロード `payload` からヴァリアントを作る。
    pub fn variant_new(&mut self, label: LabelId, payload: Word) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::Variant, 0, 3)?;
        // SAFETY: p は 3 ワードの新しいオブジェクトである。
        unsafe {
            *p.add(1) = u64::from(label.0);
            *p.add(2) = payload.0;
        }
        Ok(Word::from_ptr(p))
    }

    /// ヴァリアントのラベルの番号を返す。事前条件:`v` はヴァリアントである。
    #[inline]
    pub fn variant_tag(&self, v: Word) -> i32 {
        // SAFETY: 事前条件により v はヴァリアントのオブジェクトである。
        unsafe { value::get(v, 1) as i32 }
    }

    /// ヴァリアントのペイロードを返す。事前条件:`v` はヴァリアントである。
    #[inline]
    pub fn variant_payload(&self, v: Word) -> Word {
        // SAFETY: 事前条件により v はヴァリアントのオブジェクトである。
        Word(unsafe { value::get(v, 2) })
    }

    // ---- レコード ----

    /// シェイプとフィールドからレコードを作る。フィールドがなければ Unit を返す。
    pub fn rec_new(&mut self, shape: ShapeId, fields: &[Word]) -> Result<Word, Stop> {
        if fields.is_empty() {
            return Ok(Word::UNIT);
        }
        let n = fields.len();
        let p = self.alloc_obj(Kind::Record, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n ワードの新しいオブジェクトである。
        unsafe {
            *p.add(1) = u64::from(shape.0);
            std::ptr::copy_nonoverlapping(fields.as_ptr().cast::<u64>(), p.add(2), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// レコードのシェイプの番号とフィールドの個数を返す。Unit は空のシェイプ(0 番)として扱う。
    ///
    /// 事前条件:`v` はレコードか Unit である。
    #[inline]
    pub fn rec_shape(&self, v: Word) -> (u32, u32) {
        if v.is_imm() {
            return (0, 0);
        }
        // SAFETY: 事前条件により v はレコードのオブジェクトである。
        unsafe { (value::get(v, 1) as u32, value::count_of(v)) }
    }

    /// レコードの `i` 番目のフィールドを返す。どの `i` にも結果を定めていて、フィールドがなければ Unit を返す。
    #[inline]
    pub fn rec_get(&self, v: Word, i: u32) -> Word {
        let (_, n) = self.rec_shape(v);
        if i < n {
            // SAFETY: v はレコードのオブジェクトで、i は要素数より小さい。
            Word(unsafe { value::get(v, 2 + i as usize) })
        } else {
            Word::UNIT
        }
    }

    /// ラベル `label` を持つフィールドのうち、外側から 0 始まりで `nth` 番目のフィールドの位置を返す。なければ `None` を返す。
    fn rec_pos(&self, v: Word, label: LabelId, nth: u32) -> Option<u32> {
        let (shape, _) = self.rec_shape(v);
        let first = self.shapes.find(RtShape(shape), label.0)?;
        let labels = self.shapes.labels(RtShape(shape));
        let p = first.checked_add(nth)?;
        (labels.get(p as usize) == Some(&label.0)).then_some(p)
    }

    /// 命令 `rec_field` を実行する。`label` のフィールドのうち外側から `nth` 番目の値を返し、なければ Unit を返す。
    pub fn rec_field(&self, v: Word, label: LabelId, nth: u32) -> Word {
        match self.rec_pos(v, label, nth) {
            Some(p) => self.rec_get(v, p),
            None => Word::UNIT,
        }
    }

    /// 命令 `rec_has` を実行する。`label` のフィールドのうち外側から `nth` 番目があれば真を返す。
    pub fn rec_has(&self, v: Word, label: LabelId, nth: u32) -> bool {
        self.rec_pos(v, label, nth).is_some()
    }

    /// レコードのフィールドをコピーした `Vec` を作る。フィールドを 1 つ追加できる容量で確保する。
    ///
    /// コピー先は、確保の失敗を呼び出し元が処理する範囲(`crate::alloc::handled`。以下、自己処理の範囲)で確保する。
    /// 確保できなければ、メモリ不足のエラーを返す。
    fn rec_fields(&self, v: Word) -> Result<Vec<Word>, Stop> {
        let (_, n) = self.rec_shape(v);
        let _site = alloc::site(Site::RecFields);
        let mut fs = Vec::new();
        alloc::handled(|| fs.try_reserve_exact(n as usize + 1)).map_err(|_| Stop::OutOfMemory)?;
        fs.extend((0..n).map(|i| self.rec_get(v, i)));
        Ok(fs)
    }

    /// 命令 `rec_extend` を実行する。`v` の外側にフィールド `label = x` を追加したレコードを作る。
    ///
    /// シェイプ表を拡張できなければ [`Stop::OutOfMemory`] を返す。
    /// シェイプ表の確保は自己処理の範囲(`crate::alloc::handled`)で行い、グローバルアロケータの予備領域を `cancel` 節のために残す。
    pub fn rec_extend(&mut self, v: Word, label: LabelId, x: Word) -> Result<Word, Stop> {
        let (shape, _) = self.rec_shape(v);
        let (ns, pos) = {
            let _site = alloc::site(Site::Shape);
            alloc::handled(|| self.shapes.extend(RtShape(shape), label.0))?
        };
        let mut fs = self.rec_fields(v)?;
        fs.insert(pos as usize, x);
        self.rec_new(ShapeId(ns.0), &fs)
    }

    /// 命令 `rec_restrict` を実行する。`label` の最も外側のフィールドを取り除いたレコードを作る。
    ///
    /// どの入力にも結果を定めていて、`label` のフィールドがなければ、同じフィールドを持つレコードを作る。
    /// シェイプ表は [`Rt::rec_extend`] と同じく自己処理の範囲で拡張し、拡張できなければ [`Stop::OutOfMemory`] を返す。
    pub fn rec_restrict(&mut self, v: Word, label: LabelId) -> Result<Word, Stop> {
        let (shape, _) = self.rec_shape(v);
        let mut fs = self.rec_fields(v)?;
        let restricted = {
            let _site = alloc::site(Site::Shape);
            alloc::handled(|| self.shapes.restrict(RtShape(shape), label.0))?
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

    /// 命令 `rec_update` を実行する。`label` の最も外側のフィールドを `x` に置き換えたレコードを作る。
    ///
    /// どの入力にも結果を定めていて、`label` のフィールドがなければ、同じフィールドを持つレコードを作る。
    pub fn rec_update(&mut self, v: Word, label: LabelId, x: Word) -> Result<Word, Stop> {
        let (shape, _) = self.rec_shape(v);
        let mut fs = self.rec_fields(v)?;
        if let Some(p) = self.shapes.find(RtShape(shape), label.0) {
            fs[p as usize] = x;
        }
        self.rec_new(ShapeId(shape), &fs)
    }

    // ---- 関数の値と辞書 ----

    /// 関数 `func` とキャプチャ `caps` からクロージャを作る。
    pub fn closure_new(&mut self, func: FuncId, caps: &[Word]) -> Result<Word, Stop> {
        let n = caps.len();
        let p = self.alloc_obj(Kind::Closure, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n ワードの新しいオブジェクトである。
        unsafe {
            *p.add(1) = u64::from(func.0);
            std::ptr::copy_nonoverlapping(caps.as_ptr().cast::<u64>(), p.add(2), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// クロージャの関数を返す。事前条件:`c` はクロージャである。
    #[inline]
    pub fn closure_func(&self, c: Word) -> FuncId {
        // SAFETY: 事前条件により c はクロージャのオブジェクトである。
        FuncId(unsafe { value::get(c, 1) } as u32)
    }

    /// クロージャの `i` 番目のキャプチャを返す。
    ///
    /// 事前条件:`c` はクロージャで、`i` はキャプチャの個数より小さい。
    #[inline]
    pub fn closure_cap(&self, c: Word, i: u32) -> Word {
        // SAFETY: 事前条件により c はクロージャのオブジェクトで、i はキャプチャの個数より小さい(検証器が検査する)。
        Word(unsafe { value::get(c, 2 + i as usize) })
    }

    /// クロージャの `i` 番目のキャプチャに `x` を書く。事前条件は [`Rt::closure_cap`] と同じである。
    #[inline]
    pub fn closure_set_cap(&mut self, c: Word, i: u32, x: Word) {
        // SAFETY: closure_cap と同じ。
        unsafe { value::set(c, 2 + i as usize, x.0) }
    }

    /// クラス `class` の `method` 番目のメソッドの値を、辞書 `dict` とメソッド自身の辞書 `own` から作る。
    pub fn method_new(
        &mut self,
        class: ClassId,
        method: u32,
        dict: Word,
        own: &[Word],
    ) -> Result<Word, Stop> {
        let n = own.len();
        let p = self.alloc_obj(Kind::Method, n as u32, 3 + n)?;
        // SAFETY: p は 3 + n ワードの新しいオブジェクトである。
        unsafe {
            *p.add(1) = (u64::from(class.0) << 32) | u64::from(method);
            *p.add(2) = dict.0;
            std::ptr::copy_nonoverlapping(own.as_ptr().cast::<u64>(), p.add(3), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// メソッドの値の内容(クラス、メソッドの番号、辞書、メソッド自身の辞書)を返す。
    ///
    /// 事前条件:`m` はメソッドの値である。
    pub fn method_parts(&self, m: Word) -> (ClassId, u32, Word, Vec<Word>) {
        // SAFETY: 事前条件により m はメソッドの値のオブジェクトである。
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

    /// ユーザ定義の前提付きインスタンス `inst` の辞書を、前提の辞書 `premises` から作る。
    pub fn dict_inst(&mut self, inst: InstanceId, premises: &[Word]) -> Result<Word, Stop> {
        let n = premises.len();
        let p = self.alloc_obj(Kind::DictInst, n as u32, 2 + n)?;
        // SAFETY: p は 2 + n ワードの新しいオブジェクトである。
        unsafe {
            *p.add(1) = u64::from(inst.0);
            std::ptr::copy_nonoverlapping(premises.as_ptr().cast::<u64>(), p.add(2), n);
        }
        Ok(Word::from_ptr(p))
    }

    /// 構造的な `Eq` の辞書を作る。`kind` はレコードの辞書かヴァリアントの辞書かを表し、`fields` はラベルと辞書の組の並びである。
    pub fn dict_struct(&mut self, kind: Kind, fields: &[(LabelId, Word)]) -> Result<Word, Stop> {
        let n = fields.len();
        let p = self.alloc_obj(kind, n as u32, 1 + 2 * n)?;
        // SAFETY: p は 1 + 2n ワードの新しいオブジェクトである。
        unsafe {
            for (i, (l, d)) in fields.iter().enumerate() {
                *p.add(1 + 2 * i) = u64::from(l.0);
                *p.add(2 + 2 * i) = d.0;
            }
        }
        Ok(Word::from_ptr(p))
    }

    /// 辞書 `d` の種類と内容を読む。
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
        // SAFETY: 事前条件により d は辞書のオブジェクトである。
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

    /// 値 `f` が、呼び出せる値のうちのどの種類であるかを調べる。呼び出せない値なら [`CalleeView::Other`] を返す。
    pub fn callee_view(&self, f: Word) -> CalleeView {
        if !f.is_ptr() {
            return CalleeView::Other;
        }
        // SAFETY: f はオブジェクトへのポインタである。
        match unsafe { value::kind_of(f) } {
            Some(Kind::Closure) => CalleeView::Closure(self.closure_func(f)),
            Some(Kind::Method) => CalleeView::Method,
            // SAFETY: 実装のない extern のオブジェクトの 1 番のワードは、グローバルの番号である。
            Some(Kind::ExternUnimpl) => {
                CalleeView::ExternUnimpl(GlobalId(unsafe { value::get(f, 1) } as u32))
            }
            _ => CalleeView::Other,
        }
    }

    /// 値 `f` が `extern` のグローバルの値であれば真を返す。
    ///
    /// `dict_app` は、このような値に対しては辞書を捨て、値そのものを返す。
    pub fn is_extern_value(&self, f: Word) -> bool {
        match self.callee_view(f) {
            CalleeView::ExternUnimpl(_) => true,
            CalleeView::Closure(func) => self.extern_funcs[func.index()],
            _ => false,
        }
    }

    // ---- 組み込みの値 ----

    /// `Ref.new` を実行する。`x` を内容とする可変参照を作る。
    pub fn ref_new(&mut self, x: Word) -> Result<Word, Stop> {
        let p = self.alloc_obj(Kind::Ref, 0, 2)?;
        // SAFETY: p は 2 ワードの新しいオブジェクトである。
        unsafe { *p.add(1) = x.0 };
        Ok(Word::from_ptr(p))
    }

    /// 長さ `n`(0 以上)の配列を作り、すべての要素を `x` にする。`n` が `N_alloc_max` を超えれば、メモリ不足のエラーを返す。
    pub fn array_new(&mut self, kind: Kind, n: usize, x: Word) -> Result<Word, Stop> {
        if n > N_ALLOC_MAX as usize {
            return Err(Stop::OutOfMemory);
        }
        let p = self.alloc_obj(kind, n as u32, 1 + n)?;
        // SAFETY: p は 1 + n ワードの新しいオブジェクトである。
        unsafe {
            for i in 0..n {
                *p.add(1 + i) = x.0;
            }
        }
        Ok(Word::from_ptr(p))
    }

    /// 配列の長さを返す。事前条件:`a` は配列である。
    #[inline]
    pub fn array_len(&self, a: Word) -> i32 {
        // SAFETY: 事前条件により a は配列のオブジェクトである。
        unsafe { value::count_of(a) as i32 }
    }

    /// 配列の `i` 番目の要素を返す。どの `i` にも結果を定めていて、範囲外なら Unit を返す。
    #[inline]
    pub fn array_get(&self, a: Word, i: i32) -> Word {
        if i < 0 || i >= self.array_len(a) {
            return Word::UNIT;
        }
        // SAFETY: i は範囲内である。
        Word(unsafe { value::get(a, 1 + i as usize) })
    }

    /// 可変配列の `i` 番目の要素に `x` を書く。どの `i` にも動作を定めていて、範囲外なら何もしない。
    #[inline]
    pub fn array_set(&mut self, a: Word, i: i32, x: Word) {
        if i >= 0 && i < self.array_len(a) {
            // SAFETY: i は範囲内である。
            unsafe { value::set(a, 1 + i as usize, x.0) }
        }
    }

    /// 可変配列の要素をコピーした不変配列(`Array`)を作る。
    pub fn array_freeze(&mut self, a: Word) -> Result<Word, Stop> {
        let n = self.array_len(a) as usize;
        let p = self.alloc_obj(Kind::Array, n as u32, 1 + n)?;
        // SAFETY: p は 1 + n ワードの新しいオブジェクトで、a は n 個の要素を持つ。
        unsafe { std::ptr::copy_nonoverlapping(a.ptr().add(1), p.add(1), n) };
        Ok(Word::from_ptr(p))
    }

    // ---- エフェクト ----

    /// 表の添字 `i` を 1 番のワードに持つ、種類 `kind` のオブジェクトを作る。
    fn index_box(&mut self, kind: Kind, i: usize) -> Result<Word, Stop> {
        let p = self.alloc_obj(kind, 0, 2)?;
        // SAFETY: p は 2 ワードの新しいオブジェクトである。
        unsafe { *p.add(1) = i as u64 };
        Ok(Word::from_ptr(p))
    }

    /// ワード `w` が種類 `kind` のオブジェクトを指していれば真を返す。
    pub fn is_kind(&self, w: Word, kind: Kind) -> bool {
        // SAFETY: ポインタのワードは生きているオブジェクトを指す。
        w.is_ptr() && unsafe { value::kind_of(w) } == Some(kind)
    }

    /// 表の添字を持つオブジェクト `w` から添字を読む。
    fn index_of(w: Word) -> usize {
        // SAFETY: 呼び出し元がオブジェクトの種類を確かめている。
        unsafe { value::get(w, 1) as usize }
    }

    /// 表に項目を追加してから、その添字を持つオブジェクトを作る。オブジェクトを作れなければ、項目を取り除いてエラーを返す。
    ///
    /// GC で空いた添字があれば再利用する。
    /// オブジェクトの確保(安全点)の間は、項目を指すオブジェクトがまだないので、GC はこの項目を直接ルートとして扱う([`crate::gc`])。
    /// 表を拡張できないときも、プロセスを異常終了させずに、メモリ不足のエラーを返す。
    /// `ahead` は、新しい項目の後に空けておく項目の個数である([`TableGc::keep_spare`](crate::gc::TableGc::keep_spare))。
    /// `urgent` が真なら、オブジェクトの確保の前に、実行スレッドのフラグを調べずに下ろす([`Rt::pending_box`])。
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
                // SAFETY: p は 2 ワードの新しい領域である。
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

    /// ハンドラ記録 `rec` を表に登録し、それを指すオブジェクトを作る。記録には、ハンドラの起動の番号を振る。
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

    /// ハンドラ記録のオブジェクトから表の添字を読む。ハンドラ記録でなければ内部エラーを返す。
    ///
    /// エフェクトの命令が何度も呼ぶので、ほかの crate の呼び出し元にもインライン展開できるようにしている。
    /// 種類の検査は [`Rt::is_kind`] を呼ばずに直接書く。
    /// `is_kind` をインライン展開できるようにすると、インタプリタのディスパッチループのコンパイル結果が変わるからである。
    #[inline]
    pub fn hr_index(&self, w: Word) -> Result<usize, Stop> {
        // SAFETY: ポインタのワードは生きているオブジェクトを指す。
        if w.is_ptr() && unsafe { value::kind_of(w) } == Some(Kind::Handler) {
            // SAFETY: 種類を上で確かめた。ハンドラ記録のオブジェクトの 1 番のワードは表の添字である。
            Ok(unsafe { value::get(w, 1) as usize })
        } else {
            Err(self.not_handler(w))
        }
    }

    /// ハンドラ記録でない値を受け取ったときの内部エラーを作る([`Rt::hr_index`])。
    #[cold]
    #[inline(never)]
    fn not_handler(&self, w: Word) -> Stop {
        Stop::Internal(format!(
            "ハンドラ記録でない値: {}",
            String::from_utf8_lossy(&self.show(w))
        ))
    }

    /// オブジェクト `w` が指すハンドラ記録への参照を返す。ハンドラ記録でなければ内部エラーを返す。
    #[inline]
    pub fn hr(&self, w: Word) -> Result<&HandlerRec, Stop> {
        let i = self.hr_index(w)?;
        Ok(&self.handlers[i])
    }

    /// オブジェクト `w` が指すハンドラ記録への可変参照を返す。ハンドラ記録でなければ内部エラーを返す。
    #[inline]
    pub fn hr_mut(&mut self, w: Word) -> Result<&mut HandlerRec, Stop> {
        let i = self.hr_index(w)?;
        Ok(&mut self.handlers[i])
    }

    /// 継続 `c` を表に登録し、それを指すオブジェクトを作る。
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

    /// 継続のオブジェクトから表の添字を読む。継続でなければ内部エラーを返す。
    pub fn cont_index(&self, w: Word) -> Result<usize, Stop> {
        if self.is_kind(w, Kind::Cont) {
            Ok(Self::index_of(w))
        } else {
            Err(Stop::Internal("継続でない値".into()))
        }
    }

    /// 状態ワード `p` を表に退避し、それを指す値を作る。
    ///
    /// 表には、項目 1 個分の空きを確保しておく(`Rt::new` で確保し、退避のたびに確保し直す)。
    /// メモリ不足の状態ワード(`ERROR(OutOfMemory)`)は、空けておいた項目に置くので、表を拡張しない。
    /// このときは、オブジェクトを確保する前に、実行スレッドのフラグを調べずに下ろす。
    /// フラグは現在巻き戻しているメモリ不足と同じメモリ不足を表していて、ここで失敗すると状態ワードが失われ、`cancel` 節が実行されなくなるからである。
    /// 状態ワードを置いた後で空きを確保し直せなくても、エラーにせず、退避した値を返す。
    /// それ以外の状態ワードでは、状態ワードを置く項目に加えて、その後に残す空きの項目 1 個分も先に確保し、確保できなければメモリ不足のエラーを返す。
    /// 表を拡張する確保と空きを確保し直す確保は、確保箇所 `pending`([`Site::Pending`])で行う。
    /// `Rt::new` が行う最初の空きの確保は、この確保箇所に含まない。
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

    /// 退避した状態ワードの表に、項目 1 個分の空きを自己処理の範囲(`crate::alloc::handled`)で確保する。
    fn keep_pending_spare(&mut self) -> Result<(), Stop> {
        alloc::handled(|| self.saved.try_reserve(1)).map_err(|_| Stop::OutOfMemory)?;
        self.gc.tables[Table::Pending as usize]
            .keep_spare(1)
            .map_err(|_| Stop::OutOfMemory)
    }

    /// 退避した値 `w` から状態ワードを取り出し、表の項目を空にする。退避した状態ワードでなければ内部エラーを返す。
    pub fn pending_unbox(&mut self, w: Word) -> Result<Pending, Stop> {
        if !self.is_kind(w, Kind::Pending) {
            return Err(Stop::Internal("退避した状態ワードでない値".into()));
        }
        Ok(std::mem::take(&mut self.saved[Self::index_of(w)]))
    }

    /// `depth_limit` のエラーを作り、深さの上限を予備の深さ `R` だけ引き上げる(IRSpec 11.5 節)。
    pub fn depth_error(&mut self) -> Stop {
        self.limit = self.max_depth + crate::reserve(self.max_depth);
        Stop::DepthLimit(self.max_depth)
    }

    /// 実行時エラー `s` を状態ワードに設定する。
    ///
    /// 巻き戻さずに終了するエラー([`Stop::is_fatal`])は、そのまま `Err` で返す。
    /// `Stop::Pending` は、状態ワードがすでに設定されていることを表すので、何もしない。
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

    /// 初期化の前のグローバル `g` を読んだときの実行時エラーを作る(IRSpec 付録 B の 2 番)。
    pub fn unbound(&self, g: GlobalId) -> Stop {
        Stop::runtime(format!(
            "未束縛の変数: {}",
            self.prog.globals[g.index()].display
        ))
    }

    /// 値 `w` を、IRSpec 付録 B の `<値>` の形式のバイト列にする(Diktor の `show` に対応する)。
    ///
    /// 深い値でもネイティブスタックを使わないように、再帰の代わりにワークリストでたどる。
    /// レコードのフィールドは、正準なレイアウトの順に書く。
    /// ラベルの番号の昇順で、同じラベルのフィールドは外側から並べる。
    /// 付録 B は、同じラベルのフィールドの順(外側から)だけを定め、ラベルの違うフィールドどうしの順はリーダに任せている。
    /// そのため、値を作った順に書く Diktor と出力が違っても、仕様に合う。
    /// ただし、この順はバックエンドの間で同じでなければならないので、host の `show`(`host/keleut-host.mjs`)も同じ順で書く。
    /// `<値>` は実行時エラーのメッセージの一部であり、診断のメッセージは LangSpec §18 が定める評価の結果に含まれない。
    pub fn show(&self, w: Word) -> Vec<u8> {
        enum Item {
            V(Word),
            T(&'static str),
            S(String),
        }
        let p = self.prog;
        let mut out = Vec::new();
        let mut work = vec![Item::V(w)];
        // 並びの要素を、後ろから順にワークリストへ追加する。先頭の要素が最初に取り出される。
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
            // SAFETY: w は生きているオブジェクトを指す。
            let (kind, n) = unsafe { (value::kind_of(w), value::count_of(w) as usize) };
            // SAFETY: 各分岐は、オブジェクトの種類ごとのレイアウトに従って読む。
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

    /// 型 `t` の `c` 番目のコンストラクタの、表示に使う名前を返す。
    fn ctor_name(&self, t: u32, c: u32) -> &str {
        display_name(&self.prog.types[t as usize].ctors[c as usize].name)
    }

    /// 関数 `f` の種類を返す。
    pub fn func_kind(&self, f: FuncId) -> FuncKind {
        self.prog.funcs[f.index()].kind
    }
}

/// バイト列 `s` を、OCaml の `String.escaped` と同じ規則でエスケープして `out` に追記する。
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

/// 辞書の種類と内容([`Rt::dict_view`] の結果)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DictView {
    /// 組み込みのインスタンスの辞書(クラスと型)。
    Builtin(ClassId, TypeId),
    /// ユーザ定義のインスタンスの辞書(インスタンスと、前提の辞書)。
    Inst(InstanceId, Vec<Word>),
    /// レコードに対する構造的な `Eq` の辞書(ラベルと辞書の組の並び)。
    Record(Vec<(LabelId, Word)>),
    /// ヴァリアントに対する構造的な `Eq` の辞書(ラベルと辞書の組の並び)。
    Variant(Vec<(LabelId, Word)>),
    /// 辞書でない値。呼び出し元が事前条件を満たしていない。
    Other,
}

/// 呼び出せる値の種類([`Rt::callee_view`] の結果)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalleeView {
    /// クロージャ(組み込みの値と `extern` の本体を含む)。
    Closure(FuncId),
    /// メソッドの値。
    Method,
    /// 実装のない `extern` の値(グローバルの番号)。
    ExternUnimpl(GlobalId),
    /// 呼び出せない値。呼び出し元が事前条件を満たしていない。
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
