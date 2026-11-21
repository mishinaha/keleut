//! エフェクトの LIR への変換(パス 8。`doc/lir.md` の 16 節)。
//!
//! `handle`、ユーザの操作の `perform`、`resume` と、操作節の関数の入口をここで変換する。
//! 変換の規約は次のとおりである。
//!
//! - ハンドラのフレームになる関数は、すべての `handle` が共有する LIR の関数 `$rt_handle(hr, k, v)` である
//!   ([`rt_handle_body`])。
//!   `handle` は、ハンドラ記録 `hr` を作って `$rt_handle(hr, unit, unit)` を呼ぶ。
//!   末尾の `handle` なら、末尾呼び出しで呼ぶ。
//!   継続 `k` を再開するとき(`doc/lir.md` の `rt_resume`)は、`$rt_handle(k.hr, k, v)` を呼ぶ。
//!   呼ばれた `$rt_handle` は、新しいハンドラのフレームの上に `k` のフレームを復元して、実行を続ける。
//!   本体の値、自分宛ての破棄、自分宛ての yield、ほかのハンドラ宛ての巻き戻しは、どちらの呼ばれ方でも同じ LIR の命令列で処理する。
//!   ほかのハンドラ宛ての巻き戻しでは、防壁(`doc/lir.md` の 16.4 節)の中で `cancel` 節を実行する。
//! - 操作節の関数は、KIR の引数の後ろに暗黙の引数 `x` と `sel` を持つ。
//!   `sel` が 0 以上なら選択フェーズで、`x` はハンドラ記録、`sel` はこの節の候補の番号である。
//!   パターンが一致しないかガードが偽なら、次の候補を末尾呼び出しで呼ぶ(`tail_call_clause`)。
//!   一致したとき、分類(IRSpec 9.2 節)が T の節はそのまま本体を実行し、N と G の節は `clause_matched` で戻る。
//!   `sel` が負なら本体フェーズで、`x` は G の節では継続、N の節では Unit である。
//!   パターンをもう一度照合し、ガードを飛ばして本体を実行する。
//!   照合は副作用を持たないので、束縛は選択フェーズと同じになる。
//! - ハンドラ式の本体の関数は、暗黙の引数として、引き継ぐ継続を受け取る。
//!   引き継ぐ継続は、ハンドラ式を書いた関数が G の節の関数かハンドラ式の本体の関数なら、その関数が持つ継続である。
//!   そうでなければ Unit である。
//!   本体の中の `resume` はこの継続を再開する(IRSpec 9.2 節)。

use kir::{ClauseClass, FnKind};

use crate::builder::Builder;
use crate::ir::*;
use crate::lower::{Low, V};
use crate::pattern::Target;

/// 操作節の関数ごとに、操作と分類の組を求めて `out` に書く。
/// 操作節でない関数は `None` になる。
///
/// 単位を追加したプログラムでは、それまでの関数の結果を `out` に入れて渡し、`f0` 番以降の関数の本体だけを調べる。
/// 追加した操作節の関数を作る `handle` は、追加した関数の本体にあるからである。
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

/// エフェクトの命令 `op` を置く。
/// 結果を持つ命令なら、結果のレジスタを返す。
fn fx(b: &mut Builder, op: FxOp, args: Vec<Reg>) -> Option<Reg> {
    let dst = op.sig().1.map(|rep| b.reg(rep));
    b.emit(Inst::Fx { op, dst, args });
    dst
}

/// 結果を持つエフェクトの命令 `op` を置き、結果のレジスタを返す。
fn fx_val(b: &mut Builder, op: FxOp, args: Vec<Reg>) -> Reg {
    fx(b, op, args).expect("結果を持つエフェクトの命令")
}

/// ユーザの操作の `perform` を変換する。
/// `args` は、引数を評価して一様表現の値にしたものである。
pub(crate) fn perform(lw: &mut Low, effect: u32, op: u32, args: Vec<Reg>) -> V {
    let hr = fx_val(&mut lw.b, FxOp::HandlerFind { effect, op }, Vec::new());
    let saved = fx_val(&mut lw.b, FxOp::HandlersGet, Vec::new());
    let mut a = vec![hr, saved];
    a.extend(args);
    V::any(fx_val(&mut lw.b, FxOp::Perform { effect, op }, a))
}

