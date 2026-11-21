//! LIR の関数から Wasm の関数を生成する。
//!
//! ## 関数の型
//!
//! 引数が N 個の LIR の関数の型は `$FnN = (env (ref null $Clo)) (eqref × N) (frame (ref null $Frame)) → eqref` である。
//! `env` は自分を呼び出したクロージャで、直接呼び出しでは null である。
//! `frame` は再開用エントリが渡すフレームの記録で、通常の呼び出しでは null である。
//! N が [`crate::P_DIRECT`] を超える関数は、`eqref × N` の代わりに、先頭の `P_DIRECT` 個と、残りをまとめた配列 `(ref $anyarr)` を受け取る。
//! 呼び出し元は、引数を積んだ後で、残りを `array.new_fixed` で配列にまとめる。
//! 呼び出し先は、関数の入口で配列を読み、残りの引数を通常のレジスタに置く。
//! 関数は、状態ワードが設定されていれば null(番兵)を返す。
//!
//! ## ブロック
//!
//! LIR のブロックはトポロジカル順に並び、どの分岐も後ろのブロックへ向かう。
//! そこで、ブロック 1〜n-1 の入口を、入れ子の `block` として先に開く。
//!
//! ```text
//! block $B(n-1) … block $B1
//!   B0 の命令
//! end
//! B1 の命令
//! …
//! B(n-1) の命令
//! ```
//!
//! `$Bk` の `end` の直後が Bk の命令なので、後ろのブロックへの分岐は前向きの `br` になる。
//! 次のブロックへの `jump` には命令を書かず、そのまま次へ進む。
//! 状態ワードを伝播する箇所は、1 つの出口へ分岐せず、その場で深さを書き戻して null を返す。
//!
//! ## 重い関数の分割
//!
//! 命令の多い関数と、フィールドの多い大きなフレーム方式の関数(下の「ローカル」)は、いくつかの Wasm の関数に分ける。
//! 分けてできた Wasm の関数を、以下では分割関数と呼ぶ。
//! 命令の多い関数を分けるのは、V8 の最適化コンパイルが使うメモリが、1 つの関数のサイズに対して線形より速く増えるからである。
//! 何度も呼び出される大きな関数では、V8 がメモリを使い尽くす。
//! 大きなフレーム方式の関数を小さな分割関数に分けるのは、1 段が使うスタックを抑えるためである([`piece_max_big`])。
//! 分割する関数は大きなフレーム方式にし、すべてのレジスタを構造体に置く。
//! LIR のブロックは、重み(命令と終端命令の個数に、それらが読むレジスタの個数を足した値)に応じてさらに区切り(ブロックを切った断片)に分け、区切りを順に分割関数へまとめる。
//! 分割関数は、引数で受け取った番号の区切りへ `br_table` で分岐する。
//! ほかの分割関数にある区切りへ分岐するときは、その分割関数を末尾呼び出しで呼ぶ([`generate`])。
//!
//! ## 継続のキャプチャと再開
//!
//! yield を受け取る再開点の呼び出しは、前半(結果を積むまで)と後半(結果を受け取る部分)の間で区切る。
//! yield を受け取る再開点は、パス 11 が `yield` の印を付けた再開点で、`perform` と、番兵を受け取る呼び出しを含む。
//! 後半の区切りは入れ子の `block` の境界から始まるので、関数の先頭の `br_table` から前向きに分岐できる。
//!
//! - 継続のキャプチャ: 呼び出しが番兵を返し、状態ワードが `YIELD` なら、後半は、ローカルと深さと再開する区切りを、
//!   関数ごとのフレームの記録(`$Frame` の部分型)に入れる。
//!   記録に入れるローカルは、分割しない関数では引数とスロットのローカルで、大きなフレーム方式では構造体の参照である。
//!   後半は、その記録をグローバルの並び `yf` の末尾に追加して、番兵を返す。
//!   並びは、最上段(G の節を選んだ `perform`)から外側へ向かう順である。
//!   自分宛ての `YIELD` を受け取った `$rt_handle` の `cont_make` が、この並びを継続 `$Cont` にする。
//!   `perform` は、継続をキャプチャした後に再開する区切りを、後半とは別に持つ。
//!   その区切りでは、再開の値が `perform` の結果になる。
//!   `$rt_handle` の `propagate` は、最後に番兵を受け取った呼び出しの後半(`lastrp`)を、再開する区切りとして記録に書く。
//! - 再開: `rt_resume` は、記録の並びを最上段から順にたどり、各記録の再開用エントリ(`$ResFn`)を呼ぶ。
//!   再開用エントリは小さな関数で、関数ごとに作る。分割する関数では分割関数ごとに作る。
//!   分割しない関数の再開用エントリは、記録を最後の引数に渡して関数を呼ぶ。
//!   関数は、記録からローカルを復元して、記録にある区切りへ分岐する。
//!   分割する関数の再開用エントリは、記録から構造体の参照と区切りの番号を読み、それらを渡して分割関数を末尾呼び出しで呼ぶ。
//!   再開するフレームに渡す値(内側のフレームの結果か番兵)は、グローバル `$rv` に置く。
//!   再開したフレームがまた番兵を返せば、外側のフレームも番兵を受け取り、自分の記録を作り直す。
//!
//! yield を受け取る再開点のない関数は、再開点での区切りも再開用エントリも持たない。
//! G の節を持つハンドラがプログラムのどこにもなければ `YIELD` は設定されないので、どの関数も再開点で区切らない。
//! そのときの `perform` は、継続がキャプチャされたかどうかを調べずに伝播し、`YIELD` を受け取ったら内部エラーで終了する。
//!
//! ## ローカル
//!
//! レジスタは、パス 12 のスロットと Wasm の型の組ごとに 1 つのローカルを使う。
//! 2 つのレジスタが同じ組を使うのは、生存区間が重ならないときだけである。
//! 一時ローカルは Wasm の型ごとに 1 つで、使う型は 8 種類である。
//! これに深さのローカルと `lastrp` のローカルを合わせても、ローカルの個数は、スロットと型の組の個数に `TMP_MAX`(10)を足した数を超えない。
//! 1 段のバイト数の見積もり(`8 × K + 24 × P + 64`。K はローカルの個数、P は Wasm の引数の個数)が関数ごとの上限([`budget_of`])を超える関数は、
//! レジスタを、関数の入口で確保する構造体のフィールドに置く。
//! この方式を、大きなフレーム方式と呼ぶ。
//! フィールドが多ければ、関数を小さな分割関数に分ける([`piece_max_big`])。
//! 上限は通常 `B_wasm` である。
//! `$rt_handle` を非末尾呼び出しで呼ぶ関数と `cancel` 節では `B_wasm − E_rth − E_rres`、`$rt_handle` 自身では `E_rth` である。
//! まとめた配列から読む引数もこの構造体に置くので、引数の多い関数でも 1 段が上限に収まる。
//! 生成し終えた関数の見積もりが上限を超えたら、コード生成はエラーを返す。

use std::collections::HashMap;

use keleut_lir::{
    CmpOp, ConvOp, FloatOp, FuncId, FuncKind, FxOp, FxTermOp, HandlerPart, Inst, IntOp, IntTy,
    MathFn, Rep, RtError, ShowOp, Slot, Term,
};
use wasm_encoder::{BlockType, Function, HeapType, Ieee64, RefType, ValType};

use crate::emit::{EQ, F64, Fb, HEQ, HI31, I32, I64, ctf, hrf, imp, rv, svf};
use crate::meta::{self, arg, kind};
use crate::rt::{self, H};
use crate::{B_WASM, Cg, E_RRES, E_RTH, FKey, cls, frame_estimate, pk};

/// 1 回の `array.new_fixed` で作る要素の個数の上限。V8 の上限は 10,000 である。
const NEW_FIXED_MAX: usize = 1000;

#[derive(Clone, Copy)]
enum Loc {
    Local(u32),
    /// 大きなフレームの構造体のフィールド。値は、何番目の構造体かと、フィールドの番号である。
    Field(u32, u32),
}

/// 大きなフレームの構造体 1 つに置くフィールドの個数の上限。V8 の上限は 10,000 である。
const BIG_CHUNK: usize = 1000;

/// 関数を分けるときの、分割関数 1 つの重みの上限。
///
/// 重みは、命令と終端命令の個数に、それらが読むレジスタの個数を足したものである。
/// V8 の最適化コンパイルが使うメモリは、1 つの関数のサイズに対して線形より速く増える。
/// 実測では、3,000 個の値を生存させる分岐のない関数(重み約 42,000)を分けずに何度も呼び出すと、V8 は約 1 GiB を使った。
/// その 2 倍の関数では、コンパイルに使うメモリが尽きて Node が異常終了した。
/// 伝播の分岐を 1 つの出口に集める方式では、重み約 27,000 の関数で異常終了した。
/// 上限は、異常終了した最も小さい関数の重みの 6 分の 1 ほどにしてある。
const PIECE_MAX: usize = 4000;

/// [`piece_max_big`] が返す上限の最小値。
const PIECE_BIG_MIN: usize = 32;

/// 大きなフレーム方式の関数の、分割関数 1 つの重みの上限を返す。
///
/// V8 の最適化コンパイルは、構造体のフィールドに書いた値を、後で読む箇所へ直接渡し、フィールドからの読み出しを省く。
/// 同じ計算も 1 つにまとめる。
/// そのため 1 つの Wasm の関数の中では、構造体に置いたはずの値がレジスタとスタックスロットに残り、
/// フレームのサイズが、関数の中で同時に生存する値の個数(最大で関数の重み)に比例する。
/// 実測では、200 個の値を生存させる節でフレームがスタックスロット 194 個(約 1.6 KB)になり、既定の深さの上限 L の近くでスタックオーバーフローになった。
/// 分割関数の境界は末尾呼び出しで、分割関数は構造体を引数で受け取るので、値は境界をまたいでレジスタに残らない。
/// そこで、分割関数 1 つの重みを、退避する値を 1 つ 8 バイトと数えて、分割関数の 1 段が上限 `budget` に収まる数に抑える。
/// 返す値は [`PIECE_BIG_MIN`] 以上、[`PIECE_MAX`] 以下である。
/// `nkeys` は構造体のフィールドの個数の見積もりである。
fn piece_max_big(budget: u64, nkeys: usize) -> usize {
    let chunks = nkeys.div_ceil(BIG_CHUNK) as u64;
    let base = frame_estimate(TMP_MAX, chunks + 3);
    ((budget.saturating_sub(base) / 8) as usize).clamp(PIECE_BIG_MIN, PIECE_MAX)
}

/// 区切りの最初の命令の生成方法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Head {
    /// 命令の全体を生成する。
    Full,
    /// 呼び出しの後半(結果を受け取る部分)だけを生成する。結果は `eqref` の一時ローカルにある。
    Post,
    /// 継続をキャプチャした後で `perform` を再開するときの後半だけを生成する。結果は再開の値である。
    PostY,
}

/// 区切り。LIR のブロック `block` の命令 `start..end` を表し、`last` なら終端命令も含む。
///
/// `head` が `Full` でなければ、`start` の命令は後半だけを生成する。
/// `pre` なら、`end - 1` の命令は呼び出しの前半(結果を積むまで)だけを生成し、結果を後半に渡して `next` へ分岐する。
/// 結果は、`next` が同じ分割関数にあれば一時ローカルに、ほかの分割関数にあればグローバル `$rv` に置く。
/// `last` でない区切りは、最後に `next` へ分岐する。
#[derive(Clone, Copy, Debug)]
struct Piece {
    block: usize,
    start: usize,
    end: usize,
    head: Head,
    pre: bool,
    last: bool,
    next: usize,
}

/// yield を受け取る再開点の呼び出しが持つ区切りの番号。
#[derive(Clone, Copy, Debug)]
struct Site {
    /// 後半の区切り。
    post: usize,
    /// `perform` なら、継続をキャプチャした後に再開する区切り。
    posty: Option<usize>,
}

/// 区切りの並びと、区切りを分割関数にまとめた結果。
struct Layout {
    pieces: Vec<Piece>,
    /// LIR のブロックごとの、最初の区切りの番号。
    first: Vec<usize>,
    /// 区切りごとの、それを含む分割関数の番号。
    part_of: Vec<usize>,
    /// 分割関数ごとの、最初の区切りの番号。末尾に区切りの総数を置く。
    part_lo: Vec<usize>,
    /// 分割関数ごとの Wasm の関数の番号。分割しない関数では空である。
    funcs: Vec<u32>,
    /// yield を受け取る再開点の呼び出し。キーは、ブロックの番号と、ブロックの中での命令の位置である。
    sites: HashMap<(usize, usize), Site>,
}

