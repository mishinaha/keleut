//! サブコマンド `keleut run` である(計画 261024-1 の 6.2 節)。
//!
//! `keleut run [OPTIONS] FILE.kel… | FILE.kir` は、プログラムを実行する。
//!
//! - `.kir`(バイナリ)か `.kirt`(テキスト)の 1 ファイルは、そのまま読む。
//! - `.kel` のファイルは、フロントエンドで KIR にする。
//!   フロントエンドは `--frontend diktor|rust` で選ぶ。
//!   既定は `rust` である(M99。速さの関門を通ったので切り替えた。計画 7 節)。
//!   - `--frontend diktor` は、`$KELEUT_DIKTOR`(無ければ PATH の `diktor`)を `--emit-ir` で子プロセスとして呼ぶ。
//!     Diktor が 0 で終わらなければ、その終了状態で終わる(診断は Diktor が標準エラーに書いたものである)。
//!   - `--frontend rust` は、keleut-front でプロセスの中で型検査して KIR を書く
//!     ([`crate::emit_kir_cmd::program_for_run`])。
//!     診断と終了状態は `diktor --emit-ir` と同じである。
//!     `--native-stack` は、インタプリタと JIT のスタックとともに、フロントエンドのスレッドのスタックも決める(計画 6.11 節)。
//!     フロントエンドは、構文解析の限度をそのスタックに収まる深さまで下げる(`keleut check --native-stack` と同じ)。
//!     フロントエンドが構文解析の限度の下限(64 段)に要るスタックより小さい値は、その大きさまで上げて使う
//!     ([`crate::emit_kir_cmd::front_stack_for_run`])。
//!     `--native-stack` を与えなければ、フロントエンドは既定の 8 GiB のスタックで走り、
//!     仮想メモリの上限があれば、スタックをそれに合わせて小さくし、入れ子の限度を下げる。
//!
//!   `--prelude`、`--no-prelude`、`--import-path`、`--strict-exhaustive` は、どちらのフロントエンドにもそのまま渡す。
//!
//! 読み込んだ KIR を検証し、LIR に下ろして、後段で実行する。
//! `--native-stack` を与えたときは、KIR の読み込みと検証のスタックもフロントエンドと同じ大きさにし、
//! 入れ子の限度をそれに収まる深さまで下げる(計画 6.11 節。[`kir_limits`])。
//! 後段はインタプリタ(`--backend interp`)、WasmGC(`--backend wasm`。M94。crate keleut-wasm が Node を起動する)、
//! LLVM の JIT(`--backend jit`。M91。crate keleut-llvm)である。
//! JIT は feature `llvm`(既定で有効。計画 6.1 節、P241)で組んだときだけ使え、`--no-default-features` で外して組めば未実装の 4 である。
//! 既定の後段は、feature `llvm` で組んだときは `jit`、外して組んだときは `interp` である(計画 6.2 節)。
//! `--backend jit` の段の設定(`--tier`。M92)は `interp-only`、`jit-only`、`threshold=N`、`alternating=SEED`、`mixed=SEED` で、
//! 既定は `threshold=1000` である(しきい値の既定は M93 で決める。P249)。
//! `--backend` を省いて `interp-only` 以外の `--tier` を与えたときは `jit` で走る。
//! `--backend interp` と `--backend wasm` は `--tier` を受け付けて使わない
//! (組 `kt` などの選択肢を足しても、`--backend interp` を書いたコマンドはインタプリタで走る)。
//!
//! 内部の旗 `--source-only`(使い方には載せない)を与えると、名前が `.kir` や `.kirt` で終わる入力も
//! Keleut のソースとしてフロントエンドに渡す。
//! `keleut compat-diktor` が、すべての入力をソースとして読む Diktor と同じにするために使う。
//!
//! `keleut dump bytecode` も同じ読み方で入力を LIR にし、インタプリタのバイトコードを印字する([`dump_bytecode`])。
//!
//! 終了状態は、プログラムの結果(0、3、4、74)、KIR の読み込みと検証の失敗が 65、
//! 下ろしと内部の誤りが 70、使い方の誤りと入力を読めないときが 64 である。

use crate::exit;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `keleut run` の使い方である。
pub const USAGE: &str = "\
usage: keleut run [OPTIONS] FILE.kel... | FILE.kir