/// ハンドラ式を変換する。
/// `tail` が真なら、`$rt_handle` を末尾呼び出しで呼んでブロックを終え、`None` を返す。
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
        return lw.bug("$rt_handle がない");
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
    // ハンドラのフレームはハンドラ式の位置の深さ `h` で実行するので、深さを増やさずに呼ぶ。
    let dst = lw.b.reg(Rep::Any);
    lw.b.emit(Inst::CallDirect {
        dst,
        func: f,
        args,
        yields: true,
    });
    Some(V::any(dst))
}

/// 末尾位置にない `resume(v)` を変換する。
/// `k` は再開する継続、`v` は評価した値である。
pub(crate) fn resume(lw: &mut Low, k: Reg, v: Reg) -> Option<V> {
    let Some(f) = lw.p.rt_handle else {
        return lw.bug("$rt_handle がない");
    };
    fx(&mut lw.b, FxOp::ContCheck { nontail: true }, vec![k]);
    let hr = fx_val(&mut lw.b, FxOp::ContHr, vec![k]);
    // 再開した計算のハンドラのフレームは、`resume` の位置の深さ `c` で実行する(IRSpec 11.4 節)。
    // そのため、深さを増やさずに呼ぶ。
    let dst = lw.b.reg(Rep::Any);
    lw.b.emit(Inst::CallDirect {
        dst,
        func: f,
        args: vec![hr, k, v],
        yields: true,
    });
    Some(V::any(dst))
}

/// 関数の入口の命令の後に、関数の種類に応じた暗黙の引数を追加する。
/// 操作節の関数なら、本体まで変換して `true` を返す。
/// それ以外の関数では `false` を返す。
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

/// 操作節の関数の、入口の命令より後の部分を変換する。
/// 暗黙の引数、パターンの照合、ガード、本体をこの順に置く。
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
            // 本体フェーズ(`sel` が負)なら、ガードを飛ばす。
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

/// 操作節のガードを変換する。
/// ガードが偽なら `miss` へ分岐する。
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

/// `c == n` なら `yes` へ、そうでなければ新しいブロックへ分岐する命令を置く。
/// 続きの命令は、その新しいブロックに追加される。
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

/// `pending_class` が返す値(`doc/lir.md` の 16.5 節)。
pub mod class {
    /// 自分宛ての `DISCARD(Value v)`。
    pub const OWN_VALUE: i32 = 1;
    /// 自分宛ての `DISCARD(NeverBody)`。
    pub const OWN_NEVER: i32 = 2;
    /// 自分宛ての `YIELD`。
    pub const OWN_YIELD: i32 = 3;
    /// ほかのハンドラ宛ての `YIELD`。
    pub const OTHER_YIELD: i32 = 4;
    /// ほかのハンドラ宛ての `DISCARD` と `ERROR`。
    pub const OTHER_UNWIND: i32 = 5;
    /// G の節の末尾 `resume` が設定する、`tail_resume` の目印。
    pub const TAIL_RESUME: i32 = 6;
}

