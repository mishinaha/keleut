//! サブコマンド `keleut run` を実装する。
//!
//! `keleut run [OPTIONS] FILE.kel… | FILE.kir` は、プログラムを実行する。
//!
//! - `.kir`(バイナリ)か `.kirt`(テキスト)の 1 ファイルは、そのまま読む。
//! - `.kel` のファイルは、フロントエンドで KIR に変換する。
//!   フロントエンドは `--frontend diktor|rust` で選ぶ。
//!   既定は `rust` である。
//!   - `--frontend diktor` は、`$KELEUT_DIKTOR`(なければ PATH の `diktor`)を `--emit-ir` 付きで子プロセスとして実行する。
//!     Diktor の終了コードが 0 でなければ、その終了コードで終了する(診断は Diktor が標準エラーに書いている)。
//!   - `--frontend rust` は、keleut-front を使い、同じプロセスの中で型検査して KIR を作る
//!     ([`crate::emit_kir_cmd::program_for_run`])。
//!     診断と終了コードは `diktor --emit-ir` と同じである。
//!     `--native-stack` は、インタプリタと JIT のスタックサイズに加えて、フロントエンドのスレッドのスタックサイズも決める。
//!     フロントエンドは、構文解析の入れ子の上限を、そのスタックに収まる深さまで下げる(`keleut check --native-stack` と同じ)。
//!     構文解析の上限の最小値(64 段)に必要なスタックより小さい値を指定したときは、その大きさまで引き上げて使う
//!     ([`crate::emit_kir_cmd::front_stack_for_run`])。
//!     `--native-stack` を指定しなければ、フロントエンドは既定の 8 GiB のスタックで動く。
//!     仮想メモリを制限しているとき(`ulimit -v`)は、スタックをその上限に合わせて小さくし、入れ子の上限を下げる。
//!
//!   `--prelude`、`--no-prelude`、`--import-path`、`--strict-exhaustive` は、どちらのフロントエンドにもそのまま渡す。
//!
//! 読み込んだ KIR を検証し、LIR に変換(lowering)して、バックエンドで実行する。
//! `--native-stack` を指定したときは、KIR の読み込みと検証に使うスタックもフロントエンドと同じ大きさにし、
//! 入れ子の上限をそれに収まる深さまで下げる([`kir_limits`])。
//! バックエンドは次の 3 つである。
//!
//! - インタプリタ(`--backend interp`)。
//! - WasmGC(`--backend wasm`)。
//!   crate keleut-wasm が Node を起動する。
//! - LLVM の JIT(`--backend jit`)。
//!   crate keleut-llvm が実装する。
//!
//! JIT は、feature `llvm`(既定で有効)を付けてビルドしたときだけ使える。
//! `--no-default-features` で外してビルドした `keleut` では、`--backend jit` は未実装として終了コード 4 で終了する。
//! 既定のバックエンドは、feature `llvm` を付けてビルドしたときは `jit`、外してビルドしたときは `interp` である。
//! `--backend jit` のティアの設定(`--tier`)は `interp-only`、`jit-only`、`threshold=N`、`alternating=SEED`、`mixed=SEED` のどれかで、
//! 既定は `threshold=1000` である。
//! `--backend` を省いて `interp-only` 以外の `--tier` を指定したときは、`jit` で実行する。
//! `--backend interp` と `--backend wasm` は、`--tier` を受け付けて無視する。
//! kconform のペア `kt` などのオプションを追加しても、`--backend interp` と書いたコマンドはインタプリタで実行される。
//!
//! 内部用のオプション `--source-only`(使い方の説明には載せない)を指定すると、名前が `.kir` や `.kirt` で終わる入力も
//! Keleut のソースとしてフロントエンドに渡す。
//! `keleut compat-diktor` が、すべての入力をソースとして読む Diktor と同じ動作にするために使う。
//!
//! `keleut dump bytecode` も同じ方法で入力を読んで LIR に変換し、インタプリタのバイトコードを出力する([`dump_bytecode`])。
//!
//! 終了コードは次のとおりである。
//!
//! - プログラムの結果:0、3、4、74
//! - KIR の読み込みと検証の失敗:65
//! - LIR への変換の失敗と内部エラー:70
//! - 使い方のエラーと、入力を読めないとき:64
//!
//! ただし `.kel` の入力では、次の 2 つの場合の終了コードは 1 である(LangSpec §15.5)。
//! 1 つは、KIR の読み込み、検証、LIR への変換が処理系の限度で止まった場合である
//! (スタックに合わせて下げた入れ子の上限を超えたときと、処理用のスレッドを作れないとき)。
//! もう 1 つは、変換スレッドがメモリの確保に失敗した場合である(`keleut_rt::alloc`)。
//! `.kir` の入力では、どちらも 65 である(IRSpec 13.9 節)。

