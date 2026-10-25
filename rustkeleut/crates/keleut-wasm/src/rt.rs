//! 実行時の関数(モジュールの中に置く)。
//!
//! ## 値の表現(計画 6.10 節の表からの変更を含む)
//!
//! | 値 | WasmGC |
//! | --- | --- |
//! | 一様な語 | `eqref`(null は番兵で、値には現れない) |
//! | `Int32` | 31 ビットに収まれば `i31ref`、収まらなければ `$BoxI32` |
//! | Unit、真偽値 | 大域に 1 つずつ置いた `$Unit`、`$Bool` の構造体(`ref.eq` で比べる) |
//! | データ | `$Data`(型、コンストラクタ、フィールドの配列)。引数の無いコンストラクタは空の配列で、大域に 1 つずつ置く |
//! | `Int64`、`Float64` | `$BoxI64`、`$BoxF64` |
//! | 文字列 | `$Str = (array (mut i8))` |
//! | レコード、ヴァリアント | `$Record`(形、フィールドの配列)、`$Variant`(ラベル、積載値)。Unit は空のレコードとして扱う |
//! | クロージャ | `$CloN`(関数の番号、捕獲の配列、`(ref $FnN)`)。基底は `$Clo` |
//! | `Ref`、`Array`、`MutableArray` | `$Ref`、`$ImmArr`、`$MutArr`(どちらも `$AnyArr` の部分型) |
//!
//! 計画の表は、Unit、真偽値、引数の無いコンストラクタも `i31ref` にする。
//! しかし `Int32` と同じ `i31ref` にすると値の種類が分からず、実行時エラーの文面の `<値>`
//! (`()`、`true`、コンストラクタの名前)をインタプリタと同じに書けない。
//! 後段の間の一致(D499)は標準エラーも比べるので、これらは種類の分かる構造体にした。
//!
//! ## 状態語
//!
//! 状態語は大域 `$p_kind`(種類)と、`$p_hr`、`$p_val`、`$p_clause`、`$p_args`、`$p_msg` に置く。
//! 状態語が立っているとき、関数は null(番兵)を返す。
//! 実行時エラーの文面は、立てた時点でホストが作り(`host.err`)、番号 `$p_msg` で指す。
//!
//! ## trap しないこと(D543)
//!
//! trap しうる命令(整数の除算と剰余、配列の添字と長さと写し、`array.new_data`、線形メモリの読み書き)は、
//! 名前が `safe.` で始まる関数の中にだけ置く([`crate::check`] の検査器が確かめる)。
//! これらの関数は、先に添字や長さを確かめてから命令を使う。
//! 長さを取る確保は `N_alloc_max` と比べ、超えればメモリの不足の `ERROR` にする。

use keleut_lir::{FuncId, N_ALLOC_MAX};
use wasm_encoder::{BlockType, Function, HeapType, MemArg, RefType, ValType};

use crate::emit::{EQ, F64, Fb, HEQ, HI31, I32, I64, gimp, hrf, imp, rv, svf};
use crate::meta::{arg, kind};
use crate::{Cg, FKey, cls, pk};

/// 固定の実行時の関数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum H {
    Raise,
    RaiseDepth,
    RaiseOom,
    BoxI32,
    UnboxI32,
    Aget,
    Aset,
    I32Get,
    AnyArrNew,
    StrNewData,
    StrConcat,
    StrSub,
    StrCmp,
    ShowI64,
    ShowF64,
    MArrayNew,
    Freeze,
    Write,
    SOut,
    RecGet,
    RecField,
    RecHas,
    RecExtend,
    RecRestrict,
    RecUpdate,
    CtorField,
    ClauseFind,
    HandlerFind,
    PendingClass,
    PendingTake,
    PendingRestore,
    PerformUnwound,
    Suppress,
    IsExtern,
    I32Div,
    I32Rem,
    I64Div,
    I64Rem,
    Main,
    VKind,
    VA,
    VB,
    VI64,
    VN,
    VGet,
}

/// ホストに export する関数(値の印字に使う)。
pub const EXPORTED: &[(&str, H)] = &[
    ("v_kind", H::VKind),
    ("v_a", H::VA),
    ("v_b", H::VB),
    ("v_i64", H::VI64),
    ("v_n", H::VN),
    ("v_get", H::VGet),
    ("s_out", H::SOut),
];

/// 値の種類の番号(ホストの `v_kind` と同じ)。
pub mod vk {
    pub const OTHER: i32 = 0;
    pub const I31: i32 = 1;
    pub const BOX_I32: i32 = 2;
    pub const I64: i32 = 3;
    pub const F64: i32 = 4;
    pub const STR: i32 = 5;
    pub const UNIT: i32 = 6;
    pub const BOOL: i32 = 7;
    pub const DATA: i32 = 8;
    pub const VARIANT: i32 = 9;
    pub const RECORD: i32 = 10;
    pub const CLOSURE: i32 = 11;
    pub const METHOD: i32 = 12;
    pub const EXTERNU: i32 = 13;
    pub const REF: i32 = 14;
    pub const ARRAY: i32 = 15;
    pub const MARRAY: i32 = 16;
    pub const DICT: i32 = 17;
}

const MEM8: MemArg = MemArg {
    offset: 0,
    align: 0,
    memory_index: 0,
};

const MEM32: MemArg = MemArg {
    offset: 0,
    align: 2,
    memory_index: 0,
};

/// `krt_shape_*` の「見つからない」。
const SHAPE_NONE: i32 = -1;
/// `krt_shape_*` の「表が満ちた」。
const SHAPE_FULL: i32 = -2;

/// 関数の型と、trap しうる命令を許すか。
pub fn sig(cg: &mut Cg, h: H) -> (Vec<ValType>, Vec<ValType>, bool) {
    let t = cg.m.t;
    let s = rv(true, t.str_);
    let arr = rv(true, t.anyarr);
    use H::*;
    match h {
        Raise => (vec![I32, EQ, I64], vec![], false),
        RaiseDepth | RaiseOom | Suppress => (vec![], vec![], false),
        BoxI32 => (vec![I32], vec![EQ], false),
        UnboxI32 => (vec![EQ], vec![I32], false),
        Aget => (vec![arr, I32], vec![EQ], true),
        Aset => (vec![arr, I32, EQ], vec![], true),
        I32Get => (vec![rv(true, t.i32arr), I32], vec![I32], true),
        AnyArrNew => (vec![I32], vec![rv(false, t.anyarr)], true),
        StrNewData => (vec![I32, I32], vec![rv(false, t.str_)], true),
        StrConcat => (vec![s, s], vec![s], true),
        StrSub => (vec![s, I32, I32], vec![s], true),
        StrCmp => (vec![s, s], vec![I32], true),
        ShowI64 => (vec![I64], vec![rv(false, t.str_)], true),
        ShowF64 => (vec![F64], vec![rv(false, t.str_)], true),
        MArrayNew => (vec![I32, EQ], vec![rv(true, t.mutarr)], true),
        Freeze => (vec![arr], vec![rv(false, t.immarr)], true),
        Write => (vec![s], vec![I32], true),
        SOut => (vec![EQ], vec![I32], true),
        RecGet | CtorField => (vec![EQ, I32], vec![EQ], false),
        RecField => (vec![EQ, I32, I32], vec![EQ], true),
        RecHas => (vec![EQ, I32, I32], vec![I32], true),
        RecExtend | RecUpdate => (vec![EQ, I32, EQ], vec![EQ], true),
        RecRestrict => (vec![EQ, I32], vec![EQ], true),
        ClauseFind => (vec![rv(true, t.hr), I32, I32], vec![I32], true),
        HandlerFind => (vec![I32], vec![EQ], false),
        PendingClass => (vec![EQ], vec![I32], false),
        PendingTake => (vec![], vec![rv(false, t.saved)], false),
        PendingRestore => (vec![EQ], vec![], false),
        PerformUnwound => (vec![EQ, EQ, EQ], vec![], false),
        IsExtern => (vec![EQ], vec![I32], false),
        I32Div | I32Rem => (vec![I32, I32], vec![I32], true),
        I64Div | I64Rem => (vec![I64, I64], vec![I64], true),
        Main => (vec![I64], vec![I32], false),
        VKind | VA | VB | VN => (vec![EQ], vec![I32], false),
        VI64 => (vec![EQ], vec![I64], false),
        VGet => (vec![EQ, I32], vec![EQ], false),
    }
}

