//! LIR の型。
//!
//! LIR は関数ごとの基本ブロックのグラフである(計画 261024-1 の 6.4 節)。
//! 各命令の意味は `rustkeleut/doc/lir.md` に定める。
//! ここでは型と、命令の被演算子をたどる補助だけを置く。
//!
//! - 仮想レジスタ [`Reg`] は関数ごとに番号を振り、それぞれがちょうど 1 つの [`Rep`] を持つ。
//!   SSA ではない。
//!   合流する値は、各前任のブロックが同じレジスタに書く。
//! - 関数の制御は非巡回である。
//!   ブロックは位相の順に並び、枝はどれも番号の大きいブロックへ向かう。
//! - 関数の境目は一様で、引数も戻り値も `any` である(D503)。

pub use crate::live::{LiveWork, Point, Points};
pub use kir::{Abi, ClassId, ClauseClass, FnKind, GlobalId, InstanceId, Rep, TypeId};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);

        impl $name {
            /// 添字を `usize` で返す。
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(
    /// 関数の中の仮想レジスタ。
    Reg
);
id_type!(
    /// 関数の中の基本ブロック。
    BlockId
);
id_type!(
    /// LIR の関数の表の添字。KIR の関数は同じ添字に置く。
    FuncId
);
id_type!(
    /// ラベルの表 [`Program::labels`] の添字。
    LabelId
);
id_type!(
    /// レコードの形の表 [`Program::shapes`] の添字。
    ShapeId
);
id_type!(
    /// 文字列の定数の表 [`Program::strings`] の添字。
    StrId
);

/// 定数。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Const {
    /// Unit(空のレコード)。
    Unit,
    /// 真偽値。
    Bool(bool),
    /// `Int32` の値。
    I32(i32),
    /// `Int64` の値。
    I64(i64),
    /// `Float64` の値のビット列。
    F64(u64),
    /// 文字列の定数。
    Str(StrId),
    /// 引数の無いコンストラクタ。
    Ctor0 {
        /// 型。
        ty: TypeId,
        /// コンストラクタ(型の中の位置)。
        ctor: u32,
    },
    /// 組み込みのインスタンスの辞書。
    DictBuiltin {
        /// クラス。
        class: ClassId,
        /// 型。
        ty: TypeId,
    },
    /// 前提を持たない利用者のインスタンスの辞書。
    DictInst0(InstanceId),
}

impl Const {
    /// 定数の Rep。
    pub fn rep(&self) -> Rep {
        match self {
            Const::Bool(_) => Rep::Bool,
            Const::I32(_) => Rep::I32,
            Const::I64(_) => Rep::I64,
            Const::F64(_) => Rep::F64,
            Const::Str(_) => Rep::Str,
            Const::Unit | Const::Ctor0 { .. } | Const::DictBuiltin { .. } | Const::DictInst0(_) => {
                Rep::Any
            }
        }
    }
}

/// 整数の幅。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntTy {
    /// 32 ビット。
    I32,
    /// 64 ビット。
    I64,
}

impl IntTy {
    /// 整数の Rep。
    pub fn rep(self) -> Rep {
        match self {
            IntTy::I32 => Rep::I32,
            IntTy::I64 => Rep::I64,
        }
    }

    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
        }
    }
}

/// 整数の二項演算。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntOp {
    /// 折り返す加算。
    Add,
    /// 折り返す減算。
    Sub,
    /// 折り返す乗算。
    Mul,
    /// 0 へ切り捨てる除算(全域。`b = 0` なら 0、`MIN / -1` は `MIN`)。
    Div,
    /// 除算の剰余(全域。`b = 0` なら 0、`MIN % -1` は 0)。
    Rem,
}

impl IntOp {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            IntOp::Add => "add",
            IntOp::Sub => "sub",
            IntOp::Mul => "mul",
            IntOp::Div => "div",
            IntOp::Rem => "rem",
        }
    }
}

/// 浮動小数点数の二項演算(IEEE 754 の binary64)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FloatOp {
    /// 加算。
    Add,
    /// 減算。
    Sub,
    /// 乗算。
    Mul,
    /// 除算。
    Div,
}

impl FloatOp {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            FloatOp::Add => "add",
            FloatOp::Sub => "sub",
            FloatOp::Mul => "mul",
            FloatOp::Div => "div",
        }
    }
}

/// 比較。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CmpOp {
    /// 等しい。
    Eq,
    /// より小さい。
    Lt,
    /// 以下。
    Le,
    /// より大きい。
    Gt,
    /// 以上。
    Ge,
}

impl CmpOp {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            CmpOp::Eq => "eq",
            CmpOp::Lt => "lt",
            CmpOp::Le => "le",
            CmpOp::Gt => "gt",
            CmpOp::Ge => "ge",
        }
    }
}

/// `extern "C"` の数学の関数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MathFn {
    /// rtcore の `krt_sin`。
    Sin,
    /// rtcore の `krt_cos`。
    Cos,
    /// IEEE 754 の平方根。
    Sqrt,
    /// rtcore の `krt_exp`。
    Exp,
    /// rtcore の `krt_log`。
    Log,
}

impl MathFn {
    /// 印字の綴り(`extern "C"` の実装名と同じ)。
    pub fn name(self) -> &'static str {
        match self {
            MathFn::Sin => "sin",
            MathFn::Cos => "cos",
            MathFn::Sqrt => "sqrt",
            MathFn::Exp => "exp",
            MathFn::Log => "log",
        }
    }
}

/// 数の変換。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConvOp {
    /// 符号を拡張する。
    I32ToI64,
    /// 正確に変換する。
    I32ToF64,
    /// 下位 32 ビットを取る。
    I64ToI32,
    /// 最も近い値へ丸める(等距離なら偶数)。
    I64ToF64,
    /// 0 へ切り捨てる(全域。NaN は 0、範囲の外は飽和)。
    F64ToI32,
    /// 0 へ切り捨てる(全域。NaN は 0、範囲の外は飽和)。
    F64ToI64,
}

