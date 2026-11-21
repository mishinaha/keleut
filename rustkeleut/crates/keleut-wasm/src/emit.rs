//! モジュールの組み立て(型、関数、グローバル、データ、名前の各セクション)。
//!
//! 型は、import する関数の型を除いて、すべて 1 つの rec グループに置く。
//! 構造が同じ型どうしも、rec グループの中の位置が違えば別の型になるので、`ref.test` で値の種類を区別できる。
//! import する関数の型は、rec グループの前に、独立した型として置く。
//! `rt.wasm` が export する関数の型と一致させるためである。
//! ホストから import する関数の型も同じ場所に置く。
//! 同じシグネチャの関数は 1 つの型を共有する。
//!
//! 関数の番号は、import する関数の後ろに、宣言した順に振る([`Mb::declare_func`])。
//! 本体は後で置く([`Mb::set_body`])。
//! そのため、まだ本体のない関数も、番号と型を先に決めて呼び出せる。

use std::collections::{BTreeSet, HashMap};

use wasm_encoder::{
    AbstractHeapType, ArrayType, CodeSection, CompositeInnerType, CompositeType, ConstExpr,
    DataCountSection, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    FieldType, FuncType, Function, FunctionSection, GlobalSection, GlobalType, HeapType,
    ImportSection, IndirectNameMap, InstructionSink, MemoryType, Module, NameMap, NameSection,
    RefType, StorageType, StructType, SubType, TypeSection, ValType,
};

/// `eqref`。
pub const EQ: ValType = ValType::Ref(RefType::EQREF);
/// `i32`。
pub const I32: ValType = ValType::I32;
/// `i64`。
pub const I64: ValType = ValType::I64;
/// `f64`。
pub const F64: ValType = ValType::F64;

/// 抽象ヒープ型 `eq`。
pub const HEQ: HeapType = HeapType::Abstract {
    shared: false,
    ty: AbstractHeapType::Eq,
};

/// 抽象ヒープ型 `i31`。
pub const HI31: HeapType = HeapType::Abstract {
    shared: false,
    ty: AbstractHeapType::I31,
};

/// 型 `idx` への参照型(`RefType`)を作る。
pub fn rt(nullable: bool, idx: u32) -> RefType {
    RefType {
        nullable,
        heap_type: HeapType::Concrete(idx),
    }
}

/// 型 `idx` への参照型を、値型(`ValType`)として作る。
pub fn rv(nullable: bool, idx: u32) -> ValType {
    ValType::Ref(rt(nullable, idx))
}

/// どのモジュールにも置く固定の型の番号。
#[derive(Clone, Copy, Debug)]
pub struct Tys {
    /// 文字列 `(array (mut i8))`。
    pub str_: u32,
    /// 一様表現の値(`eqref`)の配列。`Array` と `MutableArray` の値の型は、この型の部分型である。
    pub anyarr: u32,
    /// `Array` の値。
    pub immarr: u32,
    /// `MutableArray` の値。
    pub mutarr: u32,
    /// `i32` の配列。
    pub i32arr: u32,
    /// 31 ビットに収まらない `Int32` のボックス。
    pub box_i32: u32,
    /// `Int64` のボックス。
    pub box_i64: u32,
    /// `Float64` のボックス。
    pub box_f64: u32,
    /// Unit(1 つだけの値)。
    pub unit: u32,
    /// 真偽値(真と偽の 2 つの値)。
    pub bool_: u32,
    /// データ(型、コンストラクタ、フィールドの配列)。
    pub data: u32,
    /// ヴァリアント(ラベル、ペイロード)。
    pub variant: u32,
    /// レコード(シェイプ、フィールドの配列)。
    pub record: u32,
    /// クロージャの基底型(関数の番号、キャプチャの配列)。引数の個数ごとのクロージャの型は、この型の部分型である。
    pub clo: u32,
    /// メソッドの値。
    pub method: u32,
    /// 実装のない `extern` の値。
    pub externu: u32,
    /// `Ref`。
    pub ref_: u32,
    /// 組み込みのインスタンスの辞書。
    pub dictb: u32,
    /// ユーザのインスタンスの辞書。
    pub dicti: u32,
    /// 構造的な `Eq` のレコードの辞書。
    pub dictr: u32,
    /// 構造的な `Eq` のヴァリアントの辞書。
    pub dictv: u32,
    /// ハンドラ記録。
    pub hr: u32,
    /// 退避した状態ワード。
    pub saved: u32,
    /// 継続。
    pub cont: u32,
    /// フレームの記録の基底型。関数ごとの記録の型は、この型の部分型である。
    pub frame: u32,
    /// 再開用エントリの型 `(ref $Frame) → eqref`。
    pub resfn: u32,
    /// 関数の参照の配列。
    pub funcarr: u32,
}

