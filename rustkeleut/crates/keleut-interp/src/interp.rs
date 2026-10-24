//! インタプリタ(計画 261024-1 の 6.7 節)。
//!
//! 振り分けは Rust の `match` の平坦なループで行う。
//! フレームは分割値スタックに置き(`keleut_rt::stack` の形)、インタプリタの中の呼び出しはネイティブのスタックを使わない。
//! 末尾呼び出しはフレームを再利用する。
//!
//! 末尾でない呼び出しは、呼び出し元の見出しに再開点を書いてから、呼ばれた側のフレームを積む。
//! 呼ばれた側が戻ったら、呼び出し元の再開点から [`crate::bytecode::Code::resume`] を引いて、書き先と続きの位置を決める。
//!
//! メソッドを実行時に選ぶ呼び出しで、辞書が前提を持つ利用者のインスタンスのときは、
//! 実装の関数を先に評価してから、得た関数を引数で呼ぶ(`doc/lir.md` の 13 節)。
//! 実装の関数はふつうのフレームとして積み、呼び出し元の再開点に印 [`PHASE`] を付ける。
//! 実装の関数が戻ったら、呼び出しの命令を読み直し、得た関数を同じ引数で呼ぶ(末尾の形ならフレームを再利用する)。
//! 引数は呼び出し元のフレームのスロットに残っている(実装の関数のフレームはその上に積むので、スロットを上書きしない)。
//!
//! Rust の実行時の関数が利用者のコードを呼ぶのは、構造的な `Eq` の比べ方(`dict_record`、`dict_variant`)だけで、
//! これは [`Interp::run`] を入れ子に呼ぶ。
//! 入れ子は、比べる値がレコードかヴァリアントを通って入れ子になる段ごとに 1 つ増える。
//! 再帰する名前の付いた型(`Node({k: T})` のようなもの)では、値の入れ子の深さだけ入れ子になる。
//! 1 段は論理的な深さを 2 つ使い、ネイティブのスタックを約 1.5 KiB 使うので、既定の `S_native` では論理的な深さの限度が先に来る。
//! 入れ子の入口では、ネイティブのスタックの残りを予備の検査で確かめ、足りなければ内部の誤り(70)にする(計画 6.8 節)。
//! 小さな `--native-stack` では、この検査が論理的な深さの限度より先に働きうる。

use keleut_lir::{ClassId, ConvOp, FuncId, InstanceId, LabelId, Program, RtError, ShapeId, TypeId};
use keleut_rt::stack::{HEADER_WORDS, NATIVE_CALLER};
use keleut_rt::value::Kind;
use keleut_rt::{CalleeView, DictView, Rt, Stop, Word};
use std::collections::HashMap;

use crate::bytecode::{self, Code, op};

/// 再開点の番号に付けて、実装の関数を評価している途中であることを表す印。
pub const PHASE: u32 = 1 << 31;

/// ネイティブのスタックの予備の検査で残す余裕(バイト)。
pub const NATIVE_MARGIN: usize = 256 * 1024;

/// 引数の出どころ。
#[derive(Clone, Copy)]
enum Args {
    /// 今のフレームのスロットの番号の列(命令の語を指す)。
    Slots(*const u32, usize),
    /// 一様な語の列。
    Words(*const Word, usize),
}

impl Args {
    fn len(self) -> usize {
        match self {
            Args::Slots(_, n) | Args::Words(_, n) => n,
        }
    }
}

/// 実行中のフレームの状態。
struct St {
    fpos: u64,
    fp: *mut u64,
    code: *const Code,
    pc: usize,
    env: Word,
}

impl St {
    #[inline]
    fn get(&self, i: u32) -> u64 {
        // SAFETY: i はこのフレームのスロットの番号で、フレームは生きている。
        unsafe { *self.fp.add(HEADER_WORDS + i as usize) }
    }

    #[inline]
    fn set(&mut self, i: u32, v: u64) {
        // SAFETY: get と同じ。
        unsafe { *self.fp.add(HEADER_WORDS + i as usize) = v }
    }

    #[inline]
    fn w(&self, i: u32) -> Word {
        Word(self.get(i))
    }

    #[inline]
    fn code(&self) -> &Code {
        // SAFETY: Code は Interp::codes の Box の中にあり、Interp が生きている間は動かず解放されない。
        unsafe { &*self.code }
    }

    fn set_resume(&mut self, rp: u32) {
        // SAFETY: fp はこのフレームの見出しを指す。
        unsafe {
            let h = *self.fp;
            *self.fp = (h & 0xffff_ffff) | (u64::from(rp) << 32);
        }
    }
}

/// インタプリタ。
pub struct Interp<'p> {
    /// 実行時の文脈。
    pub rt: Rt<'p>,
    prog: &'p Program,
    codes: Vec<Option<Box<Code>>>,
    builtin_methods: HashMap<(u32, u32, u32), FuncId>,
    stack_low: usize,
    words: Vec<Word>,
}

fn approx_sp() -> usize {
    let x = 0u8;
    std::hint::black_box(&x) as *const u8 as usize
}

/// 付録 B の 17 番と 18 番の文面。
fn unimpl_prim(prog: &Program, g: keleut_lir::GlobalId) -> Stop {
    let gl = &prog.globals[g.index()];
    let name = match &gl.kind {
        keleut_lir::GlobalKind::Extern { name, .. } => name.as_str(),
        _ => gl.display.as_str(),
    };
    if prog.error_number(&RtError::UnimplPrim(g)) == 18 {
        Stop::runtime(format!(
            "未実装のプリミティブ: {}(実装名 {name} が見つかりません)",
            gl.display
        ))
    } else {
        Stop::runtime(format!("未実装のプリミティブ: {}", gl.display))
    }
}

