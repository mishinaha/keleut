//! LLVM ORCv2 (LLJIT) spike for RustKeleut: llvm-sys 221 against the system LLVM 22.
//!
//! Experiments (run all with `cargo run --release`):
//!   basic     JIT a small function and call it from Rust; timings
//!   callback  module built with the IRBuilder C API; JIT -> Rust -> JIT re-entrancy
//!   tail      tailcc / ccc musttail mutual recursion, same module and across modules
//!   tohost    musttail from JIT code into a Rust extern "C" function
//!   tier      call through a Rust-owned dispatch table; "interpreter" slot patched to JIT code
//!   gc        gc "shadow-stack" in the JIT, with the root chain head owned by Rust
//!   timing    per-module compile time for many small modules (O0 IR vs default<O2> IR)
//! IR texts are also written to target/ir/ for inspection with llc.

use llvm_sys::analysis::{LLVMVerifierFailureAction, LLVMVerifyModule};
use llvm_sys::core::*;
use llvm_sys::error::*;
use llvm_sys::ir_reader::LLVMParseIRInContext2;
use llvm_sys::orc2::lljit::*;
use llvm_sys::orc2::*;
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::transforms::pass_builder::*;
use std::ffi::{CStr, CString};
use std::hint::black_box;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- helpers

unsafe fn err_to_string(e: LLVMErrorRef) -> String {
    unsafe {
        let m = LLVMGetErrorMessage(e);
        let s = CStr::from_ptr(m).to_string_lossy().into_owned();
        LLVMDisposeErrorMessage(m);
        s
    }
}

unsafe fn check(e: LLVMErrorRef) -> Result<(), String> {
    if e.is_null() { Ok(()) } else { Err(unsafe { err_to_string(e) }) }
}

struct Jit {
    j: LLVMOrcLLJITRef,
}

impl Jit {
    fn new() -> Jit {
        let mut j = null_mut();
        unsafe { check(LLVMOrcCreateLLJIT(&mut j, null_mut())).expect("LLVMOrcCreateLLJIT") };
        Jit { j }
    }

    fn jd(&self) -> LLVMOrcJITDylibRef {
        unsafe { LLVMOrcLLJITGetMainJITDylib(self.j) }
    }

    /// Define host (Rust) addresses as absolute symbols in the main JITDylib.
    fn define_abs(&self, syms: &[(&str, usize, bool)]) -> Result<(), String> {
        unsafe {
            let mut pairs: Vec<LLVMOrcCSymbolMapPair> = syms
                .iter()
                .map(|&(name, addr, callable)| {
                    let c = CString::new(name).unwrap();
                    let mut flags = LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsExported as u8;
                    if callable {
                        flags |= LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsCallable as u8;
                    }
                    LLVMOrcCSymbolMapPair {
                        Name: LLVMOrcLLJITMangleAndIntern(self.j, c.as_ptr()),
                        Sym: LLVMJITEvaluatedSymbol {
                            Address: addr as u64,
                            Flags: LLVMJITSymbolFlags { GenericFlags: flags, TargetFlags: 0 },
                        },
                    }
                })
                .collect();
            let mu = LLVMOrcAbsoluteSymbols(pairs.as_mut_ptr(), pairs.len());
            check(LLVMOrcJITDylibDefine(self.jd(), mu))
        }
    }

    /// Hand a module (owned by `ctx`) to the JIT. One ThreadSafeContext per LLVMContext.
    fn add_module(&self, ctx: LLVMContextRef, m: LLVMModuleRef) -> Result<(), String> {
        unsafe {
            let tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
            let tsm = LLVMOrcCreateNewThreadSafeModule(m, tsc);
            LLVMOrcDisposeThreadSafeContext(tsc); // the module keeps the context alive
            let e = LLVMOrcLLJITAddLLVMIRModule(self.j, self.jd(), tsm);
            if !e.is_null() {
                LLVMOrcDisposeThreadSafeModule(tsm);
            }
            check(e)
        }
    }

