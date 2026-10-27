//! PATH の先頭に置く `diktor` と `keleut` の shim である(計画 261024-1 の 8.1 節)。
//!
//! shim は小さな `sh` のスクリプトで、呼び出しを記録し、組 `dk`(と組の無いとき)の `diktor` は本物へ直に `exec` する。
//! そのほかは `kconform __shim diktor|keleut ARGS…` に `exec` し、本モジュールが振り分ける。
//!
//! shim を Rust の実行形そのものにしないのは、Rust の実行時が起動のときに、閉じた標準入出力(0、1、2)を `/dev/null` で開き直すからである。
//! `diktor x.kel >&-` のような試験(閉じた標準出力で 74)が、shim を挟むと 0 になってしまう。
//! そこでスクリプトが、閉じていた記述子の番号を `KCONFORM_CLOSED_FDS` で渡し、本モジュールが `exec` の前に閉じ直す。
//!
//! 行き先は環境変数で受け取る。
//!
//! | 変数 | 中身 |
//! |---|---|
//! | `KCONFORM_REAL_DIKTOR` | 本物の diktor の実行形 |
//! | `KCONFORM_REAL_KELEUT` | 本物の keleut の実行形 |
//! | `KCONFORM_PAIR` | 組の名前(`dk`、`dk-ir`、`ki` など) |
//! | `KCONFORM_PAIR_ARGS` | 組の `keleut run` の選択肢(空白区切り。`--extra` の旗を含む) |
//! | `KCONFORM_LOG` | 呼び出しを 1 行ずつ足す記録のファイル |
//! | `KCONFORM_CMD` | いま走っている cram のコマンドの番号 |
//! | `KCONFORM_CLOSED_FDS` | shim が呼ばれたときに閉じていた標準入出力の番号(空白区切り) |
//!
//! 行き先へは `exec` で移る。
//! 標準入力、開いているファイル記述子、シグナルによる終わり方が、本物を直に呼んだときと同じになる。

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// diktor の呼び出しの振り分け先(計画 8.1 節の shim の表の行)である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// 実行(旗なし、`--prelude`、`--no-prelude`、`--import-path`)。
    Run,
    /// `--type-check`、`--strict-exhaustive`。
    TypeCheck,
    /// `--dump-tokens`、`--dump-ast`。
    Dump,
    /// `--repl`。
    Repl,
    /// Diktor 自身の IR の経路を試す呼び出し。
    /// 組に依らず、いつも本物の diktor に渡す。
    DiktorIr,
}

/// Diktor の IR の旗である。
const IR_FLAGS: &[&str] = &[
    "--emit-ir",
    "--emit-ir-text",
    "--run-ir",
    "--via-ir",
    "--max-depth",
    "--dump-ir-tables",
];

/// 引数を 1 つ取る Diktor の旗である。
/// その引数を旗と取り違えないために使う。
const TAKES_ARG: &[&str] = &[
    "--prelude",
    "--import-path",
    "--emit-ir",
    "--emit-ir-text",
    "--run-ir",
    "--max-depth",
];

/// diktor の呼び出しを振り分ける。
///
/// 2 つ以上の行に当たる呼び出しは、Diktor の IR の旗の行を先に当てる。
/// `via_ir` は、環境変数 `DIKTOR_VIA_IR` が立っているかである。
pub fn classify(args: &[OsString], via_ir: bool) -> Route {
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        // 旗は ASCII なので、UTF-8 でない引数は旗でない。
        let a = args[i].to_str().unwrap_or("");
        if a.starts_with("--") {
            flags.push(a);
            if TAKES_ARG.contains(&a) {
                i += 1;
            }
        }
        i += 1;
    }
    let has = |names: &[&str]| flags.iter().any(|f| names.contains(f));
    if via_ir || has(IR_FLAGS) {
        Route::DiktorIr
    } else if has(&["--repl"]) {
        Route::Repl
    } else if has(&["--dump-tokens", "--dump-ast"]) {
        Route::Dump
    } else if has(&["--type-check", "--strict-exhaustive"]) {
        Route::TypeCheck
    } else {
        Route::Run
    }
}

/// 環境変数 `DIKTOR_VIA_IR` が立っているかを返す。
///
/// kconform は試験の環境からこの変数を消すので、立っているのは試験のコマンドが自分で立てたときだけである。
/// `DIKTOR_VIA_IR=0` も、木の評価器を名指しする呼び出しなので、値に依らず Diktor の IR の行に当てる。
pub fn via_ir_env() -> bool {
    std::env::var_os("DIKTOR_VIA_IR").is_some()
}

fn env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

