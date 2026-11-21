//! 単位を追加したプログラムへの、コンテキストの差し替え(`keleut repl`)。
//!
//! REPL は、受理した入力ごとに KIR の単位(コンパイル単位。ここでは REPL の入力 1 つ)を 1 つプログラムの後ろに追加する。
//! 追加した単位だけを LIR に変換し、LIR のプログラムの各表の後ろに追加する(`keleut_lir::Extender`)。
//! それまでの関数、グローバル、インスタンス、シェイプ、文字列の添字は変わらないので、ヒープに残った値はそのまま使える。
//!
//! [`Rt`] はプログラムを借用しているので、プログラムに単位を追加する間は、コンテキストが指すプログラムを別のものに差し替えておく。
//! [`Rt::rebind`] はコンテキストを消費して、ヒープ、GC ルート、スタック、表をそのまま別のプログラムのコンテキストに移す。
//! 追加する間は空のプログラム(`keleut_lir::Program::empty`)に差し替えておき、
//! 追加が済んだら、単位を追加したプログラムに差し替えて [`Rt::grow`] で表を拡張する。

use crate::ctx::Rt;
use crate::error::Stop;
use crate::value::{Kind, Word, header};
use keleut_lir::{GlobalKind, Program};

impl<'p> Rt<'p> {
    /// コンテキストが指すプログラムを `prog` に差し替える。表は拡張しない(拡張は [`Rt::grow`] が行う)。
    ///
    /// `prog` は、現在のプログラムと同じか、その各表の後ろに項目を追加したものか、
    /// コンテキストを使わない間だけ置いておく空のプログラムでなければならない。
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

    /// プログラムの各表の後ろに追加した項目の分だけ、コンテキストの表を拡張する。規則は [`Rt::new_with`] と同じである。
    ///
    /// 追加したシェイプを [`Rt::grow_shapes`] でランタイムのシェイプ表に入れてから、ほかの表を拡張する。
    /// 追加した文字列定数と、追加したグローバルのうち関数を値とするもののクロージャを確保する。
    /// グローバルの表は先に Unit で拡張してから値を置くので、その間に GC が起きても、GC はそれまでのグローバルの値を GC ルートとして走査する。
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
                    // SAFETY: p は新しく確保した 2 ワードの領域を指す。
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

    /// プログラムのシェイプ表の後ろに追加したシェイプを、ランタイムのシェイプ表に入れる。[`Rt::grow`] の最初の段階である。
    ///
    /// ランタイムの番号が LIR の番号と一致することを検査し、一致しなければ内部エラーを返す。
    /// シェイプ表の確保は、確保の失敗を呼び出し元が処理する範囲([`crate::alloc::handled`])で行い、拡張できなければ [`Stop::OutOfMemory`] を返す。
    /// 失敗しても、それまでに入れたシェイプが残るだけで、ほかの表は変わらない。
    /// 失敗の後に呼び直すと、すでに入れたシェイプを個数で数えて読み飛ばし、残りのシェイプから入れる。
    /// そのため、REPL はこの失敗をその入力のメモリ不足として扱い、セッションを続けられる。
    /// ただし、失敗した入力の残りのシェイプは次の入力のときに入れるので、シェイプを追加しない入力でも、前の入力のシェイプのためにメモリ不足になることがある。
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
