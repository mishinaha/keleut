//! 壊れた入力を読んでも異常終了しないことのテスト。
//!
//! 入力は、フィクスチャのバイナリを切り詰めるかビットを反転して作り、テキストを切り詰めるか文字を置き換え、削除、挿入して作る。
//! ヘッダの後に乱数のバイト列を並べたバイナリも入力にする。
//! 読み込みはどの入力でも `Err` か `Ok` を返し、パニックやスタックオーバーフローで異常終了してはならない。
//! 読めたプログラムは、検証、バイナリの書き出し、テキストの出力に通しても異常終了してはならない。

mod common;

use kir::{Limits, N_NEST, binary, text, verify};

/// 固定のシードから始める擬似乱数生成器(xorshift64*)。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(common::fixtures().join(name)).unwrap()
}

#[derive(Default, Debug)]
struct Stats {
    rejected: usize,
    invalid: usize,
    accepted: usize,
}

/// 読めたプログラムを、検証、バイナリの書き出し、テキストの出力に通す。
fn exercise(p: &kir::Program, st: &mut Stats) {
    match verify::check(p, N_NEST) {
        Ok(()) => {
            st.accepted += 1;
            let b = binary::encode(p, N_NEST).expect("検証を通ったプログラムは書ける");
            let q = binary::decode(&b, N_NEST).expect("書いたものは読める");
            assert_eq!(binary::encode(&q, N_NEST).unwrap(), b);
            text::print(p, N_NEST).expect("検証を通ったプログラムは印字できる");
        }
        Err(_) => {
            st.invalid += 1;
            // 検証に通らないプログラムでも、バイナリの書き出しとテキストの出力は異常終了しない
            let _ = binary::encode(p, N_NEST);
            let _ = text::print(p, N_NEST);
        }
    }
}

fn run(f: impl FnOnce() -> Result<(), kir::Error> + Send) {
    kir::on_stack(Limits::default().stack_bytes, f).unwrap();
}

#[test]
fn binary_truncation() {
    run(|| {
        for name in ["sample.kir", "ex_handle.kir"] {
            let src = fixture(name);
            for n in 0..src.len() {
                assert!(
                    binary::decode(&src[..n], N_NEST).is_err(),
                    "{name}: 長さ {n} を受け付けました"
                );
            }
            // 末尾に 1 バイト追加したものも拒否する
            let mut longer = src.clone();
            longer.push(0);
            assert!(binary::decode(&longer, N_NEST).is_err());
        }
        Ok(())
    });
}

#[test]
fn binary_bit_flips() {
    run(|| {
        let mut st = Stats::default();
        for name in ["ex_handle.kir", "ex_dict.kir", "typecheck_m6_001.kir"] {
            let src = fixture(name);
            for i in 0..src.len() * 8 {
                let mut b = src.clone();
                b[i / 8] ^= 1 << (i % 8);
                match binary::decode(&b, N_NEST) {
                    Ok(p) => exercise(&p, &mut st),
                    Err(_) => st.rejected += 1,
                }
            }
        }
        let src = fixture("sample.kir");
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..20_000 {
            let mut b = src.clone();
            for _ in 0..1 + rng.below(3) {
                let i = rng.below(b.len() * 8);
                b[i / 8] ^= 1 << (i % 8);
            }
            match binary::decode(&b, N_NEST) {
                Ok(p) => exercise(&p, &mut st),
                Err(_) => st.rejected += 1,
            }
        }
        eprintln!("bit flips: {st:?}");
        assert!(st.rejected > 0 && st.invalid > 0 && st.accepted > 0);
        Ok(())
    });
}

#[test]
fn binary_garbage() {
    run(|| {
        let mut rng = Rng(12345);
        let head = &fixture("ex_loop.kir")[..16];
        for _ in 0..20_000 {
            let n = rng.below(200);
            let mut b = head.to_vec();
            for _ in 0..n {
                b.push(rng.next() as u8);
            }
            let _ = binary::decode(&b, N_NEST);
        }
        Ok(())
    });
}

#[test]
fn text_truncation_and_mutation() {
    run(|| {
        let mut st = Stats::default();
        let src = fixture("ex_handle.kirt");
        for n in 0..src.len() {
            match text::parse(&src[..n], N_NEST) {
                Ok(p) => exercise(&p, &mut st),
                Err(_) => st.rejected += 1,
            }
        }
        let alphabet = b"() \n\"\\x0123456789-abfnotlcrq_.";
        let mut rng = Rng(777);
        for _ in 0..10_000 {
            let mut b = src.clone();
            for _ in 0..1 + rng.below(3) {
                let i = rng.below(b.len());
                match rng.below(3) {
                    0 => b[i] = alphabet[rng.below(alphabet.len())],
                    1 => {
                        b.remove(i);
                    }
                    _ => b.insert(i, alphabet[rng.below(alphabet.len())]),
                }
            }
            match text::parse(&b, N_NEST) {
                Ok(p) => exercise(&p, &mut st),
                Err(_) => st.rejected += 1,
            }
        }
        eprintln!("text: {st:?}");
        assert!(st.rejected > 0 && st.accepted > 0);
        Ok(())
    });
}