    fn lookup(&self, name: &str) -> Result<usize, String> {
        let c = CString::new(name).unwrap();
        let mut addr: LLVMOrcExecutorAddress = 0;
        unsafe { check(LLVMOrcLLJITLookup(self.j, &mut addr, c.as_ptr()))? };
        Ok(addr as usize)
    }
}

impl Drop for Jit {
    fn drop(&mut self) {
        unsafe { check(LLVMOrcDisposeLLJIT(self.j)).expect("dispose") };
    }
}

/// Parse textual IR from memory into a fresh context; verify; optionally run a pass pipeline.
fn parse_ir(name: &str, src: &str, passes: Option<&str>) -> Result<(LLVMContextRef, LLVMModuleRef), String> {
    std::fs::create_dir_all("target/ir").ok();
    std::fs::write(format!("target/ir/{name}.ll"), src).ok();
    unsafe {
        let ctx = LLVMContextCreate();
        let cname = CString::new(name).unwrap();
        let buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(src.as_ptr() as *const _, src.len(), cname.as_ptr());
        let mut m = null_mut();
        let mut msg = null_mut();
        let failed = LLVMParseIRInContext2(ctx, buf, &mut m, &mut msg);
        LLVMDisposeMemoryBuffer(buf);
        if failed != 0 {
            let s = CStr::from_ptr(msg).to_string_lossy().into_owned();
            LLVMDisposeMessage(msg);
            LLVMContextDispose(ctx);
            return Err(format!("parse: {s}"));
        }
        verify(m).map_err(|e| {
            LLVMDisposeModule(m);
            LLVMContextDispose(ctx);
            e
        })?;
        if let Some(p) = passes {
            let opts = LLVMCreatePassBuilderOptions();
            let cp = CString::new(p).unwrap();
            let r = check(LLVMRunPasses(m, cp.as_ptr(), null_mut(), opts));
            LLVMDisposePassBuilderOptions(opts);
            r?;
        }
        Ok((ctx, m))
    }
}

fn verify(m: LLVMModuleRef) -> Result<(), String> {
    unsafe {
        let mut msg = null_mut();
        let bad = LLVMVerifyModule(m, LLVMVerifierFailureAction::LLVMReturnStatusAction, &mut msg);
        let s = if msg.is_null() { String::new() } else { CStr::from_ptr(msg).to_string_lossy().into_owned() };
        if !msg.is_null() {
            LLVMDisposeMessage(msg);
        }
        if bad != 0 { Err(format!("verify: {}", s.trim())) } else { Ok(()) }
    }
}

type F1 = extern "C" fn(i64) -> i64;
fn as_f1(addr: usize) -> F1 {
    unsafe { std::mem::transmute::<usize, F1>(addr) }
}

fn ms(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1e3)
}

// ---------------------------------------------------------------- host callbacks

static PROBES: Mutex<Vec<(i64, usize)>> = Mutex::new(Vec::new());

/// Records the current stack address (address of a local) under a tag.
extern "C" fn probe(tag: i64) {
    let x = 0u8;
    let sp = black_box(&x) as *const u8 as usize;
    PROBES.lock().unwrap().push((tag, sp));
}

fn take_probes() -> Vec<(i64, usize)> {
    std::mem::take(&mut *PROBES.lock().unwrap())
}

static CB_ENTRY: AtomicUsize = AtomicUsize::new(0);
static CB_LEAF: AtomicUsize = AtomicUsize::new(0);

/// Called from JIT code; calls back into JIT code (re-entrancy).
extern "C" fn host_cb(n: i64) -> i64 {
    if n > 0 {
        as_f1(CB_ENTRY.load(Ordering::SeqCst))(n - 1)
    } else {
        as_f1(CB_LEAF.load(Ordering::SeqCst))(21)
    }
}

static HOST_TAIL_SP: Mutex<Vec<usize>> = Mutex::new(Vec::new());
#[inline(never)]
extern "C" fn host_tail(n: i64) -> i64 {
    let x = 0u8;
    HOST_TAIL_SP.lock().unwrap().push(black_box(&x) as *const u8 as usize);
    n * 3
}

