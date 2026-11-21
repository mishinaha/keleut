//! `show(Float64)` の結果を、glibc の `%.*g` で最短の桁数を探して作った期待値と比較する。
//!
//! 入力は 2 組ある。
//! 1 つは、乱数と 2 の冪から作る 1,998,964 件の候補(`fltcmp_candidates`)で、ファイル名と出力では fltcmp と呼ぶ。
//! もう 1 つは境界値の集合(`boundary_values`)である。
//! 期待値は、Diktor の `float_repr`(`diktor/lib/value.ml`)と同じ手順を glibc の `snprintf` で行って作る。
//! NaN は glibc に渡さず、LangSpec §16.2 の `nan` と比較する。
//!
//! 入力と期待値は `<target>/rtcore/` に書き、`tests/wasm_check.mjs` が wasm32 版の検査に使う。

mod common;

use std::ffi::{CStr, c_char, c_int};

unsafe extern "C" {
    fn snprintf(buf: *mut c_char, n: usize, fmt: *const c_char, ...) -> c_int;
}

/// fltcmp の候補の件数。
/// このテストと `tests/wasm_check.mjs` が、候補がこの件数であることを検査する。
const FLTCMP_COUNT: usize = 1_998_964;

fn pow2(k: i32) -> f64 {
    assert!((-1022..=1023).contains(&k));
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// fltcmp の候補を作る。
///
/// まず、次の 3 種類の値を集める。
/// 固定の値、2^54 から 2^1023 までの 2 の冪とその 1.5 倍、シードを固定した xorshift で作る 2,000,000 個のビット列のうち有限の値を表すものである。
/// 集めた候補から、絶対値が 1e16 未満の整数値を取り除く。
/// 2 の冪は `2f64.powi(k)` で計算せず、ビット列から正確な値を作る(`pow2` と `f64::from_bits(1)`)。
/// `2f64.powi(-1074)` は、リリースビルドでは定数畳み込みで `5e-324` になり、デバッグビルドでは実行時の計算で `0.0` になるからである。
/// 件数 1,998,964 は、2^-1074 を `5e-324` として数えた値である。
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

/// 境界値をビット列で返す。
///
/// 0、NaN、無限大、表記が切り替わる値の付近、非正規化数、2 の冪とその付近、最大の有限値の付近、いくつかの小数(0.1 など)の付近の値を集め、それぞれの符号を反転した値も追加する。
fn boundary_values() -> Vec<u64> {
    let mut v: Vec<u64> = vec![
        0x0000_0000_0000_0000,
        0x8000_0000_0000_0000,
        // NaN: quiet NaN、signaling NaN、符号ビットが立ったもの、ペイロードが違うもの。
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
    // 1e16 と 1e17 の付近(整数値を %.1f で文字列にするかどうかの分岐と、指数表記に切り替わる境界)、2^53 と 2^54 の付近、1e15、1e21、1e22 の付近。
    // 負の値は、最後に符号を反転して追加する。
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
    // 非正規化数: 小さいほうから 16 個、最大の非正規化数とその前後、乱数で作ったビット列。
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
    // 2^-1074 から 2^1023 までのすべての 2 の冪。
    // 2^-1022 以上の冪には、ビット列で前後 2 個ずつの値も追加する。
    for k in 0..52 {
        v.push(1u64 << k);
    }
    for k in -1022..=1023 {
        push_nb(&mut v, pow2(k), 2);
    }
    // 最大の有限値の付近と、指数表記と小数点表記の境界(1e-4、1e-5)の付近。
    // 0.1、0.5、1.5、123456.789 の付近も追加する。
    push_nb(&mut v, f64::MAX, 2);
    for x in [1e-4, 1e-5, 0.1, 0.5, 1.5, 123456.789] {
        push_nb(&mut v, x, 2);
    }
    // ここまでの値の符号を反転したもの。
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
    // SAFETY: snprintf が NUL で終わる文字列を buf に書いている。
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_str()
        .unwrap()
        .to_string()
}

/// Diktor の `float_repr` と同じ手順を glibc の `snprintf` で行い、期待値の文字列を作る。
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

/// `bits` の各入力について、rtcore の結果を `expected` と比較し、違った件数を返す。
/// 違った入力は、最初の 10 件までを表示する。
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

    // wasm32 版の検査(tests/wasm_check.mjs)に渡す入力と期待値。
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