impl Layout {
    /// 区切りを作る。
    ///
    /// yield を受け取る再開点の呼び出し(`cuts`)では、呼び出しの前半と後半の間で区切る。
    /// 再開するときに、後半の区切りへ `br_table` で分岐できるようにするためである。
    /// `perform` では、継続をキャプチャした後に再開する区切りを後半の区切りの前に置き、`perform` に続く命令を別の区切りにする。
    /// `split` なら、重みが `pmax`([`PIECE_MAX`] か [`piece_max_big`])を超えないようにブロックの途中でも区切り、区切りを分割関数にまとめる。
    /// `save_w` は、yield を受け取る再開点 1 つにつき加える重みで、フレームの記録を作る命令の分である。
    fn build(
        f: &keleut_lir::Func,
        cuts: &HashMap<(usize, usize), bool>,
        split: bool,
        save_w: usize,
        pmax: usize,
    ) -> Layout {
        let mut pieces: Vec<Piece> = Vec::new();
        let mut first = Vec::new();
        let mut weights = Vec::new();
        let mut sites = HashMap::new();
        for (k, b) in f.blocks.iter().enumerate() {
            first.push(pieces.len());
            let mut start = 0;
            let mut head = Head::Full;
            let mut w = 0;
            for (i, inst) in b.insts.iter().enumerate() {
                let iw = 1 + inst.uses().len();
                if split && w > 0 && w + iw > pmax && !(head != Head::Full && start == i) {
                    let n = pieces.len();
                    pieces.push(Piece {
                        block: k,
                        start,
                        end: i,
                        head,
                        pre: false,
                        last: false,
                        next: n + 1,
                    });
                    weights.push(w);
                    start = i;
                    head = Head::Full;
                    w = 0;
                }
                w += iw;
                let Some(&perform) = cuts.get(&(k, i)) else {
                    continue;
                };
                // `perform` の記録を作る命令は 2 組とも後半の区切りにあるので、後半の重みに数える。
                if !perform {
                    w += save_w;
                }
                let n = pieces.len();
                if perform {
                    // 前半、継続をキャプチャした後に再開する区切り、後半、続きの順に置く。
                    pieces.push(Piece {
                        block: k,
                        start,
                        end: i + 1,
                        head,
                        pre: true,
                        last: false,
                        next: n + 2,
                    });
                    weights.push(w);
                    pieces.push(Piece {
                        block: k,
                        start: i,
                        end: i + 1,
                        head: Head::PostY,
                        pre: false,
                        last: false,
                        next: n + 3,
                    });
                    weights.push(4);
                    pieces.push(Piece {
                        block: k,
                        start: i,
                        end: i + 1,
                        head: Head::Post,
                        pre: false,
                        last: false,
                        next: n + 3,
                    });
                    weights.push(2 * save_w + 8);
                    sites.insert(
                        (k, i),
                        Site {
                            post: n + 2,
                            posty: Some(n + 1),
                        },
                    );
                    start = i + 1;
                    head = Head::Full;
                } else {
                    pieces.push(Piece {
                        block: k,
                        start,
                        end: i + 1,
                        head,
                        pre: true,
                        last: false,
                        next: n + 1,
                    });
                    weights.push(w);
                    sites.insert(
                        (k, i),
                        Site {
                            post: n + 1,
                            posty: None,
                        },
                    );
                    start = i;
                    head = Head::Post;
                }
                w = 0;
            }
            let n = pieces.len();
            pieces.push(Piece {
                block: k,
                start,
                end: b.insts.len(),
                head,
                pre: false,
                last: true,
                next: n + 1,
            });
            weights.push(w + 1 + b.term.uses().len());
        }
        let mut part_of = Vec::new();
        let mut part_lo = vec![0];
        let mut w = 0;
        for (j, pw) in weights.iter().enumerate() {
            if split && w > 0 && w + pw > pmax {
                part_lo.push(j);
                w = 0;
            }
            w += pw;
            part_of.push(part_lo.len() - 1);
        }
        part_lo.push(pieces.len());
        Layout {
            pieces,
            first,
            part_of,
            part_lo,
            funcs: Vec::new(),
            sites,
        }
    }

    fn nparts(&self) -> usize {
        self.part_lo.len() - 1
    }
}

/// LIR の関数の重みを返す。
fn weight(f: &keleut_lir::Func) -> usize {
    f.blocks
        .iter()
        .map(|b| {
            b.insts.iter().map(|i| 1 + i.uses().len()).sum::<usize>() + 1 + b.term.uses().len()
        })
        .sum()
}

/// yield を受け取る再開点の呼び出しを集める。
///
/// キーは、ブロックの番号と、ブロックの中での命令の位置である。値は、命令が `perform` なら真である。
fn yield_cuts(f: &keleut_lir::Func, frame: &keleut_lir::Frame) -> HashMap<(usize, usize), bool> {
    let mut out = HashMap::new();
    let mut rp = 0usize;
    for (k, b) in f.blocks.iter().enumerate() {
        for (i, inst) in b.insts.iter().enumerate() {
            if inst.is_call() {
                if frame.yield_points.get(rp).copied().unwrap_or(true) {
                    out.insert(
                        (k, i),
                        matches!(
                            inst,
                            Inst::Fx {
                                op: FxOp::Perform { .. },
                                ..
                            }
                        ),
                    );
                }
                rp += 1;
            }
        }
    }
    out
}

/// 呼び出しの命令の生成方法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// 前半と後半を続けて生成する。
    Full,
    /// 前半(結果を積むまで)だけを生成する。
    Pre,
    /// 後半だけを生成する。結果は一時ローカルから読む。
    Post,
    /// 継続をキャプチャした後で `perform` を再開するときの後半だけを生成する。
    PostY,
}

/// 関数ごとの、フレームの記録の作り方。
#[derive(Clone)]
struct Rec {
    /// 記録の型(`$Frame` の部分型)。
    ty: u32,
    /// 記録に書くローカル。基底型のフィールドの後ろに置く順に並べる。
    saved: Vec<u32>,
    /// 分割関数ごとの再開用エントリ(`$ResFn`)。分割しない関数では 1 つだけである。
    tramps: Vec<Option<u32>>,
}

struct Tr<'a, 'p> {
    cg: &'a mut Cg<'p>,
    f: &'p keleut_lir::Func,
    fb: Fb,
    loc: Vec<Loc>,
    wt: Vec<ValType>,
    lay: std::rc::Rc<Layout>,
    /// 現在の区切り。
    cur: usize,
    /// 現在の分割関数に入る区切りの範囲。分割しない関数では、すべての区切りである。
    lo: usize,
    hi: usize,
    /// 現在の区切りの命令の中で開いている制御構造(`if` など)の個数。
    extra: u32,
    fdepth: u32,
    /// 分割関数が入口の区切りの番号を受け取る、`i32` の引数のローカル。
    entry: Option<u32>,
    /// 大きなフレームの構造体。値は、構造体を置くローカルの番号と、構造体の型の番号である。
    bf: Vec<(u32, u32)>,
    tmps: HashMap<ValType, u32>,
    /// 関数の入口で、まとめた配列から読む引数のレジスタ。
    packed: Vec<keleut_lir::Reg>,
    /// まとめた配列を受け取る引数のローカル。
    packed_arr: Option<u32>,
    /// yield を受け取る再開点があるときの、フレームの記録の作り方。
    rec: Option<std::rc::Rc<Rec>>,
    /// 分割する関数なら真。
    split: bool,
    /// 最後に番兵を受け取った呼び出しの、後半の区切りの番号を置く `i32` の格納先。`propagate` を持つ関数だけが使う。
    ///
    /// 大きなフレーム方式の関数では構造体のフィールドに置くので、分割関数をまたいでも値が残る。
    lastrp: Option<Loc>,
    /// 番兵を受け取る呼び出しの後半の区切り。`propagate` が、再開する区切りとして記録に書く候補である。
    catch_posts: std::rc::Rc<Vec<usize>>,
    /// 1 段のバイト数の見積もりに許す上限([`budget_of`])。
    budget: u64,
}

/// 関数の 1 段に許す、見積もりバイト数の上限を返す。
///
/// 論理的な 1 段に入る Wasm のフレームの合計が `B_wasm` に収まるようにする。
/// ハンドラ式と末尾位置にない `resume` は、深さを増やさずに `$rt_handle` を呼ぶ。
/// 継続の再開では、さらに `rt_resume` が同じ深さにある(`doc/lir.md` の 13 節と 16.2 節)。
/// そこで、`$rt_handle` を非末尾呼び出しで呼ぶ関数の上限は、`B_wasm` からこの 2 つのフレームの分を引いた値にする。
/// `cancel` 節も `$rt_handle` と同じ深さで実行されるので、同じ上限にする。
/// `$rt_handle` 自身の上限は [`E_RTH`] である。
fn budget_of(prog: &keleut_lir::Program, fid: FuncId) -> u64 {
    let Some(rh) = prog.rt_handle else {
        return B_WASM;
    };
    if fid == rh {
        return E_RTH;
    }
    let f = &prog.funcs[fid.index()];
    let calls = f.blocks.iter().any(|b| {
        b.insts
            .iter()
            .any(|i| matches!(i, Inst::CallDirect { func, .. } if *func == rh))
    });
    if calls || f.kind == FuncKind::Kir(keleut_lir::FnKind::Cancelclause) {
        B_WASM - E_RTH - E_RRES
    } else {
        B_WASM
    }
}

fn ht(idx: u32) -> HeapType {
    HeapType::Concrete(idx)
}

/// Rep に対応する Wasm の型を返す。
fn wtype(cg: &Cg, r: Rep) -> ValType {
    match r {
        Rep::Bool | Rep::I32 => I32,
        Rep::I64 => I64,
        Rep::F64 => F64,
        Rep::Str => rv(true, cg.m.t.str_),
        Rep::Any => EQ,
    }
}

/// スロットに対応しないローカルの個数の上限。
///
/// 一時ローカルは Wasm の型ごとに 1 つで、使う型は i32、i64、f64、eqref、文字列、`$anyarr`、`$Clo` と、
/// 再開するときに読むフレームの記録の 8 種類である。
/// これに、深さのローカルと、`propagate` を持つ関数の `lastrp` のローカルを加えて 10 個になる。
/// 大きなフレーム方式では多めの見積もりになる。
const TMP_MAX: u64 = 10;

/// yield を受け取る再開点 1 つにつき、フレームの記録を作る命令の分として見積もる重み。
///
/// 実際に加える重みは、これに、記録に書くローカルの個数を足した値である。
const SAVE_W: usize = 8;

/// `propagate` を終端命令に持つブロックがあれば真を返す。
///
/// `propagate` を持つのは、ハンドラのフレームになる関数 `$rt_handle` だけである。
fn has_propagate(f: &keleut_lir::Func) -> bool {
    f.blocks.iter().any(|b| {
        matches!(
            b.term,
            Term::Fx {
                op: FxTermOp::Propagate,
                ..
            }
        )
    })
}

