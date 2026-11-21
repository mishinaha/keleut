//! rtcore を x86_64 の静的ライブラリと `rt.wasm` にビルドする。
//!
//! コンパイルオプションは、`-std=c99 -O2 -ffreestanding -fno-builtin -ffp-contract=off -fno-math-errno` に、`-nostdlibinc`、警告の設定、ターゲットごとのオプションを追加したものである。
//! どちらのターゲットでも clang を直接呼び出し、ここに書いたオプションだけを渡す。
//! `CFLAGS` などの環境変数は読まないので、cargo の profile や環境によらず、同じオプションでビルドする。
//!
//! ツールは環境変数 `KELEUT_CLANG`、`KELEUT_AR`、`KELEUT_WASM_LD` で変更できる。
//! 既定は `clang`、`llvm-ar`、`wasm-ld` である。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// rtcore のソースファイル。`rtcore/` からの相対パスで書く。
const SOURCES: &[&str] = &[
    "f64show.c",
    "shape.c",
    "bytes.c",
    "musl/src/math/sin.c",
    "musl/src/math/cos.c",
    "musl/src/math/__sin.c",
    "musl/src/math/__cos.c",
    "musl/src/math/__rem_pio2.c",
    "musl/src/math/__rem_pio2_large.c",
    "musl/src/math/exp.c",
    "musl/src/math/exp_data.c",
    "musl/src/math/log.c",
    "musl/src/math/log_data.c",
    "musl/src/math/floor.c",
    "musl/src/math/scalbn.c",
    "musl/src/math/__math_oflow.c",
    "musl/src/math/__math_uflow.c",
    "musl/src/math/__math_xflow.c",
    "musl/src/math/__math_divzero.c",
    "musl/src/math/__math_invalid.c",
];

/// `rt.wasm` が export するシンボル。`src/lib.rs` の `RT_WASM_EXPORTS` と同じ内容にする。
const WASM_EXPORTS: &[&str] = &[
    "krt_sin",
    "krt_cos",
    "krt_exp",
    "krt_log",
    "krt_f64_show",
    "krt_f64_show_bits",
    "krt_scratch",
    "krt_bytes_compare",
    "krt_shapes_bytes",
    "krt_shapes_init",
    "krt_shapes_copy",
    "krt_shapes_count",
    "krt_shapes_labels_used",
    "krt_shape_intern",
    "krt_shape_len",
    "krt_shape_labels",
    "krt_shape_find",
    "krt_shape_extend",
    "krt_shape_restrict",
    "__heap_base",
];

/// 両方のターゲットに共通のコンパイルオプション。`-I` と `-include` は `main` で追加する。
const COMMON_FLAGS: &[&str] = &[
    "-std=c99",
    "-O2",
    "-ffreestanding",
    "-fno-builtin",
    "-ffp-contract=off",
    "-fno-math-errno",
    "-nostdlibinc",
    "-Wall",
    "-Wno-unused-function",
    "-Wno-parentheses",
    "-Wno-dangling-else",
    "-Wno-unused-but-set-variable",
];

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let rtcore = manifest.join("../../rtcore");
    let rtcore = rtcore.canonicalize().unwrap_or(rtcore);
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let clang = env::var("KELEUT_CLANG").unwrap_or_else(|_| "clang".to_string());
    let ar = env::var("KELEUT_AR").unwrap_or_else(|_| "llvm-ar".to_string());
    let wasm_ld = env::var("KELEUT_WASM_LD").unwrap_or_else(|_| "wasm-ld".to_string());
    println!("cargo:rerun-if-env-changed=KELEUT_CLANG");
    println!("cargo:rerun-if-env-changed=KELEUT_AR");
    println!("cargo:rerun-if-env-changed=KELEUT_WASM_LD");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    rerun_if_changed(&rtcore);

    let target = env::var("TARGET").expect("TARGET");
    if !target.starts_with("x86_64-") {
        panic!("keleut-rtcore: x86_64 のターゲットだけに対応している(TARGET = {target})");
    }

    let shim = rtcore.join("shim");
    let internal = rtcore.join("musl/src/internal");
    let names = shim.join("krt_names.h");
    let include_flags: Vec<String> = vec![
        format!("-I{}", shim.display()),
        format!("-I{}", internal.display()),
        format!("-I{}", rtcore.display()),
        "-include".to_string(),
        names.display().to_string(),
    ];

    // x86_64: 1 ファイルずつコンパイルし、ar で静的ライブラリにまとめる。
    // clang は、配布物によっては既定でスタック保護を有効にしている(開発に使っている clang 22.1.8 の配布物は有効にしている)。
    // そのときは sin.c などが rtcore の外のシンボル __stack_chk_fail を参照するので、-fno-stack-protector を渡す。
    // Rust のテストの実行ファイルは PIE なので、-fPIC でコンパイルする。
    let x86_flags: Vec<String> = [
        format!("--target={target}"),
        "-fPIC".to_string(),
        "-fno-stack-protector".to_string(),
        "-ffunction-sections".to_string(),
        "-fdata-sections".to_string(),
    ]
    .into_iter()
    .chain(include_flags.iter().cloned())
    .collect();
    let x86_objs = compile_all(&clang, &x86_flags, &rtcore, &out.join("x86_64"));
    let archive = out.join("libkeleut_rtcore.a");
    if archive.exists() {
        fs::remove_file(&archive).expect("remove old archive");
    }
    let mut cmd = Command::new(&ar);
    cmd.arg("crs").arg(&archive).args(&x86_objs);
    run(&mut cmd);
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=keleut_rtcore");
    println!("cargo:rustc-env=KRT_STATIC_LIB={}", archive.display());

    // wasm32: 1 ファイルずつコンパイルし、wasm-ld でリンクして rt.wasm にする。
    let wasm_flags: Vec<String> = ["--target=wasm32".to_string(), "-nostdlib".to_string()]
        .into_iter()
        .chain(include_flags.iter().cloned())
        .collect();
    let wasm_objs = compile_all(&clang, &wasm_flags, &rtcore, &out.join("wasm32"));
    let wasm = out.join("rt.wasm");
    let mut cmd = Command::new(&wasm_ld);
    cmd.arg("--no-entry")
        .arg("-z")
        .arg("stack-size=1048576")
        .args(WASM_EXPORTS.iter().map(|e| format!("--export={e}")))
        .arg("-o")
        .arg(&wasm)
        .args(&wasm_objs);
    run(&mut cmd);

    // テストが Node での検査(tests/wasm_check.mjs)に渡すファイルに、どのビルドの結果かを書けるようにする。
    let profile = env::var("PROFILE").expect("PROFILE");
    println!("cargo:rustc-env=KRT_PROFILE={profile}");
    println!("cargo:rustc-env=KRT_TARGET={target}");

    // Node での検査が引数なしで rt.wasm を見つけられるように、<target>/rtcore/ にもコピーする。
    let stage = stage_dir();
    fs::create_dir_all(&stage).expect("create stage dir");
    let tmp = stage.join(format!("rt.wasm.{}.tmp", std::process::id()));
    fs::copy(&wasm, &tmp).expect("copy rt.wasm");
    fs::rename(&tmp, stage.join("rt.wasm")).expect("rename rt.wasm");
    println!("cargo:rustc-env=KRT_STAGE_DIR={}", stage.display());
}