impl ConvOp {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            ConvOp::I32ToI64 => "i32_to_i64",
            ConvOp::I32ToF64 => "i32_to_f64",
            ConvOp::I64ToI32 => "i64_to_i32",
            ConvOp::I64ToF64 => "i64_to_f64",
            ConvOp::F64ToI32 => "f64_to_i32",
            ConvOp::F64ToI64 => "f64_to_i64",
        }
    }

    /// (引数の Rep, 結果の Rep)。
    pub fn reps(self) -> (Rep, Rep) {
        match self {
            ConvOp::I32ToI64 => (Rep::I32, Rep::I64),
            ConvOp::I32ToF64 => (Rep::I32, Rep::F64),
            ConvOp::I64ToI32 => (Rep::I64, Rep::I32),
            ConvOp::I64ToF64 => (Rep::I64, Rep::F64),
            ConvOp::F64ToI32 => (Rep::F64, Rep::I32),
            ConvOp::F64ToI64 => (Rep::F64, Rep::I64),
        }
    }

    /// 対応するプリミティブの実装名。
    pub fn prim_name(self) -> &'static str {
        match self {
            ConvOp::I32ToI64 => "__i32_to_i64",
            ConvOp::I32ToF64 => "__i32_to_f64",
            ConvOp::I64ToI32 => "__i64_to_i32",
            ConvOp::I64ToF64 => "__i64_to_f64",
            ConvOp::F64ToI32 => "__f64_to_i32",
            ConvOp::F64ToI64 => "__f64_to_i64",
        }
    }
}

/// 値を文字列にする種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShowOp {
    /// `Int32` の 10 進。
    I32,
    /// `Int64` の 10 進。
    I64,
    /// LangSpec §16.2 の `Float64` の綴り。
    F64,
    /// `true` か `false`。
    Bool,
}

impl ShowOp {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            ShowOp::I32 => "i32",
            ShowOp::I64 => "i64",
            ShowOp::F64 => "f64",
            ShowOp::Bool => "bool",
        }
    }

    /// 引数の Rep。
    pub fn rep(self) -> Rep {
        match self {
            ShowOp::I32 => Rep::I32,
            ShowOp::I64 => Rep::I64,
            ShowOp::F64 => Rep::F64,
            ShowOp::Bool => Rep::Bool,
        }
    }
}