/// LIR の関数から Wasm の関数を生成する。
///
/// 重み([`weight`])に記録を作る命令の分を加えた値が [`PIECE_MAX`] を超える関数は、大きなフレーム方式にしたうえで、いくつかの分割関数に分ける。
/// 見積もりが上限を超えて大きなフレーム方式にする関数も、フィールドが多ければ、重みが [`piece_max_big`] 以下の分割関数に分ける。
/// 分割する関数では、LIR の関数の入口が構造体を確保して引数を置き、最初の分割関数を末尾呼び出しで呼ぶ。
/// 分割関数は `(env (ref null $Clo)) (構造体 × C) (深さ i64) (入口 i32) → eqref` を受け取り、入口の区切りへ `br_table` で分岐する。
/// ほかの分割関数にある区切りへ分岐するときは、その分割関数を末尾呼び出しで呼ぶ。
/// 末尾呼び出しなので、論理的な 1 段が持つ Wasm のフレームは、どの時点でも分割関数 1 つのフレームだけである。
///
/// yield を受け取る再開点を持つ関数には、再開用エントリ(`$ResFn` の小さな関数)を作る(モジュールの説明の「継続のキャプチャと再開」)。
/// 返すのは LIR の関数の入口になる Wasm の関数の本体で、分割関数と再開用エントリの本体はモジュールに直接置く。
pub fn generate(cg: &mut Cg, fid: FuncId) -> Result<Function, String> {
    let prog = cg.prog;
    let f = &prog.funcs[fid.index()];
    let frame = f.frame.as_ref().ok_or("フレームのない関数")?;
    let n = f.params.len() as u32;
    // Wasm の引数の個数。まとめた引数の配列は 1 つと数える。
    let w = crate::wire_arity(n);
    let mut fb = Fb::new(w + 2);
    let wt: Vec<ValType> = f.regs.iter().map(|r| wtype(cg, *r)).collect();
    // 直接受け取る引数と、関数の入口でまとめた配列から読む引数。
    let direct = n.min(crate::P_DIRECT) as usize;
    let packed: Vec<keleut_lir::Reg> = f.params[direct..].to_vec();
    if packed.iter().any(|p| wt[p.index()] != EQ) {
        return Err("引数のレジスタの型が eqref でありません".into());
    }
    // G の節がプログラムになければ YIELD は設定されないので、再開点で区切らない([`Cg::has_g`])。
    let cuts = if cg.has_g {
        yield_cuts(f, frame)
    } else {
        HashMap::new()
    };
    let lastrp_needed = cg.has_g && has_propagate(f);
    // 分けるかどうかの判断には、記録を作る命令の重みも含める。ローカルの個数は、スロットと型の組の個数で見積もる。
    let nkeys_est = (0..f.regs.len())
        .map(|r| (frame.slot[r], wt[r]))
        .collect::<std::collections::HashSet<_>>()
        .len();
    let budget = budget_of(prog, fid);
    let big_est = frame_estimate(nkeys_est as u64 + TMP_MAX, u64::from(w) + 2) > budget;
    let save_est = if big_est {
        nkeys_est.div_ceil(BIG_CHUNK) + direct
    } else {
        nkeys_est
    };
    // `perform` の再開点は、記録を作る命令を 2 組持つ。
    let nsaves = cuts.values().map(|&p| if p { 2 } else { 1 }).sum::<usize>();
    // 大きなフレーム方式にする関数は、構造体のフィールドが分割関数 1 つの重みの上限より多ければ、
    // 重みによらず小さな分割関数に分ける([`piece_max_big`])。
    // フィールドが少なければ、1 つの Wasm の関数の中で同時に生存する値もその個数を超えないので、分けない。
    let pbig = piece_max_big(budget, nkeys_est + 1);
    let force = big_est && nkeys_est > pbig;
    let split = force || weight(f) + nsaves * (SAVE_W + save_est) > PIECE_MAX;
    let pmax = if force { pbig } else { PIECE_MAX };
    // スロットと型の組ごとのローカル。分割しない関数では、直接受け取る引数は引数のローカルを使い、
    // まとめた配列から読む引数は通常のレジスタと同じように扱う。
    // 分割する関数では、分割関数の間で受け渡すために、直接受け取る引数も構造体に置く。
    let mut keys: HashMap<(Slot, ValType), usize> = HashMap::new();
    let mut key_list: Vec<ValType> = Vec::new();
    let mut is_param = vec![None; f.regs.len()];
    let mut param_keys: HashMap<(Slot, ValType), u32> = HashMap::new();
    if !split {
        for (i, p) in f.params[..direct].iter().enumerate() {
            is_param[p.index()] = Some(i as u32);
            param_keys.insert((frame.slot[p.index()], wt[p.index()]), 1 + i as u32);
        }
    }
    let mut reg_key = vec![usize::MAX; f.regs.len()];
    for r in 0..f.regs.len() {
        if is_param[r].is_some() {
            continue;
        }
        let key = (frame.slot[r], wt[r]);
        if param_keys.contains_key(&key) {
            continue;
        }
        let next = key_list.len();
        let k = *keys.entry(key).or_insert_with(|| {
            key_list.push(wt[r]);
            next
        });
        reg_key[r] = k;
    }
    // 見積もり。ローカルの個数に、深さのローカルと一時ローカルの分(TMP_MAX 個)を足す。
    let est = frame_estimate(key_list.len() as u64 + TMP_MAX, u64::from(w) + 2);
    let big = split || est > budget;
    // `lastrp` は、大きなフレーム方式ではレジスタと同じように構造体に置く。
    let lastrp_key = if lastrp_needed && big {
        key_list.push(I32);
        Some(key_list.len() - 1)
    } else {
        None
    };
    let mut loc = vec![Loc::Local(0); f.regs.len()];
    let mut bf = Vec::new();
    let mut key_loc: Vec<Loc> = Vec::new();
    let mut key_locals: Vec<u32> = Vec::new();
    if big {
        for (c, chunk) in key_list.chunks(BIG_CHUNK).enumerate() {
            let ty = cg.m.add_struct(chunk);
            let l = fb.local(rv(true, ty));
            bf.push((l, ty));
            for i in 0..chunk.len() {
                key_loc.push(Loc::Field(c as u32, i as u32));
            }
        }
        cg.stats.big_frames += 1;
    } else {
        for t in &key_list {
            let l = fb.local(*t);
            key_locals.push(l);
            key_loc.push(Loc::Local(l));
        }
    }
    for r in 0..f.regs.len() {
        loc[r] = if let Some(i) = is_param[r] {
            Loc::Local(1 + i)
        } else if let Some(&p) = param_keys.get(&(frame.slot[r], wt[r])) {
            Loc::Local(p)
        } else {
            key_loc[reg_key[r]]
        };
    }
    let fdepth = fb.local(I64);
    let lastrp = match (lastrp_needed, lastrp_key) {
        (true, Some(k)) => Some(key_loc[k]),
        (true, None) => Some(Loc::Local(fb.local(I32))),
        _ => None,
    };
    let nb = f.blocks.len();
    let packed_arr = if packed.is_empty() {
        None
    } else {
        Some(1 + crate::P_DIRECT)
    };
    let save_w = SAVE_W
        + if big {
            bf.len() + if split { 0 } else { direct }
        } else {
            key_list.len() + direct
        };
    let mut lay = Layout::build(f, &cuts, split, save_w, pmax);
    let t = cg.m.t;
    let idx = cg.lir(fid);
    let base = cg.m.func_name(idx).to_string();
    if split {
        // 分割関数を宣言する。
        let mut params = vec![rv(true, t.clo)];
        params.extend(bf.iter().map(|&(_, ty)| rv(true, ty)));
        params.extend([I64, I32]);
        let ty = cg.m.sig(&params, &[EQ]);
        for q in 0..lay.nparts() {
            lay.funcs
                .push(cg.m.declare_func(ty, format!("{base}#p{q}"), false));
        }
        cg.stats.split_funcs += 1;
        cg.stats.max_parts = cg.stats.max_parts.max(lay.nparts());
    }
    // フレームの記録の型と再開用エントリ。
    let rec = if lay.sites.is_empty() {
        None
    } else {
        let (saved, types): (Vec<u32>, Vec<ValType>) = if split {
            let mut v = vec![(0u32, rv(true, t.clo))];
            for (i, &(_, ty)) in bf.iter().enumerate() {
                v.push((1 + i as u32, rv(true, ty)));
            }
            v.into_iter().unzip()
        } else {
            let mut v = vec![(0u32, rv(true, t.clo))];
            for i in 0..direct {
                v.push((1 + i as u32, EQ));
            }
            if big {
                for &(l, ty) in &bf {
                    v.push((l, rv(true, ty)));
                }
            } else {
                for (k, &l) in key_locals.iter().enumerate() {
                    v.push((l, key_list[k]));
                }
            }
            v.into_iter().unzip()
        };
        let ty = cg.m.add_frame_record(&types);
        let mut tramps = Vec::new();
        let nparts = lay.nparts();
        for q in 0..nparts {
            let has = lay
                .sites
                .values()
                .any(|s| lay.part_of[s.post] == q || s.posty.is_some_and(|y| lay.part_of[y] == q));
            if has {
                let name = if split {
                    format!("{base}#p{q}#r")
                } else {
                    format!("{base}#r")
                };
                let fi = cg.m.declare_func(t.resfn, name, false);
                cg.m.declare_ref(fi);
                tramps.push(Some(fi));
            } else {
                tramps.push(None);
            }
        }
        // 再開用エントリの本体。
        for (q, tr) in tramps.iter().enumerate() {
            let Some(fi) = *tr else { continue };
            let mut tb = Fb::new(1);
            if split {
                let r = tb.local(rv(true, ty));
                tb.i().local_get(0).ref_cast_non_null(ht(ty)).local_tee(r);
                tb.i().struct_get(ty, crate::emit::frf::BASE);
                for i in 0..bf.len() {
                    tb.i()
                        .local_get(r)
                        .struct_get(ty, crate::emit::frf::BASE + 1 + i as u32);
                }
                tb.i()
                    .global_get(cg.g.depth)
                    .local_get(r)
                    .struct_get(ty, crate::emit::frf::RP)
                    .return_call(lay.funcs[q]);
            } else {
                tb.i().ref_null(ht(t.clo));
                for _ in 0..direct {
                    tb.i().ref_null(HEQ);
                }
                if packed_arr.is_some() {
                    tb.i().global_get(cg.g.empty);
                }
                tb.i().local_get(0).return_call(idx);
            }
            cg.m.set_body(fi, tb.finish());
        }
        Some(std::rc::Rc::new(Rec { ty, saved, tramps }))
    };
    let mut catch_posts: Vec<usize> = lay
        .sites
        .iter()
        .filter(
            |((k, i), _)| matches!(&f.blocks[*k].insts[*i], Inst::Fx { op, .. } if op.catches()),
        )
        .map(|(_, s)| s.post)
        .collect();
    catch_posts.sort_unstable();
    let catch_posts = std::rc::Rc::new(catch_posts);
    let lay = std::rc::Rc::new(lay);
    let nparts = lay.nparts();
    let mut x = Tr {
        cg,
        f,
        fb,
        loc: loc.clone(),
        wt: wt.clone(),
        lay: lay.clone(),
        cur: 0,
        lo: 0,
        hi: lay.part_lo[1],
        extra: 0,
        fdepth,
        entry: None,
        bf: bf.clone(),
        tmps: HashMap::new(),
        packed,
        packed_arr,
        rec: rec.clone(),
        split,
        lastrp,
        catch_posts: catch_posts.clone(),
        budget,
    };
    if split {
        // 入口: 直接受け取った引数を構造体に置き、最初の分割関数を末尾呼び出しで呼ぶ。
        x.prologue();
        for (i, p) in f.params[..direct].iter().enumerate() {
            x.fb.i().local_get(1 + i as u32);
            x.set(*p);
        }
        x.fb.i().i32_const(0);
        x.call_part(0);
    } else {
        x.part(false)?;
    }
    let est = x.finish_estimate(u64::from(w) + 2)?;
    let st = &mut x.cg.stats;
    st.max_estimate = st.max_estimate.max(est);
    st.max_params = st.max_params.max(n as usize);
    st.max_blocks = st.max_blocks.max(nb);
    let entry_body = x.fb.finish();
    for q in (0..nparts).filter(|_| split) {
        let c = bf.len() as u32;
        let mut y = Tr {
            cg: &mut *x.cg,
            f,
            fb: Fb::new(c + 3),
            loc: loc.clone(),
            wt: wt.clone(),
            lay: lay.clone(),
            cur: lay.part_lo[q],
            lo: lay.part_lo[q],
            hi: lay.part_lo[q + 1],
            extra: 0,
            fdepth: c + 1,
            entry: Some(c + 2),
            bf: bf
                .iter()
                .enumerate()
                .map(|(i, &(_, ty))| (1 + i as u32, ty))
                .collect(),
            tmps: HashMap::new(),
            packed: Vec::new(),
            packed_arr: None,
            rec: rec.clone(),
            split,
            lastrp,
            catch_posts: catch_posts.clone(),
            budget,
        };
        y.part(true)?;
        let est = y.finish_estimate(u64::from(c) + 3)?;
        y.cg.stats.max_estimate = y.cg.stats.max_estimate.max(est);
        let body = y.fb.finish();
        y.cg.m.set_body(lay.funcs[q], body);
    }
    Ok(entry_body)
}

impl<'a, 'p> Tr<'a, 'p> {
    fn t(&self) -> crate::emit::Tys {
        self.cg.m.t
    }

