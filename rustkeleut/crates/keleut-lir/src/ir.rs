//! LIR の型。
//!
//! LIR は、関数ごとの基本ブロックのグラフである。
//! 各命令の意味は `rustkeleut/doc/lir.md` が定める。
//! ここには、型と、命令のオペランドをたどる補助関数だけを置く。
//!
//! - 仮想レジスタ [`Reg`] は関数ごとに番号を振り、それぞれがちょうど 1 つの [`Rep`] を持つ。
//!   SSA 形式ではない。
//!   合流する値は、各先行ブロックが同じレジスタに書く。
//! - 関数の制御フローは非巡回である。
//!   ブロックはトポロジカル順に並び、どの分岐も番号の大きいブロックへ向かう。
//! - 関数の境界で受け渡す値の表現は一様で、引数も返り値も `any` である。

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
    /// LIR の関数表の添字。
    /// 最初の変換でまとめて LIR に変換した KIR の関数は、KIR と同じ添字に置く。
    /// 単位を追加しない通常の変換では、KIR のプログラムのすべての関数がこれに当たる。
    /// 単位を追加したプログラムでの位置は [`Program::funcs`] に書く。
    FuncId
);
id_type!(
    /// ラベル表 [`Program::labels`] の添字。
    LabelId
);
id_type!(
    /// レコードのシェイプ表 [`Program::shapes`] の添字。
    ShapeId
);
id_type!(
    /// 文字列定数の表 [`Program::strings`] の添字。
    StrId
);

/// `const` 命令が置く定数。
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
    /// 文字列定数。
    Str(StrId),
    /// 引数のないコンストラクタの値。
    Ctor0 {
        /// 名目的な型。
        ty: TypeId,
        /// コンストラクタの番号(型の中の位置)。
        ctor: u32,
    },
    /// 組み込みのインスタンスの辞書。
    DictBuiltin {
        /// インスタンスのクラス。
        class: ClassId,
        /// インスタンスの対象の型。
        ty: TypeId,
    },
    /// 前提を持たない、ユーザのインスタンスの辞書。
    DictInst0(InstanceId),
}

impl Const {
    /// 定数の Rep を返す。
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
    /// この幅の整数の Rep を返す。
    pub fn rep(self) -> Rep {
        match self {
            IntTy::I32 => Rep::I32,
            IntTy::I64 => Rep::I64,
        }
    }

    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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
    /// 折り返す(2 の補数で下位のビットだけが残る)加算。
    Add,
    /// 折り返す減算。
    Sub,
    /// 折り返す乗算。
    Mul,
    /// 0 へ切り捨てる除算。
    /// どの入力にも結果を定めていて、`b = 0` なら 0、`MIN / -1` は `MIN` になる。
    Div,
    /// 除算の剰余。
    /// どの入力にも結果を定めていて、`b = 0` なら 0、`MIN % -1` は 0 になる。
    Rem,
}

impl IntOp {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
    pub fn name(self) -> &'static str {
        match self {
            FloatOp::Add => "add",
            FloatOp::Sub => "sub",
            FloatOp::Mul => "mul",
            FloatOp::Div => "div",
        }
    }
}

/// 比較の種類。
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
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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

/// `extern "C"` の数学関数。
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
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
    /// `extern "C"` の実装名と同じである。
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

/// 数値変換の種類。
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
    /// 0 へ切り捨てる。
    /// どの入力にも結果を定めていて、NaN は 0 になり、範囲外の値は飽和する。
    F64ToI32,
    /// 0 へ切り捨てる。
    /// どの入力にも結果を定めていて、NaN は 0 になり、範囲外の値は飽和する。
    F64ToI64,
}

impl ConvOp {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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

    /// 引数の Rep と結果の Rep の組を返す。
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

