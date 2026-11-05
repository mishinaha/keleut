//! `keleut run --backend wasm` と `keleut build` の出力、Node の起動(計画 6.10 節の「Node の起動」、D545)。
//!
//! 出力は `program.wasm`、`rt.wasm`、`main.mjs` の 3 つである。
//! `main.mjs` は `host/keleut-host.mjs` の後ろに、表(JSON)を渡して起動する 1 行を足したものである。
//! `keleut build` は、このほかに起動の台本 `run`([`RUN_SH`]、実行の許可付き)を書く。
//! 台本は Node の前に標準出力が閉じているかを確かめ、`keleut run --backend wasm` と同じく `KELEUT_STDOUT_CLOSED` で host に伝える。
//! Node は起動のときに閉じた fd 1 を `/dev/null` で開き直すので、`node DIR/main.mjs` を直に走らせると、
//! 閉じた標準出力を `/dev/null` に向けた実行と見分けられず、書いても 0 で終わる。
//!
//! `keleut run --backend wasm` は 3 つを一時の場所(`$TMPDIR/keleut-wasm-PID-N-NANOS`、0700)に書き、`node` を子プロセスで起動する。
//! 子の `RLIMIT_STACK` を `S = --stack-size + 64 MiB` にし(`pre_exec` の中の `setrlimit`)、
//! `--stack-size` に `(L + R) × B_prog + 32 MiB`(KiB)を渡す(M79 の記録の 1.5 節で改めた式)。
//! `B_prog` はプログラムの 1 段に見込むバイト数で、`cancel` 節の中にハンドラ式が無く、再帰の道になる `return` 節も無ければ `B_wasm` である(`crate::b_prog`)。
//! そうでないプログラムは、既定の L でもスタックが V8 の上限を超えるので、`Worker` の経路で走る。
//! スタックは [`crate::STACK_CAP_KIB`] で切る。
//! 次のときは、`main.mjs` が `Worker` の `resourceLimits.stackSizeMb` で走らせる。
//!
//! - `RLIMIT_STACK` の硬い限度が `S` より小さいとき。
//! - `--stack-size` が V8 の上限を超えるとき。
//! - `RLIMIT_AS` か `RLIMIT_DATA` の軟らかい限度が無限でないとき。
//!   glibc は `RLIMIT_STACK` を新しいスレッドのスタックの既定の大きさに使うので、上げると Node のどのスレッドも `S` を予約する。
//!   仮想メモリに上限があると、Node はスレッドを作れずに止まったまま戻らない。
//!
//! 標準入出力は子に引き継ぐ。
//! 子の終了状態のうち、ホストが使う 0、3、4、70、74 はそのまま返し、ほかの終了状態とシグナルによる終わりは 70 にする。
//! 子には `PR_SET_PDEATHSIG` で、`keleut` が終わったら SIGKILL が届くようにする。
//! `keleut` が SIGINT、SIGTERM、SIGHUP を受けたら子に送り、子が終わるのを待ち、一時の場所を消してから、同じシグナルで終わる。
//! ただし、`keleut` が起動したときに無視する扱い(`SIG_IGN`)だったシグナルは、無視したままにして子に送らない。
//! Node は引き継いだ `SIG_IGN` を起動のときに既定の扱いに戻すので、無視するシグナルの名前を環境変数
//! `KELEUT_IGNORED_SIGNALS`(`SIGHUP,SIGINT` のようにコンマで区切る)で渡し、`main.mjs` が何もしない処理を登録する。
//! こうして、`nohup` の下の SIGHUP や、シェルの台本の中の後ろで走る仕事の SIGINT で、インタプリタと違って終わることがない。
//!
//! V8 のヒープそのものが尽きたときは、Node が SIGABRT で終わり、`keleut` は 70 で終わる(§15.5 からの既知の逸脱、P252、V215)。
//! このとき `cancel` 節は走らず、ホストのバッファに残っていた標準出力は失われる。
//! `ERROR(OutOfMemory)` と終了状態 3 になるのは、`N_alloc_max` を超える割り当て(`conformance/oom/` が確かめる)と、
//! ホストが線形メモリを広げられずに形の表を作れないか広げられない場合(起動のときの最初の表と `META.shapes` の登録を含む)だけである。
//! V8 のヒープが尽きて 70 で終わることは、既知の差として登録していない。
//! `main.mjs` は 2 つの `.wasm` を読んだ後で一時の場所を自分で消す(環境変数 `KELEUT_WASM_RMDIR` がその場所を指すときだけ)。
//! 前の実行が SIGKILL で終わって残した一時の場所は、次の実行が、作ったプロセスが無いことを確かめて消す。

