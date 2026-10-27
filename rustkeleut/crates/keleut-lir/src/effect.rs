//! エフェクトの下ろし(パス 8。計画 261024-1 の 6.6 節、`doc/lir.md` の 16 節)。
//!
//! `handle`、利用者の操作の `perform`、`resume` と、操作節の関数の入口をここで下ろす。
//! 規約の要は次のとおりである。
//!
//! - ハンドラのフレームは、すべての `handle` が共有する LIR の関数 `$rt_handle(hr, k, v)` である
//!   ([`rt_handle_body`])。
//!   `handle` は、ハンドラ記録 `hr` を作って `$rt_handle(hr, unit, unit)` を呼ぶ(末尾の `handle` なら末尾で呼ぶ)。
//!   継続 `k` の再開(計画 6.6 節の `rt_resume`)は `$rt_handle(k.hr, k, v)` を呼ぶことで、
//!   新しいハンドラのフレームの上に `k` のフレームを写し戻して続ける。
//!   本体の値、自分宛ての破棄、自分宛ての yield、ほかの宛ての巻き戻し(防壁の中の `cancel` 節)の扱いは、
//!   どちらの入り方でも同じ LIR の文面である。
//! - 操作節の関数は、KIR の引数の後ろに隠れた引数 `x` と `sel` を持つ。
//!   `sel` が 0 以上なら選択の段で、`x` はハンドラ記録、`sel` はこの節の候補の番号である。
//!   パターンとガードが一致しなければ、次の候補を末尾で呼ぶ(`tail_call_clause`)。
//!   T の節は一致したらそのまま本体を走らせ、N と G の節は `clause_matched` で戻る。
//!   `sel` が負なら本体の段で、`x` は G の節では継続、N の節では Unit である。
//!   パターンをもう一度照合し(照合は副作用を持たない)、ガードを飛ばして本体を走らせる。
//! - ハンドラ式の本体の関数は、隠れた引数として、囲む操作節の継続(無ければ Unit)を受け取る。
//!   本体の中の `resume` はこの継続を再開する(IRSpec 9.2 節)。

use kir::{ClauseClass, FnKind};

use crate::builder::Builder;
use crate::ir::*;
use crate::lower::{Low, V};
use crate::pattern::Target;

/// 操作節の関数ごとの(操作, 分類)。操作節でない関数は `None`。
///
/// 単位を足したプログラムでは、`out` に前の関数の分類を持ち、関数 `f0` から後ろの本体だけを調べる
/// (足した操作節の関数を作る `handle` は、足した関数の本体にある)。
pub(crate) fn clause_classes(
    k: &kir::Program,
    out: &mut Vec<Option<(u32, ClauseClass)>>,
    f0: usize,
) {
    out.resize(k.functions.len(), None);
    for f in &k.functions[f0..] {
        crate::initck::visit(k, f, |e| {
            if let kir::Exp::Handle(h) = e {
                for o in &h.ops {
                    if let Some(slot) = out.get_mut(o.part.func.index()) {
                        *slot = Some((o.op, o.class));
                    }
                }
            }
        });
    }
}

fn fx(b: &mut Builder, op: FxOp, args: Vec<Reg>) -> Option<Reg> {
    let dst = op.sig().1.map(|rep| b.reg(rep));
    b.emit(Inst::Fx { op, dst, args });
    dst
}

fn fx_val(b: &mut Builder, op: FxOp, args: Vec<Reg>) -> Reg {
    fx(b, op, args).expect("結果を持つエフェクトの命令")
}

/// 利用者の操作の `perform`(引数は評価して一様な語にしたもの)。
pub(crate) fn perform(lw: &mut Low, effect: u32, op: u32, args: Vec<Reg>) -> V {
    let hr = fx_val(&mut lw.b, FxOp::HandlerFind { effect, op }, Vec::new());
    let saved = fx_val(&mut lw.b, FxOp::HandlersGet, Vec::new());
    let mut a = vec![hr, saved];
    a.extend(args);
    V::any(fx_val(&mut lw.b, FxOp::Perform { effect, op }, a))
}

