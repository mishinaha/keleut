// WasmGC spike: assemble wat/*.wat with the `wat` crate, validate with
// `wasmparser`, and build one module directly with the `wasm-encoder` API.
use std::{borrow::Cow, fs, path::Path};

use wasm_encoder::{
    AbstractHeapType, ArrayType, BlockType, Catch, CodeSection, CompositeInnerType, CompositeType,
    ContType, ExportKind, ExportSection, FieldType, Function, FunctionSection, Handle, HeapType,
    Instruction as I, Module, RefType, StorageType, StructType, SubType, TagKind, TagSection,
    TagType, TypeSection, ValType,
};
use wasmparser::{Validator, WasmFeatures};

fn validate(name: &str, bytes: &[u8], features: WasmFeatures) {
    let mut v = Validator::new_with_features(features);
    match v.validate_all(bytes) {
        Ok(_) => println!("{name}: wasmparser validate OK ({} bytes)", bytes.len()),
        Err(e) => println!("{name}: wasmparser validate ERROR: {e}"),
    }
}

fn sub(inner: CompositeInnerType) -> SubType {
    SubType {
        is_final: true,
        supertype_idxs: vec![],
        composite_type: CompositeType { inner, shared: false, descriptor: None, describes: None },
    }
}

/// Module built with the wasm-encoder API only:
///   type 0 = (array (mut i8)), type 1 = (struct (field i32) (field (ref null 0)))
///   type 2 = (func (param i32) (result i32)), tag 0 : (param i32)
///   func 0 "enc_main" (param i32) (result i32):
///     try_table (catch tag0 -> block) { if p>10 throw tag0(p) ; return_call_ref f1 }
///   func 1 : array.len of a struct-held byte array + p
fn build_encoder_module() -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().subtype(&sub(CompositeInnerType::Array(ArrayType(FieldType {
        element_type: StorageType::I8,
        mutable: true,
    }))));
    let bytes_ref = ValType::Ref(RefType { nullable: true, heap_type: HeapType::Concrete(0) });
    types.ty().subtype(&sub(CompositeInnerType::Struct(StructType {
        fields: Box::new([
            FieldType { element_type: StorageType::Val(ValType::I32), mutable: false },
            FieldType { element_type: StorageType::Val(bytes_ref), mutable: false },
        ]),
    })));
    types.ty().function([ValType::I32], [ValType::I32]); // type 2
    types.ty().function([ValType::I32], []); // type 3 (tag)

    let mut funcs = FunctionSection::new();
    funcs.function(2);
    funcs.function(2);
    let mut tags = TagSection::new();
    tags.tag(TagType { kind: TagKind::Exception, func_type_idx: 3 });
    let mut exports = ExportSection::new();
    exports.export("enc_main", ExportKind::Func, 0);

    let mut code = CodeSection::new();
    // func 0
    let mut f = Function::new([]);
    f.instruction(&I::Block(BlockType::Result(ValType::I32)));
    f.instruction(&I::TryTable(BlockType::Result(ValType::I32), Cow::Owned(vec![Catch::One { tag: 0, label: 0 }])));
    f.instruction(&I::LocalGet(0));
    f.instruction(&I::I32Const(10));
    f.instruction(&I::I32GtS);
    f.instruction(&I::If(BlockType::Empty));
    f.instruction(&I::LocalGet(0));
    f.instruction(&I::Throw(0));
    f.instruction(&I::End);
    f.instruction(&I::LocalGet(0));
    f.instruction(&I::RefFunc(1));
    f.instruction(&I::ReturnCallRef(2));
    f.instruction(&I::End); // try_table
    f.instruction(&I::End); // block: caught value (i32) on stack
    f.instruction(&I::I32Const(1000));
    f.instruction(&I::I32Add);
    f.instruction(&I::End);
    code.function(&f);
    // func 1
    let mut g = Function::new([]);
    g.instruction(&I::I32Const(1));
    g.instruction(&I::I32Const(0));
    g.instruction(&I::I32Const(5));
    g.instruction(&I::ArrayNew(0));
    g.instruction(&I::StructNew(1));
    g.instruction(&I::StructGet { struct_type_index: 1, field_index: 1 });
    g.instruction(&I::ArrayLen);
    g.instruction(&I::LocalGet(0));
    g.instruction(&I::I32Add);
    g.instruction(&I::RefI31);
    g.instruction(&I::I31GetS);
    g.instruction(&I::End);
    code.function(&g);

    // ref.func 1 needs a declarative element segment
    let mut elems = wasm_encoder::ElementSection::new();
    elems.declared(wasm_encoder::Elements::Functions(Cow::Borrowed(&[1])));

    let mut m = Module::new();
    m.section(&types).section(&funcs).section(&tags).section(&exports).section(&elems).section(&code);
    let _ = (AbstractHeapType::Any, Handle::OnLabel { tag: 0, label: 0 });
    m.finish()
}