/// ハンドラ記録のフィールドの番号。
pub mod hrf {
    pub const ID: u32 = 0;
    pub const EFFECT: u32 = 1;
    pub const BODY: u32 = 2;
    pub const RES: u32 = 3;
    pub const OPS: u32 = 4;
    pub const CLASSES: u32 = 5;
    pub const CLAUSES: u32 = 6;
    pub const RET: u32 = 7;
    pub const CANCEL: u32 = 8;
    pub const PARENT: u32 = 9;
    /// インタプリタと JIT のランタイム(keleut-rt)のハンドラ記録が持つ `h` に相当するフィールド。
    ///
    /// WasmGC バックエンドは、ハンドラのフレームの深さを `$rt_handle` のローカルに持つ。
    /// そのため、このフィールドは読まず、書き換えもしない。値は、記録を作ったときの 0 のままである。
    pub const H: u32 = 10;
}

/// 退避した状態ワードのフィールドの番号。グローバルに置く状態ワードと同じ順序である。
pub mod svf {
    pub const KIND: u32 = 0;
    pub const HR: u32 = 1;
    pub const VAL: u32 = 2;
    pub const CLAUSE: u32 = 3;
    pub const ARGS: u32 = 4;
    pub const MSG: u32 = 5;
}

/// 継続のフィールドの番号。
pub mod ctf {
    /// 継続をキャプチャしたハンドラのハンドラ記録。
    pub const HR: u32 = 0;
    /// フレームの記録の並びの先頭。最上段の `perform` のフレームの記録を指し、再開したら null にする。
    pub const FRAMES: u32 = 1;
    /// 継続をキャプチャしたときの、ハンドラのフレームの深さ `h`。
    pub const H: u32 = 2;
    /// `Δ = p − h`。
    pub const DELTA: u32 = 3;
    /// ハンドラの連鎖から切り出した部分の先頭。再開するときに、ハンドラの連鎖の先頭に戻す。
    pub const CUT: u32 = 4;
    /// 選んだ G の節の候補の番号。
    pub const CLAUSE: u32 = 5;
    /// 操作の引数の配列。
    pub const ARGS: u32 = 6;
    /// 再開済みであることを表すフラグ。
    pub const USED: u32 = 7;
    /// 節を評価している間であることを表すフラグ。
    pub const ALIVE: u32 = 8;
}

/// フレームの記録のフィールドの番号(基底型の分)。
pub mod frf {
    /// 再開する区切りの番号。分割する関数では、分割関数の中での番号である。
    pub const RP: u32 = 0;
    /// 再開用エントリ(`$ResFn`)。
    pub const ENTRY: u32 = 1;
    /// 並びの次の記録(外側のフレームの記録)。
    pub const NEXT: u32 = 2;
    /// 継続をキャプチャしたときの、このフレームの深さ。
    pub const DEPTH: u32 = 3;
    /// 関数ごとのフィールドが始まる番号。
    pub const BASE: u32 = 4;
}

/// フレームの記録の基底型のフィールドを返す。部分型も、同じフィールドの並びで始める。
fn frame_base_fields(frame: u32, resfn: u32) -> Vec<FieldType> {
    vec![
        field(val(I32), false),
        field(val(rv(false, resfn)), false),
        field(val(rv(true, frame)), true),
        field(val(I64), false),
    ]
}