/// `SOURCES` を `clang` で 1 ファイルずつ並行にコンパイルし、`dir` に置いたオブジェクトファイルのパスを返す。
///
/// 渡すコンパイルオプションは `COMMON_FLAGS` と `extra` だけで、環境変数からは追加しない。
fn compile_all(clang: &str, extra: &[String], rtcore: &Path, dir: &Path) -> Vec<PathBuf> {
    fs::create_dir_all(dir).expect("create object dir");
    let jobs: Vec<(Command, std::process::Child, PathBuf)> = SOURCES
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let obj = dir.join(format!("{i:02}.o"));
            let mut cmd = Command::new(clang);
            cmd.args(COMMON_FLAGS)
                .args(extra)
                .arg("-c")
                .arg(rtcore.join(s))
                .arg("-o")
                .arg(&obj);
            let child = cmd
                .spawn()
                .unwrap_or_else(|e| panic!("{cmd:?} を起動できない: {e}"));
            (cmd, child, obj)
        })
        .collect();
    jobs.into_iter()
        .map(|(cmd, mut child, obj)| {
            let status = child.wait().expect("clang を待てない");
            if !status.success() {
                panic!("{cmd:?} が失敗した: {status}");
            }
            obj
        })
        .collect()
}

/// `<target>/rtcore` のパスを返す。
///
/// `<target>` は、`CARGO_TARGET_DIR` があればその値、なければ workspace のルートの `target` である。
/// `--target` や profile によらず同じパスになるので、`tests/wasm_check.mjs` が既定で読むディレクトリと一致する。
/// ただし、相対パスの `CARGO_TARGET_DIR` を使うときに一致するのは、`tests/wasm_check.mjs` を workspace のルート(`rustkeleut/`)で実行した場合だけである。
/// `tests/wasm_check.mjs` は、相対パスを作業ディレクトリから解決するからである。
fn stage_dir() -> PathBuf {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let dir = match env::var_os("CARGO_TARGET_DIR") {
        // 相対パスは workspace のルートからの相対パスとみなす。
        Some(dir) => root.join(dir),
        None => root.join("target"),
    };
    let dir = dir.join("rtcore");
    // コピー先のディレクトリはまだないことがあるので、親ディレクトリだけを正規化する。
    match dir.parent().and_then(|p| p.canonicalize().ok()) {
        Some(p) => p.join("rtcore"),
        None => dir,
    }
}

fn rerun_if_changed(dir: &Path) {
    for entry in fs::read_dir(dir).expect("read rtcore dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rerun_if_changed(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn run(cmd: &mut Command) {
    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("{cmd:?} を起動できない: {e}"));
    if !status.success() {
        panic!("{cmd:?} が失敗した: {status}");
    }
}
