//! KIR のプログラムの型。
//!
//! 規範は `doc/IRSpec.md` である。
//! 型は IRSpec の 12.5 節の文法と 13.5 節、13.6 節の表をそのまま写す。
//!
//! 表の項目は、バイナリと同じく表の中の位置(添字)で指す。
//! コンストラクタ、操作、メソッドは、それを持つ型、エフェクト、クラスの中の位置で指す。
//! 記号と短い名前は文字列で持ち、テキストを書くときにだけ使う。
//!
//! 式、辞書、パターンのノードは、プログラムに 1 つある [`Nodes`] の配列に置き、
//! 子を [`ExpId`]、[`DictId`]、[`PatId`] で指す。
//! 木を `Box` でつなぐと、深い木を捨てるときの再帰がスタックを使い尽くす。
//! 配列に置けば、捨てることも写すことも比べることも平らな操作になる。

/// Rep(値の機械の上の表現の種類。IRSpec 4 節)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rep {
    /// 真偽値。
    Bool,
    /// 32 ビットの整数。
    I32,
    /// 64 ビットの整数。
    I64,
    /// IEEE 754 の binary64。
    F64,
    /// バイトの列。
    Str,
    /// ほかのすべての値。
    Any,
}

impl Rep {
    /// すべての Rep を、バイナリの番号の順に並べたもの。
    pub const ALL: [Rep; 6] = [Rep::Bool, Rep::I32, Rep::I64, Rep::F64, Rep::Str, Rep::Any];

    /// テキストの綴り。
    pub fn name(self) -> &'static str {
        match self {
            Rep::Bool => "bool",
            Rep::I32 => "i32",
            Rep::I64 => "i64",
            Rep::F64 => "f64",
            Rep::Str => "str",
            Rep::Any => "any",
        }
    }

    /// バイナリの番号。
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// 関数の種類(IRSpec 6.1 節)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FnKind {
    /// ラムダ、関数の定義、種類 `fn` の大域。
    Lambda,
    /// `handle` の操作節。
    Opclause,
    /// `handle` の `return` 節。
    Retclause,
    /// `handle` の `cancel` 節。
    Cancelclause,
    /// `handle` の本体。
    Handlebody,
    /// `def_value`、`def_pattern`、`run_exp` の式。
    Thunk,
    /// 辞書を受け取る束縛と、インスタンスのメソッドの実装。
    Dictabs,
}

impl FnKind {
    /// すべての種類を、バイナリの番号の順に並べたもの。
    pub const ALL: [FnKind; 7] = [
        FnKind::Lambda,
        FnKind::Opclause,
        FnKind::Retclause,
        FnKind::Cancelclause,
        FnKind::Handlebody,
        FnKind::Thunk,
        FnKind::Dictabs,
    ];

    /// テキストの綴り。
    pub fn name(self) -> &'static str {
        match self {
            FnKind::Lambda => "lambda",
            FnKind::Opclause => "opclause",
            FnKind::Retclause => "retclause",
            FnKind::Cancelclause => "cancelclause",
            FnKind::Handlebody => "handlebody",
            FnKind::Thunk => "thunk",
            FnKind::Dictabs => "dictabs",
        }
    }

    /// バイナリの番号。
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// 操作節の分類(IRSpec 9.2 節)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClauseClass {
    /// 操作の返り値の型が `Never` である。
    N,
    /// 本体の `resume` がすべて末尾の `resume` である。
    T,
    /// それ以外。
    G,
}

impl ClauseClass {
    /// すべての分類を、バイナリの番号の順に並べたもの。
    pub const ALL: [ClauseClass; 3] = [ClauseClass::N, ClauseClass::T, ClauseClass::G];

    /// テキストの綴り。
    pub fn name(self) -> &'static str {
        match self {
            ClauseClass::N => "N",
            ClauseClass::T => "T",
            ClauseClass::G => "G",
        }
    }

    /// バイナリの番号。
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// `extern` の ABI。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Abi {
    /// `extern "prim"`。
    Prim,
    /// `extern "C"`。
    C,
}

