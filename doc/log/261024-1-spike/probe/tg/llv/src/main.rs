use llvm_sys::orc2::lljit::*;
use llvm_sys::target::*;
fn main() { unsafe {
    LLVM_InitializeNativeTarget(); LLVM_InitializeNativeAsmPrinter();
    let mut jit = std::ptr::null_mut();
    let err = LLVMOrcCreateLLJIT(&mut jit, std::ptr::null_mut());
    println!("LLJIT created: err_null={} triple={:?}", err.is_null(), std::ffi::CStr::from_ptr(LLVMOrcLLJITGetTripleString(jit)));
}}