    /// 対応するプリミティブの実装名を返す。
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

/// `show` 命令が文字列にする値の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShowOp {
    /// `Int32` の 10 進表記。
    I32,
    /// `Int64` の 10 進表記。
    I64,
    /// LangSpec §16.2 が定める `Float64` の表記。
    F64,
    /// `true` か `false`。
    Bool,
}

impl ShowOp {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
    pub fn name(self) -> &'static str {
        match self {
            ShowOp::I32 => "i32",
            ShowOp::I64 => "i64",
            ShowOp::F64 => "f64",
            ShowOp::Bool => "bool",
        }
    }

    /// 引数の Rep を返す。
    pub fn rep(self) -> Rep {
        match self {
            ShowOp::I32 => Rep::I32,
            ShowOp::I64 => Rep::I64,
            ShowOp::F64 => Rep::F64,
            ShowOp::Bool => Rep::Bool,
        }
    }
}

/// 終端命令以外の命令。
///
/// オペランドの Rep と結果の Rep は [`crate::verify`] が検査する。
/// 意味と trap しない条件は、`doc/lir.md` の各節にある。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Inst {
    /// 定数を置く。
    Const { dst: Reg, value: Const },
    /// 同じ Rep のレジスタへコピーする。
    Move { dst: Reg, src: Reg },
    /// ボックス化していない値を一様表現の値(Rep `any` の値)にする。
    Box { dst: Reg, src: Reg },
    /// 一様表現の値から、ボックス化していない値を取り出す。
    Unbox { dst: Reg, src: Reg },
    /// 自分の環境の `index` 番目のキャプチャを読む。
    /// 入口の `depth_enter` の直後にだけ置く。
    EnvGet { dst: Reg, index: u32 },
    /// 入口で、コンテキストの深さを自分のフレームの深さとして記録する。
    DepthEnter,
    /// 呼び出し先の深さ(コンテキストの深さ + 1)を上限と比較する。
    DepthCheck,
    /// `DepthCheck` と同じ検査を行い、コンテキストの深さを 1 増やす。
    DepthPush,
    /// コンテキストの深さを自分のフレームの深さに書き戻す。
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
    /// 符号付き整数の比較。
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
    /// 符号反転(符号ビットを反転する)。
    FloatNeg { dst: Reg, a: Reg },
    /// IEEE 754 の比較(NaN を含めばどれも偽)。
    FloatCmp { op: CmpOp, dst: Reg, a: Reg, b: Reg },
    /// 数学関数。
    FloatMath { f: MathFn, dst: Reg, a: Reg },
    /// 0 へ切り捨てる。
    FloatTrunc { dst: Reg, a: Reg },
    /// NaN なら真。
    FloatIsNan { dst: Reg, a: Reg },
    /// 数値変換。
    Conv { op: ConvOp, dst: Reg, a: Reg },
    /// 真偽値の否定。
    BoolNot { dst: Reg, a: Reg },
    /// 2 つの真偽値が等しければ真。
    BoolEq { dst: Reg, a: Reg, b: Reg },
    /// 文字列の連結。
    StrConcat { dst: Reg, a: Reg, b: Reg },
    /// 2 つの文字列を、バイトを符号なしの値として辞書順に比較する。
    StrCmp { op: CmpOp, dst: Reg, a: Reg, b: Reg },
    /// 文字列のバイト数。
    StrLen { dst: Reg, a: Reg },
    /// 部分文字列。
    /// どの入力にも結果を定めていて、範囲外の部分は切り詰める。
    StrSub {
        dst: Reg,
        s: Reg,
        pos: Reg,
        len: Reg,
    },
    /// 値を文字列にする。
    Show { op: ShowOp, dst: Reg, a: Reg },
    /// 名目的な型の値を作る。
    /// `fields` はフィールドの位置の順に並べる。
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
    /// ヴァリアントのペイロードを読む。
    VariantPayload { dst: Reg, v: Reg },
    /// シェイプの決まったレコードを作る。
    /// `fields` は、シェイプのラベルの並びと同じ順に並べる。
    RecNew {
        dst: Reg,
        shape: ShapeId,
        fields: Vec<Reg>,
    },
    /// シェイプの決まったレコードの `index` 番目のフィールドを読む。
    RecGet { dst: Reg, rec: Reg, index: u32 },
    /// レコードの最も外側にフィールドを追加する。
    RecExtend {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        value: Reg,
    },
    /// ラベルが `label` であるフィールドのうち、外側から 0 始まりで `nth` 番目のものを読む。
    /// どの入力にも結果を定めていて、そのフィールドがなければ Unit になる。
    RecField {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        nth: u32,
    },
    /// ラベルが `label` であるフィールドのうち、外側から 0 始まりで `nth` 番目のものがあれば真。
    RecHas {
        dst: Reg,
        rec: Reg,
        label: LabelId,
        nth: u32,
    },
    /// 最も外側の `label` のフィールドを取り除く。
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
    /// 作ったばかりのクロージャのキャプチャを埋める(`letrec`)。
    ClosureSetCap { clo: Reg, index: u32, value: Reg },
    /// メソッドの値を作る。
    MethodNew {
        dst: Reg,
        class: ClassId,
        method: u32,
        dict: Reg,
        own: Vec<Reg>,
    },
    /// ユーザのインスタンスの辞書を作る。
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
    /// グローバルの値を読む。
    /// 初期化済みかどうかは検査しない。
    GlobalGet { dst: Reg, global: GlobalId },
    /// グローバルが初期化済みなら真。
    GlobalIsInit { dst: Reg, global: GlobalId },
    /// グローバルに値を置く。
    GlobalSet { global: GlobalId, src: Reg },
    /// グローバルを初期化済みにする。
    GlobalMarkInit { global: GlobalId },
    /// インスタンスが初期化済みなら真。
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
    /// `Array.get`。
    /// どの入力にも結果を定めていて、範囲外の添字では Unit になる。
    ArrayGet { dst: Reg, a: Reg, index: Reg },
    /// `MutableArray.new`。
    /// どの入力にも結果を定めていて、負の長さは 0 として扱う。
    MArrayNew { dst: Reg, len: Reg, init: Reg },
    /// `MutableArray.length`。
    MArrayLen { dst: Reg, a: Reg },
    /// `MutableArray.get`。
    /// どの入力にも結果を定めていて、範囲外の添字では Unit になる。
    MArrayGet { dst: Reg, a: Reg, index: Reg },
    /// `MutableArray.set`。
    /// どの入力にも結果を定めていて、範囲外の添字では何もしない。
    MArraySet { a: Reg, index: Reg, value: Reg },
    /// `MutableArray.freeze`(コピーを作る)。
    MArrayFreeze { dst: Reg, a: Reg },
    /// キャプチャを持たない関数を、非末尾呼び出しで直接呼ぶ。
    CallDirect {
        dst: Reg,
        func: FuncId,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 関数値(クロージャかメソッドの値)を非末尾呼び出しで呼ぶ。
    CallValue {
        dst: Reg,
        callee: Reg,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 辞書からメソッドを選んで非末尾呼び出しで呼ぶ。
    CallMethod {
        dst: Reg,
        class: ClassId,
        method: u32,
        dict: Reg,
        own: Vec<Reg>,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 辞書の抽象に辞書を渡す、非末尾の呼び出し。
    DictApp { dst: Reg, f: Reg, dicts: Vec<Reg> },
    /// 標準出力に書く(ランタイムが処理する `Console.write`)。
    ConsoleWrite { dst: Reg, s: Reg },
    /// 未実装の機能に実行が達したことを表す(終了コード 4)。
    /// `dst`(`any`)は、後に続く命令を変換できるようにするためのレジスタで、この命令が実行時に値を書くことはない。
    ///
    /// `handle`、`perform`、`resume` はパス 8 がエフェクトの命令に変換するので、現在の変換器はこの命令を生成しない。
    /// LIR への変換がまだない機能のプレースホルダとして残している。
    UnimplementedValue { dst: Reg, what: String },
    /// エフェクトの命令(パス 8。`doc/lir.md` の 16 節)。
    ///
    /// オペランドの個数と Rep、結果の有無と Rep は [`FxOp::sig`] が決める。
    Fx {
        op: FxOp,
        dst: Option<Reg>,
        args: Vec<Reg>,
    },
}

/// ハンドラ記録が持つ節のうち、`handler_has` と `handler_get` が指定する節。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandlerPart {
    /// `return` 節。
    Ret,
    /// `cancel` 節。
    Cancel,
}

impl HandlerPart {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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
/// `hr` はハンドラ記録、`k` は継続、`p` は退避した状態ワードの値で、どれも一様表現の値(`any`)である。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FxOp {
    /// `hr = handler_new E (op class)… [ret] [cancel]`。
    /// オペランドは、本体、引き継ぐ継続、節の関数の値(書いた順)、`return` 節(`has_ret` のとき)、`cancel` 節(`has_cancel` のとき)である。
    HandlerNew {
        effect: u32,
        ops: Vec<(u32, kir::ClauseClass)>,
        has_ret: bool,
        has_cancel: bool,
    },
    /// `hr = handler_find E op`。
    /// ハンドラの連鎖の中で、エフェクトが `E` である最も内側のハンドラ記録を結果にする。
    /// `op` は、見つからないときのメッセージに書く操作である。
    HandlerFind { effect: u32, op: u32 },
    /// `saved = handlers_get`。
    /// 現在のハンドラの連鎖の先頭を結果にする(空なら Unit)。
    HandlersGet,
    /// `r = perform E op hr saved (a…)`。
    /// 非末尾呼び出しの一種で、再開点を持つ。
    Perform { effect: u32, op: u32 },
    /// `cont_check k`。
    /// `nontail` なら、継続を受け取った節の外で再開しようとしていないこと、継続が再開済みでないこと、再開する `perform` の深さ(`c + Δ`。IRSpec 11.3 節)が上限を超えないことを検査する。
    /// そうでなければ、再開済みでないことだけを検査する。
    ContCheck { nontail: bool },
    /// `b = is_cont x`。
    IsCont,
    /// `hr = cont_hr k`。
    ContHr,
    /// `handler_install hr`。
    HandlerInstall,
    /// `handler_uninstall hr`。
    HandlerUninstall,
    /// `r = call_body hr`。
    /// 番兵を受け取る呼び出しである。
    CallBody,
    /// `r = cont_resume k v`。
    /// 番兵を受け取る呼び出しの一種で、継続のフレームを値スタックに復元して実行を続ける。
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
    /// `r = call_clause k`。
    /// 番兵を受け取る呼び出しである。
    CallClause,
    /// `cont_kill k`。
    ContKill,
    /// `b = cont_used k`。
    ContUsed,
    /// `b = handler_has hr ret|cancel`。
    HandlerHas(HandlerPart),
    /// `f = handler_get hr ret|cancel`。
    HandlerGet(HandlerPart),
    /// `r = call_cancel f`。
    /// 番兵を受け取る呼び出しである。
    CallCancel,
    /// `l = limit_get`(`i64`)。
    LimitGet,
    /// `limit_set l`。
    LimitSet,
}

/// [`FxOp`] のオペランドの規則。
pub enum FxArgs {
    /// 個数の決まったオペランド。
    /// 各オペランドの Rep を順に並べて持つ。
    Fixed(&'static [Rep]),
    /// この個数以上のオペランド。
    /// どれも `any` である。
    AtLeast(usize),
}

impl FxOp {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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

    /// オペランドの規則と、結果の Rep を返す。
    /// 結果を持たなければ、Rep は `None` である。
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

    /// 再開点を持つ、非末尾呼び出しの一種なら真を返す。
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

    /// 番兵を受け取る呼び出しなら真を返す。
    /// 呼び出し先が番兵を返して戻ったときに、伝播せずに番兵を結果に書く呼び出しである。
    pub fn catches(&self) -> bool {
        matches!(
            self,
            FxOp::CallBody | FxOp::ContResume | FxOp::CallClause | FxOp::CallCancel
        )
    }

    /// ヒープにメモリを確保することがあれば真を返す。
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
    /// `propagate`。
    /// 状態ワードが設定されているときに、番兵を返して戻る。
    Propagate,
    /// `tail_call_never p`。
    /// 退避した `DISCARD(hr, NeverBody)` の節の本体を末尾呼び出しで呼ぶ。
    TailCallNever,
    /// `clause_value r`。
    /// T の節が `resume` せずに値 `r` になったことを伝える。
    ClauseValue,
    /// `clause_matched sel`。
    /// N か G の節が選ばれたことを伝える。
    ClauseMatched,
    /// `tail_resume v`。
    /// G の節の末尾 `resume` である `resume(v)` を表す。
    TailResume,
    /// `tail_call_clause op hr sel (a…)`。
    /// 次の候補の節を末尾呼び出しで呼ぶ。
    TailCallClause { op: u32 },
}

impl FxTermOp {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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

    /// オペランドの個数の規則を、`(最小の個数, 固定かどうか)` の組で返す。
    /// オペランドはどれも `any` である。
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
    /// 2: 初期化前のグローバルを読んだ。
    Unbound(GlobalId),
    /// 3: `letpat`、仮引数のパターン、`def_pattern` の照合の失敗。
    /// オペランドは照合した値(`any`)である。
    PatternFail,
    /// 4: `match` のどの節にも一致しない。
    /// オペランドは照合した値(`any`)である。
    MatchFail,
    /// 5: 範囲外の数値リテラル。
    /// リテラルの表記を持つ。
    LitRange(String),
    /// 6: 初期化前のインスタンスからメソッドを選んだ。
    InstanceNotReady(InstanceId),
    /// 10: 整数のゼロ除算。
    DivZero,
    /// 11: NaN を整数に変換しようとした。
    F64ToIntNan(ConvOp),
    /// 12: 整数への変換の結果が範囲外。
    /// オペランドは変換前の値(`f64`)である。
    F64ToIntRange(ConvOp),
    /// 13: `__string_sub` の引数が範囲外。
    StrSubRange,
    /// 14: `__panic`。
    /// オペランドはメッセージ(`str`)である。
    Panic,
    /// 15: 配列の添字が範囲外。
    ArrayIndex,
    /// 16: `MutableArray.new` の負の長さ。
    MArrayNegLen,
    /// 17、18: 実装のないプリミティブを呼んだ。
    ///
    /// どちらの番号になるかは、グローバルの表示名と実装名で決まる([`Program::error_number`])。
    UnimplPrim(GlobalId),
    /// 19: メモリ不足。
    ///
    /// メモリを確保する命令が実行時に起こすエラーで、変換器は `error` に使わない(検証器が拒否する)。
    /// インタプリタと WasmGC バックエンドは、実行時エラーの種類からメッセージを決める処理で、この種類だけはメッセージを作らず、メモリ不足による終了として扱う。
    OutOfMemory,
}

impl RtError {
    /// IRSpec 付録 B の番号を返す。
    ///
    /// [`RtError::UnimplPrim`] は 17 と 18 の両方を表し、ここでは 17 を返す。
    /// 17 と 18 を区別するには [`Program::error_number`] を使う。
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

    /// オペランドの Rep を返す。
    /// オペランドを取らない種類では `None` である。
    pub fn operand_rep(&self) -> Option<Rep> {
        match self {
            RtError::PatternFail | RtError::MatchFail => Some(Rep::Any),
            RtError::F64ToIntRange(_) => Some(Rep::F64),
            RtError::Panic => Some(Rep::Str),
            _ => None,
        }
    }
}

/// ブロックの終端命令。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Term {
    /// 無条件に分岐する。
    Jump(BlockId),
    /// `cond`(`bool`)の値で分岐する。
    Branch {
        cond: Reg,
        then: BlockId,
        else_: BlockId,
    },
    /// `any` の値を返す。
    Return(Reg),
    /// キャプチャを持たない関数を、末尾呼び出しで直接呼ぶ。
    TailCallDirect {
        func: FuncId,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 関数値を末尾呼び出しで呼ぶ。
    TailCallValue {
        callee: Reg,
        args: Vec<Reg>,
        yields: bool,
    },
    /// 辞書からメソッドを選んで末尾呼び出しで呼ぶ。
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
    /// 未実装の機能に実行が達したことを表す(終了コード 4)。
    Unimplemented { what: String },
    /// エフェクトの終端命令(パス 8。`doc/lir.md` の 16 節)。
    /// オペランドはどれも `any` である。
    Fx { op: FxTermOp, args: Vec<Reg> },
}

impl Term {
    /// 後続ブロックを返す。
    pub fn succs(&self) -> Vec<BlockId> {
        match self {
            Term::Jump(b) => vec![*b],
            Term::Branch { then, else_, .. } => vec![*then, *else_],
            _ => Vec::new(),
        }
    }

    /// 後続ブロックを書き換える。
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

    /// 読むレジスタを返す。
    pub fn uses(&self) -> Vec<Reg> {
        let mut v = Vec::new();
        self.for_each_use(|r| v.push(r));
        v
    }

    /// 読むレジスタを順に `f` に渡す。
    /// メモリを確保しないので、GC の実行中にも使える。
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
    /// 書くレジスタを返す。
    /// 何も書かない命令では `None` である。
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

    /// 読むレジスタをオペランドの順に返す。
    pub fn uses(&self) -> Vec<Reg> {
        let mut v = Vec::new();
        self.for_each_use(|r| v.push(r));
        v
    }

    /// 読むレジスタをオペランドの順に `f` に渡す。
    /// メモリを確保しないので、GC の実行中にも使える。
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

    /// 非末尾呼び出し(再開点を持つ命令)なら真を返す。
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

    /// 再開点で yield を受け取ることがあると印の付いた非末尾呼び出しなら真を返す。
    /// 印は、パス 11 が枝刈りする前のものである。
    ///
    /// `dict_app` と、エフェクトの命令のうち非末尾呼び出しの一種であるものは、印を持たず、どれも受け取ることがあるものとして扱う。
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

    /// ヒープにメモリを確保することがあれば真を返す。
    /// 真を返す命令が GC の安全点になる。
    /// `regs` はレジスタの Rep の表である。
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
    /// 命令の並び。
    pub insts: Vec<Inst>,
    /// 終端命令。
    pub term: Term,
}

/// 関数の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FuncKind {
    /// KIR の関数。
    Kir(FnKind),
    /// 単位(コンパイル単位)の初期化関数。
    Init,
    /// `extern` のグローバルの値の本体(プリミティブか C の関数)。
    Extern,
    /// 組み込みの値の本体。
    Builtin,
    /// 組み込みのインスタンスのメソッドの本体。
    BuiltinMethod,
    /// ハンドラのフレームになる関数 `$rt_handle`(パス 8。`doc/lir.md` の 16 節)。
    Handler,
}

impl FuncKind {
    /// LIR のテキスト表示(`keleut dump lir` の出力など)での名前を返す。
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

/// パス 12 がレジスタに割り当てるスロット。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// `any` の領域(GC が走査する)の中の番号。
    Any(u32),
    /// ボックス化していない値の領域の中の番号。
    Raw(u32),
}

/// パス 12 の結果である、フレームのレイアウト。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// レジスタごとのスロット。
    pub slot: Vec<Slot>,
    /// `any` の領域のスロット数。
    pub any_slots: u32,
    /// ボックス化していない値の領域のスロット数。
    pub raw_slots: u32,
    /// 再開点(非末尾呼び出し)。
    /// 番号は、この並びの中の位置である。
    ///
    /// 各点の集合は、呼び出しの後で生存しているレジスタ(呼び出しが書くレジスタを除く)である。
    pub resume_points: Points,
    /// 安全点(メモリを確保する命令)。
    ///
    /// 各点の集合は、確保をまたいで生存しているレジスタである。
    /// 命令の後で生存しているレジスタ(命令が書くレジスタを除く)と、命令のオペランドからなる。
    pub safepoints: Points,
    /// 再開点ごとに、その再開点が yield を受け取ることがあれば真を持つ(パス 11。`doc/lir.md` の 17 節)。
    ///
    /// 値が偽である再開点で待っているフレームに yield が届くのは、`may_yield` の印が誤っているときである。
    /// そのとき、バックエンドは内部エラー(`yield が noyield の再開点に届きました(may_yield の印の誤り)`)として終了コード 70 で終了する。
    pub yield_points: Vec<bool>,
}

