//! エフェクトの実行時のデータ(`doc/lir.md` の 16 節)。
//!
//! | 名前 | 内容 | 値の表現 |
//! | --- | --- | --- |
//! | ハンドラ記録 [`HandlerRec`] | 起動ごとの番号、エフェクト、節の表、`return` 節と `cancel` 節、親、深さ `h` | オブジェクト [`crate::value::Kind::Handler`] |
//! | 継続 [`Cont`] | コピーしたフレームの並び、ハンドラ記録、連鎖の切り出した部分の先頭、選んだ節と引数、`Δ`、`used`、`alive` | オブジェクト [`crate::value::Kind::Cont`] |
//! | 状態ワード [`Pending`] | `NONE`、`YIELD`、`DISCARD`、`ERROR` と、節とその呼び出し元の間だけで使う目印 | 退避した値はオブジェクト [`crate::value::Kind::Pending`] |
//!
//! 3 種類のオブジェクトは 1 番のワードに表の添字を持ち、内容は [`crate::Rt`] の表にある。
//! 表は GC ルートではない。
//! GC([`crate::gc`])は、添字を持つオブジェクトをマークしたときにその項目のワードを走査し、マークの後に、オブジェクトがマークされなかった項目を表から外す。
//! 外した項目はオブジェクトを参照しない空の値に置き換え、その添字を次に表に入れる項目に再利用する。
//! こうすれば、項目どうしの循環参照(差し替えた親、`res` の継続、その継続のハンドラ記録)も表から外れる。
//! 項目は、表に入れてから添字を持つオブジェクトを確保する。
//! まだオブジェクトのない項目は、オブジェクトができるまで GC ルートである。
//! 使い終えた継続のワード(ハンドラ記録、連鎖の切り出した部分の先頭、引数)は消さない。
//! 継続のオブジェクトは、継続を受け取った節のフレームが終われば到達できなくなり、項目ごと表から外れるからである。
//! 継続が持つフレームのコピーは、値スタックの生きているフレームと同じく、ヘッダの再開点の集合のスロットだけを走査する
//! (`doc/lir.md` の 1.8 節)。
//! コピーの残りのスロットには、すでに死んだ値や、まだ書いていない値が残っていることがある。
//!
//! 継続が持つフレームの深さのワードは、継続をキャプチャしたハンドラ記録の深さ `h` からの差で持つ。
//! 値スタックに復元するときは、復元先のハンドラのフレームの深さを足す(IRSpec 11.4 節)。
//! 継続の中にある、まだ再開していない継続のフレームも、それをキャプチャしたハンドラ記録の `h` からの差で持つ。
//! そのため、外側の継続を復元してそのハンドラ記録の `h` がずれると、中の継続も一緒にずれる。

use crate::error::Stop;
use crate::value::Word;
use keleut_lir::ClauseClass;

/// ハンドラ記録。ハンドラの起動 1 回につき 1 つ作る。
#[derive(Clone, Debug)]
pub struct HandlerRec {
    /// 起動の番号。64 ビットの単調に増える番号である(IRSpec 9.3 節)。
    pub id: u64,
    /// ハンドラが処理するエフェクトの番号。
    pub effect: u32,
    /// 本体の関数の値。
    pub body: Word,
    /// 本体に渡す、外側の操作節の継続。なければ Unit である。
    pub res: Word,
    /// 操作節。(操作, 分類, 関数の値)の組を、書いた順に並べる。分類は N、T、G のどれかである(IRSpec 9.2 節)。
    pub ops: Vec<(u32, ClauseClass, Word)>,
    /// `return` 節の関数の値。節がなければ `None` である。
    pub ret: Option<Word>,
    /// `cancel` 節の関数の値。節がなければ `None` である。
    pub cancel: Option<Word>,
    /// 親。ハンドラの連鎖の次のハンドラ記録で、なければ Unit である。
    pub parent: Word,
    /// 深さ `h`。ハンドラのフレームの深さである。
    pub h: u64,
}

impl HandlerRec {
    /// GC が表から外した項目に置く、オブジェクトを参照しない空の値を返す。
    pub fn dead() -> HandlerRec {
        HandlerRec {
            id: 0,
            effect: 0,
            body: Word::UNIT,
            res: Word::UNIT,
            ops: Vec::new(),
            ret: None,
            cancel: None,
            parent: Word::UNIT,
            h: 0,
        }
    }
}