fn conv_name(op: &ConvOp) -> &'static str {
    op.prim_name()
}

impl<'p> Interp<'p> {
    /// インタプリタを作る。`stack_low` はネイティブのスタックの下限の番地(0 なら検査しない)。
    pub fn new(rt: Rt<'p>, stack_low: usize) -> Interp<'p> {
        let prog = rt.prog;
        let builtin_methods = prog
            .builtin_methods
            .iter()
            .map(|b| ((b.class.0, b.ty.0, b.method), b.func))
            .collect();
        Interp {
            rt,
            prog,
            codes: (0..prog.funcs.len()).map(|_| None).collect(),
            builtin_methods,
            stack_low,
            words: Vec::new(),
        }
    }

    /// 関数のバイトコード(無ければ作る)。
    fn code(&mut self, f: FuncId) -> Result<*const Code, Stop> {
        if let Some(c) = &self.codes[f.index()] {
            return Ok(&**c as *const Code);
        }
        let c = bytecode::encode(self.prog, &self.rt, f).map_err(Stop::Internal)?;
        let b = Box::new(c);
        let p = &*b as *const Code;
        self.codes[f.index()] = Some(b);
        Ok(p)
    }

    /// 関数のバイトコードを作って返す(`keleut dump bytecode` 用)。
    pub fn code_of(&mut self, f: FuncId) -> Result<&Code, Stop> {
        let p = self.code(f)?;
        // SAFETY: Code は self.codes の Box の中にある。
        Ok(unsafe { &*p })
    }

    fn check_native_stack(&self) -> Result<(), Stop> {
        if self.stack_low != 0 && approx_sp() < self.stack_low {
            return Err(Stop::Internal(
                "ネイティブのスタックの予備を使い切りました".to_string(),
            ));
        }
        Ok(())
    }

    /// 関数 `func` を環境 `env` と引数 `args` で呼び、戻り値を返す。
    ///
    /// フレームは論理的な位置 `base`(今のフレームの終わり)の後に積み、最下段に「ネイティブの呼び出し元へ戻る」印を置く。
    /// 深さは変えない(呼ばれた側は `ctx.depth` で始まる)。
    pub fn run(&mut self, func: FuncId, env: Word, args: &[Word], base: u64) -> Result<Word, Stop> {
        self.check_native_stack()?;
        let code = self.code(func)?;
        // SAFETY: code は生きている Code を指す。
        let (fw, any, raw) = unsafe { ((*code).frame_words, (*code).any_slots, (*code).raw_slots) };
        let fpos = self.rt.stack.place(base, fw)?;
        let fp = self.rt.stack.ptr_at(fpos);
        // SAFETY: fp からフレームの語の数だけ値スタックの塊の中にある。
        unsafe {
            *fp = u64::from(func.0);
            *fp.add(1) = self.rt.depth;
            *fp.add(2) = u64::from(any) | (u64::from(raw) << 32);
            *fp.add(3) = NATIVE_CALLER;
            for (i, a) in args.iter().enumerate() {
                *fp.add(HEADER_WORDS + i) = a.0;
            }
        }
        let mut st = St {
            fpos,
            fp,
            code,
            pc: 0,
            env,
        };
        self.exec(&mut st)
    }

    /// 今のフレームの上に呼ばれた側のフレームを積み、その入口へ移る。
    fn enter(&mut self, st: &mut St, func: FuncId, env: Word, args: Args) -> Result<(), Stop> {
        let top = st.fpos + st.code().frame_words as u64;
        let code = self.code(func)?;
        // SAFETY: code は生きている Code を指す。
        let (fw, any, raw) = unsafe { ((*code).frame_words, (*code).any_slots, (*code).raw_slots) };
        let npos = self.rt.stack.place(top, fw)?;
        let nfp = self.rt.stack.ptr_at(npos);
        // SAFETY: nfp からフレームの語の数だけ塊の中にあり、呼び出し元のフレーム(st.fp)とは重ならない。
        unsafe {
            match args {
                Args::Slots(p, n) => {
                    for i in 0..n {
                        *nfp.add(HEADER_WORDS + i) = st.get(*p.add(i));
                    }
                }
                Args::Words(p, n) => {
                    for i in 0..n {
                        *nfp.add(HEADER_WORDS + i) = (*p.add(i)).0;
                    }
                }
            }
            *nfp = u64::from(func.0);
            *nfp.add(1) = 0;
            *nfp.add(2) = u64::from(any) | (u64::from(raw) << 32);
            *nfp.add(3) = st.fpos;
        }
        *st = St {
            fpos: npos,
            fp: nfp,
            code,
            pc: 0,
            env,
        };
        Ok(())
    }

    /// 今のフレームを再利用して、呼ばれた側の入口へ移る(末尾呼び出し)。
    fn tail_enter(&mut self, st: &mut St, func: FuncId, env: Word, args: Args) -> Result<(), Stop> {
        let mut buf = std::mem::take(&mut self.words);
        buf.clear();
        // SAFETY: 引数の出どころは今のフレームのスロットか、生きている語の列である。
        unsafe {
            match args {
                Args::Slots(p, n) => {
                    for i in 0..n {
                        buf.push(st.w(*p.add(i)));
                    }
                }
                Args::Words(p, n) => {
                    for i in 0..n {
                        buf.push(*p.add(i));
                    }
                }
            }
        }
        // SAFETY: fp は今のフレームの見出しを指す。
        let (caller, depth) = unsafe { (*st.fp.add(3), *st.fp.add(1)) };
        let code = self.code(func)?;
        // SAFETY: code は生きている Code を指す。
        let (fw, any, raw) = unsafe { ((*code).frame_words, (*code).any_slots, (*code).raw_slots) };
        let npos = self.rt.stack.place(st.fpos, fw)?;
        let nfp = self.rt.stack.ptr_at(npos);
        // SAFETY: nfp からフレームの語の数だけ塊の中にある。
        unsafe {
            for (i, a) in buf.iter().enumerate() {
                *nfp.add(HEADER_WORDS + i) = a.0;
            }
            *nfp = u64::from(func.0);
            *nfp.add(1) = depth;
            *nfp.add(2) = u64::from(any) | (u64::from(raw) << 32);
            *nfp.add(3) = caller;
        }
        self.words = buf;
        *st = St {
            fpos: npos,
            fp: nfp,
            code,
            pc: 0,
            env,
        };
        Ok(())
    }

    /// 値 `v` で今のフレームから戻る。呼び出し元がネイティブなら `Some(v)` を返す。
    fn ret(&mut self, st: &mut St, v: Word) -> Result<Option<Word>, Stop> {
        // SAFETY: fp は今のフレームの見出しを指す。
        let caller = unsafe { *st.fp.add(3) };
        if caller == NATIVE_CALLER {
            return Ok(Some(v));
        }
        let cfp = self.rt.stack.ptr_at(caller);
        // SAFETY: cfp は呼び出し元のフレームの見出しを指す。
        let h0 = unsafe { *cfp };
        let func = FuncId(h0 as u32);
        let rp = (h0 >> 32) as u32;
        let code = self.code(func)?;
        st.fpos = caller;
        st.fp = cfp;
        st.code = code;
        st.env = Word(0);
        if rp & PHASE != 0 {
            return self.phase2(st, rp & !PHASE, v);
        }
        let e = st.code().resume[rp as usize];
        st.set(e.dst, v.0);
        st.pc = e.pc_after as usize;
        Ok(None)
    }

    /// 実装の関数が値 `f` を返した後、呼び出しの命令を読み直して `f` を呼ぶ。
    fn phase2(&mut self, st: &mut St, rp: u32, f: Word) -> Result<Option<Word>, Stop> {
        let e = st.code().resume[rp as usize];
        let pc = e.call_pc as usize;
        let w = &st.code().words;
        let opc = (w[pc] & 0xff) as u8;
        let (start, n, tail) = match opc {
            op::CALL_METHOD | op::TAIL_METHOD => {
                let no = w[pc + 5] as usize;
                (pc + 7 + no, w[pc + 6 + no] as usize, opc == op::TAIL_METHOD)
            }
            op::CALL_VALUE | op::TAIL_VALUE => (pc + 4, w[pc + 3] as usize, opc == op::TAIL_VALUE),
            _ => {
                return Err(Stop::Internal(format!(
                    "再開点 {rp} の命令がメソッドの呼び出しではありません"
                )));
            }
        };
        let args = Args::Slots(w[start..].as_ptr(), n);
        self.call_value(st, rp, f, args, tail)
    }

    /// 値 `f` を呼ぶ(`call_value` と `tail_call_value`)。
    fn call_value(
        &mut self,
        st: &mut St,
        rp: u32,
        f: Word,
        args: Args,
        tail: bool,
    ) -> Result<Option<Word>, Stop> {
        match self.rt.callee_view(f) {
            CalleeView::Closure(func) => {
                if tail {
                    self.tail_enter(st, func, f, args)?;
                } else {
                    st.set_resume(rp);
                    self.enter(st, func, f, args)?;
                }
                Ok(None)
            }
            CalleeView::ExternUnimpl(g) => Err(unimpl_prim(self.prog, g)),
            CalleeView::Method => {
                let (c, m, d, own) = self.rt.method_parts(f);
                self.select(st, rp, c, m, d, &own, args, tail)
            }
            CalleeView::Other => Err(Stop::Internal(format!(
                "呼べない値を呼びました: {}",
                String::from_utf8_lossy(&self.rt.show(f))
            ))),
        }
    }

    /// 辞書 `d` からメソッドを選んで呼ぶ(`call_method` と `tail_call_method`)。
    #[allow(clippy::too_many_arguments)]
    fn select(
        &mut self,
        st: &mut St,
        rp: u32,
        class: ClassId,
        m: u32,
        d: Word,
        own: &[Word],
        args: Args,
        tail: bool,
    ) -> Result<Option<Word>, Stop> {
        match self.rt.dict_view(d) {
            DictView::Builtin(c, t) => {
                let body = self.builtin_method(c, t, m)?;
                if tail {
                    self.tail_enter(st, body, Word(0), args)?;
                } else {
                    st.set_resume(rp);
                    self.enter(st, body, Word(0), args)?;
                }
                Ok(None)
            }
            DictView::Inst(i, premises) => {
                self.check_inst(i)?;
                let prog = self.prog;
                let im = &prog.instances[i.index()].methods[m as usize];
                if im.dicts == 0 {
                    let v = self.rt.inst_values[i.index()][m as usize];
                    return self.call_value(st, rp, v, args, tail);
                }
                let func = im.func;
                let mut dicts = premises;
                dicts.extend_from_slice(own);
                st.set_resume(rp | PHASE);
                self.enter(st, func, Word(0), Args::Words(dicts.as_ptr(), dicts.len()))?;
                Ok(None)
            }
            DictView::Record(_) | DictView::Variant(_) => {
                if args.len() != 2 {
                    return Err(Stop::Internal(
                        "構造的な Eq の引数の数が 2 でありません".into(),
                    ));
                }
                let (a, b) = self.args2(st, args);
                // 比べ方は確保し、利用者のコードを呼ぶので、先に見出しの再開点を今の呼び出しにする。
                // 末尾の形の番号(`tail_entry`)は LIR の生存の集合を持たず、根は Root に置いた両辺と辞書だけである。
                st.set_resume(rp);
                let base = st.fpos + st.code().frame_words as u64;
                let r = Word::bool(self.eq_struct(class, d, a, b, base)?);
                if tail {
                    return self.ret(st, r);
                }
                let e = st.code().resume[rp as usize];
                st.set(e.dst, r.0);
                st.pc = e.pc_after as usize;
                Ok(None)
            }
            DictView::Other => Err(Stop::Internal(format!(
                "辞書でない値からメソッドを選びました: {}",
                String::from_utf8_lossy(&self.rt.show(d))
            ))),
        }
    }

    fn args2(&self, st: &St, args: Args) -> (Word, Word) {
        // SAFETY: 引数は 2 つある(呼び出し側が確かめた)。
        unsafe {
            match args {
                Args::Slots(p, _) => (st.w(*p), st.w(*p.add(1))),
                Args::Words(p, _) => (*p, *p.add(1)),
            }
        }
    }

    fn builtin_method(&self, c: ClassId, t: TypeId, m: u32) -> Result<FuncId, Stop> {
        self.builtin_methods
            .get(&(c.0, t.0, m))
            .copied()
            .ok_or_else(|| {
                Stop::Internal(format!(
                    "組み込みのメソッドがありません: クラス {} 型 {} メソッド {m}",
                    c.0, t.0
                ))
            })
    }

    fn check_inst(&self, i: InstanceId) -> Result<(), Stop> {
        if self.rt.inst_init[i.index()] {
            Ok(())
        } else {
            Err(keleut_rt::error::instance_not_ready(
                &self.prog.instances[i.index()].display,
            ))
        }
    }

    // ---- 入れ子の呼び出し(構造的な Eq) ----

    /// 値 `f` を入れ子に呼ぶ。
    fn call_value_nested(&mut self, f: Word, args: &[Word], base: u64) -> Result<Word, Stop> {
        match self.rt.callee_view(f) {
            CalleeView::Closure(func) => self.run(func, f, args, base),
            CalleeView::ExternUnimpl(g) => Err(unimpl_prim(self.prog, g)),
            CalleeView::Method => {
                let (c, m, d, own) = self.rt.method_parts(f);
                self.call_method_nested(c, m, d, &own, args, base)
            }
            CalleeView::Other => Err(Stop::Internal("呼べない値を呼びました".into())),
        }
    }

    /// 辞書からメソッドを選んで入れ子に呼ぶ。
    fn call_method_nested(
        &mut self,
        class: ClassId,
        m: u32,
        d: Word,
        own: &[Word],
        args: &[Word],
        base: u64,
    ) -> Result<Word, Stop> {
        match self.rt.dict_view(d) {
            DictView::Builtin(c, t) => {
                let body = self.builtin_method(c, t, m)?;
                self.run(body, Word(0), args, base)
            }
            DictView::Inst(i, premises) => {
                self.check_inst(i)?;
                let prog = self.prog;
                let im = &prog.instances[i.index()].methods[m as usize];
                let f = if im.dicts == 0 {
                    self.rt.inst_values[i.index()][m as usize]
                } else {
                    let func = im.func;
                    let mut dicts = premises;
                    dicts.extend_from_slice(own);
                    let mark = self.root_all(args);
                    let f = self.run(func, Word(0), &dicts, base);
                    self.rt.roots.truncate(mark);
                    f?
                };
                self.call_value_nested(f, args, base)
            }
            DictView::Record(_) | DictView::Variant(_) => Ok(Word::bool(
                self.eq_struct(class, d, args[0], args[1], base)?,
            )),
            DictView::Other => Err(Stop::Internal(
                "辞書でない値からメソッドを選びました".into(),
            )),
        }
    }

    fn root_all(&mut self, ws: &[Word]) -> usize {
        let mark = self.rt.roots.len();
        for w in ws {
            self.rt.roots.push(w.0);
        }
        mark
    }

    /// 構造的な `Eq` の比べ方(`doc/lir.md` の `call_method`)。
    ///
    /// 各項目の比較は、深さを 1 つ増やして項目の辞書の `eq`(クラス `class` の 0 番のメソッド)を呼ぶ。
    /// 比べる間、両辺と辞書を根の枠に置く。
    fn eq_struct(
        &mut self,
        class: ClassId,
        d: Word,
        a: Word,
        b: Word,
        base: u64,
    ) -> Result<bool, Stop> {
        let mark = self.root_all(&[d, a, b]);
        let r = self.eq_struct_inner(class, d, a, b, base);
        self.rt.roots.truncate(mark);
        r
    }

    fn eq_struct_inner(
        &mut self,
        class: ClassId,
        d: Word,
        a: Word,
        b: Word,
        base: u64,
    ) -> Result<bool, Stop> {
        match self.rt.dict_view(d) {
            DictView::Record(fs) => {
                // 項目の `l` が辞書で `k` 番目の `l` なら、両辺の外側から `k` 番目の `l` を比べる。
                // 前の項目で同名のフィールドを除いた残りの最も外側を取るのと同じで、確保をしない。
                // a、b、d は eq_struct が根の枠に置いている。
                for (i, &(l, dl)) in fs.iter().enumerate() {
                    let k = fs[..i].iter().filter(|(m, _)| *m == l).count() as u32;
                    let xa = self.rt.rec_field(a, l, k);
                    let xb = self.rt.rec_field(b, l, k);
                    if !self.eq_item(class, dl, xa, xb, base)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            DictView::Variant(fs) => {
                let (ta, tb) = (self.rt.variant_tag(a), self.rt.variant_tag(b));
                if ta != tb {
                    return Ok(false);
                }
                let Some(&(_, dl)) = fs.iter().find(|(l, _)| l.0 as i32 == ta) else {
                    return Ok(false);
                };
                let (pa, pb) = (self.rt.variant_payload(a), self.rt.variant_payload(b));
                self.eq_item(class, dl, pa, pb, base)
            }
            _ => Err(Stop::Internal("構造的な辞書ではありません".into())),
        }
    }

    fn eq_item(
        &mut self,
        class: ClassId,
        d: Word,
        x: Word,
        y: Word,
        base: u64,
    ) -> Result<bool, Stop> {
        let saved = self.rt.depth;
        if self.rt.depth + 1 > self.rt.limit {
            return Err(Stop::DepthLimit(self.rt.limit));
        }
        self.rt.depth += 1;
        let r = self.call_method_nested(class, 0, d, &[], &[x, y], base);
        self.rt.depth = saved;
        Ok(r?.as_bool())
    }

    // ---- 振り分けのループ ----

    fn error(&self, st: &St, idx: u32, operand: u32) -> Stop {
        let kind = &st.code().errors[idx as usize];
        let opnd = || st.w(operand);
        let show = |w: Word| self.rt.show(w);
        let msg: Vec<u8> = match kind {
            RtError::Hole => "??? に到達しました".into(),
            RtError::Unbound(g) => return self.rt.unbound(*g),
            RtError::PatternFail => {
                let mut m = "パターンに値が一致しません: ".as_bytes().to_vec();
                m.extend(show(opnd()));
                m
            }
            RtError::MatchFail => {
                let mut m = "match のどの節にも一致しません: ".as_bytes().to_vec();
                m.extend(show(opnd()));
                m
            }
            RtError::LitRange(s) => format!("数値リテラルが範囲外です: {s}").into(),
            RtError::InstanceNotReady(i) => {
                return keleut_rt::error::instance_not_ready(
                    &self.prog.instances[i.index()].display,
                );
            }
            RtError::DivZero => "ゼロ除算です".into(),
            RtError::F64ToIntNan(op) => {
                format!("{}: NaN は整数に変換できません", conv_name(op)).into()
            }
            RtError::F64ToIntRange(op) => format!(
                "{}: 変換結果が範囲外です: {}",
                conv_name(op),
                keleut_rtcore::show_f64(f64::from_bits(st.get(operand)))
            )
            .into(),
            RtError::StrSubRange => "__string_sub: 範囲外です".into(),
            RtError::Panic => {
                let mut m = b"panic: ".to_vec();
                m.extend_from_slice(self.rt.str_bytes(opnd()));
                m
            }
            RtError::ArrayIndex => "配列の範囲外です".into(),
            RtError::MArrayNegLen => "MutableArray.new: 長さが負です".into(),
            RtError::UnimplPrim(g) => return unimpl_prim(self.prog, *g),
            RtError::OutOfMemory => return Stop::OutOfMemory,
        };
        Stop::Runtime(msg)
    }

    fn exec(&mut self, st: &mut St) -> Result<Word, Stop> {
        use op::*;
        loop {
            let code = st.code;
            // SAFETY: Code は self.codes の Box の中にあり、動かない。
            let w: &[u32] = unsafe { &(*code).words };
            let pc = st.pc;
            let w0 = w[pc];
            let opc = (w0 & 0xff) as u8;
            let sub = w0 >> 8;
            let a = |k: usize| w[pc + k];
            macro_rules! g {
                ($k:expr) => {
                    st.get(a($k))
                };
            }
            macro_rules! gw {
                ($k:expr) => {
                    Word(st.get(a($k)))
                };
            }
            macro_rules! put {
                ($k:expr, $v:expr) => {{
                    let v: u64 = $v;
                    st.set(a($k), v);
                }};
            }
            macro_rules! slots {
                ($at:expr) => {{
                    let n = a($at) as usize;
                    (Args::Slots(w[pc + $at + 1..].as_ptr(), n), n)
                }};
            }
            macro_rules! collect {
                ($at:expr) => {{
                    let n = a($at) as usize;
                    let mut v = std::mem::take(&mut self.words);
                    v.clear();
                    for i in 0..n {
                        v.push(Word(st.get(w[pc + $at + 1 + i])));
                    }
                    (v, n)
                }};
            }
            match opc {
                CONST => {
                    put!(1, u64::from(a(2)) | (u64::from(a(3)) << 32));
                    st.pc += 4;
                }
                MOVE => {
                    put!(1, g!(2));
                    st.pc += 3;
                }
                BOX_BOOL => {
                    put!(1, Word::bool(g!(2) != 0).0);
                    st.pc += 3;
                }
                BOX_I32 => {
                    put!(1, Word::i32(g!(2) as i32).0);
                    st.pc += 3;
                }
                BOX_I64 => {
                    let b = self.rt.box_i64(g!(2) as i64)?;
                    put!(1, b.0);
                    st.pc += 3;
                }
                BOX_F64 => {
                    let b = self.rt.box_f64(g!(2))?;
                    put!(1, b.0);
                    st.pc += 3;
                }
                UNBOX_BOOL => {
                    put!(1, u64::from(gw!(2).as_bool()));
                    st.pc += 3;
                }
                UNBOX_I32 => {
                    put!(1, gw!(2).as_i32() as i64 as u64);
                    st.pc += 3;
                }
                UNBOX_I64 | UNBOX_F64 => {
                    put!(1, self.rt.unbox_word(gw!(2)));
                    st.pc += 3;
                }
                ENV_GET => {
                    put!(1, self.rt.closure_cap(st.env, a(2)).0);
                    st.pc += 3;
                }
                DEPTH_ENTER => {
                    // SAFETY: fp は今のフレームの見出しを指す。
                    unsafe { *st.fp.add(1) = self.rt.depth };
                    st.pc += 1;
                }
                DEPTH_CHECK => {
                    if self.rt.depth + 1 > self.rt.limit {
                        return Err(Stop::DepthLimit(self.rt.limit));
                    }
                    st.pc += 1;
                }
                DEPTH_PUSH => {
                    if self.rt.depth + 1 > self.rt.limit {
                        return Err(Stop::DepthLimit(self.rt.limit));
                    }
                    self.rt.depth += 1;
                    st.pc += 1;
                }
                DEPTH_RESTORE => {
                    // SAFETY: fp は今のフレームの見出しを指す。
                    self.rt.depth = unsafe { *st.fp.add(1) };
                    st.pc += 1;
                }
                I32_ARITH => {
                    let (x, y) = (g!(2) as i32, g!(3) as i32);
                    let r = match sub {
                        0 => x.wrapping_add(y),
                        1 => x.wrapping_sub(y),
                        2 => x.wrapping_mul(y),
                        3 => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_div(y)
                            }
                        }
                        _ => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_rem(y)
                            }
                        }
                    };
                    put!(1, r as i64 as u64);
                    st.pc += 4;
                }
                I64_ARITH => {
                    let (x, y) = (g!(2) as i64, g!(3) as i64);
                    let r = match sub {
                        0 => x.wrapping_add(y),
                        1 => x.wrapping_sub(y),
                        2 => x.wrapping_mul(y),
                        3 => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_div(y)
                            }
                        }
                        _ => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_rem(y)
                            }
                        }
                    };
                    put!(1, r as u64);
                    st.pc += 4;
                }
                I32_NEG => {
                    put!(1, (g!(2) as i32).wrapping_neg() as i64 as u64);
                    st.pc += 3;
                }
                I64_NEG => {
                    put!(1, (g!(2) as i64).wrapping_neg() as u64);
                    st.pc += 3;
                }
                I32_CMP => {
                    put!(1, u64::from(cmp(sub, (g!(2) as i32).cmp(&(g!(3) as i32)))));
                    st.pc += 4;
                }
                I64_CMP => {
                    put!(1, u64::from(cmp(sub, (g!(2) as i64).cmp(&(g!(3) as i64)))));
                    st.pc += 4;
                }
                F64_ARITH => {
                    let (x, y) = (f64::from_bits(g!(2)), f64::from_bits(g!(3)));
                    let r = match sub {
                        0 => x + y,
                        1 => x - y,
                        2 => x * y,
                        _ => x / y,
                    };
                    put!(1, r.to_bits());
                    st.pc += 4;
                }
                F64_NEG => {
                    put!(1, g!(2) ^ (1u64 << 63));
                    st.pc += 3;
                }
                F64_CMP => {
                    let (x, y) = (f64::from_bits(g!(2)), f64::from_bits(g!(3)));
                    let r = match sub {
                        0 => x == y,
                        1 => x < y,
                        2 => x <= y,
                        3 => x > y,
                        _ => x >= y,
                    };
                    put!(1, u64::from(r));
                    st.pc += 4;
                }
                F64_MATH => {
                    let x = f64::from_bits(g!(2));
                    let r = match sub {
                        0 => keleut_rtcore::sin(x),
                        1 => keleut_rtcore::cos(x),
                        2 => x.sqrt(),
                        3 => keleut_rtcore::exp(x),
                        _ => keleut_rtcore::log(x),
                    };
                    put!(1, r.to_bits());
                    st.pc += 3;
                }
                F64_TRUNC => {
                    put!(1, f64::from_bits(g!(2)).trunc().to_bits());
                    st.pc += 3;
                }
                F64_ISNAN => {
                    put!(1, u64::from(f64::from_bits(g!(2)).is_nan()));
                    st.pc += 3;
                }
                CONV => {
                    let x = g!(2);
                    let r = match sub {
                        0 => x as i32 as i64 as u64,
                        1 => (x as i32 as f64).to_bits(),
                        2 => x as i64 as i32 as i64 as u64,
                        3 => (x as i64 as f64).to_bits(),
                        4 => f64::from_bits(x) as i32 as i64 as u64,
                        _ => f64::from_bits(x) as i64 as u64,
                    };
                    put!(1, r);
                    st.pc += 3;
                }
                BOOL_NOT => {
                    put!(1, u64::from(g!(2) == 0));
                    st.pc += 3;
                }
                BOOL_EQ => {
                    put!(1, u64::from(g!(2) == g!(3)));
                    st.pc += 4;
                }
                STR_CONCAT => {
                    let r = self.rt.str_concat(gw!(2), gw!(3))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                STR_CMP => {
                    let o = keleut_rtcore::compare_bytes(
                        self.rt.str_bytes(gw!(2)),
                        self.rt.str_bytes(gw!(3)),
                    );
                    put!(1, u64::from(cmp(sub, o)));
                    st.pc += 4;
                }
                STR_LEN => {
                    put!(1, self.rt.str_bytes(gw!(2)).len() as i64 as u64);
                    st.pc += 3;
                }
                STR_SUB => {
                    let r = self.rt.str_sub(gw!(2), g!(3) as i32, g!(4) as i32)?;
                    put!(1, r.0);
                    st.pc += 5;
                }
                SHOW => {
                    let x = g!(2);
                    let s = match sub {
                        0 => (x as i32).to_string(),
                        1 => (x as i64).to_string(),
                        2 => keleut_rtcore::show_f64(f64::from_bits(x)),
                        _ => (if x != 0 { "true" } else { "false" }).to_string(),
                    };
                    let r = self.rt.new_str(s.as_bytes())?;
                    put!(1, r.0);
                    st.pc += 3;
                }
                CTOR_NEW => {
                    let (v, n) = collect!(4);
                    let r = self.rt.ctor_new(TypeId(a(2)), a(3), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 5 + n;
                }
                CTOR_TAG => {
                    put!(1, self.rt.ctor_tag(gw!(2)) as i64 as u64);
                    st.pc += 3;
                }
                CTOR_FIELD => {
                    put!(1, self.rt.ctor_field(gw!(2), a(3)).0);
                    st.pc += 4;
                }
                VARIANT_NEW => {
                    let r = self.rt.variant_new(LabelId(a(2)), gw!(3))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                VARIANT_TAG => {
                    put!(1, self.rt.variant_tag(gw!(2)) as i64 as u64);
                    st.pc += 3;
                }
                VARIANT_PAYLOAD => {
                    put!(1, self.rt.variant_payload(gw!(2)).0);
                    st.pc += 3;
                }
                REC_NEW => {
                    let (v, n) = collect!(3);
                    let r = self.rt.rec_new(ShapeId(a(2)), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 4 + n;
                }
                REC_GET => {
                    put!(1, self.rt.rec_get(gw!(2), a(3)).0);
                    st.pc += 4;
                }
                REC_EXTEND => {
                    let r = self.rt.rec_extend(gw!(2), LabelId(a(3)), gw!(4))?;
                    put!(1, r.0);
                    st.pc += 5;
                }
                REC_FIELD => {
                    put!(1, self.rt.rec_field(gw!(2), LabelId(a(3)), a(4)).0);
                    st.pc += 5;
                }
                REC_HAS => {
                    put!(1, u64::from(self.rt.rec_has(gw!(2), LabelId(a(3)), a(4))));
                    st.pc += 5;
                }
                REC_RESTRICT => {
                    let r = self.rt.rec_restrict(gw!(2), LabelId(a(3)))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                REC_UPDATE => {
                    let r = self.rt.rec_update(gw!(2), LabelId(a(3)), gw!(4))?;
                    put!(1, r.0);
                    st.pc += 5;
                }
                CLOSURE_NEW => {
                    let (v, n) = collect!(3);
                    let r = self.rt.closure_new(FuncId(a(2)), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 4 + n;
                }
                CLOSURE_SET_CAP => {
                    self.rt.closure_set_cap(gw!(1), a(2), gw!(3));
                    st.pc += 4;
                }
                METHOD_NEW => {
                    let (v, n) = collect!(5);
                    let r = self.rt.method_new(ClassId(a(2)), a(3), gw!(4), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 6 + n;
                }
                DICT_INST => {
                    let (v, n) = collect!(3);
                    let r = self.rt.dict_inst(InstanceId(a(2)), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 4 + n;
                }
                DICT_RECORD | DICT_VARIANT => {
                    let n = a(2) as usize;
                    let fs: Vec<(LabelId, Word)> = (0..n)
                        .map(|i| (LabelId(a(3 + 2 * i)), Word(st.get(a(4 + 2 * i)))))
                        .collect();
                    let kind = if opc == DICT_RECORD {
                        Kind::DictRecord
                    } else {
                        Kind::DictVariant
                    };
                    let r = self.rt.dict_struct(kind, &fs)?;
                    put!(1, r.0);
                    st.pc += 3 + 2 * n;
                }
                GLOBAL_GET => {
                    put!(1, self.rt.globals[a(2) as usize].0);
                    st.pc += 3;
                }
                GLOBAL_IS_INIT => {
                    put!(1, u64::from(self.rt.global_init[a(2) as usize]));
                    st.pc += 3;
                }
                GLOBAL_SET => {
                    self.rt.globals[a(1) as usize] = gw!(2);
                    st.pc += 3;
                }
                GLOBAL_MARK_INIT => {
                    self.rt.global_init[a(1) as usize] = true;
                    st.pc += 2;
                }
                INST_IS_INIT => {
                    put!(1, u64::from(self.rt.inst_init[a(2) as usize]));
                    st.pc += 3;
                }
                INST_MARK_INIT => {
                    self.rt.inst_init[a(1) as usize] = true;
                    st.pc += 2;
                }
                INST_SET_VALUE => {
                    self.rt.inst_values[a(1) as usize][a(2) as usize] = gw!(3);
                    st.pc += 4;
                }
                INST_GET_VALUE => {
                    put!(1, self.rt.inst_values[a(2) as usize][a(3) as usize].0);
                    st.pc += 4;
                }
                REF_NEW => {
                    let r = self.rt.ref_new(gw!(2))?;
                    put!(1, r.0);
                    st.pc += 3;
                }
                REF_GET => {
                    // SAFETY: 前提により値は Ref の箱である。
                    put!(1, unsafe { keleut_rt::value::get(gw!(2), 1) });
                    st.pc += 3;
                }
                REF_SET => {
                    // SAFETY: 前提により値は Ref の箱である。
                    unsafe { keleut_rt::value::set(gw!(1), 1, g!(2)) };
                    st.pc += 3;
                }
                ARRAY_LEN => {
                    put!(1, self.rt.array_len(gw!(2)) as i64 as u64);
                    st.pc += 3;
                }
                ARRAY_GET => {
                    put!(1, self.rt.array_get(gw!(2), g!(3) as i32).0);
                    st.pc += 4;
                }
                MARRAY_NEW => {
                    let n = (g!(2) as i32).max(0) as usize;
                    let r = self.rt.array_new(Kind::MArray, n, gw!(3))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                MARRAY_SET => {
                    self.rt.array_set(gw!(1), g!(2) as i32, gw!(3));
                    st.pc += 4;
                }
                MARRAY_FREEZE => {
                    let r = self.rt.array_freeze(gw!(2))?;
                    put!(1, r.0);
                    st.pc += 3;
                }
                CALL_DIRECT => {
                    let (args, _) = slots!(3);
                    st.set_resume(a(1));
                    self.enter(st, FuncId(a(2)), Word(0), args)?;
                }
                CALL_VALUE => {
                    let (args, _) = slots!(3);
                    if let Some(v) = self.call_value(st, a(1), gw!(2), args, false)? {
                        return Ok(v);
                    }
                }
                CALL_METHOD | TAIL_METHOD => {
                    let no = a(5) as usize;
                    let own: Vec<Word> = (0..no).map(|i| Word(st.get(a(6 + i)))).collect();
                    let (args, _) = slots!(6 + no);
                    let tail = opc == TAIL_METHOD;
                    let r = self.select(st, a(1), ClassId(a(2)), a(3), gw!(4), &own, args, tail)?;
                    if let Some(v) = r {
                        return Ok(v);
                    }
                }
                DICT_APP => {
                    let f = gw!(2);
                    let (args, _) = slots!(3);
                    if self.rt.is_extern_value(f) {
                        let e = st.code().resume[a(1) as usize];
                        st.set(e.dst, f.0);
                        st.pc = e.pc_after as usize;
                    } else {
                        match self.rt.callee_view(f) {
                            CalleeView::Closure(func) => {
                                st.set_resume(a(1));
                                self.enter(st, func, f, args)?;
                            }
                            _ => {
                                return Err(Stop::Internal(
                                    "辞書を受け取らない値に辞書を渡しました".into(),
                                ));
                            }
                        }
                    }
                }
                CONSOLE_WRITE => {
                    let s = gw!(2);
                    let bytes = self.rt.str_bytes(s);
                    // 文字列の箱は動かず回収されないので、写さずに渡す。
                    let bytes: &[u8] =
                        // SAFETY: bytes は生きている文字列の箱の中を指す。
                        unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
                    self.rt.out.write(bytes)?;
                    put!(1, Word::UNIT.0);
                    st.pc += 3;
                }
                UNIMPL_VALUE | UNIMPLEMENTED => {
                    return Err(Stop::Unimplemented(st.code().texts[sub as usize].clone()));
                }
                JUMP => st.pc = a(1) as usize,
                BRANCH => {
                    st.pc = if g!(1) != 0 { a(2) } else { a(3) } as usize;
                }
                RETURN => {
                    let v = gw!(1);
                    if let Some(v) = self.ret(st, v)? {
                        return Ok(v);
                    }
                }
                TAIL_DIRECT => {
                    let (args, _) = slots!(2);
                    self.tail_enter(st, FuncId(a(1)), Word(0), args)?;
                }
                TAIL_VALUE => {
                    let (args, _) = slots!(3);
                    if let Some(v) = self.call_value(st, a(1), gw!(2), args, true)? {
                        return Ok(v);
                    }
                }
                ERROR => return Err(self.error(st, sub, a(1))),
                _ => {
                    return Err(Stop::Internal(format!(
                        "知らない命令の番号 {opc}(位置 {pc})"
                    )));
                }
            }
        }
    }
}

/// 比べ方の細目 `sub` を順序の結果に当てる。
#[inline]
fn cmp(sub: u32, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match sub {
        0 => o == Equal,
        1 => o == Less,
        2 => o != Greater,
        3 => o == Greater,
        _ => o != Less,
    }
}
