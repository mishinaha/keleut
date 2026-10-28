//! 組み合わせ(計画 261024-1 の 8.2 節)である。
//!
//! 組は、KIR の書き手と実行系の組である。
//! `ka` と `km` は種ごとに 1 回ずつ走らせるので、1 つの名前が複数の実体に広がる。

/// 組の実行系である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Diktor の木の評価器(基準)。
    Diktor,
    /// `diktor --emit-ir` の後の `diktor --run-ir`。
    DiktorIr,
    /// Diktor の木の評価器を基準にし、RustKeleut に移したフロントエンドの段だけを RustKeleut で走らせる(組 `kf`)。
    Frontend,
    /// `keleut run` に組の選択肢を足したもの。
    Keleut,
    /// `keleut emit-kir` の後の `diktor --run-ir`(組 `x-ir`)。
    /// Rust のフロントエンドが書いた KIR を Diktor が実行する。
    RustIr,
    /// `keleut run --frontend rust` に組の選択肢を足したもの(組 `kr-*`)。
    /// Rust のフロントエンドが書いた KIR を RustKeleut の後段が実行する。
    RustKeleut,
}

/// 走らせる組の 1 つの実体である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// 表示と既知の差の照合に使う名前(`ka` の種 3 なら `ka`)である。
    pub name: &'static str,
    /// 種のある組の実体の名前(`ka=3`)である。
    /// 種の無い組では `name` と同じである。
    pub label: String,
    /// 実行系である。
    pub engine: Engine,
    /// `keleut run` に足す選択肢である。
    pub args: Vec<String>,
}

/// 組の名前の一覧である。
pub const NAMES: &[&str] = &[
    "dk", "dk-ir", "kf", "ki", "kj", "kt", "ka", "km", "kw", "x-ir", "kr-ki", "kr-kj", "kr-kt",
    "kr-ka", "kr-km", "kr-kw",
];

/// いま走らせられる組である。
///
/// RustKeleut の後段は M88 から順に加わるので、ここに名前を足していく。
pub const AVAILABLE: &[&str] = &[
    "dk", "dk-ir", "kf", "ki", "kj", "kt", "ka", "km", "kw", "x-ir", "kr-ki", "kr-kj", "kr-kt",
    "kr-ka", "kr-km", "kr-kw",
];

/// `--pairs` を省いたときに走らせる組である。
///
/// 計画 8.1 節は [`AVAILABLE`] のすべてを走らせるとするが、LLVM の後段の組 `kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km` は除く。
/// これらは feature `llvm` を付けて組んだ `keleut` でだけ走り(`llvm` は既定の feature でない)、
/// 付けずに組んだ `keleut` ではどのコマンドも未実装の 4 で終わるからである。
/// これらの組は `--pairs` で名を挙げて走らせる。
pub const DEFAULT: &[&str] = &["dk", "dk-ir", "kf", "ki", "kw", "x-ir", "kr-ki", "kr-kw"];

/// `kconform expect` が `keleut run` に足す選択肢(組 `ki`)である。
///
/// `keleut run` の既定のフロントエンドは M99 から `rust` なので、Diktor が KIR を書く組
/// (`ki kj kt ka km kw`)は `--frontend diktor` を明示する。
pub const EXPECT_ARGS: &[&str] = &[
    "--frontend",
    "diktor",
    "--backend",
    "interp",
    "--tier",
    "interp-only",
];

/// Diktor のフロントエンドを選ぶ選択肢である(組 `ki kj kt ka km kw` の先頭に置く)。
const DIKTOR_FRONT: &[&str] = &["--frontend", "diktor"];

/// Rust のフロントエンドを選ぶ選択肢である(組 `kr-*` の先頭に置く)。
const RUST_FRONT: &[&str] = &["--frontend", "rust"];

