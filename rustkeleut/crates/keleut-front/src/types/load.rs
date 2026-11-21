//! import 先を読み込み、KIR の単位(コンパイル単位)のキーを決める(Diktor の driver.ml の `load_units` と `unit_keys` に対応する)。
//!
//! コマンド行で渡したファイル(起点)から import をたどり、依存する単位を集める。
//! 起点の各ファイルの import を、書いた順に深さ優先でたどる。
//! 集めた単位は帰りがけ順に並べるので、import されるファイルが、それを import するファイルより先になる。
//! 同じファイルは 1 回だけ読む。
//! import の循環は、訪問中のファイルの並びを調べて検出する。
//!
//! 2 つのパスが同じファイルを指すかどうかは、シンボリックリンクを解決した絶対パス(`realpath`)で判定する。
//! ユーザに見せるパス(表示用のパス)は、起点ではコマンド行の引数のままである。
//! import 先の表示用のパスは、基準のディレクトリに import のパスをつないで作る。
//! 基準は、相対パスの import なら、import を書いたファイルの表示用のパスが指すディレクトリで、検索パスの import なら `--import-path` のディレクトリである。
//! 作ったパスからは `.` を取り除き、`..` を直前の要素とともに取り除く。
//! `..` を取り除いたパスが元と同じファイルを指さないとき(シンボリックリンクの先のディレクトリから `..` で上ったとき)は、実際に開くパスから `.` の要素だけを取り除いたものを表示用のパスにする。
//!
//! ファイルの読み込み、存在の判定、`realpath` は [`SourceFs`] を通して行う。
//! そのため、テストは作業ディレクトリを変えずに、別のディレクトリを基準にした [`SourceFs`] を渡せる。

use super::check::{Stop, parse_unit};
use crate::actions::Ctx;
use crate::ast::{Decl, ImportDecl};
use crate::pos::{Files, Span};
use std::collections::HashSet;

/// 入力ファイルを読む手段を表すトレイトである。
///
/// エラーメッセージは、OCaml の `Sys_error` と同じ `パス: 理由` の形式にする。
pub trait SourceFs {
    /// ファイルの内容を読む。
    fn read(&mut self, path: &str) -> Result<Vec<u8>, String>;
    /// ファイルの内容を読む。エラーメッセージは、OCaml の `In_channel.with_open_bin` と `input_all` で読んだときと同じにする。
    ///
    /// ファイルを開く段階のエラーは `パス: 理由` で、開いた後の読み込みのエラー(ディレクトリを開いたときなど)は理由だけである。
    /// 既定の実装は [`read`](SourceFs::read) を呼ぶ。
    fn read_channel(&mut self, path: &str) -> Result<Vec<u8>, String> {
        self.read(path)
    }
    /// ファイルがあれば真を返す(OCaml の `Sys.file_exists` と同じで、シンボリックリンクはたどる)。
    fn exists(&mut self, path: &str) -> bool;
    /// シンボリックリンクを解決した絶対パスを返す(OCaml の `Unix.realpath` と同じ)。
    fn realpath(&mut self, path: &str) -> Result<String, String>;
}

/// 入出力エラーのメッセージから、理由の部分を返す。
///
/// OCaml の `Sys_error` の理由の部分と同じになるように、Rust が末尾に付ける `(os error N)` を取り除く。
pub fn io_reason(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// 実際のファイルシステムを読む [`SourceFs`] である。
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFs;

impl SourceFs for StdFs {
    fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(path).map_err(|e| format!("{path}: {}", io_reason(&e)))
    }

    fn read_channel(&mut self, path: &str) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| format!("{path}: {}", io_reason(&e)))?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).map_err(|e| io_reason(&e))?;
        Ok(v)
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn realpath(&mut self, path: &str) -> Result<String, String> {
        std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| format!("{path}: {}", io_reason(&e)))
    }
}

