//! LIR の関数から LLVM IR の文面を作る(計画 261024-1 の 6.8 節)。
//!
//! 1 つの LIR の関数を、`tailcc i64 @kfN(ptr %ctx)` の 1 つの関数にする(`N` は関数の番号)。
//! 規約は `keleut_interp::native` の冒頭にある。
//!
//! - 入口で、ネイティブのスタックの番地を文脈の下限と比べる(予備の検査)。
//!   次に、呼ぶ側が置いたフレームの位置と見出しへのポインタ、環境を文脈から読む。
//! - LIR のレジスタは関数の入口の `alloca` に置き(最適化で SSA の値になる)、引数はフレームのスロットから読む。
//! - 末尾でない呼び出しの前には、再開点で生きているレジスタをスロットに書き、見出しの 0 番の語に再開点を書く。
//!   yield を受け取らない再開点(`doc/lir.md` の 17 節)では、生の区画のレジスタを書かない(回収は `any` の区画だけを読む)。
//!   呼ばれた側が番兵で戻ったら `krt_sentinel` を呼んで自分も番兵で戻る。
//! - 確保する命令の前には、安全点で生きている `any` の区画のレジスタをスロットに書き、
//!   実行時の関数に今のフレームの位置と安全点の番号を渡す(実行時の関数が `GcState::at_safepoint` を行う)。
//! - 末尾の呼び出しは、自分のフレームの位置に呼ばれた側のフレームを置き直してから `musttail` で呼ぶ。
//! - 相手を実行時に決める呼び出しは `krt_dispatch` に任せる。
//!   値の呼び出しは、相手がクロージャなら実行時の関数を通さずに呼ぶ。
//!
//! 整数の `+ - *` は `nsw` を付けずに折り返し、`sdiv` と `srem` の前に除数 0 と `MIN / -1` の場合を分ける。
//! `Float64` から整数への変換は飽和する組み込み関数 `llvm.fptosi.sat` を使う(`doc/lir.md` の 6 節)。
//! `sin cos exp log` は rtcore の不透明な記号 `krt_sin` などを呼び、`sqrt` は `llvm.sqrt` にする。

use std::collections::HashMap;
use std::fmt::Write as _;

use keleut_interp::native::{dispatch, off};
use keleut_lir::{
    CmpOp, Const, ConvOp, FloatOp, Func, FuncId, Inst, IntOp, IntTy, MathFn, Program, Reg, Rep,
    ShowOp, Slot, Term,
};
use keleut_rt::Word;
use keleut_rt::stack::HEADER_WORDS;

/// 関数の LLVM IR の名前。
pub fn func_name(f: FuncId) -> String {
    format!("kf{}", f.0)
}