#[inline(never)]
fn drive(f: F1, n: i64) -> i64 {
    black_box(f)(black_box(n))
}

static DISPATCH: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static INTERP_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Stand-in for the bytecode interpreter's entry for one function.
extern "C" fn interp_square(x: i64) -> i64 {
    INTERP_CALLS.fetch_add(1, Ordering::SeqCst);
    x * x
}

static ROOT_HEAD: AtomicUsize = AtomicUsize::new(0); // llvm_gc_root_chain, owned by Rust
static GC_SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[repr(C)]
struct FrameMap {
    num_roots: i32,
    num_meta: i32,
}
#[repr(C)]
struct StackEntry {
    next: *mut StackEntry,
    map: *const FrameMap,
    // roots follow
}

/// A toy "collector": walks the shadow stack and relocates every root by +8.
extern "C" fn gc_walk() {
    unsafe {
        let mut e = ROOT_HEAD.load(Ordering::SeqCst) as *mut StackEntry;
        let mut depth = 0;
        while !e.is_null() {
            let n = (*(*e).map).num_roots;
            let roots = (e as *mut u8).add(std::mem::size_of::<StackEntry>()) as *mut usize;
            for i in 0..n as usize {
                let r = roots.add(i);
                GC_SEEN.lock().unwrap().push(format!("frame{depth} root{i} = {:#x}", *r));
                *r += 8;
            }
            e = (*e).next;
            depth += 1;
        }
    }
}

// ---------------------------------------------------------------- IR generators

/// even/odd mutual recursion. `cc` is "tailcc" or "" (ccc); `tail` is "musttail" or "".
fn even_odd(prefix: &str, cc: &str, tail: &str, which: (bool, bool)) -> String {
    let call = |callee: &str| {
        if tail.is_empty() {
            // non-tail baseline: plain call (no `tail` marker => no sibcall)
            format!("  %r = call {cc} i64 @{prefix}_{callee}(i64 %m)\n  ret i64 %r\n")
        } else {
            format!("  %r = {tail} call {cc} i64 @{prefix}_{callee}(i64 %m)\n  ret i64 %r\n")
        }
    };
    let def = |me: &str, other: &str, base: i64| {
        format!(
            "define {cc} i64 @{prefix}_{me}(i64 %n) {{\nentry:\n  %z = icmp eq i64 %n, 0\n  br i1 %z, label %base, label %rec\nbase:\n  call void @probe(i64 1)\n  ret i64 {base}\nrec:\n  %m = sub i64 %n, 1\n{}}}\n",
            call(other)
        )
    };
    let decl = |me: &str| format!("declare {cc} i64 @{prefix}_{me}(i64)\n");
    let mut s = String::from("declare void @probe(i64)\n");
    if which.0 {
        s += &def("even", "odd", 1);
        s += &format!(
            "define i64 @{prefix}_entry(i64 %n) {{\n  call void @probe(i64 0)\n  %r = call {cc} i64 @{prefix}_even(i64 %n)\n  ret i64 %r\n}}\n"
        );
    } else {
        s += &decl("even");
    }
    if which.1 { s += &def("odd", "even", 0) } else { s += &decl("odd") }
    s
}

// ---------------------------------------------------------------- experiments

fn exp_basic() {
    println!("== basic");
    let t0 = Instant::now();
    let jit = Jit::new();
    let t_create = t0.elapsed();
    let src = "define i64 @add1(i64 %x) {\n  %r = add i64 %x, 1\n  ret i64 %r\n}\n";
    let t1 = Instant::now();
    let (ctx, m) = parse_ir("basic", src, None).unwrap();
    let t_parse = t1.elapsed();
    let t2 = Instant::now();
    jit.add_module(ctx, m).unwrap();
    let t_add = t2.elapsed();
    let t3 = Instant::now();
    let a = jit.lookup("add1").unwrap();
    let t_lookup1 = t3.elapsed();
    let t4 = Instant::now();
    let _ = jit.lookup("add1").unwrap();
    let t_lookup2 = t4.elapsed();
    let triple = unsafe { CStr::from_ptr(LLVMOrcLLJITGetTripleString(jit.j)).to_string_lossy() };
    println!("triple = {triple}");
    println!("add1(41) = {}", as_f1(a)(41));
    println!(
        "create LLJIT {} / parse {} / add {} / first lookup (compile+link) {} / second lookup {}",
        ms(t_create), ms(t_parse), ms(t_add), ms(t_lookup1), ms(t_lookup2)
    );
}