/// 後段の組の名前と、フロントエンドの選択肢を除いた `keleut run` の選択肢である。
///
/// 種の無い組だけを並べる。`ka` と `km` は [`expand`] が種ごとに広げる。
const BACKENDS: &[(&str, &[&str])] = &[
    ("ki", &["--backend", "interp", "--tier", "interp-only"]),
    ("kj", &["--backend", "jit", "--tier", "jit-only"]),
    ("kt", &["--tier", "threshold=1"]),
    ("kw", &["--backend", "wasm"]),
];

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// 組の名前を実体の列に広げる。
/// 知らない名前なら `None` を返す。
pub fn expand(name: &str) -> Option<Vec<Pair>> {
    let one = |name: &'static str, engine: Engine, args: Vec<String>| {
        vec![Pair {
            name,
            label: name.to_string(),
            engine,
            args,
        }]
    };
    let seeded = |name: &'static str, engine: Engine, front: &[&str], tier: &str, n: u32| {
        (1..=n)
            .map(|seed| {
                let mut args = strs(front);
                args.push("--tier".to_string());
                args.push(format!("{tier}={seed}"));
                Pair {
                    name,
                    label: format!("{name}={seed}"),
                    engine,
                    args,
                }
            })
            .collect()
    };
    let backend = |name: &'static str, engine: Engine, front: &[&str], back: &str| {
        let (_, b) = BACKENDS.iter().find(|(n, _)| *n == back)?;
        let mut args = strs(front);
        args.extend(strs(b));
        Some(one(name, engine, args))
    };
    let (k, d, r) = (Engine::Keleut, DIKTOR_FRONT, RUST_FRONT);
    Some(match name {
        "dk" => one("dk", Engine::Diktor, Vec::new()),
        "dk-ir" => one("dk-ir", Engine::DiktorIr, Vec::new()),
        "kf" => one("kf", Engine::Frontend, Vec::new()),
        "ki" => backend("ki", k, d, "ki")?,
        "kj" => backend("kj", k, d, "kj")?,
        "kt" => backend("kt", k, d, "kt")?,
        "ka" => seeded("ka", k, d, "alternating", 8),
        "km" => seeded("km", k, d, "mixed", 4),
        "kw" => backend("kw", k, d, "kw")?,
        "x-ir" => one("x-ir", Engine::RustIr, Vec::new()),
        "kr-ki" => backend("kr-ki", Engine::RustKeleut, r, "ki")?,
        "kr-kj" => backend("kr-kj", Engine::RustKeleut, r, "kj")?,
        "kr-kt" => backend("kr-kt", Engine::RustKeleut, r, "kt")?,
        "kr-ka" => seeded("kr-ka", Engine::RustKeleut, r, "alternating", 8),
        "kr-km" => seeded("kr-km", Engine::RustKeleut, r, "mixed", 4),
        "kr-kw" => backend("kr-kw", Engine::RustKeleut, r, "kw")?,
        _ => return None,
    })
}

/// 組の選択肢が LLVM の後段で走らせるものかどうかを返す。
///
/// `--backend jit` があるか、`--backend` が無く `interp-only` でない `--tier` があるときである
/// (`keleut run` の後段の選び方と同じ)。
pub fn is_jit_args<S: AsRef<str>>(args: &[S]) -> bool {
    let mut backend = None;
    let mut tier = None;
    for w in args.windows(2) {
        match w[0].as_ref() {
            "--backend" => backend = Some(w[1].as_ref()),
            "--tier" => tier = Some(w[1].as_ref()),
            _ => {}
        }
    }
    match backend {
        Some(b) => b == "jit",
        None => tier.is_some_and(|t| t != "interp-only"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn すべての名前を広げられる() {
        for n in NAMES {
            assert!(expand(n).is_some(), "{n}");
        }
        assert_eq!(expand("ka").unwrap().len(), 8);
        assert_eq!(expand("km").unwrap()[3].args[3], "mixed=4");
        assert!(expand("zz").is_none());
        assert_eq!(expand("kr-ka").unwrap().len(), 8);
        assert_eq!(expand("kr-ka").unwrap()[7].label, "kr-ka=8");
        for (kr, k) in [
            ("kr-ki", "ki"),
            ("kr-kj", "kj"),
            ("kr-kw", "kw"),
            ("kr-kt", "kt"),
            ("kr-ka", "ka"),
            ("kr-km", "km"),
        ] {
            let kr = expand(kr).unwrap();
            let k = expand(k).unwrap();
            assert_eq!(kr[0].engine, Engine::RustKeleut);
            assert_eq!(k[0].engine, Engine::Keleut);
            assert_eq!(kr[0].args[..2], ["--frontend", "rust"]);
            assert_eq!(k[0].args[..2], ["--frontend", "diktor"]);
            assert_eq!(kr[0].args[2..], k[0].args[2..]);
        }
        assert_eq!(expand("ki").unwrap()[0].args, EXPECT_ARGS);
    }

    #[test]
    fn llvm_の後段の組を見分ける() {
        for n in ["kj", "kt", "ka", "km", "kr-kj", "kr-kt", "kr-ka", "kr-km"] {
            assert!(
                expand(n).unwrap().iter().all(|p| is_jit_args(&p.args)),
                "{n}"
            );
        }
        for n in ["dk", "ki", "kw", "kr-ki", "kr-kw", "x-ir"] {
            assert!(
                !expand(n).unwrap().iter().any(|p| is_jit_args(&p.args)),
                "{n}"
            );
        }
    }

    #[test]
    fn 使える組と既定の組は名前の一覧にある() {
        for n in AVAILABLE.iter().chain(DEFAULT) {
            assert!(NAMES.contains(n));
        }
    }
}
