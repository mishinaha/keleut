//! LIR から作った LLVM IR を LLVM が検証に通し、翻訳できることの試験。
//!
//! `keleut-cli` の `tests/fixtures/run/` の各 KIR を LIR に下ろし、JIT にできる関数をすべて 1 つずつ別のモジュールにして
//! LLJIT に足し、引けることを確かめる(実行はしない)。
#![cfg(feature = "llvm")]

use std::path::PathBuf;

use keleut_lir::FuncId;
use keleut_llvm::{codegen, sys::Jit};

#[test]
fn fixture_の関数をすべて翻訳できる() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../keleut-cli/tests/fixtures/run");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("kir" | "kirt")))
        .collect();
    files.sort();
    assert!(files.len() >= 15, "{files:?}");
    let jit = Jit::new().unwrap();
    jit.define_absolute(&keleut_interp::native::helper_symbols())
        .unwrap();
    let (triple, layout) = (jit.triple(), jit.data_layout());
    let mut n = 0;
    for (k, p) in files.iter().enumerate() {
        let data = std::fs::read(p).unwrap();
        let limits = kir::Limits::default();
        let prog = kir::read_any_with(&data, &limits).unwrap();
        kir::verify_with(&prog, &limits).unwrap();
        let lp = keleut_lir::lower_opts(&prog, kir::N_NEST, true).unwrap();
        for (i, f) in lp.funcs.iter().enumerate() {
            if !keleut_interp::native::jit_eligible(f) {
                continue;
            }
            let fid = FuncId(i as u32);
            // 確保の速い道を作る形(既定)と作らない形(`--gc-stress`)の両方を翻訳する。
            for fast in [true, false] {
                let src =
                    codegen::module(&lp, fid, 0x1000, 0x2000, fast, &triple, &layout).unwrap();
                // 同じ LLJIT に足すので、名前を試験の入力ごとと形ごとに変える。
                let name = format!("{}_{k}_{}", codegen::func_name(fid), u8::from(fast));
                let src = src
                    .replace(
                        &format!("@{}(", codegen::func_name(fid)),
                        &format!("@{name}("),
                    )
                    .replace(
                        &format!("@{}.r(", codegen::func_name(fid)),
                        &format!("@{name}.r("),
                    );
                jit.add_ir(&name, &src, "default<O2>")
                    .unwrap_or_else(|e| panic!("{}: {}: {e}", p.display(), f.sym));
                jit.lookup(&name)
                    .unwrap_or_else(|e| panic!("{}: {}: {e}", p.display(), f.sym));
            }
            n += 1;
        }
    }
    assert!(n > 100, "翻訳した関数 {n}");
}

#[test]
fn llvm_の版は_22() {
    assert!(keleut_llvm::sys::llvm_version().starts_with("22."));
}

/// 同じ記号を 2 度足すと、2 度目は落ちずに誤りを返す(失敗した追加のモジュールを二重に片付けない)。
#[test]
fn 同じ記号を_2_度足すと誤りを返す() {
    let jit = Jit::new().unwrap();
    let src = "define i64 @f() {\n  ret i64 1\n}\n";
    jit.add_ir("a", src, "").unwrap();
    let e = jit.add_ir("b", src, "").unwrap_err();
    assert!(e.contains("duplicate definition"), "{e}");
    assert_ne!(jit.lookup("f").unwrap(), 0);
}

/// 同じ絶対の記号を 2 度登録すると、2 度目は誤りを返す。
#[test]
fn 同じ絶対の記号を_2_度登録すると誤りを返す() {
    extern "C" fn g() {}
    let jit = Jit::new().unwrap();
    let syms = [("keleut_test_g", g as *const () as usize)];
    jit.define_absolute(&syms).unwrap();
    assert!(jit.define_absolute(&syms).is_err());
}

/// 最適化の列に NUL があれば、LLVM の資源を作る前に誤りを返す。
#[test]
fn 最適化の列の_nul_は誤りになる() {
    let jit = Jit::new().unwrap();
    assert!(
        jit.add_ir("c", "define void @h() {\n  ret void\n}\n", "a\0b")
            .is_err()
    );
}

/// 確保の速い道は、どれも実行の役割の印を読んでからヒープに触れ、`fast_alloc` が偽なら作らない。
///
/// 今ある `KELEUT_TEST_ALLOC_FAIL` の場所では、印を立てた確保と同じ操作の中で実行時の確保が印を下ろすので、
/// `tests/oom_cmd.rs` は印の検査の抜けを見分けない。
/// そのため、速い道の文面に印の読み込みがあることをここで確かめる。
#[test]
fn 確保の速い道は実行の役割の印を読む() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../keleut-cli/tests/fixtures/run");
    let flag = format!(
        "inttoptr i64 {} to ptr",
        keleut_rt::alloc::flag_addr(keleut_rt::alloc::Role::Exec)
    );
    let mut fast_paths = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("kir") {
            continue;
        }
        let data = std::fs::read(&p).unwrap();
        let limits = kir::Limits::default();
        let prog = kir::read_any_with(&data, &limits).unwrap();
        let lp = keleut_lir::lower_opts(&prog, kir::N_NEST, true).unwrap();
        for (i, f) in lp.funcs.iter().enumerate() {
            if !keleut_interp::native::jit_eligible(f) {
                continue;
            }
            let fid = FuncId(i as u32);
            let on = codegen::function(&lp, fid, 0x1000, 0x2000, true).unwrap();
            let off = codegen::function(&lp, fid, 0x1000, 0x2000, false).unwrap();
            let lines: Vec<&str> = on.lines().collect();
            for (k, l) in lines.iter().enumerate() {
                if l.starts_with("ahave") {
                    fast_paths += 1;
                    // 印の番地、印の読み込み、比べ、枝の 4 行がこのブロックの直前にある。
                    assert!(k >= 4, "{}: {}", p.display(), f.sym);
                    assert!(lines[k - 4].contains(&flag), "{}: {}", p.display(), f.sym);
                    assert!(
                        lines[k - 3].contains("load atomic i8"),
                        "{}: {}",
                        p.display(),
                        f.sym
                    );
                    // 印が 0 でなければ遅い道へ移る。
                    assert!(
                        lines[k - 2].contains("icmp ne i8") && lines[k - 2].ends_with(", 0"),
                        "{}: {}",
                        p.display(),
                        f.sym
                    );
                }
            }
            assert!(!off.contains("ahave"), "{}: {}", p.display(), f.sym);
            assert!(!off.contains("ptr %heapp,"), "{}: {}", p.display(), f.sym);
        }
    }
    assert!(fast_paths > 10, "速い道 {fast_paths}");
}
