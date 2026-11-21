//! Diktor の `--type-check` と同じ型検査の手順である
//! (Diktor の driver.ml の `type_check_files_units` と elab.ml の `type_check_units`)。
//!
//! 入力を読む手段([`SourceFs`])を受け取り、Diktor と同じ順に次の処理を行う。
//! プレリュードの構文解析、プレリュードの平坦化、コマンド行のファイルと import 先の読み込み([`super::load`])、
//! 単位ごとの型検査([`super::units`])である。

use super::decls::{ROOT_UNIT, STD_UNIT};
use super::fx::HashMap;
use super::load::{LoadError, Loaded, SourceFs, UnitKeyError, load_units, unit_keys};
use super::units::{Bind, PubEntry, UnitTable, import_claims, skeleton};
use super::{Fail, OutLine, Tc, UnitId};
use crate::actions::Ctx;
use crate::ast::{Decl, ImportDecl};
use crate::pos::{Files, Span};
use crate::{Limits, ParseError, parse_with};

/// バイナリに埋め込んだプレリュードである(Diktor の lib/prelude.kel のコピー)。
pub const PRELUDE: &str = include_str!("prelude.kel");

/// 型検査が報告するエラーである(Diktor の `Elab.error`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckError {
    /// エラーの位置(`ファイル:行:桁`)。位置のないエラーでは `None`。
    pub loc: Option<String>,
    /// 種別名(`型エラー`、`未実装`、`import エラー`、`処理系の限度`)。
    pub word: String,
    /// 終了コード。
    pub exit: u8,
    /// メッセージ。
    pub msg: String,
}

impl CheckError {
    /// 出力する 1 行の文字列にする(Diktor の `render_error`)。
    pub fn render(&self) -> String {
        match &self.loc {
            Some(p) => format!("! {p}: {}: {}", self.word, self.msg),
            None => format!("! {}: {}", self.word, self.msg),
        }
    }
}

/// 型検査を始める前に処理を終えた理由である。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// 入力を読めない(終了コード 64)。内容は `パス: 理由` の形式のメッセージである。
    Input(String),
    /// 字句エラー(終了コード 2)。Diktor は、終了するときに標準エラーに書く。
    Lex(String),
    /// 構文エラーか、入れ子が構文解析の上限を超えたときの `処理系の限度` の行(終了コード 2)。
    /// Diktor は、検出した時点で標準エラーに書く。
    Syntax(String),
    /// 内部エラー(終了コード 3)。
    Internal(String),
}

/// 型検査の結果である。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckOutput {
    /// 出力する行(束縛の型を表示する行と警告)。
    pub lines: Vec<OutLine>,
    /// 型検査が報告したエラー。なければ `None`。
    pub error: Option<CheckError>,
    /// 内部エラー(Diktor の `Panic`)。
    /// このときは出力する行を持たない。呼び出し元はメッセージを標準エラーに書き、終了コード 3 で終了する。
    pub panic: Option<String>,
}

/// 型検査のオプションである。
pub struct CheckOptions {
    /// `--no-prelude`。
    pub no_prelude: bool,
    /// `--prelude PATH`。
    pub prelude: Option<String>,
    /// 入力ファイル。
    pub files: Vec<String>,
    /// `--import-path DIR` の並び(書いた順)。
    pub import_path: Vec<String>,
    /// 構文解析の上限(作る木の深さの上限)。
    pub max_nest: usize,
}

/// 木の深さ 1 段あたりに見積もる、型検査のスタックのバイト数である。
///
/// 実測では、式の入れ子、演算子の連鎖、ブロック、`else if` の連鎖、`match` の入れ子、レコード、パターン、型の入れ子のどれでも、
/// 型検査を終えられる最小のスタックは、1 段あたりリリースビルドで約 1.3 KiB、デバッグビルドで約 20 KiB である。
/// この定数は、その約 1.5 倍にしてある。
pub const CHECK_STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) {
    32 << 10
} else {
    2 << 10
};

/// `--native-stack` に合わせて構文解析の深さの上限を下げるときの、下限である。
///
/// 埋め込んだプレリュードは、この上限で構文解析できる(テストで確かめている)。
/// 上限がこれより小さくなるスタックでは、プレリュードを構文解析できないことがあるので、使い方のエラーにする。
pub const MIN_CHECK_NEST: usize = 64;

/// 木の深さによらず必要になると見積もる、型検査のスタックのバイト数である(プレリュードの検査などに使う分)。
const CHECK_STACK_BASE: usize = 64 << 20;

/// 型検査専用のスレッドのスタックサイズの既定値で、上限でもある(8 GiB)。
///
/// 推論した型の深さは、木の深さでは抑えられない(`let` 多相を重ねると、数行で深さ 2^n の型ができる)。
/// そこで、深さの上限から見積もったサイズではなく、作れる最大のスタックを既定にする。
/// スタックは、使った分だけ実際のメモリを使う。
pub const CHECK_STACK_BYTES: usize = 8 << 30;

/// 木の深さの上限が `max_nest` のときに、型検査に必要なスタックのバイト数の見積もりを返す。
/// 計算がオーバーフローするときは `None` を返す。
fn stack_for_nest(max_nest: usize) -> Option<usize> {
    max_nest
        .checked_mul(CHECK_STACK_BYTES_PER_LEVEL)?
        .checked_add(CHECK_STACK_BASE)
}