options:
  --backend interp|jit|wasm   back end (default: jit, or interp in a build without the llvm feature;
                              jit needs the llvm feature, which is on by default)
  --tier MODE                 interp-only|jit-only|threshold=N|alternating=SEED|mixed=SEED
                              (jit only, ignored by --backend interp|wasm; default threshold=1000;
                              a mode other than interp-only selects --backend jit when --backend
                              is not given)
  --max-depth N               logical depth limit L (default 1048576)
  --native-stack BYTES        native stack of the mutator thread (suffix k, m or g; at least 512k;
                              interp and jit; ignored by --backend wasm), of the rust front-end
                              thread and of KIR reading and verification (lowers their nesting
                              limits to what fits; raised to the front end's minimum when smaller)
  --gc-stress                 collect garbage at the first allocation of every allocation site
                              and then at short jittered intervals (for testing;
                              interp and jit; ignored by --backend wasm)
  --no-yield-prune            treat every resume point as able to receive a yield (pass 11 off)
  --frontend diktor|rust      front end for .kel files (default: rust; diktor runs
                              $KELEUT_DIKTOR, else diktor on PATH)
  --prelude FILE, --no-prelude, --import-path DIR, --strict-exhaustive
                              passed to the front end
";

/// 解釈したコマンド行である。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    backend: Option<String>,
    tier: Option<String>,
    pub(crate) max_depth: u64,
    native_stack: Option<u64>,
    gc_stress: bool,
    yield_prune: bool,
    frontend: String,
    front_args: Vec<String>,
    files: Vec<PathBuf>,
    /// 名前によらず、すべての入力をソースとして読む(内部の旗 `--source-only`)。
    source_only: bool,
}

/// 大きさの綴り(`512k`、`64m`、`1g`、バイトの数)を読む。
pub(crate) fn parse_size(s: &str) -> Option<u64> {
    let (num, mul) = match s.as_bytes().last()? {
        b'k' | b'K' => (&s[..s.len() - 1], 1u64 << 10),
        b'm' | b'M' => (&s[..s.len() - 1], 1u64 << 20),
        b'g' | b'G' => (&s[..s.len() - 1], 1u64 << 30),
        _ => (s, 1),
    };
    num.parse::<u64>().ok()?.checked_mul(mul)
}

fn valid_tier(s: &str) -> bool {
    if s == "interp-only" || s == "jit-only" {
        return true;
    }
    match s.split_once('=') {
        Some(("threshold" | "alternating" | "mixed", n)) => n.parse::<u64>().is_ok(),
        _ => false,
    }
}

pub(crate) fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        backend: None,
        tier: None,
        max_depth: keleut_rt::DEFAULT_MAX_DEPTH,
        native_stack: None,
        gc_stress: false,
        yield_prune: true,
        frontend: "rust".to_string(),
        front_args: Vec::new(),
        files: Vec::new(),
        source_only: false,
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "--backend" => {
                let v = value(a)?;
                if !matches!(v.as_str(), "interp" | "jit" | "wasm") {
                    return Err(format!("bad --backend value: {v}"));
                }
                o.backend = Some(v);
            }
            "--tier" => {
                let v = value(a)?;
                if !valid_tier(&v) {
                    return Err(format!("bad --tier value: {v}"));
                }
                o.tier = Some(v);
            }
            "--max-depth" => {
                let v = value(a)?;
                o.max_depth = match v.parse::<u64>() {
                    Ok(n) if n >= 1 => n,
                    _ => return Err(format!("bad --max-depth value: {v}")),
                };
            }
            "--native-stack" => {
                let v = value(a)?;
                o.native_stack = match parse_size(&v) {
                    Some(n) if n >= keleut_interp::MIN_NATIVE_STACK => Some(n),
                    _ => return Err(format!("bad --native-stack value: {v}")),
                };
            }
            "--gc-stress" => o.gc_stress = true,
            "--no-yield-prune" => o.yield_prune = false,
            "--source-only" => o.source_only = true,
            "--frontend" => {
                let v = value(a)?;
                if !matches!(v.as_str(), "diktor" | "rust") {
                    return Err(format!("bad --frontend value: {v}"));
                }
                o.frontend = v;
            }
            "--no-prelude" | "--strict-exhaustive" => o.front_args.push(a.to_string()),
            "--prelude" | "--import-path" => {
                let v = value(a)?;
                o.front_args.push(a.to_string());
                o.front_args.push(v);
            }
            _ if a.starts_with('-') && a != "-" => return Err(format!("unknown option: {a}")),
            _ => o.files.push(PathBuf::from(a)),
        }
        i += 1;
    }
    if o.files.is_empty() {
        return Err("missing input file".to_string());
    }
    let kir = o.files.iter().filter(|f| o.kir_input(f)).count();
    if kir > 0 && (o.files.len() > 1 || !o.front_args.is_empty()) {
        return Err(
            "a .kir or .kirt file must be the only input and takes no front-end options".into(),
        );
    }
    Ok(o)
}

