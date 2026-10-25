//! 256 KiB のスタックのスレッドで、JIT の相互の末尾再帰が一定の領域で走ることの試験(計画 9 節の M91 の受け入れ)。
//!
//! `fixtures/evenodd.kir` は 1,000 万段の相互の末尾再帰で、正しく走れば何も書かずに 0 で終わる。
//! 結果が誤っていればゼロ除算で 3 で終わるので、終了状態 0 を確かめれば答えも確かめたことになる。
//! 末尾呼び出しが `musttail` でなければ、1 段に十数バイト以上を使い、256 KiB では足りない。
//! スタックの下限でインタプリタに移った入口が 0 であることも確かめ、JIT のコードのまま完走したことを見る。
//! `fixtures/nontail.kir`(100 万段の末尾でない再帰)は、同じスレッドでスタックの下限に届き、
//! インタプリタに移って最後まで走る。
//!
//! 1 つのファイルの試験は同じプロセスの中で並んで走るので、実行の統計(`LAST_*`)を読む試験は 1 つにまとめる。
#![cfg(feature = "llvm")]

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use keleut_interp::native::{LAST_COMPILED, LAST_LOW_STACK_ENTRIES};

fn load(name: &str) -> keleut_lir::Program {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let data = std::fs::read(&p).unwrap();
    let limits = kir::Limits::default();
    let prog = kir::read_kir_with(&data, &limits).unwrap();
    kir::verify_with(&prog, &limits).unwrap();
    keleut_lir::lower_opts(&prog, kir::N_NEST, true).unwrap()
}

/// 256 KiB のスタックのスレッドで `prog` を走らせ、終了状態と統計を返す。
fn run_small(prog: keleut_lir::Program) -> (u8, u64, u64) {
    const STACK: usize = 256 * 1024;
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let mut opts = keleut_llvm::RunOptions::default();
            opts.base.native_stack = STACK as u64;
            let code = keleut_llvm::run_program_here(&prog, &opts, 32 * 1024);
            (
                code,
                LAST_COMPILED.load(Ordering::Relaxed),
                LAST_LOW_STACK_ENTRIES.load(Ordering::Relaxed),
            )
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn 小さなスタックで_1e7_段の相互の末尾再帰が完走する() {
    let (code, compiled, low) = run_small(load("evenodd.kir"));
    assert_eq!(code, 0);
    assert!(compiled >= 3, "翻訳した関数 {compiled}");
    assert_eq!(low, 0, "スタックの下限でインタプリタに移った入口");

    let (code, compiled, low) = run_small(load("nontail.kir"));
    assert_eq!(code, 0);
    assert!(compiled >= 2, "翻訳した関数 {compiled}");
    assert!(low > 0, "末尾でない再帰はスタックの下限に届くはずである");
}