impl Abi {
    /// すべての ABI を、バイナリの番号の順に並べたもの。
    pub const ALL: [Abi; 2] = [Abi::Prim, Abi::C];

    /// テキストの綴り。
    pub fn name(self) -> &'static str {
        match self {
            Abi::Prim => "prim",
            Abi::C => "C",
        }
    }

    /// バイナリの番号。
    pub fn code(self) -> u8 {
        self as u8
    }
}

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
    /// 型の表の添字。
    TypeId
);
id_type!(
    /// エフェクトの表の添字。
    EffectId
);
id_type!(
    /// クラスの表の添字。
    ClassId
);
id_type!(
    /// インスタンスの表の添字。
    InstanceId
);
id_type!(
    /// 大域の表の添字。
    GlobalId
);
id_type!(
    /// 関数の表の添字。
    FnId
);
id_type!(
    /// [`Nodes::exps`] の添字。
    ExpId
);
id_type!(
    /// [`Nodes::dicts`] の添字。
    DictId
);
id_type!(
    /// [`Nodes::pats`] の添字。
    PatId
);

/// 定数(IRSpec 5.1 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Lit {
    /// 真偽値。
    Bool(bool),
    /// `Int32` の値。
    I32(i32),
    /// `Int64` の値。
    I64(i64),
    /// `Float64` の値の IEEE 754 のビット列。
    F64(u64),
    /// `String` の値(任意のバイト列)。
    Str(Vec<u8>),
}

/// 捕獲の列の要素と、関数の中の値の読み方。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cap {
    /// スロット。
    Local(u32),
    /// 捕獲。
    Env(u32),
    /// 辞書の引数。
    DParam(u32),
}

/// 関数を作る部分(`closure`、`dictabs`、`handle` の各部分)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Part {
    /// 作る関数。
    pub func: FnId,
    /// 捕獲の列。
    pub caps: Vec<Cap>,
}

/// `match` の節。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Clause {
    /// パターン。
    pub pat: PatId,
    /// ガード。
    pub guard: Option<ExpId>,
    /// 本体。
    pub body: ExpId,
}

/// `handle` の操作節の項目。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HandlerOp {
    /// 操作(エフェクトの中の位置)。
    pub op: u32,
    /// 書いた分類。
    pub class: ClauseClass,
    /// 操作節の関数。
    pub part: Part,
}

/// ハンドラ式。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    /// 末尾の印。
    pub tail: bool,
    /// 処理するエフェクト。
    pub effect: EffectId,
    /// 本体の関数。
    pub body: Part,
    /// 操作節(書いた順。1 個以上)。
    pub ops: Vec<HandlerOp>,
    /// `return` 節。
    pub ret: Option<Part>,
    /// `cancel` 節。
    pub cancel: Option<Part>,
}

