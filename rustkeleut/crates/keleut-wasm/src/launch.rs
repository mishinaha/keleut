//! `keleut run --backend wasm` と `keleut build` の出力の書き出しと、Node の起動。
//!
//! ## 出力
//!
//! 出力は `program.wasm`、`rt.wasm`、`main.mjs` の 3 つである。
//! `main.mjs` は、`host/keleut-host.mjs` の後ろに、表(JSON)を渡して起動する 1 行を追加したものである。
//! `keleut build` は、このほかに起動スクリプト `run`([`RUN_SH`])を、実行権限を付けて書く。
//! 起動スクリプトは、Node を起動する前に標準出力が閉じているかを調べ、`keleut run --backend wasm` と同じく環境変数 `KELEUT_STDOUT_CLOSED` でホストに伝える。
//! Node は起動時に、閉じたファイル記述子 1 を `/dev/null` で開き直す。
//! そのため、`node DIR/main.mjs` を直接実行すると、ホストは閉じた標準出力を `/dev/null` に向けた実行と区別できず、プログラムが出力しても終了コード 0 で終了する。
//!
//! ## スタック
//!
//! `keleut run --backend wasm` は、3 つのファイルを一時ディレクトリ(`$TMPDIR/keleut-wasm-PID-N-NANOS`、モード 0700)に書き、`node` を子プロセスとして起動する。
//! 子プロセスには、`--stack-size` に `(L + R) × B_prog + 32 MiB` を KiB 単位で渡す([`crate::stack_size_kib`])。
//! `L` は論理的な深さの上限、`R` は予備の深さである(IRSpec 11.5 節)。
//! 子プロセスの `RLIMIT_STACK` は、`pre_exec` の中の `setrlimit` で `S = --stack-size + 64 MiB` に設定する。
//! `B_prog` は、プログラムの 1 段に必要なバイト数である([`crate::b_prog`])。
//! 同じ深さに重なる節の経路に、ハンドラ式を持つ `cancel` 節も、`r` に数える `return` 節や分類 N の操作節もなければ、`B_prog` は `B_wasm` に等しい(`k` と `r` の数え方は [`crate::clause_path`])。
//! それ以外のプログラムは、既定の `L` でもスタックが V8 の `--stack-size` の上限を超えるので、`Worker` の経路で実行する。
//! スタックサイズは [`crate::STACK_CAP_KIB`] で切り詰める。
//!
//! 次のどれかに当てはまるときは、`--stack-size` と `RLIMIT_STACK` を使わない。
//! `main.mjs` が `Worker` を作り、その `resourceLimits.stackSizeMb` で確保したスタックで実行する(`Worker` の経路)。
//!
//! - `RLIMIT_STACK` のハードリミットが `S` より小さいとき。
//! - `--stack-size` に渡す値が V8 の上限([`crate::STACK_SIZE_MAX_KIB`])を超えるとき。
//! - `RLIMIT_AS` か `RLIMIT_DATA` のソフトリミットが無限でないとき。
//!   glibc は `RLIMIT_STACK` を新しいスレッドのスタックの既定のサイズに使うので、`RLIMIT_STACK` を上げると Node のどのスレッドも `S` バイトを予約する。
//!   仮想メモリを制限していると、Node はスレッドを作れず、止まったまま終了しない。
//!
//! ## 終了コードとシグナル
//!
//! 標準入出力は子プロセスに引き継ぐ。
//! 子プロセスの終了コードのうち、ホストが使う 0、3、4、70、74 はそのまま返す。
//! 子プロセスがそれ以外の終了コードで終了したときと、シグナルで終了したときは、内部エラーとして 70 を返す。
//! 子プロセスには `PR_SET_PDEATHSIG` を設定し、`keleut` が終了したら SIGKILL が届くようにする。
//!
//! `keleut` は、SIGINT、SIGTERM、SIGHUP を受け取ると、同じシグナルを子プロセスに送る。
//! その後、子プロセスの終了を待ち、一時ディレクトリを削除してから、同じシグナルで自分も終了する。
//! ただし、`keleut` の起動時に無視する設定(`SIG_IGN`)だったシグナルは、無視したままにして子プロセスに送らない。
//! Node は引き継いだ `SIG_IGN` を起動時に既定の動作に戻すので、無視するシグナルの名前を環境変数 `KELEUT_IGNORED_SIGNALS` で渡す。
//! 名前は `SIGHUP,SIGINT` のようにコンマで区切る。
//! `main.mjs` は、名前を渡されたシグナルに、何もしないシグナルハンドラを登録する。
//! そのため、`nohup` で起動したときの SIGHUP や、シェルスクリプトの中のバックグラウンドジョブに送られた SIGINT では、インタプリタと同じく終了しない。
//!
//! ## メモリ不足
//!
//! V8 のヒープそのものを使い切ったときは、Node が SIGABRT で終了し、`keleut` は終了コード 70 で終了する。
//! これは LangSpec §15.5 との既知の相違である。
//! このとき `cancel` 節は実行されず、ホストのバッファに残っていた標準出力は失われる。
//! `ERROR(OutOfMemory)` になって終了コード 3 で終了するのは、次の 2 つの場合だけである。
//!
//! - `N_alloc_max` を超える確保(`conformance/oom/` がテストする)。
//! - ホストが線形メモリを拡張できないために、シェイプ表を作れないか拡張できない場合。
//!   起動時に行う最初の表の確保と `META.shapes` の登録も、この場合に含む。
//!
//! V8 のヒープを使い切って 70 で終了することは、既知の差分として登録していない。
//!
//! ## 一時ディレクトリの削除
//!
//! `main.mjs` は、2 つの `.wasm` を読んだ後で、一時ディレクトリを自分で削除する。
//! 削除するのは、環境変数 `KELEUT_WASM_RMDIR` がそのディレクトリを指すときだけである。
//! `keleut` も、子プロセスの終了を待った後で同じディレクトリを削除する。
//! 前の実行が SIGKILL で終了して残した一時ディレクトリは、次の実行が、作ったプロセスがもう存在しないことを確かめてから削除する。