/// 読み込んだ単位である(Diktor の `unit_src`)。
#[derive(Clone, Debug)]
pub struct SrcUnit {
    /// ファイルを区別するパス(`realpath` の結果)。
    pub id: String,
    /// ユーザに見せる表示用のパス。
    pub display: String,
    /// 実際に開いたパス。
    pub path: String,
    /// import 文の並び。
    pub imports: Vec<ImportDecl>,
    /// 宣言の並び。型検査に渡すと空になる。
    pub decls: Vec<Decl>,
    /// import 文の添字と、解決先の単位の `id`(ファイルを区別するパス)の組を並べたもの。
    pub targets: Vec<(usize, String)>,
}

/// 読み込みの結果である。`deps` は依存する単位を帰りがけ順に並べたもので、`roots` は起点のファイルである。
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub deps: Vec<SrcUnit>,
    pub roots: Vec<SrcUnit>,
}

/// 読み込みのエラーである。
#[derive(Clone, Debug)]
pub enum LoadError {
    /// import エラー(終了コード 1)。
    Import(Span, String),
    /// 入力の読み込みエラー、字句エラー、構文エラー、内部エラー。
    Stop(Stop),
}

impl From<Stop> for LoadError {
    fn from(s: Stop) -> LoadError {
        LoadError::Stop(s)
    }
}

/// OCaml の `Filename.dirname` と同じ結果を返す(Unix の規則)。
pub fn dirname(name: &str) -> String {
    let b = name.as_bytes();
    if b.is_empty() {
        return ".".to_string();
    }
    let mut n = b.len() as isize - 1;
    // 末尾の区切り文字を飛ばす。
    while n >= 0 && b[n as usize] == b'/' {
        n -= 1;
    }
    if n < 0 {
        return name[..1].to_string();
    }
    // 最後の区切り文字を探す。
    while n >= 0 && b[n as usize] != b'/' {
        n -= 1;
    }
    if n < 0 {
        return ".".to_string();
    }
    // 連続する区切り文字を飛ばす。
    while n >= 0 && b[n as usize] == b'/' {
        n -= 1;
    }
    if n < 0 {
        return name[..1].to_string();
    }
    name[..(n as usize + 1)].to_string()
}

/// OCaml の `Filename.concat` と同じ結果を返す。
pub fn concat(dir: &str, file: &str) -> String {
    if dir.is_empty() || dir.ends_with('/') {
        format!("{dir}{file}")
    } else {
        format!("{dir}/{file}")
    }
}

/// 表示用のパスを正規化する(Diktor の `normalize_display`)。
///
/// `.` と空の要素を取り除き、`..` は直前の要素とともに取り除く。
/// 直前の要素がないか、直前の要素も `..` のときは、`..` を残す。
pub fn normalize_display(p: &str) -> String {
    let mut acc: Vec<&str> = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => match acc.last() {
                Some(&x) if x != ".." => {
                    acc.pop();
                }
                _ => acc.push(".."),
            },
            c => acc.push(c),
        }
    }
    let s = acc.join("/");
    if p.starts_with('/') {
        format!("/{s}")
    } else {
        s
    }
}

fn import_error<T>(span: Span, msg: impl Into<String>) -> Result<T, LoadError> {
    Err(LoadError::Import(span, msg.into()))
}