/// Module built with the IRBuilder API (no text): cb_entry calls host_cb, host_cb calls cb_entry/cb_leaf.
fn exp_callback() {
    println!("== callback");
    let jit = Jit::new();
    jit.define_abs(&[("host_cb", host_cb as *const () as usize, true)]).unwrap();
    let t0 = Instant::now();
    unsafe {
        let ctx = LLVMContextCreate();
        let m = LLVMModuleCreateWithNameInContext(c"cb".as_ptr(), ctx);
        let i64t = LLVMInt64TypeInContext(ctx);
        let mut params = [i64t];
        let fty = LLVMFunctionType(i64t, params.as_mut_ptr(), 1, 0);
        let host = LLVMAddFunction(m, c"host_cb".as_ptr(), fty);
        let b = LLVMCreateBuilderInContext(ctx);
        // cb_entry(n) = host_cb(n) + 1
        let entry = LLVMAddFunction(m, c"cb_entry".as_ptr(), fty);
        LLVMPositionBuilderAtEnd(b, LLVMAppendBasicBlockInContext(ctx, entry, c"e".as_ptr()));
        let mut args = [LLVMGetParam(entry, 0)];
        let r = LLVMBuildCall2(b, fty, host, args.as_mut_ptr(), 1, c"r".as_ptr());
        let s = LLVMBuildAdd(b, r, LLVMConstInt(i64t, 1, 0), c"s".as_ptr());
        LLVMBuildRet(b, s);
        // cb_leaf(x) = x * 2
        let leaf = LLVMAddFunction(m, c"cb_leaf".as_ptr(), fty);
        LLVMPositionBuilderAtEnd(b, LLVMAppendBasicBlockInContext(ctx, leaf, c"e".as_ptr()));
        let y = LLVMBuildMul(b, LLVMGetParam(leaf, 0), LLVMConstInt(i64t, 2, 0), c"y".as_ptr());
        LLVMBuildRet(b, y);
        LLVMDisposeBuilder(b);
        verify(m).unwrap();
        let t_build = t0.elapsed();
        jit.add_module(ctx, m).unwrap();
        let t1 = Instant::now();
        CB_ENTRY.store(jit.lookup("cb_entry").unwrap(), Ordering::SeqCst);
        CB_LEAF.store(jit.lookup("cb_leaf").unwrap(), Ordering::SeqCst);
        let t_compile = t1.elapsed();
        let v = as_f1(CB_ENTRY.load(Ordering::SeqCst))(3);
        println!("cb_entry(3) = {v} (expected 46: 4 JIT->Rust->JIT round trips, leaf 21*2=42, +1 per level)");
        println!("IRBuilder build {} / lookup+compile 2 fns {}", ms(t_build), ms(t_compile));
    }
}

fn run_on_small_stack<T: Send + 'static>(stack: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(stack).spawn(f).unwrap().join().unwrap()
}

fn report_depth(label: &str, f: usize, n: i64, stack: usize) {
    take_probes();
    let r = run_on_small_stack(stack, move || as_f1(f)(n));
    let p = take_probes();
    let top = p.iter().find(|x| x.0 == 0).unwrap().1;
    let bottom = p.iter().find(|x| x.0 == 1).unwrap().1;
    let grow = top as i64 - bottom as i64;
    println!(
        "{label}: depth {n} result {r}; probe sp top {top:#x} bottom {bottom:#x}; growth {grow} bytes ({:.2} B/level) on a {} KiB thread stack",
        grow as f64 / n as f64,
        stack / 1024
    );
}