/// 命令(ブロックの終わり以外)。
///
/// 被演算子の Rep と結果の Rep は [`crate::verify`] が確かめる。
/// 意味と、trap しない条件は `doc/lir.md` の各節にある。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Inst {
    /// 定数を置く。
    Const { dst: Reg, value: Const },
    /// 同じ Rep のレジスタへ写す。
    Move { dst: Reg, src: Reg },
    /// 生の値を一様な語にする。
    Box { dst: Reg, src: Reg },
    /// 一様な語から生の値を取り出す。
    Unbox { dst: Reg, src: Reg },
    /// 自分の環境(捕獲)の `index` 番目を読む。入口の `depth_enter` の直後にだけ置く。
    EnvGet { dst: Reg, index: u32 },
    /// 入口で、文脈の深さを自分のフレームの深さとして書く。
    DepthEnter,
    /// 呼ばれた側の深さ(文脈の深さ + 1)を限度と比べる。
    DepthCheck,
    /// `DepthCheck` を行い、文脈の深さを 1 増やす。
    DepthPush,
    /// 文脈の深さを自分のフレームの深さに書き戻す。
    DepthRestore,
    /// 整数の二項演算。
    IntArith {
        ty: IntTy,
        op: IntOp,
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// 折り返す符号反転。
    IntNeg { ty: IntTy, dst: Reg, a: Reg },
    /// 符号付きの整数の比較。
    IntCmp {
        ty: IntTy,
        op: CmpOp,
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// 浮動小数点数の二項演算。
    FloatArith {
        op: FloatOp,
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// 符号反転(符号のビットを反転する)。
    FloatNeg { dst: Reg, a: Reg },
    /// IEEE 754 の比較(NaN を含めばどれも偽)。
    FloatCmp { op: CmpOp, dst: Reg, a: Reg, b: Reg },
    /// 数学の関数。
    FloatMath { f: MathFn, dst: Reg, a: Reg },
    /// 0 へ切り捨てる。
    FloatTrunc { dst: Reg, a: Reg },
    /// NaN かどうか。
    FloatIsNan { dst: Reg, a: Reg },
    /// 数の変換。
    Conv { op: ConvOp, dst: Reg, a: Reg },
    /// 否定。
    BoolNot { dst: Reg, a: Reg },
    /// 真偽値が等しいか。
    BoolEq { dst: Reg, a: Reg, b: Reg },
    /// 文字列の連結。
    StrConcat { dst: Reg, a: Reg, b: Reg },
    /// 符号なしバイトの辞書順の比較。
    StrCmp { op: CmpOp, dst: Reg, a: Reg, b: Reg },
    /// バイトの数。
    StrLen { dst: Reg, a: Reg },
    /// 部分の文字列(全域。範囲の外は切り詰める)。
    StrSub {
        dst: Reg,
        s: Reg,
        pos: Reg,
        len: Reg,
    },
    /// 値を文字列にする。
    Show { op: ShowOp, dst: Reg, a: Reg },
    /// 名目的な型の値を作る。`fields` はフィールドの位置の順。
    CtorNew {
        dst: Reg,
        ty: TypeId,
        ctor: u32,
        fields: Vec<Reg>,
    },
    /// コンストラクタの番号を読む。
    CtorTag { dst: Reg, v: Reg },
    /// フィールドを読む。
    CtorField { dst: Reg, v: Reg, index: u32 },
    /// ヴァリアントを作る。
    VariantNew {
        dst: Reg,
        label: LabelId,
        payload: Reg,
    },
    /// ヴァリアントのラベルの番号を読む。
    VariantTag { dst: Reg, v: Reg },
    /// ヴァリアントの積載値を読む。
    VariantPayload { dst: Reg, v: Reg },
    /// 形の決まったレコードを作る。`fields` は形の配置の順。
    RecNew {
        dst: Reg,
        shape: ShapeId,
        fields: Vec<Reg>,
    },
    /// 形の決まったレコードの `index` 番目のフィールドを読む。
    RecGet { dst: Reg, rec: Reg, index: u32 },
    /// レコードの最も外側にフィールドを足す。
    RecExtend {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        value: Reg,
    },
    /// 外側から `nth` 番目(0 から数える)の `label` のフィールドを読む(全域)。
    RecField {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        nth: u32,
    },
    /// 外側から `nth` 番目の `label` のフィールドがあるか。
    RecHas {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        nth: u32,
    },
    /// 最も外側の `label` のフィールドを除く。
    RecRestrict { dst: Reg, rec: Reg, label: LabelId },
    /// 最も外側の `label` のフィールドを置き換える。
    RecUpdate {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        value: Reg,
    },
    /// クロージャ(種類 `dictabs` の関数なら辞書の抽象)を作る。
    ClosureNew {
        dst: Reg,
        func: FuncId,
        caps: Vec<Reg>,
    },
    /// 作ったばかりのクロージャの捕獲を埋める(`letrec`)。
    ClosureSetCap { clo: Reg, index: u32, value: Reg },
    /// メソッドの値を作る。
    MethodNew {
        dst: Reg,
        class: ClassId,
        method: u32,
        dict: Reg,
        own: Vec<Reg>,
    },
    /// 利用者のインスタンスの辞書を作る。
    DictInst {
        dst: Reg,
        inst: InstanceId,
        premises: Vec<Reg>,
    },
    /// 構造的な `Eq` のレコードの辞書を作る。
    DictRecord {
        dst: Reg,
        fields: Vec<(LabelId, Reg)>,
    },
    /// 構造的な `Eq` のヴァリアントの辞書を作る。
    DictVariant {
        dst: Reg,
        fields: Vec<(LabelId, Reg)>,
    },
    /// 大域の値を読む(初期化の検査をしない)。
    GlobalGet { dst: Reg, global: GlobalId },
    /// 大域が初期化済みか。
    GlobalIsInit { dst: Reg, global: GlobalId },
    /// 大域に値を置く。
    GlobalSet { global: GlobalId, src: Reg },
    /// 大域を初期化済みにする。
    GlobalMarkInit { global: GlobalId },
    /// インスタンスが初期化済みか。
    InstIsInit { dst: Reg, inst: InstanceId },
    /// インスタンスを初期化済みにする。
    InstMarkInit { inst: InstanceId },
    /// `def_instance` が評価したメソッドの値を置く。
    InstSetValue {
        inst: InstanceId,
        method: u32,
        src: Reg,
    },
    /// `def_instance` が評価したメソッドの値を読む。
    InstGetValue {
        dst: Reg,
        inst: InstanceId,
        method: u32,
    },
    /// `Ref.new`。
    RefNew { dst: Reg, value: Reg },
    /// `Ref.get`。
    RefGet { dst: Reg, r: Reg },
    /// `Ref.set`。
    RefSet { r: Reg, value: Reg },
    /// `Array.length`。
    ArrayLen { dst: Reg, a: Reg },
    /// `Array.get`(全域。範囲の外は Unit)。
    ArrayGet { dst: Reg, a: Reg, index: Reg },
    /// `MutableArray.new`(全域。負の長さは 0)。
    MArrayNew { dst: Reg, len: Reg, init: Reg },
    /// `MutableArray.length`。
    MArrayLen { dst: Reg, a: Reg },
    /// `MutableArray.get`(全域。範囲の外は Unit)。
    MArrayGet { dst: Reg, a: Reg, index: Reg },
    /// `MutableArray.set`(全域。範囲の外は何もしない)。
    MArraySet { a: Reg, index: Reg, value: Reg },
    /// `MutableArray.freeze`(写しを作る)。
    MArrayFreeze { dst: Reg, a: Reg },
    /// 捕獲を持たない関数を末尾でなく直に呼ぶ。
    CallDirect {
        dst: Reg,
        func: FuncId,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 値(クロージャ、メソッドの値)を末尾でなく呼ぶ。
    CallValue {
        dst: Reg,
        callee: Reg,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 辞書からメソッドを選んで末尾でなく呼ぶ。
    CallMethod {
        dst: Reg,
        class: ClassId,
        method: u32,
        dict: Reg,
        own: Vec<Reg>,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 辞書の抽象に辞書を渡す(末尾でない)。
    DictApp { dst: Reg, f: Reg, dicts: Vec<Reg> },
    /// 標準出力に書く(実行時が処理する `Console.write`)。
    ConsoleWrite { dst: Reg, s: Reg },
    /// 未実装の機能に届いた(終了状態 4)。`dst`(`any`)は続きの下ろしのための値で、書かれることはない。
    ///
    /// いまの下ろしは出さない(M86 の置き場。パス 8 がエフェクトの命令に置き換えた)。
    UnimplementedValue { dst: Reg, what: String },
    /// エフェクトの命令(パス 8。`doc/lir.md` の 16 節)。
    ///
    /// 被演算子の数と Rep、結果の有無と Rep は [`FxOp::sig`] が決める。
    Fx {
        op: FxOp,
        dst: Option<Reg>,
        args: Vec<Reg>,
    },
}

/// ハンドラ記録の節の欄。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandlerPart {
    /// `return` 節。
    Ret,
    /// `cancel` 節。
    Cancel,
}

impl HandlerPart {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            HandlerPart::Ret => "ret",
            HandlerPart::Cancel => "cancel",
        }
    }
}

/// [`Inst::Fx`] の種類。
///
/// 意味は `doc/lir.md` の 16 節にある。
/// `hr` はハンドラ記録、`k` は継続、`p` は退避した状態語の値で、どれも `any` の語である。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FxOp {
    /// `hr = handler_new E (op class)… [ret] [cancel]`。被演算子は本体、受け継ぐ継続、節の関数の値(書いた順)、
    /// `return` 節(`has_ret` のとき)、`cancel` 節(`has_cancel` のとき)である。
    HandlerNew {
        effect: u32,
        ops: Vec<(u32, kir::ClauseClass)>,
        has_ret: bool,
        has_cancel: bool,
    },
    /// `hr = handler_find E op`。連鎖の中でエフェクトが `E` である最も内側のハンドラ記録。
    /// `op` は、見つからないときの文面に書く操作である。
    HandlerFind { effect: u32, op: u32 },
    /// `saved = handlers_get`。今の連鎖の先頭(空なら Unit)。
    HandlersGet,
    /// `r = perform E op hr saved (a…)`。末尾でない呼び出しの一種で、再開点を持つ。
    Perform { effect: u32, op: u32 },
    /// `cont_check k`。`nontail` なら、節の外でないこと、再開済みでないこと、再開した `perform` の深さを確かめる。
    /// そうでなければ再開済みでないことだけを確かめる。
    ContCheck { nontail: bool },
    /// `b = is_cont x`。
    IsCont,
    /// `hr = cont_hr k`。
    ContHr,
    /// `handler_install hr`。
    HandlerInstall,
    /// `handler_uninstall hr`。
    HandlerUninstall,
    /// `r = call_body hr`。番兵を受け取る呼び出し。
    CallBody,
    /// `r = cont_resume k v`。番兵を受け取る呼び出しの一種で、継続を写し戻して続ける。
    ContResume,
    /// `b = is_pending r`。
    IsPending,
    /// `c = pending_class hr`。
    PendingClass,
    /// `p = pending_take`。
    PendingTake,
    /// `pending_restore p`。
    PendingRestore,
    /// `v = pending_value p`。
    PendingValue,
    /// `p = pending_discard hr v`。
    PendingDiscard,
    /// `pending_suppress`。
    PendingSuppress,
    /// `k = cont_make hr`。
    ContMake,
    /// `r = call_clause k`。番兵を受け取る呼び出し。
    CallClause,
    /// `cont_kill k`。
    ContKill,
    /// `b = cont_used k`。
    ContUsed,
    /// `b = handler_has hr ret|cancel`。
    HandlerHas(HandlerPart),
    /// `f = handler_get hr ret|cancel`。
    HandlerGet(HandlerPart),
    /// `r = call_cancel f`。番兵を受け取る呼び出し。
    CallCancel,
    /// `l = limit_get`(`i64`)。
    LimitGet,
    /// `limit_set l`。
    LimitSet,
}

/// [`FxOp`] の被演算子の決まり。
pub enum FxArgs {
    /// 数の決まった被演算子(Rep の列)。
    Fixed(&'static [Rep]),
    /// 少なくともこの数で、どれも `any`。
    AtLeast(usize),
}

impl FxOp {
    /// 印字の綴り。
    pub fn name(&self) -> &'static str {
        match self {
            FxOp::HandlerNew { .. } => "handler_new",
            FxOp::HandlerFind { .. } => "handler_find",
            FxOp::HandlersGet => "handlers_get",
            FxOp::Perform { .. } => "perform",
            FxOp::ContCheck { .. } => "cont_check",
            FxOp::IsCont => "is_cont",
            FxOp::ContHr => "cont_hr",
            FxOp::HandlerInstall => "handler_install",
            FxOp::HandlerUninstall => "handler_uninstall",
            FxOp::CallBody => "call_body",
            FxOp::ContResume => "cont_resume",
            FxOp::IsPending => "is_pending",
            FxOp::PendingClass => "pending_class",
            FxOp::PendingTake => "pending_take",
            FxOp::PendingRestore => "pending_restore",
            FxOp::PendingValue => "pending_value",
            FxOp::PendingDiscard => "pending_discard",
            FxOp::PendingSuppress => "pending_suppress",
            FxOp::ContMake => "cont_make",
            FxOp::CallClause => "call_clause",
            FxOp::ContKill => "cont_kill",
            FxOp::ContUsed => "cont_used",
            FxOp::HandlerHas(_) => "handler_has",
            FxOp::HandlerGet(_) => "handler_get",
            FxOp::CallCancel => "call_cancel",
            FxOp::LimitGet => "limit_get",
            FxOp::LimitSet => "limit_set",
        }
    }

    /// 被演算子の決まりと、結果の Rep(結果を持たなければ `None`)。
    pub fn sig(&self) -> (FxArgs, Option<Rep>) {
        use FxArgs::*;
        const A: &[Rep] = &[Rep::Any];
        const AA: &[Rep] = &[Rep::Any, Rep::Any];
        match self {
            FxOp::HandlerNew {
                ops,
                has_ret,
                has_cancel,
                ..
            } => (
                AtLeast(2 + ops.len() + usize::from(*has_ret) + usize::from(*has_cancel)),
                Some(Rep::Any),
            ),
            FxOp::HandlerFind { .. } | FxOp::HandlersGet | FxOp::PendingTake | FxOp::LimitGet => (
                Fixed(&[]),
                Some(if matches!(self, FxOp::LimitGet) {
                    Rep::I64
                } else {
                    Rep::Any
                }),
            ),
            FxOp::Perform { .. } => (AtLeast(2), Some(Rep::Any)),
            FxOp::ContCheck { .. }
            | FxOp::HandlerInstall
            | FxOp::HandlerUninstall
            | FxOp::PendingRestore
            | FxOp::ContKill => (Fixed(A), None),
            FxOp::IsCont | FxOp::IsPending | FxOp::ContUsed | FxOp::HandlerHas(_) => {
                (Fixed(A), Some(Rep::Bool))
            }
            FxOp::PendingClass => (Fixed(A), Some(Rep::I32)),
            FxOp::ContHr
            | FxOp::CallBody
            | FxOp::PendingValue
            | FxOp::ContMake
            | FxOp::CallClause
            | FxOp::HandlerGet(_)
            | FxOp::CallCancel => (Fixed(A), Some(Rep::Any)),
            FxOp::ContResume | FxOp::PendingDiscard => (Fixed(AA), Some(Rep::Any)),
            FxOp::PendingSuppress => (Fixed(&[]), None),
            FxOp::LimitSet => (Fixed(&[Rep::I64]), None),
        }
    }

    /// 末尾でない呼び出しの一種(再開点を持つ)か。
    pub fn is_call(&self) -> bool {
        matches!(
            self,
            FxOp::Perform { .. }
                | FxOp::CallBody
                | FxOp::ContResume
                | FxOp::CallClause
                | FxOp::CallCancel
        )
    }

    /// 呼ばれた側が番兵で戻ったとき、伝播せずに番兵を結果に書く呼び出しか。
    pub fn catches(&self) -> bool {
        matches!(
            self,
            FxOp::CallBody | FxOp::ContResume | FxOp::CallClause | FxOp::CallCancel
        )
    }

    /// ヒープに確保しうるか。
    pub fn allocates(&self) -> bool {
        matches!(
            self,
            FxOp::HandlerNew { .. } | FxOp::ContMake | FxOp::PendingTake | FxOp::PendingDiscard
        )
    }
}

/// [`Term::Fx`] の種類(`doc/lir.md` の 16 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FxTermOp {
    /// `propagate`。状態語が立っているときに、番兵で戻る。
    Propagate,
    /// `tail_call_never p`。退避した `DISCARD(hr, NeverBody)` の節の本体を末尾で呼ぶ。
    TailCallNever,
    /// `clause_value r`。T の節が `resume` せずに値 `r` になった。
    ClauseValue,
    /// `clause_matched sel`。N か G の節が選ばれた。
    ClauseMatched,
    /// `tail_resume v`。G の節の末尾の `resume(v)`。
    TailResume,
    /// `tail_call_clause op hr sel (a…)`。次の候補の節を末尾で呼ぶ。
    TailCallClause { op: u32 },
}