fn is_kir(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()), Some("kir" | "kirt"))
}

/// 一時の KIR のファイル。落としたときに消す。
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_kir() -> TempFile {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let name = format!(
        "keleut-run-{}-{}-{nanos}.kir",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    );
    TempFile(std::env::temp_dir().join(name))
}

/// `.kel` を Diktor で KIR にする。成功すればバイト列、失敗すれば終了状態を返す。
fn emit_with_diktor(o: &Options) -> Result<Vec<u8>, u8> {
    let diktor = std::env::var_os("KELEUT_DIKTOR").unwrap_or_else(|| "diktor".into());
    let tmp = temp_kir();
    let status = Command::new(&diktor)
        .args(&o.front_args)
        .arg("--emit-ir")
        .arg(&tmp.0)
        .args(&o.files)
        .status();
    let status = match status {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "keleut: フロントエンド {} を起動できません: {e}",
                Path::new(&diktor).display()
            );
            return Err(exit::USAGE);
        }
    };
    match status.code() {
        Some(0) => {}
        Some(c) => return Err(u8::try_from(c).unwrap_or(70)),
        None => {
            eprintln!("keleut: フロントエンドがシグナルで終わりました: {status}");
            return Err(70);
        }
    }
    std::fs::read(&tmp.0).map_err(|e| {
        eprintln!("keleut: フロントエンドの出力を読めません: {e}");
        70
    })
}

impl Options {
    /// `p` を KIR のファイルとして読むか(`--source-only` のときは読まない)。
    fn kir_input(&self, p: &Path) -> bool {
        !self.source_only && is_kir(p)
    }

    /// 後段の名前(`--backend` を省いたときの既定を当てたもの)。
    fn backend(&self) -> &str {
        match (self.backend.as_deref(), self.tier.as_deref()) {
            (Some(b), _) => b,
            (None, Some(t)) if t != "interp-only" => "jit",
            _ if cfg!(feature = "llvm") => "jit",
            _ => "interp",
        }
    }
}

fn run(o: &Options) -> u8 {
    if o.backend() == "wasm" {
        // WasmGC の後段(M94)。Node を子プロセスで起動する(crate keleut-wasm の launch)。
        let lp = match load(o) {
            Ok(p) => p,
            Err(c) => return c,
        };
        let closed = keleut_rt::out::STDOUT_WAS_CLOSED.load(std::sync::atomic::Ordering::Relaxed);
        return keleut_wasm::launch::run(
            &lp,
            &keleut_wasm::launch::RunOptions {
                max_depth: o.max_depth,
                stdout_closed: closed,
            },
        );
    }
    if o.backend() == "jit" {
        return run_jit(o);
    }
    let lp = match load(o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    keleut_interp::run_program(&lp, &interp_options(o))
}

fn interp_options(o: &Options) -> keleut_interp::RunOptions {
    keleut_interp::RunOptions {
        max_depth: o.max_depth,
        native_stack: o
            .native_stack
            .unwrap_or_else(|| keleut_rt::native_stack_for(o.max_depth)),
        gc_stress: o.gc_stress,
    }
}

/// LLVM の後段(M91、M92)で走らせる。
#[cfg(feature = "llvm")]
fn run_jit(o: &Options) -> u8 {
    let tier = match o.tier.as_deref() {
        None => keleut_llvm::DEFAULT_TIER,
        Some(t) => match keleut_llvm::Tier::parse(t) {
            Some(t) => t,
            None => {
                eprintln!("keleut: 内部エラー: 段の設定 {t} を読めません");
                return 70;
            }
        },
    };
    let lp = match load(o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let opts = keleut_llvm::RunOptions {
        base: interp_options(o),
        tier,
        ..Default::default()
    };
    keleut_llvm::run_program(&lp, &opts)
}

#[cfg(not(feature = "llvm"))]
fn run_jit(_: &Options) -> u8 {
    eprintln!("keleut: 未実装: 後段 jit(feature llvm を外して組みました)");
    exit::UNIMPLEMENTED
}

/// 入力を読み(`.kel` ならフロントエンドを通し)、KIR を検証して LIR に下ろす。
/// 失敗すれば、診断を標準エラーに書いて終了状態を返す。
pub(crate) fn load(o: &Options) -> Result<keleut_lir::Program, u8> {
    let direct = o.kir_input(&o.files[0]);
    let data = if direct {
        match std::fs::read(&o.files[0]) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "keleut: ファイルを開けません: {}: {e}",
                    o.files[0].display()
                );
                return Err(exit::USAGE);
            }
        }
    } else if o.frontend == "rust" {
        let stack = o.native_stack.map(crate::emit_kir_cmd::front_stack_for_run);
        let prog = crate::emit_kir_cmd::program_for_run(&o.files, &o.front_args, stack)?;
        return verify_and_lower(o, &prog, &kir_limits(o));
    } else {
        emit_with_diktor(o)?
    };
    let limits = kir_limits(o);
    let read = match o.files[0].extension().and_then(|e| e.to_str()) {
        Some("kir") if direct => kir::read_kir_with,
        Some("kirt") if direct => kir::read_kirt_with,
        _ => kir::read_any_with,
    };
    let prog = match read(&data, &limits) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("keleut: KIR を読み込めません: {}", e.message());
            return Err(e.exit_code() as u8);
        }
    };
    drop(data);
    verify_and_lower(o, &prog, &limits)
}