use std::path::{Path, PathBuf};
use std::process::Command;

use keleut_lir::Program;

use crate::{Options, STACK_SIZE_MAX_KIB, compile, stack_need_kib, stack_size_kib};

/// Node 側の実行時。
pub const HOST_JS: &str = include_str!("../../../host/keleut-host.mjs");

/// `main.mjs` の中身。
pub fn main_mjs(meta: &str) -> String {
    format!("{HOST_JS}\nawait start({meta});\n")
}

/// `keleut build` が `DIR/run` に書く起動の台本。
///
/// fd 1 を fd 3 に複製できなければ、標準出力は閉じている。
/// 閉じていれば `KELEUT_STDOUT_CLOSED=1` を渡し、開いていれば環境に残った `KELEUT_STDOUT_CLOSED` を除いて、
/// `node` を `main.mjs` で起動する。
/// 確かめるのは、`$(...)` のように記述子を開く処理より前である。
/// `main.mjs` は `dirname "$0"` で探すので、`run` は自分の道筋か、`DIR` へのシンボリックリンクを通して起動する。
/// `run` そのものへのシンボリックリンクから起動すると、Node が `MODULE_NOT_FOUND` を出して 1 で終わる。
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

/// 翻訳し、検証と trap の検査をした出力(`program.wasm`、`main.mjs`、プログラムの 1 段のバイト数 `B_prog`)。
///
/// 環境変数 `KELEUT_WASM_STATS` があれば、翻訳の統計を標準エラーに書く(測定用)。
/// 統計の行の後ろには `k`、`B_prog`、`r`([`crate::clause_path`] の値)を書く。
/// 切る前の大きさ [`crate::stack_need_kib`] より小さいスタックを使うときは、その切り方も書く。
/// [`crate::STACK_CAP_KIB`] で切ったときは `、スタックを STACK_CAP で切った`、
/// `STACK_CAP` が `k = 0` の大きさより小さく、`k = 0` の大きさで切ったときは `、スタックを k = 0 の大きさで切った` である。
pub fn build_outputs(prog: &Program, max_depth: u64) -> Result<(Vec<u8>, String, u64), String> {
    let out = compile(prog, &Options { max_depth })?;
    crate::check::validate(&out.program)?;
    // rt.wasm は C の線形メモリのコードなので、trap の検査器でなく検証だけを通す。
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

/// 3 つのファイルを `dir` に書く。
pub fn write_dir(dir: &Path, program: &[u8], main: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("program.wasm"), program)?;
    std::fs::write(dir.join("rt.wasm"), keleut_rtcore::RT_WASM)?;
    std::fs::write(dir.join("main.mjs"), main)?;
    Ok(())
}

/// 起動の台本 [`RUN_SH`] を `dir/run` に、実行の許可(0755)を付けて書く。
pub fn write_run(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("run");
    std::fs::write(&p, RUN_SH)?;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
}

/// `keleut build --target wasm -o DIR` の本体。
///
/// 3 つのファイルと起動の台本 `run` を書く。
pub fn build(prog: &Program, max_depth: u64, dir: &Path) -> Result<(), String> {
    let (program, main, _) = build_outputs(prog, max_depth)?;
    write_dir(dir, &program, &main)
        .and_then(|()| write_run(dir))
        .map_err(|e| format!("{}: {e}", dir.display()))
}

/// 実行の選択肢。
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// 論理的な深さの限度 `L`。
    pub max_depth: u64,
    /// 起動のときに標準出力が閉じていたか(子の書き込みを EBADF で失敗させる)。
    pub stdout_closed: bool,
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const TEMP_PREFIX: &str = "keleut-wasm-";