fn ht(idx: u32) -> HeapType {
    HeapType::Concrete(idx)
}

/// 内部の誤りで止まる(`host.fatal`)。後に `unreachable` を置く。
pub fn fatal(cg: &mut Cg, fb: &mut Fb, text: &str) {
    let t = cg.tpl.internal(text);
    fb.i().i32_const(t as i32).call(imp::FATAL).unreachable();
}

/// 未実装で止まる。
pub fn unimpl(cg: &mut Cg, fb: &mut Fb, text: &str) {
    let t = cg.tpl.add(kind::UNIMPL, arg::NONE, text);
    fb.i().i32_const(t as i32).call(imp::FATAL).unreachable();
}

/// G の節に届いた(M94 は未実装の 4 で止まる)。
pub fn g_unimpl(cg: &mut Cg, fb: &mut Fb) {
    unimpl(
        cg,
        fb,
        "WasmGC の後段の G の節の取り出しと再開(M95 で入れる)",
    );
}

/// 本体を作る。
pub fn generate(cg: &mut Cg, k: &FKey) -> Function {
    match k {
        FKey::H(h) => gen_h(cg, *h),
        FKey::CallV(n) => gen_callv(cg, *n),
        FKey::CallM(c, m, no, n) => gen_callm(cg, *c, *m, *no, *n),
        FKey::DictApp(n) => gen_dictapp(cg, *n),
        FKey::TailNever => gen_tail_never(cg),
        FKey::EqStruct(c) => gen_eq_struct(cg, *c),
        FKey::Lir(_) => unreachable!("LIR の関数は func::generate が作る"),
    }
}

fn nparams(cg: &mut Cg, h: H) -> u32 {
    sig(cg, h).0.len() as u32
}

/// `v` の形と(Unit なら 0 と空の配列)フィールドを局所 `shape`、`fs` に置く。
fn shape_fields(cg: &mut Cg, fb: &mut Fb, v: u32, shape: u32, fs: u32) {
    let t = cg.m.t;
    let g = cg.g;
    let rr = fb.local(rv(true, t.record));
    fb.i()
        .local_get(v)
        .ref_test_non_null(ht(t.record))
        .if_(BlockType::Empty)
        .local_get(v)
        .ref_cast_non_null(ht(t.record))
        .local_tee(rr)
        .struct_get(t.record, 0)
        .local_set(shape)
        .local_get(rr)
        .struct_get(t.record, 1)
        .local_set(fs)
        .else_()
        .i32_const(0)
        .local_set(shape)
        .global_get(g.empty)
        .local_set(fs)
        .end();
}

/// `krt_shape_extend` か `krt_shape_restrict` を、表が満ちたら広げてやり直す形で呼び、結果を `ns` に置く。
fn shape_op(cg: &mut Cg, fb: &mut Fb, f: u32, shape: u32, label: u32, ns: u32) {
    let tfull = cg.tpl.internal("形の表を広げられません");
    fb.i()
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .global_get(gimp::SHAPES)
        .local_get(shape)
        .local_get(label)
        .global_get(gimp::SCRATCH)
        .call(f)
        .local_tee(ns)
        .i32_const(SHAPE_FULL)
        .i32_ne()
        .br_if(1)
        .call(imp::SHAPES_GROW)
        .i32_eqz()
        .br_if(0)
        .i32_const(tfull as i32)
        .call(imp::FATAL)
        .unreachable()
        .end()
        .end();
}

/// `i < n` の間 `body` を繰り返す(`i` は 0 から)。
fn for_range(fb: &mut Fb, i: u32, n: u32, body: impl FnOnce(&mut Fb)) {
    fb.i()
        .i32_const(0)
        .local_set(i)
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(i)
        .local_get(n)
        .i32_ge_u()
        .br_if(1);
    body(fb);
    fb.i()
        .local_get(i)
        .i32_const(1)
        .i32_add()
        .local_set(i)
        .br(0)
        .end()
        .end();
}

