//! wasmparser による検証と、trap の検査。
//!
//! 検証は wasmparser の既定の機能集合で行う。
//! この集合は GC、tail_call、exceptions を含み、stack_switching を含まない。
//!
//! trap の検査では、trap することがある命令が補助関数の外にないことを調べる。
//! 補助関数は、名前セクションでの名前が `safe.` で始まる関数である。
//! 対象の命令は次のとおりである([`forbidden`])。
//!
//! - 整数の除算と剰余
//! - 浮動小数点数から整数への変換のうち、trap するもの(飽和する `trunc_sat` は対象外)
//! - 配列の生成(`array.new`、`array.new_default`、`array.new_data`、`array.new_elem`)、
//!   要素の読み書き、`array.copy`、`array.fill`、`array.init_data`、`array.init_elem`
//! - 線形メモリの読み書き、`memory.copy`、`memory.fill`、`memory.init`、アトミック命令
//! - 表の読み書き、`table.copy`、`table.init`、`table.fill`、`call_indirect`、`return_call_indirect`
//!
//! これらは添字や長さが範囲外だと trap するので、補助関数が先に範囲を検査する。
//! `ref.cast`、`ref.as_non_null`、`unreachable`、null になることがある参照への `struct.get|set`、`array.len`、`call_ref` は、
//! 型が健全であるかぎり失敗しないので、補助関数の外に置いてよい。
//! これらが trap したら処理系のバグであり、終了コード 70 で終了する。

use std::collections::HashMap;

use wasmparser::{KnownCustom, Name, Parser, Payload, Validator, WasmFeatures};

/// モジュールのバイト列を wasmparser で検証する。失敗したらエラーメッセージを返す。
pub fn validate(bytes: &[u8]) -> Result<(), String> {
    let mut v = Validator::new_with_features(WasmFeatures::default());
    v.validate_all(bytes)
        .map(|_| ())
        .map_err(|e| format!("wasmparser の検証に失敗しました: {e}"))
}

/// 命令の名前 `name` が、補助関数の外に置けない命令のものなら真を返す。
///
/// 名前は、`Operator` を `{:?}` で文字列化したときの、先頭の英数字の並びである。
pub fn forbidden(name: &str) -> bool {
    const LIST: &[&str] = &[
        "I32DivS",
        "I32DivU",
        "I32RemS",
        "I32RemU",
        "I64DivS",
        "I64DivU",
        "I64RemS",
        "I64RemU",
        "I32TruncF32S",
        "I32TruncF32U",
        "I32TruncF64S",
        "I32TruncF64U",
        "I64TruncF32S",
        "I64TruncF32U",
        "I64TruncF64S",
        "I64TruncF64U",
        "ArrayGet",
        "ArrayGetS",
        "ArrayGetU",
        "ArraySet",
        "ArrayNew",
        "ArrayNewDefault",
        "ArrayNewData",
        "ArrayNewElem",
        "ArrayCopy",
        "ArrayFill",
        "ArrayInitData",
        "ArrayInitElem",
        "MemoryCopy",
        "MemoryFill",
        "MemoryInit",
        "TableGet",
        "TableSet",
        "TableCopy",
        "TableInit",
        "TableFill",
        "CallIndirect",
        "ReturnCallIndirect",
    ];
    LIST.contains(&name)
        || name.contains("Load")
        || name.contains("Store")
        || name.contains("Atomic")
}

/// trap の検査が調べた関数と命令の個数。
#[derive(Debug, Default)]
pub struct Report {
    /// 調べた関数の個数。
    pub funcs: usize,
    /// そのうち、補助関数の個数。
    pub safe: usize,
    /// 調べた命令の個数(補助関数の命令は数えない)。
    pub ops: usize,
}

/// trap の検査を行う。
///
/// trap することがある命令が補助関数の外にあれば、その関数と命令を書いたエラーメッセージを返す。
pub fn trap_check(bytes: &[u8]) -> Result<Report, String> {
    let mut nimports = 0u32;
    let mut bodies = Vec::new();
    let mut names: HashMap<u32, String> = HashMap::new();
    for p in Parser::new(0).parse_all(bytes) {
        match p.map_err(|e| e.to_string())? {
            Payload::ImportSection(r) => {
                for imp in r.into_imports() {
                    let imp = imp.map_err(|e| e.to_string())?;
                    if matches!(imp.ty, wasmparser::TypeRef::Func(_)) {
                        nimports += 1;
                    }
                }
            }
            Payload::CodeSectionEntry(b) => bodies.push(b),
            Payload::CustomSection(c) => {
                if let KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let Name::Function(m) = n.map_err(|e| e.to_string())? {
                            for e in m {
                                let e = e.map_err(|e| e.to_string())?;
                                names.insert(e.index, e.name.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut rep = Report::default();
    for (i, b) in bodies.iter().enumerate() {
        let idx = nimports + i as u32;
        let name = names.get(&idx).map(String::as_str).unwrap_or("");
        rep.funcs += 1;
        if name.starts_with("safe.") {
            rep.safe += 1;
            continue;
        }
        let mut ops = b.get_operators_reader().map_err(|e| e.to_string())?;
        while !ops.eof() {
            let op = ops.read().map_err(|e| e.to_string())?;
            rep.ops += 1;
            let s = format!("{op:?}");
            let head = s
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or("");
            if forbidden(head) {
                return Err(format!(
                    "trap しうる命令 {head} が許可した補助関数の外(関数 {idx} {name})にあります"
                ));
            }
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 命令の分け方() {
        for n in [
            "I32DivS",
            "ArrayGet",
            "I32Load8U",
            "I64Store",
            "MemoryCopy",
            "CallIndirect",
        ] {
            assert!(forbidden(n), "{n}");
        }
        for n in [
            "I32Add",
            "RefCast",
            "StructGet",
            "ArrayLen",
            "CallRef",
            "ArrayNewFixed",
            "I32TruncSatF64S",
        ] {
            assert!(!forbidden(n), "{n}");
        }
    }

    /// `i32.div_s` を含む関数を 1 つだけ持つモジュールを作る。関数の名前は `name` にする。
    fn module_with_div(name: &str) -> Vec<u8> {
        use wasm_encoder::{
            CodeSection, Function, FunctionSection, Module, NameMap, NameSection, TypeSection,
            ValType,
        };
        let mut m = Module::new();
        let mut ty = TypeSection::new();
        ty.ty()
            .function([ValType::I32, ValType::I32], [ValType::I32]);
        m.section(&ty);
        let mut fs = FunctionSection::new();
        fs.function(0);
        m.section(&fs);
        let mut code = CodeSection::new();
        let mut f = Function::new([]);
        f.instructions().local_get(0).local_get(1).i32_div_s().end();
        code.function(&f);
        m.section(&code);
        let mut names = NameMap::new();
        names.append(0, name);
        let mut ns = NameSection::new();
        ns.functions(&names);
        m.section(&ns);
        m.finish()
    }

    #[test]
    fn 補助関数の外の除算を見つける() {
        let bad = module_with_div("f");
        validate(&bad).unwrap();
        let e = trap_check(&bad).unwrap_err();
        assert!(e.contains("I32DivS"), "{e}");
        let ok = module_with_div("safe.div");
        let r = trap_check(&ok).unwrap();
        assert_eq!((r.funcs, r.safe), (1, 1));
    }

    /// `rt.wasm` も既定の機能集合で検証に通る。
    ///
    /// `rt.wasm` は C で書いた線形メモリのコードなので、trap の検査は適用しない。
    #[test]
    fn rt_wasm_は検証に通る() {
        validate(keleut_rtcore::RT_WASM).unwrap();
    }
}