/// 生成した関数が呼ぶ実行時の関数と組み込み関数の宣言。
pub const DECLS: &str = "\
declare ptr @krt_prep(ptr, i64, i64, i64, i64, i64) nounwind
declare void @krt_sentinel(ptr, ptr, i64) nounwind
declare i64 @krt_dispatch(ptr, i64, i64, i64, i64, i64, i64, i64) nounwind
declare void @krt_depth_error(ptr) nounwind
declare void @krt_error(ptr, i64, i64, i64) nounwind
declare void @krt_unimpl(ptr, i64, i64, i64) nounwind
declare i64 @krt_box(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_str_concat(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_str_sub(ptr, i64, i64, i64, i64, i64) nounwind
declare i64 @krt_show(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_ctor_new(ptr, i64, i64, i64, i64, i64) nounwind
declare i64 @krt_variant_new(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_rec_new(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_rec_op(ptr, i64, i64, i64, i64, i64, i64) nounwind
declare i64 @krt_closure_new(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_method_new(ptr, i64, i64, i64, i64, i64, i64) nounwind
declare i64 @krt_dict_inst(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_dict_struct(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_ref_new(ptr, i64, i64, i64) nounwind
declare i64 @krt_marray_new(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_marray_freeze(ptr, i64, i64, i64) nounwind
declare i64 @krt_str_cmp(ptr, i64, i64, i64) nounwind
declare i64 @krt_rec_field(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_inst(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_array(ptr, i64, i64, i64, i64) nounwind
declare i64 @krt_console_write(ptr, i64) nounwind
declare double @krt_sin(double) nounwind
declare double @krt_cos(double) nounwind
declare double @krt_exp(double) nounwind
declare double @krt_log(double) nounwind
declare double @llvm.sqrt.f64(double)
declare double @llvm.trunc.f64(double)
declare i32 @llvm.fptosi.sat.i32.f64(double)
declare i64 @llvm.fptosi.sat.i64.f64(double)
declare ptr @llvm.addressofreturnaddress.p0()
";

/// 起動時に作るスタブのモジュール(`interp_stub` と `enter_tailcc`)の文面。`entry` は入口の表の番地。
pub fn stubs(entry: usize) -> String {
    format!(
        "\
declare i64 @krt_interp_run(ptr) nounwind

define tailcc i64 @interp_stub(ptr %ctx) nounwind {{
entry:
  %r = call i64 @krt_interp_run(ptr %ctx)
  %q = icmp eq i64 %r, {tailreq}
  br i1 %q, label %tail, label %done
tail:
  %cp = getelementptr i8, ptr %ctx, i64 {callee}
  %c = load i64, ptr %cp
  %tab = inttoptr i64 {entry} to ptr
  %ep = getelementptr ptr, ptr %tab, i64 %c
  %e = load atomic ptr, ptr %ep acquire, align 8
  %r2 = musttail call tailcc i64 %e(ptr %ctx)
  ret i64 %r2
done:
  ret i64 %r
}}

define i64 @enter_tailcc(ptr %ctx, ptr %f) nounwind {{
entry:
  %r = call tailcc i64 %f(ptr %ctx)
  ret i64 %r
}}
",
        tailreq = keleut_interp::native::TAILREQ.0,
        callee = off::CALLEE,
    )
}

fn ty(rep: Rep) -> &'static str {
    match rep {
        Rep::Bool => "i1",
        Rep::I32 => "i32",
        Rep::I64 | Rep::Str | Rep::Any => "i64",
        Rep::F64 => "double",
    }
}

fn is_any_class(rep: Rep) -> bool {
    matches!(rep, Rep::Any | Rep::Str)
}

fn cmp_i(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "eq",
        CmpOp::Lt => "slt",
        CmpOp::Le => "sle",
        CmpOp::Gt => "sgt",
        CmpOp::Ge => "sge",
    }
}

fn cmp_f(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "oeq",
        CmpOp::Lt => "olt",
        CmpOp::Le => "ole",
        CmpOp::Gt => "ogt",
        CmpOp::Ge => "oge",
    }
}

/// 呼び出しの相手。
enum Callee {
    /// 関数の番号(定数)。
    Direct(FuncId),
    /// 実行時に決める(`krt_dispatch` の操作、呼ぶ値か辞書のレジスタ、`クラス << 32 | メソッド`、メソッド自身の辞書)。
    Dynamic {
        op: u64,
        x: Reg,
        cm: u64,
        own: Vec<Reg>,
    },
}

struct Gen<'a> {
    f: &'a Func,
    fid: FuncId,
    entry: usize,
    out: String,
    n: usize,
    slot: Vec<u32>,
    fw: u64,
    rp_of: HashMap<(u32, u32), u32>,
    sp_of: HashMap<(u32, u32), u32>,
    tail_row: HashMap<u32, u32>,
    /// 今のブロックの名前(phi の入り口に使う)。
    cur: String,
}

impl Gen<'_> {
    fn t(&mut self) -> String {
        self.n += 1;
        format!("%t{}", self.n)
    }

    fn label(&mut self, base: &str) -> String {
        self.n += 1;
        format!("{base}{}", self.n)
    }

    fn line(&mut self, s: impl AsRef<str>) {
        self.out.push_str("  ");
        self.out.push_str(s.as_ref());
        self.out.push('\n');
    }

    fn start_block(&mut self, l: &str) {
        let _ = writeln!(self.out, "{l}:");
        self.cur = l.to_string();
    }

    fn rep(&self, r: Reg) -> Rep {
        self.f.regs[r.index()]
    }

    /// レジスタの値を読む。
    fn get(&mut self, r: Reg) -> String {
        let t = self.t();
        let ty = ty(self.rep(r));
        self.line(format!("{t} = load {ty}, ptr %r{}", r.0));
        t
    }

    /// レジスタの値を読み、一様な語の幅(`i64`)にする(生の値はスロットと同じ形)。
    fn get64(&mut self, r: Reg) -> String {
        let v = self.get(r);
        self.widen(self.rep(r), &v)
    }

    fn widen(&mut self, rep: Rep, v: &str) -> String {
        match rep {
            Rep::I64 | Rep::Str | Rep::Any => v.to_string(),
            Rep::I32 => {
                let t = self.t();
                self.line(format!("{t} = sext i32 {v} to i64"));
                t
            }
            Rep::Bool => {
                let t = self.t();
                self.line(format!("{t} = zext i1 {v} to i64"));
                t
            }
            Rep::F64 => {
                let t = self.t();
                self.line(format!("{t} = bitcast double {v} to i64"));
                t
            }
        }
    }

    fn set(&mut self, r: Reg, v: &str) {
        let ty = ty(self.rep(r));
        self.line(format!("store {ty} {v}, ptr %r{}", r.0));
    }

    /// 一様な語の幅の値 `v` を、レジスタ `r` の Rep にして書く。
    fn set64(&mut self, r: Reg, v: &str) {
        let v = match self.rep(r) {
            Rep::I64 | Rep::Str | Rep::Any => v.to_string(),
            Rep::I32 => {
                let t = self.t();
                self.line(format!("{t} = trunc i64 {v} to i32"));
                t
            }
            Rep::Bool => {
                let t = self.t();
                self.line(format!("{t} = trunc i64 {v} to i1"));
                t
            }
            Rep::F64 => {
                let t = self.t();
                self.line(format!("{t} = bitcast i64 {v} to double"));
                t
            }
        };
        self.set(r, &v);
    }

    /// フレームの語 `i`(見出しの 0〜3、スロットは 4 から)へのポインタ。
    fn frame_word(&mut self, i: u64) -> String {
        let t = self.t();
        self.line(format!("{t} = getelementptr i64, ptr %fp, i64 {i}"));
        t
    }

    /// レジスタをそのスロットに書く。
    fn spill(&mut self, r: Reg) {
        let v = self.get64(r);
        let p = self.frame_word(HEADER_WORDS as u64 + u64::from(self.slot[r.index()]));
        self.line(format!("store i64 {v}, ptr {p}"));
    }

    fn spill_set(&mut self, regs: &[Reg], raw_too: bool) {
        for &r in regs {
            if raw_too || is_any_class(self.rep(r)) {
                self.spill(r);
            }
        }
    }

    /// 見出しの 0 番の語に再開点 `row` を書く。
    fn set_row(&mut self, row: u32) {
        let w = u64::from(self.fid.0) | (u64::from(row) << 32);
        self.line(format!("store i64 {w}, ptr %fp"));
    }

    /// 文脈の引数の領域の `i` 番目に `v` を書く。
    fn put_arg(&mut self, i: usize, v: &str) {
        let p = self.t();
        self.line(format!("{p} = getelementptr i64, ptr %argp, i64 {i}"));
        self.line(format!("store i64 {v}, ptr {p}"));
    }

    fn put_args(&mut self, base: usize, regs: &[Reg]) {
        for (i, &r) in regs.iter().enumerate() {
            let v = self.get64(r);
            self.put_arg(base + i, &v);
        }
    }

    /// 箱 `w` の `i` 番の語を読む。
    fn load_box(&mut self, w: &str, i: u64) -> String {
        let p = self.t();
        self.line(format!("{p} = inttoptr i64 {w} to ptr"));
        let q = if i == 0 {
            p
        } else {
            let q = self.t();
            self.line(format!("{q} = getelementptr i64, ptr {p}, i64 {i}"));
            q
        };
        let t = self.t();
        self.line(format!("{t} = load i64, ptr {q}"));
        t
    }

    fn store_box(&mut self, w: &str, i: u64, v: &str) {
        let p = self.t();
        self.line(format!("{p} = inttoptr i64 {w} to ptr"));
        let q = self.t();
        self.line(format!("{q} = getelementptr i64, ptr {p}, i64 {i}"));
        self.line(format!("store i64 {v}, ptr {q}"));
    }

    /// 文脈の欄 `o`(語)を読む。
    fn ctx_load(&mut self, o: u64, ty: &str) -> String {
        let p = self.t();
        self.line(format!("{p} = getelementptr i8, ptr %ctx, i64 {o}"));
        let t = self.t();
        self.line(format!("{t} = load {ty}, ptr {p}"));
        t
    }

    /// 結果が 0 なら番兵で戻る(実行時の関数が状態語を立てた)。
    fn check_zero(&mut self, v: &str) {
        let c = self.t();
        self.line(format!("{c} = icmp eq i64 {v}, 0"));
        let ok = self.label("ok");
        self.line(format!("br i1 {c}, label %ret2, label %{ok}"));
        self.start_block(&ok);
    }

    /// 確保する命令の前の安全点。生きている `any` の区画のレジスタをスロットに書き、安全点の番号を返す。
    fn safepoint(&mut self, bi: usize, ii: usize) -> Result<u32, String> {
        let sp = *self
            .sp_of
            .get(&(bi as u32, ii as u32))
            .ok_or_else(|| format!("b{bi} の {ii} 番の確保に安全点がありません"))?;
        let fr = self.f.frame.as_ref().expect("フレームがある");
        let live = fr.safepoints.live(sp as usize);
        self.spill_set(&live, false);
        Ok(sp)
    }

    /// 呼び出しの後に番兵を調べる。番兵なら深さを書き戻して番兵で戻り、そうでなければ値を返す。
    fn after_call(&mut self, r: &str, yields: bool) {
        let s = self.t();
        self.line(format!("{s} = icmp eq i64 {r}, 2"));
        let sent = self.label("sent");
        let ok = self.label("ok");
        self.line(format!("br i1 {s}, label %{sent}, label %{ok}"));
        self.start_block(&sent);
        self.line(format!(
            "call void @krt_sentinel(ptr %ctx, ptr %fp, i64 {})",
            u64::from(yields)
        ));
        self.line("ret i64 2");
        self.start_block(&ok);
    }

    /// 入口の表の `fid` 番の番地。
    fn entry_of(&mut self, fid: &str) -> String {
        let tab = self.t();
        self.line(format!("{tab} = inttoptr i64 {} to ptr", self.entry));
        let p = self.t();
        self.line(format!("{p} = getelementptr ptr, ptr {tab}, i64 {fid}"));
        let e = self.t();
        // 翻訳の糸が入口を書き換えても競合にならないよう、獲得の原子的な読み込みにする(書く側は Release)。
        self.line(format!("{e} = load atomic ptr, ptr {p} acquire, align 8"));
        e
    }

    /// 置いたフレーム(見出しは `np`)のスロットに引数を書く。
    fn store_args(&mut self, np: &str, args: &[Reg]) {
        for (i, &r) in args.iter().enumerate() {
            let v = self.get64(r);
            let p = self.t();
            self.line(format!(
                "{p} = getelementptr i64, ptr {np}, i64 {}",
                HEADER_WORDS + i
            ));
            self.line(format!("store i64 {v}, ptr {p}"));
        }
    }

    /// 関数 `fid`(値)のフレームを置く。末尾なら自分の位置に、そうでなければ自分の上に。
    fn prep(&mut self, fid: &str, env: &str, tail: bool) -> String {
        let np = self.t();
        if tail {
            self.line(format!(
                "{np} = call ptr @krt_prep(ptr %ctx, i64 {fid}, i64 %fpos, i64 %fpos, i64 %w3, i64 {env})"
            ));
        } else {
            self.line(format!(
                "{np} = call ptr @krt_prep(ptr %ctx, i64 {fid}, i64 %fpos, i64 %top, i64 %fpos, i64 {env})"
            ));
        }
        let c = self.t();
        self.line(format!("{c} = icmp eq ptr {np}, null"));
        let ok = self.label("ok");
        self.line(format!("br i1 {c}, label %ret2, label %{ok}"));
        self.start_block(&ok);
        np
    }

    /// 呼び出し(末尾でないなら結果を `dst` に書く。末尾なら `musttail` で戻る)。
    ///
    /// `row` は見出しに書く再開点(末尾の形では末尾の形の行)、`live` は再開点で生きているレジスタ。
    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        callee: Callee,
        args: &[Reg],
        dst: Option<Reg>,
        row: Option<u32>,
        live: &[Reg],
        yields: bool,
        operands: &[Reg],
    ) {
        let tail = dst.is_none();
        if !tail {
            self.spill_set(live, yields);
        }
        if let Some(row) = row {
            self.set_row(row);
        }
        let done = self.label("done");
        match callee {
            Callee::Direct(f) => {
                let fid = f.0.to_string();
                let np = self.prep(&fid, "0", tail);
                self.store_args(&np, args);
                let e = self.entry_of(&fid);
                self.finish_call(&e, dst, yields, &done);
            }
            Callee::Dynamic { op, x, cm, own } => {
                let slow = self.label("slow");
                if op == dispatch::OP_VALUE {
                    // 相手がクロージャなら実行時の関数を通さずに呼ぶ。
                    let fv = self.get(x);
                    let lo = self.t();
                    self.line(format!("{lo} = and i64 {fv}, 7"));
                    let isp = self.t();
                    self.line(format!("{isp} = icmp eq i64 {lo}, 0"));
                    let nz = self.t();
                    self.line(format!("{nz} = icmp ne i64 {fv}, 0"));
                    let both = self.t();
                    self.line(format!("{both} = and i1 {isp}, {nz}"));
                    let chk = self.label("chk");
                    self.line(format!("br i1 {both}, label %{chk}, label %{slow}"));
                    self.start_block(&chk);
                    let h = self.load_box(&fv, 0);
                    let k = self.t();
                    self.line(format!("{k} = and i64 {h}, 255"));
                    let isc = self.t();
                    self.line(format!(
                        "{isc} = icmp eq i64 {k}, {}",
                        keleut_rt::value::Kind::Closure as u64
                    ));
                    let clo = self.label("clo");
                    self.line(format!("br i1 {isc}, label %{clo}, label %{slow}"));
                    self.start_block(&clo);
                    let fid = self.load_box(&fv, 1);
                    let np = self.prep(&fid, &fv, tail);
                    self.store_args(&np, args);
                    let e = self.entry_of(&fid);
                    self.finish_call(&e, dst, yields, &done);
                } else {
                    self.line(format!("br label %{slow}"));
                }
                // 実行時に決める道。実装の関数を評価する間(印 PHASE)は、呼び出しの命令の被演算子がスロットから根になる。
                self.start_block(&slow);
                for &r in operands {
                    self.spill(r);
                }
                let x0 = self.get(x);
                let disp = self.label("disp");
                let from = self.cur.clone();
                self.line(format!("br label %{disp}"));
                self.start_block(&disp);
                let phase_lbl = self.label("phase");
                let xv = self.t();
                let opv = self.t();
                let back = self.label("back");
                self.line(format!(
                    "{xv} = phi i64 [ {x0}, %{from} ], [ %fi{back}, %{back} ]"
                ));
                let op0 = op | if tail { dispatch::TAIL } else { 0 };
                let op1 = dispatch::OP_VALUE | if tail { dispatch::TAIL } else { 0 };
                self.line(format!(
                    "{opv} = phi i64 [ {op0}, %{from} ], [ {op1}, %{back} ]"
                ));
                self.put_args(0, args);
                self.put_args(args.len(), &own);
                let row = row.unwrap_or(0);
                let code = self.t();
                self.line(format!(
                    "{code} = call i64 @krt_dispatch(ptr %ctx, i64 {opv}, i64 %fpos, i64 {row}, i64 {xv}, i64 {cm}, i64 {}, i64 {})",
                    own.len(),
                    args.len()
                ));
                let call_l = self.label("dcall");
                let val_l = self.label("dval");
                self.line(format!(
                    "switch i64 {code}, label %ret2 [ i64 {}, label %{call_l} i64 {}, label %{phase_lbl} i64 {}, label %{val_l} ]",
                    dispatch::CALL,
                    dispatch::PHASE,
                    dispatch::VALUE
                ));
                // CALL: 実行時の関数がフレームを置いた。
                self.start_block(&call_l);
                let cid = self.ctx_load(off::CALLEE, "i64");
                let e = self.entry_of(&cid);
                self.finish_call(&e, dst, yields, &done);
                // PHASE: 実装の関数を末尾でなく呼び、戻った値を相手として選び直す。
                self.start_block(&phase_lbl);
                let cid = self.ctx_load(off::CALLEE, "i64");
                let e = self.entry_of(&cid);
                let fi = format!("%fi{back}");
                self.line(format!("{fi} = call tailcc i64 {e}(ptr %ctx)"));
                self.after_call(&fi, false);
                self.line(format!("br label %{back}"));
                self.start_block(&back);
                self.line(format!("br label %{disp}"));
                // VALUE: 結果は文脈にある。
                self.start_block(&val_l);
                let v = self.ctx_load(off::RESULT, "i64");
                match dst {
                    Some(d) => {
                        self.set64(d, &v);
                        self.line(format!("br label %{done}"));
                    }
                    None => self.line(format!("ret i64 {v}")),
                }
            }
        }
        if !tail {
            self.start_block(&done);
        }
    }

    /// 入口 `e` を呼ぶ。末尾でなければ番兵を調べて `dst` に書き、`done` へ移る。末尾なら `musttail`。
    fn finish_call(&mut self, e: &str, dst: Option<Reg>, yields: bool, done: &str) {
        match dst {
            Some(d) => {
                let r = self.t();
                self.line(format!("{r} = call tailcc i64 {e}(ptr %ctx)"));
                self.after_call(&r, yields);
                self.set(d, &r);
                self.line(format!("br label %{done}"));
            }
            None => {
                let r = self.t();
                self.line(format!("{r} = musttail call tailcc i64 {e}(ptr %ctx)"));
                self.line(format!("ret i64 {r}"));
            }
        }
    }

    fn int_arith(&mut self, ty: IntTy, op: IntOp, a: &str, b: &str) -> String {
        let t = match ty {
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
        };
        let min = match ty {
            IntTy::I32 => i64::from(i32::MIN),
            IntTy::I64 => i64::MIN,
        };
        let r = self.t();
        match op {
            IntOp::Add => self.line(format!("{r} = add {t} {a}, {b}")),
            IntOp::Sub => self.line(format!("{r} = sub {t} {a}, {b}")),
            IntOp::Mul => self.line(format!("{r} = mul {t} {a}, {b}")),
            IntOp::Div | IntOp::Rem => {
                let bz = self.t();
                self.line(format!("{bz} = icmp eq {t} {b}, 0"));
                let am = self.t();
                self.line(format!("{am} = icmp eq {t} {a}, {min}"));
                let bm = self.t();
                self.line(format!("{bm} = icmp eq {t} {b}, -1"));
                let ov = self.t();
                self.line(format!("{ov} = and i1 {am}, {bm}"));
                let bad = self.t();
                self.line(format!("{bad} = or i1 {bz}, {ov}"));
                let sb = self.t();
                self.line(format!("{sb} = select i1 {bad}, {t} 1, {t} {b}"));
                let q = self.t();
                let (ins, ovv) = if op == IntOp::Div {
                    ("sdiv", min.to_string())
                } else {
                    ("srem", "0".to_string())
                };
                self.line(format!("{q} = {ins} {t} {a}, {sb}"));
                let q2 = self.t();
                self.line(format!("{q2} = select i1 {ov}, {t} {ovv}, {t} {q}"));
                self.line(format!("{r} = select i1 {bz}, {t} 0, {t} {q2}"));
            }
        }
        r
    }

    fn inst(&mut self, bi: usize, ii: usize, inst: &Inst) -> Result<(), String> {
        let unit = Word::UNIT.0;
        match inst {
            Inst::Const { dst, value } => {
                let v = match value {
                    Const::Unit => unit.to_string(),
                    Const::Bool(b) => b.to_string(),
                    Const::I32(n) => n.to_string(),
                    Const::I64(n) => n.to_string(),
                    Const::F64(bits) => format!("0x{bits:016X}"),
                    Const::Str(s) => {
                        let p = self.t();
                        self.line(format!("{p} = getelementptr i64, ptr %strs, i64 {}", s.0));
                        let t = self.t();
                        self.line(format!("{t} = load i64, ptr {p}"));
                        t
                    }
                    Const::Ctor0 { ty, ctor } => Word::ctor0(ty.0, *ctor).0.to_string(),
                    Const::DictBuiltin { class, ty } => {
                        Word::dict_builtin(class.0, ty.0).0.to_string()
                    }
                    Const::DictInst0(i) => Word::dict_inst0(i.0).0.to_string(),
                };
                self.set(*dst, &v);
            }
            Inst::Move { dst, src } => {
                let v = self.get(*src);
                self.set(*dst, &v);
            }
            Inst::Box { dst, src } => {
                let v = self.get(*src);
                let w = match self.rep(*src) {
                    Rep::Bool => {
                        let t = self.t();
                        self.line(format!(
                            "{t} = select i1 {v}, i64 {}, i64 {}",
                            Word::TRUE.0,
                            Word::FALSE.0
                        ));
                        t
                    }
                    Rep::I32 => {
                        let a = self.t();
                        self.line(format!("{a} = sext i32 {v} to i64"));
                        let b = self.t();
                        self.line(format!("{b} = shl i64 {a}, 1"));
                        let c = self.t();
                        self.line(format!("{c} = or i64 {b}, 1"));
                        c
                    }
                    Rep::I64 | Rep::F64 => {
                        let sp = self.safepoint(bi, ii)?;
                        let (kind, bits) = if self.rep(*src) == Rep::I64 {
                            (0, v.clone())
                        } else {
                            let b = self.t();
                            self.line(format!("{b} = bitcast double {v} to i64"));
                            (1, b)
                        };
                        let t = self.t();
                        self.line(format!(
                            "{t} = call i64 @krt_box(ptr %ctx, i64 %fpos, i64 {sp}, i64 {kind}, i64 {bits})"
                        ));
                        self.check_zero(&t);
                        t
                    }
                    Rep::Str | Rep::Any => v,
                };
                self.set(*dst, &w);
            }
            Inst::Unbox { dst, src } => {
                let w = self.get(*src);
                let v = match self.rep(*dst) {
                    Rep::Bool => {
                        let a = self.t();
                        self.line(format!("{a} = lshr i64 {w}, 1"));
                        let b = self.t();
                        self.line(format!("{b} = trunc i64 {a} to i1"));
                        b
                    }
                    Rep::I32 => {
                        let a = self.t();
                        self.line(format!("{a} = ashr i64 {w}, 1"));
                        let b = self.t();
                        self.line(format!("{b} = trunc i64 {a} to i32"));
                        b
                    }
                    Rep::I64 => self.load_box(&w, 1),
                    Rep::F64 => {
                        let a = self.load_box(&w, 1);
                        let b = self.t();
                        self.line(format!("{b} = bitcast i64 {a} to double"));
                        b
                    }
                    Rep::Str | Rep::Any => w,
                };
                self.set(*dst, &v);
            }
            Inst::EnvGet { dst, index } => {
                let v = self.load_box("%env", 2 + u64::from(*index));
                self.set(*dst, &v);
            }
            Inst::DepthEnter => {
                let d = self.t();
                self.line(format!("{d} = load i64, ptr %dp"));
                let p = self.frame_word(1);
                self.line(format!("store i64 {d}, ptr {p}"));
                self.line(format!("store i64 {d}, ptr %fdepth"));
            }
            Inst::DepthCheck | Inst::DepthPush => {
                let d = self.t();
                self.line(format!("{d} = load i64, ptr %dp"));
                let l = self.t();
                self.line(format!("{l} = load i64, ptr %lp"));
                let d1 = self.t();
                self.line(format!("{d1} = add i64 {d}, 1"));
                let bad = self.t();
                self.line(format!("{bad} = icmp ugt i64 {d1}, {l}"));
                let ok = self.label("ok");
                self.line(format!("br i1 {bad}, label %deptherr, label %{ok}"));
                self.start_block(&ok);
                if matches!(inst, Inst::DepthPush) {
                    self.line(format!("store i64 {d1}, ptr %dp"));
                }
            }
            Inst::DepthRestore => {
                let d = self.t();
                self.line(format!("{d} = load i64, ptr %fdepth"));
                self.line(format!("store i64 {d}, ptr %dp"));
            }
            Inst::IntArith { ty, op, dst, a, b } => {
                let (x, y) = (self.get(*a), self.get(*b));
                let r = self.int_arith(*ty, *op, &x, &y);
                self.set(*dst, &r);
            }
            Inst::IntNeg { ty, dst, a } => {
                let x = self.get(*a);
                let t = if *ty == IntTy::I32 { "i32" } else { "i64" };
                let r = self.t();
                self.line(format!("{r} = sub {t} 0, {x}"));
                self.set(*dst, &r);
            }
            Inst::IntCmp { ty, op, dst, a, b } => {
                let (x, y) = (self.get(*a), self.get(*b));
                let t = if *ty == IntTy::I32 { "i32" } else { "i64" };
                let r = self.t();
                self.line(format!("{r} = icmp {} {t} {x}, {y}", cmp_i(*op)));
                self.set(*dst, &r);
            }
            Inst::FloatArith { op, dst, a, b } => {
                let (x, y) = (self.get(*a), self.get(*b));
                let ins = match op {
                    FloatOp::Add => "fadd",
                    FloatOp::Sub => "fsub",
                    FloatOp::Mul => "fmul",
                    FloatOp::Div => "fdiv",
                };
                let r = self.t();
                self.line(format!("{r} = {ins} double {x}, {y}"));
                self.set(*dst, &r);
            }
            Inst::FloatNeg { dst, a } => {
                let x = self.get(*a);
                let r = self.t();
                self.line(format!("{r} = fneg double {x}"));
                self.set(*dst, &r);
            }
            Inst::FloatCmp { op, dst, a, b } => {
                let (x, y) = (self.get(*a), self.get(*b));
                let r = self.t();
                self.line(format!("{r} = fcmp {} double {x}, {y}", cmp_f(*op)));
                self.set(*dst, &r);
            }
            Inst::FloatMath { f, dst, a } => {
                let x = self.get(*a);
                let name = match f {
                    MathFn::Sin => "krt_sin",
                    MathFn::Cos => "krt_cos",
                    MathFn::Sqrt => "llvm.sqrt.f64",
                    MathFn::Exp => "krt_exp",
                    MathFn::Log => "krt_log",
                };
                let r = self.t();
                self.line(format!("{r} = call double @{name}(double {x})"));
                self.set(*dst, &r);
            }
            Inst::FloatTrunc { dst, a } => {
                let x = self.get(*a);
                let r = self.t();
                self.line(format!("{r} = call double @llvm.trunc.f64(double {x})"));
                self.set(*dst, &r);
            }
            Inst::FloatIsNan { dst, a } => {
                let x = self.get(*a);
                let r = self.t();
                self.line(format!("{r} = fcmp uno double {x}, {x}"));
                self.set(*dst, &r);
            }
            Inst::Conv { op, dst, a } => {
                let x = self.get(*a);
                let r = self.t();
                let s = match op {
                    ConvOp::I32ToI64 => format!("sext i32 {x} to i64"),
                    ConvOp::I32ToF64 => format!("sitofp i32 {x} to double"),
                    ConvOp::I64ToI32 => format!("trunc i64 {x} to i32"),
                    ConvOp::I64ToF64 => format!("sitofp i64 {x} to double"),
                    ConvOp::F64ToI32 => format!("call i32 @llvm.fptosi.sat.i32.f64(double {x})"),
                    ConvOp::F64ToI64 => format!("call i64 @llvm.fptosi.sat.i64.f64(double {x})"),
                };
                self.line(format!("{r} = {s}"));
                self.set(*dst, &r);
            }
            Inst::BoolNot { dst, a } => {
                let x = self.get(*a);
                let r = self.t();
                self.line(format!("{r} = xor i1 {x}, true"));
                self.set(*dst, &r);
            }
            Inst::BoolEq { dst, a, b } => {
                let (x, y) = (self.get(*a), self.get(*b));
                let r = self.t();
                self.line(format!("{r} = icmp eq i1 {x}, {y}"));
                self.set(*dst, &r);
            }
            Inst::StrConcat { dst, a, b } => {
                let sp = self.safepoint(bi, ii)?;
                let (x, y) = (self.get(*a), self.get(*b));
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_str_concat(ptr %ctx, i64 %fpos, i64 {sp}, i64 {x}, i64 {y})"
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::StrCmp { op, dst, a, b } => {
                let (x, y) = (self.get(*a), self.get(*b));
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_str_cmp(ptr %ctx, i64 {}, i64 {x}, i64 {y})",
                    keleut_interp::bytecode::cmp_sub(*op)
                ));
                self.set64(*dst, &r);
            }
            Inst::StrLen { dst, a } | Inst::ArrayLen { dst, a } | Inst::MArrayLen { dst, a } => {
                let x = self.get(*a);
                let h = self.load_box(&x, 0);
                let n = self.t();
                self.line(format!("{n} = lshr i64 {h}, 32"));
                self.set64(*dst, &n);
            }
            Inst::StrSub { dst, s, pos, len } => {
                let sp = self.safepoint(bi, ii)?;
                let x = self.get(*s);
                let p = self.get64(*pos);
                let l = self.get64(*len);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_str_sub(ptr %ctx, i64 %fpos, i64 {sp}, i64 {x}, i64 {p}, i64 {l})"
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::Show { op, dst, a } => {
                let sp = self.safepoint(bi, ii)?;
                let x = self.get64(*a);
                let o = match op {
                    ShowOp::I32 => 0,
                    ShowOp::I64 => 1,
                    ShowOp::F64 => 2,
                    ShowOp::Bool => 3,
                };
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_show(ptr %ctx, i64 %fpos, i64 {sp}, i64 {o}, i64 {x})"
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::CtorNew {
                dst,
                ty,
                ctor,
                fields,
            } => {
                let sp = self.safepoint(bi, ii)?;
                self.put_args(0, fields);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_ctor_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {ctor}, i64 {})",
                    ty.0,
                    fields.len()
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::CtorTag { dst, v } => {
                let w = self.get(*v);
                let r = self.imm_or_box(
                    &w,
                    |g, w| {
                        let a = g.t();
                        g.line(format!("{a} = lshr i64 {w}, 1"));
                        let b = g.t();
                        g.line(format!("{b} = and i64 {a}, 16777215"));
                        b
                    },
                    |g, w| {
                        let x = g.load_box(w, 1);
                        let b = g.t();
                        g.line(format!("{b} = and i64 {x}, 4294967295"));
                        b
                    },
                );
                self.set64(*dst, &r);
            }
            Inst::CtorField { dst, v, index } | Inst::RecGet { dst, rec: v, index } => {
                let w = self.get(*v);
                let i = u64::from(*index);
                let r = self.imm_or_box(
                    &w,
                    |_, _| unit.to_string(),
                    |g, w| {
                        let h = g.load_box(w, 0);
                        let n = g.t();
                        g.line(format!("{n} = lshr i64 {h}, 32"));
                        let inr = g.t();
                        g.line(format!("{inr} = icmp ult i64 {i}, {n}"));
                        let yes = g.label("in");
                        let no = g.label("out");
                        let join = g.label("join");
                        g.line(format!("br i1 {inr}, label %{yes}, label %{no}"));
                        g.start_block(&yes);
                        let x = g.load_box(w, 2 + i);
                        g.line(format!("br label %{join}"));
                        g.start_block(&no);
                        g.line(format!("br label %{join}"));
                        g.start_block(&join);
                        let p = g.t();
                        g.line(format!("{p} = phi i64 [ {x}, %{yes} ], [ {unit}, %{no} ]"));
                        p
                    },
                );
                self.set(*dst, &r);
            }
            Inst::VariantNew {
                dst,
                label,
                payload,
            } => {
                let sp = self.safepoint(bi, ii)?;
                let p = self.get(*payload);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_variant_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {p})",
                    label.0
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::VariantTag { dst, v } => {
                let w = self.get(*v);
                let x = self.load_box(&w, 1);
                self.set64(*dst, &x);
            }
            Inst::VariantPayload { dst, v } => {
                let w = self.get(*v);
                let x = self.load_box(&w, 2);
                self.set(*dst, &x);
            }
            Inst::RecNew { dst, shape, fields } => {
                let sp = self.safepoint(bi, ii)?;
                self.put_args(0, fields);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_rec_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {})",
                    shape.0,
                    fields.len()
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::RecExtend {
                dst,
                rec,
                label,
                value,
            }
            | Inst::RecUpdate {
                dst,
                rec,
                label,
                value,
            } => {
                let op = if matches!(inst, Inst::RecExtend { .. }) {
                    0
                } else {
                    2
                };
                let sp = self.safepoint(bi, ii)?;
                let v = self.get(*rec);
                let x = self.get(*value);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_rec_op(ptr %ctx, i64 %fpos, i64 {sp}, i64 {op}, i64 {v}, i64 {}, i64 {x})",
                    label.0
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::RecRestrict { dst, rec, label } => {
                let sp = self.safepoint(bi, ii)?;
                let v = self.get(*rec);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_rec_op(ptr %ctx, i64 %fpos, i64 {sp}, i64 1, i64 {v}, i64 {}, i64 0)",
                    label.0
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::RecField {
                dst,
                rec,
                label,
                nth,
            }
            | Inst::RecHas {
                dst,
                rec,
                label,
                nth,
            } => {
                let has = u64::from(matches!(inst, Inst::RecHas { .. }));
                let v = self.get(*rec);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_rec_field(ptr %ctx, i64 {has}, i64 {v}, i64 {}, i64 {nth})",
                    label.0
                ));
                self.set64(*dst, &r);
            }
            Inst::ClosureNew { dst, func, caps } => {
                let sp = self.safepoint(bi, ii)?;
                self.put_args(0, caps);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_closure_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {})",
                    func.0,
                    caps.len()
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::ClosureSetCap { clo, index, value } => {
                let c = self.get(*clo);
                let x = self.get(*value);
                self.store_box(&c, 2 + u64::from(*index), &x);
            }
            Inst::MethodNew {
                dst,
                class,
                method,
                dict,
                own,
            } => {
                let sp = self.safepoint(bi, ii)?;
                self.put_args(0, own);
                let d = self.get(*dict);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_method_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {method}, i64 {d}, i64 {})",
                    class.0,
                    own.len()
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::DictInst {
                dst,
                inst,
                premises,
            } => {
                let sp = self.safepoint(bi, ii)?;
                self.put_args(0, premises);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_dict_inst(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {})",
                    inst.0,
                    premises.len()
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::DictRecord { dst, fields } | Inst::DictVariant { dst, fields } => {
                let kind = u64::from(matches!(inst, Inst::DictVariant { .. }));
                let sp = self.safepoint(bi, ii)?;
                for (i, (l, r)) in fields.iter().enumerate() {
                    self.put_arg(2 * i, &l.0.to_string());
                    let v = self.get(*r);
                    self.put_arg(2 * i + 1, &v);
                }
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_dict_struct(ptr %ctx, i64 %fpos, i64 {sp}, i64 {kind}, i64 {})",
                    fields.len()
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::GlobalGet { dst, global } => {
                let p = self.t();
                self.line(format!(
                    "{p} = getelementptr i64, ptr %glob, i64 {}",
                    global.0
                ));
                let v = self.t();
                self.line(format!("{v} = load i64, ptr {p}"));
                self.set(*dst, &v);
            }
            Inst::GlobalIsInit { dst, global } => {
                let p = self.t();
                self.line(format!(
                    "{p} = getelementptr i8, ptr %ginit, i64 {}",
                    global.0
                ));
                let v = self.t();
                self.line(format!("{v} = load i8, ptr {p}"));
                let b = self.t();
                self.line(format!("{b} = icmp ne i8 {v}, 0"));
                self.set(*dst, &b);
            }
            Inst::GlobalSet { global, src } => {
                let v = self.get(*src);
                let p = self.t();
                self.line(format!(
                    "{p} = getelementptr i64, ptr %glob, i64 {}",
                    global.0
                ));
                self.line(format!("store i64 {v}, ptr {p}"));
            }
            Inst::GlobalMarkInit { global } => {
                let p = self.t();
                self.line(format!(
                    "{p} = getelementptr i8, ptr %ginit, i64 {}",
                    global.0
                ));
                self.line(format!("store i8 1, ptr {p}"));
            }
            Inst::InstIsInit { dst, inst } => {
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_inst(ptr %ctx, i64 0, i64 {}, i64 0, i64 0)",
                    inst.0
                ));
                self.set64(*dst, &r);
            }
            Inst::InstMarkInit { inst } => {
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_inst(ptr %ctx, i64 1, i64 {}, i64 0, i64 0)",
                    inst.0
                ));
            }
            Inst::InstSetValue { inst, method, src } => {
                let v = self.get(*src);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_inst(ptr %ctx, i64 2, i64 {}, i64 {method}, i64 {v})",
                    inst.0
                ));
            }
            Inst::InstGetValue { dst, inst, method } => {
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_inst(ptr %ctx, i64 3, i64 {}, i64 {method}, i64 0)",
                    inst.0
                ));
                self.set(*dst, &r);
            }
            Inst::RefNew { dst, value } => {
                let sp = self.safepoint(bi, ii)?;
                let x = self.get(*value);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_ref_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {x})"
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::RefGet { dst, r } => {
                let c = self.get(*r);
                let v = self.load_box(&c, 1);
                self.set(*dst, &v);
            }
            Inst::RefSet { r, value } => {
                let c = self.get(*r);
                let x = self.get(*value);
                self.store_box(&c, 1, &x);
            }
            Inst::ArrayGet { dst, a, index } | Inst::MArrayGet { dst, a, index } => {
                let x = self.get(*a);
                let i = self.get64(*index);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_array(ptr %ctx, i64 0, i64 {x}, i64 {i}, i64 0)"
                ));
                self.set(*dst, &r);
            }
            Inst::MArraySet { a, index, value } => {
                let x = self.get(*a);
                let i = self.get64(*index);
                let v = self.get(*value);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_array(ptr %ctx, i64 1, i64 {x}, i64 {i}, i64 {v})"
                ));
            }
            Inst::MArrayNew { dst, len, init } => {
                let sp = self.safepoint(bi, ii)?;
                let n = self.get64(*len);
                let x = self.get(*init);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_marray_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {n}, i64 {x})"
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::MArrayFreeze { dst, a } => {
                let sp = self.safepoint(bi, ii)?;
                let x = self.get(*a);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_marray_freeze(ptr %ctx, i64 %fpos, i64 {sp}, i64 {x})"
                ));
                self.check_zero(&r);
                self.set(*dst, &r);
            }
            Inst::CallDirect {
                dst,
                func,
                args,
                yields: _,
            } => {
                let (rp, live, y) = self.resume(bi, ii)?;
                self.call(
                    Callee::Direct(*func),
                    args,
                    Some(*dst),
                    Some(rp),
                    &live,
                    y,
                    &[],
                );
            }
            Inst::CallValue {
                dst, callee, args, ..
            } => {
                let (rp, live, y) = self.resume(bi, ii)?;
                let ops = inst.uses();
                self.call(
                    Callee::Dynamic {
                        op: dispatch::OP_VALUE,
                        x: *callee,
                        cm: 0,
                        own: Vec::new(),
                    },
                    args,
                    Some(*dst),
                    Some(rp),
                    &live,
                    y,
                    &ops,
                );
            }
            Inst::CallMethod {
                dst,
                class,
                method,
                dict,
                own,
                args,
                ..
            } => {
                let (rp, live, y) = self.resume(bi, ii)?;
                let ops = inst.uses();
                self.call(
                    Callee::Dynamic {
                        op: dispatch::OP_METHOD,
                        x: *dict,
                        cm: (u64::from(class.0) << 32) | u64::from(*method),
                        own: own.clone(),
                    },
                    args,
                    Some(*dst),
                    Some(rp),
                    &live,
                    y,
                    &ops,
                );
            }
            Inst::DictApp { dst, f, dicts } => {
                let (rp, live, y) = self.resume(bi, ii)?;
                self.call(
                    Callee::Dynamic {
                        op: dispatch::OP_DICTAPP,
                        x: *f,
                        cm: 0,
                        own: Vec::new(),
                    },
                    dicts,
                    Some(*dst),
                    Some(rp),
                    &live,
                    y,
                    &[],
                );
            }
            Inst::ConsoleWrite { dst, s } => {
                let x = self.get(*s);
                let r = self.t();
                self.line(format!(
                    "{r} = call i64 @krt_console_write(ptr %ctx, i64 {x})"
                ));
                let c = self.t();
                self.line(format!("{c} = icmp ne i64 {r}, 0"));
                let ok = self.label("ok");
                self.line(format!("br i1 {c}, label %ret2, label %{ok}"));
                self.start_block(&ok);
                self.set64(*dst, &unit.to_string());
            }
            Inst::UnimplementedValue { .. } => {
                self.line(format!(
                    "call void @krt_unimpl(ptr %ctx, i64 {}, i64 {bi}, i64 {ii})",
                    self.fid.0
                ));
                self.line("ret i64 2");
                let dead = self.label("dead");
                self.start_block(&dead);
            }
            Inst::Fx { .. } => return Err("エフェクトの命令は JIT にできません".into()),
        }
        Ok(())
    }

    /// 語 `w` が即値なら `imm`、箱なら `boxed` の値を選ぶ(どちらも i64 の値を作る)。
    fn imm_or_box(
        &mut self,
        w: &str,
        imm: impl FnOnce(&mut Self, &str) -> String,
        boxed: impl FnOnce(&mut Self, &str) -> String,
    ) -> String {
        let lo = self.t();
        self.line(format!("{lo} = and i64 {w}, 1"));
        let isimm = self.t();
        self.line(format!("{isimm} = icmp ne i64 {lo}, 0"));
        let li = self.label("imm");
        let lb = self.label("box");
        let join = self.label("join");
        self.line(format!("br i1 {isimm}, label %{li}, label %{lb}"));
        self.start_block(&li);
        let a = imm(self, w);
        let la = self.cur.clone();
        self.line(format!("br label %{join}"));
        self.start_block(&lb);
        let b = boxed(self, w);
        let lbb = self.cur.clone();
        self.line(format!("br label %{join}"));
        self.start_block(&join);
        let p = self.t();
        self.line(format!("{p} = phi i64 [ {a}, %{la} ], [ {b}, %{lbb} ]"));
        p
    }

    /// 命令の再開点の番号、そこで生きているレジスタ、yield を受け取りうるか。
    fn resume(&self, bi: usize, ii: usize) -> Result<(u32, Vec<Reg>, bool), String> {
        let rp = *self
            .rp_of
            .get(&(bi as u32, ii as u32))
            .ok_or_else(|| format!("b{bi} の {ii} 番の呼び出しに再開点がありません"))?;
        let fr = self.f.frame.as_ref().expect("フレームがある");
        let live = fr.resume_points.live(rp as usize);
        let y = fr.yield_points.get(rp as usize).copied().unwrap_or(true);
        Ok((rp, live, y))
    }

    fn term(&mut self, bi: usize, t: &Term) -> Result<(), String> {
        match t {
            Term::Jump(b) => self.line(format!("br label %b{}", b.0)),
            Term::Branch { cond, then, else_ } => {
                let c = self.get(*cond);
                self.line(format!(
                    "br i1 {c}, label %b{}, label %b{}",
                    then.0, else_.0
                ));
            }
            Term::Return(r) => {
                let v = self.get(*r);
                self.line(format!("ret i64 {v}"));
            }
            Term::TailCallDirect { func, args, .. } => {
                self.call(Callee::Direct(*func), args, None, None, &[], true, &[]);
            }
            Term::TailCallValue { callee, args, .. } => {
                let row = self.tail_row[&(bi as u32)];
                let ops = t.uses();
                self.call(
                    Callee::Dynamic {
                        op: dispatch::OP_VALUE,
                        x: *callee,
                        cm: 0,
                        own: Vec::new(),
                    },
                    args,
                    None,
                    Some(row),
                    &[],
                    true,
                    &ops,
                );
            }
            Term::TailCallMethod {
                class,
                method,
                dict,
                own,
                args,
                ..
            } => {
                let row = self.tail_row[&(bi as u32)];
                let ops = t.uses();
                self.call(
                    Callee::Dynamic {
                        op: dispatch::OP_METHOD,
                        x: *dict,
                        cm: (u64::from(class.0) << 32) | u64::from(*method),
                        own: own.clone(),
                    },
                    args,
                    None,
                    Some(row),
                    &[],
                    true,
                    &ops,
                );
            }
            Term::Error { operand, .. } => {
                let o = match operand {
                    Some(r) => self.get64(*r),
                    None => "0".to_string(),
                };
                self.line(format!(
                    "call void @krt_error(ptr %ctx, i64 {}, i64 {bi}, i64 {o})",
                    self.fid.0
                ));
                self.line("ret i64 2");
            }
            Term::Unimplemented { .. } => {
                self.line(format!(
                    "call void @krt_unimpl(ptr %ctx, i64 {}, i64 {bi}, i64 {})",
                    self.fid.0,
                    u64::MAX
                ));
                self.line("ret i64 2");
            }
            Term::Fx { .. } => return Err("エフェクトのブロックの終わりは JIT にできません".into()),
        }
        Ok(())
    }
}