    fn tmp(&mut self, t: ValType) -> u32 {
        if let Some(&l) = self.tmps.get(&t) {
            return l;
        }
        let l = self.fb.local(t);
        self.tmps.insert(t, l);
        l
    }

    /// レジスタの値を積む。
    fn get(&mut self, r: keleut_lir::Reg) {
        self.get_loc(self.loc[r.index()]);
    }

    /// 格納先 `l` の値を積む。
    fn get_loc(&mut self, l: Loc) {
        match l {
            Loc::Local(l) => {
                self.fb.i().local_get(l);
            }
            Loc::Field(c, i) => {
                let (l, ty) = self.bf[c as usize];
                self.fb.i().local_get(l).struct_get(ty, i);
            }
        }
    }

    /// 積んだ値をレジスタに書く。
    fn set(&mut self, r: keleut_lir::Reg) {
        self.set_loc(self.loc[r.index()], self.wt[r.index()]);
    }

    /// 積んだ型 `t` の値を格納先 `loc` に書く。
    fn set_loc(&mut self, loc: Loc, t: ValType) {
        match loc {
            Loc::Local(l) => {
                self.fb.i().local_set(l);
            }
            Loc::Field(c, i) => {
                let (l, ty) = self.bf[c as usize];
                let tl = self.tmp(t);
                self.fb
                    .i()
                    .local_set(tl)
                    .local_get(l)
                    .local_get(tl)
                    .struct_set(ty, i);
            }
        }
    }

    /// 区切り `j` へ分岐する `br` の深さを返す。
    ///
    /// `j` は、現在の分割関数の中にあり、現在の区切りより後ろでなければならない。
    fn depth_to(&self, j: usize) -> u32 {
        debug_assert!(j > self.cur && j < self.hi);
        (j - self.cur - 1) as u32 + self.extra
    }

    /// 区切り `j` が現在の分割関数にあれば真を返す。
    fn in_part(&self, j: usize) -> bool {
        self.lo <= j && j < self.hi
    }

    /// 区切り `j` へ分岐する。次の区切りなら命令を書かず、そのまま次へ進む。
    fn goto(&mut self, j: usize) {
        if !(j == self.cur + 1 && self.in_part(j)) {
            self.branch_to(j);
        }
    }

    /// 区切り `j` へ分岐する命令を書く。`j` が次の区切りでも、命令を省かない。
    fn branch_to(&mut self, j: usize) {
        if self.in_part(j) {
            let d = self.depth_to(j);
            self.fb.i().br(d);
        } else {
            let q = self.lay.part_of[j];
            let e = j - self.lay.part_lo[q];
            self.fb.i().i32_const(e as i32);
            self.call_part(q);
        }
    }

    /// 分割関数 `q` を末尾呼び出しで呼ぶ。入口の区切りの番号は、あらかじめ積んでおく。
    fn call_part(&mut self, q: usize) {
        let tl = self.tmp(I32);
        self.fb.i().local_set(tl).local_get(0);
        for &(l, _) in &self.bf {
            self.fb.i().local_get(l);
        }
        self.fb
            .i()
            .local_get(self.fdepth)
            .local_get(tl)
            .return_call(self.lay.funcs[q]);
    }

    /// LIR の関数の入口の処理を生成する。
    ///
    /// 深さをローカルに記録し、構造体を確保し、まとめた配列から引数を読む。
    fn prologue(&mut self) {
        let g = self.cg.g;
        self.fb.i().global_get(g.depth).local_set(self.fdepth);
        for &(l, ty) in &self.bf {
            self.fb.i().struct_new_default(ty).local_set(l);
        }
        if let Some(arr) = self.packed_arr {
            let aget = self.cg.func(FKey::H(H::Aget));
            for (k, r) in self.packed.clone().into_iter().enumerate() {
                self.fb.i().local_get(arr).i32_const(k as i32).call(aget);
                self.set(r);
            }
        }
    }

    /// 現在の分割関数に入る区切りのコードを生成する。分割しない関数では、すべての区切りのコードを生成する。
    ///
    /// `dispatch` が真なら(分割関数)、入口の区切りの番号で区切りへ分岐する `br_table` を先頭に置く。
    /// 偽なら(分割しない関数)、関数の入口の処理([`Tr::prologue`])を置く。
    /// 分割しない関数のうち、yield を受け取る再開点を持つものは、その前に、再開のための分岐([`Tr::resume_entry`])を置く。
    fn part(&mut self, dispatch: bool) -> Result<(), String> {
        let (lo, hi) = (self.lo, self.hi);
        let resumable = self.rec.is_some();
        let opened = if dispatch { lo } else { lo + 1 };
        for _ in opened..hi {
            self.fb.i().block(BlockType::Empty);
        }
        if let (true, Some(e)) = (dispatch, self.entry) {
            if resumable {
                // 再開用エントリから呼ばれたときと、分割関数をまたぐ呼び出しの後半は、結果をグローバル $rv で受け取る。
                let tl = self.tmp(EQ);
                self.fb.i().global_get(self.cg.g.rv).local_set(tl);
            }
            let targets: Vec<u32> = (0..(hi - lo) as u32).collect();
            self.fb.i().local_get(e).br_table(targets, 0).end();
        } else if !dispatch {
            if let Some(rec) = self.rec.clone() {
                self.resume_entry(&rec, (hi - lo) as u32);
            }
            self.prologue();
        }
        let f = self.f;
        let lay = self.lay.clone();
        for j in lo..hi {
            self.cur = j;
            self.extra = 0;
            let pc = lay.pieces[j];
            let b = &f.blocks[pc.block];
            for ii in pc.start..pc.end {
                let mode = if ii == pc.start && pc.head == Head::Post {
                    Mode::Post
                } else if ii == pc.start && pc.head == Head::PostY {
                    Mode::PostY
                } else if ii + 1 == pc.end && pc.pre {
                    Mode::Pre
                } else {
                    Mode::Full
                };
                let site = lay.sites.get(&(pc.block, ii)).copied();
                self.inst(&b.insts[ii], mode, site)?;
            }
            if pc.last {
                self.term(&b.term)?;
            } else {
                if pc.pre {
                    // 前半の結果を後半へ渡す。同じ分割関数なら一時ローカルに、ほかの分割関数ならグローバル $rv に置く。
                    if self.in_part(pc.next) {
                        let tl = self.tmp(EQ);
                        self.fb.i().local_set(tl);
                    } else {
                        self.fb.i().global_set(self.cg.g.rv);
                    }
                }
                self.goto(pc.next);
            }
            if j + 1 < hi {
                self.fb.i().end();
            }
        }
        // どの区切りも分岐か return で終わるので、ここは実行されない。型を合わせるために、伝播して返る命令を置く。
        self.prop_br();
        Ok(())
    }

    /// 分割しない関数の先頭に置く、再開のための分岐を生成する。
    ///
    /// 最後の引数(フレームの記録)が null なら、通常の入口へそのまま進む。
    /// null でなければ、記録からローカルを復元し、深さのローカルに、`rt_resume` が設定したグローバルの深さを書く。
    /// 続いて、渡された値を一時ローカルに置き、記録にある区切りへ `br_table` で分岐する。
    fn resume_entry(&mut self, rec: &Rec, npieces: u32) {
        let g = self.cg.g;
        let w = crate::wire_arity(self.f.params.len() as u32);
        let r = self.tmp(rv(true, rec.ty));
        let tl = self.tmp(EQ);
        self.fb
            .i()
            .block(BlockType::Empty)
            .local_get(w + 1)
            .br_on_null(0)
            .ref_cast_non_null(ht(rec.ty))
            .local_set(r);
        for (k, &l) in rec.saved.iter().enumerate() {
            self.fb
                .i()
                .local_get(r)
                .struct_get(rec.ty, crate::emit::frf::BASE + k as u32)
                .local_set(l);
        }
        let targets: Vec<u32> = (0..npieces).collect();
        self.fb
            .i()
            .global_get(g.depth)
            .local_set(self.fdepth)
            .global_get(g.rv)
            .local_set(tl)
            .local_get(r)
            .struct_get(rec.ty, crate::emit::frf::RP)
            .br_table(targets, 0)
            .end();
    }

    /// フレームの記録を作って、並び `yf` の末尾に追加する。伝播する前に呼ぶ。`j` は再開する区切りである。
    fn save(&mut self, j: usize) {
        let Some(rec) = self.rec.clone() else {
            return;
        };
        let t = self.t();
        let push = self.cg.func(FKey::H(H::YfPush));
        let q = self.lay.part_of[j];
        let rp = if self.split {
            j - self.lay.part_lo[q]
        } else {
            j
        };
        let entry = rec.tramps[q].expect("再開用エントリのない分割関数");
        self.fb
            .i()
            .i32_const(rp as i32)
            .ref_func(entry)
            .ref_null(ht(t.frame))
            .local_get(self.fdepth);
        for &l in &rec.saved {
            self.fb.i().local_get(l);
        }
        self.fb.i().struct_new(rec.ty).call(push);
    }

    /// 状態ワードが `YIELD` なら、フレームの記録を作って並びに追加する。
    fn save_if_yield(&mut self, j: usize) {
        let g = self.cg.g;
        self.fb
            .i()
            .global_get(g.p_kind)
            .i32_const(pk::YIELD)
            .i32_eq()
            .if_(BlockType::Empty);
        self.save(j);
        self.fb.i().end();
    }

    /// yield を受け取らない再開点が `YIELD` を受け取ったら、内部エラーで終了する(`doc/lir.md` の 17 節)。
    fn fatal_if_yield(&mut self) {
        let g = self.cg.g;
        let tp = self
            .cg
            .tpl
            .internal("yield が noyield の再開点に達しました(may_yield の印の誤り)");
        self.fb
            .i()
            .global_get(g.p_kind)
            .i32_const(pk::YIELD)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(tp as i32)
            .call(imp::FATAL)
            .end();
    }

    /// 生成し終えた関数の、1 段のバイト数の見積もりを返す。
    ///
    /// 見積もりが関数ごとの上限(`self.budget`。[`budget_of`])を超えたら、エラーを返す。
    fn finish_estimate(&self, params: u64) -> Result<u64, String> {
        let est = frame_estimate(self.fb.nlocals() as u64, params);
        if est > self.budget {
            return Err(format!(
                "関数 {} の 1 段の見積もり {est} バイトが上限 {} を超えます(B_wasm = {B_WASM})",
                self.f.sym, self.budget
            ));
        }
        Ok(est)
    }

    /// 伝播する命令を生成する。深さを書き戻して null を返す。
    ///
    /// 伝播の分岐を 1 つの出口のブロックに集めると、伝播することがある命令の多い関数で、V8 の最適化コンパイルがメモリを使い尽くす。
    /// そこで、伝播する箇所ごとに、その場で返す。
    fn prop_br(&mut self) {
        let g = self.cg.g;
        self.fb
            .i()
            .local_get(self.fdepth)
            .global_set(g.depth)
            .ref_null(HEQ)
            .return_();
    }

    /// 積んだ値が null なら(状態ワードが設定されている)伝播し、そうでなければ値をレジスタ `dst` に書く。
    fn check_set(&mut self, dst: keleut_lir::Reg) {
        let tl = self.tmp(self.wt[dst.index()]);
        self.fb
            .i()
            .local_tee(tl)
            .ref_is_null()
            .if_(BlockType::Empty);
        self.prop_br();
        self.fb.i().end().local_get(tl);
        self.set(dst);
    }

    /// 積んだ `i32` が 0 でなければ伝播する。
    fn prop_if(&mut self) {
        self.fb.i().if_(BlockType::Empty);
        self.prop_br();
        self.fb.i().end();
    }

    fn any_array(&mut self, regs: &[keleut_lir::Reg]) {
        let t = self.t();
        if regs.len() <= NEW_FIXED_MAX {
            for r in regs {
                self.get(*r);
            }
            self.fb.i().array_new_fixed(t.anyarr, regs.len() as u32);
        } else {
            let new = self.cg.func(FKey::H(H::AnyArrNew));
            let aset = self.cg.func(FKey::H(H::Aset));
            let tl = self.tmp(rv(true, t.anyarr));
            self.fb
                .i()
                .i32_const(regs.len() as i32)
                .call(new)
                .local_set(tl);
            for (i, r) in regs.iter().enumerate() {
                self.fb.i().local_get(tl).i32_const(i as i32);
                self.get(*r);
                self.fb.i().call(aset);
            }
            self.fb.i().local_get(tl).ref_as_non_null();
        }
    }

