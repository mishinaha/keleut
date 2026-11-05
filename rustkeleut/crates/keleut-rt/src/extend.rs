//! 単位を足したプログラムへの文脈の付け替え(`keleut repl`)。
//!
//! 対話的な実行は、受理した入力ごとに KIR の単位を 1 つプログラムの後ろに足し、
//! 足した単位だけを LIR に下ろして LIR のプログラムの各表の後ろに足す(`keleut_lir::Extender`)。
//! 前の関数、大域、インスタンス、形、文字列の添字は変わらないので、ヒープに残った値はそのまま使える。
//!
//! [`Rt`] はプログラムを借りているので、プログラムに単位を足す間は文脈を別のプログラムに付け替えておく。
//! [`Rt::rebind`] は文脈を消費して、ヒープ、根、スタック、表をそのまま別のプログラムに付け替える。
//! 足す間は空のプログラム(`keleut_lir::Program::empty`)に付け替えて置き、
//! 足した後で足したプログラムに付け替えて [`Rt::grow`] で表を伸ばす。

use crate::ctx::Rt;
use crate::error::Stop;
use crate::value::{Kind, Word, header};
use keleut_lir::{GlobalKind, Program};

impl<'p> Rt<'p> {
    /// 文脈を、プログラム `prog` に付け替える。表は伸ばさない([`Rt::grow`])。
    ///
    /// `prog` は、今のプログラムと同じか、その各表の後ろに項目を足したものか、
    /// 文脈を使わない間だけ置いておく空のプログラムでなければならない。
    pub fn rebind<'q>(self, prog: &'q Program) -> Rt<'q> {
        let Rt {
            prog: _,
            heap,
            roots,
            stack,
            out,
            shapes,
            depth,
            limit,
            max_depth,
            handlers,
            conts,
            saved,
            chain,
            pending,
            next_handler,
            globals,
            global_init,
            inst_init,
            inst_values,
            strings,
            func_prim_name,
            extern_funcs,
            gc,
        } = self;
        Rt {
            prog,
            heap,
            roots,
            stack,
            out,
            shapes,
            depth,
            limit,
            max_depth,
            handlers,
            conts,
            saved,
            chain,
            pending,
            next_handler,
            globals,
            global_init,
            inst_init,
            inst_values,
            strings,
            func_prim_name,
            extern_funcs,
            gc,
        }
    }

    /// プログラムの各表の後ろに足した項目の分だけ、文脈の表を伸ばす([`Rt::new_with`] と同じ規則)。
    ///
    /// 足した形を [`Rt::grow_shapes`] で実行時の形の表に入れてから、ほかの表を伸ばす。
    /// 足した文字列の定数と、関数を値とする足した大域のクロージャを確保する。
    /// 大域の表は先に Unit で伸ばしてから値を置くので、その間の回収は前の大域の値を根として扱う。
    pub fn grow(&mut self) -> Result<(), Stop> {
        self.grow_shapes()?;
        let prog = self.prog;
        let nf = prog.funcs.len();
        let g0 = self.globals.len();
        self.func_prim_name.resize(nf, None);
        self.extern_funcs.resize(nf, false);
        self.gc.grow_funcs(nf);
        for g in &prog.globals[g0..] {
            match &g.kind {
                GlobalKind::Builtin { func } => {
                    self.func_prim_name[func.index()] = Some(g.display.clone());
                }
                GlobalKind::Extern { func: Some(f), .. } => {
                    self.func_prim_name[f.index()] = Some(g.display.clone());
                    self.extern_funcs[f.index()] = true;
                }
                _ => {}
            }
        }
        self.globals.resize(prog.globals.len(), Word::UNIT);
        self.global_init.resize(prog.globals.len(), false);
        let i0 = self.inst_values.len();
        self.inst_init.resize(prog.instances.len(), false);
        for i in &prog.instances[i0..] {
            self.inst_values.push(vec![Word::UNIT; i.methods.len()]);
        }
        for s in &prog.strings[self.strings.len()..] {
            let w = self.new_str(s)?;
            self.strings.push(w);
        }
        for (i, g) in prog.globals.iter().enumerate().skip(g0) {
            let w = match &g.kind {
                GlobalKind::Builtin { func } => {
                    self.global_init[i] = true;
                    self.closure_new(*func, &[])?
                }
                GlobalKind::Extern { func: Some(f), .. } => self.closure_new(*f, &[])?,
                GlobalKind::Extern { func: None, .. } => {
                    let p = self.alloc(2)?;
                    // SAFETY: p は 2 語の新しい領域である。
                    unsafe {
                        *p = header(Kind::ExternUnimpl, 0);
                        *p.add(1) = i as u64;
                    }
                    Word::from_ptr(p)
                }
                GlobalKind::Fn(f) | GlobalKind::Dictabs(f) => self.closure_new(*f, &[])?,
                GlobalKind::Value(_) => Word::UNIT,
            };
            self.globals[i] = w;
        }
        Ok(())
    }

    /// プログラムの形の表の後ろに足した形を、実行時の形の表に入れる([`Rt::grow`] の最初の段)。
    ///
    /// 番号が LIR の番号と一致することを確かめる。
    /// 形の表の確保は失敗を自分で扱う範囲で行い、広げられなければ [`Stop::OutOfMemory`] を返す。
    /// 失敗しても、それまでに入れた形が残るだけで、ほかの表には触れていない。
    /// 残った形は呼び直したときに数えて飛ばすので、REPL はこの失敗をその入力のメモリの不足として扱い、セッションを続けられる。
    /// 呼び直すと、失敗した入力の残りの形から入れるので、形を足さない入力でも前の入力の形のためにメモリの不足になることがある。
    pub fn grow_shapes(&mut self) -> Result<(), Stop> {
        let prog = self.prog;
        let mut pos = Vec::new();
        let have = self.shapes.count() as usize;
        for (i, s) in prog.shapes.iter().enumerate().skip(have) {
            let labels: Vec<u32> = s.iter().map(|l| l.0).collect();
            let id = {
                let _site = crate::alloc::site(crate::alloc::Site::Shape);
                crate::alloc::handled(|| self.shapes.intern(&labels, &mut pos))?
            };
            if id.0 as usize != i {
                return Err(Stop::Internal(format!(
                    "形の表の番号が合いません(LIR の {i} 番が実行時の {} 番)",
                    id.0
                )));
            }
        }
        Ok(())
    }
}