/// import する関数の番号。
pub mod imp {
    pub const WRITE: u32 = 0;
    pub const BUF: u32 = 1;
    pub const ERR: u32 = 2;
    pub const FATAL: u32 = 3;
    pub const SUPPRESS: u32 = 4;
    pub const SHAPES_GROW: u32 = 5;
    pub const SIN: u32 = 6;
    pub const COS: u32 = 7;
    pub const EXP: u32 = 8;
    pub const LOG: u32 = 9;
    pub const F64_SHOW_BITS: u32 = 10;
    pub const SHAPE_LEN: u32 = 11;
    pub const SHAPE_LABELS: u32 = 12;
    pub const SHAPE_FIND: u32 = 13;
    pub const SHAPE_EXTEND: u32 = 14;
    pub const SHAPE_RESTRICT: u32 = 15;
    /// import する関数の個数。
    pub const COUNT: u32 = 16;
}

/// import するグローバルの番号。
pub mod gimp {
    /// シェイプ表のアドレス。ホストが表を拡張したら書き換える。
    pub const SHAPES: u32 = 0;
    /// `krt_scratch` のアドレス。
    pub const SCRATCH: u32 = 1;
    /// import するグローバルの個数。
    pub const COUNT: u32 = 2;
}

/// import する関数の表を返す。項目は、モジュール名、名前、引数の型、結果の型である。
fn imports() -> Vec<(&'static str, &'static str, Vec<ValType>, Vec<ValType>)> {
    vec![
        ("host", "write", vec![I32, I32, I32], vec![I32]),
        ("host", "buf", vec![I32], vec![I32]),
        ("host", "err", vec![I32, EQ, I64], vec![I32]),
        ("host", "fatal", vec![I32], vec![]),
        ("host", "suppress", vec![I32, I32], vec![]),
        ("host", "shapes_grow", vec![], vec![I32]),
        ("rt", "krt_sin", vec![F64], vec![F64]),
        ("rt", "krt_cos", vec![F64], vec![F64]),
        ("rt", "krt_exp", vec![F64], vec![F64]),
        ("rt", "krt_log", vec![F64], vec![F64]),
        ("rt", "krt_f64_show_bits", vec![I64, I32], vec![I32]),
        ("rt", "krt_shape_len", vec![I32, I32], vec![I32]),
        ("rt", "krt_shape_labels", vec![I32, I32], vec![I32]),
        ("rt", "krt_shape_find", vec![I32, I32, I32], vec![I32]),
        (
            "rt",
            "krt_shape_extend",
            vec![I32, I32, I32, I32],
            vec![I32],
        ),
        (
            "rt",
            "krt_shape_restrict",
            vec![I32, I32, I32, I32],
            vec![I32],
        ),
    ]
}

/// 関数の本体を作るビルダ。
///
/// ローカルの型は本体を書きながら追加し、最後に [`Function`] にまとめる。
pub struct Fb {
    nparams: u32,
    locals: Vec<ValType>,
    body: Vec<u8>,
}

impl Fb {
    /// 引数が `nparams` 個の関数の本体を作り始める。
    pub fn new(nparams: u32) -> Fb {
        Fb {
            nparams,
            locals: Vec::new(),
            body: Vec::new(),
        }
    }

    /// 型 `t` のローカルを追加し、その番号を返す。
    pub fn local(&mut self, t: ValType) -> u32 {
        self.locals.push(t);
        self.nparams + self.locals.len() as u32 - 1
    }

    /// 引数を除いたローカルの個数を返す。
    pub fn nlocals(&self) -> usize {
        self.locals.len()
    }

    /// 本体の末尾に命令を書き込む `InstructionSink` を返す。
    pub fn i(&mut self) -> InstructionSink<'_> {
        InstructionSink::new(&mut self.body)
    }

    /// 本体を `end` で閉じて [`Function`] にする。
    pub fn finish(mut self) -> Function {
        self.i().end();
        let mut f = Function::new_with_locals_types(self.locals);
        f.raw(self.body);
        f
    }
}

/// モジュールの中で定義する関数。
struct DefFunc {
    ty: u32,
    name: String,
    body: Option<Function>,
    /// trap することがある命令を置いてよい補助関数なら真。
    safe: bool,
}