fn gen_h(cg: &mut Cg, h: H) -> Function {
    let t = cg.m.t;
    let g = cg.g;
    let np = nparams(cg, h);
    let mut fb = Fb::new(np);
    let eqt = RefType::EQREF;
    use H::*;
    match h {
        Raise => {
            fb.i()
                .local_get(0)
                .local_get(1)
                .local_get(2)
                .call(imp::ERR)
                .global_set(g.p_msg)
                .i32_const(pk::ERROR)
                .global_set(g.p_kind);
        }
        RaiseDepth => {
            let td = cg.tpl.add(kind::DEPTH, arg::NONE, "");
            let raise = cg.func(FKey::H(Raise));
            fb.i()
                .global_get(g.lr)
                .global_set(g.limit)
                .i32_const(td as i32)
                .ref_null(HEQ)
                .i64_const(0)
                .call(raise);
        }
        RaiseOom => {
            let to = cg.tpl.add(kind::OOM, arg::NONE, "");
            let raise = cg.func(FKey::H(Raise));
            fb.i()
                .i32_const(to as i32)
                .ref_null(HEQ)
                .i64_const(0)
                .call(raise);
        }
        BoxI32 => {
            fb.i()
                .local_get(0)
                .i32_const(1)
                .i32_shl()
                .i32_const(1)
                .i32_shr_s()
                .local_get(0)
                .i32_eq()
                .if_(BlockType::Result(EQ))
                .local_get(0)
                .ref_i31()
                .else_()
                .local_get(0)
                .struct_new(t.box_i32)
                .end();
        }
        UnboxI32 => {
            fb.i()
                .block(BlockType::Result(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HI31,
                })))
                .local_get(0)
                .br_on_cast(
                    0,
                    eqt,
                    RefType {
                        nullable: false,
                        heap_type: HI31,
                    },
                )
                .ref_cast_non_null(ht(t.box_i32))
                .struct_get(t.box_i32, 0)
                .return_()
                .end()
                .i31_get_s();
        }
        Aget => {
            fb.i()
                .local_get(1)
                .local_get(0)
                .array_len()
                .i32_lt_u()
                .if_(BlockType::Result(EQ))
                .local_get(0)
                .local_get(1)
                .array_get(t.anyarr)
                .else_()
                .global_get(g.unit)
                .end();
        }
        Aset => {
            fb.i()
                .local_get(1)
                .local_get(0)
                .array_len()
                .i32_lt_u()
                .if_(BlockType::Empty)
                .local_get(0)
                .local_get(1)
                .local_get(2)
                .array_set(t.anyarr)
                .end();
        }
        I32Get => {
            fb.i()
                .local_get(1)
                .local_get(0)
                .array_len()
                .i32_lt_u()
                .if_(BlockType::Result(I32))
                .local_get(0)
                .local_get(1)
                .array_get(t.i32arr)
                .else_()
                .i32_const(0)
                .end();
        }
        AnyArrNew => {
            fb.i().local_get(0).array_new_default(t.anyarr);
        }
        StrNewData => {
            fb.i().local_get(0).local_get(1).array_new_data(t.str_, 0);
        }
        StrConcat => {
            let oom = cg.func(FKey::H(RaiseOom));
            let la = fb.local(I32);
            let lb = fb.local(I32);
            let res = fb.local(rv(true, t.str_));
            fb.i()
                .local_get(0)
                .array_len()
                .local_set(la)
                .local_get(1)
                .array_len()
                .local_set(lb)
                .local_get(la)
                .local_get(lb)
                .i32_add()
                .i32_const(N_ALLOC_MAX)
                .i32_gt_u()
                .if_(BlockType::Empty)
                .call(oom)
                .ref_null(ht(t.str_))
                .return_()
                .end()
                .local_get(la)
                .local_get(lb)
                .i32_add()
                .array_new_default(t.str_)
                .local_set(res)
                .local_get(res)
                .i32_const(0)
                .local_get(0)
                .i32_const(0)
                .local_get(la)
                .array_copy(t.str_, t.str_)
                .local_get(res)
                .local_get(la)
                .local_get(1)
                .i32_const(0)
                .local_get(lb)
                .array_copy(t.str_, t.str_)
                .local_get(res);
        }
        StrSub => {
            let n = fb.local(I32);
            let start = fb.local(I32);
            let l = fb.local(I32);
            let end = fb.local(I32);
            let res = fb.local(rv(true, t.str_));
            // start = min(max(pos, 0), n)、l = min(max(len, 0), n)、end = min(start + l, n)
            let mut ins = fb.i();
            ins.local_get(0).array_len().local_set(n);
            for (src, dst) in [(1u32, start), (2u32, l)] {
                ins.local_get(src)
                    .i32_const(0)
                    .local_get(src)
                    .i32_const(0)
                    .i32_gt_s()
                    .select()
                    .local_set(dst)
                    .local_get(dst)
                    .local_get(n)
                    .local_get(dst)
                    .local_get(n)
                    .i32_lt_s()
                    .select()
                    .local_set(dst);
            }
            ins.local_get(start)
                .local_get(l)
                .i32_add()
                .local_set(end)
                .local_get(end)
                .local_get(n)
                .local_get(end)
                .local_get(n)
                .i32_lt_s()
                .select()
                .local_set(end)
                .local_get(end)
                .local_get(start)
                .i32_sub()
                .local_set(l)
                .local_get(l)
                .array_new_default(t.str_)
                .local_set(res)
                .local_get(res)
                .i32_const(0)
                .local_get(0)
                .local_get(start)
                .local_get(l)
                .array_copy(t.str_, t.str_)
                .local_get(res);
        }
        StrCmp => {
            let la = fb.local(I32);
            let lb = fb.local(I32);
            let m = fb.local(I32);
            let i = fb.local(I32);
            let ca = fb.local(I32);
            let cb = fb.local(I32);
            fb.i()
                .local_get(0)
                .array_len()
                .local_set(la)
                .local_get(1)
                .array_len()
                .local_set(lb)
                .local_get(la)
                .local_get(lb)
                .local_get(la)
                .local_get(lb)
                .i32_lt_u()
                .select()
                .local_set(m);
            for_range(&mut fb, i, m, |fb| {
                fb.i()
                    .local_get(0)
                    .local_get(i)
                    .array_get_u(t.str_)
                    .local_set(ca)
                    .local_get(1)
                    .local_get(i)
                    .array_get_u(t.str_)
                    .local_set(cb)
                    .local_get(ca)
                    .local_get(cb)
                    .i32_ne()
                    .if_(BlockType::Empty)
                    .local_get(ca)
                    .local_get(cb)
                    .i32_gt_u()
                    .local_get(ca)
                    .local_get(cb)
                    .i32_lt_u()
                    .i32_sub()
                    .return_()
                    .end();
            });
            fb.i()
                .local_get(la)
                .local_get(lb)
                .i32_gt_u()
                .local_get(la)
                .local_get(lb)
                .i32_lt_u()
                .i32_sub();
        }
        ShowI64 => {
            let neg = fb.local(I32);
            let u = fb.local(I64);
            let tt = fb.local(I64);
            let n = fb.local(I32);
            let len = fb.local(I32);
            let res = fb.local(rv(true, t.str_));
            let i = fb.local(I32);
            fb.i()
                .local_get(0)
                .i64_const(0)
                .i64_lt_s()
                .local_set(neg)
                .i64_const(0)
                .local_get(0)
                .i64_sub()
                .local_get(0)
                .local_get(neg)
                .select()
                .local_set(u)
                .local_get(u)
                .local_set(tt)
                .i32_const(1)
                .local_set(n)
                .block(BlockType::Empty)
                .loop_(BlockType::Empty)
                .local_get(tt)
                .i64_const(10)
                .i64_div_u()
                .local_tee(tt)
                .i64_eqz()
                .br_if(1)
                .local_get(n)
                .i32_const(1)
                .i32_add()
                .local_set(n)
                .br(0)
                .end()
                .end()
                .local_get(n)
                .local_get(neg)
                .i32_add()
                .local_tee(len)
                .array_new_default(t.str_)
                .local_set(res)
                .local_get(len)
                .local_set(i)
                .loop_(BlockType::Empty)
                .local_get(i)
                .i32_const(1)
                .i32_sub()
                .local_set(i)
                .local_get(res)
                .local_get(i)
                .local_get(u)
                .i64_const(10)
                .i64_rem_u()
                .i32_wrap_i64()
                .i32_const(48)
                .i32_add()
                .array_set(t.str_)
                .local_get(u)
                .i64_const(10)
                .i64_div_u()
                .local_tee(u)
                .i64_eqz()
                .i32_eqz()
                .br_if(0)
                .end()
                .local_get(neg)
                .if_(BlockType::Empty)
                .local_get(res)
                .i32_const(0)
                .i32_const(45)
                .array_set(t.str_)
                .end()
                .local_get(res)
                .ref_as_non_null();
        }
        ShowF64 => {
            let n = fb.local(I32);
            let res = fb.local(rv(true, t.str_));
            let i = fb.local(I32);
            fb.i()
                .local_get(0)
                .i64_reinterpret_f64()
                .global_get(gimp::SCRATCH)
                .i32_const(16)
                .i32_add()
                .call(imp::F64_SHOW_BITS)
                .local_tee(n)
                .array_new_default(t.str_)
                .local_set(res);
            for_range(&mut fb, i, n, |fb| {
                fb.i()
                    .local_get(res)
                    .local_get(i)
                    .global_get(gimp::SCRATCH)
                    .i32_const(16)
                    .i32_add()
                    .local_get(i)
                    .i32_add()
                    .i32_load8_u(MEM8)
                    .array_set(t.str_);
            });
            fb.i().local_get(res).ref_as_non_null();
        }
        MArrayNew => {
            let oom = cg.func(FKey::H(RaiseOom));
            fb.i()
                .local_get(0)
                .i32_const(0)
                .i32_lt_s()
                .if_(BlockType::Empty)
                .i32_const(0)
                .local_set(0)
                .end()
                .local_get(0)
                .i32_const(N_ALLOC_MAX)
                .i32_gt_s()
                .if_(BlockType::Empty)
                .call(oom)
                .ref_null(ht(t.mutarr))
                .return_()
                .end()
                .local_get(1)
                .local_get(0)
                .array_new(t.mutarr);
        }
        Freeze => {
            let n = fb.local(I32);
            let res = fb.local(rv(true, t.immarr));
            fb.i()
                .local_get(0)
                .array_len()
                .local_tee(n)
                .array_new_default(t.immarr)
                .local_set(res)
                .local_get(res)
                .i32_const(0)
                .local_get(0)
                .i32_const(0)
                .local_get(n)
                .array_copy(t.immarr, t.anyarr)
                .local_get(res)
                .ref_as_non_null();
        }
        Write | SOut => {
            let s = fb.local(rv(true, t.str_));
            let n = fb.local(I32);
            let ptr = fb.local(I32);
            let i = fb.local(I32);
            let rr = fb.local(I32);
            let mut ins = fb.i();
            ins.local_get(0);
            if h == SOut {
                ins.ref_cast_nullable(ht(t.str_));
            }
            ins.local_set(s)
                .local_get(s)
                .array_len()
                .local_tee(n)
                .call(imp::BUF)
                .local_set(ptr);
            for_range(&mut fb, i, n, |fb| {
                fb.i()
                    .local_get(ptr)
                    .local_get(i)
                    .i32_add()
                    .local_get(s)
                    .local_get(i)
                    .array_get_u(t.str_)
                    .i32_store8(MEM8);
            });
            if h == SOut {
                fb.i().local_get(ptr);
            } else {
                fb.i()
                    .local_get(ptr)
                    .local_get(n)
                    .call(imp::WRITE)
                    .local_tee(rr)
                    .if_(BlockType::Empty)
                    .local_get(rr)
                    .global_set(g.p_msg)
                    .i32_const(pk::ERROR)
                    .global_set(g.p_kind)
                    .i32_const(1)
                    .return_()
                    .end()
                    .i32_const(0);
            }
        }
        RecGet | CtorField => {
            let aget = cg.func(FKey::H(Aget));
            let (ty, fld) = if h == RecGet {
                (t.record, 1)
            } else {
                (t.data, 2)
            };
            fb.i()
                .local_get(0)
                .ref_test_non_null(ht(ty))
                .if_(BlockType::Result(EQ))
                .local_get(0)
                .ref_cast_non_null(ht(ty))
                .struct_get(ty, fld)
                .local_get(1)
                .call(aget)
                .else_()
                .global_get(g.unit)
                .end();
        }
        RecField | RecHas => {
            let aget = cg.func(FKey::H(Aget));
            let shape = fb.local(I32);
            let fs = fb.local(rv(true, t.anyarr));
            let first = fb.local(I32);
            let p = fb.local(I32);
            shape_fields(cg, &mut fb, 0, shape, fs);
            let miss = |fb: &mut Fb| {
                if h == RecHas {
                    fb.i().i32_const(0).return_();
                } else {
                    fb.i().global_get(g.unit).return_();
                }
            };
            fb.i()
                .global_get(gimp::SHAPES)
                .local_get(shape)
                .local_get(1)
                .call(imp::SHAPE_FIND)
                .local_tee(first)
                .i32_const(SHAPE_NONE)
                .i32_eq()
                .if_(BlockType::Empty);
            miss(&mut fb);
            fb.i()
                .end()
                .local_get(first)
                .local_get(2)
                .i32_add()
                .local_tee(p)
                .global_get(gimp::SHAPES)
                .local_get(shape)
                .call(imp::SHAPE_LEN)
                .i32_ge_u()
                .if_(BlockType::Empty);
            miss(&mut fb);
            fb.i()
                .end()
                .global_get(gimp::SHAPES)
                .local_get(shape)
                .call(imp::SHAPE_LABELS)
                .local_get(p)
                .i32_const(2)
                .i32_shl()
                .i32_add()
                .i32_load(MEM32)
                .local_get(1)
                .i32_ne()
                .if_(BlockType::Empty);
            miss(&mut fb);
            fb.i().end();
            if h == RecHas {
                fb.i().i32_const(1);
            } else {
                fb.i().local_get(fs).local_get(p).call(aget);
            }
        }
        RecExtend => {
            let shape = fb.local(I32);
            let fs = fb.local(rv(true, t.anyarr));
            let ns = fb.local(I32);
            let pos = fb.local(I32);
            let n = fb.local(I32);
            let nf = fb.local(rv(true, t.anyarr));
            shape_fields(cg, &mut fb, 0, shape, fs);
            shape_op(cg, &mut fb, imp::SHAPE_EXTEND, shape, 1, ns);
            fb.i()
                .global_get(gimp::SCRATCH)
                .i32_load(MEM32)
                .local_set(pos)
                .local_get(fs)
                .array_len()
                .local_tee(n)
                .i32_const(1)
                .i32_add()
                .array_new_default(t.anyarr)
                .local_set(nf)
                .local_get(nf)
                .i32_const(0)
                .local_get(fs)
                .i32_const(0)
                .local_get(pos)
                .array_copy(t.anyarr, t.anyarr)
                .local_get(nf)
                .local_get(pos)
                .local_get(2)
                .array_set(t.anyarr)
                .local_get(nf)
                .local_get(pos)
                .i32_const(1)
                .i32_add()
                .local_get(fs)
                .local_get(pos)
                .local_get(n)
                .local_get(pos)
                .i32_sub()
                .array_copy(t.anyarr, t.anyarr)
                .local_get(ns)
                .local_get(nf)
                .ref_as_non_null()
                .struct_new(t.record);
        }
        RecRestrict => {
            let shape = fb.local(I32);
            let fs = fb.local(rv(true, t.anyarr));
            let ns = fb.local(I32);
            let pos = fb.local(I32);
            let n = fb.local(I32);
            let nf = fb.local(rv(true, t.anyarr));
            shape_fields(cg, &mut fb, 0, shape, fs);
            shape_op(cg, &mut fb, imp::SHAPE_RESTRICT, shape, 1, ns);
            fb.i()
                .local_get(fs)
                .array_len()
                .local_set(n)
                .local_get(ns)
                .i32_const(SHAPE_NONE)
                .i32_eq()
                .if_(BlockType::Empty)
                .local_get(shape)
                .local_set(ns)
                .local_get(n)
                .array_new_default(t.anyarr)
                .local_tee(nf)
                .i32_const(0)
                .local_get(fs)
                .i32_const(0)
                .local_get(n)
                .array_copy(t.anyarr, t.anyarr)
                .else_()
                .global_get(gimp::SCRATCH)
                .i32_load(MEM32)
                .local_set(pos)
                .local_get(n)
                .i32_const(1)
                .i32_sub()
                .array_new_default(t.anyarr)
                .local_tee(nf)
                .i32_const(0)
                .local_get(fs)
                .i32_const(0)
                .local_get(pos)
                .array_copy(t.anyarr, t.anyarr)
                .local_get(nf)
                .local_get(pos)
                .local_get(fs)
                .local_get(pos)
                .i32_const(1)
                .i32_add()
                .local_get(n)
                .local_get(pos)
                .i32_sub()
                .i32_const(1)
                .i32_sub()
                .array_copy(t.anyarr, t.anyarr)
                .end()
                .local_get(nf)
                .array_len()
                .i32_eqz()
                .if_(BlockType::Empty)
                .global_get(g.unit)
                .return_()
                .end()
                .local_get(ns)
                .local_get(nf)
                .ref_as_non_null()
                .struct_new(t.record);
        }
        RecUpdate => {
            let shape = fb.local(I32);
            let fs = fb.local(rv(true, t.anyarr));
            let n = fb.local(I32);
            let p = fb.local(I32);
            let nf = fb.local(rv(true, t.anyarr));
            shape_fields(cg, &mut fb, 0, shape, fs);
            fb.i()
                .local_get(fs)
                .array_len()
                .local_tee(n)
                .array_new_default(t.anyarr)
                .local_tee(nf)
                .i32_const(0)
                .local_get(fs)
                .i32_const(0)
                .local_get(n)
                .array_copy(t.anyarr, t.anyarr)
                .global_get(gimp::SHAPES)
                .local_get(shape)
                .local_get(1)
                .call(imp::SHAPE_FIND)
                .local_tee(p)
                .i32_const(SHAPE_NONE)
                .i32_ne()
                .if_(BlockType::Empty)
                .local_get(p)
                .local_get(n)
                .i32_lt_u()
                .if_(BlockType::Empty)
                .local_get(nf)
                .local_get(p)
                .local_get(2)
                .array_set(t.anyarr)
                .end()
                .end()
                .local_get(n)
                .i32_eqz()
                .if_(BlockType::Empty)
                .global_get(g.unit)
                .return_()
                .end()
                .local_get(shape)
                .local_get(nf)
                .ref_as_non_null()
                .struct_new(t.record);
        }
        ClauseFind => {
            let ops = fb.local(rv(true, t.i32arr));
            let i = fb.local(I32);
            let n = fb.local(I32);
            fb.i()
                .local_get(0)
                .struct_get(t.hr, hrf::OPS)
                .local_tee(ops)
                .array_len()
                .local_set(n)
                .local_get(2)
                .local_set(i)
                .block(BlockType::Empty)
                .loop_(BlockType::Empty)
                .local_get(i)
                .local_get(n)
                .i32_ge_u()
                .br_if(1)
                .local_get(ops)
                .local_get(i)
                .array_get(t.i32arr)
                .local_get(1)
                .i32_eq()
                .if_(BlockType::Empty)
                .local_get(i)
                .return_()
                .end()
                .local_get(i)
                .i32_const(1)
                .i32_add()
                .local_set(i)
                .br(0)
                .end()
                .end()
                .i32_const(-1);
        }
        HandlerFind => {
            let c = fb.local(EQ);
            fb.i()
                .global_get(g.chain)
                .local_set(c)
                .block(BlockType::Empty)
                .loop_(BlockType::Empty)
                .local_get(c)
                .ref_test_non_null(ht(t.hr))
                .i32_eqz()
                .br_if(1)
                .local_get(c)
                .ref_cast_non_null(ht(t.hr))
                .struct_get(t.hr, hrf::EFFECT)
                .local_get(0)
                .i32_eq()
                .if_(BlockType::Empty)
                .local_get(c)
                .return_()
                .end()
                .local_get(c)
                .ref_cast_non_null(ht(t.hr))
                .struct_get(t.hr, hrf::PARENT)
                .local_set(c)
                .br(0)
                .end()
                .end()
                .ref_null(HEQ);
        }
        PendingClass => {
            let own = fb.local(I32);
            use keleut_lir::effect::class::*;
            fb.i()
                .global_get(g.p_hr)
                .local_get(0)
                .ref_eq()
                .local_set(own);
            let case = |fb: &mut Fb, k: i32, mine: i32, other: i32| {
                fb.i()
                    .global_get(g.p_kind)
                    .i32_const(k)
                    .i32_eq()
                    .if_(BlockType::Empty)
                    .i32_const(mine)
                    .i32_const(other)
                    .local_get(own)
                    .select()
                    .return_()
                    .end();
            };
            case(&mut fb, pk::DISCARD_VALUE, OWN_VALUE, OTHER_UNWIND);
            case(&mut fb, pk::DISCARD_NEVER, OWN_NEVER, OTHER_UNWIND);
            case(&mut fb, pk::YIELD, OWN_YIELD, OTHER_YIELD);
            case(&mut fb, pk::TAIL_RESUME, TAIL_RESUME, TAIL_RESUME);
            case(&mut fb, pk::ERROR, OTHER_UNWIND, OTHER_UNWIND);
            fatal(cg, &mut fb, "ハンドラのフレームに届いた状態語が不正です");
        }
        PendingTake => {
            fb.i()
                .global_get(g.p_kind)
                .global_get(g.p_hr)
                .global_get(g.p_val)
                .global_get(g.p_clause)
                .global_get(g.p_args)
                .global_get(g.p_msg)
                .struct_new(t.saved)
                .i32_const(pk::NONE)
                .global_set(g.p_kind)
                .ref_null(HEQ)
                .global_set(g.p_hr)
                .ref_null(HEQ)
                .global_set(g.p_val)
                .ref_null(HEQ)
                .global_set(g.p_args);
        }
        PendingRestore => {
            let s = fb.local(rv(true, t.saved));
            fb.i()
                .local_get(0)
                .ref_cast_non_null(ht(t.saved))
                .local_set(s);
            for (f, gl) in [
                (svf::KIND, g.p_kind),
                (svf::HR, g.p_hr),
                (svf::VAL, g.p_val),
                (svf::CLAUSE, g.p_clause),
                (svf::ARGS, g.p_args),
                (svf::MSG, g.p_msg),
            ] {
                fb.i().local_get(s).struct_get(t.saved, f).global_set(gl);
            }
        }
        Suppress => {
            fb.i()
                .global_get(g.p_kind)
                .global_get(g.p_msg)
                .call(imp::SUPPRESS)
                .i32_const(pk::NONE)
                .global_set(g.p_kind)
                .ref_null(HEQ)
                .global_set(g.p_hr)
                .ref_null(HEQ)
                .global_set(g.p_val)
                .ref_null(HEQ)
                .global_set(g.p_args);
        }
        PerformUnwound => {
            let i32get = cg.func(FKey::H(I32Get));
            let ttr = cg.tpl.internal("末尾の resume の印が perform に届きました");
            let tnone = cg.tpl.internal("状態語の立っていない巻き戻し");
            fb.i()
                .global_get(g.p_kind)
                .i32_const(pk::MATCHED)
                .i32_eq()
                .if_(BlockType::Empty)
                .local_get(1)
                .global_set(g.chain)
                .local_get(0)
                .ref_cast_non_null(ht(t.hr))
                .struct_get(t.hr, hrf::CLASSES)
                .global_get(g.p_clause)
                .call(i32get)
                .i32_const(cls::N)
                .i32_eq()
                .if_(BlockType::Empty)
                .i32_const(pk::DISCARD_NEVER)
                .global_set(g.p_kind)
                .local_get(0)
                .global_set(g.p_hr)
                .local_get(2)
                .global_set(g.p_args)
                .return_()
                .end();
            g_unimpl(cg, &mut fb);
            fb.i()
                .end()
                .global_get(g.p_kind)
                .i32_const(pk::CLAUSE_VALUE)
                .i32_eq()
                .if_(BlockType::Empty)
                .local_get(1)
                .global_set(g.chain)
                .i32_const(pk::DISCARD_VALUE)
                .global_set(g.p_kind)
                .local_get(0)
                .global_set(g.p_hr)
                .return_()
                .end()
                .global_get(g.p_kind)
                .i32_const(pk::TAIL_RESUME)
                .i32_eq()
                .if_(BlockType::Empty)
                .i32_const(ttr as i32)
                .call(imp::FATAL)
                .end()
                .global_get(g.p_kind)
                .i32_const(pk::YIELD)
                .i32_eq()
                .if_(BlockType::Empty)
                .return_()
                .end()
                .global_get(g.p_kind)
                .i32_eqz()
                .if_(BlockType::Empty)
                .i32_const(tnone as i32)
                .call(imp::FATAL)
                .end()
                .local_get(1)
                .global_set(g.chain);
        }
        IsExtern => {
            let fid = fb.local(I32);
            fb.i()
                .local_get(0)
                .ref_test_non_null(ht(t.externu))
                .if_(BlockType::Empty)
                .i32_const(1)
                .return_()
                .end()
                .local_get(0)
                .ref_test_non_null(ht(t.clo))
                .i32_eqz()
                .if_(BlockType::Empty)
                .i32_const(0)
                .return_()
                .end()
                .local_get(0)
                .ref_cast_non_null(ht(t.clo))
                .struct_get(t.clo, 0)
                .local_set(fid);
            for &e in &cg.extern_funcs.clone() {
                fb.i()
                    .local_get(fid)
                    .i32_const(e as i32)
                    .i32_eq()
                    .if_(BlockType::Empty)
                    .i32_const(1)
                    .return_()
                    .end();
            }
            fb.i().i32_const(0);
        }
        I32Div | I32Rem => {
            let mut ins = fb.i();
            ins.local_get(1)
                .i32_eqz()
                .if_(BlockType::Empty)
                .i32_const(0)
                .return_()
                .end()
                .local_get(1)
                .i32_const(-1)
                .i32_eq()
                .if_(BlockType::Empty);
            if h == I32Div {
                ins.i32_const(0).local_get(0).i32_sub();
            } else {
                ins.i32_const(0);
            }
            ins.return_().end().local_get(0).local_get(1);
            if h == I32Div {
                ins.i32_div_s();
            } else {
                ins.i32_rem_s();
            }
        }
        I64Div | I64Rem => {
            let mut ins = fb.i();
            ins.local_get(1)
                .i64_eqz()
                .if_(BlockType::Empty)
                .i64_const(0)
                .return_()
                .end()
                .local_get(1)
                .i64_const(-1)
                .i64_eq()
                .if_(BlockType::Empty);
            if h == I64Div {
                ins.i64_const(0).local_get(0).i64_sub();
            } else {
                ins.i64_const(0);
            }
            ins.return_().end().local_get(0).local_get(1);
            if h == I64Div {
                ins.i64_div_s();
            } else {
                ins.i64_rem_s();
            }
        }
        Main => gen_main(cg, &mut fb),
        VKind => {
            let cases = [
                (t.box_i32, vk::BOX_I32),
                (t.box_i64, vk::I64),
                (t.box_f64, vk::F64),
                (t.str_, vk::STR),
                (t.unit, vk::UNIT),
                (t.bool_, vk::BOOL),
                (t.data, vk::DATA),
                (t.variant, vk::VARIANT),
                (t.record, vk::RECORD),
                (t.clo, vk::CLOSURE),
                (t.method, vk::METHOD),
                (t.externu, vk::EXTERNU),
                (t.ref_, vk::REF),
                (t.immarr, vk::ARRAY),
                (t.mutarr, vk::MARRAY),
                (t.dictb, vk::DICT),
                (t.dicti, vk::DICT),
                (t.dictr, vk::DICT),
                (t.dictv, vk::DICT),
            ];
            for (ty, code) in cases {
                fb.i()
                    .local_get(0)
                    .ref_test_non_null(ht(ty))
                    .if_(BlockType::Empty)
                    .i32_const(code)
                    .return_()
                    .end();
            }
            fb.i()
                .local_get(0)
                .ref_test_non_null(HI31)
                .if_(BlockType::Empty)
                .i32_const(vk::I31)
                .return_()
                .end()
                .i32_const(vk::OTHER);
        }
        VA | VB => {
            let cases: Vec<(u32, u32)> = if h == VA {
                vec![
                    (t.box_i32, 0),
                    (t.bool_, 0),
                    (t.data, 0),
                    (t.variant, 0),
                    (t.record, 0),
                    (t.clo, 0),
                    (t.method, 0),
                    (t.externu, 0),
                ]
            } else {
                vec![(t.data, 1), (t.method, 1)]
            };
            for (ty, f) in cases {
                fb.i()
                    .local_get(0)
                    .ref_test_non_null(ht(ty))
                    .if_(BlockType::Empty)
                    .local_get(0)
                    .ref_cast_non_null(ht(ty))
                    .struct_get(ty, f)
                    .return_()
                    .end();
            }
            fb.i().i32_const(0);
        }
        VI64 => {
            fb.i()
                .local_get(0)
                .ref_test_non_null(ht(t.box_i64))
                .if_(BlockType::Empty)
                .local_get(0)
                .ref_cast_non_null(ht(t.box_i64))
                .struct_get(t.box_i64, 0)
                .return_()
                .end()
                .local_get(0)
                .ref_test_non_null(ht(t.box_f64))
                .if_(BlockType::Empty)
                .local_get(0)
                .ref_cast_non_null(ht(t.box_f64))
                .struct_get(t.box_f64, 0)
                .i64_reinterpret_f64()
                .return_()
                .end()
                .i64_const(0);
        }
        VN => {
            for (ty, f) in [(t.data, 2u32), (t.record, 1)] {
                fb.i()
                    .local_get(0)
                    .ref_test_non_null(ht(ty))
                    .if_(BlockType::Empty)
                    .local_get(0)
                    .ref_cast_non_null(ht(ty))
                    .struct_get(ty, f)
                    .array_len()
                    .return_()
                    .end();
            }
            for ty in [t.anyarr, t.str_] {
                fb.i()
                    .local_get(0)
                    .ref_test_non_null(ht(ty))
                    .if_(BlockType::Empty)
                    .local_get(0)
                    .ref_cast_non_null(ht(ty))
                    .array_len()
                    .return_()
                    .end();
            }
            fb.i().i32_const(0);
        }
        VGet => {
            let aget = cg.func(FKey::H(Aget));
            for (ty, f) in [(t.data, 2u32), (t.record, 1)] {
                fb.i()
                    .local_get(0)
                    .ref_test_non_null(ht(ty))
                    .if_(BlockType::Empty)
                    .local_get(0)
                    .ref_cast_non_null(ht(ty))
                    .struct_get(ty, f)
                    .local_get(1)
                    .call(aget)
                    .return_()
                    .end();
            }
            fb.i()
                .local_get(0)
                .ref_test_non_null(ht(t.variant))
                .if_(BlockType::Empty)
                .local_get(0)
                .ref_cast_non_null(ht(t.variant))
                .struct_get(t.variant, 1)
                .return_()
                .end()
                .local_get(0)
                .ref_test_non_null(ht(t.anyarr))
                .if_(BlockType::Empty)
                .local_get(0)
                .ref_cast_non_null(ht(t.anyarr))
                .local_get(1)
                .call(aget)
                .return_()
                .end()
                .global_get(g.unit);
        }
    }
    fb.finish()
}

