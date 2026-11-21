//! 評価が止まる理由と終了コード(IRSpec 11.1 節と付録 B)。

use crate::out::WriteError;

/// 評価が止まる理由。
///
/// 実行時エラー、`depth_limit`、メモリ不足、出力エラー、未処理の操作は、バックエンドが状態ワード `ctx.pending` の
/// `ERROR(kind, メッセージ)` に変えて巻き戻す。
/// 未実装と内部エラーは、巻き戻さずに実行を終了する。
/// 防壁(`cancel` 節の実行中に、伝播中の状態を退避して守る手順)が行う flush の失敗も、巻き戻さずに実行を終了する。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// 実行時エラー(IRSpec 付録 B)。持つ値は `実行時エラー: ` の後ろに書くバイト列である。
    Runtime(Vec<u8>),
    /// 論理的な深さが上限 `L` を超えた。持つ値は `L` である。
    DepthLimit(u64),
    /// メモリ不足(IRSpec 付録 B の 19 番)。
    OutOfMemory,
    /// 標準出力に書けない。終了コードは 74 である。
    Io(String),
    /// 標準出力に書けず、巻き戻さずに終了する。終了コードは 74 である。
    ///
    /// 防壁が、抑制した例外を記録する前に行う flush の失敗である。
    /// Diktor の `flush_stdout_or_die` と同じく、すぐに終了する。
    /// メッセージは [`Stop::Io`] と同じである。
    IoAbort(String),
    /// どのハンドラも処理しないユーザの操作。終了コードは 3 である。持つ値は操作の名前である。
    Unhandled(String),
    /// 未実装の機能を実行しようとした。終了コードは 4 である。
    Unimplemented(String),
    /// 処理系の内部エラー。終了コードは 70 である。
    Internal(String),
    /// 状態ワード `ctx.pending` が設定されていて、巻き戻しを続ける。バックエンドの中だけで使う。
    Pending,
}

impl From<WriteError> for Stop {
    fn from(e: WriteError) -> Stop {
        Stop::Io(e.0)
    }
}

impl From<crate::heap::OutOfMemory> for Stop {
    fn from(_: crate::heap::OutOfMemory) -> Stop {
        Stop::OutOfMemory
    }
}

impl From<keleut_rtcore::OutOfMemory> for Stop {
    fn from(_: keleut_rtcore::OutOfMemory) -> Stop {
        Stop::OutOfMemory
    }
}

impl Stop {
    /// 実行時エラーを作る。
    pub fn runtime(msg: impl Into<Vec<u8>>) -> Stop {
        Stop::Runtime(msg.into())
    }

    /// この理由で終了するときの終了コードを返す。
    pub fn exit_code(&self) -> u8 {
        match self {
            Stop::Runtime(_) | Stop::DepthLimit(_) | Stop::OutOfMemory | Stop::Unhandled(_) => 3,
            Stop::Io(_) | Stop::IoAbort(_) => 74,
            Stop::Unimplemented(_) => 4,
            Stop::Internal(_) | Stop::Pending => 70,
        }
    }

    /// 巻き戻さずに終了するエラー(未実装、内部エラー、防壁の flush の失敗)なら真を返す。
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Stop::Unimplemented(_) | Stop::Internal(_) | Stop::IoAbort(_)
        )
    }

    /// 標準エラーに書く診断を返す。末尾の改行を含む。
    pub fn message(&self) -> Vec<u8> {
        let mut m = Vec::new();
        match self {
            Stop::Runtime(s) => {
                m.extend_from_slice("実行時エラー: ".as_bytes());
                m.extend_from_slice(s);
            }
            Stop::DepthLimit(l) => m.extend_from_slice(
                format!("実行時エラー: 論理的な深さが限度 {l} を超えました").as_bytes(),
            ),
            Stop::OutOfMemory => m.extend_from_slice("実行時エラー: メモリ不足です".as_bytes()),
            Stop::Io(s) | Stop::IoAbort(s) => {
                m.extend_from_slice(format!("keleut: 標準出力に書き出せません: {s}").as_bytes())
            }
            Stop::Unimplemented(s) => {
                m.extend_from_slice(format!("keleut: 未実装: {s}").as_bytes())
            }
            Stop::Unhandled(s) => {
                m.extend_from_slice(format!("未処理のエフェクト操作: {s}").as_bytes())
            }
            Stop::Internal(s) => m.extend_from_slice(format!("keleut: 内部エラー: {s}").as_bytes()),
            Stop::Pending => {
                m.extend_from_slice("keleut: 内部エラー: 状態語が最上位で残りました".as_bytes())
            }
        }
        m.push(b'\n');
        m
    }
}

/// IRSpec 付録 B の 6 番(初期化の前のインスタンスからメソッドを選んだ)の実行時エラーを作る。
///
/// `display` は、メッセージの先頭に書くインスタンスの表示(`<C>[<T>]`)である。
pub fn instance_not_ready(display: &str) -> Stop {
    Stop::runtime(format!(
        "{display} のインスタンスは宣言より前の位置では使えません(実行はまだ実体を持ちません。インスタンス宣言を使用より前に置いてください)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文面と終了状態() {
        assert_eq!(
            String::from_utf8(Stop::runtime("ゼロ除算です").message()).unwrap(),
            "実行時エラー: ゼロ除算です\n"
        );
        assert_eq!(
            String::from_utf8(Stop::DepthLimit(10).message()).unwrap(),
            "実行時エラー: 論理的な深さが限度 10 を超えました\n"
        );
        assert_eq!(Stop::OutOfMemory.exit_code(), 3);
        assert_eq!(Stop::Io(String::new()).exit_code(), 74);
        assert_eq!(Stop::IoAbort(String::new()).exit_code(), 74);
        assert!(Stop::IoAbort(String::new()).is_fatal());
        assert_eq!(Stop::Unimplemented(String::new()).exit_code(), 4);
        assert_eq!(Stop::Internal(String::new()).exit_code(), 70);
    }
}