/// 式(IRSpec 5.1 節)。子は評価の順に並ぶ。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Exp {
    /// 定数。
    Lit(Lit),
    /// 範囲外のリテラル(`lit_range`)と、その字面。
    LitError(String),
    /// スロットを読む。
    Local(u32),
    /// 捕獲を読む。
    Env(u32),
    /// 大域を読む。
    Global(GlobalId),
    /// 構築。項目はソースに書いた順の(フィールドの位置, 式)。
    Construct {
        /// 型。
        ty: TypeId,
        /// コンストラクタ(型の中の位置)。
        ctor: u32,
        /// 項目。
        fields: Vec<(u32, ExpId)>,
    },
    /// ヴァリアント。
    Variant {
        /// ラベル。
        label: String,
        /// 積載値。
        value: ExpId,
    },
    /// 呼び出し。
    Call {
        /// 末尾の印。
        tail: bool,
        /// `yield` の印。
        yields: bool,
        /// 呼び出し先。
        func: ExpId,
        /// 引数。
        args: Vec<ExpId>,
    },
    /// メソッドの値。
    Method {
        /// クラス。
        class: ClassId,
        /// メソッド(クラスの中の位置)。
        method: u32,
        /// クラスの辞書 `d`。
        dict: DictId,
        /// メソッド自身の辞書 `o`。
        own: Vec<DictId>,
    },
    /// 演算子の呼び出し。
    Opcall {
        /// クラス。
        class: ClassId,
        /// メソッド(クラスの中の位置)。
        method: u32,
        /// クラスの辞書。
        dict: DictId,
        /// 引数。
        args: Vec<ExpId>,
    },
    /// 否定。
    Not(ExpId),
    /// 論理積(短絡)。
    And(ExpId, ExpId),
    /// 論理和(短絡)。
    Or(ExpId, ExpId),
    /// クロージャ。
    Closure(Part),
    /// 辞書の抽象。
    Dictabs(Part),
    /// 辞書の適用。
    Dictapp {
        /// 辞書を渡す相手。
        func: ExpId,
        /// 辞書の列。
        dicts: Vec<DictId>,
    },
    /// 束縛。
    Let {
        /// スロット。
        slot: u32,
        /// 初期化式。
        value: ExpId,
        /// 続きの式。
        body: ExpId,
    },
    /// パターンの束縛。
    Letpat {
        /// パターン。
        pat: PatId,
        /// 初期化式。
        value: ExpId,
        /// 続きの式。
        body: ExpId,
    },
    /// 再帰の束縛。
    Letrec {
        /// 項目(スロット, `closure` か `dictabs` の式)。1 個以上。
        items: Vec<(u32, ExpId)>,
        /// 続きの式。
        body: ExpId,
    },
    /// 列(2 個以上)。
    Seq(Vec<ExpId>),
    /// 照合。
    Match {
        /// 照合対象。
        scrutinee: ExpId,
        /// 節。
        clauses: Vec<Clause>,
    },
    /// 空のレコード。
    RecEmpty,
    /// レコードの拡張。`value` を先に、`record` を後に評価する。
    RecExtend {
        /// ラベル。
        label: String,
        /// 足す値。
        value: ExpId,
        /// 元のレコード。
        record: ExpId,
    },
    /// 選択。
    RecSelect {
        /// ラベル。
        label: String,
        /// レコード。
        record: ExpId,
    },
    /// 制限。
    RecRestrict {
        /// ラベル。
        label: String,
        /// レコード。
        record: ExpId,
    },
    /// 更新。`record` を先に、`value` を後に評価する。
    RecUpdate {
        /// ラベル。
        label: String,
        /// レコード。
        record: ExpId,
        /// 新しい値。
        value: ExpId,
    },
    /// `perform`。
    Perform {
        /// エフェクト。
        effect: EffectId,
        /// 操作(エフェクトの中の位置)。
        op: u32,
        /// 引数。
        args: Vec<ExpId>,
    },
    /// ハンドラ式。
    Handle(Box<Handle>),
    /// `resume`。
    Resume {
        /// 末尾の印。
        tail: bool,
        /// 渡す値。
        value: ExpId,
    },
    /// 穴(`???`)。
    Hole,
}

/// 辞書の式(IRSpec 5.2 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dict {
    /// 利用者のインスタンスに前提の辞書を渡したもの。
    Inst {
        /// インスタンス。
        instance: InstanceId,
        /// 前提の辞書。
        premises: Vec<DictId>,
    },
    /// 辞書の引数。
    DParam(u32),
    /// 捕獲した辞書。
    Env(u32),
    /// 閉じたレコード型の構造的な `Eq`。
    Record(Vec<(String, DictId)>),
    /// 閉じたヴァリアント型の構造的な `Eq`。
    Variant(Vec<(String, DictId)>),
    /// 組み込みのインスタンス。
    Builtin {
        /// クラス。
        class: ClassId,
        /// 型。
        ty: TypeId,
    },
}

