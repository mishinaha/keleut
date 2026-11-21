//! サブコマンド `keleut compat-diktor` である。
//!
//! `keleut compat-diktor [RUN-OPTIONS --] DIKTOR-ARGS…` は、Diktor のオプションを Diktor と同じ規則で解釈し、対応する keleut のサブコマンドを同じプロセスの中で呼ぶ。
//! kconform の shim が、Diktor を実行する呼び出しに現在のペアのオプションを付けて渡すために使う。
//!
//! オプションの読み方は、Diktor の引数解析(driver.ml の `parse_args`)と同じである。
//!
//! - モードのオプション `--type-check`、`--repl`、`--dump-tokens`、`--dump-ast` は、後に書いたものが優先される。
//!   モードのオプションがなければ、プログラムを実行する(`--strict-exhaustive` だけを書いた呼び出しも実行になる)。
//! - `--no-prelude`、`--prelude PATH`、`--import-path DIR`(繰り返せる)、`--strict-exhaustive` を受理する。
//! - 次の呼び出しは、使い方のエラー(終了コード 64)になる。
//!   - ハイフンで始まる未知の引数(`--help` と `-h` を含む)を書いた呼び出し。
//!   - `--prelude` か `--import-path` に値を書かなかった呼び出し。
//!   - 実行、型検査、ダンプのどれかで、入力ファイルを 1 つも書かなかった呼び出し。
//!   - `--repl` に入力ファイルを書いた呼び出し。
//! - Diktor の IR のオプション(`--emit-ir`、`--emit-ir-text`、`--run-ir`、`--via-ir`、`--dump-ir-tables`、`--max-depth`)は解釈せず、使い方のエラーにする。
//!   これらは Diktor 自身の IR の経路をテストするためのオプションだからである。
//!   環境変数 `DIKTOR_VIA_IR` も読まない。
//! - 入力ファイルは、名前が `.kir` や `.kirt` で終わっていても、Diktor と同じく Keleut のソースとして読む。
//!   そのために、`keleut run` の内部オプション `--source-only` を渡す。
//!
//! `RUN-OPTIONS` は `keleut run` の実行オプション(`--backend`、`--tier`、`--max-depth`、`--native-stack`、`--frontend`、`--gc-stress`、`--no-yield-prune`)である。
//! 引数の先頭に `RUN-OPTIONS` だけを並べて `--` で区切ると、`--` より後が Diktor の引数になる。
//! `--` は Diktor の引数の始まりを表す区切りで、POSIX の「オプションの終わり」ではない。
//! `--` より後の `--no-prelude` なども、オプションとして読む。
//! `--` より後に実行オプションを書くと、Diktor と同じく未知のオプションとして拒否する(`--max-depth` は IR のオプションとして拒否する)。
//! `--` がなければ、`--max-depth` 以外の実行オプションを Diktor のオプションと混ぜて並べてよい。
//! `--max-depth` は、Diktor では `--run-ir` と `--via-ir` のためのオプションで、構文木の評価器には影響しない。
//! そのため、`keleut run` に論理的な深さの上限として渡すのは、`--` の前に書いたときだけである。
//!
//! モードごとの転送先は次のとおりである。
//! 表の `OPTIONS` は、呼び出しに書かれた `--no-prelude`、`--prelude PATH`、`--import-path DIR`、`--strict-exhaustive` である。
//!
//! | モード | 転送先 |
//! |---|---|
//! | 実行 | `keleut run --source-only RUN-OPTIONS OPTIONS FILE…` |
//! | `--type-check` | `keleut check [--native-stack N] OPTIONS FILE…`(`RUN-OPTIONS` からは `--native-stack` だけを使い、`keleut run` がフロントエンドに使うスタックサイズに換算して渡す) |
//! | `--dump-tokens`、`--dump-ast` | `keleut dump tokens|ast FILE…`(Diktor と同じく、プレリュードと import のオプションは使わない) |
//! | `--repl` | `keleut repl RUN-OPTIONS --no-prelude --prelude PATH`(`--import-path` と `--strict-exhaustive` は使わない) |
//!
//! `--native-stack` を型検査にも渡すのは、実行と型検査で構文解析の上限を同じにするためである。
//! `keleut run` も、`--native-stack` からフロントエンドのスタックを決める。
//!
//! 終了コードは、転送先のサブコマンドのものである。
//! 使い方のエラーのメッセージは keleut の形式(`keleut compat-diktor: 理由` と使い方の説明)で、Diktor のメッセージとは違う。

