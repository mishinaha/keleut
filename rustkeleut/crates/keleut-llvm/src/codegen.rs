//! LIR の関数から LLVM IR の文面を作る(計画 261024-1 の 6.8 節)。
//!
//! 1 つの LIR の関数を、レジスタの入口 `tailcc i64 @kfN.r(ptr %ctx, i64 %env, i64 ×4)` と
//! 値スタックの入口 `tailcc i64 @kfN(ptr %ctx)` の 2 つの関数にする(`N` は関数の番号)。
//! 値スタックの入口は、スロットと文脈から引数と環境を読んでレジスタの入口を `musttail` で呼ぶ。
//! 規約は `keleut_interp::native` の冒頭にある。
//!
//! - 入口で、ネイティブのスタックの番地を文脈の下限と比べる(予備の検査)。
//!   次に、呼ぶ側が置いたフレームの位置と見出しへのポインタ、環境を文脈から読む。
//! - LIR のレジスタは関数の入口の `alloca` に置き(最適化で SSA の値になる)。
//!   前の 4 個の引数と環境はレジスタの入口の引数から、残りの引数はフレームのスロットから読む。
//! - 呼ばれる側のフレームは、値スタックの使っている最後の塊に収まれば、`krt_prep` を呼ばずにその場で置く。
//! - 末尾でない呼び出しの前には、再開点で生きているレジスタをスロットに書き、見出しの 0 番の語に再開点を書く。
//!   yield を受け取らない再開点(`doc/lir.md` の 17 節)では、生の区画のレジスタを書かない(回収は `any` の区画だけを読む)。
//!   呼ばれた側が番兵で戻ったら `krt_sentinel` を呼んで自分も番兵で戻る。
//! - 確保する命令の前には、安全点で生きている `any` の区画のレジスタをスロットに書き、
//!   実行時の関数に今のフレームの位置と安全点の番号を渡す(実行時の関数が `GcState::at_safepoint` を行う)。
//! - 箱(`box`)、`ctor_new`、`rec_new` の小さな箱は、実行時の関数を呼ばずにヒープから 1 区画を取る(確保の速い道。[`Gen::alloc_small`])。
//!   取れないときだけ、安全点のレジスタを書いて実行時の関数を呼ぶ。
//!   `--gc-stress` のときは速い道を作らない([`function`] の `fast_alloc`)。
//! - 末尾の呼び出しは、自分のフレームの位置に呼ばれた側のフレームを置き直してから `musttail` で呼ぶ。
//! - 相手を実行時に決める呼び出しは `krt_dispatch` に任せる。
//!   値の呼び出しは、相手がクロージャなら実行時の関数を通さずに呼ぶ。
//!
//! - エフェクトの命令は、被演算子をスロットに書いてから実行時の関数 `krt_fx` を呼ぶ(`keleut_interp::native` の冒頭)。
//!   呼び出しの一種は、`krt_fx` が置いたフレームの関数を入口の表から呼び、番兵を受け取る呼び出しなら番兵を結果にし、
//!   `perform` なら戻りを `krt_perform_back` に渡す。
//!   エフェクトのブロックの終わりは `krt_fx_term` を呼び、末尾で呼ぶ終わりは置いたフレームの関数を `musttail` で呼ぶ。
//! - 入口では、`alternating` の擬似乱数の状態が文脈にあれば、それを進めて段を選ぶ(インタプリタに移るなら `interp_stub` を `musttail` で呼ぶ)。
//!
//! 整数の `+ - *` は `nsw` を付けずに折り返し、`sdiv` と `srem` の前に除数 0 と `MIN / -1` の場合を分ける。
//! `Float64` から整数への変換は飽和する組み込み関数 `llvm.fptosi.sat` を使う(`doc/lir.md` の 6 節)。
//! `sin cos exp log` は rtcore の不透明な記号 `krt_sin` などを呼び、`sqrt` は `llvm.sqrt` にする。

use std::collections::HashMap;
use std::fmt::Write as _;

use keleut_interp::native::{dispatch, fx, off};
use keleut_lir::{
    CmpOp, Const, ConvOp, FloatOp, Func, FuncId, FxOp, FxTermOp, Inst, IntOp, IntTy, MathFn,
    Program, Reg, Rep, ShowOp, Slot, Term,
};
use keleut_rt::Word;
use keleut_rt::stack::HEADER_WORDS;
use keleut_rt::value::{Kind, header};

/// レジスタで渡す引数の数。
const NREG: usize = keleut_interp::native::REG_ARGS;

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
declare i64 @krt_fx(ptr, i64, ptr, i64, i64) nounwind
declare i64 @krt_fx_term(ptr, i64, ptr, i64) nounwind
declare i64 @krt_perform_back(ptr, i64, ptr, i64) nounwind
declare void @krt_bug(ptr, i64, i64) nounwind
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
declare void @krt_store_regs(ptr, i64, i64, i64, i64, i64) nounwind

