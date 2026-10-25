//! llvm-sys の薄い包み(計画 261024-1 の 6.8 節)。
//!
//! LLJIT(ORCv2)を作り、Rust の関数の番地を絶対の記号として登録し、LLVM IR の文面のモジュールを足して翻訳する。
//! モジュール 1 つに文脈 1 つを使う。
//! 文脈は `LLVMContextCreate` で作り、`LLVMOrcCreateNewThreadSafeContextFromLLVMContext` で包む
//! (LLVM 21 以降の C API には `LLVMOrcThreadSafeContextGetContext` が無い)。
//!
//! この包みのどの値も、作ったスレッドの中だけで使う(JIT の翻訳のスレッド。[`crate`] の冒頭)。

use std::ffi::{CStr, CString};
use std::ptr::null_mut;

use llvm_sys::analysis::{LLVMVerifierFailureAction, LLVMVerifyModule};
use llvm_sys::core::*;
use llvm_sys::error::*;
use llvm_sys::ir_reader::LLVMParseIRInContext2;
use llvm_sys::orc2::lljit::*;
use llvm_sys::orc2::*;
use llvm_sys::prelude::LLVMModuleRef;
use llvm_sys::transforms::pass_builder::*;

/// LLVM の誤りを文字列にして捨てる。
///
/// # Safety
///
/// `e` は空でない LLVM の誤りで、ここで所有権を受け取る。
unsafe fn err_string(e: LLVMErrorRef) -> String {
    // SAFETY: 呼び出し側の約束による。文面は LLVMDisposeErrorMessage で返す。
    unsafe {
        let m = LLVMGetErrorMessage(e);
        let s = CStr::from_ptr(m).to_string_lossy().into_owned();
        LLVMDisposeErrorMessage(m);
        s
    }
}

/// LLVM の誤りを `Result` にする。
///
/// # Safety
///
/// `e` は LLVM の関数が返した誤り(空なら成功)である。
unsafe fn check(e: LLVMErrorRef) -> Result<(), String> {
    if e.is_null() {
        Ok(())
    } else {
        // SAFETY: 呼び出し側の約束による。
        Err(unsafe { err_string(e) })
    }
}

/// LLJIT の 1 つの実体。
pub struct Jit {
    j: LLVMOrcLLJITRef,
}

impl Jit {
    /// 既定の設定で LLJIT を作る(対象はこの機械)。
    pub fn new() -> Result<Jit, String> {
        static INIT: std::sync::Once = std::sync::Once::new();
        let mut failed = false;
        INIT.call_once(|| {
            // SAFETY: この機械の対象と、機械語を書く部品を登録するだけである。
            unsafe {
                failed = llvm_sys::target::LLVM_InitializeNativeTarget() != 0
                    || llvm_sys::target::LLVM_InitializeNativeAsmPrinter() != 0;
            }
        });
        if failed {
            return Err("LLVM のこの機械の対象を登録できません".into());
        }
        let mut j = null_mut();
        // SAFETY: 設定を渡さずに作る。成功すれば j は LLJIT を指す。
        unsafe { check(LLVMOrcCreateLLJIT(&mut j, null_mut()))? };
        Ok(Jit { j })
    }

    /// 対象の三つ組。
    pub fn triple(&self) -> String {
        // SAFETY: j は生きている LLJIT で、文字列は LLJIT が持つ。
        unsafe {
            CStr::from_ptr(LLVMOrcLLJITGetTripleString(self.j))
                .to_string_lossy()
                .into_owned()
        }
    }

