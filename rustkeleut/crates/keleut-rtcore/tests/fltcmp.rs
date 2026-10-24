//! `show(Float64)` を glibc の `%.*g` の探索と比べる(計画 261024-1 の 8.3 節)。
//!
//! 入力は `doc/log/261024-1-spike/probe/fltcmp/src/main.rs` の候補の生成を移したもの(1,998,964 件)と、
//! 境界の値の集合である。
//! 基準は Diktor の `float_repr`(diktor/lib/value.ml)と同じ手順を glibc の `snprintf` で行ったものである。
//! NaN は glibc に渡さず、LangSpec 16.2 節の `nan` と比べる。
//!
//! 入力と基準は `<target>/rtcore/` に書き、`tests/wasm_check.mjs` が wasm32 版の確認に使う。

mod common;

use std::ffi::{CStr, c_char, c_int};

unsafe extern "C" {
    fn snprintf(buf: *mut c_char, n: usize, fmt: *const c_char, ...) -> c_int;
}

/// fltcmp が生成する件数(計画 8.3 節)。
const FLTCMP_COUNT: usize = 1_998_964;

fn pow2(k: i32) -> f64 {
    assert!((-1022..=1023).contains(&k));
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// fltcmp の候補(同じ xorshift と種、同じ 2 の冪と境界の値、同じ除外)。
///
/// fltcmp は `2f64.powi(k)` で 2 の冪を作る。
/// `2f64.powi(-1074)` は、release で組むと定数畳み込みで `5e-324` になり、debug では実行時の計算で `0.0` になる。
/// fltcmp の 1,998,964 件は release の値なので、ここではビット列から作った正確な値を使う。
fn fltcmp_candidates() -> Vec<f64> {
    let mut st: u64 = 0x9E3779B97F4A7C15;
    let mut cands: Vec<f64> = vec![
        2.5,
        0.5,
        1e23,
        5e-324,
        1.7976931348623157e308,
        9007199254740993.0,
        1e16,
        pow2(70),
        f64::from_bits(1),
    ];
    for k in 54..1024 {
        cands.push(pow2(k));
        cands.push(pow2(k) * 1.5);
    }
    for _ in 0..2_000_000 {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        let f = f64::from_bits(st);
        if !f.is_finite() {
            continue;
        }
        cands.push(f);
    }
    cands.retain(|f| !(f.fract() == 0.0 && f.abs() < 1e16));
    cands
}

/// 計画 8.3 節の境界の値(ビット列)。
fn boundary_values() -> Vec<u64> {
    let mut v: Vec<u64> = vec![
        0x0000_0000_0000_0000,
        0x8000_0000_0000_0000,
        // NaN: 静かな NaN、信号の NaN、符号の立ったもの、積載の違うもの。
        0x7ff8_0000_0000_0000,
        0xfff8_0000_0000_0000,
        0x7ff0_0000_0000_0001,
        0xfff0_0000_0000_0001,
        0x7ff4_0000_0000_0000,
        0x7ff8_0000_0000_0123,
        0xfff8_0000_0000_0123,
        0x7fff_ffff_ffff_ffff,
        0xffff_ffff_ffff_ffff,
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
    ];
    let push_nb = |v: &mut Vec<u64>, x: f64, k: u64| {
        let u = x.to_bits();
        v.push(u);
        for i in 1..=k {
            v.push(u + i);
            if u >= i {
                v.push(u - i);
            }
        }
    };
    // ±1e16 と ±1e17 の境目(整数の分岐と指数表記の境)と、2^53、2^54 の付近。
    for x in [
        1e16,
        1e17,
        9007199254740992.0,
        18014398509481984.0,
        1e15,
        1e21,
        1e22,
    ] {
        push_nb(&mut v, x, 4);
    }
    // 非正規化数: 最小から 16 個、最大とその下、ビット列の乱数。
    for i in 1..=16 {
        v.push(i);
    }
    push_nb(&mut v, f64::from_bits(0x000f_ffff_ffff_ffff), 4);
    let mut st: u64 = 0x2545F4914F6CDD1D;
    for _ in 0..4096 {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        v.push(st & 0x000f_ffff_ffff_ffff);
    }
    // k ≤ -1022 のすべての 2^k と、2^k の前後の値(2^-1074 から 2^1023 まで)。
    for k in 0..52 {
        v.push(1u64 << k);
    }
    for k in -1022..=1023 {
        push_nb(&mut v, pow2(k), 2);
    }
    // 最大の有限値と、指数表記と小数点表記の境(1e-4、1e-5 の付近)。
    push_nb(&mut v, f64::MAX, 2);
    for x in [1e-4, 1e-5, 0.1, 0.5, 1.5, 123456.789] {
        push_nb(&mut v, x, 2);
    }
    // 以上の値の符号を反転したもの。
    let n = v.len();
    for i in 0..n {
        v.push(v[i] ^ 0x8000_0000_0000_0000);
    }
    v
}

fn c_fmt(fmt: &CStr, p: Option<i32>, x: f64) -> String {
    let mut buf = [0 as c_char; 128];
    // SAFETY: 書式は %.*g か %.1f で、結果は 128 バイトに収まる。
    let n = unsafe {
        match p {
            Some(p) => snprintf(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), p as c_int, x),
            None => snprintf(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), x),
        }
    };
    assert!(n > 0 && (n as usize) < buf.len());
    // SAFETY: snprintf は NUL で終わる文字列を書いた。
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_str()
        .unwrap()
        .to_string()
}