/// 継続。キャプチャしたフレームのコピーと、再開に使う情報を持つ。
#[derive(Clone, Debug, Default)]
pub struct Cont {
    /// 継続をキャプチャしたハンドラ記録。
    pub hr: Word,
    /// コピーしたフレームのワード。最下段のフレームから順に並べる。各フレームの 1 番のワードは `h` からの差である。
    pub frames: Vec<u64>,
    /// 各フレームのワード数。最下段から順に並べる。
    pub sizes: Vec<u32>,
    /// `Δ = p − h`。
    pub delta: u64,
    /// 連鎖の切り出した部分の先頭。`perform` の時点の連鎖で、なければ Unit である。
    pub cut: Word,
    /// 選んだ操作節(分類 G)の候補の番号。ハンドラ記録の操作節の表での位置である。
    pub clause: u32,
    /// 操作の引数。`Array` のオブジェクトである。
    pub args: Word,
    /// 再開済みなら真。
    pub used: bool,
    /// 節を評価している間は真。節が戻ったら偽にする。
    pub alive: bool,
}

/// 状態ワード `ctx.pending`。
#[derive(Clone, Debug, Default)]
pub enum Pending {
    /// 何も設定されていない。
    #[default]
    None,
    /// `YIELD(hr, 節, 引数)`。`cut` は連鎖の切り出した部分の先頭、`top` は `perform` のフレームの位置である。
    Yield {
        hr: Word,
        clause: u32,
        args: Word,
        cut: Word,
        top: u64,
    },
    /// `DISCARD(hr, Value v)`。
    DiscardValue { hr: Word, v: Word },
    /// `DISCARD(hr, NeverBody(節, 引数))`。
    DiscardNever { hr: Word, clause: u32, args: Word },
    /// `ERROR(kind, メッセージ)`。`Stop` は実行時エラー、`depth_limit`、メモリ不足、出力エラー、未処理の操作のどれかである。
    Error(Stop),
    /// 分類 N か G の操作節が選ばれたことを、節から `perform` のフレームへ伝える。
    Matched(u32),
    /// 分類 T の操作節が `resume` せずに値になったことを、節から `perform` のフレームへ伝える。
    ClauseValue(Word),
    /// 分類 G の操作節が末尾 `resume(v)` を行ったことを、節からハンドラのフレームへ伝える。
    TailResume(Word),
}

impl Pending {
    /// 状態ワードが設定されていれば真を返す。
    pub fn is_some(&self) -> bool {
        !matches!(self, Pending::None)
    }

    /// 状態ワードが `YIELD` なら真を返す。
    pub fn is_yield(&self) -> bool {
        matches!(self, Pending::Yield { .. })
    }

    /// 宛先のハンドラ記録を返す。宛先を持つのは `YIELD` と `DISCARD` だけである。
    pub fn target(&self) -> Option<Word> {
        match self {
            Pending::Yield { hr, .. }
            | Pending::DiscardValue { hr, .. }
            | Pending::DiscardNever { hr, .. } => Some(*hr),
            _ => None,
        }
    }
}

/// IRSpec 付録 B の 9 番のメッセージ `handle のどの節にも一致しません: <操作><値>` の `<値>` を作る。
///
/// `<値>` は、引数を組として文字列化したものである。引数が 1 個のときは `(v,)` と書く。
/// 呼び出し元は各引数の文字列 `shown` を [`crate::ctx::Rt::show`] で作るので、レコードのフィールドの順はそれに従う。
pub fn args_text(shown: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"(".to_vec();
    for (i, s) in shown.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b", ");
        }
        out.extend_from_slice(s);
    }
    if shown.len() == 1 {
        out.push(b',');
    }
    out.push(b')');
    out
}

/// `cancel` 節の中で抑制した脱出を記録するメッセージを作る。`cancel 節で例外が抑制されました: ` の後ろに続く部分である。
///
/// メッセージは Diktor の KIR の評価器(`ir_eval.ml` の `run_cancel`)と同じにする。
pub fn suppressed_text(p: &Pending) -> Vec<u8> {
    match p {
        Pending::Error(Stop::Runtime(m)) => m.clone(),
        Pending::Error(Stop::DepthLimit(l)) => {
            format!("論理的な深さが限度 {l} を超えました").into_bytes()
        }
        Pending::Error(Stop::OutOfMemory) => "メモリ不足です".as_bytes().to_vec(),
        Pending::Error(Stop::Io(m)) => format!("標準出力に書き出せません: {m}").into_bytes(),
        Pending::Error(Stop::Unhandled(n)) => format!("未処理のエフェクト操作: {n}").into_bytes(),
        Pending::Error(s) => s.message().strip_suffix(b"\n").unwrap_or(&[]).to_vec(),
        _ => "cancel 節から操作の巻き戻しで脱出しようとしました"
            .as_bytes()
            .to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 引数の組の綴り() {
        assert_eq!(args_text(&[]), b"()");
        assert_eq!(args_text(&[b"1".to_vec()]), b"(1,)");
        assert_eq!(
            args_text(&[b"1".to_vec(), b"\"a\"".to_vec()]),
            b"(1, \"a\")"
        );
    }
}