impl FxTermOp {
    /// 印字の綴り。
    pub fn name(&self) -> &'static str {
        match self {
            FxTermOp::Propagate => "propagate",
            FxTermOp::TailCallNever => "tail_call_never",
            FxTermOp::ClauseValue => "clause_value",
            FxTermOp::ClauseMatched => "clause_matched",
            FxTermOp::TailResume => "tail_resume",
            FxTermOp::TailCallClause { .. } => "tail_call_clause",
        }
    }

    /// 被演算子の数の決まり(どれも `any`)。`(最小, 固定か)`。
    pub fn arity(&self) -> (usize, bool) {
        match self {
            FxTermOp::Propagate => (0, true),
            FxTermOp::TailCallNever
            | FxTermOp::ClauseValue
            | FxTermOp::ClauseMatched
            | FxTermOp::TailResume => (1, true),
            FxTermOp::TailCallClause { .. } => (2, false),
        }
    }
}

/// 実行時エラーの種類(IRSpec の付録 B)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RtError {
    /// 1: `hole`。
    Hole,
    /// 2: 初期化の前の大域を読んだ。
    Unbound(GlobalId),
    /// 3: `letpat`、仮引数のパターン、`def_pattern` の照合の失敗。被演算子は照合した値(`any`)。
    PatternFail,
    /// 4: `match` のどの節にも一致しない。被演算子は照合した値(`any`)。
    MatchFail,
    /// 5: 範囲外の数値リテラル。字面を持つ。
    LitRange(String),
    /// 6: 初期化の前のインスタンスからメソッドを選んだ。
    InstanceNotReady(InstanceId),
    /// 10: 整数のゼロ除算。
    DivZero,
    /// 11: NaN を整数に変換しようとした。
    F64ToIntNan(ConvOp),
    /// 12: 整数への変換の結果が範囲外。被演算子は元の値(`f64`)。
    F64ToIntRange(ConvOp),
    /// 13: `__string_sub` の範囲外。
    StrSubRange,
    /// 14: `__panic`。被演算子は文面(`str`)。
    Panic,
    /// 15: 配列の範囲外。
    ArrayIndex,
    /// 16: `MutableArray.new` の負の長さ。
    MArrayNegLen,
    /// 17、18: 実装の無いプリミティブを呼んだ。
    ///
    /// どちらの番号かは大域の表示名と実装名で決まる([`Program::error_number`])。
    UnimplPrim(GlobalId),
    /// 19: メモリの不足。
    ///
    /// 確保する命令が実行時に起こすもので、下ろしは `error` に使わない(検証器が拒否する)。
    /// M87、M91、M94 の後段が実行時の状態語に使うために置く(計画 6.5 節)。
    OutOfMemory,
}