/// Diktor の `float_repr` と同じ手順を glibc で行う。
fn glibc_show(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x == f64::INFINITY {
        return "inf".to_string();
    }
    if x == f64::NEG_INFINITY {
        return "-inf".to_string();
    }
    if x.fract() == 0.0 && x.abs() < 1e16 {
        return c_fmt(c"%.1f", None, x);
    }
    let mut s = None;
    for p in 1..=17 {
        let c = c_fmt(c"%.*g", Some(p), x);
        if c.parse::<f64>().unwrap() == x {
            s = Some(c);
            break;
        }
    }
    let s = s.unwrap_or_else(|| c_fmt(c"%.*g", Some(17), x));
    let s = match s.split_once('e') {
        None => s,
        Some((mant, ex)) => {
            let (sign, digits) = match ex.as_bytes()[0] {
                b'+' => ("", &ex[1..]),
                b'-' => ("-", &ex[1..]),
                _ => ("", ex),
            };
            let digits = digits.trim_start_matches('0');
            let digits = if digits.is_empty() { "0" } else { digits };
            format!("{mant}e{sign}{digits}")
        }
    };
    if s.contains(['.', 'e', 'E']) {
        s
    } else {
        s + ".0"
    }
}

fn rtcore_show_bits(bits: u64) -> String {
    let mut buf = [0u8; keleut_rtcore::F64_SHOW_MAX];
    // SAFETY: buf は F64_SHOW_MAX バイトである。
    let n = unsafe { keleut_rtcore::sys::krt_f64_show_bits(bits, buf.as_mut_ptr().cast()) };
    assert!(n < keleut_rtcore::F64_SHOW_MAX && buf[n] == 0);
    String::from_utf8(buf[..n].to_vec()).unwrap()
}

/// 入力の集合を比べ、違う件数を返す(最初の数件を印字する)。
fn compare(label: &str, bits: &[u64], expected: &[String]) -> usize {
    let got = common::par_map(bits, |&b| rtcore_show_bits(b));
    let mut diff = 0;
    for ((b, e), g) in bits.iter().zip(expected).zip(&got) {
        if e != g {
            if diff < 10 {
                println!("  {label} 0x{b:016x}: glibc {e} rtcore {g}");
            }
            diff += 1;
        }
    }
    println!(
        "x86_64 {label}: {} cases, {diff} differ from glibc",
        bits.len()
    );
    diff
}

#[test]
fn fltcmp_and_boundary_match_glibc() {
    let cands = fltcmp_candidates();
    assert_eq!(
        cands.len(),
        FLTCMP_COUNT,
        "fltcmp の候補の件数が計画 8.3 節と違う"
    );
    let fl_bits: Vec<u64> = cands.iter().map(|f| f.to_bits()).collect();
    let bd_bits = boundary_values();
    let fl_exp = common::par_map(&fl_bits, |&b| glibc_show(f64::from_bits(b)));
    let bd_exp = common::par_map(&bd_bits, |&b| glibc_show(f64::from_bits(b)));

    // wasm32 版の確認(tests/wasm_check.mjs)のための入力と基準。
    let all_bits: Vec<u64> = fl_bits.iter().chain(&bd_bits).copied().collect();
    let mut text = String::new();
    for s in fl_exp.iter().chain(&bd_exp) {
        text.push_str(s);
        text.push('\n');
    }
    common::stage_write("fltcmp-inputs.bin", &common::bits_le(all_bits));
    common::stage_write("fltcmp-expected.txt", text.as_bytes());
    common::stage_write(
        "fltcmp-meta.json",
        format!(
            "{{\"fltcmp\": {}, \"boundary\": {}, {}}}\n",
            fl_bits.len(),
            bd_bits.len(),
            common::provenance_json()
        )
        .as_bytes(),
    );

    let d1 = compare("fltcmp", &fl_bits, &fl_exp);
    let d2 = compare("boundary", &bd_bits, &bd_exp);
    assert_eq!(d1 + d2, 0, "rtcore の show が glibc と違う");
}