/// 関数 `f` の、捕獲を持たないクロージャを積む。
pub fn closure0(cg: &mut Cg, fb: &mut Fb, f: FuncId) {
    let n = cg.arity(f);
    let (_, clo) = cg.m.fn_types(n);
    let idx = cg.lir(f);
    cg.m.declare_ref(idx);
    let empty = cg.g.empty;
    fb.i()
        .i32_const(f.0 as i32)
        .global_get(empty)
        .ref_func(idx)
        .struct_new(clo);
}

fn gen_main(cg: &mut Cg, fb: &mut Fb) {
    let g = cg.g;
    let t = cg.m.t;
    let prog = cg.prog;
    let r = fb.local(I64);
    fb.i()
        .local_get(0)
        .global_set(g.lmax)
        .local_get(0)
        .global_set(g.limit)
        .local_get(0)
        .i64_const(6)
        .i64_shr_u()
        .local_set(r)
        .local_get(0)
        .local_get(r)
        .i64_const(1024)
        .local_get(r)
        .i64_const(1024)
        .i64_gt_u()
        .select()
        .i64_add()
        .global_set(g.lr)
        .i64_const(0)
        .global_set(g.depth)
        .global_get(g.unit)
        .global_set(g.chain);
    let snd = cg.func(FKey::H(H::StrNewData));
    let mut strs = cg.strs.clone();
    let off_t = cg.m.add_data(b"true");
    let off_f = cg.m.add_data(b"false");
    strs.push((g.str_true, off_t, 4));
    strs.push((g.str_false, off_f, 5));
    for (gi, off, len) in strs {
        fb.i()
            .i32_const(off as i32)
            .i32_const(len as i32)
            .call(snd)
            .global_set(gi);
    }
    for (i, gl) in prog.globals.iter().enumerate() {
        let (v, init) = cg.pglobals[i];
        use keleut_lir::GlobalKind::*;
        match &gl.kind {
            Builtin { func } => {
                closure0(cg, fb, *func);
                fb.i().global_set(v).i32_const(1).global_set(init);
            }
            Extern { func: Some(f), .. } | Fn(f) | Dictabs(f) => {
                closure0(cg, fb, *f);
                fb.i().global_set(v);
            }
            Extern { func: None, .. } => {
                fb.i()
                    .i32_const(i as i32)
                    .struct_new(t.externu)
                    .global_set(v);
            }
            Value(_) => {
                fb.i().global_get(g.unit).global_set(v);
            }
        }
    }
    for &f in &prog.init {
        let idx = cg.lir(f);
        let n = cg.arity(f);
        fb.i().ref_null(ht(t.clo));
        for _ in 0..n {
            fb.i().global_get(g.unit);
        }
        cg.pack_args(fb, n);
        fb.i()
            .ref_null(ht(t.frame))
            .call(idx)
            .ref_is_null()
            .if_(BlockType::Empty)
            .i32_const(1)
            .return_()
            .end();
    }
    fb.i().i32_const(0);
}

