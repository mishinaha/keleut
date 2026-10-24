//! `--type-check` の手順である(Diktor の driver.ml の `type_check_files_units` と elab.ml の `type_check_units`)。
//!
//! 入力を読む手段([`SourceFs`])を受け取り、Diktor と同じ順に、プレリュードの構文解析、プレリュードの平坦化、
//! コマンド行のファイルと import 先の読み込み([`super::load`])、単位ごとの型検査([`super::units`])を行う。

use super::decls::{ROOT_UNIT, STD_UNIT};
use super::fx::HashMap;
use super::load::{LoadError, Loaded, SourceFs, UnitKeyError, load_units, unit_keys};
use super::units::{Bind, PubEntry, UnitTable, import_claims, skeleton};
use super::{Fail, OutLine, Tc, UnitId};
use crate::actions::Ctx;
use crate::ast::{Decl, ImportDecl};
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
}

/// 型検査の結果である。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    /// `--import-path DIR` の並び(書いた順)。
    pub import_path: Vec<String>,
    /// 構文解析の限度。
    pub max_nest: usize,
}

/// 木の深さ 1 段あたりに見込む型検査のスタックのバイト数である。
///
/// 測った値の約 1.5 倍にしてある。
/// 式の入れ子、演算子の鎖、ブロック、`else if` の鎖、`match` の入れ子、レコード、パターン、型の入れ子のどれでも、
/// 型検査に通る最小のスタックは、1 段あたりリリースの構築で約 1.3 KiB、最適化なしの構築で約 20 KiB だった。
pub const CHECK_STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) {
    32 << 10
} else {
    2 << 10
};

/// `--native-stack` で下げた構文解析の深さの限度の下限である。
///
/// 埋め込んだプレリュードの木の深さはこれより浅い(試験で確かめる)。
/// これより浅い限度にしかならない小さいスタックは、プレリュードも構文解析できないので、使い方の誤りにする。
pub const MIN_CHECK_NEST: usize = 64;

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
/// その深さが [`MIN_CHECK_NEST`] に満たない小さいスタックと、[`CHECK_STACK_BYTES`] を超えるスタックは誤りである。
///
/// 限度を下げるのは、[`guard`](super::guard) で調べない再帰(木を捨てることなど)も、スタックに収めるためである。
/// 下げずに `guard` だけに頼ると、深い木を捨てるときに小さいスタックがあふれる。
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
            if fits < MIN_CHECK_NEST {
                return Err("--native-stack is too small".to_string());
            }
            let n = max_nest.min(fits);
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

