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
    "dk", "dk-ir", "kf", "ki", "kj", "kt", "ka", "km", "kw", "x-ir", "kr-ki",
];

/// いま走らせられる組である。
///
/// RustKeleut の後段は M88 から順に加わるので、ここに名前を足していく。
pub const AVAILABLE: &[&str] = &[
    "dk", "dk-ir", "kf", "ki", "kj", "kt", "ka", "km", "kw", "x-ir", "kr-ki",
];

/// `--pairs` を省いたときに走らせる組である。
///
/// 計画 8.1 節は [`AVAILABLE`] のすべてを走らせるとするが、LLVM の後段の組 `kj`、`kt`、`ka`、`km` は除く。
/// これらは feature `llvm` を付けて組んだ `keleut` でだけ走り(`llvm` は既定の feature でない)、
/// 付けずに組んだ `keleut` ではどのコマンドも未実装の 4 で終わるからである。
/// これらの組は `--pairs` で名を挙げて走らせる。
pub const DEFAULT: &[&str] = &["dk", "dk-ir", "kf", "ki", "kw", "x-ir", "kr-ki"];

/// `kconform expect` が `keleut run` に足す選択肢(組 `ki`)である。
pub const EXPECT_ARGS: &[&str] = &["--backend", "interp", "--tier", "interp-only"];

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// 組の名前を実体の列に広げる。
/// 知らない名前なら `None` を返す。
pub fn expand(name: &str) -> Option<Vec<Pair>> {
    let one = |name: &'static str, engine: Engine, args: &[&str]| {
        vec![Pair {
            name,
            label: name.to_string(),
            engine,
            args: strs(args),
        }]
    };
    let seeded = |name: &'static str, tier: &str, n: u32| {
        (1..=n)
            .map(|seed| Pair {
                name,
                label: format!("{name}={seed}"),
                engine: Engine::Keleut,
                args: vec!["--tier".to_string(), format!("{tier}={seed}")],
            })
            .collect()
    };
    Some(match name {
        "dk" => one("dk", Engine::Diktor, &[]),
        "dk-ir" => one("dk-ir", Engine::DiktorIr, &[]),
        "kf" => one("kf", Engine::Frontend, &[]),
        "ki" => one("ki", Engine::Keleut, EXPECT_ARGS),
        "kj" => one(
            "kj",
            Engine::Keleut,
            &["--backend", "jit", "--tier", "jit-only"],
        ),
        "kt" => one("kt", Engine::Keleut, &["--tier", "threshold=1"]),
        "ka" => seeded("ka", "alternating", 8),
        "km" => seeded("km", "mixed", 4),
        "kw" => one("kw", Engine::Keleut, &["--backend", "wasm"]),
        "x-ir" => one("x-ir", Engine::RustIr, &[]),
        "kr-ki" => one(
            "kr-ki",
            Engine::RustKeleut,
            &[
                "--frontend",
                "rust",
                "--backend",
                "interp",
                "--tier",
                "interp-only",
            ],
        ),
        _ => return None,
    })
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
        assert_eq!(expand("km").unwrap()[3].args[1], "mixed=4");
        assert!(expand("zz").is_none());
        let kr = expand("kr-ki").unwrap();
        assert_eq!(kr[0].engine, Engine::RustKeleut);
        assert_eq!(kr[0].args[..2], ["--frontend", "rust"]);
    }

    #[test]
    fn 使える組と既定の組は名前の一覧にある() {
        for n in AVAILABLE.iter().chain(DEFAULT) {
            assert!(NAMES.contains(n));
        }
    }
}