use crate::exit;

/// `keleut compat-diktor` の使い方の説明(usage)である。
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

/// Diktor のモードである。モードのオプションで決まり、どれも書かれていなければ `Run` になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    TypeCheck,
    DumpTokens,
    DumpAst,
    Repl,
}

/// `keleut run` の実行オプションと、それぞれが取る値の個数である。
const RUN_OPTIONS: &[(&str, usize)] = &[
    ("--backend", 1),
    ("--tier", 1),
    ("--max-depth", 1),
    ("--native-stack", 1),
    ("--frontend", 1),
    ("--gc-stress", 0),
    ("--no-yield-prune", 0),
];

/// Diktor の IR のオプションである。解釈せず、使い方のエラーにする。
const IR_FLAGS: &[&str] = &[
    "--emit-ir",
    "--emit-ir-text",
    "--run-ir",
    "--via-ir",
    "--dump-ir-tables",
    "--max-depth",
];

/// 呼び出しを解釈した結果である。
#[derive(Debug, PartialEq, Eq)]
struct Call {
    mode: Mode,
    /// `keleut run` の実行オプション(書いた順)。
    run: Vec<String>,
    no_prelude: bool,
    prelude: Option<String>,
    import_path: Vec<String>,
    strict: bool,
    files: Vec<String>,
}

/// 引数の先頭にある `RUN-OPTIONS --` の長さ(`--` を含む引数の個数)を返す。
///
/// 先頭から実行オプションだけが並び、その直後が `--` のときだけ、区切りがあるとみなす。
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
    // 区切りがあれば、その前は実行オプションだけで、後は Diktor の引数だけである。
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
    // 型検査に渡す `--native-stack` の値は、実行と同じ規則で検査する(実行では `keleut run` が検査する)。
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
    /// プレリュードのオプション(`--no-prelude`、`--prelude PATH`)を、引数の並びにして返す。
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

    /// フロントエンドのオプション(プレリュード、import の検索パス、`--strict-exhaustive`)を、引数の並びにして返す。
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

    /// 型検査に渡す `--native-stack` の引数を返す。実行オプションに `--native-stack` がなければ空である。
    ///
    /// 実行オプションの最後の `--native-stack N` を、`keleut run` がフロントエンドに使うスタックサイズのバイト数に換算して渡す。
    /// 換算の規則は [`crate::emit_kir_cmd::front_stack_for_run`] にあり、下限より小さい値は下限に引き上げる。
    /// 値は [`parse`] が `keleut run` と同じ規則で検査してある。
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

    /// 転送先のサブコマンドに渡す引数の並び(先頭はサブコマンドの名前)を返す。
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
                // 実行オプションのうち、`keleut check` が受理するのは `--native-stack` だけである。
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
        // `-` で始まるファイル名は、Diktor と同じく未知のオプションとして `parse` が拒否するので、ここには来ない。
        v.extend(self.files.iter().cloned());
        v
    }
}

/// `keleut compat-diktor …` のエントリポイントである。引数の先頭は `compat-diktor` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    // `--help` と `-h` も、Diktor と同じく未知のオプションとして使い方のエラーにする。
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
        // 両方を書くと `--no-prelude` と `--prelude` の両方を渡す(どちらが優先されるかは、フロントエンドが Diktor と同じ規則で決める)。
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
        // オプションの値は、オプションとして読まない。
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