    fn depth_check(&mut self, push: bool) {
        let g = self.cg.g;
        let raise = self.cg.func(FKey::H(H::RaiseDepth));
        self.fb
            .i()
            .global_get(g.depth)
            .i64_const(1)
            .i64_add()
            .global_get(g.limit)
            .i64_gt_u()
            .if_(BlockType::Empty)
            .call(raise);
        self.extra += 1;
        self.prop_br();
        self.fb.i().end();
        self.extra -= 1;
        if push {
            self.fb
                .i()
                .global_get(g.depth)
                .i64_const(1)
                .i64_add()
                .global_set(g.depth);
        }
    }

    /// テンプレート `tp` の実行時エラーを設定して伝播する。
    ///
    /// `v` はエラーに添える値のレジスタ、`x` はエラーに添える `f64` の値のレジスタである。
    /// どちらかがないときは、それぞれ null と 0 を渡す。
    fn raise_with(&mut self, tp: u32, v: Option<keleut_lir::Reg>, x: Option<keleut_lir::Reg>) {
        let raise = self.cg.func(FKey::H(H::Raise));
        self.fb.i().i32_const(tp as i32);
        match v {
            Some(r) => self.get(r),
            None => {
                self.fb.i().ref_null(HEQ);
            }
        }
        match x {
            Some(r) => {
                self.get(r);
                self.fb.i().i64_reinterpret_f64();
            }
            None => {
                self.fb.i().i64_const(0);
            }
        }
        self.fb.i().call(raise);
        self.prop_br();
    }

    /// 関数値の呼び出しを生成する。
    ///
    /// 呼び出し先が引数 `n` 個のクロージャなら、高速経路(fast path)として `call_ref` で直接呼ぶ。
    /// そうでなければ、引数の個数ごとに生成する関数 `callvN`(`FKey::CallV`)を呼ぶ。
    fn call_value(&mut self, callee: keleut_lir::Reg, args: &[keleut_lir::Reg], tail: bool) {
        let n = args.len() as u32;
        let (fty, cty) = self.cg.m.fn_types(n);
        let callv = self.cg.func(FKey::CallV(n));
        let t = self.t();
        self.get(callee);
        let bt = if tail {
            BlockType::Empty
        } else {
            BlockType::Result(EQ)
        };
        self.fb.i().ref_test_non_null(ht(cty)).if_(bt);
        self.extra += 1;
        // クロージャは、環境の引数と関数の参照の 2 か所で使う。どちらもレジスタから読み直して型を絞る。
        // クロージャの型ごとに一時ローカルを追加すると、ローカルの個数に上限がなくなるからである。
        self.get(callee);
        self.fb.i().ref_cast_non_null(ht(cty));
        for a in args {
            self.get(*a);
        }
        self.cg.pack_args(&mut self.fb, n);
        self.fb.i().ref_null(ht(t.frame));
        self.get(callee);
        self.fb.i().ref_cast_non_null(ht(cty)).struct_get(cty, 2);
        if tail {
            self.fb.i().return_call_ref(fty);
        } else {
            self.fb.i().call_ref(fty);
        }
        self.fb.i().else_();
        self.get(callee);
        for a in args {
            self.get(*a);
        }
        self.cg.pack_args(&mut self.fb, n);
        if tail {
            self.fb.i().return_call(callv);
        } else {
            self.fb.i().call(callv);
        }
        self.fb.i().end();
        self.extra -= 1;
        if tail {
            self.fb.i().unreachable();
        }
    }