/// KIR の読み込み、検証、LIR への下ろしの限度を返す(計画 6.11 節)。
///
/// `--native-stack` を与えたときは、フロントエンドと同じ大きさ([`crate::emit_kir_cmd::front_stack_for_run`])の
/// スタックで読み込みと検証を走らせ、入れ子の限度をそのスタックに収まる深さまで下げる。
/// 下ろしの入れ子の限度も同じ値にする(下ろしのスタックは、その限度から決まる)。
/// 与えなければ、限度は `N_nest` で、スタックはそれに足りる大きさである。
fn kir_limits(o: &Options) -> kir::Limits {
    match o.native_stack {
        Some(s) => {
            kir::Limits::with_stack(kir::N_NEST, crate::emit_kir_cmd::front_stack_for_run(s))
        }
        None => kir::Limits::default(),
    }
}

/// KIR を検証して LIR に下ろす。失敗すれば、診断を標準エラーに書いて終了状態を返す。
fn verify_and_lower(
    o: &Options,
    prog: &kir::Program,
    limits: &kir::Limits,
) -> Result<keleut_lir::Program, u8> {
    if let Err(e) = kir::verify_with(prog, limits) {
        eprintln!("keleut: KIR の検証に失敗しました: {}", e.message());
        return Err(e.exit_code() as u8);
    }
    keleut_lir::lower_opts(prog, limits.max_nest, o.yield_prune).map_err(|e| {
        eprintln!("keleut: {e}");
        e.exit_code() as u8
    })
}

/// `keleut dump bytecode` の使い方である。
pub const DUMP_USAGE: &str = "\
usage: keleut dump bytecode [--frontend diktor|rust] [--prelude FILE | --no-prelude] [--import-path DIR]
       FILE.kel... | FILE.kir

prints the interpreter's bytecode of every function (the format is for inspection only)
";

/// `keleut dump bytecode …` の入口である。引数の先頭は `bytecode` である。
///
/// 入力の読み方は `keleut run` と同じで、LIR に下ろした後、各関数をバイトコードにして逆アセンブルを印字する。
/// 印字の形は調べるためのもので、互換性を約束しない。
pub fn dump_bytecode(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{DUMP_USAGE}");
        return 0;
    }
    let o = match parse(rest) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{DUMP_USAGE}");
            return exit::USAGE;
        }
    };
    let lp = match load(&o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let rt = match keleut_rt::Rt::new(&lp, o.max_depth) {
        Ok(rt) => rt,
        Err(e) => {
            eprint!("{}", String::from_utf8_lossy(&e.message()));
            return e.exit_code();
        }
    };
    let mut text = String::new();
    for i in 0..lp.funcs.len() {
        match keleut_interp::bytecode::encode(&lp, &rt, keleut_lir::FuncId(i as u32)) {
            Ok(c) => text.push_str(&keleut_interp::bytecode::disasm(&c)),
            Err(e) => {
                eprintln!("keleut: 内部エラー: {e}");
                return 70;
            }
        }
    }
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("keleut: バイトコードを書き出せません: {e}");
            exit::IO
        }
    }
}