use std::path::{Path, PathBuf};
use std::process::Command;

use keleut_lir::Program;

use crate::{Options, STACK_SIZE_MAX_KIB, compile, stack_need_kib, stack_size_kib};

/// Node 側のランタイム `host/keleut-host.mjs` のソース。
pub const HOST_JS: &str = include_str!("../../../host/keleut-host.mjs");

/// `main.mjs` の内容を返す。
///
/// ホストのソースの後ろに、表 `meta`(JSON)を渡して `start` を呼ぶ 1 行を追加する。
pub fn main_mjs(meta: &str) -> String {
    format!("{HOST_JS}\nawait start({meta});\n")
}

/// `keleut build` が `DIR/run` に書く起動スクリプト。
///
/// ファイル記述子 1 をファイル記述子 3 に複製できなければ、標準出力は閉じている。
/// 閉じていれば `KELEUT_STDOUT_CLOSED=1` を設定し、開いていれば環境に残っている `KELEUT_STDOUT_CLOSED` を取り除いてから、`node` で `main.mjs` を起動する。
/// この検査は、`$(...)` のようにファイル記述子を開く処理より前に行う。
/// `main.mjs` は `dirname "$0"` で探すので、`run` は、`run` 自身のパスか、`DIR` へのシンボリックリンクを通したパスで起動する。
/// `run` そのものへのシンボリックリンクから起動すると、Node が `MODULE_NOT_FOUND` のエラーを出して終了コード 1 で終了する。
pub const RUN_SH: &str = r#"#!/bin/sh
# keleut build の出力の起動の台本。node main.mjs を直に走らせると、閉じた標準出力を見分けられない。
if { true 3>&1; } 2>/dev/null; then
  unset KELEUT_STDOUT_CLOSED
else
  KELEUT_STDOUT_CLOSED=1
  export KELEUT_STDOUT_CLOSED