/// 一時の場所を作る(0700。同じ名前があれば作り直さずに誤りにする)。
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

/// 作ったプロセスがもう無い一時の場所(SIGKILL で終わった実行の残り)を消す。
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
        // SAFETY: シグナル 0 は送らずに、プロセスがあるかだけを調べる。
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
/// `RLIM_INFINITY`。
const RLIM_INFINITY: u64 = u64::MAX;
/// `PR_SET_PDEATHSIG`。
const PR_SET_PDEATHSIG: i32 = 1;
const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
const ESRCH: i32 = 3;
/// `SIG_DFL`。
const SIG_DFL: usize = 0;
/// `SIG_IGN`。
const SIG_IGN: usize = 1;
/// `signal` の失敗(`SIG_ERR`)。
const SIG_ERR: usize = usize::MAX;

/// 今の限度(軟らかい限度、硬い限度)。
fn limit(resource: i32) -> Option<(u64, u64)> {
    let mut r = RLimit { cur: 0, max: 0 };
    // SAFETY: r は呼び出しの間だけ使う有効な領域である。
    if unsafe { getrlimit(resource, &mut r) } == 0 {
        Some((r.cur, r.max))
    } else {
        None
    }
}

/// 今の `RLIMIT_STACK` の硬い限度。
fn hard_stack_limit() -> Option<u64> {
    limit(RLIMIT_STACK).map(|l| l.1)
}

/// 子の `RLIMIT_STACK` の軟らかい限度を上げてよいか(仮想メモリとデータの軟らかい限度がどちらも無限)。
fn address_space_unlimited() -> bool {
    [RLIMIT_AS, RLIMIT_DATA]
        .iter()
        .all(|&r| matches!(limit(r), Some((RLIM_INFINITY, _))))
}

/// スタックの大きさの計画(`--stack-size` の KiB と RLIMIT_STACK のバイト数。使えなければ `None` で Worker の経路)。
///
/// `b_prog` はプログラムの 1 段に見込むバイト数 `B_prog` である。
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

/// 送られたシグナルを渡す子のプロセス番号(0 なら渡さない)。
static CHILD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// 受けたシグナル(0 なら受けていない)。
static GOT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

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

const FORWARDED: [(i32, &str); 3] = [(SIGINT, "SIGINT"), (SIGTERM, "SIGTERM"), (SIGHUP, "SIGHUP")];

/// 今の扱いが無視する扱い(`SIG_IGN`)のシグナルか。
fn ignored(sig: i32) -> bool {
    let mut a = SigAction {
        handler: 0,
        mask: [0; 16],
        flags: 0,
        restorer: 0,
    };
    // SAFETY: 新しい扱いを渡さずに今の扱いを読むだけで、a は呼び出しの間だけ使う有効な領域である。
    unsafe { sigaction(sig, std::ptr::null(), &mut a) == 0 && a.handler == SIG_IGN }
}

/// 子の終了状態を `keleut` の終了状態にする。
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

/// `keleut run --backend wasm` の本体。終了状態を返す。
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
        // SAFETY: prctl、getppid、setrlimit は async-signal-safe で、fork の後の子で呼んでよい。
        unsafe {
            cmd.pre_exec(move || {
                // keleut が終わったら(SIGKILL でも)子も終わる。
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
    // 無視する扱いだったシグナルは無視したままにする(GOT に入らず、自分に送り直すこともない)。
    for sig in forwarded {
        // SAFETY: forward は async-signal-safe な操作(原子的な読み書きと kill)だけを行う。
        old.push((sig, unsafe { signal(sig, forward as *const () as usize) }));
    }
    let waited = child.wait();
    CHILD.store(0, Ordering::SeqCst);
    for (sig, h) in old {
        if h != SIG_ERR {
            // SAFETY: 前の扱いに戻す。
            unsafe {
                signal(sig, h);
            }
        }
    }
    let got = GOT.swap(0, Ordering::SeqCst);
    if got != 0 {
        // 受けたシグナルで終わる(一時の場所を消してから)。
        drop(dir);
        // SAFETY: 既定の扱いに戻して同じシグナルを自分に送る。
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