/// 内部の誤り(70)で止まる。
fn bail(cg: &mut Cg, fb: &mut Fb, text: &str) {
    fatal(cg, fb, text);
}

fn gen_callv(cg: &mut Cg, n: u32) -> Function {
    let t = cg.m.t;
    let (fty, cty) = cg.m.fn_types(n);
    // 引数は呼び出し先の型と同じ形(まとめた形を含む)で受け取り、そのまま渡す。
    let w = crate::wire_arity(n);
    let mut fb = Fb::new(1 + w);
    let c = fb.local(rv(true, cty));
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(cty))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(cty))
        .local_tee(c);
    for k in 0..w {
        fb.i().local_get(1 + k);
    }
    fb.i()
        .ref_null(ht(t.frame))
        .local_get(c)
        .struct_get(cty, 2)
        .return_call_ref(fty)
        .end();
    // メソッドの値。
    let mv = fb.local(rv(true, t.method));
    let aget = cg.func(FKey::H(H::Aget));
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.method))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(t.method))
        .local_set(mv);
    for (c_, m_, no) in cg.method_vals.clone() {
        let callm = cg.func(FKey::CallM(c_, m_, no, n));
        fb.i()
            .local_get(mv)
            .struct_get(t.method, 0)
            .i32_const(c_ as i32)
            .i32_eq()
            .local_get(mv)
            .struct_get(t.method, 1)
            .i32_const(m_ as i32)
            .i32_eq()
            .i32_and()
            .local_get(mv)
            .struct_get(t.method, 3)
            .array_len()
            .i32_const(no as i32)
            .i32_eq()
            .i32_and()
            .if_(BlockType::Empty)
            .local_get(mv)
            .struct_get(t.method, 2);
        for k in 0..no {
            fb.i()
                .local_get(mv)
                .struct_get(t.method, 3)
                .i32_const(k as i32)
                .call(aget);
        }
        for k in 0..w {
            fb.i().local_get(1 + k);
        }
        fb.i().return_call(callm).end();
    }
    bail(cg, &mut fb, "メソッドの値の種類が翻訳の表にありません");
    fb.i().end();
    // 実装の無い extern。
    let raise = cg.func(FKey::H(H::Raise));
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.externu))
        .if_(BlockType::Empty);
    for g in cg.extern_unimpl.clone() {
        let tp = crate::meta::unimpl_prim(&mut cg.tpl, cg.prog, g as usize);
        fb.i()
            .local_get(0)
            .ref_cast_non_null(ht(t.externu))
            .struct_get(t.externu, 0)
            .i32_const(g as i32)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(tp as i32)
            .ref_null(HEQ)
            .i64_const(0)
            .call(raise)
            .ref_null(HEQ)
            .return_()
            .end();
    }
    bail(cg, &mut fb, "実装の無い extern の大域が表にありません");
    fb.i().end();
    bail(cg, &mut fb, "呼べない値を呼びました");
    fb.finish()
}

