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
        ("dk" | "dk-ir" | "", _)
        | (_, Route::TypeCheck | Route::Dump | Route::Repl | Route::DiktorIr) => {
            exec(&real, "diktor", args)
        }
        (p, Route::Run) => {
            // 実行を keleut に振り向けるのは M88 からである。
            eprintln!("kconform shim: 組 {p} の diktor の実行の振り分けはまだありません");
            4
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
    if is_run && (pair == "dk" || pair == "dk-ir") {
        let real = match required("KCONFORM_REAL_DIKTOR") {
            Ok(r) => r,
            Err(c) => return c,
        };
        let (dargs, ir_extra, is_kir) = run_to_diktor(&args[1..]);
        if is_kir {
            let mut v = vec![OsString::from("--run-ir")];
            v.extend(dargs);
            v.extend(ir_extra);
            return exec(&real, "diktor", &v);
        }
        if pair == "dk-ir" {
            return diktor_via_emit(&real, &dargs, &ir_extra);
        }
        return exec(&real, "diktor", &dargs);
    }
    let real = match required("KCONFORM_REAL_KELEUT") {
        Ok(r) => r,
        Err(c) => return c,
    };
    if is_run {
        let mut v = vec![OsString::from("run")];
        v.extend(pair_args());
        v.extend_from_slice(&args[1..]);
        return exec(&real, "keleut", &v);
    }
    exec(&real, "keleut", args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(xs: &[&str]) -> Vec<OsString> {
        xs.iter().map(OsString::from).collect()
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
}
