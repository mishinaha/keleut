//! サブコマンド `keleut compat-diktor` である(計画 261024-1 の 6.2 節と 8.1 節)。
//!
//! `keleut compat-diktor [RUN-OPTIONS --] DIKTOR-ARGS…` は、Diktor の旗を Diktor と同じに解釈し、
//! 対応する keleut のサブコマンドをプロセスの中で呼ぶ。
//! kconform の shim が、Diktor の実行の呼び出しにいまの組の選択肢を足して渡すためのものである。
//!
//! 旗の読み方は Diktor の引数解析(driver.ml の `parse_args`)と同じである。
//!
//! - モードの旗 `--type-check`、`--repl`、`--dump-tokens`、`--dump-ast` は、後に書いたものが効く。
//!   モードの旗が無ければ実行である(`--strict-exhaustive` だけの呼び出しも実行である)。
//! - `--no-prelude`、`--prelude PATH`、`--import-path DIR`(繰り返せる)、`--strict-exhaustive` を受ける。
//! - ハイフンで始まる知らない引数(`--help` と `-h` を含む)、値の無い `--prelude` と `--import-path`、
//!   入力の無い実行と型検査とダンプ、入力のある `--repl` は、使い方の誤り(64)にする。
//! - Diktor の IR の旗(`--emit-ir`、`--emit-ir-text`、`--run-ir`、`--via-ir`、`--dump-ir-tables`、`--max-depth`)は解釈せず、
//!   使い方の誤りにする(計画 8.1 節。これらは Diktor 自身の IR の経路を試す旗である)。
//!   環境変数 `DIKTOR_VIA_IR` も見ない。
//! - 入力のファイルは、名前が `.kir` や `.kirt` で終わっても、Diktor と同じく Keleut のソースとして読む
//!   (`keleut run` の内部の旗 `--source-only` を渡す)。
//!
//! `RUN-OPTIONS` は `keleut run` の後段の選択肢(`--backend`、`--tier`、`--max-depth`、`--native-stack`、
//! `--frontend`、`--gc-stress`、`--no-yield-prune`)である。
//! 引数の先頭に `RUN-OPTIONS` だけを並べて `--` で区切ると、`--` より後が Diktor の引数になる。
//! `--` は「Diktor の引数がここから始まる」という印で、POSIX の「旗の終わり」ではない(後の `--no-prelude` なども旗として読む)。
//! `--` より後に後段の選択肢を書くと、Diktor と同じく知らない旗(`--max-depth` は IR の旗)として断る。
//! `--` が無ければ、`--max-depth` のほかの後段の選択肢を Diktor の旗と混ぜて並べてよい。
//! `--max-depth` は Diktor では `--run-ir` と `--via-ir` の旗で、木の評価器には効かないので、
//! `keleut run` の論理的な深さの限度として渡すのは `--` の前に書いたときだけである。
//!
//! モードごとの行き先は次のとおりである。
//!
//! | モード | 行き先 |
//! |---|---|
//! | 実行 | `keleut run --source-only RUN-OPTIONS 旗 FILE…` |
//! | `--type-check` | `keleut check [--native-stack N] 旗 FILE…`(`RUN-OPTIONS` からは `--native-stack` だけを使い、`keleut run` がフロントエンドに使う大きさにして渡す) |
//! | `--dump-tokens`、`--dump-ast` | `keleut dump tokens|ast FILE…`(Diktor と同じく、プレリュードと import の旗は使わない) |
//! | `--repl` | `keleut repl RUN-OPTIONS --no-prelude --prelude PATH`(`--import-path` と `--strict-exhaustive` は使わない) |
//!
//! `--native-stack` を型検査にも渡すのは、実行と型検査で構文解析の限度を同じにするためである
//! (`keleut run` も `--native-stack` でフロントエンドのスタックを決める。計画 6.11 節)。
//!
//! 終了状態は行き先のサブコマンドのものである。
//! 使い方の誤りの文面は keleut の形(`keleut compat-diktor: 理由` と使い方)で、Diktor の文面とは違う。

use crate::exit;

/// 使い方である。
pub const USAGE: &str = "\
usage: keleut compat-diktor [RUN-OPTIONS --] [DIKTOR-OPTIONS] FILE.kel...
interprets Diktor's options the same way and runs the matching keleut command:
  (no mode)            keleut run RUN-OPTIONS ... FILE... (every FILE is read as source)
  --type-check         keleut check [--native-stack N] ... FILE...
  --dump-tokens        keleut dump tokens FILE...
  --dump-ast           keleut dump ast FILE...
  --repl               keleut repl RUN-OPTIONS ... (no input files)