/// ハンドラ式。`tail` なら `$rt_handle` を末尾で呼んでブロックを終え、`None` を返す。
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle(
    lw: &mut Low,
    effect: u32,
    body: Reg,
    res: Reg,
    ops: Vec<(u32, ClauseClass)>,
    clauses: Vec<Reg>,
    ret: Option<Reg>,
    cancel: Option<Reg>,
    tail: bool,
) -> Option<V> {
    let Some(f) = lw.p.rt_handle else {
        return lw.bug("$rt_handle が無い");
    };
    let mut args = vec![body, res];
    args.extend(clauses);
    args.extend(ret);
    args.extend(cancel);
    let hr = fx_val(
        &mut lw.b,
        FxOp::HandlerNew {
            effect,
            ops,
            has_ret: ret.is_some(),
            has_cancel: cancel.is_some(),
        },
        args,
    );
    let unit = lw.b.konst(Const::Unit);
    let args = vec![hr, unit, unit];
    if tail {
        lw.b.terminate(Term::TailCallDirect {
            func: f,
            args,
            yields: true,
        });
        return None;
    }
    // ハンドラのフレームはハンドラ式の位置の深さ `h` で走るので、深さを増やさずに呼ぶ。
    let dst = lw.b.reg(Rep::Any);
    lw.b.emit(Inst::CallDirect {
        dst,
        func: f,
        args,
        yields: true,
    });
    Some(V::any(dst))
}

/// 末尾でない `resume(v)`。`k` は再開する継続、`v` は評価した値。
pub(crate) fn resume(lw: &mut Low, k: Reg, v: Reg) -> Option<V> {
    let Some(f) = lw.p.rt_handle else {
        return lw.bug("$rt_handle が無い");
    };
    fx(&mut lw.b, FxOp::ContCheck { nontail: true }, vec![k]);
    let hr = fx_val(&mut lw.b, FxOp::ContHr, vec![k]);
    // 再開した計算のハンドラのフレームは `resume` の位置の深さ `c` で走る(IRSpec 11.4 節)。
    let dst = lw.b.reg(Rep::Any);
    lw.b.emit(Inst::CallDirect {
        dst,
        func: f,
        args: vec![hr, k, v],
        yields: true,
    });
    Some(V::any(dst))
}

/// 関数の入口の後に、種類に応じた隠れた引数を足す。操作節の関数なら本体まで下ろして `true` を返す。
pub(crate) fn lower_entry(lw: &mut Low, f: &kir::Function, fid: usize) -> bool {
    match f.kind {
        FnKind::Handlebody => {
            let res = lw.b.param();
            lw.resumption = Some(res);
            false
        }
        FnKind::Opclause => {
            opclause(lw, f, fid);
            true
        }
        _ => false,
    }
}

/// 操作節の関数(入口の後)。
fn opclause(lw: &mut Low, f: &kir::Function, fid: usize) {
    let Some((op, class)) = lw.p.clause_class.get(fid).copied().flatten() else {
        lw.bug(format!("どの handle にも属さない操作節の関数 {}", f.sym));
        return;
    };
    let user: Vec<Reg> = lw.b.params().to_vec();
    let x = lw.b.param();
    let sel = lw.b.param();
    lw.clause = Some(class);
    if class == ClauseClass::G {
        lw.resumption = Some(x);
    }
    let miss = lw.b.block();
    for (j, &pat) in f.pats.iter().enumerate() {
        if lw.b.current().is_none() {
            break;
        }
        if let Some(reg) = lw.slots[j] {
            let v = V {
                reg,
                rep: lw.slot_rep[j],
            };
            lw.pattern(pat, v, miss, &Target::Slots);
        }
    }
    if lw.b.current().is_some() {
        if class != ClauseClass::T {
            // 本体の段(`sel` が負)ならガードを飛ばす。
            let s = lw.b.reg(Rep::I32);
            lw.b.emit(Inst::Unbox { dst: s, src: sel });
            let zero = lw.b.konst(Const::I32(0));
            let isbody = lw.b.reg(Rep::Bool);
            lw.b.emit(Inst::IntCmp {
                ty: IntTy::I32,
                op: CmpOp::Lt,
                dst: isbody,
                a: s,
                b: zero,
            });
            let body = lw.b.block();
            let select = lw.b.block();
            lw.b.terminate(Term::Branch {
                cond: isbody,
                then: body,
                else_: select,
            });
            lw.b.switch_to(select);
            guard(lw, f, miss);
            if lw.b.current().is_some() {
                lw.b.terminate(Term::Fx {
                    op: FxTermOp::ClauseMatched,
                    args: vec![sel],
                });
            }
            lw.b.switch_to(body);
        } else {
            guard(lw, f, miss);
        }
    }
    if lw.b.current().is_some() {
        lw.exp(f.body, true);
        if lw.b.current().is_some() {
            lw.bug(format!("関数 {} の本体がブロックを終えなかった", f.sym));
        }
    }
    lw.b.switch_to(miss);
    let mut args = vec![x, sel];
    args.extend(user);
    lw.b.terminate(Term::Fx {
        op: FxTermOp::TailCallClause { op },
        args,
    });
}

