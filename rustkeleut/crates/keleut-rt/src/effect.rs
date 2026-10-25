//! エフェクトの実行時のデータ(計画 261024-1 の 6.6 節、`doc/lir.md` の 16 節)。
//!
//! | 名前 | 中身 | 語 |
//! | --- | --- | --- |
//! | ハンドラ記録 [`HandlerRec`] | 起動ごとの番号、エフェクト、節の表、`return` 節と `cancel` 節、親、深さ `h` | 箱 [`crate::value::Kind::Handler`] |
//! | 継続 [`Cont`] | 写したフレームの列、ハンドラ記録、連鎖の切り取りの先頭、選んだ節と引数、`Δ`、`used`、`alive` | 箱 [`crate::value::Kind::Cont`] |
//! | 状態語 [`Pending`] | `NONE`、`YIELD`、`DISCARD`、`ERROR` と、節とその呼び手の間だけで使う印 | 退避した値は箱 [`crate::value::Kind::Pending`] |
//!
//! 3 つの箱は 1 番の語に表の添字を持ち、中身は [`crate::Rt`] の表にある。
//! 表は根ではない。
//! 回収([`crate::gc`])は、添字の箱に印を付けたときにその項目の語を走査し、印付けの後に、箱に印の付かなかった項目を表から外す。
//! 外した項目は語を持たない値に置き換え、その添字を次に表に入れる項目に使い回す。
//! 項目どうしの参照の輪(付け替えた親、`res` の継続、その継続のハンドラ記録)も、こうすれば外れる。
//! 項目は表に入れてから添字の箱を確保し、まだ箱の無い項目は箱ができるまで根である。
//! 使い終えた継続の語(ハンドラ記録、連鎖の切り取りの先頭、引数)は消さない。
//! 継続の箱は、継続を受け取った節のフレームが終われば届かなくなり、項目ごと外れるからである。
//! 継続の中のフレームの写しは、値スタックの生きたフレームと同じく、見出しの再開点の集合のスロットだけを走査する
//! (`doc/lir.md` の 1.8 節)。
//! 写しの残りのスロットには、死んだ値やまだ書いていない値が残りうる。
//!
//! 継続のフレームの深さの語は、継続を取り出したハンドラ記録の深さ `h` からの差で持つ。
//! 写し戻すときは、写し戻す先のハンドラのフレームの深さを足す(IRSpec 11.4 節)。
//! 継続の中の、まだ再開していない継続のフレームも、それを取り出したハンドラ記録の `h` からの差で持つので、
//! 外側の継続を写し戻してそのハンドラ記録の `h` がずれると、中の継続も一緒にずれる。

use crate::error::Stop;
use crate::value::Word;
use keleut_lir::ClauseClass;

/// ハンドラ記録。
#[derive(Clone, Debug)]
pub struct HandlerRec {
    /// 起動の番号(64 ビットの単調に増える番号。IRSpec 9.3 節)。
    pub id: u64,
    /// エフェクト。
    pub effect: u32,
    /// 本体の関数の値。
    pub body: Word,
    /// 本体に渡す、囲む操作節の継続(無ければ Unit)。
    pub res: Word,
    /// 操作節(書いた順の(操作, 分類, 関数の値))。
    pub ops: Vec<(u32, ClauseClass, Word)>,
    /// `return` 節。
    pub ret: Option<Word>,
    /// `cancel` 節。
    pub cancel: Option<Word>,
    /// 親(連鎖の次。無ければ Unit)。
    pub parent: Word,
    /// 深さ `h`(ハンドラのフレームの深さ)。
    pub h: u64,
}

impl HandlerRec {
    /// 回収した項目の置き換え(語を持たない)。
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

/// 継続。
#[derive(Clone, Debug, Default)]
pub struct Cont {
    /// 取り出したハンドラ記録。
    pub hr: Word,
    /// 写したフレームの語(最下段のフレームから順に並べる)。各フレームの 1 番の語は `h` からの差である。
    pub frames: Vec<u64>,
    /// 各フレームの語の数(最下段から順)。
    pub sizes: Vec<u32>,
    /// `Δ = p − h`。
    pub delta: u64,
    /// 連鎖の切り取りの先頭(`perform` の時点の連鎖。無ければ Unit)。
    pub cut: Word,
    /// 選んだ G の節の候補の番号。
    pub clause: u32,
    /// 操作の引数(`Array` の箱)。
    pub args: Word,
    /// 再開済みか。
    pub used: bool,
    /// 節の評価の中か(節が戻ったら偽にする)。
    pub alive: bool,
}

/// 状態語 `ctx.pending`。
#[derive(Clone, Debug, Default)]
pub enum Pending {
    /// 何も立っていない。
    #[default]
    None,
    /// `YIELD(hr, 節, 引数)`。`cut` は連鎖の切り取りの先頭、`top` は `perform` のフレームの位置である。
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
    /// `ERROR(kind, 文面)`。`Stop` は実行時エラー、`depth_limit`、メモリの不足、出力の失敗、未処理の操作のどれかである。
    Error(Stop),
    /// N か G の節が選ばれた印(節から `perform` のフレームへ)。
    Matched(u32),
    /// T の節が `resume` せずに値になった印(節から `perform` のフレームへ)。
    ClauseValue(Word),
    /// G の節の末尾の `resume(v)` の印(節からハンドラのフレームへ)。
    TailResume(Word),
}

impl Pending {
    /// 立っているか。
    pub fn is_some(&self) -> bool {
        !matches!(self, Pending::None)
    }

    /// `YIELD` か。
    pub fn is_yield(&self) -> bool {
        matches!(self, Pending::Yield { .. })
    }

    /// 宛てのハンドラ記録(`YIELD` と `DISCARD`)。
    pub fn target(&self) -> Option<Word> {
        match self {
            Pending::Yield { hr, .. }
            | Pending::DiscardValue { hr, .. }
            | Pending::DiscardNever { hr, .. } => Some(*hr),
            _ => None,
        }
    }
}

/// 付録 B の 9 番の文面の `<操作><値>` の `<値>`(引数の組の綴り)。
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

/// `cancel` 節の中で打ち切った脱出を記録する文面(`cancel 節で例外が抑制されました: ` の後ろ)。
///
/// 文面は Diktor の KIR の評価器(`ir_eval.ml` の `run_cancel`)と同じにする。
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
