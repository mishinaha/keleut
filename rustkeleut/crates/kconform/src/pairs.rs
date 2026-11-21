//! テストを実行するペアの定義である。
//!
//! ペアは、KIR のライタ(KIR を書き出す処理系)と、その KIR を実行する処理系の組み合わせである。
//! `ka`、`km`、`kr-ka`、`kr-km` はシードごとに 1 回ずつ実行するので、1 つの名前を複数の個々の構成に展開する。

/// ペアの処理系。KIR を書き出す処理系と、それを実行する処理系の組み合わせを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Diktor の構文木の評価器(ペア `dk`)。ペアの間の比較の基準になる。
    Diktor,
    /// `diktor --emit-ir` の後の `diktor --run-ir`(ペア `dk-ir`)。
    DiktorIr,
    /// Diktor の構文木の評価器で実行し、フロントエンドの段階だけを RustKeleut で実行する(ペア `kf`)。
    Frontend,
    /// `keleut run --frontend diktor` にペアのオプションを追加したもの(ペア `ki`、`kj`、`kt`、`ka`、`km`、`kw`)。
    /// Diktor が書いた KIR を RustKeleut のバックエンドが実行する。
    Keleut,
    /// `keleut emit-kir` の後の `diktor --run-ir`(ペア `x-ir`)。
    /// Rust のフロントエンドが書いた KIR を Diktor が実行する。
    RustIr,
    /// `keleut run --frontend rust` にペアのオプションを追加したもの(ペア `kr-*`)。
    /// Rust のフロントエンドが書いた KIR を RustKeleut のバックエンドが実行する。
    RustKeleut,
}

/// 実行するペアの、個々の構成 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// 既知の差分の照合と shim への指定(`KCONFORM_PAIR`)に使う、ペアの名前(`ka` のシード 3 なら `ka`)。
    pub name: &'static str,
    /// 表示に使う、個々の構成の名前(`ka=3`)。
    /// シードのないペアでは `name` と同じである。
    pub label: String,
    /// ペアの処理系。
    pub engine: Engine,
    /// `keleut run` に追加するオプション。
    pub args: Vec<String>,
}

/// ペアの名前の一覧。
pub const NAMES: &[&str] = &[
    "dk", "dk-ir", "kf", "ki", "kj", "kt", "ka", "km", "kw", "x-ir", "kr-ki", "kr-kj", "kr-kt",
    "kr-ka", "kr-km", "kr-kw",
];

/// 現在実行できるペア。
///
/// [`NAMES`] のうち、実行に必要な実装がそろったペアを並べる。
/// 現在は [`NAMES`] のすべてが入っている。
/// [`NAMES`] にあってここにない名前を `--pairs` に書くと、kconform は `kconform: ペア {n} はまだ使えません(使えるペア: …)` と書いて終了コード 64 で終了する。
pub const AVAILABLE: &[&str] = &[
    "dk", "dk-ir", "kf", "ki", "kj", "kt", "ka", "km", "kw", "x-ir", "kr-ki", "kr-kj", "kr-kt",
    "kr-ka", "kr-km", "kr-kw",
];

/// `--pairs` を省いたときに実行するペア。
///
/// [`AVAILABLE`] のすべてである。
/// LLVM ペア([`needs_jit`])は、既定の feature `llvm` を有効にしたままビルドした `keleut` でだけ実行できる。
/// `--no-default-features` でビルドした `keleut` では、kconform は LLVM ペアを除き、除いたペアを標準エラーに書く。
/// `--suite depth` では、kconform は `dk` と `kf` も除く。
pub const DEFAULT: &[&str] = AVAILABLE;

/// ペアの名前が LLVM ペア(`kj`、`kt`、`ka`、`km`、`kr-kj`、`kr-kt`、`kr-ka`、`kr-km`)なら真を返す。
///
/// ペアの個々の構成のオプションで判別する([`is_jit_args`])。
/// 未知の名前なら `false` を返す。
pub fn needs_jit(name: &str) -> bool {
    expand(name).is_some_and(|ps| ps.iter().any(|p| is_jit_args(&p.args)))
}

/// `keleut --version` の出力から、その `keleut` がバックエンド `jit` を持つかどうかを返す。
///
/// `backends:` で始まる最初の行(`keleut --version` は 2 行目に書く)に `jit` があれば、持つと判定する。
/// その行がない出力なら、判定できないので `None` を返す。
pub fn version_has_jit(out: &str) -> Option<bool> {
    out.lines()
        .find_map(|l| l.strip_prefix("backends:"))
        .map(|bs| bs.split_whitespace().any(|b| b == "jit"))
}

/// `kconform expect` が `keleut run` に追加するオプション。ペア `ki` のオプションと同じである。
///
/// `keleut run` の既定のフロントエンドは `rust` なので、Diktor が KIR を書くペア
/// (`ki kj kt ka km kw`)は `--frontend diktor` を明示する。
pub const EXPECT_ARGS: &[&str] = &[
    "--frontend",
    "diktor",
    "--backend",
    "interp",
    "--tier",
    "interp-only",
];

/// Diktor のフロントエンドを選ぶオプション。ペア `ki kj kt ka km kw` のオプションの先頭に置く。
const DIKTOR_FRONT: &[&str] = &["--frontend", "diktor"];

/// Rust のフロントエンドを選ぶオプション。ペア `kr-*` のオプションの先頭に置く。
const RUST_FRONT: &[&str] = &["--frontend", "rust"];

/// バックエンドのペアの名前と、フロントエンドのオプションを除いた `keleut run` のオプションの対応。
///
/// シードのないペアだけを並べる。
/// `ka` と `km` は [`expand`] がシードごとに展開する。
const BACKENDS: &[(&str, &[&str])] = &[
    ("ki", &["--backend", "interp", "--tier", "interp-only"]),
    ("kj", &["--backend", "jit", "--tier", "jit-only"]),
    ("kt", &["--tier", "threshold=1"]),
    ("kw", &["--backend", "wasm"]),
];

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// ペアの名前を、個々の構成の並びに展開する。
/// 未知の名前なら `None` を返す。
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

/// ペアのオプションが LLVM バックエンドで実行するものなら真を返す。
///
/// `--backend jit` があるか、`--backend` がなく `interp-only` でない `--tier` があるときに真を返す。
/// `--backend` か `--tier` を書いたときの、`keleut run` のバックエンドの選び方と同じである。
/// どちらもなければ偽を返す(`dk` のように、オプションのないペアが該当する)。
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
    fn 既定の組は使える組のすべてで_llvm_の組を見分ける() {
        assert_eq!(DEFAULT, AVAILABLE);
        let jit: Vec<&str> = NAMES.iter().copied().filter(|n| needs_jit(n)).collect();
        assert_eq!(
            jit,
            ["kj", "kt", "ka", "km", "kr-kj", "kr-kt", "kr-ka", "kr-km"]
        );
        assert!(!needs_jit("zz"));
    }

    #[test]
    fn keleut_の版の出力から後段_jit_を読む() {
        assert_eq!(
            version_has_jit("keleut 0.1.0\nbackends: interp jit wasm\n"),
            Some(true)
        );
        assert_eq!(
            version_has_jit("keleut 0.1.0\nbackends: interp wasm\n"),
            Some(false)
        );
        assert_eq!(version_has_jit("keleut 0.1.0\n"), None);
    }

    #[test]
    fn 使える組と既定の組は名前の一覧にある() {
        for n in AVAILABLE.iter().chain(DEFAULT) {
            assert!(NAMES.contains(n));
        }
    }
}