impl RtError {
    /// 付録 B の番号。
    ///
    /// [`RtError::UnimplPrim`] は 17 と 18 の両方を表し、ここでは 17 を返す。
    /// 18 を区別するには [`Program::error_number`] を使う。
    pub fn number(&self) -> u32 {
        match self {
            RtError::Hole => 1,
            RtError::Unbound(_) => 2,
            RtError::PatternFail => 3,
            RtError::MatchFail => 4,
            RtError::LitRange(_) => 5,
            RtError::InstanceNotReady(_) => 6,
            RtError::DivZero => 10,
            RtError::F64ToIntNan(_) => 11,
            RtError::F64ToIntRange(_) => 12,
            RtError::StrSubRange => 13,
            RtError::Panic => 14,
            RtError::ArrayIndex => 15,
            RtError::MArrayNegLen => 16,
            RtError::UnimplPrim(_) => 17,
            RtError::OutOfMemory => 19,
        }
    }

    /// 被演算子に要る Rep(要らなければ `None`)。
    pub fn operand_rep(&self) -> Option<Rep> {
        match self {
            RtError::PatternFail | RtError::MatchFail => Some(Rep::Any),
            RtError::F64ToIntRange(_) => Some(Rep::F64),
            RtError::Panic => Some(Rep::Str),
            _ => None,
        }
    }
}

