//! IRSpec の付録 A が定める組み込みの表。
//!
//! 各表の項目は付録 A の順に並べる。
//! この順序は、テキストとバイナリの表での順序(IRSpec 12.4 節)でもある。
//! シンボルは、接頭辞 `builtin/` を付けない名前で持つ。

/// A.1 組み込みの型。付録の順に並べる。
pub const TYPES: [&str; 9] = [
    "Boolean",
    "Int32",
    "Int64",
    "Float64",
    "String",
    "Never",
    "Ref",
    "Array",
    "MutableArray",
];

/// A.2 組み込みのクラスと、そのメソッド。付録の順に並べる。
///
/// どのメソッドも、メソッド自身の辞書(IRSpec 7.2 節)の個数は 0 である。
pub const CLASSES: [(&str, &[&str]); 9] = [
    ("Add", &["add"]),
    ("Sub", &["sub"]),
    ("Mul", &["mul"]),
    ("Div", &["div"]),
    ("Rem", &["rem"]),
    ("Neg", &["neg"]),
    ("Eq", &["eq"]),
    ("Ord", &["lt", "le", "gt", "ge"]),
    ("Show", &["show"]),
];

/// A.3 組み込みのインスタンスのメソッド。項目は (クラス, 型, メソッド, 実装のプリミティブ) の組である。
///
/// 実装がプリミティブでないメソッドは、4 番目が `None` である。
pub const METHODS: [(&str, &str, &str, Option<&str>); 44] = [
    ("Add", "Int32", "add", Some("__int32_add")),
    ("Add", "Int64", "add", Some("__int64_add")),
    ("Add", "Float64", "add", Some("__float64_add")),
    ("Add", "String", "add", Some("__string_concat")),
    ("Sub", "Int32", "sub", Some("__int32_sub")),
    ("Sub", "Int64", "sub", Some("__int64_sub")),
    ("Sub", "Float64", "sub", Some("__float64_sub")),
    ("Mul", "Int32", "mul", Some("__int32_mul")),
    ("Mul", "Int64", "mul", Some("__int64_mul")),
    ("Mul", "Float64", "mul", Some("__float64_mul")),
    ("Div", "Int32", "div", Some("__int32_div")),
    ("Div", "Int64", "div", Some("__int64_div")),
    ("Div", "Float64", "div", Some("__float64_div")),
    ("Rem", "Int32", "rem", Some("__int32_rem")),
    ("Rem", "Int64", "rem", Some("__int64_rem")),
    ("Neg", "Int32", "neg", Some("__int32_neg")),
    ("Neg", "Int64", "neg", Some("__int64_neg")),
    ("Neg", "Float64", "neg", Some("__float64_neg")),
    ("Eq", "Int32", "eq", Some("__int32_eq")),
    ("Eq", "Int64", "eq", Some("__int64_eq")),
    ("Eq", "Float64", "eq", Some("__float64_eq")),
    ("Eq", "String", "eq", Some("__string_eq")),
    ("Eq", "Boolean", "eq", None),
    ("Ord", "Int32", "lt", Some("__int32_lt")),
    ("Ord", "Int32", "le", Some("__int32_le")),
    ("Ord", "Int32", "gt", Some("__int32_gt")),
    ("Ord", "Int32", "ge", Some("__int32_ge")),
    ("Ord", "Int64", "lt", Some("__int64_lt")),
    ("Ord", "Int64", "le", Some("__int64_le")),
    ("Ord", "Int64", "gt", Some("__int64_gt")),
    ("Ord", "Int64", "ge", Some("__int64_ge")),
    ("Ord", "Float64", "lt", Some("__float64_lt")),
    ("Ord", "Float64", "le", Some("__float64_le")),
    ("Ord", "Float64", "gt", Some("__float64_gt")),
    ("Ord", "Float64", "ge", Some("__float64_ge")),
    ("Ord", "String", "lt", Some("__string_lt")),
    ("Ord", "String", "le", Some("__string_le")),
    ("Ord", "String", "gt", Some("__string_gt")),
    ("Ord", "String", "ge", Some("__string_ge")),
    ("Show", "Int32", "show", Some("__show_int32")),
    ("Show", "Int64", "show", None),
    ("Show", "Float64", "show", None),
    ("Show", "String", "show", None),
    ("Show", "Boolean", "show", None),
];

/// A.4 組み込みの値。付録の順に並べる。
pub const VALUES: [&str; 12] = [
    "Ref.new",
    "Ref.get",
    "Ref.set",
    "Array.length",
    "Array.get",
    "Array.each",
    "MutableArray.new",
    "MutableArray.length",
    "MutableArray.get",
    "MutableArray.set",
    "MutableArray.freeze",
    "pinned",
];

/// A.5 `extern "prim"` の実装名。付録の順に並べる。
pub const PRIMS: [&str; 48] = [
    "__int32_add",
    "__int32_sub",
    "__int32_mul",
    "__int32_div",
    "__int32_rem",
    "__int32_neg",
    "__int32_eq",
    "__int32_lt",
    "__int32_le",
    "__int32_gt",
    "__int32_ge",
    "__int64_add",
    "__int64_sub",
    "__int64_mul",
    "__int64_div",
    "__int64_rem",
    "__int64_neg",
    "__int64_eq",
    "__int64_lt",
    "__int64_le",
    "__int64_gt",
    "__int64_ge",
    "__float64_add",
    "__float64_sub",
    "__float64_mul",
    "__float64_div",
    "__float64_neg",
    "__float64_eq",
    "__float64_lt",
    "__float64_le",
    "__float64_gt",
    "__float64_ge",
    "__string_concat",
    "__string_eq",
    "__string_lt",
    "__string_le",
    "__string_gt",
    "__string_ge",
    "__string_length",
    "__string_sub",
    "__i32_to_i64",
    "__i32_to_f64",
    "__i64_to_i32",
    "__i64_to_f64",
    "__f64_to_i32",
    "__f64_to_i64",
    "__show_int32",
    "__panic",
];

/// A.5 `extern "C"` の実装名。付録の順に並べる。
pub const C_FUNCTIONS: [&str; 5] = ["sin", "cos", "sqrt", "exp", "log"];

/// クラスと型の組が A.3 の組み込みのインスタンスにあれば真を返す。
///
/// クラスと型は、接頭辞 `builtin/` を付けない名前で指定する。
pub fn is_builtin_instance(class: &str, ty: &str) -> bool {
    METHODS.iter().any(|&(c, t, _, _)| c == class && t == ty)
}
