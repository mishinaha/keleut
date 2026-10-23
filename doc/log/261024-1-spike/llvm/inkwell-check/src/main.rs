use inkwell::{OptimizationLevel, context::Context};
fn main() {
    let ctx = Context::create();
    let m = ctx.create_module("m");
    let i64t = ctx.i64_type();
    let f = m.add_function("add1", i64t.fn_type(&[i64t.into()], false), None);
    let b = ctx.create_builder();
    b.position_at_end(ctx.append_basic_block(f, "e"));
    let x = f.get_nth_param(0).unwrap().into_int_value();
    b.build_return(Some(&b.build_int_add(x, i64t.const_int(1, false), "r").unwrap())).unwrap();
    let ee = m.create_jit_execution_engine(OptimizationLevel::Default).unwrap();
    let add1 = unsafe { ee.get_function::<unsafe extern "C" fn(i64) -> i64>("add1").unwrap() };
    println!("inkwell MCJIT add1(41) = {}", unsafe { add1.call(41) });
}
