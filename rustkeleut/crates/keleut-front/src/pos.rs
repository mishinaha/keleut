//! ソース上の位置と、ファイル名の表を定義する。
//!
//! 位置は Diktor が使う `Lexing.position` と同じ 4 つの値を持つ。
//! 行番号は 1 始まりである。
//! 桁の計算に使う `cnum`(ファイルの先頭からの位置)と `bol`(行頭の位置)は、バイト数ではなくコードポイント数で数える。
//! sedlex の UTF-8 の字句解析器と同じ数え方である。
//! 位置は `ファイル:行:桁` の形式で表示し、桁は `cnum - bol + 1` である。

/// ファイル名の表([`Files`])の添字である。
///
/// 0 は名前が空のファイルを表す(Diktor の構文解析の開始位置 `Lexing.from_string ""` のファイル名に相当する)。
pub type FileId = u32;

/// ソース上の 1 点を表す位置である(Diktor の `Lexing.position`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    /// ファイル名の表の添字。
    pub file: FileId,
    /// 行番号(1 始まり)。
    pub line: u32,
    /// 行頭の、ファイルの先頭からの位置(コードポイント数)。
    pub bol: u32,
    /// ファイルの先頭からの位置(コードポイント数)。
    pub cnum: u32,
}

impl Pos {
    /// 名前が空のファイルの、1 行目の先頭を指す位置である。
    pub const START: Pos = Pos {
        file: 0,
        line: 1,
        bol: 0,
        cnum: 0,
    };

    /// 桁(1 始まり)を返す。
    pub fn col(self) -> u32 {
        self.cnum - self.bol + 1
    }
}

/// ノードがソース上で占める範囲である(Diktor の `Location.span`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// 範囲の始まりの位置。
    pub start: Pos,
    /// 範囲の終わりの位置。
    pub finish: Pos,
}

/// ファイル名の表である。
/// 添字([`FileId`])からファイル名を引く。
#[derive(Clone, Debug)]
pub struct Files {
    names: Vec<String>,
}

impl Default for Files {
    fn default() -> Files {
        Files {
            names: vec![String::new()],
        }
    }
}

impl Files {
    /// 名前が空のファイル(添字 0)だけを持つ表を作る。
    pub fn new() -> Files {
        Files::default()
    }

    /// ファイル名を追加し、その添字を返す。
    pub fn add(&mut self, name: &str) -> FileId {
        self.names.push(name.to_string());
        (self.names.len() - 1) as FileId
    }

    /// 添字 `id` のファイル名を返す。
    pub fn name(&self, id: FileId) -> &str {
        &self.names[id as usize]
    }

    /// 位置を `ファイル:行:桁` の形式の文字列にする(Diktor の `Location.show_pos`)。
    pub fn show(&self, p: Pos) -> String {
        format!("{}:{}:{}", self.name(p.file), p.line, p.col())
    }
}