DIKTOR-OPTIONS: --no-prelude, --prelude PATH, --import-path DIR, --strict-exhaustive
  (the last mode option wins; Diktor's IR options, --max-depth among them, are not interpreted)
RUN-OPTIONS: --backend, --tier, --max-depth, --native-stack, --frontend (one value each),
  --gc-stress, --no-yield-prune (see keleut run --help); before --, only RUN-OPTIONS;
  without --, all but --max-depth may be mixed with DIKTOR-OPTIONS
";

/// Diktor のモードである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    TypeCheck,
    DumpTokens,
    DumpAst,
    Repl,
}

/// `keleut run` の後段の選択肢と、その値の数である。
const RUN_OPTIONS: &[(&str, usize)] = &[
    ("--backend", 1),
    ("--tier", 1),
    ("--max-depth", 1),
    ("--native-stack", 1),
    ("--frontend", 1),
    ("--gc-stress", 0),
    ("--no-yield-prune", 0),
];

/// Diktor の IR の旗である(解釈しない)。
const IR_FLAGS: &[&str] = &[
    "--emit-ir",
    "--emit-ir-text",
    "--run-ir",
    "--via-ir",
    "--dump-ir-tables",
    "--max-depth",
];

/// 解釈した呼び出しである。
#[derive(Debug, PartialEq, Eq)]
struct Call {
    mode: Mode,
    /// `keleut run` の後段の選択肢(書いた順)。
    run: Vec<String>,
    no_prelude: bool,
    prelude: Option<String>,
    import_path: Vec<String>,
    strict: bool,
    files: Vec<String>,
}

/// 引数の先頭の `RUN-OPTIONS --` の長さ(`--` を含む)を返す。
///
/// 先頭から後段の選択肢だけが並び、その直後が `--` のときだけ、区切りがあるとみなす。
fn separator(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            return Some(i + 1);
        }
        let (_, n) = RUN_OPTIONS.iter().find(|(f, _)| *f == args[i])?;
        i += 1 + n;
    }
    None
}

fn parse(args: &[String]) -> Result<Call, String> {
    let mut c = Call {
        mode: Mode::Run,
        run: Vec::new(),
        no_prelude: false,
        prelude: None,
        import_path: Vec::new(),
        strict: false,
        files: Vec::new(),
    };
    // 区切りがあれば、その前は後段の選択肢だけで、後は Diktor の引数だけである。
    let (mixed, start) = match separator(args) {
        Some(k) => {
            c.run.extend_from_slice(&args[..k - 1]);
            (false, k)
        }
        None => (true, 0),
    };
    let mut i = start;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--type-check" => c.mode = Mode::TypeCheck,
            "--repl" => c.mode = Mode::Repl,
            "--dump-tokens" => c.mode = Mode::DumpTokens,
            "--dump-ast" => c.mode = Mode::DumpAst,
            "--no-prelude" => c.no_prelude = true,
            "--strict-exhaustive" => c.strict = true,
            "--prelude" => {
                i += 1;
                c.prelude = Some(args.get(i).ok_or("--prelude requires a path")?.clone());
            }
            "--import-path" => {
                i += 1;
                let d = args.get(i).ok_or("--import-path requires a directory")?;
                c.import_path.push(d.clone());
            }
            _ if IR_FLAGS.contains(&a) => {
                return Err(format!("Diktor's IR option is not interpreted: {a}"));
            }
            _ => {
                if let Some((_, n)) = RUN_OPTIONS.iter().find(|(f, _)| mixed && *f == a) {
                    if i + n >= args.len() {
                        return Err(format!("{a} needs a value"));
                    }
                    c.run.extend_from_slice(&args[i..=i + n]);
                    i += n;
                } else if a.starts_with('-') {
                    return Err(format!("unknown option: {a}"));
                } else {
                    c.files.push(a.to_string());
                }
            }
        }
        i += 1;
    }
    // 型検査に渡す `--native-stack` の値は、実行と同じ規則で確かめる(実行では `keleut run` が確かめる)。
    if c.mode == Mode::TypeCheck {
        for w in c.run.windows(2).filter(|w| w[0] == "--native-stack") {
            match crate::run_cmd::parse_size(&w[1]) {
                Some(n) if n >= keleut_interp::MIN_NATIVE_STACK => {}
                _ => return Err(format!("bad --native-stack value: {}", w[1])),
            }
        }
    }
    match c.mode {
        Mode::Repl if !c.files.is_empty() => Err("--repl takes no input files".to_string()),
        Mode::Repl => Ok(c),
        _ if c.files.is_empty() => Err("no input files".to_string()),
        _ => Ok(c),
    }
}

