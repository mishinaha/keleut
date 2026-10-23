use corosensei::{Coroutine, CoroutineResult};
use corosensei::stack::DefaultStack;
use std::time::Instant;
fn main() {
    // perform/resume round trips: body yields n times (perform), handler resumes each time (resume)
    let n: u64 = 10_000_000;
    let t = Instant::now();
    let mut co = Coroutine::with_stack(DefaultStack::new(64 * 1024).unwrap(), move |y, _: u64| {
        let mut acc = 0u64;
        for i in 0..n { acc += y.suspend(i); }
        acc
    });
    let mut v = 0u64; let mut rounds = 0u64;
    loop {
        match co.resume(v) { CoroutineResult::Yield(x) => { v = x + 1; rounds += 1; } CoroutineResult::Return(r) => { println!("acc={r} rounds={rounds}"); break; } }
    }
    let dt = t.elapsed();
    println!("round trip ns = {:.1}", dt.as_nanos() as f64 / n as f64);
    // fiber creation cost (handler installation with a fresh 64KiB stack, no pooling)
    let m = 100_000u64; let t = Instant::now(); let mut s = 0u64;
    for i in 0..m {
        let mut c = Coroutine::with_stack(DefaultStack::new(64 * 1024).unwrap(), move |_y: &corosensei::Yielder<u64, u64>, x: u64| x + i);
        if let CoroutineResult::Return(r) = c.resume(1) { s += r; }
    }
    println!("create+run+drop ns = {:.1} (s={s})", t.elapsed().as_nanos() as f64 / m as f64);
}