fi
exec node "$(dirname "$0")/main.mjs" "$@"
"#;

/// コード生成を行い、結果を検証して trap の検査をし、出力を返す。
///
/// 返すのは、`program.wasm` の内容、`main.mjs` の内容、プログラムの 1 段に必要なバイト数 `B_prog` の組である。
///
/// 環境変数 `KELEUT_WASM_STATS` が設定されていれば、コード生成の統計を標準エラーに 1 行で書く。
/// この行は測定に使う。
/// 行の末尾には `k`、`B_prog`、`r`([`crate::clause_path`] の値)を書く。
/// 切り詰める前のサイズ [`crate::stack_need_kib`] より小さいスタックを使うときは、どう切り詰めたかも書く。
/// [`crate::STACK_CAP_KIB`] で切り詰めたときは `、スタックを STACK_CAP で切った` と書く。
/// `STACK_CAP` が `B_prog = B_wasm` のときのサイズ(`k = 0` のサイズ)より小さく、そのサイズで切り詰めたときは `、スタックを k = 0 の大きさで切った` と書く。
pub fn build_outputs(prog: &Program, max_depth: u64) -> Result<(Vec<u8>, String, u64), String> {
    let out = compile(prog, &Options { max_depth })?;
    crate::check::validate(&out.program)?;
    // rt.wasm は C で書いた、線形メモリを使うコードなので、trap の検査はせず、検証だけを行う。
    crate::check::validate(keleut_rtcore::RT_WASM).map_err(|e| format!("rt.wasm: {e}"))?;
    let rep = crate::check::trap_check(&out.program)?;
    if std::env::var_os("KELEUT_WASM_STATS").is_some() {
        let need = stack_need_kib(max_depth, out.stats.b_prog);
        let size = stack_size_kib(max_depth, out.stats.b_prog);
        let cut = if need <= size {
            ""
        } else if size == crate::STACK_CAP_KIB {
            "、スタックを STACK_CAP で切った"
        } else {
            "、スタックを k = 0 の大きさで切った"
        };
        eprintln!(
            "keleut-wasm: {:?}、program.wasm {} バイト、検査した関数 {} 個(補助関数 {} 個)、命令 {} 個、k = {}、B_prog = {} バイト、r = {}{}",
            out.stats,
            out.program.len(),
            rep.funcs,
            rep.safe,
            rep.ops,
            out.stats.cancel_nest,
            out.stats.b_prog,
            out.stats.ret_chain,
            cut
        );
    }
    let b = out.stats.b_prog;
    Ok((out.program, main_mjs(&out.meta), b))
}

/// 3 つのファイル(`program.wasm`、`rt.wasm`、`main.mjs`)を `dir` に書く。
pub fn write_dir(dir: &Path, program: &[u8], main: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("program.wasm"), program)?;
    std::fs::write(dir.join("rt.wasm"), keleut_rtcore::RT_WASM)?;
    std::fs::write(dir.join("main.mjs"), main)?;
    Ok(())
}

/// 起動スクリプト [`RUN_SH`] を `dir/run` に書き、実行権限(モード 0755)を付ける。
pub fn write_run(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("run");
    std::fs::write(&p, RUN_SH)?;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
}

/// `keleut build --target wasm -o DIR` の本体。
///
/// 3 つのファイルと起動スクリプト `run` を `dir` に書く。
pub fn build(prog: &Program, max_depth: u64, dir: &Path) -> Result<(), String> {
    let (program, main, _) = build_outputs(prog, max_depth)?;
    write_dir(dir, &program, &main)
        .and_then(|()| write_run(dir))
        .map_err(|e| format!("{}: {e}", dir.display()))
}

/// `keleut run --backend wasm` の実行オプション。
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// 論理的な深さの上限 `L`。
    pub max_depth: u64,
    /// `keleut` の起動時に標準出力が閉じていたら真。
    ///
    /// 真なら、子プロセスに `KELEUT_STDOUT_CLOSED=1` を渡し、ホストが標準出力への書き込みを EBADF の出力エラーにする。
    pub stdout_closed: bool,
}