fn gen_callm(cg: &mut Cg, c: u32, m: u32, no: u32, n: u32) -> Function {
    let t = cg.m.t;
    let g = cg.g;
    let prog = cg.prog;
    let w = crate::wire_arity(n);
    let mut fb = Fb::new(1 + no + w);
    let arg0 = 1 + no;
    let args = |fb: &mut Fb| {
        for k in 0..w {
            fb.i().local_get(arg0 + k);
        }
    };
    // 組み込みのインスタンス。
    let db = fb.local(rv(true, t.dictb));
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.dictb))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(t.dictb))
        .local_set(db);
    for bm in &prog.builtin_methods {
        if bm.method != m || cg.arity(bm.func) != n {
            continue;
        }
        let body = cg.lir(bm.func);
        fb.i()
            .local_get(db)
            .struct_get(t.dictb, 0)
            .i32_const(bm.class.0 as i32)
            .i32_eq()
            .local_get(db)
            .struct_get(t.dictb, 1)
            .i32_const(bm.ty.0 as i32)
            .i32_eq()
            .i32_and()
            .if_(BlockType::Empty)
            .ref_null(ht(t.clo));
        args(&mut fb);
        fb.i().ref_null(ht(t.frame)).return_call(body).end();
    }
    bail(cg, &mut fb, "組み込みのメソッドがありません");
    fb.i().end();
    // 利用者のインスタンス。
    let di = fb.local(rv(true, t.dicti));
    let tf = fb.local(EQ);
    let raise = cg.func(FKey::H(H::Raise));
    let aget = cg.func(FKey::H(H::Aget));
    let callv = cg.func(FKey::CallV(n));
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.dicti))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(t.dicti))
        .local_set(di);
    for (ii, inst) in prog.instances.iter().enumerate() {
        if inst.class.0 != c || m as usize >= inst.methods.len() {
            continue;
        }
        let tnr = crate::meta::inst_not_ready(&mut cg.tpl, prog, ii);
        let init = cg.inst_init[ii];
        fb.i()
            .local_get(di)
            .struct_get(t.dicti, 0)
            .i32_const(ii as i32)
            .i32_eq()
            .if_(BlockType::Empty)
            .global_get(init)
            .i32_eqz()
            .if_(BlockType::Empty)
            .i32_const(tnr as i32)
            .ref_null(HEQ)
            .i64_const(0)
            .call(raise)
            .ref_null(HEQ)
            .return_()
            .end();
        let im = &inst.methods[m as usize];
        if im.dicts == 0 {
            fb.i().global_get(cg.inst_vals[ii][m as usize]);
            args(&mut fb);
            fb.i().return_call(callv);
        } else if cg.arity(im.func) != inst.premises + no {
            bail(cg, &mut fb, "実装の関数の辞書の数が合いません");
        } else {
            let imp_f = cg.lir(im.func);
            fb.i().ref_null(ht(t.clo));
            for p in 0..inst.premises {
                fb.i()
                    .local_get(di)
                    .struct_get(t.dicti, 1)
                    .i32_const(p as i32)
                    .call(aget);
            }
            for k in 0..no {
                fb.i().local_get(1 + k);
            }
            cg.pack_args(&mut fb, inst.premises + no);
            fb.i()
                .ref_null(ht(t.frame))
                .call(imp_f)
                .local_tee(tf)
                .ref_is_null()
                .if_(BlockType::Empty)
                .ref_null(HEQ)
                .return_()
                .end()
                .local_get(tf);
            args(&mut fb);
            fb.i().return_call(callv);
        }
        fb.i().end();
    }
    bail(cg, &mut fb, "インスタンスが翻訳の表にありません");
    fb.i().end();
    // 構造的な Eq。
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.dictr))
        .local_get(0)
        .ref_test_non_null(ht(t.dictv))
        .i32_or()
        .if_(BlockType::Empty);
    if n == 2 {
        let eqs = cg.func(FKey::EqStruct(c));
        let rr = fb.local(I32);
        fb.i()
            .local_get(0)
            .local_get(arg0)
            .local_get(arg0 + 1)
            .call(eqs)
            .local_tee(rr)
            .i32_const(0)
            .i32_lt_s()
            .if_(BlockType::Empty)
            .ref_null(HEQ)
            .return_()
            .end()
            .global_get(g.true_)
            .global_get(g.false_)
            .local_get(rr)
            .typed_select(EQ)
            .return_();
    } else {
        bail(cg, &mut fb, "構造的な Eq の引数の数が 2 でありません");
    }
    fb.i().end();
    bail(cg, &mut fb, "辞書でない値からメソッドを選びました");
    fb.finish()
}