/// import のパスの形式を検査し、パスを分解して返す(Diktor の `split_import_path`)。
///
/// 返り値の 1 つ目は、相対パスなら親ディレクトリへ上る段数(`./` で始まるパスは 0)で、検索パスなら `None` である。
/// 2 つ目は残りの要素の並びである。
pub fn split_import_path(span: Span, src: &str) -> Result<(Option<usize>, Vec<String>), LoadError> {
    if src.is_empty() {
        return import_error(span, "import のパスが空です");
    }
    if src.contains(':') {
        return import_error(
            span,
            format!(
                "import のパスにコロンは書けません(\"kel:\" などの接頭辞は予約されています): {src}"
            ),
        );
    }
    if src.contains('\\') {
        return import_error(span, format!("import のパスの区切りは / です: {src}"));
    }
    if src.starts_with('/') {
        return import_error(span, format!("import のパスに絶対パスは書けません: {src}"));
    }
    if src.ends_with(".kel") {
        return import_error(
            span,
            format!("import のパスに拡張子 .kel は書きません: {src}"),
        );
    }
    let comps: Vec<&str> = src.split('/').collect();
    if comps.len() >= 2 && comps[0] == "." && comps[1] == ".." {
        return import_error(
            span,
            format!("import のパスの ./ と ../ は混ぜられません: {src}"),
        );
    }
    let (rel, rest): (Option<usize>, &[&str]) = match comps[0] {
        "." => (Some(0), &comps[1..]),
        ".." => {
            let n = comps.iter().take_while(|c| **c == "..").count();
            (Some(n), &comps[n..])
        }
        _ => (None, &comps[..]),
    };
    if rest.is_empty() {
        return import_error(
            span,
            format!("import のパスにファイル名がありません: {src}"),
        );
    }
    if rest.iter().any(|c| c.is_empty() || *c == "." || *c == "..") {
        return import_error(span, format!("import のパスの要素が不正です: {src}"));
    }
    Ok((rel, rest.iter().map(|s| s.to_string()).collect()))
}

/// 表示用のパスを決める(Diktor の `faithful_display`)。
///
/// `..` を取り除いたパス `display` が `id` と同じファイルを指せば、`display` を返す。
/// 指さなければ、実際に開くパス `path` から `.` と空の要素だけを取り除いたパスを返す。
fn faithful_display(fs: &mut dyn SourceFs, id: &str, path: &str, display: String) -> String {
    let comps: Vec<&str> = path
        .split('/')
        .filter(|c| *c != "." && !c.is_empty())
        .collect();
    let undotted = format!(
        "{}{}",
        if path.starts_with('/') { "/" } else { "" },
        comps.join("/")
    );
    match fs.realpath(&display) {
        Ok(r) if r == id => display,
        _ => undotted,
    }
}