/// `keleut dump llvm` の使い方である。
pub const DUMP_LLVM_USAGE: &str = "\
usage: keleut dump llvm [--frontend diktor|rust] [--prelude FILE | --no-prelude] [--import-path DIR]
       FILE.kel... | FILE.kir

prints the LLVM IR that the jit back end makes for every function it can compile
(the format is for inspection only; needs the llvm feature, which is on by default)
";

/// `keleut dump llvm …` の入口である。引数の先頭は `llvm` である。
///
/// 入力の読み方は `keleut run` と同じで、LIR に下ろした後、JIT にできる各関数の LLVM IR の文面を印字する。
pub fn dump_llvm(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{DUMP_LLVM_USAGE}");
        return 0;
    }
    let o = match parse(rest) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{DUMP_LLVM_USAGE}");
            return exit::USAGE;
        }
    };
    dump_llvm_of(&o)
}

#[cfg(feature = "llvm")]
fn dump_llvm_of(o: &Options) -> u8 {
    let lp = match load(o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let text = match keleut_llvm::dump_ir(&lp) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("keleut: 内部エラー: {e}");
            return 70;
        }
    };
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("keleut: LLVM IR を書き出せません: {e}");
            exit::IO
        }
    }
}

#[cfg(not(feature = "llvm"))]
fn dump_llvm_of(_: &Options) -> u8 {
    eprintln!("keleut: 未実装: dump llvm(feature llvm を外して組みました)");
    exit::UNIMPLEMENTED
}

/// `keleut run …` の入口である。引数の先頭は `run` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return 0;
    }
    match parse(rest) {
        Ok(o) => run(&o),
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            exit::USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn コマンド行を解釈する() {
        let o = parse(&argv(&[
            "--backend",
            "interp",
            "--tier",
            "interp-only",
            "--max-depth",
            "10",
            "--native-stack",
            "512k",
            "--no-prelude",
            "a.kel",
            "b.kel",
        ]))
        .unwrap();
        assert_eq!(o.max_depth, 10);
        assert_eq!(o.native_stack, Some(512 << 10));
        assert!(o.yield_prune);
        assert!(
            !parse(&argv(&["--no-yield-prune", "a.kel"]))
                .unwrap()
                .yield_prune
        );
        assert_eq!(o.front_args, argv(&["--no-prelude"]));
        assert_eq!(o.files.len(), 2);
        for a in [
            &[][..],
            &["--backend", "x", "a.kel"],
            &["--tier", "threshold=x", "a.kel"],
            &["--max-depth", "0", "a.kel"],
            &["--native-stack", "1", "a.kel"],
            &["--native-stack", "256k", "a.kel"],
            &["--native-stack", "511k", "a.kel"],
            &["a.kir", "b.kel"],
            &["--no-prelude", "a.kir"],
            &["--bogus", "a.kel"],
        ] {
            assert!(parse(&argv(a)).is_err(), "{a:?}");
        }
        assert_eq!(parse_size("2m"), Some(2 << 20));
        assert_eq!(parse_size("1g"), Some(1 << 30));
        assert_eq!(parse_size("123"), Some(123));
        assert_eq!(parse_size("k"), None);
        let b = |a: &[&str]| parse(&argv(a)).unwrap().backend().to_string();
        assert_eq!(b(&["--tier", "alternating=3", "a.kel"]), "jit");
        assert_eq!(
            b(&["--backend", "interp", "--tier", "threshold=1", "a.kel"]),
            "interp"
        );
        assert_eq!(
            b(&["--backend", "wasm", "--tier", "mixed=1", "a.kel"]),
            "wasm"
        );
        let dflt = if cfg!(feature = "llvm") {
            "jit"
        } else {
            "interp"
        };
        assert_eq!(b(&["a.kel"]), dflt);
        assert_eq!(b(&["--tier", "interp-only", "a.kel"]), dflt);
        // 既定のフロントエンドは rust で、`--frontend diktor` で Diktor を選べる。
        let f = |a: &[&str]| parse(&argv(a)).unwrap().frontend;
        assert_eq!(f(&["a.kel"]), "rust");
        assert_eq!(f(&["--frontend", "diktor", "a.kel"]), "diktor");
        assert_eq!(
            f(&["--frontend", "diktor", "--frontend", "rust", "a.kel"]),
            "rust"
        );
        assert!(parse(&argv(&["--frontend", "ocaml", "a.kel"])).is_err());
    }
}