/// モジュールの中で定義するグローバル。
struct DefGlobal {
    ty: GlobalType,
    init: ConstExpr,
    name: String,
}

/// 組み立て中のモジュール。
pub struct Mb {
    import_types: Vec<FuncType>,
    group: Vec<SubType>,
    sig_map: HashMap<(Vec<ValType>, Vec<ValType>), u32>,
    /// 固定の型の番号。
    pub t: Tys,
    fnty: HashMap<u32, (u32, u32)>,
    funcs: Vec<DefFunc>,
    globals: Vec<DefGlobal>,
    /// 受動的な(passive)データセグメントの内容。データセグメントはこの 1 つだけである。
    pub data: Vec<u8>,
    declared: BTreeSet<u32>,
    exports: Vec<(String, ExportKind, u32)>,
    import_func_types: Vec<u32>,
}

fn sub(final_: bool, sup: Option<u32>, inner: CompositeInnerType) -> SubType {
    SubType {
        is_final: final_,
        supertype_idxs: sup.into_iter().collect(),
        composite_type: CompositeType {
            inner,
            shared: false,
            descriptor: None,
            describes: None,
        },
    }
}

fn field(t: StorageType, mutable: bool) -> FieldType {
    FieldType {
        element_type: t,
        mutable,
    }
}

fn val(t: ValType) -> StorageType {
    StorageType::Val(t)
}