/// import 文の解決先を、(実際に開くパス, 表示用のパス, ファイルを区別するパス) の組で返す(Diktor の `resolve_import`)。
fn resolve_import(
    fs: &mut dyn SourceFs,
    import_path: &[String],
    importer: &SrcUnit,
    im: &ImportDecl,
) -> Result<(String, String, String), LoadError> {
    let span = im.span;
    let src = &im.source;
    let (rel, comps) = split_import_path(span, src)?;
    let file = format!("{}.kel", comps.join("/"));
    match rel {
        Some(ups) => {
            let prefix = if ups == 0 {
                "./".to_string()
            } else {
                "../".repeat(ups)
            };
            let path = concat(&dirname(&importer.path), &format!("{prefix}{file}"));
            let display =
                normalize_display(&format!("{}/{prefix}{file}", dirname(&importer.display)));
            if fs.exists(&path) {
                let id = fs.realpath(&path).map_err(Stop::Input)?;
                let d = faithful_display(fs, &id, &path, display);
                Ok((path, d, id))
            } else {
                import_error(span, format!("import 先が見つかりません: {src}({display})"))
            }
        }
        None => {
            let mut found: Vec<(String, String, String)> = Vec::new();
            for d in import_path {
                let path = concat(d, &file);
                if fs.exists(&path) {
                    let id = fs.realpath(&path).map_err(Stop::Input)?;
                    let base = if d.is_empty() { "." } else { d.as_str() };
                    let disp = normalize_display(&format!("{base}/{file}"));
                    let disp = faithful_display(fs, &id, &path, disp);
                    found.push((path, disp, id));
                }
            }
            // 複数の検索パスで見つけた同じファイルは、1 つと数える。
            let mut distinct: Vec<(String, String, String)> = Vec::new();
            for c in found {
                if !distinct.iter().any(|x| x.2 == c.2) {
                    distinct.push(c);
                }
            }
            match distinct.len() {
                1 => Ok(distinct.pop().expect("1 件ある")),
                0 => import_error(span, format!("import 先が検索パスに見つかりません: {src}")),
                _ => import_error(
                    span,
                    format!(
                        "import 先が検索パスに複数あります: {src}({})",
                        distinct
                            .iter()
                            .map(|c| c.1.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                ),
            }
        }
    }
}

/// ファイルを 1 個読んで構文解析する(Diktor の `read_unit`)。
fn read_unit(
    cx: &mut Ctx,
    files: &mut Files,
    fs: &mut dyn SourceFs,
    path: &str,
    display: &str,
    id: String,
) -> Result<SrcUnit, LoadError> {
    let src = fs.read(path).map_err(|m| {
        // ファイルを開く段階のエラーメッセージは実際に開くパスを含むので、それを表示用のパスに置き換える。
        let m = match m.strip_prefix(path) {
            Some(rest) => format!("{display}{rest}"),
            None => m,
        };
        Stop::Input(m)
    })?;
    let prog = parse_unit(cx, files, display, &src)?;
    Ok(SrcUnit {
        id,
        display: display.to_string(),
        path: path.to_string(),
        imports: prog.imports,
        decls: prog.decls,
        targets: Vec::new(),
    })
}

/// コマンド行で渡したファイルから import をたどって単位を集める(Diktor の `load_units`)。
///
/// 起点のファイルをすべて読んでから、import をたどる。
pub fn load_units(
    cx: &mut Ctx,
    files: &mut Files,
    fs: &mut dyn SourceFs,
    roots: &[String],
    import_path: &[String],
) -> Result<Loaded, LoadError> {
    let mut rs = Vec::new();
    for f in roots {
        let id = fs.realpath(f).map_err(Stop::Input)?;
        rs.push(read_unit(cx, files, fs, f, f, id)?);
    }
    let root_ids: Vec<String> = rs.iter().map(|u| u.id.clone()).collect();
    let mut st = LoadState {
        cx,
        files,
        fs,
        import_path,
        root_ids: &root_ids,
        finished: HashSet::new(),
        order: Vec::new(),
    };
    for r in &mut rs {
        st.visit(r)?;
    }
    Ok(Loaded {
        deps: st.order,
        roots: rs,
    })
}

struct LoadState<'a> {
    cx: &'a mut Ctx,
    files: &'a mut Files,
    fs: &'a mut dyn SourceFs,
    import_path: &'a [String],
    root_ids: &'a [String],
    finished: HashSet<String>,
    order: Vec<SrcUnit>,
}

impl LoadState<'_> {
    /// 起点のファイル `root` から import を深さ優先でたどる。
    ///
    /// import の連鎖の長さに比例するネイティブスタックを使わないように、再帰の代わりに、訪問中の単位を積む `Vec` を使う。
    /// `Vec` の各要素は、読んだ単位と、次に調べる import の添字の組である。
    fn visit(&mut self, root: &mut SrcUnit) -> Result<(), LoadError> {
        // 訪問中の (ファイルを区別するパス, 表示用のパス) を並べたスタックである。末尾が最も新しく、先頭は起点である。
        let mut path_stack = vec![(root.id.clone(), root.display.clone())];
        let mut on_path: HashSet<String> = HashSet::new();
        on_path.insert(root.id.clone());
        let mut frames: Vec<(SrcUnit, usize)> = Vec::new();
        let mut root_next = 0;
        loop {
            let (u, next) = match frames.last_mut() {
                Some((u, k)) => (u, k),
                None => (&mut *root, &mut root_next),
            };
            let k = *next;
            if k == u.imports.len() {
                match frames.pop() {
                    None => return Ok(()),
                    Some((dep, _)) => {
                        path_stack.pop();
                        on_path.remove(&dep.id);
                        self.finished.insert(dep.id.clone());
                        self.order.push(dep);
                    }
                }
                continue;
            }
            *next += 1;
            let im = &u.imports[k];
            let (path, display, id) = resolve_import(self.fs, self.import_path, u, im)?;
            let span = im.span;
            let source = im.source.clone();
            u.targets.push((k, id.clone()));
            if on_path.contains(&id) {
                let i = path_stack
                    .iter()
                    .position(|(x, _)| *x == id)
                    .expect("訪問中のスタックにある");
                let mut cycle: Vec<&str> =
                    path_stack[i..].iter().map(|(_, d)| d.as_str()).collect();
                cycle.push(&display);
                return import_error(
                    span,
                    format!("import が循環しています: {}", cycle.join(" → ")),
                );
            }
            if self.root_ids.contains(&id) {
                return import_error(
                    span,
                    format!("コマンド行で連結したファイルは import できません: {source}"),
                );
            }
            if !self.finished.contains(&id) {
                let dep = read_unit(self.cx, self.files, self.fs, &path, &display, id.clone())?;
                on_path.insert(id.clone());
                path_stack.push((id, display));
                frames.push((dep, 0));
            }
        }
    }
}

