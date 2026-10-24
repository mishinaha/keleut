//! `--type-check` の手順である(Diktor の driver.ml の `type_check_files_units` と elab.ml の `type_check_units`)。
//!
//! 入力を読む関数を受け取り、Diktor と同じ順に、プレリュードの構文解析、プレリュードの平坦化、
//! コマンド行のファイルの構文解析、型検査を行う。
//! import を書いたファイルは、まだ移していない段(単位の表と import の解決)を要するので、
//! [`Stop::Unsupported`] で止まる。

use super::decls::{ROOT_UNIT, STD_UNIT};
use super::{Fail, OutLine, Tc};
use crate::actions::Ctx;
use crate::ast::Decl;
use crate::pos::{Files, Span};
use crate::{Limits, ParseError, parse_with};

/// 埋め込んだプレリュード(Diktor の lib/prelude.kel の写し)である。
pub const PRELUDE: &str = include_str!("prelude.kel");

/// 型検査の誤り(Diktor の `Elab.error`)である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckError {
    /// 位置(`ファイル:行:桁`)。
    pub loc: Option<String>,
    /// 種別の語。
    pub word: String,
    /// 終了状態。
    pub exit: u8,
    /// 文面。
    pub msg: String,
}

impl CheckError {
    /// 表示(Diktor の `render_error`)。
    pub fn render(&self) -> String {
        match &self.loc {
            Some(p) => format!("! {p}: {}: {}", self.word, self.msg),
            None => format!("! {}: {}", self.word, self.msg),
        }
    }
}

/// 型検査に進む前に止まった理由である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// 入力を読めない(終了状態 64)。中身は `パス: 理由`。
    Input(String),
    /// 字句エラー(終了状態 2。Diktor は終わるときに標準エラーへ書く)。
    Lex(String),
    /// 構文エラーと構文解析の限度(終了状態 2。Diktor はその場で標準エラーへ書く)。
    Syntax(String),
    /// 内部の誤り(終了状態 3)。
    Internal(String),
    /// 移していない段を要する入力。
    Unsupported(String),
}

/// 型検査の結果である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckOutput {
    /// 出力の行(型の行と警告)。
    pub lines: Vec<OutLine>,
    /// 誤り。
    pub error: Option<CheckError>,
    /// 内部の誤り(Diktor の `Panic`。行を出さずに標準エラーへ書いて 3 で終わる)。
    pub panic: Option<String>,
}

/// 型検査の選択肢である。
pub struct CheckOptions {
    /// `--no-prelude`。
    pub no_prelude: bool,
    /// `--prelude PATH`。
    pub prelude: Option<String>,
    /// 入力のファイル。
    pub files: Vec<String>,
    /// 構文解析の限度。
    pub max_nest: usize,
}

/// 木の深さ 1 段あたりに見込む型検査のスタックのバイト数である。
pub const CHECK_STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) {
    32 << 10
} else {
    8 << 10
};

/// 深さに依らずに見込む型検査のスタック(プレリュードの検査など)のバイト数である。
const CHECK_STACK_BASE: usize = 64 << 20;

/// 型検査の専用のスレッドのスタックの既定の大きさで、上限でもある(8 GiB)。
///
/// 推論した型の深さは木の深さで抑えられない(`let` 多相の合成は、数行で深さ 2^n の型を作る)。
/// そこで、深さの上限から見積もった大きさではなく、作れる最大のスタックを既定にする。
/// スタックは使った分だけ実際の記憶を使う。
pub const CHECK_STACK_BYTES: usize = 8 << 30;

/// 深さの上限 `max_nest` の木の型検査に見込むスタックのバイト数である。
fn stack_for_nest(max_nest: usize) -> Option<usize> {
    max_nest
        .checked_mul(CHECK_STACK_BYTES_PER_LEVEL)?
        .checked_add(CHECK_STACK_BASE)
}