impl Call {
    /// プレリュードの旗(`--no-prelude`、`--prelude PATH`)である。
    fn prelude_args(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.no_prelude {
            v.push("--no-prelude".to_string());
        }
        if let Some(p) = &self.prelude {
            v.push("--prelude".to_string());
            v.push(p.clone());
        }
        v
    }

    /// フロントエンドの旗(プレリュード、import、網羅性)である。
    fn front_args(&self) -> Vec<String> {
        let mut v = self.prelude_args();
        for d in &self.import_path {
            v.push("--import-path".to_string());
            v.push(d.clone());
        }
        if self.strict {
            v.push("--strict-exhaustive".to_string());
        }
        v
    }

    /// 型検査に渡す `--native-stack` の引数である(無ければ空)。
    ///
    /// 後段の選択肢の最後の `--native-stack N` を、`keleut run` がフロントエンドに使う大きさ
    /// ([`crate::emit_kir_cmd::front_stack_for_run`]。下限より小さい値は下限に上げる)のバイト数にして渡す。
    /// 値は [`parse`] が `keleut run` と同じ規則で確かめてある。
    fn native_stack(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut i = 0;
        while i < self.run.len() {
            let n = RUN_OPTIONS
                .iter()
                .find(|(f, _)| *f == self.run[i])
                .map_or(0, |(_, n)| *n);
            if self.run[i] == "--native-stack"
                && let Some(b) = crate::run_cmd::parse_size(&self.run[i + 1])
            {
                let b = crate::emit_kir_cmd::front_stack_for_run(b);
                found = vec!["--native-stack".to_string(), b.to_string()];
            }
            i += 1 + n;
        }
        found
    }

    /// 行き先のサブコマンドの引数の列(先頭はサブコマンドの名前)である。
    fn target(&self) -> Vec<String> {
        let mut v = Vec::new();
        match self.mode {
            Mode::Run => {
                v.push("run".to_string());
                v.push("--source-only".to_string());
                v.extend(self.run.iter().cloned());
                v.extend(self.front_args());
            }
            Mode::TypeCheck => {
                v.push("check".to_string());
                // 後段の選択肢のうち、`keleut check` が受けるのは `--native-stack` だけである。
                v.extend(self.native_stack());
                v.extend(self.front_args());
            }
            Mode::DumpTokens | Mode::DumpAst => {
                v.push("dump".to_string());
                v.push(
                    if self.mode == Mode::DumpTokens {
                        "tokens"
                    } else {
                        "ast"
                    }
                    .to_string(),
                );
            }
            Mode::Repl => {
                v.push("repl".to_string());
                v.extend(self.run.iter().cloned());
                v.extend(self.prelude_args());
            }
        }
        // `-` で始まるファイルの名前は、Diktor では知らない旗として断られるのでここには来ない。
        v.extend(self.files.iter().cloned());
        v
    }
}