/// Same generator as wat/fx.wat but emitted with wasm-encoder, to show the
/// stack-switching instruction encoding the crate produces.
fn build_fx_encoder_module() -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([], []); // 0 ft
    types.ty().subtype(&sub(CompositeInnerType::Cont(ContType(0)))); // 1 ct
    types.ty().function([ValType::I32], []); // 2 tag type
    types.ty().function([ValType::I32], [ValType::I32]); // 3 sum
    let mut funcs = FunctionSection::new();
    funcs.function(0); // gen
    funcs.function(3); // sum
    let mut tags = TagSection::new();
    tags.tag(TagType { kind: TagKind::Exception, func_type_idx: 2 });
    let mut globals = wasm_encoder::GlobalSection::new();
    globals.global(
        wasm_encoder::GlobalType { val_type: ValType::I32, mutable: true, shared: false },
        &wasm_encoder::ConstExpr::i32_const(0),
    );
    let mut exports = ExportSection::new();
    exports.export("sum", ExportKind::Func, 1);
    let mut elems = wasm_encoder::ElementSection::new();
    elems.declared(wasm_encoder::Elements::Functions(Cow::Borrowed(&[0])));
    let mut code = CodeSection::new();
    // gen: local0 i
    let mut f = Function::new([(1, ValType::I32)]);
    f.instruction(&I::Loop(BlockType::Empty));
    f.instruction(&I::LocalGet(0)).instruction(&I::I32Const(1)).instruction(&I::I32Add).instruction(&I::LocalSet(0));
    f.instruction(&I::LocalGet(0)).instruction(&I::Suspend(0));
    f.instruction(&I::LocalGet(0)).instruction(&I::GlobalGet(0)).instruction(&I::I32LtU).instruction(&I::BrIf(0));
    f.instruction(&I::End).instruction(&I::End);
    code.function(&f);
    // sum: param0 n, local1 k (ref null ct), local2 acc
    let ct_ref = RefType { nullable: true, heap_type: HeapType::Concrete(1) };
    let mut s = Function::new([(1, ValType::Ref(ct_ref)), (1, ValType::I32)]);
    s.instruction(&I::LocalGet(0)).instruction(&I::GlobalSet(0));
    s.instruction(&I::RefFunc(0)).instruction(&I::ContNew(1)).instruction(&I::LocalSet(1));
    s.instruction(&I::Loop(BlockType::Empty));
    // block type (i32, ref ct) -> needs a func type; add as type 4
    s.instruction(&I::Block(BlockType::FunctionType(4)));
    s.instruction(&I::LocalGet(1));
    s.instruction(&I::Resume { cont_type_index: 1, resume_table: Cow::Owned(vec![Handle::OnLabel { tag: 0, label: 0 }]) });
    s.instruction(&I::LocalGet(2)).instruction(&I::Return);
    s.instruction(&I::End);
    s.instruction(&I::LocalSet(1));
    s.instruction(&I::LocalGet(2)).instruction(&I::I32Add).instruction(&I::LocalSet(2));
    s.instruction(&I::Br(0));
    s.instruction(&I::End);
    s.instruction(&I::Unreachable).instruction(&I::End);
    code.function(&s);
    types.ty().function([], [ValType::I32, ValType::Ref(RefType { nullable: false, heap_type: HeapType::Concrete(1) })]); // 4
    let mut m = Module::new();
    m.section(&types).section(&funcs).section(&tags).section(&globals).section(&exports).section(&elems).section(&code);
    m.finish()
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("out");
    fs::create_dir_all(&out).unwrap();
    let base = WasmFeatures::default();
    let fx = base | WasmFeatures::STACK_SWITCHING;
    let mut entries: Vec<_> = fs::read_dir(root.join("wat")).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_stem().unwrap().to_str().unwrap().to_string();
        match wat::parse_file(&p) {
            Ok(bytes) => {
                validate(&name, &bytes, base);
                if name.starts_with("fx") {
                    validate(&format!("{name} (+stack_switching)"), &bytes, fx);
                }
                fs::write(out.join(format!("{name}.wasm")), bytes).unwrap();
            }
            Err(e) => println!("{name}: wat parse ERROR: {e}"),
        }
    }
    let enc = build_encoder_module();
    validate("encoder", &enc, base);
    validate("encoder (+stack_switching)", &enc, fx);
    fs::write(out.join("encoder.wasm"), enc).unwrap();
    let fxe = build_fx_encoder_module();
    validate("fx_encoder (+stack_switching)", &fxe, fx);
    fs::write(out.join("fx_encoder.wasm"), fxe).unwrap();
}
