//! llvm-sys の薄いラッパ。
//!
//! LLJIT(ORCv2)を作り、Rust の関数のアドレスを絶対シンボルとして登録し、LLVM IR のテキストで書いたモジュールを追加してコンパイルする。
//! モジュール 1 つにつき、LLVM のコンテキストを 1 つ使う。
//! コンテキストは `LLVMContextCreate` で作ってから、`LLVMOrcCreateNewThreadSafeContextFromLLVMContext` でスレッドセーフなコンテキストに包む。
//! LLVM 21 以降の C API には `LLVMOrcThreadSafeContextGetContext` がないので、この順序で作る。
//!
//! このラッパの値はどれも、作ったスレッドの中だけで使う。
//! そのスレッドは JIT のコンパイルスレッドである([`crate`] の冒頭)。

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

/// LLVM のエラーからメッセージを取り出して文字列で返し、エラーを破棄する。
///
/// # Safety
///
/// `e` は null でない LLVM のエラーでなければならない。
/// この関数が所有権を受け取る。
unsafe fn err_string(e: LLVMErrorRef) -> String {
    // SAFETY: e は、呼び出し元の事前条件から、null でない LLVM のエラーである。
    // メッセージは LLVMDisposeErrorMessage で解放する。
    unsafe {
        let m = LLVMGetErrorMessage(e);
        let s = CStr::from_ptr(m).to_string_lossy().into_owned();
        LLVMDisposeErrorMessage(m);
        s
    }
}

/// LLVM のエラーを `Result` に変換する。
///
/// # Safety
///
/// `e` は LLVM の関数が返したエラーでなければならない。
/// null は成功を表す。
unsafe fn check(e: LLVMErrorRef) -> Result<(), String> {
    if e.is_null() {
        Ok(())
    } else {
        // SAFETY: e は null でなく、呼び出し元の事前条件から、LLVM の関数が返したエラーである。
        Err(unsafe { err_string(e) })
    }
}

/// LLJIT を 1 つ所有する。
/// ドロップすると LLJIT を破棄する。
pub struct Jit {
    j: LLVMOrcLLJITRef,
}

impl Jit {
    /// 既定の設定で LLJIT を作る。
    /// ターゲットはネイティブターゲット(実行しているマシン)である。
    pub fn new() -> Result<Jit, String> {
        static INIT: std::sync::Once = std::sync::Once::new();
        let mut failed = false;
        INIT.call_once(|| {
            // SAFETY: ネイティブターゲットとアセンブリプリンタを登録するだけである。
            unsafe {
                failed = llvm_sys::target::LLVM_InitializeNativeTarget() != 0
                    || llvm_sys::target::LLVM_InitializeNativeAsmPrinter() != 0;
            }
        });
        if failed {
            return Err("LLVM のネイティブターゲットを登録できません".into());
        }
        let mut j = null_mut();
        // SAFETY: 設定のビルダには null を渡す。
        // 成功すれば j は LLJIT を指す。
        unsafe { check(LLVMOrcCreateLLJIT(&mut j, null_mut()))? };
        Ok(Jit { j })
    }

    /// ターゲットトリプルを返す。
    pub fn triple(&self) -> String {
        // SAFETY: j は破棄していない LLJIT を指し、文字列は LLJIT が所有する。
        unsafe {
            CStr::from_ptr(LLVMOrcLLJITGetTripleString(self.j))
                .to_string_lossy()
                .into_owned()
        }
    }

    /// データレイアウトの文字列を返す。
    pub fn data_layout(&self) -> String {
        // SAFETY: j は破棄していない LLJIT を指し、文字列は LLJIT が所有する。
        unsafe {
            CStr::from_ptr(LLVMOrcLLJITGetDataLayoutStr(self.j))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn jd(&self) -> LLVMOrcJITDylibRef {
        // SAFETY: j は破棄していない LLJIT を指す。
        unsafe { LLVMOrcLLJITGetMainJITDylib(self.j) }
    }

    /// Rust の関数のアドレスを、呼び出せる絶対シンボルとしてメインの JITDylib に登録する。
    pub fn define_absolute(&self, syms: &[(&str, usize)]) -> Result<(), String> {
        let names: Vec<CString> = syms
            .iter()
            .map(|(n, _)| CString::new(*n).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        // SAFETY: j は破棄していない LLJIT を指し、名前の文字列はこの呼び出しの間は解放されない。
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
            // 失敗したときは、MaterializationUnit の所有権が呼び出し元に残るので、ここで破棄する。
            if !e.is_null() {
                LLVMOrcDisposeMaterializationUnit(mu);
            }
            check(e)
        }
    }

    /// LLVM IR のテキスト `src` をモジュールとして読み込んで検証し、`passes` が空でなければその最適化パイプラインを適用して、JIT に追加する。
    pub fn add_ir(&self, name: &str, src: &str, passes: &str) -> Result<(), String> {
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        // LLVM のリソースを作る前に変換し、ここでエラーを返しても何もリークしないようにする。
        let cpasses = CString::new(passes).map_err(|e| e.to_string())?;
        // SAFETY: コンテキストとモジュールはこの関数の中で作り、成功すれば所有権を LLJIT に渡す。
        unsafe {
            let ctx = LLVMContextCreate();
            let buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(
                src.as_ptr().cast(),
                src.len(),
                cname.as_ptr(),
            );
            let mut m = null_mut();
            let mut msg = null_mut();
            // LLVMParseIRInContext2 はバッファの所有権を受け取らないので、ここで破棄する。
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
            // モジュールがコンテキストを保持し続けるので、ここで持っている参照は破棄する。
            LLVMOrcDisposeThreadSafeContext(tsc);
            // LLVMOrcLLJITAddLLVMIRModule は、失敗したときも tsm の所有権を受け取る。
            check(LLVMOrcLLJITAddLLVMIRModule(self.j, self.jd(), tsm))
        }
    }

    /// シンボルのアドレスを解決する。
    /// あるシンボルを初めて解決するときに、そのシンボルを定義するモジュールをコンパイルしてリンクする。
    pub fn lookup(&self, name: &str) -> Result<usize, String> {
        let c = CString::new(name).map_err(|e| e.to_string())?;
        let mut addr: LLVMOrcExecutorAddress = 0;
        // SAFETY: j は破棄していない LLJIT を指す。
        unsafe { check(LLVMOrcLLJITLookup(self.j, &mut addr, c.as_ptr()))? };
        Ok(addr as usize)
    }
}

impl Drop for Jit {
    fn drop(&mut self) {
        // SAFETY: j は破棄していない LLJIT を指し、この後は使わない。
        if let Err(e) = unsafe { check(LLVMOrcDisposeLLJIT(self.j)) } {
            eprintln!("keleut: LLJIT を片付けられません: {e}");
        }
    }
}

/// モジュールを LLVM の検証器で検証する。
fn verify(m: LLVMModuleRef) -> Result<(), String> {
    // SAFETY: m は破棄していないモジュールを指す。
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

/// リンクしている LLVM のバージョン(`22.1.8` など)を返す。
pub fn llvm_version() -> String {
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    // SAFETY: 3 つの整数にバージョンを書き込むだけである。
    unsafe { LLVMGetVersion(&mut a, &mut b, &mut c) };
    format!("{a}.{b}.{c}")
}
