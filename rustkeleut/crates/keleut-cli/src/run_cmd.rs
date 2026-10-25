//! サブコマンド `keleut run` である(計画 261024-1 の 6.2 節)。
//!
//! `keleut run [OPTIONS] FILE.kel… | FILE.kir` は、プログラムを実行する。
//!
//! - `.kir`(バイナリ)か `.kirt`(テキスト)の 1 ファイルは、そのまま読む。
//! - `.kel` のファイルは、フロントエンドで KIR にする。
//!   M99 までの既定のフロントエンドは `diktor` で、`$KELEUT_DIKTOR`(無ければ PATH の `diktor`)を
//!   `--emit-ir` で子プロセスとして呼ぶ。
//!   `--prelude`、`--no-prelude`、`--import-path` はそのまま渡す。
//!   Diktor が 0 で終わらなければ、その終了状態で終わる(診断は Diktor が標準エラーに書いたものである)。
//!
//! 読み込んだ KIR を検証し、LIR に下ろして、後段で実行する。
//! 後段はインタプリタ(`--backend interp`)、WasmGC(`--backend wasm`。M94。crate keleut-wasm が Node を起動する)、
//! LLVM の JIT(`--backend jit`。M91。crate keleut-llvm。feature `llvm` で組んだときだけ使え、無ければ未実装の 4)である。
//! `--backend jit` の段の設定は `--tier jit-only`(既定)と `interp-only` で、ほかは M92 まで未実装(4)である。
//! 既定の後段は、段の切り替え(M92)が入るまで `interp` である。
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
  --backend interp|jit|wasm   back end (default: interp; jit needs a build with the llvm feature)
  --tier MODE                 interp-only|jit-only|threshold=N|alternating=SEED|mixed=SEED
                              (jit: jit-only (default) or interp-only; the others are not implemented yet)
  --max-depth N               logical depth limit L (default 1048576)
  --native-stack BYTES        native stack of the mutator thread (suffix k, m or g; at least 512k;
                              interp and jit; ignored by --backend wasm)
  --gc-stress                 collect garbage at the first allocation of every allocation site
                              and then at short jittered intervals (for testing;
                              interp and jit; ignored by --backend wasm)
  --no-yield-prune            treat every resume point as able to receive a yield (pass 11 off)
  --frontend diktor|rust      front end for .kel files (default: diktor via $KELEUT_DIKTOR)
  --prelude FILE, --no-prelude, --import-path DIR   passed to the front end
";

/// 解釈したコマンド行である。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    backend: String,
    tier: Option<String>,
    pub(crate) max_depth: u64,
    native_stack: Option<u64>,
    gc_stress: bool,
    yield_prune: bool,
    frontend: String,
    front_args: Vec<String>,
    files: Vec<PathBuf>,
}

/// 大きさの綴り(`512k`、`64m`、`1g`、バイトの数)を読む。
fn parse_size(s: &str) -> Option<u64> {
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
        backend: "interp".to_string(),
        tier: None,
        max_depth: keleut_rt::DEFAULT_MAX_DEPTH,
        native_stack: None,
        gc_stress: false,
        yield_prune: true,
        frontend: "diktor".to_string(),
        front_args: Vec::new(),
        files: Vec::new(),
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
                o.backend = v;
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
            "--frontend" => {
                let v = value(a)?;
                if !matches!(v.as_str(), "diktor" | "rust") {
                    return Err(format!("bad --frontend value: {v}"));
                }
                o.frontend = v;
            }
            "--no-prelude" => o.front_args.push(a.to_string()),
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
    let kir = o.files.iter().filter(|f| is_kir(f)).count();
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

fn run(o: &Options) -> u8 {
    if o.backend == "wasm" {
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
    if o.backend == "jit" {
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

/// LLVM の後段(M91)で走らせる。
#[cfg(feature = "llvm")]
fn run_jit(o: &Options) -> u8 {
    let tier = match o.tier.as_deref() {
        None | Some("jit-only") => keleut_llvm::Tier::JitOnly,
        Some("interp-only") => keleut_llvm::Tier::InterpOnly,
        Some(t) => {
            eprintln!("keleut: 未実装: 段の設定 {t}");
            return exit::UNIMPLEMENTED;
        }
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
    eprintln!("keleut: 未実装: 後段 jit(feature llvm を付けずに組みました)");
    exit::UNIMPLEMENTED
}

/// 入力を読み(`.kel` ならフロントエンドを通し)、KIR を検証して LIR に下ろす。
/// 失敗すれば、診断を標準エラーに書いて終了状態を返す。
pub(crate) fn load(o: &Options) -> Result<keleut_lir::Program, u8> {
    let data = if is_kir(&o.files[0]) {
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
        eprintln!("keleut: 未実装: フロントエンド rust の KIR の書き出し");
        return Err(exit::UNIMPLEMENTED);
    } else {
        emit_with_diktor(o)?
    };
    let limits = kir::Limits::default();
    let read = match o.files[0].extension().and_then(|e| e.to_str()) {
        Some("kir") => kir::read_kir_with,
        Some("kirt") => kir::read_kirt_with,
        _ => kir::read_any_with,
    };
    let prog = match read(&data, &limits) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("keleut: KIR を読み込めません: {}", e.message());
            return Err(e.exit_code() as u8);
        }
    };
    if let Err(e) = kir::verify_with(&prog, &limits) {
        eprintln!("keleut: KIR の検証に失敗しました: {}", e.message());
        return Err(e.exit_code() as u8);
    }
    drop(data);
    keleut_lir::lower_opts(&prog, kir::N_NEST, o.yield_prune).map_err(|e| {
        eprintln!("keleut: {e}");
        e.exit_code() as u8
    })
}

/// `keleut dump bytecode` の使い方である。
pub const DUMP_USAGE: &str = "\
usage: keleut dump bytecode [--prelude FILE | --no-prelude] [--import-path DIR] FILE.kel... | FILE.kir

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
usage: keleut dump llvm [--prelude FILE | --no-prelude] [--import-path DIR] FILE.kel... | FILE.kir

prints the LLVM IR that the jit back end makes for every function it can compile
(the format is for inspection only; needs a build with the llvm feature)
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
    eprintln!("keleut: 未実装: dump llvm(feature llvm を付けずに組みました)");
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
    }
}