/// LIR の関数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Func {
    /// 関数のシンボル。
    pub sym: String,
    /// 関数の種類。
    pub kind: FuncKind,
    /// `yield` の印。
    /// KIR の関数では KIR の印と同じ値である。
    /// それ以外の関数では変換器が決め、初期化関数や `Array.each` の本体では真になる。
    pub yields: bool,
    /// キャプチャの個数。
    pub ncaps: u32,
    /// レジスタごとの Rep。
    pub regs: Vec<Rep>,
    /// 引数を受け取るレジスタ(どれも `any`)。
    pub params: Vec<Reg>,
    /// ブロックの並び。
    /// 先頭が入口である。
    pub blocks: Vec<Block>,
    /// パス 12 の結果。
    pub frame: Option<Frame>,
}

/// 名目的な型のコンストラクタ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtorInfo {
    /// コンストラクタの名前。
    pub name: String,
    /// フィールドの個数。
    pub arity: u32,
}

/// 名目的な型。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    /// 名目的な型のシンボル。
    pub sym: String,
    /// コンストラクタの並び。
    pub ctors: Vec<CtorInfo>,
}

/// 型クラス。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassInfo {
    /// 型クラスのシンボル。
    pub sym: String,
    /// メソッドの名前(宣言の順)。
    pub methods: Vec<String>,
}