fn gen_dictapp(cg: &mut Cg, n: u32) -> Function {
    let t = cg.m.t;
    let (fty, cty) = cg.m.fn_types(n);
    let is_ext = cg.func(FKey::H(H::IsExtern));
    let w = crate::wire_arity(n);
    let mut fb = Fb::new(1 + w);
    let c = fb.local(rv(true, cty));
    fb.i()
        .local_get(0)
        .call(is_ext)
        .if_(BlockType::Empty)
        .local_get(0)
        .return_()
        .end()
        .local_get(0)
        .ref_test_non_null(ht(cty))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(cty))
        .local_tee(c);
    for k in 0..w {
        fb.i().local_get(1 + k);
    }
    fb.i()
        .ref_null(ht(t.frame))
        .local_get(c)
        .struct_get(cty, 2)
        .return_call_ref(fty)
        .end();
    bail(cg, &mut fb, "辞書を受け取らない値に辞書を渡しました");
    fb.finish()
}

fn gen_tail_never(cg: &mut Cg) -> Function {
    let t = cg.m.t;
    let g = cg.g;
    let aget = cg.func(FKey::H(H::Aget));
    let mut fb = Fb::new(1);
    let s = fb.local(rv(true, t.saved));
    let h = fb.local(rv(true, t.hr));
    let clo = fb.local(EQ);
    let args = fb.local(rv(true, t.anyarr));
    let n = fb.local(I32);
    fb.i()
        .local_get(0)
        .ref_cast_non_null(ht(t.saved))
        .local_tee(s)
        .struct_get(t.saved, svf::HR)
        .ref_cast_non_null(ht(t.hr))
        .local_tee(h)
        .struct_get(t.hr, hrf::CLAUSES)
        .local_get(s)
        .struct_get(t.saved, svf::CLAUSE)
        .call(aget)
        .local_set(clo)
        .local_get(s)
        .struct_get(t.saved, svf::ARGS)
        .ref_cast_non_null(ht(t.anyarr))
        .local_tee(args)
        .array_len()
        .local_set(n);
    for a in cg.clause_arities.clone() {
        if a < 2 {
            continue;
        }
        let (fty, cty) = cg.m.fn_types(a);
        let c = fb.local(rv(true, cty));
        fb.i()
            .local_get(n)
            .i32_const(a as i32 - 2)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(clo)
            .ref_cast_non_null(ht(cty))
            .local_tee(c);
        for k in 0..a - 2 {
            fb.i().local_get(args).i32_const(k as i32).call(aget);
        }
        fb.i().global_get(g.unit).i32_const(-1).ref_i31();
        cg.pack_args(&mut fb, a);
        fb.i()
            .ref_null(ht(t.frame))
            .local_get(c)
            .struct_get(cty, 2)
            .return_call_ref(fty)
            .end();
    }
    bail(cg, &mut fb, "N の節の引数の数が翻訳の表にありません");
    fb.finish()
}