fn exp_tail() {
    println!("== tail");
    let jit = Jit::new();
    jit.define_abs(&[("probe", probe as *const () as usize, true)]).unwrap();
    // same module, tailcc + musttail
    let (c, m) = parse_ir("tail_tailcc_same", &even_odd("ts", "tailcc", "musttail", (true, true)), None).unwrap();
    jit.add_module(c, m).unwrap();
    // same module, ccc + musttail
    let (c, m) = parse_ir("tail_ccc_same", &even_odd("cs", "", "musttail", (true, true)), None).unwrap();
    jit.add_module(c, m).unwrap();
    // different modules (and different LLVMContexts), tailcc + musttail
    let (c, m) = parse_ir("tail_tailcc_xA", &even_odd("tx", "tailcc", "musttail", (true, false)), None).unwrap();
    jit.add_module(c, m).unwrap();
    let (c, m) = parse_ir("tail_tailcc_xB", &even_odd("tx", "tailcc", "musttail", (false, true)), None).unwrap();
    jit.add_module(c, m).unwrap();
    // different modules, ccc + musttail
    let (c, m) = parse_ir("tail_ccc_xA", &even_odd("cx", "", "musttail", (true, false)), None).unwrap();
    jit.add_module(c, m).unwrap();
    let (c, m) = parse_ir("tail_ccc_xB", &even_odd("cx", "", "musttail", (false, true)), None).unwrap();
    jit.add_module(c, m).unwrap();
    // baseline: plain calls
    let (c, m) = parse_ir("tail_baseline", &even_odd("nb", "", "", (true, true)), None).unwrap();
    jit.add_module(c, m).unwrap();

    const N: i64 = 10_000_000;
    const STACK: usize = 256 * 1024;
    for (label, p) in [
        ("tailcc musttail, same module", "ts"),
        ("ccc musttail, same module", "cs"),
        ("tailcc musttail, two modules", "tx"),
        ("ccc musttail, two modules", "cx"),
    ] {
        let t = Instant::now();
        let f = jit.lookup(&format!("{p}_entry")).unwrap();
        let tc = t.elapsed();
        let t = Instant::now();
        report_depth(label, f, N, STACK);
        println!("   compile {} run {}", ms(tc), ms(t.elapsed()));
    }
    let f = jit.lookup("nb_entry").unwrap();
    report_depth("baseline plain call (no tail)", f, 1000, STACK);
    // musttail in a different calling convention than the callee: expect a verifier error
    let bad = "declare i64 @host_tail(i64)\ndefine tailcc i64 @bad(i64 %n) {\n  %r = musttail call i64 @host_tail(i64 %n)\n  ret i64 %r\n}\n";
    match parse_ir("tail_bad_cc", bad, None) {
        Ok(_) => println!("tailcc -> ccc musttail: accepted (unexpected)"),
        Err(e) => println!("tailcc -> ccc musttail: rejected: {e}"),
    }
}

fn exp_tohost() {
    println!("== tohost");
    let jit = Jit::new();
    jit.define_abs(&[("host_tail", host_tail as *const () as usize, true)]).unwrap();
    let src = "declare i64 @host_tail(i64)\n\
define i64 @to_host(i64 %n) {\n  %a = alloca [64 x i8]\n  call void @llvm.lifetime.start.p0(ptr %a)\n  call void @llvm.lifetime.end.p0(ptr %a)\n  %r = musttail call i64 @host_tail(i64 %n)\n  ret i64 %r\n}\n\
define i64 @to_host_nt(i64 %n) {\n  %r = call i64 @host_tail(i64 %n)\n  ret i64 %r\n}\n\
declare void @llvm.lifetime.start.p0(ptr)\ndeclare void @llvm.lifetime.end.p0(ptr)\n";
    let (c, m) = match parse_ir("tohost", src, None) {
        Ok(x) => x,
        Err(e) => {
            println!("tohost IR rejected: {e}");
            return;
        }
    };
    jit.add_module(c, m).unwrap();
    let mt = as_f1(jit.lookup("to_host").unwrap());
    let nt = as_f1(jit.lookup("to_host_nt").unwrap());
    HOST_TAIL_SP.lock().unwrap().clear();
    let r0 = drive(host_tail, 1);
    let r1 = drive(mt, 2);
    let r2 = drive(nt, 3);
    let sp = HOST_TAIL_SP.lock().unwrap().clone();
    println!("results {r0} {r1} {r2}");
    println!(
        "host_tail sp: direct {:#x}, via musttail {:#x} (diff {}), via plain call {:#x} (diff {})",
        sp[0], sp[1], sp[0] as i64 - sp[1] as i64, sp[2], sp[0] as i64 - sp[2] as i64
    );
}

