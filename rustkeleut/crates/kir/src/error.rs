//! この crate が返すエラーの型。

use std::fmt;

/// エラーの種類。
///
/// 種類ごとの終了コードは [`Error::exit_code`] が返す。
/// 読み込みと検証が失敗したときの終了コードは IRSpec 13.8 節と 13.9 節が、書き出しで入れ子が上限を超えたときの終了コードは 13.9 節が定める。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// バイナリの形式のエラー(IRSpec 13.7 節)。
    Malformed,
    /// テキストの字句エラーと構文エラー、名前の解決の失敗(IRSpec 12 節)。
    Syntax,
    /// 読み込みか検証で、入れ子が上限 `N_nest` を超えたときのエラー(IRSpec 13.9 節)。
    /// スタックに合わせて上限を下げたときは、下げた後の上限を超えた入力がこの種類になる。
    /// 最も小さいスタックでも処理用のスレッドを作れないときも、同じ `処理系の限度` の診断を持つこの種類のエラーを返す。
    Limit,
    /// 検証の失敗(IRSpec 13.8 節)。
    Invalid,
    /// 書き出しで、入れ子が上限 `N_nest` を超えたときのエラー(IRSpec 13.9 節)。
    WriteLimit,
    /// 書き出すプログラムの不整合(表の範囲外の添字など)や、処理の途中の内部エラー。
    Internal,
    /// 出力先に書けないときのエラー。
    /// ディスクの空きがなくなったときを含む(IRSpec 12.3 節が定める、ライタの出力エラー)。
    Output,
}

/// この crate の関数が返すエラー。種類とメッセージを持つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    /// 種類とメッセージからエラーを作る。
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Error {
        Error {
            kind,
            message: message.into(),
        }
    }

    /// エラーの種類を返す。
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// エラーメッセージを返す。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// このエラーで終了するときの終了コードを返す。
    ///
    /// 読み込みと検証の失敗は 65、書き出しで入れ子が上限を超えたときは 1(IRSpec 13.9 節)、内部エラーは 70、出力先に書けないときは 74 である。
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

/// 読み込みで入れ子が上限を超えたときの診断(IRSpec 13.9 節)。
pub const READ_LIMIT_MESSAGE: &str = "処理系の限度: KIR の読み込みで入れ子が深すぎます";

/// 検証で入れ子が上限を超えたときの診断(IRSpec 13.9 節)。
pub const VERIFY_LIMIT_MESSAGE: &str = "処理系の限度: KIR の検証で入れ子が深すぎます";

/// 書き出しで入れ子が上限を超えたときの診断(IRSpec 13.9 節)。
pub const WRITE_LIMIT_MESSAGE: &str = "処理系の限度: KIR の書き出しで入れ子が深すぎます";