define tailcc i64 @interp_stub_r(ptr %ctx, i64 %env, i64 %x0, i64 %x1, i64 %x2, i64 %x3) nounwind {{
entry:
  call void @krt_store_regs(ptr %ctx, i64 %env, i64 %x0, i64 %x1, i64 %x2, i64 %x3)
  %r = musttail call tailcc i64 @interp_stub(ptr %ctx)
  ret i64 %r
}}

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
    prog: &'a Program,
    /// レジスタの入口の表の番地。
    rentry: usize,
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
    /// エフェクトの命令の通し番号(`keleut_interp::bytecode::Code::fx_pc` の添字)。
    fx_idx: HashMap<(u32, u32), u32>,
    /// エフェクトのブロックの終わりの通し番号(`Code::fx_term_pc` の添字)。
    fxt_idx: HashMap<u32, u32>,
    /// 番兵を受け取るエフェクトの呼び出しの再開点(`cont_make` の失敗の後に戻り直す先)。
    catch_rps: Vec<u32>,
    /// 今のブロックの名前(phi の入り口に使う)。
    cur: String,
    /// 小さな箱の確保の速い道を作るか(`--gc-stress` では作らない)。
    fast_alloc: bool,
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

    /// 見出しを含めて `words` 語の箱を確保し、見出し `header` と、`fill` の(語の番号、値)を書いて、箱の語を返す。
    ///
    /// 速い道は `keleut_rt::heap::Heap::try_alloc` の最初の 2 つの道と同じ手順で、文脈の `heap` のヒープから 1 区画を取る。
    /// 実行の役割の印が立っているとき、前の回収の後に確保した量(`since_gc`)が回収の時機(`trigger`)に届いたとき、
    /// 空きリストにも切り出し中のページにも区画が無いときは、`slow` で実行時の関数を呼ぶ。
    /// 実行時の関数は印を下ろし、回収し、ページを足すので、速い道は印を読むだけで、回収の数えを区画の語の数だけ足す。
    /// 速い道は回収を起こさないので、安全点のレジスタを書くのは遅い道だけである。
    /// `slow` は安全点の番号を受け取り、実行時の関数を呼んでその結果を返す(0 の検査はここで行う)。
    fn alloc_small(
        &mut self,
        bi: usize,
        ii: usize,
        words: usize,
        header: u64,
        fill: &[(u64, String)],
        slow: impl FnOnce(&mut Self, u32) -> String,
    ) -> Result<String, String> {
        use keleut_rt::heap::fast;
        let class = if self.fast_alloc {
            fast::class_of(words)
        } else {
            None
        };
        let Some((c, cw)) = class else {
            let sp = self.safepoint(bi, ii)?;
            let r = slow(self, sp);
            self.check_zero(&r);
            return Ok(r);
        };
        let slow_l = self.label("aslow");
        let a1 = self.label("ahave");
        let a2 = self.label("aclass");
        let pop = self.label("apop");
        let bump = self.label("abump");
        let take = self.label("atake");
        let got = self.label("agot");
        let join = self.label("ajoin");
        // 実行の役割の印。立っていれば実行時の関数が下ろしてメモリの不足にする。
        let fa = self.t();
        self.line(format!(
            "{fa} = inttoptr i64 {} to ptr",
            keleut_rt::alloc::flag_addr(keleut_rt::alloc::Role::Exec)
        ));
        let fl = self.t();
        self.line(format!(
            "{fl} = load atomic i8, ptr {fa} monotonic, align 1"
        ));
        let c0 = self.t();
        self.line(format!("{c0} = icmp ne i8 {fl}, 0"));
        self.line(format!("br i1 {c0}, label %{slow_l}, label %{a1}"));
        self.start_block(&a1);
        let sgp = self.t();
        self.line(format!(
            "{sgp} = getelementptr i8, ptr %heapp, i64 {}",
            fast::SINCE_GC
        ));
        let sg = self.t();
        self.line(format!("{sg} = load i64, ptr {sgp}"));
        let tgp = self.t();
        self.line(format!(
            "{tgp} = getelementptr i8, ptr %heapp, i64 {}",
            fast::TRIGGER
        ));
        let tg = self.t();
        self.line(format!("{tg} = load i64, ptr {tgp}"));
        let c1 = self.t();
        self.line(format!("{c1} = icmp uge i64 {sg}, {tg}"));
        self.line(format!("br i1 {c1}, label %{slow_l}, label %{a2}"));
        self.start_block(&a2);
        let kp = fast::CLASSES_AT + c as u64 * fast::CLASS_BYTES;
        let field = |g: &mut Self, o: u64| {
            let q = g.t();
            g.line(format!(
                "{q} = getelementptr i8, ptr %heapp, i64 {}",
                kp + o
            ));
            q
        };
        let freep = field(self, fast::FREE);
        let curp = field(self, fast::CUR);
        let endp = field(self, fast::END);
        let fr = self.t();
        self.line(format!("{fr} = load ptr, ptr {freep}"));
        let c2 = self.t();
        self.line(format!("{c2} = icmp eq ptr {fr}, null"));
        self.line(format!("br i1 {c2}, label %{bump}, label %{pop}"));
        self.start_block(&pop);
        // 空きリストの区画の 1 番の語は次の空きの区画を指す。
        let np = self.t();
        self.line(format!("{np} = getelementptr i64, ptr {fr}, i64 1"));
        let nx = self.t();
        self.line(format!("{nx} = load ptr, ptr {np}"));
        self.line(format!("store ptr {nx}, ptr {freep}"));
        self.line(format!("br label %{got}"));
        self.start_block(&bump);
        let cur = self.t();
        self.line(format!("{cur} = load ptr, ptr {curp}"));
        let end = self.t();
        self.line(format!("{end} = load ptr, ptr {endp}"));
        let ci = self.t();
        self.line(format!("{ci} = ptrtoint ptr {cur} to i64"));
        let ei = self.t();
        self.line(format!("{ei} = ptrtoint ptr {end} to i64"));
        let d = self.t();
        self.line(format!("{d} = sub i64 {ei}, {ci}"));
        let c3 = self.t();
        self.line(format!("{c3} = icmp uge i64 {d}, {}", cw * 8));
        let c4 = self.t();
        self.line(format!("{c4} = icmp ne ptr {cur}, null"));
        let c5 = self.t();
        self.line(format!("{c5} = and i1 {c3}, {c4}"));
        self.line(format!("br i1 {c5}, label %{take}, label %{slow_l}"));
        self.start_block(&take);
        let nc = self.t();
        self.line(format!("{nc} = getelementptr i64, ptr {cur}, i64 {cw}"));
        self.line(format!("store ptr {nc}, ptr {curp}"));
        self.line(format!("br label %{got}"));
        self.start_block(&got);
        let p = self.t();
        self.line(format!(
            "{p} = phi ptr [ {fr}, %{pop} ], [ {cur}, %{take} ]"
        ));
        let ns = self.t();
        self.line(format!("{ns} = add i64 {sg}, {cw}"));
        self.line(format!("store i64 {ns}, ptr {sgp}"));
        // 取った区画には、次の回収の前に種類の入った見出しを書く(`keleut_rt::heap` の冒頭)。
        self.line(format!("store i64 {header}, ptr {p}"));
        for (i, v) in fill {
            let q = self.t();
            self.line(format!("{q} = getelementptr i64, ptr {p}, i64 {i}"));
            self.line(format!("store i64 {v}, ptr {q}"));
        }
        let w = self.t();
        self.line(format!("{w} = ptrtoint ptr {p} to i64"));
        self.line(format!("br label %{join}"));
        self.start_block(&slow_l);
        let sp = self.safepoint(bi, ii)?;
        let r = slow(self, sp);
        self.check_zero(&r);
        let slow_end = self.cur.clone();
        self.line(format!("br label %{join}"));
        self.start_block(&join);
        let v = self.t();
        self.line(format!(
            "{v} = phi i64 [ {w}, %{got} ], [ {r}, %{slow_end} ]"
        ));
        Ok(v)
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

    /// レジスタの入口の表の `fid` 番の番地。
    fn rentry_of(&mut self, fid: &str) -> String {
        let tab = self.t();
        self.line(format!("{tab} = inttoptr i64 {} to ptr", self.rentry));
        let p = self.t();
        self.line(format!("{p} = getelementptr ptr, ptr {tab}, i64 {fid}"));
        let e = self.t();
        self.line(format!("{e} = load atomic ptr, ptr {p} acquire, align 8"));
        e
    }

    /// レジスタで渡す引数の並び(`ptr %ctx` の後)。
    fn reg_args(&mut self, env: &str, args: &[Reg]) -> String {
        let mut v = format!("ptr %ctx, i64 {env}");
        for i in 0..NREG {
            let a = match args.get(i) {
                Some(&r) => self.get64(r),
                None => "0".to_string(),
            };
            let _ = write!(v, ", i64 {a}");
        }
        v
    }

    /// 置いたフレーム(見出しは `np`)のスロットに、`from` 番目からの引数を書く(前の引数はレジスタで渡す)。
    fn store_args_from(&mut self, np: &str, args: &[Reg], from: usize) {
        for (i, &r) in args.iter().enumerate().skip(from) {
            let v = self.get64(r);
            let p = self.t();
            self.line(format!(
                "{p} = getelementptr i64, ptr {np}, i64 {}",
                HEADER_WORDS + i
            ));
            self.line(format!("store i64 {v}, ptr {p}"));
        }
    }

    /// 関数 `f` のフレームの語の数と見出しの 2 番の語(`keleut_interp::native` の `frames` と同じ式)。
    fn frame_of(&self, f: FuncId) -> Option<(u64, u64)> {
        let fr = self.prog.funcs[f.index()].frame.as_ref()?;
        Some((
            (HEADER_WORDS as u32 + fr.any_slots + fr.raw_slots) as u64,
            u64::from(fr.any_slots) | (u64::from(fr.raw_slots) << 32),
        ))
    }

    /// 関数 `fid`(値)のフレームを置く。末尾なら自分の位置に、そうでなければ自分の上に。
    ///
    /// `frame` は呼ばれる側のフレームの語の数と見出しの 2 番の語で、翻訳のときに分かれば渡す。
    /// そのときは、置く場所が値スタックの使っている最後の塊(文脈の `win`)に収まれば、
    /// `krt_prep` を呼ばずにその場で見出しと文脈の欄を書く(速い道)。
    /// 自分のフレームも同じ塊にあるので、`krt_prep` の `ValueStack::place` と同じ位置に置くことになる。
    fn prep(&mut self, fid: &str, env: &str, tail: bool, frame: Option<(u64, u64)>) -> String {
        let (cfw, sizes) = match frame {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => {
                // 相手を実行時に知る呼び出しは、文脈の `frames` の列から読む。
                let i = self.t();
                self.line(format!("{i} = shl i64 {fid}, 1"));
                let p = self.t();
                self.line(format!("{p} = getelementptr i64, ptr %frtab, i64 {i}"));
                let a = self.t();
                self.line(format!("{a} = load i64, ptr {p}"));
                let q = self.t();
                self.line(format!("{q} = getelementptr i64, ptr {p}, i64 1"));
                let b = self.t();
                self.line(format!("{b} = load i64, ptr {q}"));
                (a, b)
            }
        };
        let at = if tail { "%fpos" } else { "%top" };
        let caller = if tail { "%w3" } else { "%fpos" };
        let lo = self.t();
        self.line(format!("{lo} = load i64, ptr %winp"));
        let hip = self.t();
        self.line(format!("{hip} = getelementptr i64, ptr %winp, i64 1"));
        let hi = self.t();
        self.line(format!("{hi} = load i64, ptr {hip}"));
        let c1 = self.t();
        self.line(format!("{c1} = icmp uge i64 %fpos, {lo}"));
        let end = self.t();
        self.line(format!("{end} = add i64 {at}, {cfw}"));
        let c2 = self.t();
        self.line(format!("{c2} = icmp ule i64 {end}, {hi}"));
        let c = self.t();
        self.line(format!("{c} = and i1 {c1}, {c2}"));
        let fast = self.label("pfast");
        let slow = self.label("pslow");
        let join = self.label("pjoin");
        self.line(format!("br i1 {c}, label %{fast}, label %{slow}"));
        self.start_block(&fast);
        let bp = self.t();
        self.line(format!("{bp} = getelementptr i64, ptr %winp, i64 2"));
        let base = self.t();
        self.line(format!("{base} = load ptr, ptr {bp}"));
        let o = self.t();
        self.line(format!("{o} = sub i64 {at}, {lo}"));
        let p = self.t();
        self.line(format!("{p} = getelementptr i64, ptr {base}, i64 {o}"));
        self.line(format!("store i64 {fid}, ptr {p}"));
        for (i, v) in [(1, "0".to_string()), (2, sizes), (3, caller.to_string())] {
            let q = self.t();
            self.line(format!("{q} = getelementptr i64, ptr {p}, i64 {i}"));
            self.line(format!("store i64 {v}, ptr {q}"));
        }
        for (o, ty, v) in [
            (off::CALLEE, "i64", fid.to_string()),
            (off::CUR_POS, "i64", at.to_string()),
            (off::CUR_FP, "ptr", p.clone()),
        ] {
            let q = self.t();
            self.line(format!("{q} = getelementptr i8, ptr %ctx, i64 {o}"));
            self.line(format!("store {ty} {v}, ptr {q}"));
        }
        let fast_end = self.cur.clone();
        self.line(format!("br label %{join}"));
        self.start_block(&slow);
        let sp = self.prep_slow(fid, env, tail);
        let slow_end = self.cur.clone();
        self.line(format!("br label %{join}"));
        self.start_block(&join);
        let np = self.t();
        self.line(format!(
            "{np} = phi ptr [ {p}, %{fast_end} ], [ {sp}, %{slow_end} ]"
        ));
        np
    }

    /// `krt_prep` でフレームを置く([`Gen::prep`] の遅い道)。
    fn prep_slow(&mut self, fid: &str, env: &str, tail: bool) -> String {
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
                let frame = self.frame_of(f);
                let np = self.prep(&fid, "0", tail, frame);
                self.store_args_from(&np, args, NREG);
                let e = self.rentry_of(&fid);
                let ra = self.reg_args("0", args);
                self.finish_call_r(&e, &ra, dst, yields, &done);
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
                    let np = self.prep(&fid, &fv, tail, None);
                    self.store_args_from(&np, args, NREG);
                    let e = self.rentry_of(&fid);
                    let ra = self.reg_args(&fv, args);
                    self.finish_call_r(&e, &ra, dst, yields, &done);
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

    /// レジスタの入口 `e` を引数の並び `ra` で呼ぶ。ほかは [`Gen::finish_call`] と同じ。
    fn finish_call_r(&mut self, e: &str, ra: &str, dst: Option<Reg>, yields: bool, done: &str) {
        match dst {
            Some(d) => {
                let r = self.t();
                self.line(format!("{r} = call tailcc i64 {e}({ra})"));
                self.after_call(&r, yields);
                self.set(d, &r);
                self.line(format!("br label %{done}"));
            }
            None => {
                let r = self.t();
                self.line(format!("{r} = musttail call tailcc i64 {e}({ra})"));
                self.line(format!("ret i64 {r}"));
            }
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
                        let (kind, bits) = if self.rep(*src) == Rep::I64 {
                            (0, v.clone())
                        } else {
                            let b = self.t();
                            self.line(format!("{b} = bitcast double {v} to i64"));
                            (1, b)
                        };
                        let k = if kind == 0 { Kind::I64 } else { Kind::F64 };
                        let fill = [(1, bits.clone())];
                        self.alloc_small(bi, ii, 2, header(k, 0), &fill, |g, sp| {
                            let t = g.t();
                            g.line(format!(
                                "{t} = call i64 @krt_box(ptr %ctx, i64 %fpos, i64 {sp}, i64 {kind}, i64 {bits})"
                            ));
                            t
                        })?
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
                let n = fields.len();
                let slow = |g: &mut Self, sp: u32| {
                    g.put_args(0, fields);
                    let r = g.t();
                    g.line(format!(
                        "{r} = call i64 @krt_ctor_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {ctor}, i64 {n})",
                        ty.0
                    ));
                    r
                };
                let r = if n == 0 {
                    // フィールドの無いコンストラクタは即値で、確保しない(`Rt::ctor_new`)。
                    let sp = self.safepoint(bi, ii)?;
                    let r = slow(self, sp);
                    self.check_zero(&r);
                    r
                } else {
                    let mut fill =
                        vec![(1, ((u64::from(ty.0) << 32) | u64::from(*ctor)).to_string())];
                    for (i, &f) in fields.iter().enumerate() {
                        let v = self.get64(f);
                        fill.push((2 + i as u64, v));
                    }
                    self.alloc_small(bi, ii, 2 + n, header(Kind::Data, n as u32), &fill, slow)?
                };
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
                let n = fields.len();
                let slow = |g: &mut Self, sp: u32| {
                    g.put_args(0, fields);
                    let r = g.t();
                    g.line(format!(
                        "{r} = call i64 @krt_rec_new(ptr %ctx, i64 %fpos, i64 {sp}, i64 {}, i64 {n})",
                        shape.0
                    ));
                    r
                };
                let r = if n == 0 {
                    // フィールドの無いレコードは Unit で、確保しない(`Rt::rec_new`)。
                    let sp = self.safepoint(bi, ii)?;
                    let r = slow(self, sp);
                    self.check_zero(&r);
                    r
                } else {
                    let mut fill = vec![(1, shape.0.to_string())];
                    for (i, &f) in fields.iter().enumerate() {
                        let v = self.get64(f);
                        fill.push((2 + i as u64, v));
                    }
                    self.alloc_small(bi, ii, 2 + n, header(Kind::Record, n as u32), &fill, slow)?
                };
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
            Inst::Fx { op, dst, args } => self.fx_inst(bi, ii, op, *dst, args)?,
        }
        Ok(())
    }

    /// スロットの語をレジスタ `r` に読む。
    fn load_slot(&mut self, r: Reg) {
        let p = self.frame_word(HEADER_WORDS as u64 + u64::from(self.slot[r.index()]));
        let v = self.t();
        self.line(format!("{v} = load i64, ptr {p}"));
        self.set64(r, &v);
    }

    /// エフェクトの命令(`krt_fx`)。
    fn fx_inst(
        &mut self,
        bi: usize,
        ii: usize,
        op: &FxOp,
        dst: Option<Reg>,
        args: &[Reg],
    ) -> Result<(), String> {
        let idx = self.fx_idx[&(bi as u32, ii as u32)];
        if !op.is_call() {
            for &a in args {
                self.spill(a);
            }
            let sp = if op.allocates() {
                i64::from(self.safepoint(bi, ii)?)
            } else {
                -1
            };
            let c = self.t();
            self.line(format!(
                "{c} = call i64 @krt_fx(ptr %ctx, i64 %fpos, ptr %fp, i64 {idx}, i64 {sp})"
            ));
            let next = self.label("fxnext");
            if matches!(op, FxOp::ContMake) {
                let redo = self.label("fxredo");
                self.line(format!(
                    "switch i64 {c}, label %ret2 [ i64 {}, label %{next} i64 {}, label %{redo} ]",
                    fx::NEXT,
                    fx::REDELIVER
                ));
                // 継続を作れず、区間の最上段から巻き戻した。見出しの再開点(本体の呼び出し)の結果として受け取り直す。
                self.start_block(&redo);
                let v = self.ctx_load(off::RESULT, "i64");
                self.line(format!("store i64 {v}, ptr %catchr"));
                let h = self.t();
                self.line(format!("{h} = load i64, ptr %fp"));
                let rp = self.t();
                self.line(format!("{rp} = lshr i64 {h}, 32"));
                let bad = self.label("fxbad");
                let cases: String = self
                    .catch_rps
                    .iter()
                    .map(|r| format!(" i64 {r}, label %cin{r}"))
                    .collect();
                self.line(format!("switch i64 {rp}, label %{bad} [{cases} ]"));
                self.start_block(&bad);
                self.line(format!(
                    "call void @krt_bug(ptr %ctx, i64 {}, i64 1)",
                    self.fid.0
                ));
                self.line("ret i64 2");
            } else {
                let z = self.t();
                self.line(format!("{z} = icmp eq i64 {c}, {}", fx::NEXT));
                self.line(format!("br i1 {z}, label %{next}, label %ret2"));
            }
            self.start_block(&next);
            if let Some(d) = dst {
                self.load_slot(d);
            }
            return Ok(());
        }
        let d = dst.ok_or("書き先の無いエフェクトの呼び出し")?;
        let (rp, live, y) = self.resume(bi, ii)?;
        self.spill_set(&live, y);
        for &a in args {
            self.spill(a);
        }
        self.set_row(rp);
        let c = self.t();
        self.line(format!(
            "{c} = call i64 @krt_fx(ptr %ctx, i64 %fpos, ptr %fp, i64 {idx}, i64 -1)"
        ));
        let lcall = self.label("fxcall");
        if matches!(op, FxOp::Perform { .. }) {
            self.line(format!(
                "switch i64 {c}, label %ret2 [ i64 {}, label %{lcall} ]",
                fx::CALL
            ));
            self.start_block(&lcall);
            let cid = self.ctx_load(off::CALLEE, "i64");
            let e = self.entry_of(&cid);
            let r = self.t();
            self.line(format!("{r} = call tailcc i64 {e}(ptr %ctx)"));
            let k = self.t();
            self.line(format!(
                "{k} = call i64 @krt_perform_back(ptr %ctx, i64 %fpos, ptr %fp, i64 {r})"
            ));
            let ok = self.t();
            self.line(format!("{ok} = icmp ne i64 {k}, 0"));
            let go = self.label("pok");
            self.line(format!("br i1 {ok}, label %{go}, label %ret2"));
            self.start_block(&go);
            self.set64(d, &r);
            return Ok(());
        }
        // 番兵を受け取る呼び出し。
        let lval = self.label("fxval");
        let lnext = self.label("fxnext");
        self.line(format!(
            "switch i64 {c}, label %ret2 [ i64 {}, label %{lnext} i64 {}, label %{lcall} i64 {}, label %{lval} ]",
            fx::NEXT,
            fx::CALL,
            fx::VALUE
        ));
        // 呼ばずに番兵を結果に書いた(`call_clause` が深さの限度を超えた)。深さは変えない。
        self.start_block(&lnext);
        let p = self.frame_word(HEADER_WORDS as u64 + u64::from(self.slot[d.index()]));
        let v = self.t();
        self.line(format!("{v} = load i64, ptr {p}"));
        self.line(format!("store i64 {v}, ptr %catchr"));
        self.line(format!("br label %cj{rp}"));
        self.start_block(&lcall);
        let cid = self.ctx_load(off::CALLEE, "i64");
        let e = self.entry_of(&cid);
        let r = self.t();
        self.line(format!("{r} = call tailcc i64 {e}(ptr %ctx)"));
        self.line(format!("store i64 {r}, ptr %catchr"));
        self.line(format!("br label %cin{rp}"));
        self.start_block(&lval);
        let v = self.ctx_load(off::RESULT, "i64");
        self.line(format!("store i64 {v}, ptr %catchr"));
        self.line(format!("br label %cin{rp}"));
        // 戻った。番兵なら深さをこのフレームの深さに書き戻す。
        self.start_block(&format!("cin{rp}"));
        let v = self.t();
        self.line(format!("{v} = load i64, ptr %catchr"));
        let s = self.t();
        self.line(format!("{s} = icmp eq i64 {v}, 2"));
        self.line(format!("br i1 {s}, label %cs{rp}, label %cj{rp}"));
        self.start_block(&format!("cs{rp}"));
        let fd = self.frame_word(1);
        let dv = self.t();
        self.line(format!("{dv} = load i64, ptr {fd}"));
        self.line(format!("store i64 {dv}, ptr %dp"));
        self.line(format!("br label %cj{rp}"));
        self.start_block(&format!("cj{rp}"));
        let v = self.t();
        self.line(format!("{v} = load i64, ptr %catchr"));
        self.set64(d, &v);
        Ok(())
    }

    /// エフェクトのブロックの終わり(`krt_fx_term`)。
    fn fx_term(&mut self, bi: usize, op: &FxTermOp, args: &[Reg]) {
        if matches!(op, FxTermOp::Propagate) {
            self.line("ret i64 2");
            return;
        }
        let idx = self.fxt_idx[&(bi as u32)];
        for &a in args {
            self.spill(a);
        }
        let c = self.t();
        self.line(format!(
            "{c} = call i64 @krt_fx_term(ptr %ctx, i64 %fpos, ptr %fp, i64 {idx})"
        ));
        if matches!(
            op,
            FxTermOp::TailCallNever | FxTermOp::TailCallClause { .. }
        ) {
            let z = self.t();
            self.line(format!("{z} = icmp eq i64 {c}, {}", fx::CALL));
            let tc = self.label("fxtail");
            self.line(format!("br i1 {z}, label %{tc}, label %ret2"));
            self.start_block(&tc);
            let cid = self.ctx_load(off::CALLEE, "i64");
            let e = self.entry_of(&cid);
            let r = self.t();
            self.line(format!("{r} = musttail call tailcc i64 {e}(ptr %ctx)"));
            self.line(format!("ret i64 {r}"));
        } else {
            self.line("ret i64 2");
        }
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
            Term::Fx { op, args } => self.fx_term(bi, op, args),
        }
        Ok(())
    }
}

