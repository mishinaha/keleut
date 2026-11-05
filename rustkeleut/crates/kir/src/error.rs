//! 誤りの型。

use std::fmt;

/// 誤りの種類。
///
/// 終了状態は IRSpec 13.8 節、13.9 節と、計画の 6.2 節の表に従う([`Error::exit_code`])。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// バイナリの形式の誤り(IRSpec 13.7 節)。
    Malformed,
    /// テキストの字句と構文の誤り、名前の解決の失敗(IRSpec 12 節)。
    Syntax,
    /// 読み込みか検証で、入れ子が限度 `N_nest`(スタックに合わせて下げた限度を含む)を超えた(IRSpec 13.9 節)。
    /// 最も小さいスタックでも処理のスレッドを作れないときも、処理系の限度としてこれにする。
    Limit,
    /// 検証の失敗(IRSpec 13.8 節)。
    Invalid,
    /// 書き出しで、入れ子が限度 `N_nest` を超えた(IRSpec 13.9 節)。
    WriteLimit,
    /// 書き出す対象の不整合(表の範囲の外の添字など)や、処理の途中の内部の誤り。
    Internal,
    /// 出力先に書けない(ディスクが尽きたときを含む。IRSpec 12.3 節の書き手の出力の誤り)。
    Output,
}

/// この crate のすべての誤り。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    /// 種類と文面から作る。
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Error {
        Error {
            kind,
            message: message.into(),
        }
    }

    /// 種類。
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// 文面。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// この誤りで終わるときの終了状態。
    ///
    /// 読み込みと検証の失敗は 65、書き出しの入れ子の限度は 1(IRSpec 13.9 節)、内部の誤りは 70、
    /// 出力先に書けないときは 74 である。
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::Malformed | ErrorKind::Syntax | ErrorKind::Limit | ErrorKind::Invalid => 65,
            ErrorKind::WriteLimit => 1,
            ErrorKind::Internal => 70,
            ErrorKind::Output => 74,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// 読み込みで入れ子が限度を超えたときの診断(IRSpec 13.9 節)。
pub const READ_LIMIT_MESSAGE: &str = "処理系の限度: KIR の読み込みで入れ子が深すぎます";

/// 検証で入れ子が限度を超えたときの診断(IRSpec 13.9 節)。
pub const VERIFY_LIMIT_MESSAGE: &str = "処理系の限度: KIR の検証で入れ子が深すぎます";

/// 書き出しで入れ子が限度を超えたときの診断(IRSpec 13.9 節)。
pub const WRITE_LIMIT_MESSAGE: &str = "処理系の限度: KIR の書き出しで入れ子が深すぎます";
