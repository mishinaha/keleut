//! インタプリタと JIT を交互にまたぐ末尾呼び出しが一定の領域で走ることの試験(計画 9 節の M92 の受け入れ)。
//!
//! `fixtures/alt.kir` は 1 億回の相互の末尾呼び出し(`ping` と `pong`)で、正しく走れば何も書かずに 0 で終わる。
//! `mixed=SEED` の種を、`ping` と `pong` のちょうど片方だけを翻訳するものに選ぶ。
//! すると、JIT の関数は相手の入口の `interp_stub` を `musttail` で呼び、
//! インタプリタの起動の最下段は JIT の関数を末尾で呼んで起動を抜け、スタブが `musttail` で呼ぶ(最下段の規則。計画 6.9 節)。
//! これを `--native-stack 512k` と同じ 512 KiB のミューテータのスレッドで、本番の `tailcc i64 @f(ptr %ctx)` の型のまま走らせる
//! (`doc/log/261024-1-spike/survey/tiering-gc.md` の 3.3 節の試験を本番の形にしたもの)。
//! 境目をまたいだ回数(`interp_stub` からの起動)と、ネイティブのスタックの使用量も確かめる。
//!
//! 最適化なしの組み(`cargo test`)では 100 万回の `fixtures/alt6.kir` を使い、
//! 1 億回は `cargo test --release` で走らせる。
//!
//! `threshold=1` では、インタプリタで走り始めた `ping` と `pong` を翻訳のスレッドが待たずに翻訳し、
//! 翻訳が済んだ後の呼び出しから JIT のコードに移ることも確かめる。
//!
//! 実行には `run_program_wait` を使う。
//! `run_program` は翻訳の途中で実行が終わるとプロセスを終えるので、その後の検査が走らずに試験が通ってしまうからである。
//!
//! 1 つのファイルの試験は同じプロセスの中で並んで走るので、実行の統計(`LAST_*`)を読む試験は 1 つにまとめる。
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

/// `jit` だけを翻訳し、`interp` を翻訳しない `mixed` の種。
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
        // 末尾呼び出しの半分はインタプリタの関数に入る(JIT の関数から interp_stub を通る)。
        assert!(stub >= calls / 2 - 1, "interp_stub からの起動 {stub}");
        // インタプリタの起動の最下段から JIT の関数への末尾呼び出しも、ほぼ半分ある。
        assert!(tail >= calls / 2 - 2, "末尾呼び出しの依頼 {tail}");
        assert_eq!(low, 0, "スタックの下限でインタプリタに移った入口");
        // 交互にまたいでもネイティブのスタックは伸びない(1 段の境目だけ)。
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