/// shim のスクリプトを作る。
///
/// `who` は shim の名前(`diktor` か `keleut`)、`kconform` は kconform の実行形の絶対パスである。
/// 記録の行は「コマンドの番号、タブ、shim の名前、タブ、引数を 0x1f で区切った列」で、1 回の `printf` で足す。
/// 1 回の write で書くので、パイプの両側で並行に呼ばれた shim の行は混ざらない。
pub fn script(who: &str, kconform: &str) -> String {
    let direct = if who == "diktor" {
        "case \"${KCONFORM_PAIR-}\" in\n  dk|'') exec \"$KCONFORM_REAL_DIKTOR\" \"$@\" ;;\nesac\n"
    } else {
        ""
    };
    format!(
        "#!/bin/sh\n\
         # kconform の shim({who})。kconform が走るたびに作り、終わったら消す。\n\
         if [ -n \"${{KCONFORM_LOG-}}\" ]; then\n\
         \x20 kc_us=$(printf '\\037'); kc_l=\"${{KCONFORM_CMD-}}\t{who}\t\"; kc_s=\n\
         \x20 for kc_a in \"$@\"; do kc_l=\"$kc_l$kc_s$kc_a\"; kc_s=$kc_us; done\n\
         \x20 printf '%s\\n' \"$kc_l\" >> \"$KCONFORM_LOG\"\n\
         fi\n\
         {direct}\
         kc_c=\n\
         for kc_n in 0 1 2; do [ -L /proc/$$/fd/$kc_n ] || kc_c=\"$kc_c $kc_n\"; done\n\
         KCONFORM_CLOSED_FDS=$kc_c; export KCONFORM_CLOSED_FDS\n\
         exec {} __shim {who} \"$@\"\n",
        crate::util::sh_quote(kconform)
    )
}

/// shim のスクリプトが呼ばれたときに閉じていた標準入出力を、閉じ直す。
fn restore_closed_fds() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let closed = std::env::var("KCONFORM_CLOSED_FDS").unwrap_or_default();
    for n in closed
        .split_whitespace()
        .filter_map(|n| n.parse::<i32>().ok())
    {
        if (0..=2).contains(&n) {
            // SAFETY: 0〜2 は Rust の実行時が起動のときに `/dev/null` で開いたもので、
            // 本プロセスはこれらの記述子を所有する値をほかに持たない(標準入出力は使う都度に借りる)。
            drop(unsafe { OwnedFd::from_raw_fd(n) });
        }
    }
}

/// `exec` に失敗したときの終わり方である(シェルがコマンドを見つけられないときと同じ 127)。
fn exec_failed(prog: &OsString, e: std::io::Error) -> u8 {
    eprintln!(
        "kconform shim: {} を起動できません: {e}",
        String::from_utf8_lossy(prog.as_bytes())
    );
    127
}

/// 本物の実行形へ `exec` で移る。
/// 戻るのは失敗したときだけである。
fn exec(prog: &OsString, arg0: &str, args: &[OsString]) -> u8 {
    let e = Command::new(prog).arg0(arg0).args(args).exec();
    exec_failed(prog, e)
}

fn required(name: &str) -> Result<OsString, u8> {
    env(name).ok_or_else(|| {
        eprintln!("kconform shim: 環境変数 {name} がありません");
        127
    })
}

fn pair_args() -> Vec<OsString> {
    std::env::var("KCONFORM_PAIR_ARGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(OsString::from)
        .collect()
}

/// 一時ファイルの名前を作る(呼び出しの作業ディレクトリの `TMPDIR` の中)。
fn temp_kir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("kconform-shim-{}-{nanos}.kir", std::process::id()))
}

/// Diktor の実行の呼び出しを、`--emit-ir` の後の `--run-ir` にする(組 `dk-ir`)。
///
/// `run_ir_args` は `--run-ir` の呼び出しに足す旗(`--max-depth N` など)である。
/// 書き出しが 0 で終わらなければ、その出力と終了状態をそのまま返す。
fn diktor_via_emit(real: &OsString, args: &[OsString], run_ir_args: &[OsString]) -> u8 {
    let kir = temp_kir().into_os_string();
    let mut emit = vec![OsString::from("--emit-ir"), kir.clone()];
    emit.extend_from_slice(args);
    let st = match Command::new(real).arg0("diktor").args(&emit).status() {
        Ok(st) => st,
        Err(e) => return exec_failed(real, e),
    };
    if !st.success() {
        return st.code().map_or(70, |c| c as u8);
    }
    let mut run = vec![OsString::from("--run-ir"), kir];
    run.extend_from_slice(run_ir_args);
    exec(real, "diktor", &run)
}

/// Diktor の実行の呼び出しの旗とファイルを、`keleut emit-kir` の引数に写す(組 `x-ir`)。
///
/// `--no-prelude`、`--prelude`、`--import-path` は同じ綴りで渡す。
/// ほかの旗(使い方の誤りになるものを含む)と、UTF-8 でない引数を含む呼び出しは `None` を返す(本物の diktor に渡す)。
fn emit_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::new();
    let mut files = 0;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_str()?;
        match a {
            "--no-prelude" => out.push(OsString::from(a)),
            "--prelude" | "--import-path" => {
                let v = args.get(i + 1)?.to_str()?;
                out.push(OsString::from(a));
                out.push(OsString::from(v));
                i += 1;
            }
            _ if a.starts_with('-') => return None,
            _ => {
                files += 1;
                out.push(OsString::from(a));
            }
        }
        i += 1;
    }
    (files > 0).then_some(out)
}

