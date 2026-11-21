//! スタックが 256 KiB のスレッドで、JIT の相互末尾再帰がスタックを増やさずに実行できることを確かめるテスト。
//!
//! `fixtures/evenodd.kir` は 1,000 万段の相互末尾再帰で、正しく実行できれば何も出力せずに終了コード 0 で終了する。
//! 結果が誤っていればゼロ除算の実行時エラーになり、終了コード 3 で終了するので、終了コード 0 を確かめれば答えも確かめたことになる。
//! 末尾呼び出しが `musttail` でなければ、1 段に十数バイト以上を使うので、256 KiB では足りない。
//! スタックの下限でインタプリタに切り替えたエントリの数が 0 であることも確かめ、JIT のコードのまま最後まで実行したことを確認する。
//! `fixtures/nontail.kir`(100 万段の非末尾の再帰)は、同じ 256 KiB のスタックのスレッドでスタックの下限に達し、インタプリタに切り替えて最後まで実行する。
//!
//! 1 つのファイルのテストは同じプロセスの中で並行して実行されるので、実行の統計(`LAST_*`)を読むテストは 1 つの関数にまとめる。
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

/// スタックが 256 KiB のスレッドで `prog` を実行し、終了コードと統計(コンパイルした関数の数、スタックの下限でインタプリタに切り替えたエントリの数)を返す。
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