    fn inst(&mut self, i: &Inst, mode: Mode, site: Option<Site>) -> Result<(), String> {
        if i.is_call() {
            if !matches!(mode, Mode::Post | Mode::PostY) {
                self.call_pre(i)?;
            }
            match mode {
                Mode::Pre => {}
                Mode::Full => self.call_post(i, Mode::Full, site)?,
                Mode::Post | Mode::PostY => {
                    let tl = self.tmp(EQ);
                    self.fb.i().local_get(tl);
                    self.call_post(i, mode, site)?;
                }
            }
            return Ok(());
        }
        let t = self.t();
        let g = self.cg.g;
        use Inst::*;
        match i {
            Const { dst, value } => {
                match value {
                    keleut_lir::Const::Unit => {
                        self.fb.i().global_get(g.unit);
                    }
                    keleut_lir::Const::Bool(b) => {
                        self.fb.i().i32_const(i32::from(*b));
                    }
                    keleut_lir::Const::I32(n) => {
                        self.fb.i().i32_const(*n);
                    }
                    keleut_lir::Const::I64(n) => {
                        self.fb.i().i64_const(*n);
                    }
                    keleut_lir::Const::F64(bits) => {
                        self.fb.i().f64_const(Ieee64::new(*bits));
                    }
                    keleut_lir::Const::Str(s) => {
                        let gi = self.cg.strs[s.index()].0;
                        self.fb.i().global_get(gi);
                    }
                    keleut_lir::Const::Ctor0 { ty, ctor } => {
                        let gi = self.cg.ctor0(ty.0, *ctor);
                        self.fb.i().global_get(gi);
                    }
                    keleut_lir::Const::DictBuiltin { class, ty } => {
                        let gi = self.cg.dictb(class.0, ty.0);
                        self.fb.i().global_get(gi);
                    }
                    keleut_lir::Const::DictInst0(inst) => {
                        let gi = self.cg.dicti0(inst.0);
                        self.fb.i().global_get(gi);
                    }
                }
                self.set(*dst);
            }
            Move { dst, src } => {
                self.get(*src);
                self.set(*dst);
            }
            Box { dst, src } => {
                self.get(*src);
                match self.f.regs[src.index()] {
                    Rep::Bool => {
                        let tl = self.tmp(I32);
                        self.fb
                            .i()
                            .local_set(tl)
                            .global_get(g.true_)
                            .global_get(g.false_)
                            .local_get(tl)
                            .typed_select(EQ);
                    }
                    Rep::I32 => {
                        let b = self.cg.func(FKey::H(H::BoxI32));
                        self.fb.i().call(b);
                    }
                    Rep::I64 => {
                        self.fb.i().struct_new(t.box_i64);
                    }
                    Rep::F64 => {
                        self.fb.i().struct_new(t.box_f64);
                    }
                    Rep::Str | Rep::Any => {}
                }
                self.set(*dst);
            }
            Unbox { dst, src } => {
                self.get(*src);
                match self.f.regs[dst.index()] {
                    Rep::Bool => {
                        self.fb.i().global_get(g.true_).ref_eq();
                    }
                    Rep::I32 => {
                        let u = self.cg.func(FKey::H(H::UnboxI32));
                        self.fb.i().call(u);
                    }
                    Rep::I64 => {
                        self.fb
                            .i()
                            .ref_cast_non_null(ht(t.box_i64))
                            .struct_get(t.box_i64, 0);
                    }
                    Rep::F64 => {
                        self.fb
                            .i()
                            .ref_cast_non_null(ht(t.box_f64))
                            .struct_get(t.box_f64, 0);
                    }
                    Rep::Str => {
                        self.fb.i().ref_cast_non_null(ht(t.str_));
                    }
                    Rep::Any => {}
                }
                self.set(*dst);
            }
            EnvGet { dst, index } => {
                let aget = self.cg.func(FKey::H(H::Aget));
                self.fb
                    .i()
                    .local_get(0)
                    .struct_get(t.clo, 1)
                    .i32_const(*index as i32)
                    .call(aget);
                self.set(*dst);
            }
            DepthEnter => {
                self.fb.i().global_get(g.depth).local_set(self.fdepth);
            }
            DepthCheck => self.depth_check(false),
            DepthPush => self.depth_check(true),
            DepthRestore => {
                self.fb.i().local_get(self.fdepth).global_set(g.depth);
            }
            IntArith { ty, op, dst, a, b } => {
                self.get(*a);
                self.get(*b);
                match (ty, op) {
                    (IntTy::I32, IntOp::Add) => {
                        self.fb.i().i32_add();
                    }
                    (IntTy::I32, IntOp::Sub) => {
                        self.fb.i().i32_sub();
                    }
                    (IntTy::I32, IntOp::Mul) => {
                        self.fb.i().i32_mul();
                    }
                    (IntTy::I64, IntOp::Add) => {
                        self.fb.i().i64_add();
                    }
                    (IntTy::I64, IntOp::Sub) => {
                        self.fb.i().i64_sub();
                    }
                    (IntTy::I64, IntOp::Mul) => {
                        self.fb.i().i64_mul();
                    }
                    (IntTy::I32, IntOp::Div) => {
                        let h = self.cg.func(FKey::H(H::I32Div));
                        self.fb.i().call(h);
                    }
                    (IntTy::I32, IntOp::Rem) => {
                        let h = self.cg.func(FKey::H(H::I32Rem));
                        self.fb.i().call(h);
                    }
                    (IntTy::I64, IntOp::Div) => {
                        let h = self.cg.func(FKey::H(H::I64Div));
                        self.fb.i().call(h);
                    }
                    (IntTy::I64, IntOp::Rem) => {
                        let h = self.cg.func(FKey::H(H::I64Rem));
                        self.fb.i().call(h);
                    }
                };
                self.set(*dst);
            }
            IntNeg { ty, dst, a } => {
                match ty {
                    IntTy::I32 => self.fb.i().i32_const(0),
                    IntTy::I64 => self.fb.i().i64_const(0),
                };
                self.get(*a);
                match ty {
                    IntTy::I32 => self.fb.i().i32_sub(),
                    IntTy::I64 => self.fb.i().i64_sub(),
                };
                self.set(*dst);
            }
            IntCmp { ty, op, dst, a, b } => {
                self.get(*a);
                self.get(*b);
                let mut s = self.fb.i();
                match (ty, op) {
                    (IntTy::I32, CmpOp::Eq) => s.i32_eq(),
                    (IntTy::I32, CmpOp::Lt) => s.i32_lt_s(),
                    (IntTy::I32, CmpOp::Le) => s.i32_le_s(),
                    (IntTy::I32, CmpOp::Gt) => s.i32_gt_s(),
                    (IntTy::I32, CmpOp::Ge) => s.i32_ge_s(),
                    (IntTy::I64, CmpOp::Eq) => s.i64_eq(),
                    (IntTy::I64, CmpOp::Lt) => s.i64_lt_s(),
                    (IntTy::I64, CmpOp::Le) => s.i64_le_s(),
                    (IntTy::I64, CmpOp::Gt) => s.i64_gt_s(),
                    (IntTy::I64, CmpOp::Ge) => s.i64_ge_s(),
                };
                self.set(*dst);
            }
            FloatArith { op, dst, a, b } => {
                self.get(*a);
                self.get(*b);
                let mut s = self.fb.i();
                match op {
                    FloatOp::Add => s.f64_add(),
                    FloatOp::Sub => s.f64_sub(),
                    FloatOp::Mul => s.f64_mul(),
                    FloatOp::Div => s.f64_div(),
                };
                self.set(*dst);
            }
            FloatNeg { dst, a } => {
                self.get(*a);
                self.fb.i().f64_neg();
                self.set(*dst);
            }
            FloatCmp { op, dst, a, b } => {
                self.get(*a);
                self.get(*b);
                let mut s = self.fb.i();
                match op {
                    CmpOp::Eq => s.f64_eq(),
                    CmpOp::Lt => s.f64_lt(),
                    CmpOp::Le => s.f64_le(),
                    CmpOp::Gt => s.f64_gt(),
                    CmpOp::Ge => s.f64_ge(),
                };
                self.set(*dst);
            }
            FloatMath { f, dst, a } => {
                self.get(*a);
                match f {
                    MathFn::Sin => self.fb.i().call(imp::SIN),
                    MathFn::Cos => self.fb.i().call(imp::COS),
                    MathFn::Exp => self.fb.i().call(imp::EXP),
                    MathFn::Log => self.fb.i().call(imp::LOG),
                    MathFn::Sqrt => self.fb.i().f64_sqrt(),
                };
                self.set(*dst);
            }
            FloatTrunc { dst, a } => {
                self.get(*a);
                self.fb.i().f64_trunc();
                self.set(*dst);
            }
            FloatIsNan { dst, a } => {
                self.get(*a);
                self.get(*a);
                self.fb.i().f64_ne();
                self.set(*dst);
            }
            Conv { op, dst, a } => {
                self.get(*a);
                let mut s = self.fb.i();
                match op {
                    ConvOp::I32ToI64 => s.i64_extend_i32_s(),
                    ConvOp::I32ToF64 => s.f64_convert_i32_s(),
                    ConvOp::I64ToI32 => s.i32_wrap_i64(),
                    ConvOp::I64ToF64 => s.f64_convert_i64_s(),
                    ConvOp::F64ToI32 => s.i32_trunc_sat_f64_s(),
                    ConvOp::F64ToI64 => s.i64_trunc_sat_f64_s(),
                };
                self.set(*dst);
            }
            BoolNot { dst, a } => {
                self.get(*a);
                self.fb.i().i32_eqz();
                self.set(*dst);
            }
            BoolEq { dst, a, b } => {
                self.get(*a);
                self.get(*b);
                self.fb.i().i32_eq();
                self.set(*dst);
            }
            StrConcat { dst, a, b } => {
                let h = self.cg.func(FKey::H(H::StrConcat));
                self.get(*a);
                self.get(*b);
                self.fb.i().call(h);
                self.check_set(*dst);
            }
            StrCmp { op, dst, a, b } => {
                let h = self.cg.func(FKey::H(H::StrCmp));
                self.get(*a);
                self.get(*b);
                let mut s = self.fb.i();
                s.call(h);
                match op {
                    CmpOp::Eq => s.i32_eqz(),
                    CmpOp::Lt => s.i32_const(0).i32_lt_s(),
                    CmpOp::Le => s.i32_const(0).i32_le_s(),
                    CmpOp::Gt => s.i32_const(0).i32_gt_s(),
                    CmpOp::Ge => s.i32_const(0).i32_ge_s(),
                };
                self.set(*dst);
            }
            StrLen { dst, a } => {
                self.get(*a);
                self.fb.i().array_len();
                self.set(*dst);
            }
            StrSub { dst, s, pos, len } => {
                let h = self.cg.func(FKey::H(H::StrSub));
                self.get(*s);
                self.get(*pos);
                self.get(*len);
                self.fb.i().call(h);
                self.check_set(*dst);
            }
            Show { op, dst, a } => {
                match op {
                    ShowOp::I32 => {
                        let h = self.cg.func(FKey::H(H::ShowI64));
                        self.get(*a);
                        self.fb.i().i64_extend_i32_s().call(h);
                    }
                    ShowOp::I64 => {
                        let h = self.cg.func(FKey::H(H::ShowI64));
                        self.get(*a);
                        self.fb.i().call(h);
                    }
                    ShowOp::F64 => {
                        let h = self.cg.func(FKey::H(H::ShowF64));
                        self.get(*a);
                        self.fb.i().call(h);
                    }
                    ShowOp::Bool => {
                        let tl = self.tmp(I32);
                        self.get(*a);
                        self.fb
                            .i()
                            .local_set(tl)
                            .global_get(g.str_true)
                            .global_get(g.str_false)
                            .local_get(tl)
                            .typed_select(rv(true, t.str_));
                    }
                }
                self.set(*dst);
            }
            CtorNew {
                dst,
                ty,
                ctor,
                fields,
            } => {
                self.fb.i().i32_const(ty.0 as i32).i32_const(*ctor as i32);
                self.any_array(fields);
                self.fb.i().struct_new(t.data);
                self.set(*dst);
            }
            CtorTag { dst, v } => {
                self.get(*v);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.data))
                    .struct_get(t.data, 1);
                self.set(*dst);
            }
            CtorField { dst, v, index } => {
                let h = self.cg.func(FKey::H(H::CtorField));
                self.get(*v);
                self.fb.i().i32_const(*index as i32).call(h);
                self.set(*dst);
            }
            VariantNew {
                dst,
                label,
                payload,
            } => {
                self.fb.i().i32_const(label.0 as i32);
                self.get(*payload);
                self.fb.i().struct_new(t.variant);
                self.set(*dst);
            }
            VariantTag { dst, v } => {
                self.get(*v);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.variant))
                    .struct_get(t.variant, 0);
                self.set(*dst);
            }
            VariantPayload { dst, v } => {
                self.get(*v);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.variant))
                    .struct_get(t.variant, 1);
                self.set(*dst);
            }
            RecNew { dst, shape, fields } => {
                self.fb.i().i32_const(shape.0 as i32);
                self.any_array(fields);
                self.fb.i().struct_new(t.record);
                self.set(*dst);
            }
            RecGet { dst, rec, index } => {
                let h = self.cg.func(FKey::H(H::RecGet));
                self.get(*rec);
                self.fb.i().i32_const(*index as i32).call(h);
                self.set(*dst);
            }
            RecExtend {
                dst,
                rec,
                label,
                value,
            } => {
                let h = self.cg.func(FKey::H(H::RecExtend));
                self.get(*rec);
                self.fb.i().i32_const(label.0 as i32);
                self.get(*value);
                self.fb.i().call(h);
                self.check_set(*dst);
            }
            RecField {
                dst,
                rec,
                label,
                nth,
            }
            | RecHas {
                dst,
                rec,
                label,
                nth,
            } => {
                let h = if matches!(i, RecField { .. }) {
                    H::RecField
                } else {
                    H::RecHas
                };
                let h = self.cg.func(FKey::H(h));
                self.get(*rec);
                self.fb
                    .i()
                    .i32_const(label.0 as i32)
                    .i32_const(*nth as i32)
                    .call(h);
                self.set(*dst);
            }
            RecRestrict { dst, rec, label } => {
                let h = self.cg.func(FKey::H(H::RecRestrict));
                self.get(*rec);
                self.fb.i().i32_const(label.0 as i32).call(h);
                self.check_set(*dst);
            }
            RecUpdate {
                dst,
                rec,
                label,
                value,
            } => {
                let h = self.cg.func(FKey::H(H::RecUpdate));
                self.get(*rec);
                self.fb.i().i32_const(label.0 as i32);
                self.get(*value);
                self.fb.i().call(h);
                self.set(*dst);
            }
            ClosureNew { dst, func, caps } => {
                let n = self.cg.arity(*func);
                let (_, cty) = self.cg.m.fn_types(n);
                let idx = self.cg.lir(*func);
                self.cg.m.declare_ref(idx);
                self.fb.i().i32_const(func.0 as i32);
                self.any_array(caps);
                self.fb.i().ref_func(idx).struct_new(cty);
                self.set(*dst);
            }
            ClosureSetCap { clo, index, value } => {
                let aset = self.cg.func(FKey::H(H::Aset));
                self.get(*clo);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.clo))
                    .struct_get(t.clo, 1)
                    .i32_const(*index as i32);
                self.get(*value);
                self.fb.i().call(aset);
            }
            MethodNew {
                dst,
                class,
                method,
                dict,
                own,
            } => {
                self.fb
                    .i()
                    .i32_const(class.0 as i32)
                    .i32_const(*method as i32);
                self.get(*dict);
                self.any_array(own);
                self.fb.i().struct_new(t.method);
                self.set(*dst);
            }
            DictInst {
                dst,
                inst,
                premises,
            } => {
                self.fb.i().i32_const(inst.0 as i32);
                self.any_array(premises);
                self.fb.i().struct_new(t.dicti);
                self.set(*dst);
            }
            DictRecord { dst, fields } | DictVariant { dst, fields } => {
                for (l, _) in fields {
                    self.fb.i().i32_const(l.0 as i32);
                }
                self.fb.i().array_new_fixed(t.i32arr, fields.len() as u32);
                let regs: Vec<keleut_lir::Reg> = fields.iter().map(|(_, r)| *r).collect();
                self.any_array(&regs);
                let ty = if matches!(i, DictRecord { .. }) {
                    t.dictr
                } else {
                    t.dictv
                };
                self.fb.i().struct_new(ty);
                self.set(*dst);
            }
            GlobalGet { dst, global } => {
                let gv = self.cg.pglobals[global.index()].0;
                self.fb.i().global_get(gv);
                self.set(*dst);
            }
            GlobalIsInit { dst, global } => {
                let gv = self.cg.pglobals[global.index()].1;
                self.fb.i().global_get(gv);
                self.set(*dst);
            }
            GlobalSet { global, src } => {
                let gv = self.cg.pglobals[global.index()].0;
                self.get(*src);
                self.fb.i().global_set(gv);
            }
            GlobalMarkInit { global } => {
                let gv = self.cg.pglobals[global.index()].1;
                self.fb.i().i32_const(1).global_set(gv);
            }
            InstIsInit { dst, inst } => {
                let gv = self.cg.inst_init[inst.index()];
                self.fb.i().global_get(gv);
                self.set(*dst);
            }
            InstMarkInit { inst } => {
                let gv = self.cg.inst_init[inst.index()];
                self.fb.i().i32_const(1).global_set(gv);
            }
            InstSetValue { inst, method, src } => {
                let gv = self.cg.inst_vals[inst.index()][*method as usize];
                self.get(*src);
                self.fb.i().global_set(gv);
            }
            InstGetValue { dst, inst, method } => {
                let gv = self.cg.inst_vals[inst.index()][*method as usize];
                self.fb.i().global_get(gv);
                self.set(*dst);
            }
            RefNew { dst, value } => {
                self.get(*value);
                self.fb.i().struct_new(t.ref_);
                self.set(*dst);
            }
            RefGet { dst, r } => {
                self.get(*r);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.ref_))
                    .struct_get(t.ref_, 0);
                self.set(*dst);
            }
            RefSet { r, value } => {
                self.get(*r);
                self.fb.i().ref_cast_non_null(ht(t.ref_));
                self.get(*value);
                self.fb.i().struct_set(t.ref_, 0);
            }
            ArrayLen { dst, a } | MArrayLen { dst, a } => {
                self.get(*a);
                self.fb.i().ref_cast_non_null(ht(t.anyarr)).array_len();
                self.set(*dst);
            }
            ArrayGet { dst, a, index } | MArrayGet { dst, a, index } => {
                let aget = self.cg.func(FKey::H(H::Aget));
                self.get(*a);
                self.fb.i().ref_cast_non_null(ht(t.anyarr));
                self.get(*index);
                self.fb.i().call(aget);
                self.set(*dst);
            }
            MArraySet { a, index, value } => {
                let aset = self.cg.func(FKey::H(H::Aset));
                self.get(*a);
                self.fb.i().ref_cast_non_null(ht(t.anyarr));
                self.get(*index);
                self.get(*value);
                self.fb.i().call(aset);
            }
            MArrayNew { dst, len, init } => {
                let h = self.cg.func(FKey::H(H::MArrayNew));
                self.get(*len);
                self.get(*init);
                self.fb.i().call(h);
                self.check_set(*dst);
            }
            MArrayFreeze { dst, a } => {
                let h = self.cg.func(FKey::H(H::Freeze));
                self.get(*a);
                self.fb.i().ref_cast_non_null(ht(t.anyarr)).call(h);
                self.set(*dst);
            }
            ConsoleWrite { dst, s } => {
                let h = self.cg.func(FKey::H(H::Write));
                self.get(*s);
                self.fb.i().call(h);
                self.prop_if();
                self.fb.i().global_get(g.unit);
                self.set(*dst);
            }
            UnimplementedValue { what, .. } => {
                rt::unimpl(self.cg, &mut self.fb, what);
            }
            Fx { op, dst, args } => self.fx(op, *dst, args)?,
            CallDirect { .. } | CallValue { .. } | CallMethod { .. } | DictApp { .. } => {
                return Err("呼び出しの命令は call_pre が生成する".into());
            }
        }
        Ok(())
    }

    /// 呼び出しの前半を生成する。結果(`eqref`。null は番兵)を積むところまでである。
    fn call_pre(&mut self, i: &Inst) -> Result<(), String> {
        let t = self.t();
        let g = self.cg.g;
        match i {
            Inst::CallDirect { func, args, .. } => {
                let idx = self.cg.lir(*func);
                self.fb.i().ref_null(ht(t.clo));
                for a in args {
                    self.get(*a);
                }
                self.cg.pack_args(&mut self.fb, args.len() as u32);
                self.fb.i().ref_null(ht(t.frame)).call(idx);
            }
            Inst::CallValue { callee, args, .. } => {
                self.call_value(*callee, args, false);
            }
            Inst::CallMethod {
                class,
                method,
                dict,
                own,
                args,
                ..
            } => {
                let h = self.cg.func(FKey::CallM(
                    class.0,
                    *method,
                    own.len() as u32,
                    args.len() as u32,
                ));
                self.get(*dict);
                for r in own.iter().chain(args) {
                    self.get(*r);
                }
                self.cg.pack_args(&mut self.fb, args.len() as u32);
                self.fb.i().call(h);
            }
            Inst::DictApp { f, dicts, .. } => {
                let h = self.cg.func(FKey::DictApp(dicts.len() as u32));
                self.get(*f);
                for r in dicts {
                    self.get(*r);
                }
                self.cg.pack_args(&mut self.fb, dicts.len() as u32);
                self.fb.i().call(h);
            }
            Inst::Fx { op, args, .. } => match op {
                FxOp::Perform { effect, op } => {
                    let (hr, user) = (args[0], &args[2..]);
                    let find = self.cg.func(FKey::H(H::ClauseFind));
                    self.depth_check(false);
                    let tj = self.tmp(I32);
                    self.hr_cast(hr);
                    self.fb
                        .i()
                        .i32_const(*op as i32)
                        .i32_const(0)
                        .call(find)
                        .local_tee(tj)
                        .i32_const(0)
                        .i32_lt_s()
                        .if_(BlockType::Empty);
                    self.extra += 1;
                    self.no_clause(*effect, *op, user);
                    self.fb.i().end();
                    self.extra -= 1;
                    self.hr_cast(hr);
                    self.fb
                        .i()
                        .struct_get(t.hr, hrf::PARENT)
                        .global_set(g.chain)
                        .global_get(g.depth)
                        .i64_const(1)
                        .i64_add()
                        .global_set(g.depth);
                    self.call_clause(hr, tj, user, false);
                }
                FxOp::CallBody => {
                    let callv = self.cg.func(FKey::CallV(1));
                    self.hr_cast(args[0]);
                    self.fb.i().struct_get(t.hr, hrf::BODY);
                    self.hr_cast(args[0]);
                    self.fb.i().struct_get(t.hr, hrf::RES).call(callv);
                }
                FxOp::CallCancel => {
                    let callv = self.cg.func(FKey::CallV(0));
                    self.get(args[0]);
                    self.fb.i().call(callv);
                }
                FxOp::ContResume => {
                    let h = self.cg.func(FKey::H(H::RtResume));
                    self.get(args[0]);
                    self.get(args[1]);
                    self.fb.i().local_get(self.fdepth).call(h);
                }
                FxOp::CallClause => {
                    // 深さ h + 1 が上限を超えたら、節を呼ばずに ERROR を設定し、番兵を結果にする。
                    let raise = self.cg.func(FKey::H(H::RaiseDepth));
                    let ck = self.cg.func(FKey::ClauseK);
                    self.fb
                        .i()
                        .local_get(self.fdepth)
                        .i64_const(1)
                        .i64_add()
                        .global_get(g.limit)
                        .i64_gt_u()
                        .if_(BlockType::Result(EQ))
                        .call(raise)
                        .ref_null(HEQ)
                        .else_()
                        .local_get(self.fdepth)
                        .i64_const(1)
                        .i64_add()
                        .global_set(g.depth);
                    self.get(args[0]);
                    self.fb.i().call(ck).end();
                }
                _ => return Err(format!("呼び出しでないエフェクトの命令 {op:?}")),
            },
            _ => return Err("呼び出しでない命令".into()),
        }
        Ok(())
    }

    /// 呼び出しの後半を生成する。積んである結果を受け取る。
    ///
    /// 通常の呼び出しでは、結果が番兵なら伝播する。
    /// そのとき状態ワードが `YIELD` なら、yield を受け取る再開点ではフレームの記録を作って並びに追加し、
    /// 受け取らない再開点では内部エラーで終了する。
    /// 番兵を受け取る呼び出しでは、結果が番兵でもレジスタに書いて、次の命令へ進む。
    fn call_post(&mut self, i: &Inst, mode: Mode, site: Option<Site>) -> Result<(), String> {
        let g = self.cg.g;
        match i {
            Inst::Fx {
                op: FxOp::Perform { .. },
                dst,
                args,
            } => {
                let dst = dst.ok_or("perform の結果")?;
                // `perform` が再開点の表にないのは、G の節がプログラムにないときだけである。
                if site.is_none() && self.cg.has_g {
                    return Err("perform が再開点の表にありません".into());
                }
                let tr = self.tmp(EQ);
                if mode == Mode::PostY {
                    // 継続の再開。再開の値が perform の結果になる(連鎖と深さは rt_resume が復元した)。
                    // 番兵なら、注入された状態ワードが設定されているので、そのまま伝播する。
                    self.fb
                        .i()
                        .local_tee(tr)
                        .ref_is_null()
                        .if_(BlockType::Empty);
                    self.prop_br();
                    self.fb.i().end().local_get(tr);
                    self.set(dst);
                    return Ok(());
                }
                let (hr, saved, user) = (args[0], args[1], &args[2..]);
                let unwound = self.cg.func(FKey::H(H::PerformUnwound));
                self.fb
                    .i()
                    .local_set(tr)
                    .local_get(self.fdepth)
                    .global_set(g.depth)
                    .local_get(tr)
                    .ref_is_null()
                    .if_(BlockType::Empty);
                self.extra += 1;
                self.get(hr);
                self.get(saved);
                self.any_array(user);
                self.fb.i().call(unwound);
                if let Some(site) = site {
                    self.fb.i().if_(BlockType::Empty);
                    // この perform の G の節が選ばれた場合: 継続をキャプチャした後に再開する区切りを再開先にして、記録を作る。
                    self.save(site.posty.ok_or("perform の再開の区切りがありません")?);
                    self.fb.i().else_();
                    // 節の中からほかのハンドラ宛ての YIELD を受け取った場合: 節の呼び出しの後半を再開先にして、記録を作る。
                    self.save_if_yield(site.post);
                    self.fb.i().end();
                } else {
                    // G の節がないので、結果は常に 0 である。
                    self.fb.i().drop();
                    self.fatal_if_yield();
                }
                self.prop_br();
                self.fb.i().end();
                self.extra -= 1;
                self.get(saved);
                self.fb.i().global_set(g.chain).local_get(tr);
                self.set(dst);
            }
            Inst::Fx { op, dst, .. } if op.catches() => {
                let dst = dst.ok_or("番兵を受け取る呼び出しの結果")?;
                if let (Some(l), Some(s)) = (self.lastrp, site) {
                    self.fb.i().i32_const(s.post as i32);
                    self.set_loc(l, I32);
                }
                self.catch_after(dst);
            }
            Inst::CallDirect { dst, .. }
            | Inst::CallValue { dst, .. }
            | Inst::CallMethod { dst, .. }
            | Inst::DictApp { dst, .. } => {
                let tl = self.tmp(EQ);
                self.fb
                    .i()
                    .local_tee(tl)
                    .ref_is_null()
                    .if_(BlockType::Empty);
                match site {
                    Some(s) => self.save_if_yield(s.post),
                    None => self.fatal_if_yield(),
                }
                self.prop_br();
                self.fb.i().end().local_get(tl);
                self.set(*dst);
            }
            _ => return Err("呼び出しでない命令".into()),
        }
        Ok(())
    }

    fn hr_cast(&mut self, r: keleut_lir::Reg) {
        let t = self.t();
        self.get(r);
        self.fb.i().ref_cast_non_null(ht(t.hr));
    }

    /// 番兵を受け取る呼び出しの後の処理を生成する。結果をレジスタ `dst` に書き、null なら深さを書き戻す。
    fn catch_after(&mut self, dst: keleut_lir::Reg) {
        let g = self.cg.g;
        self.set(dst);
        self.get(dst);
        self.fb
            .i()
            .ref_is_null()
            .if_(BlockType::Empty)
            .local_get(self.fdepth)
            .global_set(g.depth)
            .end();
    }

    /// 操作に一致する節がないときの実行時エラー(IRSpec 付録 B の 9 番)を設定して、伝播する。
    ///
    /// `args` は操作の引数である。
    fn no_clause(&mut self, effect: u32, op: u32, args: &[keleut_lir::Reg]) {
        let prog = self.cg.prog;
        let eff = &prog.effects[effect as usize];
        let text = format!(
            "handle のどの節にも一致しません: {}.{}",
            eff.display, eff.ops[op as usize].0
        );
        let tp = self.cg.tpl.add(kind::RT, arg::ARGS, text);
        let raise = self.cg.func(FKey::H(H::Raise));
        self.fb.i().i32_const(tp as i32);
        self.any_array(args);
        self.fb.i().i64_const(0).call(raise);
        self.prop_br();
    }

    /// 節の候補 `tj`(`i32` のローカル)の関数を、引数 `args`、`hr`、`tj` で呼ぶ。`tail` なら末尾呼び出しで呼ぶ。
    fn call_clause(&mut self, hr: keleut_lir::Reg, tj: u32, args: &[keleut_lir::Reg], tail: bool) {
        let t = self.t();
        let a = args.len() as u32 + 2;
        let (fty, cty) = self.cg.m.fn_types(a);
        let aget = self.cg.func(FKey::H(H::Aget));
        // 一時ローカルは基底型 `$Clo` のもの 1 つを共用し、関数の参照を読むときに型を絞り直す。
        let c = self.tmp(rv(true, t.clo));
        self.hr_cast(hr);
        self.fb
            .i()
            .struct_get(t.hr, hrf::CLAUSES)
            .local_get(tj)
            .call(aget)
            .ref_cast_non_null(ht(cty))
            .local_tee(c);
        for r in args {
            self.get(*r);
        }
        self.get(hr);
        self.fb.i().local_get(tj).ref_i31();
        self.cg.pack_args(&mut self.fb, a);
        self.fb
            .i()
            .ref_null(ht(t.frame))
            .local_get(c)
            .ref_cast_non_null(ht(cty))
            .struct_get(cty, 2);
        if tail {
            self.fb.i().return_call_ref(fty);
        } else {
            self.fb.i().call_ref(fty);
        }
    }

    fn fx(
        &mut self,
        op: &FxOp,
        dst: Option<keleut_lir::Reg>,
        args: &[keleut_lir::Reg],
    ) -> Result<(), String> {
        let t = self.t();
        let g = self.cg.g;
        let out = |s: &mut Self| dst.ok_or("結果のないエフェクトの命令").map(|d| s.set(d));
        match op {
            FxOp::HandlerNew { effect, ops, .. } => {
                let nops = ops.len();
                let tl = self.tmp(I64);
                self.fb
                    .i()
                    .global_get(g.next_hid)
                    .local_tee(tl)
                    .i64_const(1)
                    .i64_add()
                    .global_set(g.next_hid)
                    .local_get(tl)
                    .i32_const(*effect as i32);
                self.get(args[0]);
                self.get(args[1]);
                for (o, _) in ops {
                    self.fb.i().i32_const(*o as i32);
                }
                self.fb.i().array_new_fixed(t.i32arr, nops as u32);
                for (_, c) in ops {
                    let c = match c {
                        keleut_lir::ClauseClass::T => cls::T,
                        keleut_lir::ClauseClass::N => cls::N,
                        keleut_lir::ClauseClass::G => cls::G,
                    };
                    self.fb.i().i32_const(c);
                }
                self.fb.i().array_new_fixed(t.i32arr, nops as u32);
                self.any_array(&args[2..2 + nops]);
                let (has_ret, has_cancel) = match op {
                    FxOp::HandlerNew {
                        has_ret,
                        has_cancel,
                        ..
                    } => (*has_ret, *has_cancel),
                    _ => unreachable!(),
                };
                let mut at = 2 + nops;
                if has_ret {
                    self.get(args[at]);
                    at += 1;
                } else {
                    self.fb.i().ref_null(HEQ);
                }
                if has_cancel {
                    self.get(args[at]);
                } else {
                    self.fb.i().ref_null(HEQ);
                }
                self.fb.i().global_get(g.unit).i64_const(0).struct_new(t.hr);
                out(self)?;
            }
            FxOp::HandlerFind { effect, op } => {
                let prog = self.cg.prog;
                let e = &prog.effects[*effect as usize];
                let tp = self.cg.tpl.add(
                    kind::UNHANDLED,
                    arg::NONE,
                    format!("{}.{}", e.display, e.ops[*op as usize].0),
                );
                let hfind = self.cg.func(FKey::H(H::HandlerFind));
                let raise = self.cg.func(FKey::H(H::Raise));
                self.fb
                    .i()
                    .block(BlockType::Result(ValType::Ref(RefType {
                        nullable: false,
                        heap_type: HEQ,
                    })))
                    .i32_const(*effect as i32)
                    .call(hfind)
                    .br_on_non_null(0)
                    .i32_const(tp as i32)
                    .ref_null(HEQ)
                    .i64_const(0)
                    .call(raise);
                self.extra += 1;
                self.prop_br();
                self.fb.i().end();
                self.extra -= 1;
                out(self)?;
            }
            FxOp::HandlersGet => {
                self.fb.i().global_get(g.chain);
                out(self)?;
            }
            FxOp::Perform { .. }
            | FxOp::CallBody
            | FxOp::CallCancel
            | FxOp::ContResume
            | FxOp::CallClause => {
                return Err(format!(
                    "呼び出しのエフェクトの命令 {op:?} は call_pre が生成する"
                ));
            }
            FxOp::ContCheck { nontail } => {
                // nontail なら、節の外での再開(IRSpec 付録 B の 8 番)、再開済み(7 番)、
                // c + Δ が上限を超えること(depth_limit)を調べる。
                // tail なら、再開済みかどうかだけを調べる。
                let k = args[0];
                if *nontail {
                    let tp = self.cg.tpl.add(
                        kind::RT,
                        arg::NONE,
                        "resume を節の外で呼び出しました(second-class)",
                    );
                    self.get(k);
                    self.fb
                        .i()
                        .ref_cast_non_null(ht(t.cont))
                        .struct_get(t.cont, ctf::ALIVE)
                        .i32_eqz()
                        .if_(BlockType::Empty);
                    self.raise_with(tp, None, None);
                    self.fb.i().end();
                }
                let tp = self.cg.tpl.add(
                    kind::RT,
                    arg::NONE,
                    "resume は高々1回しか呼べません(アフィン)",
                );
                self.get(k);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.cont))
                    .struct_get(t.cont, ctf::USED)
                    .if_(BlockType::Empty);
                self.raise_with(tp, None, None);
                self.fb.i().end();
                if *nontail {
                    let raise = self.cg.func(FKey::H(H::RaiseDepth));
                    self.fb.i().local_get(self.fdepth);
                    self.get(k);
                    self.fb
                        .i()
                        .ref_cast_non_null(ht(t.cont))
                        .struct_get(t.cont, ctf::DELTA)
                        .i64_add()
                        .global_get(g.limit)
                        .i64_gt_u()
                        .if_(BlockType::Empty)
                        .call(raise);
                    self.prop_br();
                    self.fb.i().end();
                }
            }
            FxOp::ContHr => {
                self.get(args[0]);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.cont))
                    .struct_get(t.cont, ctf::HR);
                out(self)?;
            }
            FxOp::ContMake => {
                let h = self.cg.func(FKey::H(H::ContMake));
                self.get(args[0]);
                self.fb.i().local_get(self.fdepth).call(h);
                out(self)?;
            }
            FxOp::ContKill => {
                self.get(args[0]);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.cont))
                    .i32_const(0)
                    .struct_set(t.cont, ctf::ALIVE);
            }
            FxOp::ContUsed => {
                self.get(args[0]);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.cont))
                    .struct_get(t.cont, ctf::USED);
                out(self)?;
            }
            FxOp::IsCont => {
                self.get(args[0]);
                self.fb.i().ref_test_non_null(ht(t.cont));
                out(self)?;
            }
            FxOp::HandlerInstall => {
                self.hr_cast(args[0]);
                self.fb
                    .i()
                    .global_get(g.chain)
                    .struct_set(t.hr, hrf::PARENT);
                self.get(args[0]);
                self.fb.i().global_set(g.chain);
            }
            FxOp::HandlerUninstall => {
                self.hr_cast(args[0]);
                self.fb
                    .i()
                    .struct_get(t.hr, hrf::PARENT)
                    .global_set(g.chain);
            }
            FxOp::IsPending => {
                self.get(args[0]);
                self.fb.i().ref_is_null();
                out(self)?;
            }
            FxOp::PendingClass => {
                let h = self.cg.func(FKey::H(H::PendingClass));
                self.get(args[0]);
                self.fb.i().call(h);
                out(self)?;
            }
            FxOp::PendingTake => {
                let h = self.cg.func(FKey::H(H::PendingTake));
                self.fb.i().call(h);
                out(self)?;
            }
            FxOp::PendingRestore => {
                let h = self.cg.func(FKey::H(H::PendingRestore));
                self.get(args[0]);
                self.fb.i().call(h);
            }
            FxOp::PendingValue => {
                self.get(args[0]);
                self.fb
                    .i()
                    .ref_cast_non_null(ht(t.saved))
                    .struct_get(t.saved, svf::VAL);
                out(self)?;
            }
            FxOp::PendingDiscard => {
                self.fb.i().i32_const(pk::DISCARD_VALUE);
                self.get(args[0]);
                self.get(args[1]);
                self.fb
                    .i()
                    .i32_const(0)
                    .ref_null(HEQ)
                    .i32_const(0)
                    .struct_new(t.saved);
                out(self)?;
            }
            FxOp::PendingSuppress => {
                let h = self.cg.func(FKey::H(H::Suppress));
                self.fb.i().call(h);
            }
            FxOp::HandlerHas(part) | FxOp::HandlerGet(part) => {
                let fld = match part {
                    HandlerPart::Ret => hrf::RET,
                    HandlerPart::Cancel => hrf::CANCEL,
                };
                self.hr_cast(args[0]);
                self.fb.i().struct_get(t.hr, fld);
                if matches!(op, FxOp::HandlerHas(_)) {
                    self.fb.i().ref_is_null().i32_eqz();
                }
                out(self)?;
            }
            FxOp::LimitGet => {
                self.fb.i().global_get(g.limit);
                out(self)?;
            }
            FxOp::LimitSet => {
                self.get(args[0]);
                self.fb.i().global_set(g.limit);
            }
        }
        Ok(())
    }

    fn term(&mut self, term: &Term) -> Result<(), String> {
        let t = self.t();
        let g = self.cg.g;
        match term {
            Term::Jump(b) => {
                let j = self.lay.first[b.index()];
                self.goto(j);
            }
            Term::Branch { cond, then, else_ } => {
                let th = self.lay.first[then.index()];
                let el = self.lay.first[else_.index()];
                let next = self.cur + 1;
                self.get(*cond);
                if !(self.in_part(th) && self.in_part(el)) {
                    self.fb.i().if_(BlockType::Empty);
                    self.extra += 1;
                    self.branch_to(th);
                    self.extra -= 1;
                    self.fb.i().end();
                    self.goto(el);
                } else if th == next && el != next {
                    let d = self.depth_to(el);
                    self.fb.i().i32_eqz().br_if(d);
                } else {
                    let d = self.depth_to(th);
                    self.fb.i().br_if(d);
                    self.goto(el);
                }
            }
            Term::Return(r) => {
                self.get(*r);
                self.fb.i().return_();
            }
            Term::TailCallDirect { func, args, .. } => {
                let idx = self.cg.lir(*func);
                self.fb.i().ref_null(ht(t.clo));
                for a in args {
                    self.get(*a);
                }
                self.cg.pack_args(&mut self.fb, args.len() as u32);
                self.fb.i().ref_null(ht(t.frame)).return_call(idx);
            }
            Term::TailCallValue { callee, args, .. } => {
                self.call_value(*callee, args, true);
            }
            Term::TailCallMethod {
                class,
                method,
                dict,
                own,
                args,
                ..
            } => {
                let h = self.cg.func(FKey::CallM(
                    class.0,
                    *method,
                    own.len() as u32,
                    args.len() as u32,
                ));
                self.get(*dict);
                for r in own.iter().chain(args) {
                    self.get(*r);
                }
                self.cg.pack_args(&mut self.fb, args.len() as u32);
                self.fb.i().return_call(h);
            }
            Term::Error { kind: k, operand } => {
                let prog = self.cg.prog;
                let tp = meta::error_template(&mut self.cg.tpl, prog, k);
                match k {
                    RtError::F64ToIntRange(_) => self.raise_with(tp, None, *operand),
                    _ => self.raise_with(tp, *operand, None),
                }
            }
            Term::Unimplemented { what } => {
                rt::unimpl(self.cg, &mut self.fb, what);
            }
            Term::Fx { op, args } => match op {
                FxTermOp::Propagate => {
                    // ほかのハンドラ宛ての YIELD なら、自分のフレームも継続に含める。
                    // 再開されたら、最後に番兵を受け取った呼び出しの結果を受け取るところから続ける。
                    if let (Some(l), true) = (self.lastrp, self.rec.is_some()) {
                        self.fb
                            .i()
                            .global_get(g.p_kind)
                            .i32_const(pk::YIELD)
                            .i32_eq()
                            .if_(BlockType::Empty);
                        for j in self.catch_posts.clone().iter().copied() {
                            self.get_loc(l);
                            self.fb
                                .i()
                                .i32_const(j as i32)
                                .i32_eq()
                                .if_(BlockType::Empty);
                            self.save(j);
                            self.fb.i().end();
                        }
                        self.fb.i().end();
                    }
                    self.prop_br();
                }
                FxTermOp::TailCallNever => {
                    let h = self.cg.func(FKey::TailNever);
                    self.get(args[0]);
                    self.fb.i().return_call(h);
                }
                FxTermOp::ClauseValue | FxTermOp::TailResume => {
                    let k = if matches!(op, FxTermOp::ClauseValue) {
                        pk::CLAUSE_VALUE
                    } else {
                        pk::TAIL_RESUME
                    };
                    self.get(args[0]);
                    self.fb
                        .i()
                        .global_set(g.p_val)
                        .i32_const(k)
                        .global_set(g.p_kind);
                    self.prop_br();
                }
                FxTermOp::ClauseMatched => {
                    self.get(args[0]);
                    self.fb
                        .i()
                        .ref_cast_non_null(HI31)
                        .i31_get_s()
                        .global_set(g.p_clause)
                        .i32_const(pk::MATCHED)
                        .global_set(g.p_kind);
                    self.prop_br();
                }
                FxTermOp::TailCallClause { op } => {
                    let (hr, sel, user) = (args[0], args[1], &args[2..]);
                    let find = self.cg.func(FKey::H(H::ClauseFind));
                    let tj = self.tmp(I32);
                    self.hr_cast(hr);
                    self.fb.i().i32_const(*op as i32);
                    self.get(sel);
                    self.fb
                        .i()
                        .ref_cast_non_null(HI31)
                        .i31_get_s()
                        .i32_const(1)
                        .i32_add()
                        .local_tee(tj)
                        .i32_const(0)
                        .local_get(tj)
                        .i32_const(0)
                        .i32_gt_s()
                        .select()
                        .call(find)
                        .local_tee(tj)
                        .i32_const(0)
                        .i32_lt_s()
                        .if_(BlockType::Empty);
                    self.extra += 1;
                    // 操作のエフェクトはハンドラ記録が持つ(候補がないときのメッセージに使う)。
                    self.no_clause_dyn(hr, *op, user);
                    self.fb.i().end();
                    self.extra -= 1;
                    self.call_clause(hr, tj, user, true);
                }
            },
        }
        Ok(())
    }

    /// 選択フェーズで候補がなくなったときの実行時エラーを設定して、伝播する。
    ///
    /// エフェクトはハンドラ記録から読む。
    fn no_clause_dyn(&mut self, hr: keleut_lir::Reg, op: u32, args: &[keleut_lir::Reg]) {
        let t = self.t();
        let prog = self.cg.prog;
        // エフェクトごとにテンプレートを作り、ハンドラ記録のエフェクトの番号で選ぶ。
        let effs: Vec<u32> = (0..prog.effects.len() as u32)
            .filter(|e| (op as usize) < prog.effects[*e as usize].ops.len())
            .collect();
        for e in effs {
            self.hr_cast(hr);
            self.fb
                .i()
                .struct_get(t.hr, hrf::EFFECT)
                .i32_const(e as i32)
                .i32_eq()
                .if_(BlockType::Empty);
            self.extra += 1;
            self.no_clause(e, op, args);
            self.fb.i().end();
            self.extra -= 1;
        }
        rt::fatal(
            self.cg,
            &mut self.fb,
            "ハンドラ記録のエフェクトが表にありません",
        );
    }
}