fn gen_eq_struct(cg: &mut Cg, c: u32) -> Function {
    let t = cg.m.t;
    let g = cg.g;
    let i32get = cg.func(FKey::H(H::I32Get));
    let aget = cg.func(FKey::H(H::Aget));
    let recf = cg.func(FKey::H(H::RecField));
    let depth_err = cg.func(FKey::H(H::RaiseDepth));
    let callm = cg.func(FKey::CallM(c, 0, 0, 2));
    let mut fb = Fb::new(3);
    let labels = fb.local(rv(true, t.i32arr));
    let dicts = fb.local(rv(true, t.anyarr));
    let i = fb.local(I32);
    let j = fb.local(I32);
    let k = fb.local(I32);
    let l = fb.local(I32);
    let n = fb.local(I32);
    let xa = fb.local(EQ);
    let xb = fb.local(EQ);
    let sd = fb.local(I64);
    let rv_ = fb.local(EQ);
    // 項目の辞書 `d`(積んだ形で)で xa と xb を比べ、偽なら 0、番兵なら -1 を返す。
    let eq_item = |fb: &mut Fb| {
        fb.i()
            .local_set(rv_)
            .global_get(g.depth)
            .i64_const(1)
            .i64_add()
            .global_get(g.limit)
            .i64_gt_u()
            .if_(BlockType::Empty)
            .call(depth_err)
            .i32_const(-1)
            .return_()
            .end()
            .global_get(g.depth)
            .local_tee(sd)
            .i64_const(1)
            .i64_add()
            .global_set(g.depth)
            .local_get(rv_)
            .local_get(xa)
            .local_get(xb)
            .call(callm)
            .local_set(rv_)
            .local_get(sd)
            .global_set(g.depth)
            .local_get(rv_)
            .ref_is_null()
            .if_(BlockType::Empty)
            .i32_const(-1)
            .return_()
            .end()
            .local_get(rv_)
            .global_get(g.true_)
            .ref_eq()
            .i32_eqz()
            .if_(BlockType::Empty)
            .i32_const(0)
            .return_()
            .end();
    };
    // レコード。
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.dictr))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(t.dictr))
        .struct_get(t.dictr, 0)
        .local_set(labels)
        .local_get(0)
        .ref_cast_non_null(ht(t.dictr))
        .struct_get(t.dictr, 1)
        .local_set(dicts)
        .local_get(labels)
        .array_len()
        .local_set(n);
    for_range(&mut fb, i, n, |fb| {
        fb.i()
            .local_get(labels)
            .local_get(i)
            .call(i32get)
            .local_set(l)
            .i32_const(0)
            .local_set(k);
        for_range(fb, j, i, |fb| {
            fb.i()
                .local_get(labels)
                .local_get(j)
                .call(i32get)
                .local_get(l)
                .i32_eq()
                .local_get(k)
                .i32_add()
                .local_set(k);
        });
        fb.i()
            .local_get(1)
            .local_get(l)
            .local_get(k)
            .call(recf)
            .local_set(xa)
            .local_get(2)
            .local_get(l)
            .local_get(k)
            .call(recf)
            .local_set(xb)
            .local_get(dicts)
            .local_get(i)
            .call(aget);
        eq_item(fb);
    });
    fb.i().i32_const(1).return_().end();
    // ヴァリアント。
    fb.i()
        .local_get(0)
        .ref_test_non_null(ht(t.dictv))
        .if_(BlockType::Empty)
        .local_get(0)
        .ref_cast_non_null(ht(t.dictv))
        .struct_get(t.dictv, 0)
        .local_set(labels)
        .local_get(0)
        .ref_cast_non_null(ht(t.dictv))
        .struct_get(t.dictv, 1)
        .local_set(dicts)
        .local_get(1)
        .ref_cast_non_null(ht(t.variant))
        .struct_get(t.variant, 0)
        .local_tee(l)
        .local_get(2)
        .ref_cast_non_null(ht(t.variant))
        .struct_get(t.variant, 0)
        .i32_ne()
        .if_(BlockType::Empty)
        .i32_const(0)
        .return_()
        .end()
        .local_get(labels)
        .array_len()
        .local_set(n);
    for_range(&mut fb, i, n, |fb| {
        fb.i()
            .local_get(labels)
            .local_get(i)
            .call(i32get)
            .local_get(l)
            .i32_eq()
            .if_(BlockType::Empty)
            .local_get(1)
            .ref_cast_non_null(ht(t.variant))
            .struct_get(t.variant, 1)
            .local_set(xa)
            .local_get(2)
            .ref_cast_non_null(ht(t.variant))
            .struct_get(t.variant, 1)
            .local_set(xb)
            .local_get(dicts)
            .local_get(i)
            .call(aget);
        eq_item(fb);
        fb.i().i32_const(1).return_().end();
    });
    fb.i().i32_const(0).return_().end();
    bail(cg, &mut fb, "構造的な辞書ではありません");
    fb.finish()
}