/// 関数 `fid` の LLVM IR の関数の定義を作る。`entry` は入口の表の番地。
///
/// 関数は `keleut_interp::native::jit_eligible` を満たし、パス 12 を済ませていなければならない。
pub fn function(prog: &Program, fid: FuncId, entry: usize) -> Result<String, String> {
    let f = &prog.funcs[fid.index()];
    if !keleut_interp::native::jit_eligible(f) {
        return Err(format!("関数 {} は JIT にできません", f.sym));
    }
    let fr = f.frame.as_ref().ok_or("フレームがありません")?;
    let any = fr.any_slots;
    let slot: Vec<u32> = fr
        .slot
        .iter()
        .map(|s| match *s {
            Slot::Any(n) => n,
            Slot::Raw(n) => any + n,
        })
        .collect();
    let mut rp_of = HashMap::new();
    for (i, p) in fr.resume_points.points().iter().enumerate() {
        rp_of.insert((p.block.0, p.index), i as u32);
    }
    let mut sp_of = HashMap::new();
    for (i, p) in fr.safepoints.points().iter().enumerate() {
        sp_of.insert((p.block.0, p.index), i as u32);
    }
    let nrp = fr.resume_points.len() as u32;
    let mut tail_row = HashMap::new();
    let mut k = 0;
    for (bi, b) in f.blocks.iter().enumerate() {
        if matches!(
            b.term,
            Term::TailCallValue { .. } | Term::TailCallMethod { .. }
        ) {
            tail_row.insert(bi as u32, nrp + k);
            k += 1;
        }
    }
    let fw = (HEADER_WORDS as u32 + any + fr.raw_slots) as u64;
    let mut g = Gen {
        f,
        fid,
        entry,
        out: String::new(),
        n: 0,
        slot,
        fw,
        rp_of,
        sp_of,
        tail_row,
        cur: String::new(),
    };
    let _ = writeln!(
        g.out,
        "; {} ({})\ndefine tailcc i64 @{}(ptr %ctx) nounwind {{\nentry:",
        f.sym,
        f.kind.name(),
        func_name(fid)
    );
    for (i, &rep) in f.regs.iter().enumerate() {
        g.line(format!("%r{i} = alloca {}", ty(rep)));
    }
    g.line("%fdepth = alloca i64");
    // ネイティブのスタックが下限(予備の検査の余裕と SOFT_MARGIN)に近ければ、
    // 置いたフレームのままインタプリタの起動に移る(`keleut_interp::native::SOFT_MARGIN`)。
    // 予備の検査そのもの(計画 6.8 節)は、インタプリタの起動の入口が行う。
    g.line("%ra = call ptr @llvm.addressofreturnaddress.p0()");
    g.line("%rai = ptrtoint ptr %ra to i64");
    g.line(format!(
        "%lowp = getelementptr i8, ptr %ctx, i64 {}",
        off::STACK_SOFT
    ));
    g.line("%low = load i64, ptr %lowp");
    g.line("%sov = icmp ult i64 %rai, %low");
    g.line("br i1 %sov, label %lowstack, label %go");
    g.start_block("lowstack");
    g.line(format!(
        "%forcep = getelementptr i8, ptr %ctx, i64 {}",
        off::FORCE_INTERP
    ));
    g.line("store i64 1, ptr %forcep");
    g.line(format!(
        "%stubp = getelementptr i8, ptr %ctx, i64 {}",
        off::STUB
    ));
    g.line("%stub = load ptr, ptr %stubp");
    g.line("%rlow = musttail call tailcc i64 %stub(ptr %ctx)");
    g.line("ret i64 %rlow");
    g.start_block("go");
    for (name, o, t) in [
        ("fp", off::CUR_FP, "ptr"),
        ("fpos", off::CUR_POS, "i64"),
        ("env", off::ENV, "i64"),
        ("dp", off::DEPTH, "ptr"),
        ("lp", off::LIMIT, "ptr"),
        ("argp", off::ARGS, "ptr"),
        ("glob", off::GLOBALS, "ptr"),
        ("ginit", off::GLOBAL_INIT, "ptr"),
        ("strs", off::STRINGS, "ptr"),
    ] {
        g.line(format!("%{name}.p = getelementptr i8, ptr %ctx, i64 {o}"));
        g.line(format!("%{name} = load {t}, ptr %{name}.p"));
    }
    g.line("%w3p = getelementptr i64, ptr %fp, i64 3");
    g.line("%w3 = load i64, ptr %w3p");
    g.line(format!("%top = add i64 %fpos, {}", g.fw));
    for (i, &p) in f.params.iter().enumerate() {
        let sp = g.frame_word((HEADER_WORDS + i) as u64);
        let v = g.t();
        g.line(format!("{v} = load i64, ptr {sp}"));
        g.set64(p, &v);
    }
    g.line("br label %b0");
    for (bi, b) in f.blocks.iter().enumerate() {
        g.start_block(&format!("b{bi}"));
        for (ii, inst) in b.insts.iter().enumerate() {
            g.inst(bi, ii, inst)?;
        }
        g.term(bi, &b.term)?;
    }
    g.start_block("ret2");
    g.line("ret i64 2");
    g.start_block("deptherr");
    g.line("call void @krt_depth_error(ptr %ctx)");
    g.line("ret i64 2");
    g.out.push_str("}\n");
    Ok(g.out)
}

/// 関数 `fid` の 1 つのモジュールの文面(宣言を含む)。
pub fn module(
    prog: &Program,
    fid: FuncId,
    entry: usize,
    triple: &str,
    layout: &str,
) -> Result<String, String> {
    let body = function(prog, fid, entry)?;
    let mut s = String::new();
    if !layout.is_empty() {
        let _ = writeln!(s, "target datalayout = \"{layout}\"");
    }
    if !triple.is_empty() {
        let _ = writeln!(s, "target triple = \"{triple}\"");
    }
    s.push_str(DECLS);
    s.push('\n');
    s.push_str(&body);
    Ok(s)
}