/// `keleut emit-kir` を走らせる。`quiet` なら標準エラーを捨てずに預かり、失敗したときだけ書く。
fn keleut_emit(
    real_keleut: &OsString,
    text: bool,
    out: &OsString,
    eargs: &[OsString],
    quiet: bool,
) -> Result<u8, u8> {
    let mut cmd = Command::new(real_keleut);
    cmd.arg0("keleut").arg("emit-kir");
    if text {
        cmd.arg("--text");
    }
    cmd.arg("-o").arg(out).args(eargs);
    log_route("x-ir-emit", if text { "text" } else { "binary" });
    if quiet {
        match cmd.stderr(std::process::Stdio::piped()).output() {
            Ok(o) if o.status.success() => Ok(0),
            Ok(o) => {
                use std::io::Write;
                let _ = std::io::stderr().write_all(&o.stderr);
                Err(o.status.code().map_or(70, |c| c as u8))
            }
            Err(e) => Err(exec_failed(real_keleut, e)),
        }
    } else {
        match cmd.status() {
            Ok(st) if st.success() => Ok(0),
            Ok(st) => Err(st.code().map_or(70, |c| c as u8)),
            Err(e) => Err(exec_failed(real_keleut, e)),
        }
    }
}

/// Diktor の実行の呼び出しを、`keleut emit-kir` の後の `diktor --run-ir` にする(組 `x-ir`)。
///
/// 書き出しが 0 で終わらなければ、その出力と終了状態をそのまま返す。
fn keleut_via_emit(real: &OsString, args: &[OsString], run_ir_args: &[OsString]) -> u8 {
    let Some(eargs) = emit_args(args) else {
        return exec(real, "diktor", args);
    };
    let keleut = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    let kir = temp_kir().into_os_string();
    if let Err(c) = keleut_emit(&keleut, false, &kir, &eargs, false) {
        return c;
    }
    let mut run = vec![OsString::from("--run-ir"), kir];
    run.extend_from_slice(run_ir_args);
    exec(real, "diktor", &run)
}

/// 組 `x-ir` で、KIR を書き出すだけの呼び出し(`--emit-ir`、`--emit-ir-text` と、プレリュードと import の旗)を
/// `keleut emit-kir` に振り向ける。
///
/// 両方の旗があれば、Diktor と同じくテキストを先に書き、警告は 1 回だけ出す。
/// ほかの IR の旗(`--run-ir`、`--via-ir`、`--max-depth`、`--dump-ir-tables`)やモードの旗を含む呼び出しと、
/// 環境変数 `DIKTOR_VIA_IR` を立てた呼び出しは、本物の diktor に渡す。
fn x_ir_emit(real: &OsString, args: &[OsString]) -> u8 {
    if via_ir_env() {
        return exec(real, "diktor", args);
    }
    let mut bin = None;
    let mut text = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].to_str() {
            Some("--emit-ir") if i + 1 < args.len() => {
                bin = Some(args[i + 1].clone());
                i += 1;
            }
            Some("--emit-ir-text") if i + 1 < args.len() => {
                text = Some(args[i + 1].clone());
                i += 1;
            }
            Some(a @ ("--prelude" | "--import-path")) if i + 1 < args.len() => {
                rest.push(OsString::from(a));
                rest.push(args[i + 1].clone());
                i += 1;
            }
            _ => rest.push(args[i].clone()),
        }
        i += 1;
    }
    let Some(eargs) = emit_args(&rest) else {
        return exec(real, "diktor", args);
    };
    let keleut = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    if let Some(t) = &text
        && let Err(c) = keleut_emit(&keleut, true, t, &eargs, bin.is_some())
    {
        return c;
    }
    if let Some(b) = &bin
        && let Err(c) = keleut_emit(&keleut, false, b, &eargs, false)
    {
        return c;
    }
    0
}

/// `diktor` の shim の本体である。
pub fn diktor(args: &[OsString]) -> u8 {
    restore_closed_fds();
    let real = match required("KCONFORM_REAL_DIKTOR") {
        Ok(r) => r,
        Err(c) => return c,
    };
    let pair = std::env::var("KCONFORM_PAIR").unwrap_or_default();
    let route = classify(args, via_ir_env());
    match (pair.as_str(), route) {
        ("dk-ir", Route::Run) => diktor_via_emit(&real, args, &[]),
        // `--strict-exhaustive` だけの呼び出しは表では型検査の行にあるが、Diktor はプログラムを実行するので、
        // 組 `ki` と同じく実行として扱う。
        ("dk-ir", Route::TypeCheck) if strict_exhaustive_run(args) => {
            diktor_via_emit(&real, args, &[])
        }
        ("x-ir", Route::Run) => keleut_via_emit(&real, args, &[]),
        ("x-ir", Route::DiktorIr) => x_ir_emit(&real, args),
        ("x-ir", _) => exec(&real, "diktor", args),
        (p, Route::Repl) if repl_pair(p) => repl_to_keleut(&real, args),
        ("dk" | "dk-ir" | "", _) | (_, Route::Repl | Route::DiktorIr) => {
            exec(&real, "diktor", args)
        }
        (_, Route::Dump) => dump_to_keleut(&real, args),
        ("kf", Route::Run | Route::TypeCheck) => kf_check(&real, args),
        (p, Route::Run | Route::TypeCheck) if is_keleut_pair(p) => keleut_backend(&real, args),
        (p, Route::Run | Route::TypeCheck) if is_rust_pair(p) => rust_backend(&real, args),
        (_, Route::TypeCheck) => exec(&real, "diktor", args),
        (p, Route::Run) => {
            eprintln!("kconform shim: 組 {p} の diktor の実行の振り分けはありません");
            4
        }
    }
}