/// 型検査の限度(構文解析の深さの上限と、専用のスレッドのスタックの大きさ)を決める。
///
/// `native_stack` が無ければ、スタックは [`CHECK_STACK_BYTES`] にする。
/// `max_nest` の木に見込むスタックが [`CHECK_STACK_BYTES`] を超えるときは誤りである。
///
/// `native_stack` があれば、そのスタックで型検査を終えられる深さまで `max_nest` を下げる。
/// [`guard`](super::guard) の余白を除いた残りを、1 段あたり [`CHECK_STACK_BYTES_PER_LEVEL`] で割った深さである。
/// その深さが 0 になる小さいスタックと、[`CHECK_STACK_BYTES`] を超えるスタックは誤りである。
///
/// 誤りの文面は、コマンド行の使い方の誤りとしてそのまま出せる形にしてある。
pub fn check_limits(max_nest: usize, native_stack: Option<usize>) -> Result<Limits, String> {
    match native_stack {
        None => {
            if max_nest == 0 || stack_for_nest(max_nest).is_none_or(|b| b > CHECK_STACK_BYTES) {
                return Err(format!("bad --max-nest value: {max_nest}"));
            }
            Ok(Limits {
                max_nest,
                stack_bytes: CHECK_STACK_BYTES,
            })
        }
        Some(s) => {
            if s > CHECK_STACK_BYTES {
                return Err(format!("bad --native-stack value: {s}"));
            }
            let fits = s.saturating_sub(super::guard_margin(s)) / CHECK_STACK_BYTES_PER_LEVEL;
            let n = max_nest.min(fits);
            if n == 0 {
                return Err("--native-stack is too small".to_string());
            }
            Ok(Limits {
                max_nest: n,
                stack_bytes: s,
            })
        }
    }
}

/// `f` を型検査の専用のスレッドで走らせる。
///
/// スレッドのスタックの大きさは `limits.stack_bytes` で、入口でスタックの下端を決めてから `f` を呼ぶ。
/// 型検査([`check_files`])は、このスレッドの中で呼ぶ。
/// ほかのスレッドでは、スタックの検査がつねに失敗し、型検査は限度の診断で止まる。
///
/// 既定の大きさ([`CHECK_STACK_BYTES`])のスタックを作れない環境では、
/// `limits.max_nest` の木に見込む大きさで作り直す。
/// スレッドを作れないときと、`f` が落ちたときは `Err` に文面を入れて返す。
pub fn on_check_stack<T: Send>(limits: Limits, f: impl FnOnce() -> T + Send) -> Result<T, String> {
    let slot = std::sync::Mutex::new(Some(f));
    let mut sizes = vec![limits.stack_bytes];
    if limits.stack_bytes == CHECK_STACK_BYTES
        && let Some(b) = stack_for_nest(limits.max_nest)
        && b < CHECK_STACK_BYTES
    {
        sizes.push(b);
    }
    let mut last = String::new();
    for size in sizes {
        let slot = &slot;
        let r = std::thread::scope(|sc| {
            let h = std::thread::Builder::new()
                .name("keleut-front".to_string())
                .stack_size(size)
                .spawn_scoped(sc, move || {
                    super::set_stack_floor(size);
                    let f = slot
                        .lock()
                        .ok()
                        .and_then(|mut g| g.take())
                        .expect("型検査の本体は 1 度だけ呼ぶ");
                    f()
                });
            match h {
                Ok(h) => Some(
                    h.join()
                        .map_err(|_| "フロントエンドの中で内部の誤りが起きました".to_string()),
                ),
                Err(e) => {
                    last = format!("フロントエンドのスレッドを作れません: {e}");
                    None
                }
            }
        });
        if let Some(r) = r {
            return r;
        }
    }
    Err(last)
}

fn parse_unit(
    cx: &mut Ctx,
    files: &mut Files,
    name: &str,
    src: &[u8],
) -> Result<crate::ast::Program, Stop> {
    let id = files.add(name);
    match parse_with(cx, src, id, files) {
        Ok(p) => Ok(p),
        Err(ParseError::Lex(e)) => Err(Stop::Lex(format!(
            "{}: 字句エラー: {}",
            files.show(e.pos),
            e.msg
        ))),
        Err(ParseError::Syntax(m)) => Err(Stop::Syntax(m)),
        Err(ParseError::Internal(m)) => Err(Stop::Internal(m)),
    }
}

fn fail_to_error(files: &Files, f: Fail) -> Result<CheckError, String> {
    let show = |s: Span| Some(files.show(s.start));
    Ok(match f {
        Fail::TypeAt(s, m) => CheckError {
            loc: show(s),
            word: "型エラー".into(),
            exit: 1,
            msg: m,
        },
        Fail::Type(m) => CheckError {
            loc: None,
            word: "型エラー".into(),
            exit: 1,
            msg: m,
        },
        Fail::NotImplAt(s, m) => CheckError {
            loc: show(s),
            word: "未実装".into(),
            exit: 4,
            msg: m,
        },
        Fail::NotImpl(m) => CheckError {
            loc: None,
            word: "未実装".into(),
            exit: 4,
            msg: m,
        },
        Fail::Import(s, m) => CheckError {
            loc: show(s),
            word: "import エラー".into(),
            exit: 1,
            msg: m,
        },
        Fail::Limit => CheckError {
            loc: None,
            word: "処理系の限度".into(),
            exit: 1,
            msg: "型検査で再帰が深すぎます(スタックオーバーフロー)".into(),
        },
        Fail::Bug(m) => return Err(m),
        Fail::Unsolved(_) => return Err("[BUG] 解けない辞書の穴が漏れました".into()),
    })
}