/// 操作節のガード。偽なら `miss` へ移る。
fn guard(lw: &mut Low, f: &kir::Function, miss: BlockId) {
    if let Some(g) = f.guard
        && let Some(gv) = lw.exp(g, false)
    {
        let cond = lw.coerce(gv, Rep::Bool);
        let body = lw.b.block();
        lw.b.terminate(Term::Branch {
            cond,
            then: body,
            else_: miss,
        });
        lw.b.switch_to(body);
    }
}

/// `c == n` で `yes` へ、そうでなければ新しいブロックへ移り、そこへ切り替える。
fn branch_eq(b: &mut Builder, c: Reg, n: i32, yes: BlockId) {
    let k = b.konst(Const::I32(n));
    let t = b.reg(Rep::Bool);
    b.emit(Inst::IntCmp {
        ty: IntTy::I32,
        op: CmpOp::Eq,
        dst: t,
        a: c,
        b: k,
    });
    let no = b.block();
    b.terminate(Term::Branch {
        cond: t,
        then: yes,
        else_: no,
    });
    b.switch_to(no);
}

/// `pending_class` の値(`doc/lir.md` の 16 節)。
pub mod class {
    /// 自分宛ての `DISCARD(Value v)`。
    pub const OWN_VALUE: i32 = 1;
    /// 自分宛ての `DISCARD(NeverBody)`。
    pub const OWN_NEVER: i32 = 2;
    /// 自分宛ての `YIELD`。
    pub const OWN_YIELD: i32 = 3;
    /// ほかの宛ての `YIELD`。
    pub const OTHER_YIELD: i32 = 4;
    /// ほかの宛ての `DISCARD` と `ERROR`。
    pub const OTHER_UNWIND: i32 = 5;
    /// G の節の末尾の `resume` の印。
    pub const TAIL_RESUME: i32 = 6;
}