impl Mb {
    /// 空のモジュールを作り、固定の型を置く。
    pub fn new() -> Mb {
        let mut import_types: Vec<FuncType> = Vec::new();
        let mut import_func_types = Vec::new();
        let mut seen: HashMap<(Vec<ValType>, Vec<ValType>), u32> = HashMap::new();
        for (_, _, p, r) in imports() {
            let k = (p.clone(), r.clone());
            let idx = *seen.entry(k).or_insert_with(|| {
                import_types.push(FuncType::new(p, r));
                import_types.len() as u32 - 1
            });
            import_func_types.push(idx);
        }
        let base = import_types.len() as u32;
        let mut group: Vec<SubType> = Vec::new();
        let mut push = |s: SubType| {
            group.push(s);
            base + group.len() as u32 - 1
        };
        let str_ = push(sub(
            true,
            None,
            CompositeInnerType::Array(ArrayType(field(StorageType::I8, true))),
        ));
        let anyarr = push(sub(
            false,
            None,
            CompositeInnerType::Array(ArrayType(field(val(EQ), true))),
        ));
        let immarr = push(sub(
            true,
            Some(anyarr),
            CompositeInnerType::Array(ArrayType(field(val(EQ), true))),
        ));
        let mutarr = push(sub(
            true,
            Some(anyarr),
            CompositeInnerType::Array(ArrayType(field(val(EQ), true))),
        ));
        let i32arr = push(sub(
            true,
            None,
            CompositeInnerType::Array(ArrayType(field(val(I32), true))),
        ));
        let st = |fs: Vec<FieldType>| CompositeInnerType::Struct(StructType { fields: fs.into() });
        let box_i32 = push(sub(true, None, st(vec![field(val(I32), false)])));
        let box_i64 = push(sub(true, None, st(vec![field(val(I64), false)])));
        let box_f64 = push(sub(true, None, st(vec![field(val(F64), false)])));
        let unit = push(sub(true, None, st(vec![])));
        let bool_ = push(sub(true, None, st(vec![field(val(I32), false)])));
        let arr = rv(false, anyarr);
        let data = push(sub(
            true,
            None,
            st(vec![
                field(val(I32), false),
                field(val(I32), false),
                field(val(arr), false),
            ]),
        ));
        let variant = push(sub(
            true,
            None,
            st(vec![field(val(I32), false), field(val(EQ), false)]),
        ));
        let record = push(sub(
            true,
            None,
            st(vec![field(val(I32), false), field(val(arr), false)]),
        ));
        let clo = push(sub(
            false,
            None,
            st(vec![field(val(I32), false), field(val(arr), false)]),
        ));
        let method = push(sub(
            true,
            None,
            st(vec![
                field(val(I32), false),
                field(val(I32), false),
                field(val(EQ), false),
                field(val(arr), false),
            ]),
        ));
        let externu = push(sub(true, None, st(vec![field(val(I32), false)])));
        let ref_ = push(sub(true, None, st(vec![field(val(EQ), true)])));
        let dictb = push(sub(
            true,
            None,
            st(vec![field(val(I32), false), field(val(I32), false)]),
        ));
        let dicti = push(sub(
            true,
            None,
            st(vec![field(val(I32), false), field(val(arr), false)]),
        ));
        let iarr = rv(false, i32arr);
        let dictr = push(sub(
            true,
            None,
            st(vec![field(val(iarr), false), field(val(arr), false)]),
        ));
        let dictv = push(sub(
            true,
            None,
            st(vec![field(val(iarr), false), field(val(arr), false)]),
        ));
        let hr = push(sub(
            true,
            None,
            st(vec![
                field(val(I64), false),
                field(val(I32), false),
                field(val(EQ), false),
                field(val(EQ), false),
                field(val(iarr), false),
                field(val(iarr), false),
                field(val(arr), false),
                field(val(EQ), false),
                field(val(EQ), false),
                field(val(EQ), true),
                field(val(I64), true),
            ]),
        ));
        let saved = push(sub(
            true,
            None,
            st(vec![
                field(val(I32), false),
                field(val(EQ), false),
                field(val(EQ), false),
                field(val(I32), false),
                field(val(EQ), false),
                field(val(I32), false),
            ]),
        ));
        // 継続(フィールドの番号は `ctf`)。rec グループの中では、`saved` の直後に、
        // 継続、フレームの記録の基底型、再開用エントリの型がこの順に並ぶ。
        let frame = saved + 2;
        let resfn = saved + 3;
        let cont = push(sub(
            true,
            None,
            st(vec![
                field(val(EQ), false),
                field(val(rv(true, frame)), true),
                field(val(I64), false),
                field(val(I64), false),
                field(val(EQ), false),
                field(val(I32), false),
                field(val(EQ), false),
                field(val(I32), true),
                field(val(I32), true),
            ]),
        ));
        // フレームの記録の基底型(フィールドの番号は `frf`)と、再開用エントリの型。
        let f_ = push(sub(false, None, st(frame_base_fields(frame, resfn))));
        let r_ = push(sub(
            true,
            None,
            CompositeInnerType::Func(FuncType::new([rv(false, frame)], [EQ])),
        ));
        debug_assert_eq!((cont, f_, r_), (saved + 1, frame, resfn));
        let funcarr = push(sub(
            true,
            None,
            CompositeInnerType::Array(ArrayType(field(val(ValType::Ref(RefType::FUNCREF)), false))),
        ));
        Mb {
            import_types,
            group,
            sig_map: HashMap::new(),
            t: Tys {
                str_,
                anyarr,
                immarr,
                mutarr,
                i32arr,
                box_i32,
                box_i64,
                box_f64,
                unit,
                bool_,
                data,
                variant,
                record,
                clo,
                method,
                externu,
                ref_,
                dictb,
                dicti,
                dictr,
                dictv,
                hr,
                saved,
                cont,
                frame,
                resfn,
                funcarr,
            },
            fnty: HashMap::new(),
            funcs: Vec::new(),
            globals: Vec::new(),
            data: Vec::new(),
            declared: BTreeSet::new(),
            exports: Vec::new(),
            import_func_types,
        }
    }

    fn base(&self) -> u32 {
        self.import_types.len() as u32
    }

    /// rec グループに型を追加し、その番号を返す。
    pub fn add_type(&mut self, s: SubType) -> u32 {
        self.group.push(s);
        self.base() + self.group.len() as u32 - 1
    }

    /// フィールドがすべて可変である構造体の型を追加する。大きなフレーム方式の関数が使う。
    pub fn add_struct(&mut self, fields: &[ValType]) -> u32 {
        let fs: Vec<FieldType> = fields.iter().map(|t| field(val(*t), true)).collect();
        self.add_type(sub(
            true,
            None,
            CompositeInnerType::Struct(StructType { fields: fs.into() }),
        ))
    }