/// グローバルの種類。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalKind {
    /// 組み込みの値。
    /// 値は `func` のクロージャである。
    Builtin { func: FuncId },
    /// `extern`。
    /// 値は `func` のクロージャである。
    ///
    /// 実装名が IRSpec 付録 A.5 にないときは `func` が `None` で、値を呼ぶと実行時エラー 17 か 18 になる。
    Extern {
        /// `extern` の ABI。
        abi: Abi,
        /// `extern` の実装名。
        name: String,
        /// 受け取って捨てる辞書の個数。
        dicts: u32,
        /// 本体の関数。
        func: Option<FuncId>,
    },
    /// 関数。
    /// 値はその関数のクロージャである。
    Fn(FuncId),
    /// 辞書を受け取る束縛。
    /// 値はその関数の辞書の抽象である。
    Dictabs(FuncId),
    /// 初期化手順が置く値。
    Value(Rep),
}

/// KIR のグローバル 1 つの情報。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    /// グローバルのシンボル。
    pub sym: String,
    /// IRSpec 付録 B のメッセージに書く名前(単位のキーと末尾の `#n` を除いた修飾名)。
    pub display: String,
    /// グローバルの種類。
    pub kind: GlobalKind,
}

/// インスタンスのメソッドの実装。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstMethod {
    /// 実装の関数が受け取る辞書の個数(前提の個数 + メソッド自身の辞書の個数)。
    pub dicts: u32,
    /// 実装の関数。
    pub func: FuncId,
}