use crate::exit;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `keleut run` の使い方の説明(usage)。
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

/// `keleut run` のコマンド行を解釈した結果。
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
    /// 真なら、名前によらず、すべての入力をソースとして読む(内部用のオプション `--source-only`)。
    source_only: bool,
}

/// サイズの表記(`512k`、`64m`、`1g`、または接尾辞のないバイト数)を読んで、バイト数を返す。
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

/// KIR の一時ファイル。
/// ドロップしたときに削除する。
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

/// `.kel` を Diktor で KIR に変換する。
/// 成功すれば KIR のバイト列を、失敗すれば終了コードを返す。
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
    /// `p` を KIR のファイルとして読むなら真を返す。
    /// `--source-only` のときは、どのファイルも KIR として読まない。
    fn kir_input(&self, p: &Path) -> bool {
        !self.source_only && is_kir(p)
    }

    /// バックエンドの名前を返す。
    /// `--backend` を省いたときは、`--tier` とビルドの feature から決まる既定の名前を返す。
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
        // WasmGC バックエンド。Node を子プロセスとして起動する(crate keleut-wasm の `launch`)。
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
        fit_stack: o.native_stack.is_none(),
    }
}

/// LLVM バックエンド(JIT)でプログラムを実行する。
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

/// 入力を読み(`.kel` ならフロントエンドを通し)、KIR を検証して LIR に変換する。
/// 失敗すれば、診断を標準エラーに書いて終了コードを返す。
///
/// 初めに、入力の種類(`.kel` か KIR か)を設定する。
/// 変換スレッドがメモリの確保に失敗して `処理系の限度` の行を書いてプロセスを終了するときに、この種類で終了コードが決まる(`keleut_rt::alloc`)。
pub(crate) fn load(o: &Options) -> Result<keleut_lir::Program, u8> {
    let direct = o.kir_input(&o.files[0]);
    keleut_rt::alloc::set_input(if direct {
        keleut_rt::alloc::Input::Kir
    } else {
        keleut_rt::alloc::Input::Kel
    });
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
    let prog = {
        let _site = keleut_rt::alloc::conv_site(keleut_rt::alloc::Site::Kir);
        read(&data, &limits)
    };
    keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::KirRead);
    let prog = match prog {
        Ok(p) => p,
        Err(e) => {
            eprintln!("keleut: KIR を読み込めません: {}", e.message());
            return Err(kir_exit(
                o,
                e.exit_code(),
                e.kind() == kir::ErrorKind::Limit,
            ));
        }
    };
    drop(data);
    verify_and_lower(o, &prog, &limits)
}