/// ブロックの終わり。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Term {
    /// 無条件に移る。
    Jump(BlockId),
    /// `cond`(`bool`)で分ける。
    Branch {
        cond: Reg,
        then: BlockId,
        else_: BlockId,
    },
    /// `any` の値を返す。
    Return(Reg),
    /// 捕獲を持たない関数を末尾で呼ぶ。
    TailCallDirect {
        func: FuncId,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 値を末尾で呼ぶ。
    TailCallValue {
        callee: Reg,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 辞書からメソッドを選んで末尾で呼ぶ。
    TailCallMethod {
        class: ClassId,
        method: u32,
        dict: Reg,
        own: Vec<Reg>,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 実行時エラーを起こす。
    Error { kind: RtError, operand: Option<Reg> },
    /// 未実装の機能に届いた(終了状態 4)。
    Unimplemented { what: String },
    /// エフェクトのブロックの終わり(パス 8。`doc/lir.md` の 16 節)。被演算子はどれも `any` である。
    Fx { op: FxTermOp, args: Vec<Reg> },
}

impl Term {
    /// 移る先のブロック。
    pub fn succs(&self) -> Vec<BlockId> {
        match self {
            Term::Jump(b) => vec![*b],
            Term::Branch { then, else_, .. } => vec![*then, *else_],
            _ => Vec::new(),
        }
    }

    /// 移る先のブロックを書き換える。
    pub fn map_succs(&mut self, f: impl Fn(BlockId) -> BlockId) {
        match self {
            Term::Jump(b) => *b = f(*b),
            Term::Branch { then, else_, .. } => {
                *then = f(*then);
                *else_ = f(*else_);
            }
            _ => {}
        }
    }

    /// 読むレジスタ。
    pub fn uses(&self) -> Vec<Reg> {
        let mut v = Vec::new();
        self.for_each_use(|r| v.push(r));
        v
    }

    /// 読むレジスタを順に `f` に渡す。確保をしない(回収の中で使う)。
    pub fn for_each_use(&self, mut f: impl FnMut(Reg)) {
        match self {
            Term::Jump(_) | Term::Unimplemented { .. } => {}
            Term::Branch { cond, .. } => f(*cond),
            Term::Return(r) => f(*r),
            Term::TailCallDirect { args, .. } | Term::Fx { args, .. } => {
                args.iter().copied().for_each(f)
            }
            Term::TailCallValue { callee, args, .. } => {
                f(*callee);
                args.iter().copied().for_each(f);
            }
            Term::TailCallMethod {
                dict, own, args, ..
            } => {
                f(*dict);
                own.iter().chain(args).copied().for_each(f);
            }
            Term::Error { operand, .. } => operand.iter().copied().for_each(f),
        }
    }
}

impl Inst {
    /// 書くレジスタ。
    pub fn def(&self) -> Option<Reg> {
        use Inst::*;
        match self {
            Const { dst, .. }
            | Move { dst, .. }
            | Box { dst, .. }
            | Unbox { dst, .. }
            | EnvGet { dst, .. }
            | IntArith { dst, .. }
            | IntNeg { dst, .. }
            | IntCmp { dst, .. }
            | FloatArith { dst, .. }
            | FloatNeg { dst, .. }
            | FloatCmp { dst, .. }
            | FloatMath { dst, .. }
            | FloatTrunc { dst, .. }
            | FloatIsNan { dst, .. }
            | Conv { dst, .. }
            | BoolNot { dst, .. }
            | BoolEq { dst, .. }
            | StrConcat { dst, .. }
            | StrCmp { dst, .. }
            | StrLen { dst, .. }
            | StrSub { dst, .. }
            | Show { dst, .. }
            | CtorNew { dst, .. }
            | CtorTag { dst, .. }
            | CtorField { dst, .. }
            | VariantNew { dst, .. }
            | VariantTag { dst, .. }
            | VariantPayload { dst, .. }
            | RecNew { dst, .. }
            | RecGet { dst, .. }
            | RecExtend { dst, .. }
            | RecField { dst, .. }
            | RecHas { dst, .. }
            | RecRestrict { dst, .. }
            | RecUpdate { dst, .. }
            | ClosureNew { dst, .. }
            | MethodNew { dst, .. }
            | DictInst { dst, .. }
            | DictRecord { dst, .. }
            | DictVariant { dst, .. }
            | GlobalGet { dst, .. }
            | GlobalIsInit { dst, .. }
            | InstIsInit { dst, .. }
            | InstGetValue { dst, .. }
            | RefNew { dst, .. }
            | RefGet { dst, .. }
            | ArrayLen { dst, .. }
            | ArrayGet { dst, .. }
            | MArrayNew { dst, .. }
            | MArrayLen { dst, .. }
            | MArrayGet { dst, .. }
            | MArrayFreeze { dst, .. }
            | CallDirect { dst, .. }
            | CallValue { dst, .. }
            | CallMethod { dst, .. }
            | DictApp { dst, .. }
            | ConsoleWrite { dst, .. }
            | UnimplementedValue { dst, .. } => Some(*dst),
            Fx { dst, .. } => *dst,
            DepthEnter
            | DepthCheck
            | DepthPush
            | DepthRestore
            | ClosureSetCap { .. }
            | GlobalSet { .. }
            | GlobalMarkInit { .. }
            | InstMarkInit { .. }
            | InstSetValue { .. }
            | RefSet { .. }
            | MArraySet { .. } => None,
        }
    }

    /// 読むレジスタ(被演算子の順)。
    pub fn uses(&self) -> Vec<Reg> {
        let mut v = Vec::new();
        self.for_each_use(|r| v.push(r));
        v
    }

    /// 読むレジスタを被演算子の順に `f` に渡す。確保をしない(回収の中で使う)。
    pub fn for_each_use(&self, mut f: impl FnMut(Reg)) {
        use Inst::*;
        match self {
            Const { .. }
            | EnvGet { .. }
            | DepthEnter
            | DepthCheck
            | DepthPush
            | DepthRestore
            | GlobalGet { .. }
            | GlobalIsInit { .. }
            | GlobalMarkInit { .. }
            | InstIsInit { .. }
            | InstMarkInit { .. }
            | InstGetValue { .. }
            | UnimplementedValue { .. } => {}
            Move { src, .. } | Box { src, .. } | Unbox { src, .. } => f(*src),
            IntNeg { a, .. }
            | FloatNeg { a, .. }
            | FloatMath { a, .. }
            | FloatTrunc { a, .. }
            | FloatIsNan { a, .. }
            | Conv { a, .. }
            | BoolNot { a, .. }
            | StrLen { a, .. }
            | Show { a, .. }
            | ArrayLen { a, .. }
            | MArrayLen { a, .. }
            | MArrayFreeze { a, .. } => f(*a),
            IntArith { a, b, .. }
            | IntCmp { a, b, .. }
            | FloatArith { a, b, .. }
            | FloatCmp { a, b, .. }
            | BoolEq { a, b, .. }
            | StrConcat { a, b, .. }
            | StrCmp { a, b, .. } => {
                f(*a);
                f(*b);
            }
            StrSub { s, pos, len, .. } => {
                f(*s);
                f(*pos);
                f(*len);
            }
            CtorNew { fields, .. } | RecNew { fields, .. } => fields.iter().copied().for_each(f),
            CtorTag { v, .. } | CtorField { v, .. } | VariantTag { v, .. } => f(*v),
            VariantPayload { v, .. } => f(*v),
            VariantNew { payload, .. } => f(*payload),
            RecGet { rec, .. }
            | RecField { rec, .. }
            | RecHas { rec, .. }
            | RecRestrict { rec, .. } => f(*rec),
            RecExtend { rec, value, .. } | RecUpdate { rec, value, .. } => {
                f(*rec);
                f(*value);
            }
            ClosureNew { caps, .. } => caps.iter().copied().for_each(f),
            ClosureSetCap { clo, value, .. } => {
                f(*clo);
                f(*value);
            }
            MethodNew { dict, own, .. } => {
                f(*dict);
                own.iter().copied().for_each(f);
            }
            DictInst { premises, .. } => premises.iter().copied().for_each(f),
            DictRecord { fields, .. } | DictVariant { fields, .. } => {
                fields.iter().map(|(_, r)| *r).for_each(f)
            }
            GlobalSet { src, .. } | InstSetValue { src, .. } => f(*src),
            RefNew { value, .. } => f(*value),
            RefGet { r, .. } => f(*r),
            RefSet { r, value } => {
                f(*r);
                f(*value);
            }
            ArrayGet { a, index, .. } | MArrayGet { a, index, .. } => {
                f(*a);
                f(*index);
            }
            MArrayNew { len, init, .. } => {
                f(*len);
                f(*init);
            }
            MArraySet { a, index, value } => {
                f(*a);
                f(*index);
                f(*value);
            }
            CallDirect { args, .. } => args.iter().copied().for_each(f),
            CallValue { callee, args, .. } => {
                f(*callee);
                args.iter().copied().for_each(f);
            }
            CallMethod {
                dict, own, args, ..
            } => {
                f(*dict);
                own.iter().chain(args).copied().for_each(f);
            }
            DictApp { f: g, dicts, .. } => {
                f(*g);
                dicts.iter().copied().for_each(f);
            }
            ConsoleWrite { s, .. } => f(*s),
            Fx { args, .. } => args.iter().copied().for_each(f),
        }
    }

    /// 末尾でない呼び出しか(再開点を持つ命令)。
    pub fn is_call(&self) -> bool {
        match self {
            Inst::CallDirect { .. }
            | Inst::CallValue { .. }
            | Inst::CallMethod { .. }
            | Inst::DictApp { .. } => true,
            Inst::Fx { op, .. } => op.is_call(),
            _ => false,
        }
    }

    /// 末尾でない呼び出しで、再開点で yield を受け取りうると印の付いたものか(パス 11 の前の印)。
    ///
    /// エフェクトの呼び出しの一種はどれも受け取りうる。
    pub fn call_yields(&self) -> bool {
        match self {
            Inst::CallDirect { yields, .. }
            | Inst::CallValue { yields, .. }
            | Inst::CallMethod { yields, .. } => *yields,
            Inst::DictApp { .. } => true,
            Inst::Fx { op, .. } => op.is_call(),
            _ => false,
        }
    }

    /// ヒープに確保しうるか(GC の安全点)。`regs` はレジスタの Rep の表。
    pub fn allocates(&self, regs: &[Rep]) -> bool {
        use Inst::*;
        match self {
            Box { src, .. } => matches!(regs.get(src.index()), Some(Rep::I64 | Rep::F64)),
            CtorNew { fields, .. } => !fields.is_empty(),
            RecNew { fields, .. } => !fields.is_empty(),
            StrConcat { .. }
            | StrSub { .. }
            | Show { .. }
            | VariantNew { .. }
            | RecExtend { .. }
            | RecRestrict { .. }
            | RecUpdate { .. }
            | ClosureNew { .. }
            | MethodNew { .. }
            | DictInst { .. }
            | DictRecord { .. }
            | DictVariant { .. }
            | RefNew { .. }
            | MArrayNew { .. }
            | MArrayFreeze { .. } => true,
            Fx { op, .. } => op.allocates(),
            _ => false,
        }
    }
}

/// 基本ブロック。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// 命令の列。
    pub insts: Vec<Inst>,
    /// 終わり。
    pub term: Term,
}

/// 関数の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FuncKind {
    /// KIR の関数(同じ添字)。
    Kir(FnKind),
    /// 単位の初期化の手順。
    Init,
    /// `extern` の大域の値の本体(プリミティブか C の関数)。
    Extern,
    /// 組み込みの値の本体。
    Builtin,
    /// 組み込みのインスタンスのメソッドの本体。
    BuiltinMethod,
    /// ハンドラのフレームの関数 `$rt_handle`(パス 8。`doc/lir.md` の 16 節)。
    Handler,
}