/// レコードのパターンの尾部。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RecordTail {
    /// タプルのパターン。
    Closed,
    /// 既定のレコードのパターン。
    Open,
    /// 残りを照合するパターン。
    Rest(PatId),
}

/// パターン(IRSpec 8 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pat {
    /// ワイルドカード。
    Wild,
    /// 変数(スロットか、`def_pattern` では大域の列の位置)。
    Bind(u32),
    /// リテラル。
    Lit(Lit),
    /// レコード。
    Record {
        /// (ラベル, パターン) の列。
        fields: Vec<(String, PatId)>,
        /// 尾部。
        tail: RecordTail,
    },
    /// コンストラクタ。
    Ctor {
        /// 型。
        ty: TypeId,
        /// コンストラクタ(型の中の位置)。
        ctor: u32,
        /// (フィールドの位置, パターン) の列。
        fields: Vec<(u32, PatId)>,
    },
    /// ヴァリアント。
    Variant {
        /// ラベル。
        label: String,
        /// 積載値のパターン。
        pat: PatId,
    },
}

/// 式、辞書、パターンのノードの置き場。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nodes {
    /// 式。
    pub exps: Vec<Exp>,
    /// 辞書。
    pub dicts: Vec<Dict>,
    /// パターン。
    pub pats: Vec<Pat>,
}

impl Nodes {
    /// 式を置き、その添字を返す。
    pub fn push_exp(&mut self, e: Exp) -> ExpId {
        self.exps.push(e);
        ExpId((self.exps.len() - 1) as u32)
    }

    /// 辞書を置き、その添字を返す。
    pub fn push_dict(&mut self, d: Dict) -> DictId {
        self.dicts.push(d);
        DictId((self.dicts.len() - 1) as u32)
    }

    /// パターンを置き、その添字を返す。
    pub fn push_pat(&mut self, p: Pat) -> PatId {
        self.pats.push(p);
        PatId((self.pats.len() - 1) as u32)
    }

    /// 式を引く。添字が範囲の外なら `None`。
    pub fn exp(&self, id: ExpId) -> Option<&Exp> {
        self.exps.get(id.index())
    }

    /// 辞書を引く。添字が範囲の外なら `None`。
    pub fn dict(&self, id: DictId) -> Option<&Dict> {
        self.dicts.get(id.index())
    }

    /// パターンを引く。添字が範囲の外なら `None`。
    pub fn pat(&self, id: PatId) -> Option<&Pat> {
        self.pats.get(id.index())
    }
}

/// 型のコンストラクタ。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ctor {
    /// 名前。
    pub name: String,
    /// フィールドの数。
    pub arity: u32,
}

/// 名目的な型。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Type {
    /// 記号。
    pub sym: String,
    /// コンストラクタ。
    pub ctors: Vec<Ctor>,
}

/// エフェクトの操作。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Op {
    /// 名前。
    pub name: String,
    /// 引数の数。
    pub arity: u32,
    /// 返り値の型が `Never` か。
    pub never: bool,
    /// 実行時が処理するか。
    pub runtime: bool,
}

/// エフェクト。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Effect {
    /// 記号。
    pub sym: String,
    /// 操作。
    pub ops: Vec<Op>,
}

/// クラスのメソッド。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClassMethod {
    /// 名前。
    pub name: String,
    /// メソッド自身の辞書の数。
    pub dicts: u32,
}

/// クラス。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Class {
    /// 記号。
    pub sym: String,
    /// メソッド(宣言の順)。
    pub methods: Vec<ClassMethod>,
}

/// インスタンスのメソッドの実装の項目。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Impl {
    /// メソッド(クラスの中の位置)。
    pub method: u32,
    /// メソッド自身の辞書の数。
    pub dicts: u32,
    /// 実装の関数。
    pub func: FnId,
}