/// `diktor --type-check` と同じ手順で型検査する。
///
/// `read` は入力のファイルを読む関数で、読めなければ `パス: 理由` を返す。
/// [`on_check_stack`] のスレッドの中で呼ぶ。
pub fn check_files(
    opts: &CheckOptions,
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<CheckOutput, Stop> {
    let mut cx = Ctx::new(opts.max_nest);
    let mut files = Files::new();
    let mut tc = Tc::new();
    // プレリュードの構文解析(Diktor の `load_prelude`)。
    let prelude_decls: Vec<Decl> = if opts.no_prelude {
        Vec::new()
    } else {
        let (name, src) = match &opts.prelude {
            Some(p) => (p.clone(), read(p).map_err(Stop::Input)?),
            None => ("<prelude>".to_string(), PRELUDE.as_bytes().to_vec()),
        };
        let prog = parse_unit(&mut cx, &mut files, &name, &src)?;
        if let Some(im) = prog.imports.first() {
            return Err(Stop::Syntax(format!(
                "{}: 構文エラー: import はコマンド行で渡したファイルにだけ書けます",
                files.show(im.span.start)
            )));
        }
        prog.decls
    };
    // プレリュードの平坦化。
    let prelude_flat = match tc.flatten_modules(&[], &[], STD_UNIT, prelude_decls) {
        Ok(d) => d,
        Err(f) => {
            return Ok(match fail_to_error(&files, f) {
                Ok(e) => CheckOutput {
                    lines: Vec::new(),
                    error: Some(e),
                    panic: None,
                },
                Err(p) => CheckOutput {
                    lines: Vec::new(),
                    error: None,
                    panic: Some(p),
                },
            });
        }
    };
    // コマンド行のファイル(Diktor の `load_units` の根)。
    let mut roots: Vec<Decl> = Vec::new();
    let mut has_imports = false;
    for f in &opts.files {
        let src = read(f).map_err(Stop::Input)?;
        let prog = parse_unit(&mut cx, &mut files, f, &src)?;
        has_imports |= !prog.imports.is_empty();
        roots.extend(prog.decls);
    }
    if has_imports {
        return Err(Stop::Unsupported("import(複数の単位の型検査)".into()));
    }
    tc.d.unit_paths.insert(ROOT_UNIT, opts.files.join("、"));
    // 型検査(Diktor の `type_check_units`)。
    tc.current_out.clear();
    tc.start_program();
    let r = (|| -> Result<(), Fail> {
        let env_std = tc.check_prelude(&prelude_flat)?;
        let decls = tc.flatten_modules(&[], &[], ROOT_UNIT, std::mem::take(&mut roots))?;
        tc.check_unit(ROOT_UNIT, &env_std, &decls)?;
        Ok(())
    })();
    let lines = std::mem::take(&mut tc.current_out);
    Ok(match r {
        Ok(()) => CheckOutput {
            lines,
            error: None,
            panic: None,
        },
        Err(f) => match fail_to_error(&files, f) {
            Ok(e) => CheckOutput {
                lines,
                error: Some(e),
                panic: None,
            },
            Err(p) => CheckOutput {
                lines: Vec::new(),
                error: None,
                panic: Some(p),
            },
        },
    })
}

/// 行の表示(Diktor の `render_line`)。
pub fn render_line(l: &OutLine) -> String {
    match l {
        OutLine::Binding(s) => s.clone(),
        OutLine::Warning(s) => format!("⚠ {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 埋め込んだプレリュードが Diktor のものと同じであることを確かめる。
    /// diktor のサブモジュールが無ければ調べない。
    #[test]
    fn プレリュードは_diktor_のものと同じ() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../diktor/lib/prelude.kel"
        );
        if let Ok(s) = std::fs::read_to_string(p) {
            assert_eq!(
                s, PRELUDE,
                "diktor/lib/prelude.kel が変わりました。src/types/prelude.kel に写してください"
            );
        }
    }
}
