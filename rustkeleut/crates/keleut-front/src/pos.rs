//! 位置と、ファイル名の表である。
//!
//! 位置は Diktor が使う `Lexing.position` と同じ 4 つの値を持つ。
//! 行は 1 から数え、桁の材料の `cnum`(ファイルの先頭からの位置)と `bol`(行頭の位置)は、
//! バイトでなくコードポイントで数える(sedlex の UTF-8 の字句解析器と同じ)。
//! 表示は `ファイル:行:桁` で、桁は `cnum - bol + 1` である。

/// ファイル名の表の添字である。
///
/// 0 は名前の空のファイル(Diktor の構文解析の開始位置 `Lexing.from_string ""` のもの)を表す。
pub type FileId = u32;

/// 位置である。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    /// ファイル。
    pub file: FileId,
    /// 行(1 始まり)。
    pub line: u32,
    /// 行頭の位置(コードポイント)。
    pub bol: u32,
    /// ファイルの先頭からの位置(コードポイント)。
    pub cnum: u32,
}

impl Pos {
    /// 名前の空のファイルの 1 行目の先頭である。
    pub const START: Pos = Pos {
        file: 0,
        line: 1,
        bol: 0,
        cnum: 0,
    };

    /// 桁(1 始まり)。
    pub fn col(self) -> u32 {
        self.cnum - self.bol + 1
    }
}

/// ノードの範囲である(Diktor の `Location.span`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// 始まり。
    pub start: Pos,
    /// 終わり。
    pub finish: Pos,
}

/// ファイル名の表である。
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
    /// 名前の空のファイルだけを持つ表。
    pub fn new() -> Files {
        Files::default()
    }

    /// ファイルを足して添字を返す。
    pub fn add(&mut self, name: &str) -> FileId {
        self.names.push(name.to_string());
        (self.names.len() - 1) as FileId
    }

    /// 添字の名前。
    pub fn name(&self, id: FileId) -> &str {
        &self.names[id as usize]
    }

    /// `ファイル:行:桁` の形の表示(Diktor の `Location.show_pos`)。
    pub fn show(&self, p: Pos) -> String {
        format!("{}:{}:{}", self.name(p.file), p.line, p.col())
    }
}