/// インスタンス(IRSpec 7.2 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Instance {
    /// 記号。
    pub sym: String,
    /// クラス。
    pub class: ClassId,
    /// 頭の型構成子。
    pub head: TypeId,
    /// 前提の数。
    pub premises: u32,
    /// 実装の項目(本体に書いた順)。
    pub methods: Vec<Impl>,
}

/// 大域の種類と欄。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GlobalKind {
    /// 組み込みの値。
    Builtin,
    /// プリミティブか C の関数。
    Extern {
        /// ABI。
        abi: Abi,
        /// 実装名。
        name: String,
        /// 受け取って捨てる辞書の数。
        dicts: u32,
    },
    /// 関数。
    Fn(FnId),
    /// 辞書を受け取る関数。
    Dictabs(FnId),
    /// 初期化の手順が置く値。
    Value(Rep),
}

/// 大域。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Global {
    /// 記号。
    pub sym: String,
    /// 種類。
    pub kind: GlobalKind,
}

/// 関数の表の項目(IRSpec 6.2 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Function {
    /// 記号。
    pub sym: String,
    /// 種類。
    pub kind: FnKind,
    /// `yield` の印。
    pub yields: bool,
    /// 引数のスロットの Rep。
    pub params: Vec<Rep>,
    /// 引数でないスロットの Rep。
    pub locals: Vec<Rep>,
    /// 辞書の引数の数。
    pub dicts: u32,
    /// 捕獲の Rep。
    pub caps: Vec<Rep>,
    /// 結果の Rep。
    pub result: Rep,
    /// 引数ごとのパターン(`opclause` だけが持つ)。
    pub pats: Vec<PatId>,
    /// ガード(`opclause` だけが持つ)。
    pub guard: Option<ExpId>,
    /// 本体。
    pub body: ExpId,
}

/// 初期化の手順(IRSpec 10.1 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    /// `def_value`。
    DefValue {
        /// 定義する大域。
        global: GlobalId,
        /// `thunk` の関数。
        func: FnId,
    },
    /// `def_pattern`。
    DefPattern {
        /// `thunk` の関数。
        func: FnId,
        /// パターンが束縛する大域の列。
        globals: Vec<GlobalId>,
        /// パターン。`bind k` は大域の列の `k` 番目を指す。
        pat: PatId,
    },
    /// `def_fns`。
    DefFns(Vec<GlobalId>),
    /// `def_dictabs`。
    DefDictabs(GlobalId),
    /// `def_instance`。
    DefInstance(InstanceId),
    /// `run_exp`。
    RunExp(FnId),
}

/// 単位(IRSpec 2 節)。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Unit {
    /// 鍵。
    pub key: String,
    /// 元のファイルの綴り(任意のバイト列)。
    pub paths: Vec<Vec<u8>>,
    /// 初期化の手順。
    pub init: Vec<Step>,
}

/// 1 つのプログラム(IRSpec 2.1 節)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// 版(主版, 副版)。
    pub version: (u16, u16),
    /// 単位(初期化の順)。
    pub units: Vec<Unit>,
    /// 型。
    pub types: Vec<Type>,
    /// エフェクト。
    pub effects: Vec<Effect>,
    /// クラス。
    pub classes: Vec<Class>,
    /// インスタンス。
    pub instances: Vec<Instance>,
    /// 大域。
    pub globals: Vec<Global>,
    /// 関数。
    pub functions: Vec<Function>,
    /// 式、辞書、パターンのノード。
    pub nodes: Nodes,
}

/// この crate が書く版(主版、副版)。
///
/// 読み手は主版だけを比べ、副版は問わない(IRSpec 14 節)。
/// 副版が足すのは、読み飛ばせる節と、必要な機能のビットで守る新しい命令だけだからである。
pub const VERSION: (u16, u16) = (1, 0);

/// 記号の単位の鍵(最後の `/` より前の部分。`/` が無ければ記号そのもの)。
pub fn unit_of_symbol(sym: &str) -> &str {
    match sym.rfind('/') {
        Some(i) => &sym[..i],
        None => sym,
    }
}