impl FuncKind {
    /// 印字の綴り。
    pub fn name(self) -> &'static str {
        match self {
            FuncKind::Kir(k) => k.name(),
            FuncKind::Init => "init",
            FuncKind::Extern => "extern",
            FuncKind::Builtin => "builtin",
            FuncKind::BuiltinMethod => "builtin_method",
            FuncKind::Handler => "handler",
        }
    }
}

/// スロット(パス 12 の割り当て)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// `any` の区画(GC が走査する)の番号。
    Any(u32),
    /// 生の区画の番号。
    Raw(u32),
}

/// パス 12 の結果(フレームの配置)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// レジスタごとのスロット。
    pub slot: Vec<Slot>,
    /// `any` の区画のスロットの数。
    pub any_slots: u32,
    /// 生の区画のスロットの数。
    pub raw_slots: u32,
    /// 再開点(末尾でない呼び出し)。番号はこの列の位置である。
    ///
    /// 各点の集合は、呼び出しの後で生きているレジスタ(呼び出しが書くレジスタを除く)である。
    pub resume_points: Points,
    /// 確保の安全点。
    ///
    /// 各点の集合は、確保をまたいで生きているレジスタである。
    /// 命令の後で生きているもの(命令が書くレジスタを除く)と、命令の被演算子からなる。
    pub safepoints: Points,
    /// 再開点ごとの、yield を受け取りうるか(パス 11。`doc/lir.md` の 17 節)。
    ///
    /// 偽の再開点で待っているフレームに yield が届くのは、`may_yield` の印の誤りである。
    pub yield_points: Vec<bool>,
}

