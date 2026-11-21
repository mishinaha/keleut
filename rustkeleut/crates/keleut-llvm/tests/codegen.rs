//! LIR から作った LLVM IR が LLVM の検証に通り、コンパイルできることを確かめるテスト。
//!
//! `keleut-cli` の `tests/fixtures/run/` の各 KIR を LIR に変換し、JIT コンパイルできる関数をすべて、1 つずつ別のモジュールにして LLJIT に追加し、シンボルを解決できることを確かめる(実行はしない)。
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
            // 確保の高速経路を生成する場合(既定)と生成しない場合(`--gc-stress`)の両方をコンパイルする。
            for fast in [true, false] {
                let src =
                    codegen::module(&lp, fid, 0x1000, 0x2000, fast, &triple, &layout).unwrap();
                // 同じ LLJIT に追加するので、関数の名前を入力ファイルごと、場合ごとに変える。
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

/// 同じシンボルを定義するモジュールを 2 度追加すると、2 度目は異常終了せずにエラーを返す(追加に失敗したモジュールを二重に破棄しない)。
#[test]
fn 同じ記号を_2_度足すと誤りを返す() {
    let jit = Jit::new().unwrap();
    let src = "define i64 @f() {\n  ret i64 1\n}\n";
    jit.add_ir("a", src, "").unwrap();
    let e = jit.add_ir("b", src, "").unwrap_err();
    assert!(e.contains("duplicate definition"), "{e}");
    assert_ne!(jit.lookup("f").unwrap(), 0);
}

/// 同じ絶対シンボルを 2 度登録すると、2 度目はエラーを返す。
#[test]
fn 同じ絶対の記号を_2_度登録すると誤りを返す() {
    extern "C" fn g() {}
    let jit = Jit::new().unwrap();
    let syms = [("keleut_test_g", g as *const () as usize)];
    jit.define_absolute(&syms).unwrap();
    assert!(jit.define_absolute(&syms).is_err());
}

/// 最適化パイプラインの文字列に NUL があれば、LLVM のリソースを作る前にエラーを返す。
#[test]
fn 最適化の列の_nul_は誤りになる() {
    let jit = Jit::new().unwrap();
    assert!(
        jit.add_ir("c", "define void @h() {\n  ret void\n}\n", "a\0b")
            .is_err()
    );
}

/// 確保の高速経路は、どれも実行スレッドのフラグを読んでからヒープに触れる。
/// `fast_alloc` が偽なら、高速経路を生成しない。
///
/// `KELEUT_TEST_ALLOC_FAIL` で指定できる確保箇所はどれも、フラグを立てる確保と、そのフラグを調べて下ろすランタイムの処理が、同じ操作の中にある。
/// 高速経路がフラグを読む前にフラグが下りるので、`tests/oom_cmd.rs` は、高速経路からフラグの検査が抜けていても検出できない。
/// そのため、高速経路の LLVM IR にフラグの読み込みがあることを、ここで確かめる。
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
                    // フラグのアドレスの計算、フラグの読み込み、比較、分岐の 4 行が、このブロックの直前にある。
                    assert!(k >= 4, "{}: {}", p.display(), f.sym);
                    assert!(lines[k - 4].contains(&flag), "{}: {}", p.display(), f.sym);
                    assert!(
                        lines[k - 3].contains("load atomic i8"),
                        "{}: {}",
                        p.display(),
                        f.sym
                    );
                    // フラグが 0 でなければ低速経路へ分岐する。
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