fn exp_tier() {
    println!("== tier");
    let jit = Jit::new();
    DISPATCH[0].store(interp_square as *const () as usize, Ordering::SeqCst);
    jit.define_abs(&[("dispatch", &DISPATCH as *const _ as usize, false)]).unwrap();
    let src = "@dispatch = external global [4 x ptr]\n\
define i64 @call_slot0(i64 %x) {\n  %fp = load ptr, ptr @dispatch\n  %r = musttail call i64 %fp(i64 %x)\n  ret i64 %r\n}\n";
    let (c, m) = parse_ir("tier_caller", src, None).unwrap();
    jit.add_module(c, m).unwrap();
    let caller = as_f1(jit.lookup("call_slot0").unwrap());
    let mut log = Vec::new();
    for i in 1..=6 {
        let before = INTERP_CALLS.load(Ordering::SeqCst);
        let v = caller(i);
        let tier = if INTERP_CALLS.load(Ordering::SeqCst) > before { "interp" } else { "jit" };
        log.push(format!("{i}->{v}({tier})"));
        if INTERP_CALLS.load(Ordering::SeqCst) == 3 && DISPATCH[0].load(Ordering::SeqCst) == interp_square as *const () as usize {
            // hot: compile and patch the slot
            let t = Instant::now();
            let (c, m) = parse_ir("tier_square", "define i64 @jit_square(i64 %x) {\n  %r = mul i64 %x, %x\n  ret i64 %r\n}\n", None).unwrap();
            jit.add_module(c, m).unwrap();
            DISPATCH[0].store(jit.lookup("jit_square").unwrap(), Ordering::SeqCst);
            log.push(format!("[tier-up {}]", ms(t.elapsed())));
        }
    }
    println!("{}", log.join(" "));
}