    /// 関数ごとのフレームの記録の型を追加する。
    ///
    /// 型は `$Frame` の部分型で、基底型のフィールドの後ろに、`fields` を不変のフィールドとして置く。
    pub fn add_frame_record(&mut self, fields: &[ValType]) -> u32 {
        let mut fs = frame_base_fields(self.t.frame, self.t.resfn);
        fs.extend(fields.iter().map(|t| field(val(*t), false)));
        self.add_type(sub(
            true,
            Some(self.t.frame),
            CompositeInnerType::Struct(StructType { fields: fs.into() }),
        ))
    }

    /// 関数の型を rec グループに追加し、その番号を返す。同じ型は 1 つにまとめる。
    pub fn sig(&mut self, params: &[ValType], results: &[ValType]) -> u32 {
        let k = (params.to_vec(), results.to_vec());
        if let Some(&i) = self.sig_map.get(&k) {
            return i;
        }
        let i = self.add_type(sub(
            true,
            None,
            CompositeInnerType::Func(FuncType::new(params.to_vec(), results.to_vec())),
        ));
        self.sig_map.insert(k, i);
        i
    }

    /// LIR の `n` 個の引数を渡すときの、Wasm の引数の型を返す。
    ///
    /// `n` が [`crate::P_DIRECT`] 以下なら `eqref × n` で、超えるなら `(eqref × P_DIRECT) (ref $anyarr)` である。
    pub fn arg_types(&self, n: u32) -> Vec<ValType> {
        let p = crate::P_DIRECT;
        if n > p {
            let mut v = vec![EQ; p as usize];
            v.push(rv(false, self.t.anyarr));
            v
        } else {
            vec![EQ; n as usize]
        }
    }

    /// 引数が `n` 個の LIR の関数の型 `$FnN` と、そのクロージャの型 `$CloN` を返す。
    ///
    /// `$FnN` は `(env (ref null $Clo)) (引数) (frame (ref null $Frame)) → eqref` である。
    /// 引数の型は [`Mb::arg_types`] が決める。
    pub fn fn_types(&mut self, n: u32) -> (u32, u32) {
        if let Some(&p) = self.fnty.get(&n) {
            return p;
        }
        let mut params = vec![rv(true, self.t.clo)];
        params.extend(self.arg_types(n));
        params.push(rv(true, self.t.frame));
        let f = self.add_type(sub(
            true,
            None,
            CompositeInnerType::Func(FuncType::new(params, vec![EQ])),
        ));
        let arr = rv(false, self.t.anyarr);
        let c = self.add_type(sub(
            true,
            Some(self.t.clo),
            CompositeInnerType::Struct(StructType {
                fields: vec![
                    field(val(I32), false),
                    field(val(arr), false),
                    field(val(rv(false, f)), false),
                ]
                .into(),
            }),
        ));
        self.fnty.insert(n, (f, c));
        (f, c)
    }

    /// 関数を宣言して、その番号を返す。本体は後で [`Mb::set_body`] で置く。
    ///
    /// `safe` が真なら、trap することがある命令を置いてよい補助関数として、名前に `safe.` を付ける。
    pub fn declare_func(&mut self, ty: u32, name: String, safe: bool) -> u32 {
        self.funcs.push(DefFunc {
            ty,
            name,
            body: None,
            safe,
        });
        imp::COUNT + self.funcs.len() as u32 - 1
    }

    /// 関数の本体を置く。
    pub fn set_body(&mut self, idx: u32, f: Function) {
        self.funcs[(idx - imp::COUNT) as usize].body = Some(f);
    }

    /// 関数の名前を返す。
    pub fn func_name(&self, idx: u32) -> &str {
        &self.funcs[(idx - imp::COUNT) as usize].name
    }

    /// `ref.func` で参照する関数を宣言する。
    pub fn declare_ref(&mut self, f: u32) {
        self.declared.insert(f);
    }

