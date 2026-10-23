use std::ffi::{c_char, c_int, CStr};
extern "C" { fn snprintf(buf: *mut c_char, n: usize, fmt: *const c_char, ...) -> c_int; }
fn c_g(p: i32, f: f64) -> String {
    let mut buf = [0 as c_char; 64];
    unsafe { snprintf(buf.as_mut_ptr(), 64, b"%.*g\0".as_ptr() as *const c_char, p as c_int, f); CStr::from_ptr(buf.as_ptr()).to_str().unwrap().to_string() }
}
// p significant digits, %g semantics, built from Rust's {:.*e}
fn r_g(p: i32, f: f64) -> String {
    let s = format!("{:.*e}", (p - 1) as usize, f);
    let (mant, ex) = s.split_once('e').unwrap();
    let x: i32 = ex.parse().unwrap();
    if x < -4 || x >= p {
        let mut m = mant.to_string();
        if m.contains('.') { while m.ends_with('0') { m.pop(); } if m.ends_with('.') { m.pop(); } }
        format!("{}e{}{:02}", m, if x < 0 { '-' } else { '+' }, x.abs())
    } else {
        let prec = (p - 1 - x).max(0) as usize;
        let mut m = format!("{:.*}", prec, f);
        if m.contains('.') { while m.ends_with('0') { m.pop(); } if m.ends_with('.') { m.pop(); } }
        m
    }
}
fn loop_repr(g: &dyn Fn(i32, f64) -> String, f: f64) -> String {
    for p in 1..=17 { let s = g(p, f); if s.parse::<f64>().unwrap() == f { return s; } }
    g(17, f)
}
fn main() {
    let mut st: u64 = 0x9E3779B97F4A7C15;
    let mut n = 0u64; let mut diff_g = 0u64; let mut diff_short = 0u64; let mut shown = 0;
    let mut cands: Vec<f64> = vec![2.5, 0.5, 1e23, 5e-324, 1.7976931348623157e308, 9007199254740993.0, 1e16, 2f64.powi(70), 2f64.powi(-1074)];
    for k in 54..1024 { cands.push(2f64.powi(k)); cands.push(2f64.powi(k) * 1.5); }
    for _ in 0..2_000_000 {
        st ^= st << 13; st ^= st >> 7; st ^= st << 17;
        let f = f64::from_bits(st); if !f.is_finite() { continue; } cands.push(f);
    }
    for &f in &cands {
        if f.fract() == 0.0 && f.abs() < 1e16 { continue; }
        n += 1;
        let c = loop_repr(&c_g, f); let r = loop_repr(&r_g, f);
        if c != r { diff_g += 1; if shown < 5 { println!("g-diff {:e}: C={} R={}", f, c, r); shown += 1; } }
        // shortest round-trip digits (Rust Display {:e}) vs C loop: compare digit strings
        let sh = format!("{:e}", f); let (m, _) = sh.split_once('e').unwrap();
        let cd: String = c.split(|ch| ch == 'e').next().unwrap().chars().filter(|ch| ch.is_ascii_digit()).collect::<String>().trim_start_matches('0').to_string();
        let sd: String = m.chars().filter(|ch| ch.is_ascii_digit()).collect();
        if cd.trim_end_matches('0') != sd.trim_end_matches('0') { diff_short += 1; if shown < 12 { println!("short-diff {:e}: C={} shortest={}", f, c, sh); shown += 1; } }
    }
    println!("tested={} diff_vs_rust_precision_e={} diff_vs_rust_shortest={}", n, diff_g, diff_short);
    use std::io::Write; let mut w = std::io::BufWriter::new(std::fs::File::create("cases.txt").unwrap());
    for &f in cands.iter().take(400000) { if f.fract() == 0.0 && f.abs() < 1e16 { continue; } writeln!(w, "{:016x} {}", f.to_bits(), loop_repr(&c_g, f)).unwrap(); }
}