/// `keleut compat-diktor …` の入口である。引数の先頭は `compat-diktor` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    // `--help` と `-h` も、Diktor と同じく知らない旗として使い方の誤りにする。
    let call = match parse(rest) {
        Ok(c) => c,
        Err(msg) => {
            eprint!("keleut compat-diktor: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    let t = call.target();
    match call.mode {
        Mode::Run => crate::run_cmd::main(&t),
        Mode::TypeCheck => crate::check_cmd::main(&t),
        Mode::DumpTokens | Mode::DumpAst => crate::dump_cmd::main(&t),
        Mode::Repl => crate::repl_cmd::main(&t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    fn target(xs: &[&str]) -> Vec<String> {
        parse(&argv(xs)).unwrap().target()
    }

    #[test]
    fn 実行は組の選択肢と旗を_keleut_run_に渡す() {
        assert_eq!(
            target(&[
                "--frontend",
                "rust",
                "--backend",
                "interp",
                "--no-prelude",
                "--import-path",
                "lib",
                "--gc-stress",
                "x.kel",
                "y.kel",
            ]),
            argv(&[
                "run",
                "--source-only",
                "--frontend",
                "rust",
                "--backend",
                "interp",
                "--gc-stress",
                "--no-prelude",
                "--import-path",
                "lib",
                "x.kel",
                "y.kel",
            ])
        );
        // `--strict-exhaustive` だけの呼び出しは実行である。
        assert_eq!(
            target(&["--strict-exhaustive", "x.kel"]),
            argv(&["run", "--source-only", "--strict-exhaustive", "x.kel"])
        );
        // 両方を書くと `--no-prelude` と `--prelude` の両方を渡す(どちらが勝つかはフロントエンドが Diktor と同じに決める)。
        assert_eq!(
            target(&["--prelude", "p.kel", "--no-prelude", "x.kel"]),
            argv(&[
                "run",
                "--source-only",
                "--no-prelude",
                "--prelude",
                "p.kel",
                "x.kel"
            ])
        );
    }

    #[test]
    fn モードは後に書いたものが効く() {
        assert_eq!(
            target(&["--dump-ast", "--type-check", "--backend", "wasm", "x.kel"]),
            argv(&["check", "x.kel"])
        );
        assert_eq!(
            target(&["--type-check", "--dump-tokens", "--no-prelude", "x.kel"]),
            argv(&["dump", "tokens", "x.kel"])
        );
        assert_eq!(
            target(&[
                "--type-check",
                "--strict-exhaustive",
                "--prelude",
                "p",
                "a.kel"
            ]),
            argv(&["check", "--prelude", "p", "--strict-exhaustive", "a.kel"])
        );
        assert_eq!(
            target(&[
                "--frontend",
                "rust",
                "--repl",
                "--import-path",
                "lib",
                "--strict-exhaustive",
                "--prelude",
                "p.kel",
            ]),
            argv(&["repl", "--frontend", "rust", "--prelude", "p.kel"])
        );
    }

    #[test]
    fn 使い方の誤りを_diktor_と同じに見分ける() {
        for a in [
            &[][..],
            &["--no-prelude"],
            &["--type-check"],
            &["--repl", "x.kel"],
            &["--typo", "x.kel"],
            &["x.kel", "--prelude"],
            &["x.kel", "--import-path"],
            &["--emit-ir", "o.kir", "x.kel"],
            &["--via-ir", "x.kel"],
            &["--dump-ir-tables"],
            &["x.kel", "--backend"],
            &["--help"],
            &["-h", "x.kel"],
            &["--max-depth", "5", "x.kel"],
            &["--backend", "interp", "--max-depth", "5", "x.kel"],
            &["--backend", "interp", "--", "--max-depth", "5", "x.kel"],
            &["--backend", "interp", "--", "--backend", "wasm", "x.kel"],
            &["--", "--"],
            &["x.kel", "--"],
        ] {
            assert!(parse(&argv(a)).is_err(), "{a:?}");
        }
        assert!(parse(&argv(&["--repl"])).is_ok());
        // 旗の値は旗として読まない。
        assert_eq!(
            target(&["--prelude", "--repl", "x.kel"]),
            argv(&["run", "--source-only", "--prelude", "--repl", "x.kel"])
        );
    }

    #[test]
    fn 区切りの前は後段の選択肢で後は_diktor_の引数である() {
        assert_eq!(
            target(&[
                "--max-depth",
                "5",
                "--backend",
                "interp",
                "--",
                "--no-prelude",
                "x.kel"
            ]),
            argv(&[
                "run",
                "--source-only",
                "--max-depth",
                "5",
                "--backend",
                "interp",
                "--no-prelude",
                "x.kel",
            ])
        );
        assert_eq!(target(&["--", "--repl"]), argv(&["repl"]));
        // 型検査には `--native-stack` だけを渡す。
        assert_eq!(
            target(&[
                "--native-stack",
                "1m",
                "--backend",
                "wasm",
                "--native-stack",
                "512k",
                "--",
                "--type-check",
                "x.kel",
            ]),
            vec![
                "check".to_string(),
                "--native-stack".to_string(),
                crate::emit_kir_cmd::front_stack_for_run(512 << 10).to_string(),
                "x.kel".to_string(),
            ]
        );
        // 名前が `.kir` で終わる入力もソースとして読ませる。
        assert_eq!(
            target(&["x.kir", "y.kel"]),
            argv(&["run", "--source-only", "x.kir", "y.kel"])
        );
    }
}