    /// グローバルを追加し、その番号を返す。
    pub fn global(&mut self, t: ValType, mutable: bool, init: ConstExpr, name: &str) -> u32 {
        self.globals.push(DefGlobal {
            ty: GlobalType {
                val_type: t,
                mutable,
                shared: false,
            },
            init,
            name: name.to_string(),
        });
        gimp::COUNT + self.globals.len() as u32 - 1
    }

    /// export を追加する。
    pub fn export(&mut self, name: &str, kind: ExportKind, idx: u32) {
        self.exports.push((name.to_string(), kind, idx));
    }

    /// データセグメントにバイト列を追記し、その先頭のオフセットを返す。
    pub fn add_data(&mut self, bytes: &[u8]) -> u32 {
        let off = self.data.len() as u32;
        self.data.extend_from_slice(bytes);
        off
    }

    /// 関数の個数(import する関数を含む)を返す。
    pub fn nfuncs(&self) -> u32 {
        imp::COUNT + self.funcs.len() as u32
    }

    /// 組み立てたモジュールをバイト列にする。
    pub fn finish(self) -> Vec<u8> {
        let mut m = Module::new();
        let mut types = TypeSection::new();
        for t in &self.import_types {
            types
                .ty()
                .function(t.params().to_vec(), t.results().to_vec());
        }
        types.ty().rec(self.group);
        m.section(&types);

        let mut imps = ImportSection::new();
        for (i, (module, name, _, _)) in imports().into_iter().enumerate() {
            imps.import(
                module,
                name,
                EntityType::Function(self.import_func_types[i]),
            );
        }
        imps.import(
            "rt",
            "memory",
            EntityType::Memory(MemoryType {
                minimum: 0,
                maximum: None,
                memory64: false,
                shared: false,
                page_size_log2: None,
            }),
        );
        imps.import(
            "host",
            "shapes",
            EntityType::Global(GlobalType {
                val_type: I32,
                mutable: true,
                shared: false,
            }),
        );
        imps.import(
            "host",
            "scratch",
            EntityType::Global(GlobalType {
                val_type: I32,
                mutable: false,
                shared: false,
            }),
        );
        m.section(&imps);

        let mut fs = FunctionSection::new();
        for f in &self.funcs {
            fs.function(f.ty);
        }
        m.section(&fs);

        let mut gs = GlobalSection::new();
        for g in &self.globals {
            gs.global(g.ty, &g.init);
        }
        m.section(&gs);

        let mut ex = ExportSection::new();
        for (n, k, i) in &self.exports {
            ex.export(n, *k, *i);
        }
        m.section(&ex);

        if !self.declared.is_empty() {
            let mut es = ElementSection::new();
            let v: Vec<u32> = self.declared.iter().copied().collect();
            es.declared(Elements::Functions(v.into()));
            m.section(&es);
        }

        m.section(&DataCountSection { count: 1 });

        let mut code = CodeSection::new();
        let mut names = NameMap::new();
        for (i, (module, name, _, _)) in imports().into_iter().enumerate() {
            names.append(i as u32, &format!("{module}.{name}"));
        }
        for (i, f) in self.funcs.iter().enumerate() {
            let body = f.body.as_ref().expect("本体の無い関数");
            code.function(body);
            let prefix = if f.safe { "safe." } else { "" };
            names.append(imp::COUNT + i as u32, &format!("{prefix}{}", f.name));
        }
        m.section(&code);

        let mut ds = DataSection::new();
        ds.passive(self.data.iter().copied());
        m.section(&ds);

        let mut ns = NameSection::new();
        ns.functions(&names);
        let mut gnames = NameMap::new();
        gnames.append(gimp::SHAPES, "host.shapes");
        gnames.append(gimp::SCRATCH, "host.scratch");
        for (i, g) in self.globals.iter().enumerate() {
            gnames.append(gimp::COUNT + i as u32, &g.name);
        }
        ns.globals(&gnames);
        let _ = IndirectNameMap::new();
        m.section(&ns);
        m.finish()
    }
}

impl Default for Mb {
    fn default() -> Self {
        Mb::new()
    }
}