/// LIR の関数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Func {
    /// 記号。
    pub sym: String,
    /// 種類。
    pub kind: FuncKind,
    /// `yield` の印(KIR の関数の印。ほかは偽)。
    pub yields: bool,
    /// 捕獲の数。
    pub ncaps: u32,
    /// レジスタの Rep。
    pub regs: Vec<Rep>,
    /// 引数を受け取るレジスタ(どれも `any`)。
    pub params: Vec<Reg>,
    /// ブロック(先頭が入口)。
    pub blocks: Vec<Block>,
    /// パス 12 の結果。
    pub frame: Option<Frame>,
}

/// コンストラクタ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorInfo {
    /// 名前。
    pub name: String,
    /// フィールドの数。
    pub arity: u32,
}

/// 名目的な型。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    /// 記号。
    pub sym: String,
    /// コンストラクタ。
    pub ctors: Vec<CtorInfo>,
}

/// クラス。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassInfo {
    /// 記号。
    pub sym: String,
    /// メソッドの名前(宣言の順)。
    pub methods: Vec<String>,
}

/// 大域の種類。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalKind {
    /// 組み込みの値。値は `func` のクロージャである。
    Builtin { func: FuncId },
    /// `extern`。値は `func` のクロージャである。
    ///
    /// 実装名が付録 A.5 に無いときは `func` が `None` で、値を呼ぶと実行時エラー(17、18)になる。
    Extern {
        /// ABI。
        abi: Abi,
        /// 実装名。
        name: String,
        /// 受け取って捨てる辞書の数。
        dicts: u32,
        /// 本体の関数。
        func: Option<FuncId>,
    },
    /// 関数。値は `func` のクロージャである。
    Fn(FuncId),
    /// 辞書を受け取る束縛。値は `func` の辞書の抽象である。
    Dictabs(FuncId),
    /// 初期化の手順が置く値。
    Value(Rep),
}

/// 大域。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    /// 記号。
    pub sym: String,
    /// 付録 B の文面に書く名前(単位の鍵と末尾の `#n` を除いた修飾名)。
    pub display: String,
    /// 種類。
    pub kind: GlobalKind,
}

/// インスタンスのメソッドの実装。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstMethod {
    /// 実装の関数が受け取る辞書の数(前提の数 + メソッド自身の辞書の数)。
    pub dicts: u32,
    /// 実装の関数。
    pub func: FuncId,
}

/// インスタンス。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    /// 記号。
    pub sym: String,
    /// クラス。
    pub class: ClassId,
    /// 頭の型。
    pub head: TypeId,
    /// 付録 B の 6 番の文面の `<C>[<T>]`。
    pub display: String,
    /// 前提の数。
    pub premises: u32,
    /// クラスのメソッドの順に並べた実装。
    pub methods: Vec<InstMethod>,
}

/// 組み込みのインスタンスのメソッドの本体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinMethod {
    /// クラス。
    pub class: ClassId,
    /// 型。
    pub ty: TypeId,
    /// メソッド(クラスの中の位置)。
    pub method: u32,
    /// 本体の関数。
    pub func: FuncId,
}

/// LIR のプログラム。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// 関数。先頭の部分は KIR の関数と同じ添字である。
    ///
    /// 単位を足したプログラム([`crate::Extender`])では、足した単位の KIR の関数が、その前の関数の後ろに続く。
    pub funcs: Vec<Func>,
    /// KIR の関数の数(単位を足したプログラムでは、初めに下ろした単位の KIR の関数の数)。
    pub kir_funcs: u32,
    /// 大域(KIR と同じ添字)。
    pub globals: Vec<Global>,
    /// インスタンス(KIR と同じ添字)。
    pub instances: Vec<Instance>,
    /// 名目的な型(KIR と同じ添字)。
    pub types: Vec<TypeInfo>,
    /// クラス(KIR と同じ添字)。
    pub classes: Vec<ClassInfo>,
    /// 組み込みのインスタンスのメソッドの本体。
    pub builtin_methods: Vec<BuiltinMethod>,
    /// ラベル。
    pub labels: Vec<String>,
    /// レコードの形。各形はラベルの番号の昇順(同名は外側から)に並べた列で、先頭は空の形である。
    pub shapes: Vec<Vec<LabelId>>,
    /// 文字列の定数。
    pub strings: Vec<Vec<u8>>,
    /// 単位の初期化の関数(初期化の順)。
    pub init: Vec<FuncId>,
    /// エフェクト(KIR と同じ添字)。
    pub effects: Vec<EffectInfo>,
    /// ハンドラのフレームの関数 `$rt_handle`(プログラムが `handle` を持たなければ `None`)。
    pub rt_handle: Option<FuncId>,
}

/// エフェクト。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectInfo {
    /// 記号。
    pub sym: String,
    /// 付録 B の 9 番の文面に書く名前(単位の鍵を除いた修飾名)。
    pub display: String,
    /// 操作の名前と引数の数。
    pub ops: Vec<(String, u32)>,
}

/// 1 回の確保で作れる要素の数の上限 `N_alloc_max`(IRSpec 11.6 節)。
///
/// 下ろしはこの値を使わない。
/// 長さを取る確保の命令の意味がこの上限を含み(`doc/lir.md` の 1.7 節)、M87、M91、M94 の後段が
/// 実行時の確保の関数で確かめる(計画 6.5 節)。
pub const N_ALLOC_MAX: i32 = 1 << 26;

impl Program {
    /// 空のプログラム([`crate::Extender`] が単位を足していく初めの状態)。
    pub const fn empty() -> Program {
        Program {
            funcs: Vec::new(),
            kir_funcs: 0,
            globals: Vec::new(),
            instances: Vec::new(),
            types: Vec::new(),
            classes: Vec::new(),
            builtin_methods: Vec::new(),
            labels: Vec::new(),
            shapes: Vec::new(),
            strings: Vec::new(),
            init: Vec::new(),
            effects: Vec::new(),
            rt_handle: None,
        }
    }

    /// 実行時エラーの付録 B の番号。
    ///
    /// [`RtError::UnimplPrim`] は、大域の表示名が実装名に等しければ 17、違えば 18 である。
    pub fn error_number(&self, kind: &RtError) -> u32 {
        if let RtError::UnimplPrim(g) = kind
            && let Some(Global {
                display,
                kind: GlobalKind::Extern { name, .. },
                ..
            }) = self.globals.get(g.index())
            && display != name
        {
            return 18;
        }
        kind.number()
    }
}