/// 型検査の上限(構文解析の深さの上限と、専用のスレッドのスタックサイズ)を決める。
///
/// `native_stack` がなければ、スタックサイズを [`CHECK_STACK_BYTES`] にする。
/// `max_nest` が 0 のときと、`max_nest` の木に必要と見積もったスタックが [`CHECK_STACK_BYTES`] を超えるときはエラーである。
///
/// `native_stack` があれば、そのスタックで型検査を終えられる深さまで `max_nest` を下げる。
/// その深さは、スタックサイズから [`guard`](super::guard) の余裕を引いた残りを、
/// 1 段あたりの [`CHECK_STACK_BYTES_PER_LEVEL`] で割った値である。
/// その深さが [`MIN_CHECK_NEST`] に満たない小さいスタックと、[`CHECK_STACK_BYTES`] を超えるスタックはエラーである。
///
/// 上限を下げるのは、[`guard`](super::guard) が調べない再帰(木のドロップなど)も、スタックに収めるためである。
/// 下げずに `guard` だけに頼ると、深い木をドロップするときに小さいスタックがオーバーフローする。
///
/// エラーのメッセージは、コマンド行の使い方のエラーとしてそのまま出力できる形式にしてある。
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

/// `f` を型検査専用のスレッドで実行する。
///
/// スレッドのスタックサイズは `limits.stack_bytes` である。
/// スレッドの入口で、使ってよいスタックの下端を決めてから `f` を呼ぶ。
/// 型検査([`check_files`])は、このスレッドの中で呼ぶ。
/// ほかのスレッドでは、スタックの検査がつねに失敗し、型検査は `処理系の限度` の診断で終了する。
///
/// 既定のサイズ([`CHECK_STACK_BYTES`])のスタックを持つスレッドを作れない環境では、
/// `limits.max_nest` の木に必要と見積もったサイズで作り直す。
/// スレッドを作れないときと、`f` がパニックしたときは、`Err` にメッセージを入れて返す。
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
                        .map_err(|_| "フロントエンドの中で内部エラーが起きました".to_string()),
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

pub(crate) fn fail_to_error(files: &Files, f: Fail) -> Result<CheckError, String> {
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

/// 型検査を終えたプログラムである。
///
/// `keleut check` は [`output`](Checked::output) だけを使う。
/// KIR の書き出しと対話的な実行は、型検査のコンテキスト、ノードごとの注釈、平坦化した宣言を使う。
/// 型検査のコンテキスト [`Tc`] は `Rc` を持つので、この値は [`on_check_stack`] のスレッドの中で使い終える。
pub struct Checked {
    /// 型検査のコンテキスト(宣言表、型、ノードごとの注釈)。
    pub tc: Tc,
    /// 位置の表示に使うファイル名の表。
    pub files: Files,
    /// 平坦化したプレリュード。
    pub prelude: Vec<Decl>,
    /// 読み込んだ単位。宣言の並びは型検査に渡したので空である。
    pub loaded: Loaded,
    /// 平坦化した、単位ごとの宣言の並び。
    /// 依存する単位が帰りがけ順に並び、最後が起点である。
    /// 型検査が途中で終わったときは、そこまでの単位だけを持つ。
    pub units: Vec<Vec<Decl>>,
    /// 出力する行とエラー。
    pub output: CheckOutput,
    /// 構文解析のコンテキスト。
    ///
    /// 後から別の入力(REPL の入力)を構文解析するときは、このコンテキストを使ってノードの番号を続きから振る。
    /// 型検査のコンテキストが持つ表はノードの番号をキーにするので、番号が重なってはならない。
    pub cx: Ctx,
}

impl Checked {
    /// 依存する単位ごとに、KIR の単位のキーと表示用のパスの組を返す(Diktor の `unit_keys`)。
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

/// `diktor --type-check` と同じ手順で型検査し、出力する行とエラーを返す。
///
/// [`on_check_stack`] のスレッドの中で呼ぶ。
pub fn check_files(opts: &CheckOptions, fs: &mut dyn SourceFs) -> Result<CheckOutput, Stop> {
    check_program(opts, fs).map(|c| c.output)
}

/// `diktor --type-check` と同じ手順で型検査し、型検査を終えたプログラムを返す
/// (Diktor の driver.ml の `type_check_files_units`)。
///
/// 手順は、プレリュードの構文解析と平坦化、コマンド行のファイルと import 先の読み込み、型検査の順である。
/// 読めない入力、字句エラー、構文エラー、構文解析の内部エラーは `Err` で返す。
/// 型エラー、import エラー、未実装、処理系の限度は [`CheckOutput::error`] に、
/// 平坦化と型検査の内部エラーは [`CheckOutput::panic`] に入れて返す。
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

/// 型検査に渡す単位である(Diktor の `src_unit`)。
struct UnitIn {
    id: UnitId,
    /// import 文ごとの 3 つ組。
    /// import 文の添字、解決先の単位、その import 文を書いたファイルの添字である。
    /// 3 つ目は起点の単位でだけ使い、コマンド行の何番目のファイルかを表す。
    imports: Vec<(usize, UnitId, usize)>,
    decls: Vec<Decl>,
}

/// 単位の並びを順に型検査する(Diktor の `type_check_units`)。
///
/// 束縛の型を表示する行は起点の単位のものだけを出し、警告はどの単位のものも出す。
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
        // 宣言の骨組みは `skeleton` が作る。
        // 宣言の位置、ノードの番号、名前、`pub` の有無など、公開表の作成と衝突の検査に使う情報をコピーしたものである。
        // 起点でない単位の公開表を作るときと、import した名前との衝突を検査するときにだけ使う。
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

/// 出力の行を文字列にする(Diktor の `render_line`)。
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
    /// diktor のサブモジュールがなければ調べない。
    #[test]
    fn プレリュードは_diktor_のものと同じ() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../diktor/lib/prelude.kel"
        );
        if let Ok(s) = std::fs::read_to_string(p) {
            assert_eq!(
                s, PRELUDE,
                "diktor/lib/prelude.kel が変わりました。src/types/prelude.kel にコピーしてください"
            );
        }
    }
}