// ---- KIR の単位のキー ----

/// KIR の単位のキーを決められないときのエラーである。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitKeyError {
    /// 内部エラー。Diktor の `bug` に対応し、終了コードは 3 である。
    Bug(String),
    /// 別のファイルが同じキーになった(終了コード 64)。値は標準エラーに書く 1 行で、先頭に付ける `keleut: ` を含まない。
    Clash(String),
    /// `realpath` が失敗して入力を読めない。終了コードは 64 である。
    Input(String),
}

fn is_search_mark(c: &str) -> bool {
    c.len() > 1 && c.starts_with('@') && c[1..].bytes().all(|b| b.is_ascii_digit())
}

fn key_components(p: &str) -> Vec<String> {
    let mut acc: Vec<String> = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if acc.len() == 1 && is_search_mark(&acc[0]) {
                    acc.push("..".into());
                } else if acc.last().is_some_and(|x| x != "..") {
                    acc.pop();
                } else {
                    acc.push("..".into());
                }
            }
            c => acc.push(c.to_string()),
        }
    }
    acc
}

fn key_normalize(p: &str) -> String {
    key_components(p).join("/")
}

fn key_dir(k: &str) -> String {
    match k.rfind('/') {
        Some(i) => k[..i].to_string(),
        None => String::new(),
    }
}

/// 正規化したディレクトリ `b` から `d` への相対パスの要素を返す。
///
/// `b` のうち `d` と共通でない部分に `..` があれば、相対パスを決められないので `None` を返す。
fn relative_components(b: &[String], d: &[String]) -> Option<Vec<String>> {
    let n = b.iter().zip(d).take_while(|(x, y)| x == y).count();
    let (b2, d2) = (&b[n..], &d[n..]);
    if b2.iter().any(|x| x == "..") {
        return None;
    }
    let mut v: Vec<String> = b2.iter().map(|_| "..".to_string()).collect();
    v.extend(d2.iter().cloned());
    Some(v)
}