/// ハンドラのフレームの関数 `$rt_handle(hr, k, v)` の本体(`doc/lir.md` の 16.3 節)。`me` はこの関数の番号。
pub(crate) fn rt_handle_body(me: FuncId) -> Builder {
    use class::*;
    let mut b = Builder::new();
    crate::depth::enter(&mut b);
    let hr = b.param();
    let k = b.param();
    let v = b.param();
    let isk = fx_val(&mut b, FxOp::IsCont, vec![k]);
    let fresh = b.block();
    let res = b.block();
    let post = b.block();
    let prop = b.block();
    let r = b.reg(Rep::Any);
    b.terminate(Term::Branch {
        cond: isk,
        then: res,
        else_: fresh,
    });

    // 計画 6.6 節の rt_handle の手順 1(doc/lir.md 16.3 節の手順 1)。本体の深さ h+1 は、連鎖に入れる前に限度と比べる。
    b.switch_to(fresh);
    b.emit(Inst::DepthCheck);
    fx(&mut b, FxOp::HandlerInstall, vec![hr]);
    b.emit(Inst::DepthPush);
    let r1 = fx_val(&mut b, FxOp::CallBody, vec![hr]);
    b.emit(Inst::DepthRestore);
    b.emit(Inst::Move { dst: r, src: r1 });
    b.terminate(Term::Jump(post));

    // rt_resume(16.3 節の手順 2)。再開済みなら実行時エラー(7)。継続のフレームを写し戻して続ける。
    b.switch_to(res);
    fx(&mut b, FxOp::ContCheck { nontail: false }, vec![k]);
    let r2 = fx_val(&mut b, FxOp::ContResume, vec![k, v]);
    b.emit(Inst::DepthRestore);
    b.emit(Inst::Move { dst: r, src: r2 });
    b.terminate(Term::Jump(post));

    // 計画 6.6 節の手順 2〜7(doc/lir.md 16.3 節の手順 3〜4)。
    b.switch_to(post);
    let ip = fx_val(&mut b, FxOp::IsPending, vec![r]);
    let pend = b.block();
    let normal = b.block();
    b.terminate(Term::Branch {
        cond: ip,
        then: pend,
        else_: normal,
    });

    // 計画 6.6 節の手順 2(16.3 節の手順 3): 値 x で戻ったら、連鎖から外して return 節を末尾で呼ぶ。
    b.switch_to(normal);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let hasret = fx_val(&mut b, FxOp::HandlerHas(HandlerPart::Ret), vec![hr]);
    let retb = b.block();
    let retv = b.block();
    b.terminate(Term::Branch {
        cond: hasret,
        then: retb,
        else_: retv,
    });
    b.switch_to(retb);
    let rc = fx_val(&mut b, FxOp::HandlerGet(HandlerPart::Ret), vec![hr]);
    b.terminate(Term::TailCallValue {
        callee: rc,
        args: vec![r],
        yields: true,
    });
    b.switch_to(retv);
    b.terminate(Term::Return(r));

    b.switch_to(pend);
    let cls = fx_val(&mut b, FxOp::PendingClass, vec![hr]);
    let own_val = b.block();
    let own_never = b.block();
    let own_yield = b.block();
    let other = b.block();
    branch_eq(&mut b, cls, OWN_VALUE, own_val);
    branch_eq(&mut b, cls, OWN_NEVER, own_never);
    branch_eq(&mut b, cls, OWN_YIELD, own_yield);
    branch_eq(&mut b, cls, OTHER_YIELD, prop);
    b.terminate(Term::Jump(other));

    // 計画 6.6 節の手順 3(16.3 節の手順 4 の 1): 自分宛ての DISCARD(Value v)。return も cancel も走らせない。
    b.switch_to(own_val);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    let pv = fx_val(&mut b, FxOp::PendingValue, vec![p]);
    b.terminate(Term::Return(pv));

    // 計画 6.6 節の手順 4(16.3 節の手順 4 の 2): 自分宛ての DISCARD(NeverBody)。N の節の本体を末尾で呼ぶ。
    b.switch_to(own_never);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    b.terminate(Term::Fx {
        op: FxTermOp::TailCallNever,
        args: vec![p],
    });

    // 計画 6.6 節の手順 5(16.3 節の手順 4 の 3): 自分宛ての YIELD。連鎖から外し、継続を作って G の節を末尾でなく呼ぶ。
    // 先に外すので、継続を作れずにメモリの不足になっても、連鎖に死んだハンドラが残らない。
    b.switch_to(own_yield);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let kk = fx_val(&mut b, FxOp::ContMake, vec![hr]);
    let g = fx_val(&mut b, FxOp::CallClause, vec![kk]);
    b.emit(Inst::DepthRestore);
    let gp = fx_val(&mut b, FxOp::IsPending, vec![g]);
    let gpend = b.block();
    let gval = b.block();
    b.terminate(Term::Branch {
        cond: gp,
        then: gpend,
        else_: gval,
    });
    // 節が値 g で戻った。継続を使っていれば g がこのフレームの値である。
    // 使っていなければ DISCARD(hr, Value g) を注入して継続を写し戻し、中の cancel 節を走らせる。
    // 節が終わったら継続を節の外にする。節の中から外側へ YIELD が届いたときは、節はまだ終わっていない。
    b.switch_to(gval);
    fx(&mut b, FxOp::ContKill, vec![kk]);
    let used = fx_val(&mut b, FxOp::ContUsed, vec![kk]);
    let retg = b.block();
    let drop = b.block();
    b.terminate(Term::Branch {
        cond: used,
        then: retg,
        else_: drop,
    });
    b.switch_to(retg);
    b.terminate(Term::Return(g));
    b.switch_to(drop);
    let inj = fx_val(&mut b, FxOp::PendingDiscard, vec![hr, g]);
    b.terminate(Term::TailCallDirect {
        func: me,
        args: vec![hr, kk, inj],
        yields: true,
    });
    // 節が状態語を立てて戻った。
    b.switch_to(gpend);
    let gc = fx_val(&mut b, FxOp::PendingClass, vec![hr]);
    let tr = b.block();
    let g3 = b.block();
    branch_eq(&mut b, gc, TAIL_RESUME, tr);
    branch_eq(&mut b, gc, OTHER_YIELD, prop);
    b.terminate(Term::Jump(g3));
    // 末尾の resume(v): 自分を rt_resume(k, v) の末尾呼び出しで置き換える。
    b.switch_to(tr);
    fx(&mut b, FxOp::ContKill, vec![kk]);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    let tv = fx_val(&mut b, FxOp::PendingValue, vec![p]);
    b.terminate(Term::TailCallDirect {
        func: me,
        args: vec![hr, kk, tv],
        yields: true,
    });
    // 脱出。resume の後ならそのまま伝播し、前なら同じ状態を継続に注入する。
    b.switch_to(g3);
    fx(&mut b, FxOp::ContKill, vec![kk]);
    let used2 = fx_val(&mut b, FxOp::ContUsed, vec![kk]);
    let inj_b = b.block();
    b.terminate(Term::Branch {
        cond: used2,
        then: prop,
        else_: inj_b,
    });
    b.switch_to(inj_b);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    b.terminate(Term::TailCallDirect {
        func: me,
        args: vec![hr, kk, p],
        yields: true,
    });

    // 計画 6.6 節の手順 6(16.3 節の手順 4 の 5): ほかの宛ての DISCARD と ERROR。連鎖から外し、防壁の中で cancel 節を走らせる。
    b.switch_to(other);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let hc = fx_val(&mut b, FxOp::HandlerHas(HandlerPart::Cancel), vec![hr]);
    let bar = b.block();
    b.terminate(Term::Branch {
        cond: hc,
        then: bar,
        else_: prop,
    });
    b.switch_to(bar);
    let p3 = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    let lim = fx_val(&mut b, FxOp::LimitGet, Vec::new());
    let cf = fx_val(&mut b, FxOp::HandlerGet(HandlerPart::Cancel), vec![hr]);
    let c = fx_val(&mut b, FxOp::CallCancel, vec![cf]);
    b.emit(Inst::DepthRestore);
    let cp = fx_val(&mut b, FxOp::IsPending, vec![c]);
    let cpend = b.block();
    let cdone = b.block();
    b.terminate(Term::Branch {
        cond: cp,
        then: cpend,
        else_: cdone,
    });
    // cancel 節が YIELD を立てたら、P を持ったまま自分のフレームも継続に含めて戻る。
    // DISCARD か ERROR なら打ち切って記録する。
    b.switch_to(cpend);
    let cc = fx_val(&mut b, FxOp::PendingClass, vec![hr]);
    let supp = b.block();
    branch_eq(&mut b, cc, OTHER_YIELD, prop);
    b.terminate(Term::Jump(supp));
    b.switch_to(supp);
    fx(&mut b, FxOp::PendingSuppress, Vec::new());
    b.terminate(Term::Jump(cdone));
    b.switch_to(cdone);
    fx(&mut b, FxOp::LimitSet, vec![lim]);
    fx(&mut b, FxOp::PendingRestore, vec![p3]);
    b.terminate(Term::Fx {
        op: FxTermOp::Propagate,
        args: Vec::new(),
    });

    // 計画 6.6 節の手順 7(16.3 節の手順 4 の 4)と、伝播を続ける場合。
    b.switch_to(prop);
    b.terminate(Term::Fx {
        op: FxTermOp::Propagate,
        args: Vec::new(),
    });
    b
}