/// 型検査の行に当たる呼び出しが、実際には実行(`--strict-exhaustive` があり `--type-check` が効かない)かどうかを返す。
fn strict_exhaustive_run(args: &[OsString]) -> bool {
    matches!(diktor_mode(args), Some((Mode::Run, f, _)) if !f.is_empty())
}

/// 組が RustKeleut の後段(`keleut run`)で実行する組(`ki kj kt ka km kw`)かどうかを返す。
fn is_keleut_pair(name: &str) -> bool {
    crate::pairs::expand(name)
        .is_some_and(|ps| ps.iter().all(|p| p.engine == crate::pairs::Engine::Keleut))
}

/// 組が Rust のフロントエンドと RustKeleut の後段で実行する組(`kr-*`)かどうかを返す。
fn is_rust_pair(name: &str) -> bool {
    crate::pairs::expand(name).is_some_and(|ps| {
        ps.iter()
            .all(|p| p.engine == crate::pairs::Engine::RustKeleut)
    })
}

/// 組が RustKeleut の後段の組(`ki kj kt ka km kw` と `kr-*`)かどうかを返す。
///
/// これらの組では、`--repl` の呼び出しを `keleut repl` に振り向ける(計画 8.1 節の shim の表。M100)。
/// `keleut repl` は組の後段に関係なくインタプリタで実行し、組の選択肢(`--backend`、`--tier`、`--frontend`)は受けて捨てる。
/// どの後段の組でも REPL の出力が同じになるので、後段の組の間の完全な一致の比べ(D499)が成り立つ。
fn repl_pair(name: &str) -> bool {
    crate::pairs::expand(name).is_some_and(|ps| {
        ps.iter().all(|p| {
            matches!(
                p.engine,
                crate::pairs::Engine::Keleut | crate::pairs::Engine::RustKeleut
            )
        })
    })
}

/// Diktor の `--repl` の呼び出しを、`keleut repl 組の選択肢 旗` にする。
///
/// `--no-prelude` と `--prelude` を渡す。
/// `--import-path` と `--strict-exhaustive` は Diktor の対話的な実行に効かないので渡さない。
/// 入力のファイルを渡した呼び出し(使い方の誤り)と、モードが `--repl` でない呼び出しは、本物の diktor に渡す。
fn repl_to_keleut(real: &OsString, args: &[OsString]) -> u8 {
    let mut mode = Mode::Run;
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let Some(a) = args[i].to_str() else {
            return exec(real, "diktor", args);
        };
        match a {
            "--type-check" => mode = Mode::TypeCheck,
            "--repl" => mode = Mode::Repl,
            "--dump-tokens" => mode = Mode::DumpTokens,
            "--dump-ast" => mode = Mode::DumpAst,
            "--no-prelude" => flags.push(OsString::from(a)),
            "--strict-exhaustive" => {}
            "--prelude" | "--import-path" => {
                let Some(v) = args.get(i + 1) else {
                    return exec(real, "diktor", args);
                };
                if a == "--prelude" {
                    flags.push(OsString::from(a));
                    flags.push(v.clone());
                }
                i += 1;
            }
            _ => return exec(real, "diktor", args),
        }
        i += 1;
    }
    if mode != Mode::Repl {
        return exec(real, "diktor", args);
    }
    let keleut = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    log_route("keleut-repl", "keleut");
    let mut v = vec![OsString::from("repl")];
    v.extend(pair_args());
    v.extend(flags);
    exec(&keleut, "keleut", &v)
}

/// 組 `kr-*` で、Diktor の実行と型検査の呼び出しを振り分ける。
///
/// - 実行は `keleut run 組の選択肢 旗 FILE…` にする(組の選択肢は `--frontend rust` を含む)。
///   旗は `--no-prelude`、`--prelude`、`--import-path`、`--strict-exhaustive` である。
/// - 型検査(`--type-check`)は、組 `kf` と同じく `keleut check` に渡す。
/// - 使い方の誤りになる呼び出しは、本物の diktor に渡す。
fn rust_backend(real: &OsString, args: &[OsString]) -> u8 {
    match kf_check_args(args) {
        Some((Mode::Run, kargs)) => {
            let keleut = match required("KCONFORM_REAL_KELEUT") {
                Ok(r) => r,
                Err(c) => return c,
            };
            log_route("keleut-run", "keleut-rust");
            let mut v = vec![OsString::from("run")];
            v.extend(pair_args());
            v.extend(kargs);
            exec(&keleut, "keleut", &v)
        }
        Some((Mode::TypeCheck, _)) => kf_check(real, args),
        _ => exec(real, "diktor", args),
    }
}

/// RustKeleut の後段の組で、Diktor の実行と型検査の呼び出しを振り分ける(計画 8.1 節の shim の表)。
///
/// - 実行(旗なし、`--prelude`、`--no-prelude`、`--import-path`、`--strict-exhaustive`)は、
///   本物の `diktor --emit-ir` が 0 で終われば `keleut run 組の選択肢 FILE.kir` に移り、
///   0 でなければその出力と終了状態をそのまま返す。
///   網羅漏れの警告で 1 で終わるかどうかは、同じ旗(`--strict-exhaustive`)を渡した `diktor --emit-ir` が決める
///   (0 でなければ `keleut run` に進まず、その出力と終了状態を返す)。
/// - 型検査(`--type-check`)は、組 `kf` と同じく `keleut check` に渡す(M98 で切り替えた行)。
/// - 使い方の誤りになる呼び出しは、本物の diktor に渡す。
fn keleut_backend(real: &OsString, args: &[OsString]) -> u8 {
    match diktor_mode(args) {
        Some((Mode::Run, files, _)) if !files.is_empty() => diktor_to_keleut_run(real, args),
        Some((Mode::TypeCheck, files, _)) if !files.is_empty() => kf_check(real, args),
        _ => exec(real, "diktor", args),
    }
}