/// ハンドラのフレームになる関数 `$rt_handle(hr, k, v)` の本体を作る(`doc/lir.md` の 16.3 節)。
/// `me` は、この関数自身の番号である。
///
/// コメントの「手順 N」は、16.3 節の番号付きの項目を指す。
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

    // 手順 1(rt_handle)。本体の深さ h + 1 は、ハンドラを連鎖に入れる前に上限と比較する。
    b.switch_to(fresh);
    b.emit(Inst::DepthCheck);
    fx(&mut b, FxOp::HandlerInstall, vec![hr]);
    b.emit(Inst::DepthPush);
    let r1 = fx_val(&mut b, FxOp::CallBody, vec![hr]);
    b.emit(Inst::DepthRestore);
    b.emit(Inst::Move { dst: r, src: r1 });
    b.terminate(Term::Jump(post));

    // 手順 2(rt_resume)。再開済みなら実行時エラー 7 にする。そうでなければ、継続のフレームを復元して実行を続ける。
    b.switch_to(res);
    fx(&mut b, FxOp::ContCheck { nontail: false }, vec![k]);
    let r2 = fx_val(&mut b, FxOp::ContResume, vec![k, v]);
    b.emit(Inst::DepthRestore);
    b.emit(Inst::Move { dst: r, src: r2 });
    b.terminate(Term::Jump(post));

    // 手順 3〜4。結果が番兵かどうかで場合を分ける。
    b.switch_to(post);
    let ip = fx_val(&mut b, FxOp::IsPending, vec![r]);
    let pend = b.block();
    let normal = b.block();
    b.terminate(Term::Branch {
        cond: ip,
        then: pend,
        else_: normal,
    });

    // 手順 3。値で戻ったら、ハンドラを連鎖から外す。return 節があれば末尾呼び出しで呼び、なければその値を返す。
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

    // 手順 4 の 1。自分宛ての DISCARD(Value v)。return 節も cancel 節も実行しない。
    b.switch_to(own_val);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    let pv = fx_val(&mut b, FxOp::PendingValue, vec![p]);
    b.terminate(Term::Return(pv));

    // 手順 4 の 2。自分宛ての DISCARD(NeverBody)。N の節の本体を末尾呼び出しで呼ぶ。
    b.switch_to(own_never);
    fx(&mut b, FxOp::HandlerUninstall, vec![hr]);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    b.terminate(Term::Fx {
        op: FxTermOp::TailCallNever,
        args: vec![p],
    });

    // 手順 4 の 3。自分宛ての YIELD。ハンドラを連鎖から外し、継続を作って、G の節を非末尾呼び出しで呼ぶ。
    // 先に外すので、継続を作れずにメモリ不足になっても、無効になったハンドラが連鎖に残らない。
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
    // 節が値 g で戻った。継続を使っていれば、g がこのフレームの値である。
    // 使っていなければ、DISCARD(hr, Value g) を継続に注入して、継続の中の cancel 節を実行する。
    // 節が終わったので、cont_kill で継続の alive フラグを下ろす。
    // 以後、この継続は節の外にある継続として扱う。
    // 節の中から外側へ YIELD が届いたとき(下の gpend の分岐)は、節がまだ終わっていないので、cont_kill を行わない。
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
    // 節が番兵を返して戻った。状態ワードが設定されている。
    b.switch_to(gpend);
    let gc = fx_val(&mut b, FxOp::PendingClass, vec![hr]);
    let tr = b.block();
    let g3 = b.block();
    branch_eq(&mut b, gc, TAIL_RESUME, tr);
    branch_eq(&mut b, gc, OTHER_YIELD, prop);
    b.terminate(Term::Jump(g3));
    // 末尾 resume(v)。自分のフレームを、rt_resume(k, v) の末尾呼び出しで置き換える。
    b.switch_to(tr);
    fx(&mut b, FxOp::ContKill, vec![kk]);
    let p = fx_val(&mut b, FxOp::PendingTake, Vec::new());
    let tv = fx_val(&mut b, FxOp::PendingValue, vec![p]);
    b.terminate(Term::TailCallDirect {
        func: me,
        args: vec![hr, kk, tv],
        yields: true,
    });
    // 節が DISCARD か ERROR で脱出した。resume の後ならそのまま伝播し、前なら同じ状態ワードを継続に注入する。
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

    // 手順 4 の 5。ほかのハンドラ宛ての DISCARD と ERROR。ハンドラを連鎖から外し、cancel 節があれば防壁(doc/lir.md の 16.4 節)の中で実行する。
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
    // cancel 節がほかのハンドラ宛ての YIELD で戻ったら、退避した状態ワード p3 と上限 lim をスロットに持ったまま、自分のフレームも継続に含めて戻る。
    // DISCARD か ERROR で戻ったら、抑制して記録する。
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

    // 手順 4 の 4(ほかのハンドラ宛ての YIELD)と、それ以外で伝播を続ける場合。
    b.switch_to(prop);
    b.terminate(Term::Fx {
        op: FxTermOp::Propagate,
        args: Vec::new(),
    });
    b
}