/// 依存する単位の KIR のキーと表示用のパスの組を、`deps` の順に並べて返す(Diktor の `unit_keys`)。
///
/// `files` はコマンド行で渡したファイル、`import_path` は `--import-path` に指定したディレクトリの並びである。
pub fn unit_keys(
    fs: &mut dyn SourceFs,
    files: &[String],
    import_path: &[String],
    loaded: &Loaded,
) -> Result<Vec<(String, String)>, UnitKeyError> {
    let first = files.first().map(|s| s.as_str()).unwrap_or(".");
    let base_dir = dirname(first);
    let base = key_components(&base_dir);
    let realpath = |fs: &mut dyn SourceFs, p: &str| fs.realpath(p).map_err(UnitKeyError::Input);
    let mut keys: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let by_id: std::collections::HashMap<&str, &SrcUnit> =
        loaded.deps.iter().map(|u| (u.id.as_str(), u)).collect();
    // 起点の各ファイルのディレクトリのキー。
    let mut root_dirs = Vec::new();
    for f in files {
        let dir = dirname(f);
        let textual = if base_dir.starts_with('/') == dir.starts_with('/') {
            relative_components(&base, &key_components(&dir))
        } else {
            None
        };
        let comps = match textual {
            Some(cs) => cs,
            None => {
                let rb = realpath(fs, &base_dir)?;
                let rd = realpath(fs, &dir)?;
                match relative_components(&key_components(&rb), &key_components(&rd)) {
                    Some(cs) => cs,
                    None => {
                        return Err(UnitKeyError::Bug(format!(
                            "KIR の単位のキー: 起点のディレクトリの相対パスを決められません: {f}"
                        )));
                    }
                }
            }
        };
        root_dirs.push(comps.join("/"));
    }
    fn key_of(
        fs: &mut dyn SourceFs,
        import_path: &[String],
        dir: &str,
        im: &ImportDecl,
        id: &str,
    ) -> Result<String, UnitKeyError> {
        let src = &im.source;
        match split_import_path(im.span, src) {
            Ok((Some(_), _)) => Ok(key_normalize(&format!(
                "{}{src}.kel",
                if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                }
            ))),
            Ok((None, comps)) => {
                let file = format!("{}.kel", comps.join("/"));
                for (i, d) in import_path.iter().enumerate() {
                    let path = concat(d, &file);
                    if fs.exists(&path) && fs.realpath(&path).map_err(UnitKeyError::Input)? == id {
                        return Ok(format!("@{}/{src}.kel", i + 1));
                    }
                }
                Err(UnitKeyError::Bug(format!(
                    "KIR の単位のキー: 検索パスに見つかりません: {src}"
                )))
            }
            Err(_) => Err(UnitKeyError::Bug(format!(
                "KIR の単位のキー: import のパスが不正です: {src}"
            ))),
        }
    }
    // 起点の各ファイルから、import の解決先を深さ優先でたどってキーを決める。
    // import の連鎖の長さに比例するネイティブスタックを使わないように、再帰の代わりに、(単位, ディレクトリのキー, 次の添字) を積む `Vec` を使う。
    for (r, d) in loaded.roots.iter().zip(&root_dirs) {
        let mut frames: Vec<(&SrcUnit, String, usize)> = vec![(r, d.clone(), 0)];
        while let Some((u, dir, next)) = frames.last_mut() {
            let u: &SrcUnit = u;
            let Some((k, id)) = u.targets.get(*next) else {
                frames.pop();
                continue;
            };
            *next += 1;
            if !keys.contains_key(id) {
                let key = key_of(fs, import_path, dir.as_str(), &u.imports[*k], id)?;
                keys.insert(id.clone(), key.clone());
                let dep = by_id.get(id.as_str()).ok_or_else(|| {
                    UnitKeyError::Bug("KIR の単位のキー: 単位がありません".into())
                })?;
                frames.push((dep, key_dir(&key), 0));
            }
        }
    }
    let mut result = Vec::new();
    let mut owner: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for u in &loaded.deps {
        let k = keys
            .get(&u.id)
            .cloned()
            .ok_or_else(|| UnitKeyError::Bug("KIR の単位のキー: キーがありません".into()))?;
        if let Some(d0) = owner.get(&k) {
            return Err(UnitKeyError::Clash(format!(
                "KIR の単位のキーが重なります: {k}({d0} と {})",
                u.display
            )));
        }
        owner.insert(k.clone(), u.display.clone());
        result.push((k, u.display.clone()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname_は_ocaml_と同じ() {
        for (a, b) in [
            ("", "."),
            ("a", "."),
            ("a/b", "a"),
            ("a/b/", "a"),
            ("a//b", "a"),
            ("/a", "/"),
            ("/", "/"),
            ("//", "/"),
            ("./a", "."),
            ("x/./y", "x/."),
        ] {
            assert_eq!(dirname(a), b, "{a}");
        }
        assert_eq!(concat(".", "./x.kel"), "././x.kel");
        assert_eq!(concat("a/", "x"), "a/x");
        assert_eq!(concat("", "x"), "x");
    }

    #[test]
    fn 表示の正規化() {
        assert_eq!(normalize_display("./x/../y.kel"), "y.kel");
        assert_eq!(normalize_display("../a/./b"), "../a/b");
        assert_eq!(normalize_display("/a/../../b"), "/../b");
        assert_eq!(normalize_display("up/lib/../none.kel"), "up/none.kel");
        assert_eq!(key_normalize("@1/../x.kel"), "@1/../x.kel");
        assert_eq!(key_normalize("a/../../x.kel"), "../x.kel");
    }
}