/// KIR の読み込み、検証、LIR への変換に使う上限を返す。
///
/// `--native-stack` を指定したときは、フロントエンドと同じ大きさ([`crate::emit_kir_cmd::front_stack_for_run`])の
/// スタックで読み込みと検証を実行し、入れ子の上限をそのスタックに収まる深さまで下げる。
/// LIR への変換の入れ子の上限も同じ値にする(変換に使うスタックは、その上限から決まる)。
/// 指定しなければ、入れ子の上限は `N_nest` で、スタックはそれに足りる大きさである。
/// 仮想メモリを制限しているとき(`ulimit -v`)は、どちらの場合も、スタックを仮想メモリの上限の 4 分の 1 以下にし、収まる深さまで入れ子の上限を下げる
/// ([`kir::Limits::fit_address_space`])。
fn kir_limits(o: &Options) -> kir::Limits {
    match o.native_stack {
        Some(s) => {
            kir::Limits::with_stack(kir::N_NEST, crate::emit_kir_cmd::front_stack_for_run(s))
        }
        None => kir::Limits::default(),
    }
    .fit_address_space()
}

/// KIR の読み込み、検証、LIR への変換のエラーに対する終了コードを返す。
///
/// 処理系の限度(下げた入れ子の上限を超えたときと、処理用のスレッドを作れないとき)は、`.kir` の入力では 65 を返す(IRSpec 13.9 節)。
/// `.kel` の入力では 1 を返す(LangSpec §15.5)。
/// この場合の KIR はフロントエンド(同じプロセスの keleut-front か Diktor)が書いたものであり、
/// それを実行の前に変換する段階で上限を超えるのは、KIR の不正ではなく処理系の限度だからである。
/// それ以外のエラーでは `code` をそのまま返す。
fn kir_exit(o: &Options, code: i32, limit: bool) -> u8 {
    if limit && !o.kir_input(&o.files[0]) {
        1
    } else {
        code as u8
    }
}

/// KIR を検証して LIR に変換する。
/// 失敗すれば、診断を標準エラーに書いて終了コードを返す。
///
/// 検証の後と変換の後に、変換スレッドがメモリの確保に失敗したことを表すフラグを調べ、
/// 立っていれば `処理系の限度` の行を書いてプロセスを終了する(`keleut_rt::alloc::conv_checkpoint`)。
/// LIR への変換のスレッドを作れなければ、スタックを半分ずつ小さくし、それに収まる深さの上限で検証からやり直す
/// ([`verify_then_lower`])。
fn verify_and_lower(
    o: &Options,
    prog: &kir::Program,
    limits: &kir::Limits,
) -> Result<keleut_lir::Program, u8> {
    let _site = keleut_rt::alloc::conv_site(keleut_rt::alloc::Site::Kir);
    let r = verify_then_lower(prog, limits, |n| {
        keleut_lir::lower_opts(prog, n, o.yield_prune)
    });
    match r {
        Ok(lp) => Ok(lp),
        Err(VerifyLowerError::Verify(e)) => {
            eprintln!("keleut: KIR の検証に失敗しました: {}", e.message());
            Err(kir_exit(
                o,
                e.exit_code(),
                e.kind() == kir::ErrorKind::Limit,
            ))
        }
        Err(VerifyLowerError::Lower(e)) => {
            eprintln!("keleut: {e}");
            Err(kir_exit(
                o,
                e.exit_code(),
                e.kind() == keleut_lir::ErrorKind::Limit,
            ))
        }
    }
}

/// LIR への変換のスレッドを作り直すときは、KIR の処理に使うスタックサイズ([`kir::Limits`] の設定)を半分ずつ小さくしていく。
/// この定数は、その下限(1 MiB)である。
/// 仮想メモリを制限しているとき(`ulimit -v`)に使う。
///
/// LIR への変換に使うスタックは、検証した深さの上限から [`keleut_lir::stack_for`] で決まり、固定の 16 MiB を含む。
/// スタックを小さくするのを、KIR を処理するスレッドのスタックサイズの下限 [`kir::MIN_THREAD_STACK`] でやめると、LIR への変換に使うスタックは約 28 MiB(リリースビルド)より小さくならない。
/// KIR の処理に使うスタックサイズを 1 MiB まで下げれば、LIR への変換に使うスタックは約 17 MiB まで下がる。
const MIN_LOWER_RETRY_STACK: usize = 1 << 20;