pub(crate) fn parse_unit(
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

/// 型検査を済ませたプログラムである。
///
/// `keleut check` は [`output`](Checked::output) だけを使う。
/// KIR の書き出しと対話的な実行は、型検査の文脈、ノードごとの注釈、平坦化した宣言を使う。
/// 文脈 [`Tc`] は `Rc` を持つので、この値は [`on_check_stack`] のスレッドの中で使い切る。
pub struct Checked {
    /// 型検査の文脈(宣言表、型、ノードごとの注釈)。
    pub tc: Tc,
    /// 位置の表示に使うファイルの表。
    pub files: Files,
    /// 平坦化したプレリュード。
    pub prelude: Vec<Decl>,
    /// 読み込んだ単位(宣言列は型検査に渡したので空)。
    pub loaded: Loaded,
    /// 平坦化した単位ごとの宣言列。依存する単位の帰りがけ順で、最後が起点である。
    /// 型検査が途中で止まったときは、そこまでの単位だけを持つ。
    pub units: Vec<Vec<Decl>>,
    /// 出力の行と誤り。
    pub output: CheckOutput,
    /// 構文解析の文脈。
    ///
    /// 後から別の入力(REPL の入力)を構文解析するときは、この文脈を使ってノードの番号を続きから振る。
    /// 型検査の文脈の表はノードの番号で引くので、番号が重なってはならない。
    pub cx: Ctx,
}

impl Checked {
    /// 依存する単位の KIR の鍵と表示の列(Diktor の `unit_keys`)。
    pub fn unit_keys(
        &self,
        fs: &mut dyn SourceFs,
        opts: &CheckOptions,
    ) -> Result<Vec<(String, String)>, UnitKeyError> {
        unit_keys(fs, &opts.files, &opts.import_path, &self.loaded)
    }
}

fn failed(files: &Files, lines: Vec<OutLine>, f: Fail) -> CheckOutput {
    match fail_to_error(files, f) {
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
    }
}

/// `diktor --type-check` と同じ手順で型検査し、出力の行と誤りを返す。
///
/// [`on_check_stack`] のスレッドの中で呼ぶ。
pub fn check_files(opts: &CheckOptions, fs: &mut dyn SourceFs) -> Result<CheckOutput, Stop> {
    check_program(opts, fs).map(|c| c.output)
}

/// `diktor --type-check` と同じ手順で型検査し、型検査を済ませたプログラムを返す
/// (Diktor の driver.ml の `type_check_files_units`)。
///
/// 手順は、プレリュードの構文解析と平坦化、コマンド行のファイルと import 先の読み込み、型検査の順である。
/// 読めない入力、字句と構文の誤り、構文解析の内部の誤りは `Err` で返す。
/// 型エラー、import エラー、未実装、処理系の限度は [`CheckOutput::error`] に、
/// 平坦化と型検査の内部の誤りは [`CheckOutput::panic`] に入れて返す。
/// [`on_check_stack`] のスレッドの中で呼ぶ。
pub fn check_program(opts: &CheckOptions, fs: &mut dyn SourceFs) -> Result<Checked, Stop> {
    let mut cx = Ctx::new(opts.max_nest);
    let mut files = Files::new();
    let mut tc = Tc::new();
    let done =
        |cx: Ctx, tc: Tc, files: Files, prelude: Vec<Decl>, loaded: Loaded, units, output| {
            Ok(Checked {
                tc,
                files,
                prelude,
                loaded,
                units,
                output,
                cx,
            })
        };
    // プレリュードの構文解析(Diktor の `load_prelude`)。
    let prelude_decls: Vec<Decl> = if opts.no_prelude {
        Vec::new()
    } else {
        let (name, src) = match &opts.prelude {
            Some(p) => (p.clone(), fs.read_channel(p).map_err(Stop::Input)?),
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
            let out = failed(&files, Vec::new(), f);
            return done(
                cx,
                tc,
                files,
                Vec::new(),
                Loaded::default(),
                Vec::new(),
                out,
            );
        }
    };
    // コマンド行のファイルと import 先の読み込み(Diktor の `load_units`)。
    let mut loaded = match load_units(&mut cx, &mut files, fs, &opts.files, &opts.import_path) {
        Ok(l) => l,
        Err(LoadError::Stop(s)) => return Err(s),
        Err(LoadError::Import(span, msg)) => {
            let out = failed(&files, Vec::new(), Fail::Import(span, msg));
            return done(
                cx,
                tc,
                files,
                prelude_flat,
                Loaded::default(),
                Vec::new(),
                out,
            );
        }
    };
    // 依存する単位に、帰りがけ順に 1 から番号を振る。起点は 0 である。
    let mut ids: HashMap<String, UnitId> = HashMap::default();
    for (i, u) in loaded.deps.iter().enumerate() {
        let n = (i + 1) as UnitId;
        ids.insert(u.id.clone(), n);
        tc.d.unit_paths.insert(n, u.display.clone());
    }
    tc.d.unit_paths.insert(
        ROOT_UNIT,
        loaded
            .roots
            .iter()
            .map(|u| u.display.as_str())
            .collect::<Vec<_>>()
            .join("、"),
    );
    let mut inputs: Vec<UnitIn> = Vec::new();
    for (i, u) in loaded.deps.iter_mut().enumerate() {
        inputs.push(UnitIn {
            id: (i + 1) as UnitId,
            imports: u.targets.iter().map(|(k, t)| (*k, ids[t], 0)).collect(),
            decls: std::mem::take(&mut u.decls),
        });
    }
    let mut root = UnitIn {
        id: ROOT_UNIT,
        imports: Vec::new(),
        decls: Vec::new(),
    };
    for (r, u) in loaded.roots.iter_mut().enumerate() {
        root.imports
            .extend(u.targets.iter().map(|(k, t)| (*k, ids[t], r)));
        root.decls.append(&mut u.decls);
    }
    inputs.push(root);
    // 型検査(Diktor の `type_check_units`)。
    tc.current_out.clear();
    tc.start_program();
    let mut units: Vec<Vec<Decl>> = Vec::new();
    let r = check_units(&mut tc, &prelude_flat, &loaded, inputs, &mut units);
    let lines = std::mem::take(&mut tc.current_out);
    let output = match r {
        Ok(()) => CheckOutput {
            lines,
            error: None,
            panic: None,
        },
        Err(f) => failed(&files, lines, f),
    };
    done(cx, tc, files, prelude_flat, loaded, units, output)
}

/// 型検査に渡す単位(Diktor の `src_unit`)である。
struct UnitIn {
    id: UnitId,
    /// import 文の添字、行き先の単位、import を書いた単位の添字(起点の単位ではコマンド行の何番目のファイルか)。
    imports: Vec<(usize, UnitId, usize)>,
    decls: Vec<Decl>,
}

/// 単位の列を型検査する(Diktor の `type_check_units`)。
///
/// 起点の束縛だけを出力の行に出し、警告はどの単位のものも出す。
fn check_units(
    tc: &mut Tc,
    prelude: &[Decl],
    loaded: &Loaded,
    inputs: Vec<UnitIn>,
    flat: &mut Vec<Vec<Decl>>,
) -> Result<(), Fail> {
    let mut tables: HashMap<UnitId, UnitTable> = HashMap::default();
    let env_std = tc.check_prelude(prelude)?;
    for su in inputs {
        let unit = su.id;
        let src = |k: usize, r: usize| -> &ImportDecl {
            if unit == ROOT_UNIT {
                &loaded.roots[r].imports[k]
            } else {
                &loaded.deps[(unit - 1) as usize].imports[k]
            }
        };
        let mut binds: Vec<Bind> = Vec::new();
        for (k, target, r) in &su.imports {
            let tbl = tables
                .get(target)
                .ok_or_else(|| Fail::Bug("[BUG] import 先の公開表がありません".into()))?;
            binds.extend(Tc::resolve_import_names(tbl, src(*k, *r))?);
        }
        let seen = import_claims(tc, &binds)?;
        let imported_vals: Vec<String> = binds
            .iter()
            .filter(|b| matches!(b.1, PubEntry::Value(..)))
            .map(|b| b.0.clone())
            .collect();
        let imported_types: Vec<String> = binds
            .iter()
            .filter(|b| matches!(b.1, PubEntry::Type(..) | PubEntry::Effect(..)))
            .map(|b| b.0.clone())
            .collect();
        // 骨組みは、公開表(起点でない単位)と、import した名前との衝突の検査にだけ使う。
        let skel = if unit != ROOT_UNIT || !seen.is_empty() {
            skeleton(&su.decls)
        } else {
            Vec::new()
        };
        let decls = tc.flatten_modules(&imported_vals, &imported_types, unit, su.decls)?;
        Tc::check_own_clashes(&seen, &skel)?;
        let env = tc.apply_imports(unit, &env_std, &binds);
        let env2 = tc.check_unit(unit, &env, &decls)?;
        if unit != ROOT_UNIT {
            let t = tc.public_table(unit, &env2, &skel);
            tables.insert(unit, t);
        }
        flat.push(decls);
    }
    Ok(())
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
