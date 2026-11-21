//! rtcore の `sin`、`cos`、`exp`、`log` の x86_64 版の結果を作り、wasm32 版の検査(`tests/wasm_check.mjs`)に渡す。
//!
//! 入力は、特別な値の表とシードを固定した乱数から作る 1,000,000 個で、実行のたびに同じ値になる。
//! glibc との差分は参考として表示するだけで、テストの合否には使わない。
//! LangSpec §17 は、これらの関数が正しい値に最も近い 2 つの `Float64` のどちらを返すかを処理系に任せているからである。

mod common;

const N: usize = 1_000_000;

unsafe extern "C" {
    fn sin(x: f64) -> f64;
    fn cos(x: f64) -> f64;
    fn exp(x: f64) -> f64;
    fn log(x: f64) -> f64;
}

struct Gen {
    st: u64,
    v: Vec<f64>,
}

impl Gen {
    fn rnd(&mut self) -> u64 {
        self.st ^= self.st >> 12;
        self.st ^= self.st << 25;
        self.st ^= self.st >> 27;
        self.st.wrapping_mul(0x2545f4914f6cdd1d)
    }
    fn add(&mut self, x: f64) {
        if self.v.len() < N {
            self.v.push(x);
        }
    }
    /// `x` と、ビット列で前後 `k` 個までの値を追加する。
    /// 符号の境界を越える値と、無限大のビット列を上回る値は追加しない。
    fn add_nb(&mut self, x: f64, k: u64) {
        let u = x.to_bits();
        self.add(x);
        for i in 1..=k {
            if (u & 0x7fffffffffffffff) + i <= 0x7ff0000000000000 {
                self.add(f64::from_bits(u + i));
            }
            if (u & 0x7fffffffffffffff) >= i {
                self.add(f64::from_bits(u - i));
            }
        }
    }
    fn rnd_exp(&mut self, lo: u64, hi: u64) -> f64 {
        let e = lo + self.rnd() % (hi - lo + 1);
        let m = self.rnd() & 0xfffffffffffff;
        let s = self.rnd() & 0x8000000000000000;
        f64::from_bits(s | (e << 52) | m)
    }
    fn uni(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.rnd() >> 11) as f64 * f64::from_bits(0x3ca0000000000000))
    }
}

/// `ldexp(x, n)` を計算する。
/// 2^n をビット列から作るので、`n` は -1022 以上 1023 以下でなければならない。
/// 乗算は 1 回だけなので、丸めもオーバーフローも `ldexp` と同じである。
fn ldexp(x: f64, n: i32) -> f64 {
    x * f64::from_bits(((n + 1023) as u64) << 52)
}

fn inputs() -> Vec<f64> {
    let mut g = Gen {
        st: 0x9e3779b97f4a7c15,
        v: Vec::with_capacity(N),
    };
    const SPECIALS: &[u64] = &[
        0x0000000000000000,
        0x8000000000000000,
        0x7ff0000000000000,
        0xfff0000000000000,
        0x7ff8000000000000,
        0xfff8000000000000,
        0x7ff0000000000001,
        0xfff0000000000001,
        0x7ff4000000000000,
        0x7fffffffffffffff,
        0xffffffffffffffff,
        0x7ff8000000000123,
        0x0000000000000001,
        0x8000000000000001,
        0x000fffffffffffff,
        0x800fffffffffffff,
        0x0010000000000000,
        0x8010000000000000,
        0x7fefffffffffffff,
        0xffefffffffffffff,
        0x3ff0000000000000,
        0xbff0000000000000,
        0x3fe0000000000000,
        0x4000000000000000,
        0x40862e42fefa39ef,
        0xc0874910d52d3051,
        0xc086232bdd7abcd2,
        0x3fe921fb54442d18,
        0xbfe921fb54442d18,
        0x3ff921fb54442d18,
        0x400921fb54442d18,
        0x401921fb54442d18,
        0x3e40000000000000,
        0x3e50000000000000,
        0x413921fb54442d18,
        0x413921fb54442d19,
        0x4330000000000000,
        0x4340000000000000,
        0x3fee000000000000,
        0x3ff1000000000000,
        0x3c90000000000000,
        0x3ca0000000000000,
    ];
    for &s in SPECIALS {
        g.add_nb(f64::from_bits(s), 16);
    }
    g.add_nb(
        f64::from_bits(((797 + 52 + 1023) << 52) | (6381956970095103 & 0xfffffffffffff)),
        64,
    );
    while g.v.len() < 200_000 {
        let e = g.rnd() % 64;
        let k = (g.rnd() >> (64 - 1 - e)) as f64 + 1.0;
        let mut x = k * f64::from_bits(0x3ff921fb54442d18);
        if g.rnd() & 1 != 0 {
            let n = (g.rnd() % 960) as i32;
            x = ldexp(x, n);
        }
        if g.rnd() & 1 != 0 {
            x = -x;
        }
        g.add_nb(x, 2);
    }
    while g.v.len() < 500_000 {
        let x = f64::from_bits(g.rnd());
        g.add(x);
    }
    while g.v.len() < 700_000 {
        let x = g.rnd_exp(1023 - 60, 1023 + 60);
        g.add(x);
    }
    while g.v.len() < 800_000 {
        let x = g.uni(-760.0, 720.0);
        g.add(x);
    }
    while g.v.len() < 850_000 {
        let u = g.uni(-1.0, 1.0);
        let k = (g.rnd() % 50) as i32;
        g.add(1.0 + ldexp(u, -k));
    }
    while g.v.len() < 900_000 {
        let x = f64::from_bits((g.rnd() & 0x800fffffffffffff) | 1);
        g.add(x);
    }
    while g.v.len() < 950_000 {
        let x = g.rnd_exp(1023 + 20, 2046);
        g.add(x);
    }
    while g.v.len() < N {
        let x = g.rnd_exp(1, 1023 - 30);
        g.add(x);
    }
    g.v
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn libm_results_for_wasm_check() {
    let xs = inputs();
    assert_eq!(xs.len(), N);
    let krt: [fn(f64) -> f64; 4] = [
        keleut_rtcore::sin,
        keleut_rtcore::cos,
        keleut_rtcore::exp,
        keleut_rtcore::log,
    ];
    // SAFETY: glibc の libm にある純粋な関数である。
    let glibc: [fn(f64) -> f64; 4] = [
        |x| unsafe { sin(x) },
        |x| unsafe { cos(x) },
        |x| unsafe { exp(x) },
        |x| unsafe { log(x) },
    ];
    let mut out = Vec::with_capacity(4 * N);
    for (j, name) in ["sin", "cos", "exp", "log"].iter().enumerate() {
        let r: Vec<f64> = common::par_map(&xs, |&x| krt[j](x));
        let d = xs
            .iter()
            .zip(&r)
            .filter(|&(&x, &y)| !same(y, glibc[j](x)))
            .count();
        println!("{name}: {d} of {N} differ from glibc (informative)");
        out.extend(r.iter().map(|y| y.to_bits()));
    }
    common::stage_write(
        "libm-inputs.bin",
        &common::bits_le(xs.iter().map(|x| x.to_bits())),
    );
    common::stage_write("libm-x86_64.bin", &common::bits_le(out));
    common::stage_write(
        "libm-meta.json",
        format!("{{\"cases\": {N}, {}}}\n", common::provenance_json()).as_bytes(),
    );
}