/// Diktor の実行の呼び出しを、本物の `diktor --emit-ir` の後の `keleut run 組の選択肢 FILE.kir` にする。
///
/// 書き出しの警告(網羅漏れなど)は `diktor --emit-ir` が標準エラーに出し、`keleut run` は出さない。
/// 木の評価器と同じく、警告は実行の出力より前に出る。
fn diktor_to_keleut_run(real: &OsString, args: &[OsString]) -> u8 {
    let keleut = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    let kir = temp_kir().into_os_string();
    let mut emit = vec![OsString::from("--emit-ir"), kir.clone()];
    emit.extend_from_slice(args);
    let st = match Command::new(real).arg0("diktor").args(&emit).status() {
        Ok(st) => st,
        Err(e) => return exec_failed(real, e),
    };
    if !st.success() {
        log_route("keleut-run", "diktor-emit");
        return st.code().map_or(70, |c| c as u8);
    }
    log_route("keleut-run", "keleut");
    let mut run = vec![OsString::from("run")];
    run.extend(pair_args());
    run.push(kir);
    exec(&keleut, "keleut", &run)
}

/// Diktor のモードの旗である(後に書いたものが効く)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    TypeCheck,
    Repl,
    DumpTokens,
    DumpAst,
}

/// IR の旗を含まない diktor の呼び出しを、Diktor の引数解析(driver.ml の `parse_args`)と同じ規則で読む。
///
/// モード、入力のファイル、`--prelude` があるかを返す。
/// 知らない旗や、引数の足りない旗があれば `None` を返す(本物の diktor に使い方の誤りを報告させる)。
fn diktor_mode(args: &[OsString]) -> Option<(Mode, Vec<OsString>, bool)> {
    let mut mode = Mode::Run;
    let mut files = Vec::new();
    let mut prelude = false;
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        let a = raw.to_str().unwrap_or("");
        match a {
            "--type-check" => mode = Mode::TypeCheck,
            "--repl" => mode = Mode::Repl,
            "--dump-tokens" => mode = Mode::DumpTokens,
            "--dump-ast" => mode = Mode::DumpAst,
            "--no-prelude" | "--strict-exhaustive" => {}
            "--prelude" | "--import-path" => {
                if i + 1 >= args.len() {
                    return None;
                }
                prelude |= a == "--prelude";
                i += 1;
            }
            _ if raw.as_bytes().first() == Some(&b'-') => return None,
            _ => files.push(raw.clone()),
        }
        i += 1;
    }
    Some((mode, files, prelude))
}

/// 組 `kf` などで、`--dump-tokens` と `--dump-ast` の呼び出しを `keleut dump tokens|ast` に振り向ける。
///
/// Diktor のダンプのモードは、プレリュードと import の旗を読み飛ばし、ファイルを並べた順に処理する。
/// `keleut dump` も同じ順に処理するので、ファイルだけを渡す。
/// 使い方の誤りになる呼び出しは、本物の diktor に渡す。
fn dump_to_keleut(real_diktor: &OsString, args: &[OsString]) -> u8 {
    let tool = match diktor_mode(args) {
        Some((Mode::DumpTokens, files, _)) if !files.is_empty() => ("tokens", files),
        Some((Mode::DumpAst, files, _)) if !files.is_empty() => ("ast", files),
        _ => return exec(real_diktor, "diktor", args),
    };
    let real = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    let mut v = vec![OsString::from("dump"), OsString::from(tool.0)];
    v.extend(tool.1);
    exec(&real, "keleut", &v)
}

/// 組 `kf` の実行と型検査の呼び出しを、Diktor の旗のまま `keleut check` の引数に写す。
///
/// モードと、`keleut check` に渡す引数(旗とファイル)を返す。
/// 使い方の誤りになる呼び出しと、UTF-8 でない引数を含む呼び出しは `None` を返す(本物の diktor に渡す)。
fn kf_check_args(args: &[OsString]) -> Option<(Mode, Vec<OsString>)> {
    let mut mode = Mode::Run;
    let mut flags = Vec::new();
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_str()?;
        match a {
            "--type-check" => mode = Mode::TypeCheck,
            "--repl" => mode = Mode::Repl,
            "--dump-tokens" => mode = Mode::DumpTokens,
            "--dump-ast" => mode = Mode::DumpAst,
            "--no-prelude" | "--strict-exhaustive" => flags.push(OsString::from(a)),
            "--prelude" | "--import-path" => {
                let v = args.get(i + 1)?.to_str()?;
                flags.push(OsString::from(a));
                flags.push(OsString::from(v));
                i += 1;
            }
            _ if a.starts_with('-') => return None,
            _ => files.push(OsString::from(a)),
        }
        i += 1;
    }
    if files.is_empty() {
        return None;
    }
    flags.extend(files);
    Some((mode, flags))
}

/// 組 `kf` の振り分けを記録の行に足す(`--list-invocations` に `kf-check` の行として出る)。
fn log_kf(outcome: &str) {
    log_route("kf-check", outcome);
}