/// 一時ディレクトリ。ドロップするときにディレクトリを削除する。
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const TEMP_PREFIX: &str = "keleut-wasm-";

/// 一時ディレクトリをモード 0700 で作る。
///
/// 同じ名前のものがすでにあれば、作り直さずにエラーを返す。
/// 作る前に、残っている古い一時ディレクトリを削除する([`sweep_stale`])。
fn temp_dir() -> std::io::Result<TempDir> {
    use std::os::unix::fs::DirBuilderExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let base = std::env::temp_dir();
    sweep_stale(&base);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path = base.join(format!(
        "{TEMP_PREFIX}{}-{}-{nanos}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&path)?;
    Ok(TempDir(path))
}

/// `base` の下にある一時ディレクトリのうち、作ったプロセスがもう存在しないものを削除する。
///
/// SIGKILL で終了した実行が残したディレクトリが、これに該当する。
/// 作ったプロセスのプロセス ID は、ディレクトリの名前から読む。
fn sweep_stale(base: &Path) {
    let Ok(rd) = std::fs::read_dir(base) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(TEMP_PREFIX)) else {
            continue;
        };
        let Some(pid) = rest.split('-').next().and_then(|p| p.parse::<i32>().ok()) else {
            continue;
        };
        if pid <= 0 || pid == std::process::id() as i32 {
            continue;
        }
        // SAFETY: シグナル 0 を指定した kill はシグナルを送らず、プロセスが存在するかどうかだけを調べる。
        let r = unsafe { kill(pid, 0) };
        if r != 0 && std::io::Error::last_os_error().raw_os_error() == Some(ESRCH) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[repr(C)]
struct RLimit {
    cur: u64,
    max: u64,
}

unsafe extern "C" {
    fn getrlimit(resource: i32, rlim: *mut RLimit) -> i32;
    fn setrlimit(resource: i32, rlim: *const RLimit) -> i32;
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
    fn getppid() -> i32;
    fn getpid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
    fn sigaction(sig: i32, act: *const SigAction, old: *mut SigAction) -> i32;
    fn raise(sig: i32) -> i32;
}

/// Linux(x86_64、glibc)の `struct sigaction`。
#[repr(C)]
struct SigAction {
    handler: usize,
    mask: [u64; 16],
    flags: i32,
    restorer: usize,
}

/// Linux の `RLIMIT_DATA`。
const RLIMIT_DATA: i32 = 2;
/// Linux の `RLIMIT_STACK`。
const RLIMIT_STACK: i32 = 3;
/// Linux の `RLIMIT_AS`。
const RLIMIT_AS: i32 = 9;
/// 制限がないことを表す `RLIM_INFINITY`。
const RLIM_INFINITY: u64 = u64::MAX;
/// `prctl` の操作 `PR_SET_PDEATHSIG`。親プロセスが終了したときに届くシグナルを設定する。
const PR_SET_PDEATHSIG: i32 = 1;
const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
const ESRCH: i32 = 3;
/// シグナルの既定の動作を表す `SIG_DFL`。
const SIG_DFL: usize = 0;
/// シグナルを無視する設定を表す `SIG_IGN`。
const SIG_IGN: usize = 1;
/// `signal` が失敗したときに返す `SIG_ERR`。
const SIG_ERR: usize = usize::MAX;

/// 資源 `resource` の現在の制限を、(ソフトリミット、ハードリミット)の組で返す。
fn limit(resource: i32) -> Option<(u64, u64)> {
    let mut r = RLimit { cur: 0, max: 0 };
    // SAFETY: r は有効な領域で、getrlimit は呼び出しの間だけ r に書き込む。
    if unsafe { getrlimit(resource, &mut r) } == 0 {
        Some((r.cur, r.max))
    } else {
        None
    }
}

/// 現在の `RLIMIT_STACK` のハードリミットを返す。
fn hard_stack_limit() -> Option<u64> {
    limit(RLIMIT_STACK).map(|l| l.1)
}

/// 子プロセスの `RLIMIT_STACK` のソフトリミットを上げてよければ真を返す。
///
/// 上げてよいのは、`RLIMIT_AS` と `RLIMIT_DATA` のソフトリミットがどちらも無限のときである。
fn address_space_unlimited() -> bool {
    [RLIMIT_AS, RLIMIT_DATA]
        .iter()
        .all(|&r| matches!(limit(r), Some((RLIM_INFINITY, _))))
}

/// Node のメインスレッドのスタックで実行するときのスタックサイズを決める。
///
/// `--stack-size` に渡す KiB と、`RLIMIT_STACK` に設定するバイト数の組を返す。
/// この方法を使えないときは `None` を返し、呼び出し元は `Worker` の経路を使う。
/// `b_prog` は、プログラムの 1 段に必要なバイト数 `B_prog` である。
pub fn stack_plan(max_depth: u64, b_prog: u64) -> Option<(u64, u64)> {
    let kib = stack_size_kib(max_depth, b_prog);
    let s = (kib + (64 << 10)).saturating_mul(1024);
    if kib > STACK_SIZE_MAX_KIB || !address_space_unlimited() {
        return None;
    }
    match hard_stack_limit() {
        Some(RLIM_INFINITY) => Some((kib, s)),
        Some(h) if h >= s => Some((kib, s)),
        _ => None,
    }
}

/// 受け取ったシグナルの転送先である、子プロセスのプロセス ID。0 なら転送しない。
static CHILD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// 受け取ったシグナルの番号。0 なら受け取っていない。
static GOT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// シグナルハンドラ。受け取ったシグナルの番号を [`GOT`] に書き、同じシグナルを子プロセスに送る。
extern "C" fn forward(sig: i32) {
    use std::sync::atomic::Ordering;
    GOT.store(sig, Ordering::SeqCst);
    let pid = CHILD.load(Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: kill は async-signal-safe である。
        unsafe {
            kill(pid, sig);
        }
    }
}

/// 子プロセスに転送するシグナルの番号と名前。
const FORWARDED: [(i32, &str); 3] = [(SIGINT, "SIGINT"), (SIGTERM, "SIGTERM"), (SIGHUP, "SIGHUP")];

/// シグナル `sig` の現在の設定が無視(`SIG_IGN`)なら真を返す。
fn ignored(sig: i32) -> bool {
    let mut a = SigAction {
        handler: 0,
        mask: [0; 16],
        flags: 0,
        restorer: 0,
    };
    // SAFETY: 新しい設定を渡さず、現在の設定を読むだけである。
    // a は有効な領域で、sigaction は呼び出しの間だけ a に書き込む。
    unsafe { sigaction(sig, std::ptr::null(), &mut a) == 0 && a.handler == SIG_IGN }
}

/// 子プロセスの終了の種類から、`keleut` の終了コードを決める。
///
/// ホストが使う終了コード(0、3、4、70、74)はそのまま返す。
/// それ以外の終了コードとシグナルによる終了は、内部エラーのメッセージを標準エラーに書いて 70 を返す。
fn exit_status(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match status.code() {
        Some(c @ (0 | 3 | 4 | 70 | 74)) => c as u8,
        Some(c) => {
            eprintln!("keleut: 内部エラー: node が終了状態 {c} で終わりました");
            70
        }
        None => {
            eprintln!(
                "keleut: 内部エラー: node がシグナル {} で終わりました",
                status.signal().unwrap_or(0)
            );
            70
        }
    }
}

/// `keleut run --backend wasm` の本体。`keleut` の終了コードを返す。
pub fn run(prog: &Program, o: &RunOptions) -> u8 {
    let (program, main, b_prog) = match build_outputs(prog, o.max_depth) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("keleut: 内部エラー: {e}");
            return 70;
        }
    };
    let dir = match temp_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "keleut: 内部エラー: 一時の場所を作れません: {}: {e}",
                std::env::temp_dir().display()
            );
            return 70;
        }
    };
    if let Err(e) = write_dir(&dir.0, &program, &main) {
        eprintln!(
            "keleut: 内部エラー: 一時の場所に書けません: {}: {e}",
            dir.0.display()
        );
        return 70;
    }
    drop(program);
    let node = std::env::var_os("KELEUT_NODE").unwrap_or_else(|| "node".into());
    let mut cmd = Command::new(&node);
    let plan = stack_plan(o.max_depth, b_prog);
    if let Some((kib, _)) = plan {
        cmd.arg(format!("--stack-size={kib}"))
            .env("KELEUT_WASM_DIRECT", "1");
    } else {
        cmd.env_remove("KELEUT_WASM_DIRECT");
    }
    let stack = plan.map(|(_, s)| (s, hard_stack_limit().unwrap_or(RLIM_INFINITY)));
    // SAFETY: getpid は fork の前に呼ぶ。
    let parent = unsafe { getpid() };
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl、getppid、setrlimit は async-signal-safe で、fork の後の子プロセスで呼んでよい。
        unsafe {
            cmd.pre_exec(move || {
                // keleut が終了したら、SIGKILL による終了でも、子プロセスに SIGKILL が届くようにする。
                // 設定する前に keleut が終了していた場合は、getppid の値が変わるので、それを調べてエラーにする。
                if prctl(PR_SET_PDEATHSIG, SIGKILL as u64, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if getppid() != parent {
                    return Err(std::io::Error::other("親のプロセスが先に終わりました"));
                }
                if let Some((s, hard)) = stack {
                    let r = RLimit { cur: s, max: hard };
                    if setrlimit(RLIMIT_STACK, &r) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    if o.stdout_closed {
        cmd.env("KELEUT_STDOUT_CLOSED", "1");
    } else {
        cmd.env_remove("KELEUT_STDOUT_CLOSED");
    }
    cmd.env("KELEUT_WASM_RMDIR", &dir.0);
    let forwarded: Vec<i32> = FORWARDED
        .iter()
        .filter(|(s, _)| !ignored(*s))
        .map(|(s, _)| *s)
        .collect();
    let names: Vec<&str> = FORWARDED
        .iter()
        .filter(|(s, _)| !forwarded.contains(s))
        .map(|(_, n)| *n)
        .collect();
    if names.is_empty() {
        cmd.env_remove("KELEUT_IGNORED_SIGNALS");
    } else {
        cmd.env("KELEUT_IGNORED_SIGNALS", names.join(","));
    }
    cmd.arg(dir.0.join("main.mjs"));
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "keleut: 内部エラー: node を起動できません({}): {e}",
                Path::new(&node).display()
            );
            return 70;
        }
    };
    use std::sync::atomic::Ordering;
    CHILD.store(child.id() as i32, Ordering::SeqCst);
    let mut old = Vec::new();
    // 無視する設定だったシグナルにはシグナルハンドラを登録せず、無視したままにする。
    // そのシグナルの番号は GOT に入らないので、自分に送り直すこともない。
    for sig in forwarded {
        // SAFETY: forward は async-signal-safe な操作(アトミックな読み書きと kill)だけを行う。
        old.push((sig, unsafe { signal(sig, forward as *const () as usize) }));
    }
    let waited = child.wait();
    CHILD.store(0, Ordering::SeqCst);
    for (sig, h) in old {
        if h != SIG_ERR {
            // SAFETY: signal が返した元の設定に戻すだけである。
            unsafe {
                signal(sig, h);
            }
        }
    }
    let got = GOT.swap(0, Ordering::SeqCst);
    if got != 0 {
        // 一時ディレクトリを削除してから、受け取ったシグナルで終了する。
        drop(dir);
        // SAFETY: シグナルの設定を既定の動作に戻して、同じシグナルを自分に送るだけである。
        unsafe {
            signal(got, SIG_DFL);
            raise(got);
        }
    }
    match waited {
        Ok(s) => exit_status(s),
        Err(e) => {
            eprintln!("keleut: 内部エラー: node の終わりを待てません: {e}");
            70
        }
    }
}