/// 型クラスのインスタンス。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    /// インスタンスのシンボル。
    pub sym: String,
    /// インスタンスのクラス。
    pub class: ClassId,
    /// インスタンスの対象の型。
    pub head: TypeId,
    /// IRSpec 付録 B の 6 番のメッセージに書く `<C>[<T>]`。
    pub display: String,
    /// 前提の個数。
    pub premises: u32,
    /// クラスのメソッドの順に並べた実装。
    pub methods: Vec<InstMethod>,
}

/// 組み込みのインスタンスのメソッドの本体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinMethod {
    /// インスタンスのクラス。
    pub class: ClassId,
    /// インスタンスの対象の型。
    pub ty: TypeId,
    /// メソッドの番号(クラスの中の位置)。
    pub method: u32,
    /// 本体の関数。
    pub func: FuncId,
}

/// LIR のプログラム。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// プログラムの関数の表。
    /// 先頭の `kir_funcs` 個は KIR の関数で、KIR と同じ添字に置く。
    ///
    /// 単位を追加したプログラム([`crate::Extender`])では、追加した単位の KIR の関数が、それまでの関数の後ろに続く。
    pub funcs: Vec<Func>,
    /// KIR の関数の個数。
    /// 単位を追加したプログラムでは、最初に LIR に変換した単位の KIR の関数の個数である。
    pub kir_funcs: u32,
    /// グローバル(KIR と同じ添字)。
    pub globals: Vec<Global>,
    /// インスタンス(KIR と同じ添字)。
    pub instances: Vec<Instance>,
    /// 名目的な型(KIR と同じ添字)。
    pub types: Vec<TypeInfo>,
    /// クラス(KIR と同じ添字)。
    pub classes: Vec<ClassInfo>,
    /// 組み込みのインスタンスのメソッドの本体。
    pub builtin_methods: Vec<BuiltinMethod>,
    /// レコードとヴァリアントのラベル。
    pub labels: Vec<String>,
    /// レコードのシェイプ。
    /// 各シェイプは、ラベルを番号の昇順に並べたものである(同じラベルは外側のフィールドから並べる)。
    /// 表の先頭の項目は空のシェイプである。
    pub shapes: Vec<Vec<LabelId>>,
    /// 文字列定数。
    pub strings: Vec<Vec<u8>>,
    /// 単位ごとの初期化関数(初期化の順)。
    pub init: Vec<FuncId>,
    /// エフェクト(KIR と同じ添字)。
    pub effects: Vec<EffectInfo>,
    /// ハンドラのフレームになる関数 `$rt_handle`(プログラムが `handle` を持たなければ `None`)。
    pub rt_handle: Option<FuncId>,
}

/// エフェクトとその操作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectInfo {
    /// エフェクトのシンボル。
    pub sym: String,
    /// IRSpec 付録 B の 9 番のメッセージに書く名前(単位のキーを除いた修飾名)。
    pub display: String,
    /// 操作の名前と引数の個数。
    pub ops: Vec<(String, u32)>,
}

/// 1 回の確保で作れる要素数の上限 `N_alloc_max`(IRSpec 11.6 節)。
///
/// 変換器はこの値を使わない。
/// 長さを受け取ってメモリを確保する命令は、長さがこの上限を超えるとメモリ不足になる(`doc/lir.md` の 1.7 節)。
/// ランタイム(`keleut-rt`)のメモリを確保する関数と、WasmGC バックエンドが生成するコードが、長さをこの値と比較する。
pub const N_ALLOC_MAX: i32 = 1 << 26;

impl Program {
    /// 空のプログラムを返す。
    /// [`crate::Extender`] が単位を追加していく最初の状態である。
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

    /// 実行時エラーの、IRSpec 付録 B での番号を返す。
    ///
    /// [`RtError::UnimplPrim`] は、グローバルの表示名が実装名に等しければ 17、違えば 18 である。
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