/// 振り分けを記録の行に足す(`--list-invocations` に `tag` の行として出る)。
fn log_route(tag: &str, outcome: &str) {
    let Some(log) = env("KCONFORM_LOG") else {
        return;
    };
    let cmd = std::env::var("KCONFORM_CMD").unwrap_or_default();
    let line = format!("{cmd}\t{tag}\t{outcome}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(log)
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

/// 組 `kf` の実行と型検査の呼び出しを、RustKeleut の型検査に通す。
///
/// 型検査の呼び出し(`--type-check`)は `keleut check` に振り向け、その出力と終了状態を返す。
/// 実行の呼び出しは、先に `keleut check --quiet` を走らせ、Diktor が実行の前段の型検査で止まる入力
/// (読めない入力、字句、構文、型、import の誤り、`--strict-exhaustive` の警告)では、その診断と終了状態を返す。
/// 型検査を通った入力は、本物の diktor に渡して実行する(警告は diktor が出し直す)。
fn kf_check(real_diktor: &OsString, args: &[OsString]) -> u8 {
    let (mode, kargs) = match kf_check_args(args) {
        Some((m @ (Mode::Run | Mode::TypeCheck), k)) => (m, k),
        _ => return exec(real_diktor, "diktor", args),
    };
    let real = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    let mut cmd = Command::new(&real);
    cmd.arg0("keleut").arg("check");
    if mode == Mode::Run {
        cmd.arg("--quiet");
    }
    cmd.args(&kargs);
    if mode == Mode::TypeCheck {
        match cmd.status() {
            Ok(st) => {
                log_kf("keleut");
                st.code().map_or(70, |c| c as u8)
            }
            Err(e) => exec_failed(&real, e),
        }
    } else {
        let out = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();
        match out {
            Ok(o) if o.status.success() => {
                log_kf("keleut-pass");
                exec(real_diktor, "diktor", args)
            }
            Ok(o) => {
                log_kf("keleut");
                use std::io::Write;
                let _ = std::io::stderr().write_all(&o.stderr);
                o.status.code().map_or(70, |c| c as u8)
            }
            Err(e) => exec_failed(&real, e),
        }
    }
}

/// `keleut run` の選択肢のうち、Diktor にもある旗と、その引数の数である。
const RUN_SHARED: &[(&str, usize)] = &[("--no-prelude", 0), ("--prelude", 1), ("--import-path", 1)];

/// `keleut run` の選択肢のうち、RustKeleut だけの旗と、その引数の数である。
const RUN_KELEUT_ONLY: &[(&str, usize)] = &[
    ("--backend", 1),
    ("--tier", 1),
    ("--max-depth", 1),
    ("--native-stack", 1),
    ("--frontend", 1),
    ("--gc-stress", 0),
    ("--no-yield-prune", 0),
];

/// `keleut run ARGS` の `ARGS` を、Diktor の呼び出しに写す(組 `dk` と `dk-ir`)。
///
/// 戻り値は、Diktor の引数、`--run-ir` に足す旗、KIR のファイルを走らせるかどうかである。
/// テキストの KIR(`.kirt`)も KIR のファイルとして扱う。
/// `diktor --run-ir` はバイナリしか読まないので、[`keleut`] がその呼び出しを断る。
fn run_to_diktor(args: &[OsString]) -> (Vec<OsString>, Vec<OsString>, bool) {
    let mut out = Vec::new();
    let mut ir_extra = Vec::new();
    let mut is_kir = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_str().unwrap_or("");
        if let Some((_, n)) = RUN_SHARED.iter().find(|(f, _)| *f == a) {
            out.extend(args[i..(i + 1 + n).min(args.len())].iter().cloned());
            i += 1 + n;
        } else if let Some((f, n)) = RUN_KELEUT_ONLY.iter().find(|(f, _)| *f == a) {
            if *f == "--max-depth" {
                ir_extra.extend(args[i..(i + 1 + n).min(args.len())].iter().cloned());
            }
            i += 1 + n;
        } else {
            let b = args[i].as_bytes();
            if b.ends_with(b".kir") || b.ends_with(b".kirt") {
                is_kir = true;
            }
            out.push(args[i].clone());
            i += 1;
        }
    }
    (out, ir_extra, is_kir)
}

/// `keleut` の shim の本体である。
pub fn keleut(args: &[OsString]) -> u8 {
    restore_closed_fds();
    let pair = std::env::var("KCONFORM_PAIR").unwrap_or_default();
    let is_run = args.first().is_some_and(|a| a == "run");
    if is_run && (pair == "dk" || pair == "dk-ir" || pair == "kf" || pair == "x-ir") {
        let real = match required("KCONFORM_REAL_DIKTOR") {
            Ok(r) => r,
            Err(c) => return c,
        };
        let (dargs, ir_extra, is_kir) = run_to_diktor(&args[1..]);
        // `diktor --run-ir` はテキストの KIR を読まないので、Diktor の側だけが shim のせいで 65 になる。
        // 基準の組の結果を RustKeleut のアセンブラに頼らせないため、変換せずに断る。
        if dargs.iter().any(|a| a.as_bytes().ends_with(b".kirt")) {
            eprintln!(
                "kconform shim: 組 {pair} は keleut run のテキストの KIR(.kirt)を Diktor で走らせられません"
            );
            return 4;
        }
        if is_kir {
            let mut v = vec![OsString::from("--run-ir")];
            v.extend(dargs);
            v.extend(ir_extra);
            return exec(&real, "diktor", &v);
        }
        if pair == "dk-ir" {
            return diktor_via_emit(&real, &dargs, &ir_extra);
        }
        if pair == "x-ir" {
            return keleut_via_emit(&real, &dargs, &ir_extra);
        }
        return exec(&real, "diktor", &dargs);
    }
    let real = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    if is_run {
        let pargs = pair_args();
        let (own, note) = own_backend(&pair, &pargs, &args[1..]);
        if let Some(note) = note {
            log_route("backend-override", &note);
        }
        let mut v = vec![OsString::from("run")];
        v.extend(pargs);
        v.extend(own);
        return exec(&real, "keleut", &v);
    }
    exec(&real, "keleut", args)
}

/// LLVM の後段の組である。
const JIT_PAIRS: &[&str] = &["kj", "kt", "ka", "km"];

/// コマンドが自分で書いた `keleut run --backend` と、組の後段の食い違いを扱う。
///
/// shim は組の選択肢を先に、コマンドの引数を後に並べるので、後に書いた旗が効く `keleut run` ではコマンドの `--backend` が組の後段に勝つ。
/// LLVM の後段の組(`kj`、`kt`、`ka`、`km`)では、組の後段を優先して、コマンドの `--backend` を取り除く。
/// `--backend interp` を書いたコマンドが組の JIT を通らず、比べたことにならないのを防ぐためである。
/// そのほかの組(`kw` など)では、コマンドの `--backend` を残す(Node は仮想メモリの上限 512 MiB の下で起動できない。V215)。
///
/// 返すのは、組の選択肢の後に並べる引数と、食い違いがあったときの記録の文面である。
/// 文面は呼び出しの記録に `backend-override` の行として足し、kconform が後段を上書きしたコマンドとして報告する。
fn own_backend(
    pair: &str,
    pair_args: &[OsString],
    own: &[OsString],
) -> (Vec<OsString>, Option<String>) {
    let jit = JIT_PAIRS.contains(&pair);
    let pair_backend = pair_args
        .windows(2)
        .rev()
        .find(|w| w[0] == "--backend")
        .map(|w| w[1].to_string_lossy().into_owned())
        .or_else(|| jit.then(|| "jit".to_string()));
    let mut out = Vec::with_capacity(own.len());
    let mut notes = Vec::new();
    let mut i = 0;
    while i < own.len() {
        if own[i] == "--backend" && i + 1 < own.len() {
            let v = own[i + 1].to_string_lossy().into_owned();
            match &pair_backend {
                Some(pb) if jit => {
                    if *pb != v {
                        notes.push(format!("--backend {v} を除いて {pb} で走らせた"));
                    }
                }
                Some(pb) if *pb != v => {
                    notes.push(format!("--backend {v} が組の {pb} に勝った"));
                    out.extend_from_slice(&own[i..i + 2]);
                }
                _ => out.extend_from_slice(&own[i..i + 2]),
            }
            i += 2;
        } else {
            out.push(own[i].clone());
            i += 1;
        }
    }
    let note = (!notes.is_empty()).then(|| notes.join("、"));
    (out, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(xs: &[&str]) -> Vec<OsString> {
        xs.iter().map(OsString::from).collect()
    }

    #[test]
    fn llvm_の組ではコマンドの後段より組の後段を優先する() {
        let own = a(&["--native-stack", "512k", "--backend", "interp", "x.kel"]);
        let (v, n) = own_backend("kt", &a(&["--tier", "threshold=1"]), &own);
        assert_eq!(v, a(&["--native-stack", "512k", "x.kel"]));
        assert_eq!(
            n.as_deref(),
            Some("--backend interp を除いて jit で走らせた")
        );
        let (v, n) = own_backend("kj", &a(&["--backend", "jit", "--tier", "jit-only"]), &own);
        assert_eq!(v, a(&["--native-stack", "512k", "x.kel"]));
        assert!(n.is_some());
        let (v, n) = own_backend("kw", &a(&["--backend", "wasm"]), &own);
        assert_eq!(v, own);
        assert_eq!(n.as_deref(), Some("--backend interp が組の wasm に勝った"));
        let (v, n) = own_backend(
            "ki",
            &a(&["--backend", "interp", "--tier", "interp-only"]),
            &own,
        );
        assert_eq!((v, n), (own.clone(), None));
        let plain = a(&["x.kel"]);
        assert_eq!(
            own_backend("ka", &a(&["--tier", "alternating=1"]), &plain),
            (plain, None)
        );
    }

    #[test]
    fn 後段の組を見分ける() {
        for p in ["ki", "kj", "kt", "ka", "km", "kw"] {
            assert!(is_keleut_pair(p), "{p}");
        }
        for p in ["dk", "dk-ir", "kf", "x-ir", "zz", ""] {
            assert!(!is_keleut_pair(p), "{p}");
        }
        // 実行と型検査は diktor の引数解析と同じ規則でモードを決める(後に書いたものが効く)。
        let (m, f, _) = diktor_mode(&a(&["--strict-exhaustive", "x.kel"])).unwrap();
        assert_eq!((m, f), (Mode::Run, a(&["x.kel"])));
        let (m, _, _) = diktor_mode(&a(&["--strict-exhaustive", "--type-check", "x.kel"])).unwrap();
        assert_eq!(m, Mode::TypeCheck);
    }

    #[test]
    fn rust_のフロントエンドの組と_repl_の組を見分ける() {
        assert!(is_rust_pair("kr-ki"));
        assert!(!is_keleut_pair("kr-ki"));
        for p in ["dk", "ki", "kw", "x-ir", ""] {
            assert!(!is_rust_pair(p), "{p}");
        }
        // --repl は、後段に関係なく RustKeleut の後段の組すべてで keleut repl に振り向ける。
        for p in ["ki", "kj", "kt", "ka", "km", "kw", "kr-ki"] {
            assert!(repl_pair(p), "{p}");
        }
        for p in ["dk", "dk-ir", "kf", "x-ir", "zz", ""] {
            assert!(!repl_pair(p), "{p}");
        }
    }

    #[test]
    fn 網羅を厳しくする実行は組_dk_ir_でも実行にする() {
        assert!(strict_exhaustive_run(&a(&["--strict-exhaustive", "x.kel"])));
        assert!(strict_exhaustive_run(&a(&[
            "--no-prelude",
            "--strict-exhaustive",
            "x.kel"
        ])));
        assert!(!strict_exhaustive_run(&a(&[
            "--strict-exhaustive",
            "--type-check",
            "x.kel"
        ])));
        assert!(!strict_exhaustive_run(&a(&["--type-check", "x.kel"])));
        assert!(!strict_exhaustive_run(&a(&["--strict-exhaustive"])));
        assert!(!strict_exhaustive_run(&a(&[
            "--strict-exhaustive",
            "--typo",
            "x.kel"
        ])));
    }

    #[test]
    fn 表のとおりに振り分ける() {
        assert_eq!(classify(&a(&["x.kel"]), false), Route::Run);
        assert_eq!(classify(&a(&["--no-prelude", "x.kel"]), false), Route::Run);
        assert_eq!(
            classify(&a(&["--prelude", "p.kel", "x.kel"]), false),
            Route::Run
        );
        assert_eq!(
            classify(&a(&["--import-path", "lib", "x.kel"]), false),
            Route::Run
        );
        assert_eq!(
            classify(&a(&["--type-check", "x.kel"]), false),
            Route::TypeCheck
        );
        assert_eq!(
            classify(&a(&["--strict-exhaustive", "x.kel"]), false),
            Route::TypeCheck
        );
        assert_eq!(classify(&a(&["--dump-ast", "x.kel"]), false), Route::Dump);
        assert_eq!(classify(&a(&["--repl"]), false), Route::Repl);
        assert_eq!(
            classify(&a(&["--emit-ir", "o.kir", "x.kel"]), false),
            Route::DiktorIr
        );
        assert_eq!(classify(&a(&["x.kel"]), true), Route::DiktorIr);
    }

    #[test]
    fn 二つの行に当たれば_ir_の旗を先に当てる() {
        assert_eq!(
            classify(
                &a(&["--type-check", "--emit-ir-text", "t.kirt", "x.kel"]),
                false
            ),
            Route::DiktorIr
        );
        assert_eq!(
            classify(&a(&["--repl", "--max-depth", "5"]), false),
            Route::DiktorIr
        );
    }

    #[test]
    fn 旗の引数を旗と取り違えない() {
        assert_eq!(
            classify(&a(&["--prelude", "--repl", "x.kel"]), false),
            Route::Run
        );
    }

    #[test]
    fn diktor_のモードと入力を読む() {
        let (m, f, p) = diktor_mode(&a(&["--no-prelude", "--dump-ast", "x.kel", "y.kel"])).unwrap();
        assert_eq!((m, f, p), (Mode::DumpAst, a(&["x.kel", "y.kel"]), false));
        let (m, _, p) = diktor_mode(&a(&[
            "--dump-tokens",
            "--type-check",
            "--prelude",
            "p.kel",
            "x.kel",
        ]))
        .unwrap();
        assert_eq!((m, p), (Mode::TypeCheck, true));
        assert!(diktor_mode(&a(&["--typo", "x.kel"])).is_none());
        assert!(diktor_mode(&a(&["x.kel", "--prelude"])).is_none());
    }

    #[test]
    fn keleut_run_の選択肢を_diktor_に写す() {
        let (d, ir, k) = run_to_diktor(&a(&[
            "--backend",
            "interp",
            "--no-prelude",
            "--max-depth",
            "10",
            "x.kel",
        ]));
        assert_eq!(d, a(&["--no-prelude", "x.kel"]));
        assert_eq!(ir, a(&["--max-depth", "10"]));
        assert!(!k);
        assert!(run_to_diktor(&a(&["p.kir"])).2);
    }

    #[test]
    fn 実行の旗を_emit_kir_に写す() {
        assert_eq!(
            emit_args(&a(&["--no-prelude", "--import-path", "lib", "x.kel"])).unwrap(),
            a(&["--no-prelude", "--import-path", "lib", "x.kel"])
        );
        assert!(emit_args(&a(&["--typo", "x.kel"])).is_none());
        assert!(emit_args(&a(&["--no-prelude"])).is_none());
        assert!(emit_args(&a(&["x.kel", "--prelude"])).is_none());
    }
}