    /// データの配置の文字列。
    pub fn data_layout(&self) -> String {
        // SAFETY: j は生きている LLJIT で、文字列は LLJIT が持つ。
        unsafe {
            CStr::from_ptr(LLVMOrcLLJITGetDataLayoutStr(self.j))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn jd(&self) -> LLVMOrcJITDylibRef {
        // SAFETY: j は生きている LLJIT である。
        unsafe { LLVMOrcLLJITGetMainJITDylib(self.j) }
    }

    /// Rust の関数の番地を、呼べる絶対の記号として主の JITDylib に登録する。
    pub fn define_absolute(&self, syms: &[(&str, usize)]) -> Result<(), String> {
        let names: Vec<CString> = syms
            .iter()
            .map(|(n, _)| CString::new(*n).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        // SAFETY: j は生きている LLJIT で、名前は呼び出しの間生きている。
        unsafe {
            let flags = LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsExported as u8
                | LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsCallable as u8;
            let mut pairs: Vec<LLVMOrcCSymbolMapPair> = names
                .iter()
                .zip(syms)
                .map(|(c, &(_, addr))| LLVMOrcCSymbolMapPair {
                    Name: LLVMOrcLLJITMangleAndIntern(self.j, c.as_ptr()),
                    Sym: LLVMJITEvaluatedSymbol {
                        Address: addr as u64,
                        Flags: LLVMJITSymbolFlags {
                            GenericFlags: flags,
                            TargetFlags: 0,
                        },
                    },
                })
                .collect();
            let mu = LLVMOrcAbsoluteSymbols(pairs.as_mut_ptr(), pairs.len());
            let e = LLVMOrcJITDylibDefine(self.jd(), mu);
            // 失敗したときは、MaterializationUnit の所有権が呼ぶ側に残る。
            if !e.is_null() {
                LLVMOrcDisposeMaterializationUnit(mu);
            }
            check(e)
        }
    }

    /// LLVM IR の文面 `src` のモジュールを読み、検証し、`passes` が空でなければその最適化の列をかけて JIT に足す。
    pub fn add_ir(&self, name: &str, src: &str, passes: &str) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        // LLVM の資源を作る前に変換し、途中で戻っても何も漏らさないようにする。
        let cpasses = CString::new(passes).map_err(|e| e.to_string())?;
        // SAFETY: 文脈とモジュールはこの関数の中で作り、成功すれば所有権を LLJIT に渡す。
        unsafe {
            let ctx = LLVMContextCreate();
            let buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(
                src.as_ptr().cast(),
                src.len(),
                cname.as_ptr(),
            );
            let mut m = null_mut();
            let mut msg = null_mut();
            // LLVMParseIRInContext2 はバッファの所有権を取らない。
            let failed = LLVMParseIRInContext2(ctx, buf, &mut m, &mut msg);
            LLVMDisposeMemoryBuffer(buf);
            if failed != 0 {
                let s = CStr::from_ptr(msg).to_string_lossy().into_owned();
                LLVMDisposeMessage(msg);
                LLVMContextDispose(ctx);
                return Err(format!("LLVM IR を読めません: {s}"));
            }
            if let Err(e) = verify(m) {
                LLVMDisposeModule(m);
                LLVMContextDispose(ctx);
                return Err(e);
            }
            if !passes.is_empty() {
                let opts = LLVMCreatePassBuilderOptions();
                let r = check(LLVMRunPasses(m, cpasses.as_ptr(), null_mut(), opts));
                LLVMDisposePassBuilderOptions(opts);
                if let Err(e) = r {
                    LLVMDisposeModule(m);
                    LLVMContextDispose(ctx);
                    return Err(e);
                }
            }
            let tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
            let tsm = LLVMOrcCreateNewThreadSafeModule(m, tsc);
            // モジュールが文脈を持ち続ける。
            LLVMOrcDisposeThreadSafeContext(tsc);
            // LLVMOrcLLJITAddLLVMIRModule は、失敗したときも tsm の所有権を取る。
            check(LLVMOrcLLJITAddLLVMIRModule(self.j, self.jd(), tsm))
        }
    }

    /// 記号の番地を引く。最初に引くときに、その記号のモジュールを翻訳して結合する。
    pub fn lookup(&self, name: &str) -> Result<usize, String> {
        let c = CString::new(name).map_err(|e| e.to_string())?;
        let mut addr: LLVMOrcExecutorAddress = 0;
        // SAFETY: j は生きている LLJIT である。
        unsafe { check(LLVMOrcLLJITLookup(self.j, &mut addr, c.as_ptr()))? };
        Ok(addr as usize)
    }
}

impl Drop for Jit {
    fn drop(&mut self) {
        // SAFETY: j は生きている LLJIT で、以後使わない。
        if let Err(e) = unsafe { check(LLVMOrcDisposeLLJIT(self.j)) } {
            eprintln!("keleut: LLJIT を片付けられません: {e}");
        }
    }
}

/// モジュールを検証する。
fn verify(m: LLVMModuleRef) -> Result<(), String> {
    // SAFETY: m は生きているモジュールである。
    unsafe {
        let mut msg = null_mut();
        let bad = LLVMVerifyModule(
            m,
            LLVMVerifierFailureAction::LLVMReturnStatusAction,
            &mut msg,
        );
        let s = if msg.is_null() {
            String::new()
        } else {
            CStr::from_ptr(msg).to_string_lossy().into_owned()
        };
        if !msg.is_null() {
            LLVMDisposeMessage(msg);
        }
        if bad != 0 {
            Err(format!("LLVM IR の検証に失敗しました: {}", s.trim()))
        } else {
            Ok(())
        }
    }
}

/// 結合した LLVM の版(`22.1.8` など)。
pub fn llvm_version() -> String {
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    // SAFETY: 3 つの整数に版を書くだけである。
    unsafe { LLVMGetVersion(&mut a, &mut b, &mut c) };
    format!("{a}.{b}.{c}")
}