/// 関数 `fid` の LLVM IR の関数の定義を作る。`entry` と `rentry` は入口の表とレジスタの入口の表の番地。
///
/// `fast_alloc` が偽なら、小さな箱の確保の速い道を作らず、どの確保も実行時の関数を呼ぶ(`--gc-stress`)。
/// 関数は `keleut_interp::native::jit_eligible` を満たし、パス 12 を済ませていなければならない。
pub fn function(
    prog: &Program,
    fid: FuncId,
    entry: usize,
    rentry: usize,
    fast_alloc: bool,
) -> Result<String, String> {
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
    let mut fx_idx = HashMap::new();
    let mut fxt_idx = HashMap::new();
    let mut catch_rps = Vec::new();
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, inst) in b.insts.iter().enumerate() {
            if let Inst::Fx { op, .. } = inst {
                fx_idx.insert((bi as u32, ii as u32), fx_idx.len() as u32);
                if op.catches() {
                    let rp = rp_of
                        .get(&(bi as u32, ii as u32))
                        .ok_or("エフェクトの呼び出しに再開点がありません")?;
                    catch_rps.push(*rp);
                }
            }
        }
        if let Term::Fx { .. } = b.term {
            fxt_idx.insert(bi as u32, fxt_idx.len() as u32);
        }
    }
    let fw = (HEADER_WORDS as u32 + any + fr.raw_slots) as u64;
    let mut g = Gen {
        prog,
        rentry,
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
        fx_idx,
        fxt_idx,
        catch_rps,
        cur: String::new(),
        fast_alloc,
    };
    let _ = writeln!(
        g.out,
        "; {} ({})\ndefine tailcc i64 @{}.r(ptr %ctx, i64 %renv, i64 %ra0, i64 %ra1, i64 %ra2, i64 %ra3) nounwind {{\nentry:",
        f.sym,
        f.kind.name(),
        func_name(fid)
    );
    for (i, &rep) in f.regs.iter().enumerate() {
        g.line(format!("%r{i} = alloca {}", ty(rep)));
    }
    g.line("%fdepth = alloca i64");
    g.line("%catchr = alloca i64");
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
    g.line("br i1 %sov, label %lowstack, label %altchk");
    // alternating の擬似乱数(xorshift64)を進め、32 番のビットが立てばインタプリタに移る。
    g.start_block("altchk");
    g.line(format!(
        "%altp = getelementptr i8, ptr %ctx, i64 {}",
        off::ALT
    ));
    g.line("%alt = load i64, ptr %altp");
    g.line("%alton = icmp ne i64 %alt, 0");
    g.line("br i1 %alton, label %altpick, label %go");
    g.start_block("altpick");
    g.line("%x1s = shl i64 %alt, 13");
    g.line("%x1 = xor i64 %alt, %x1s");
    g.line("%x2s = lshr i64 %x1, 7");
    g.line("%x2 = xor i64 %x1, %x2s");
    g.line("%x3s = shl i64 %x2, 17");
    g.line("%x3 = xor i64 %x2, %x3s");
    g.line("store i64 %x3, ptr %altp");
    g.line("%xb = and i64 %x3, 4294967296");
    g.line("%toint = icmp ne i64 %xb, 0");
    g.line("br i1 %toint, label %altstack, label %go");
    for (lbl, why) in [("lowstack", 1), ("altstack", 2)] {
        g.start_block(lbl);
        g.line(format!(
            "%sfpp{why} = getelementptr i8, ptr %ctx, i64 {}",
            off::CUR_FP
        ));
        g.line(format!("%sfp{why} = load ptr, ptr %sfpp{why}"));
        for i in 0..f.params.len().min(NREG) {
            g.line(format!(
                "%sap{why}.{i} = getelementptr i64, ptr %sfp{why}, i64 {}",
                HEADER_WORDS + i
            ));
            g.line(format!("store i64 %ra{i}, ptr %sap{why}.{i}"));
        }
        g.line(format!(
            "%senvp{why} = getelementptr i8, ptr %ctx, i64 {}",
            off::ENV
        ));
        g.line(format!("store i64 %renv, ptr %senvp{why}"));
        g.line(format!(
            "%forcep{why} = getelementptr i8, ptr %ctx, i64 {}",
            off::FORCE_INTERP
        ));
        g.line(format!("store i64 {why}, ptr %forcep{why}"));
        g.line(format!(
            "%stubp{why} = getelementptr i8, ptr %ctx, i64 {}",
            off::STUB
        ));
        g.line(format!("%stub{why} = load ptr, ptr %stubp{why}"));
        g.line(format!(
            "%rstub{why} = musttail call tailcc i64 %stub{why}(ptr %ctx)"
        ));
        g.line(format!("ret i64 %rstub{why}"));
    }
    g.start_block("go");
    for (name, o, t) in [
        ("fp", off::CUR_FP, "ptr"),
        ("fpos", off::CUR_POS, "i64"),
        ("dp", off::DEPTH, "ptr"),
        ("lp", off::LIMIT, "ptr"),
        ("argp", off::ARGS, "ptr"),
        ("glob", off::GLOBALS, "ptr"),
        ("ginit", off::GLOBAL_INIT, "ptr"),
        ("strs", off::STRINGS, "ptr"),
        ("winp", off::WIN, "ptr"),
        ("frtab", off::FRAMES, "ptr"),
        ("heapp", off::HEAP, "ptr"),
    ] {
        g.line(format!("%{name}.p = getelementptr i8, ptr %ctx, i64 {o}"));
        g.line(format!("%{name} = load {t}, ptr %{name}.p"));
    }
    g.line("%env = add i64 %renv, 0");
    g.line("%w3p = getelementptr i64, ptr %fp, i64 3");
    g.line("%w3 = load i64, ptr %w3p");
    g.line(format!("%top = add i64 %fpos, {}", g.fw));
    let handler = matches!(f.kind, keleut_lir::FuncKind::Handler);
    for (i, &p) in f.params.iter().enumerate() {
        if i < NREG {
            if handler {
                // `$rt_handle` のフレームは、どの点でも 0 番のスロットを根にする(`doc/lir.md` の 1.8 節)。
                let sp = g.frame_word((HEADER_WORDS + i) as u64);
                g.line(format!("store i64 %ra{i}, ptr {sp}"));
            }
            g.set64(p, &format!("%ra{i}"));
        } else {
            let sp = g.frame_word((HEADER_WORDS + i) as u64);
            let v = g.t();
            g.line(format!("{v} = load i64, ptr {sp}"));
            g.set64(p, &v);
        }
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
    // 値スタックで引数を受け取る入口(入口の表の番地)。スロットから読んでレジスタの入口へ移る。
    let name = func_name(fid);
    let _ = writeln!(
        g.out,
        "define tailcc i64 @{name}(ptr %ctx) nounwind {{\nentry:"
    );
    let _ = writeln!(
        g.out,
        "  %envp = getelementptr i8, ptr %ctx, i64 {}",
        off::ENV
    );
    let _ = writeln!(g.out, "  %env = load i64, ptr %envp");
    let _ = writeln!(
        g.out,
        "  %fpp = getelementptr i8, ptr %ctx, i64 {}",
        off::CUR_FP
    );
    let _ = writeln!(g.out, "  %fp = load ptr, ptr %fpp");
    let mut ra = "ptr %ctx, i64 %env".to_string();
    for i in 0..NREG {
        if i < f.params.len() {
            let _ = writeln!(
                g.out,
                "  %ap{i} = getelementptr i64, ptr %fp, i64 {}",
                HEADER_WORDS + i
            );
            let _ = writeln!(g.out, "  %a{i} = load i64, ptr %ap{i}");
            let _ = write!(ra, ", i64 %a{i}");
        } else {
            ra.push_str(", i64 0");
        }
    }
    let _ = writeln!(
        g.out,
        "  %r = musttail call tailcc i64 @{name}.r({ra})\n  ret i64 %r\n}}"
    );
    Ok(g.out)
}

/// 関数 `fid` の 1 つのモジュールの文面(宣言を含む)。`fast_alloc` は [`function`] と同じ。
pub fn module(
    prog: &Program,
    fid: FuncId,
    entry: usize,
    rentry: usize,
    fast_alloc: bool,
    triple: &str,
    layout: &str,
) -> Result<String, String> {
    let body = function(prog, fid, entry, rentry, fast_alloc)?;
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
