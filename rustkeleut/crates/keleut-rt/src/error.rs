//! 評価が止まる理由と終了状態(計画 261024-1 の 6.2 節、IRSpec 11.1 節と付録 B)。

use crate::out::WriteError;

/// 評価が止まる理由。
///
/// M87 にはハンドラが無いので、どれも最上位へそのまま届く。
/// M89 は、これを状態語 `ctx.pending` の `ERROR(kind, 文面)` にして巻き戻す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// 実行時エラー(付録 B)。文面は `実行時エラー: ` の後ろに書くバイト列である。
    Runtime(Vec<u8>),
    /// 論理的な深さが限度 `L` を超えた。
    DepthLimit(u64),
    /// メモリの不足(付録 B の 19 番)。
    OutOfMemory,
    /// 標準出力に書けない(74)。
    Io(String),
    /// 未実装の機能に届いた(4)。
    Unimplemented(String),
    /// 処理系の内部の誤り(70)。
    Internal(String),
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

impl Stop {
    /// 実行時エラーを作る。
    pub fn runtime(msg: impl Into<Vec<u8>>) -> Stop {
        Stop::Runtime(msg.into())
    }

    /// 終了状態。
    pub fn exit_code(&self) -> u8 {
        match self {
            Stop::Runtime(_) | Stop::DepthLimit(_) | Stop::OutOfMemory => 3,
            Stop::Io(_) => 74,
            Stop::Unimplemented(_) => 4,
            Stop::Internal(_) => 70,
        }
    }

    /// 標準エラーに書く診断(改行を含む)。
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
            Stop::Io(s) => {
                m.extend_from_slice(format!("keleut: 標準出力に書き出せません: {s}").as_bytes())
            }
            Stop::Unimplemented(s) => {
                m.extend_from_slice(format!("keleut: 未実装: {s}").as_bytes())
            }
            Stop::Internal(s) => m.extend_from_slice(format!("keleut: 内部エラー: {s}").as_bytes()),
        }
        m.push(b'\n');
        m
    }
}

/// 付録 B の 6 番の文面。
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
        assert_eq!(Stop::Unimplemented(String::new()).exit_code(), 4);
        assert_eq!(Stop::Internal(String::new()).exit_code(), 70);
    }
}