/// [`verify_then_lower`] が返すエラー。
pub(crate) enum VerifyLowerError {
    /// KIR の検証のエラー。
    /// KIR を処理するスレッドを作れないとき(処理系の限度)を含む。
    Verify(kir::Error),
    /// LIR への変換のエラー。
    /// 変換のスレッドを作れないとき(処理系の限度)を含む。
    Lower(keleut_lir::Error),
}

/// KIR を上限 `limits` で検証し、検証した深さの上限を `lower` に渡して LIR に変換する。
///
/// 仮想メモリを制限しているとき(`ulimit -v`)に LIR への変換のスレッドを作れなければ(`keleut_lir::ErrorKind::Limit`)、
/// KIR の処理に使うスタックサイズを半分にし(下限は [`MIN_LOWER_RETRY_STACK`])、それに収まる深さで検証し直してから、もう一度変換する。
/// 検証からやり直すのは、LIR への変換には、検証した深さに必要なスタックを使うからである。
/// 検証の後と変換の後に、変換スレッドがメモリの確保に失敗したことを表すフラグを調べる(`keleut_rt::alloc::conv_checkpoint`)。
pub(crate) fn verify_then_lower<T>(
    prog: &kir::Program,
    limits: &kir::Limits,
    lower: impl Fn(usize) -> Result<T, keleut_lir::Error>,
) -> Result<T, VerifyLowerError> {
    let mut limits = *limits;
    loop {
        let v = kir::verify_with(prog, &limits);
        keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::KirVerify);
        if let Err(e) = v {
            return Err(VerifyLowerError::Verify(e));
        }
        let lp = lower(limits.max_nest);
        keleut_rt::alloc::conv_checkpoint(keleut_rt::alloc::Stage::Lower);
        match lp {
            Err(e)
                if e.kind() == keleut_lir::ErrorKind::Limit
                    && limits.stack_bytes > MIN_LOWER_RETRY_STACK =>
            {
                let next = (limits.stack_bytes / 2).max(MIN_LOWER_RETRY_STACK);
                limits = kir::Limits::with_stack(limits.max_nest, next);
            }
            Err(e) => return Err(VerifyLowerError::Lower(e)),
            Ok(lp) => return Ok(lp),
        }
    }
}

/// `keleut dump bytecode` の使い方の説明(usage)。
pub const DUMP_USAGE: &str = "\
usage: keleut dump bytecode [--frontend diktor|rust] [--prelude FILE | --no-prelude] [--import-path DIR]
       FILE.kel... | FILE.kir

prints the interpreter's bytecode of every function (the format is for inspection only)
";

/// `keleut dump bytecode …` のエントリポイントである。
/// 引数の先頭は `bytecode` である。
///
/// 入力は `keleut run` と同じ方法で読み、LIR に変換した後、各関数をバイトコードにして、逆アセンブルした結果を出力する。
/// 出力の形式は調査用で、互換性を保証しない。
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

/// `keleut dump llvm` の使い方の説明(usage)。
pub const DUMP_LLVM_USAGE: &str = "\
usage: keleut dump llvm [--frontend diktor|rust] [--prelude FILE | --no-prelude] [--import-path DIR]
       FILE.kel... | FILE.kir

prints the LLVM IR that the jit back end makes for every function it can compile
(the format is for inspection only; needs the llvm feature, which is on by default)
";

/// `keleut dump llvm …` のエントリポイントである。
/// 引数の先頭は `llvm` である。
///
/// 入力は `keleut run` と同じ方法で読み、LIR に変換した後、JIT コンパイルできる各関数の LLVM IR のテキストを出力する。
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

/// `keleut run …` のエントリポイントである。
/// 引数の先頭は `run` である。
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
