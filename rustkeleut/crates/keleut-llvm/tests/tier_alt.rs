//! インタプリタと JIT を交互にまたぐ末尾呼び出しが、スタックを増やさずに実行できることを確かめるテスト。
//!
//! `fixtures/alt.kir` は 1 億回の相互の末尾呼び出し(`ping` と `pong`)で、正しく実行できれば何も出力せずに終了コード 0 で終了する。
//! `mixed=SEED` のシードには、`ping` と `pong` のどちらか一方だけをコンパイルするものを選ぶ。
//! すると、JIT の関数は、呼び出し先のエントリである `interp_stub` を `musttail` で呼ぶ。
//! `interp_stub` から始めたインタプリタの実行は、最下段のフレームが JIT の関数を末尾呼び出しで呼ぶところで終わり、スタブがその関数を `musttail` で呼ぶ。
//! これを、`--native-stack 512k` と同じ 512 KiB のミューテータのスレッドで、`keleut run` と同じ `tailcc i64 @f(ptr %ctx)` の型のまま実行する。
//! 境界をまたいだ回数(`interp_stub` から入った回数)と、ネイティブスタックの使用量も確かめる。
//!
//! デバッグビルド(`cargo test`)では 100 万回の `fixtures/alt6.kir` を使い、1 億回の `fixtures/alt.kir` は `cargo test --release` で実行する。
//!
//! `threshold=1` では、インタプリタで実行を始めた `ping` と `pong` を、コンパイルスレッドがミューテータを待たせずにコンパイルし、コンパイルが済んだ後の呼び出しから JIT のコードに切り替わる。
//! この設定では、終了コードが 0 であることと、2 個以上の関数のコンパイルが済んだことを確かめる。
//!
//! 実行には `run_program_wait` を使う。
//! `run_program` は、コンパイルの途中で実行が終わるとプロセスを終了するので、その後の検査が実行されないままテストが成功してしまうからである。
//!
//! 1 つのファイルのテストは同じプロセスの中で並行して実行されるので、実行の統計(`LAST_*`)を読むテストは 1 つの関数にまとめる。
#![cfg(feature = "llvm")]

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use keleut_interp::native::{
    LAST_COMPILED, LAST_LOW_STACK_ENTRIES, LAST_STACK_USED, LAST_STUB_ENTRIES, LAST_TAIL_REQUESTS,
    mixed_selects,
};
use keleut_lir::FuncId;

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

fn func(prog: &keleut_lir::Program, sym: &str) -> FuncId {
    let i = prog
        .funcs
        .iter()
        .position(|f| f.sym == sym)
        .unwrap_or_else(|| panic!("{sym} がありません"));
    FuncId(i as u32)
}

/// `jit` だけをコンパイルし、`interp` をコンパイルしない `mixed` のシードを探す。
fn seed_for(jit: FuncId, interp: FuncId) -> u64 {
    (1..10_000)
        .find(|&s| mixed_selects(s, jit) && !mixed_selects(s, interp))
        .expect("種が見つかりません")
}

#[test]
fn 交互の末尾呼び出しが_512k_のスタックで完走する() {
    let (name, calls) = if cfg!(debug_assertions) {
        ("alt6.kir", 1_000_000u64)
    } else {
        ("alt.kir", 100_000_000u64)
    };
    let prog = load(name);
    let ping = func(&prog, "main/ping");
    let pong = func(&prog, "main/pong");
    for (jit, interp) in [(ping, pong), (pong, ping)] {
        let seed = seed_for(jit, interp);
        let mut opts = keleut_llvm::RunOptions {
            tier: keleut_llvm::Tier::Mixed(seed),
            ..Default::default()
        };
        opts.base.native_stack = 512 * 1024;
        let t = std::time::Instant::now();
        let code = keleut_llvm::run_program_wait(&prog, &opts);
        let secs = t.elapsed().as_secs_f64();
        let stub = LAST_STUB_ENTRIES.load(Ordering::Relaxed);
        let tail = LAST_TAIL_REQUESTS.load(Ordering::Relaxed);
        let used = LAST_STACK_USED.load(Ordering::Relaxed);
        let low = LAST_LOW_STACK_ENTRIES.load(Ordering::Relaxed);
        eprintln!(
            "mixed={seed}(JIT は {}): {calls} 回、{secs:.2} 秒、interp_stub からの起動 {stub}、\
             末尾呼び出しの依頼 {tail}、ネイティブのスタック {used} バイト",
            prog.funcs[jit.index()].sym
        );
        assert_eq!(code, 0);
        // 末尾呼び出しの半分は、インタプリタで実行する関数に入る(JIT の関数から `interp_stub` を通る)。
        assert!(stub >= calls / 2 - 1, "interp_stub からの起動 {stub}");
        // インタプリタの最下段のフレームから JIT の関数への末尾呼び出しも、ほぼ半分ある。
        assert!(tail >= calls / 2 - 2, "末尾呼び出しの依頼 {tail}");
        assert_eq!(low, 0, "スタックの下限でインタプリタに移った入口");
        // 交互にまたいでもネイティブスタックは増えない(境界の 1 段分だけを使う)。
        assert!(used < 16 * 1024, "ネイティブのスタック {used} バイト");
    }
    let mut opts = keleut_llvm::RunOptions {
        tier: keleut_llvm::Tier::Threshold(1),
        ..Default::default()
    };
    opts.base.native_stack = 512 * 1024;
    assert_eq!(keleut_llvm::run_program_wait(&prog, &opts), 0);
    let compiled = LAST_COMPILED.load(Ordering::Relaxed);
    assert!(compiled >= 2, "threshold=1 で翻訳を終えた関数 {compiled}");
}