fn exp_gc() {
    println!("== gc");
    let jit = Jit::new();
    let def = jit.define_abs(&[
        ("gc_walk", gc_walk as *const () as usize, true),
        ("llvm_gc_root_chain", &ROOT_HEAD as *const _ as usize, false),
    ]);
    println!("define llvm_gc_root_chain as Rust absolute symbol: {:?}", def);
    let src = |name: &str, callee: &str| {
        format!(
            "declare void @llvm.gcroot(ptr, ptr)\ndeclare void @{callee}(ptr)\n\
define i64 @{name}(ptr %obj) gc \"shadow-stack\" {{\nentry:\n  %root = alloca ptr\n  call void @llvm.gcroot(ptr %root, ptr null)\n  store ptr %obj, ptr %root\n  call void @{callee}(ptr %obj)\n  %v = load ptr, ptr %root\n  %i = ptrtoint ptr %v to i64\n  ret i64 %i\n}}\n"
        )
    };
    // two modules: gc_outer -> (Rust trampoline-free) gc_inner via direct call; gc_inner -> gc_walk
    let inner = src("gc_inner", "gc_walk_p").replace("declare void @gc_walk_p(ptr)\n", "declare void @gc_walk()\n").replace("call void @gc_walk_p(ptr %obj)", "call void @gc_walk()");
    let outer = src("gc_outer", "gc_inner_v").replace(
        "declare void @gc_inner_v(ptr)\n",
        "declare i64 @gc_inner(ptr)\n",
    ).replace("call void @gc_inner_v(ptr %obj)", "%unused = call i64 @gc_inner(ptr inttoptr (i64 4096 to ptr))");
    let (c, m) = parse_ir("gc_inner", &inner, None).unwrap();
    jit.add_module(c, m).unwrap();
    let (c, m) = parse_ir("gc_outer", &outer, None).unwrap();
    jit.add_module(c, m).unwrap();
    let f = jit.lookup("gc_outer");
    let chain = jit.lookup("llvm_gc_root_chain");
    println!("lookup llvm_gc_root_chain = {:?} (Rust ROOT_HEAD at {:#x})", chain.as_ref().map(|a| format!("{a:#x}")), &ROOT_HEAD as *const _ as usize);
    match f {
        Ok(f) => {
            let r = unsafe { std::mem::transmute::<usize, extern "C" fn(usize) -> i64>(f) }(0x1000_0000);
            println!("gc_outer(0x10000000) = {r:#x} (expected 0x10000008 if the walker updated the outer root)");
            for s in GC_SEEN.lock().unwrap().iter() {
                println!("  walker saw {s}");
            }
            println!("ROOT_HEAD after return = {:#x}", ROOT_HEAD.load(Ordering::SeqCst));
        }
        Err(e) => println!("gc_outer lookup failed: {e}"),
    }
}

fn exp_timing() {
    println!("== timing");
    let jit = Jit::new();
    let body = |i: usize| {
        format!(
            "define i64 @fib{i}(i64 %n) {{\nentry:\n  %c = icmp slt i64 %n, 2\n  br i1 %c, label %ret, label %rec\nret:\n  ret i64 %n\nrec:\n  %a = sub i64 %n, 1\n  %b = sub i64 %n, 2\n  %x = call i64 @fib{i}(i64 %a)\n  %y = call i64 @fib{i}(i64 %b)\n  %s = add i64 %x, %y\n  ret i64 %s\n}}\n"
        )
    };
    for (label, passes) in [("no IR passes", None), ("default<O2>", Some("default<O2>"))] {
        let mut times = Vec::new();
        let mut run = Duration::ZERO;
        for k in 0..200 {
            let i = k + if passes.is_some() { 1000 } else { 0 };
            let t = Instant::now();
            let (c, m) = parse_ir("timing", &body(i), passes).unwrap();
            jit.add_module(c, m).unwrap();
            let f = jit.lookup(&format!("fib{i}")).unwrap();
            times.push(t.elapsed());
            if k == 199 {
                let t = Instant::now();
                let v = as_f1(f)(32);
                run = t.elapsed();
                assert_eq!(v, 2178309);
            }
        }
        times.sort();
        println!(
            "{label}: parse+add+compile+link per module: min {} median {} p90 {} max {}; fib(32) run {}",
            ms(times[0]), ms(times[100]), ms(times[180]), ms(times[199]), ms(run)
        );
    }
}

fn main() {
    unsafe {
        assert_eq!(LLVM_InitializeNativeTarget(), 0);
        assert_eq!(LLVM_InitializeNativeAsmPrinter(), 0);
        let (mut a, mut b, mut c) = (0, 0, 0);
        LLVMGetVersion(&mut a, &mut b, &mut c);
        println!("LLVM runtime version {a}.{b}.{c}");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = ["basic", "callback", "tail", "tohost", "tier", "gc", "timing"];
    let sel: Vec<&str> = if args.is_empty() { all.to_vec() } else { args.iter().map(|s| s.as_str()).collect() };
    for s in sel {
        match s {
            "basic" => exp_basic(),
            "callback" => exp_callback(),
            "tail" => exp_tail(),
            "tohost" => exp_tohost(),
            "tier" => exp_tier(),
            "gc" => exp_gc(),
            "timing" => exp_timing(),
            _ => eprintln!("unknown experiment {s}"),
        }
    }
}
