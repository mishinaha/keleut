//! ネイティブバックエンド(LLVM の JIT)とインタプリタの境界。
//!
//! JIT の関数は 2 つのエントリを持つ。
//! 値スタック渡しのエントリ `tailcc i64 @f(ptr %ctx)` は、引数を値スタックのスロットで受け取る。
//! レジスタ渡しのエントリ `tailcc i64 @f.r(ptr %ctx, i64 %env, i64 %x0, …, i64 %x3)` は、環境と先頭の [`REG_ARGS`] 個の引数をレジスタで受け取る。
//! 返り値はどちらも、一様表現の値(Rep `any` の値を持つ 1 ワード)か、番兵(`0x2`。状態ワードが設定されていることを表す値で、`doc/lir.md` の 16.1 節が定める)である。
//! `ctx` は [`NativeCtx`] で、呼び出し先の関数の番号と、呼び出し先のフレームの位置を伝える。
//! フレームは分割値スタックに置き、レイアウトはインタプリタと同じである(`keleut_rt::stack`)。
//!
//! ## 呼び出し規約
//!
//! 呼び出し元は、まず呼び出し先のフレームを置く。
//! JIT のコードは、[`krt_prep`] を呼ぶか、その場で置く(後で述べる高速経路)。
//! インタプリタは自分で置く。
//! 次に、[`NativeCtx`] の `callee`、`cur_pos`、`cur_fp` を書く。
//! 値スタック渡しのエントリ(エントリテーブル `entry[callee]`)を呼ぶときは、引数をすべてスロットに書き、環境もコンテキストの `env` に書く。
//! JIT の関数どうしの直接呼び出しとクロージャの呼び出しは、レジスタ渡しのエントリテーブル `rentry[callee]` を呼ぶ。
//! このときは、先頭の [`REG_ARGS`] 個の引数と環境をレジスタで渡し、残りの引数だけをスロットに書く。
//! レジスタ渡しのエントリテーブルの項目は、はじめスタブ `interp_stub_r` を指す。
//! `interp_stub_r` は、[`krt_store_regs`] でレジスタの引数をスロットに、環境をコンテキストに書いてから、`interp_stub` を `musttail` で呼ぶ。
//! 値スタック渡しのエントリは、スロットとコンテキストから引数と環境を読んで、レジスタ渡しのエントリに制御を移す。
//! レジスタ渡しのエントリは、引数をスロットに書かずに実行を始める。
//! インタプリタでの実行に切り替えるとき(ネイティブスタックの残りが少ないときと、`alternating` で選ばれたとき)は、レジスタの引数をスロットに、環境をコンテキストに書いてから切り替える。
//! `$rt_handle` は 0 番のスロットがどの点でもルートなので(`doc/lir.md` の 1.8 節)、入口でレジスタの引数をスロットに書く。
//! それ以外の関数は、生存しているレジスタを再開点と安全点の前にスロットに書くので、入口では書かなくてよい。
//!
//! 呼び出し先のフレームが、値スタックの使用中の最後のチャンク(コンテキストの `win`)に収まるとき、JIT のコードは [`krt_prep`] を呼ばずに、その場でフレームヘッダとコンテキストのフィールドを書く。
//! これを、フレームを置く高速経路(fast path)と呼ぶ。
//! 高速経路は、自分のフレームも同じチャンクにあることを検査するので、フレームを置く位置は [`krt_prep`] と同じになる。
//! 呼び出し先が実行時に決まるクロージャの呼び出しでは、呼び出し先のフレームのワード数をコンテキストの `frames` の配列から読む。
//! 末尾呼び出しは、呼び出し先のフレームを自分のフレームの位置に置いて、`musttail` で呼ぶ。
//!
//! `box`、`ctor_new`、`rec_new` が作る小さなオブジェクトは、JIT のコードがランタイム関数を呼ばずに確保する。
//! コンテキストの `heap` が指すヒープの空きリストかバンプポインタから、セルを 1 個確保する。
//! これを、確保の高速経路と呼ぶ(`keleut_rt::heap` のモジュールの説明を参照)。
//! 次のどれかに当てはまるときは、その場で確保せずにランタイム関数を呼ぶ。
//!
//! - 実行スレッドのフラグ(`keleut_rt::alloc`)が立っている。
//! - 前の GC の後に確保した量が、GC を始めるしきい値に達している。
//! - 空きリストにも、バンプ割り当て中のページにも、空きセルがない。
//!
//! 確保の高速経路は GC を起こさないので、安全点を記録しない。
//! これは、フレームを置く高速経路が [`krt_prep`] を呼ばないのと同じである。
//! ランタイム関数を呼ぶ経路だけが安全点を記録する。
//! `--gc-stress` のときは、確保の高速経路を生成しない。
//!
//! フレームヘッダの 3 番のワードには、どの境界でも、実際の呼び出し元のフレームの位置を書く。
//! そのため、GC と継続の処理は、インタプリタと JIT がネイティブスタックの上で入れ子になっていても、値スタックの呼び出し元を順にたどるだけでよい。
//! この連鎖が [`keleut_rt::stack::NATIVE_CALLER`] で終わるのは、Rust から [`Interp::run`] で入ったフレームだけである。
//! ネイティブバックエンドを付けた実行で [`Interp::run`] を呼ぶのは、トップレベルの初期化コードの実行と、構造的な `Eq` の比較(入れ子に呼ぶ)である。
//!
//! エントリテーブルの項目は、コンパイル済みの JIT の関数か、インタプリタに入るスタブ `interp_stub` を指す。
//! `interp_stub` は [`krt_interp_run`] を呼び、インタプリタのディスパッチループを新しく呼び出す。
//! その呼び出しの最下段のフレームが JIT の関数を末尾呼び出しで呼ぶと、ディスパッチループは [`TAILREQ`] を返し、スタブがその関数を `musttail` で呼ぶ。
//! インタプリタが JIT の関数を非末尾呼び出しで呼ぶときは、スタブ `enter_tailcc`(呼び出し規約は `ccc`)を通して、通常の呼び出しで呼ぶ。
//!
//! ## 番兵とエフェクト
//!
//! JIT の関数は、通常の呼び出しから番兵が返ったら、コンテキストの深さを自分のフレームの深さに書き戻して、自分も番兵を返す([`krt_sentinel`])。
//! 状態ワード(`doc/lir.md` の 16.1 節)が `YIELD` のときは、フレームは値スタックに残り、継続にコピーされる。
//! 値スタックに復元した継続のフレームは、インタプリタが続きを実行する(脱最適化)。
//! そのため JIT のコードは、非末尾呼び出しの前に、生存しているレジスタをスロットに書き、フレームヘッダに再開点を書く。
//!
//! `handle`、`resume`、`perform` を LIR に変換して得られるエフェクトの命令と終端命令(`doc/lir.md` の 16 節)を含む関数も、JIT でコンパイルする。
//! エフェクトの命令では、JIT のコードがオペランドをスロットに書いてからランタイム関数 [`krt_fx`] を呼び、[`krt_fx`] がインタプリタの同じ命令を 1 つだけ実行する。
//! 呼び出しの一種である命令(`perform` と、番兵を受け取る呼び出し)では、[`krt_fx`] が呼び出し先のフレームを置き、JIT のコードがエントリテーブルを通して呼ぶ。
//! 番兵を受け取る呼び出しは、番兵が返ったら深さを書き戻して、番兵を命令の結果にする。
//! `perform` は、返り値を [`krt_perform_back`] に渡す。
//! [`krt_perform_back`] は、番兵が返っていれば、節の選択の結果を状態ワードに設定する。
//! 継続の再開(`cont_resume`)は、値スタックに復元した区間(継続にしたフレームの範囲)を [`krt_fx`] の中のインタプリタで実行し、その結果を呼び出しの結果にする。
//! JIT のコードは、フレームを再開点から続けるためのエントリを持たないからである。
//! エフェクトの終端命令は [`krt_fx_term`] を呼ぶ。
//! 末尾呼び出しの終端命令では、[`krt_fx_term`] が置いたフレームを `musttail` で呼ぶ。
//!
//! 巻き戻さずに終了するエラー(未実装、内部エラー、防壁の flush の失敗)は、[`Native::fatal`] に保存して番兵を返す。
//! 防壁は、`cancel` 節の実行中に、伝播中の状態を退避して守る手順である(`doc/lir.md` の 16.4 節)。
//! インタプリタは、ネイティブの呼び出しから戻るたびに [`Native::fatal`] を調べ、エラーがあれば Rust の `Err` として返して終了する。
//!
//! ## ティアの切り替え
//!
//! 関数ごとに、エントリテーブルの項目と状態を持つ。
//! エントリテーブルの項目は、はじめ `interp_stub` を指す。
//! コンパイルスレッドは、関数のコンパイルを終えたときに、その項目を 1 回だけアトミックに書き換える。
//! ティアの設定([`TierMode`]。コマンド行では `--tier`)は次のとおりである。
//!
//! | 設定 | コンパイルする関数 | ティアの選び方 |
//! | --- | --- | --- |
//! | `jit-only` | JIT でコンパイルできるすべての関数を、初めて呼ぶときに | コンパイルした関数は JIT のコードで実行する |
//! | `threshold=N` | インタプリタで `N` 回呼んだ関数を、コンパイルスレッドで(完了を待たない) | コンパイルが済んだ後の呼び出しから JIT のコードで実行する |
//! | `alternating=SEED` | `jit-only` と同じ | JIT の関数に入るたびに、シードから決まる擬似乱数で、インタプリタでの実行に切り替えるかどうかを選ぶ |
//! | `mixed=SEED` | シードで決まるおよそ半分の関数([`mixed_selects`])を、初めて呼ぶときに | コンパイルした関数は JIT のコードで実行する |
//!
//! 呼び出しの回数は、インタプリタが関数に入るたびに 1 増やす。
//! 自分自身への末尾呼び出しでフレームを再利用する場合も数える。
//! `threshold=N` では、コンパイルスレッドの起動(LLJIT の作成とスタブのコンパイル)を待たずにプログラムの実行を始める([`Native::set_pending_init`])。
//! JIT の関数の入口でインタプリタでの実行に切り替えるときは、コンテキストの `force_interp` に理由を書いて、`interp_stub` を `musttail` で呼ぶ。
//! 理由は、ネイティブスタックの残りが少ないこと([`SOFT_MARGIN`])か、`alternating` で選ばれたことである。

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use keleut_lir::{ClassId, Func, FuncId, Inst, LabelId, Program, ShapeId, Term, TypeId};
use keleut_rt::stack::HEADER_WORDS;
use keleut_rt::value::Kind;
use keleut_rt::{CalleeView, DictView, Stop, Word};

use crate::interp::{Interp, NativeFx, PHASE, runtime_error, unimpl_prim};

/// [`krt_interp_run`] の返り値の 1 つで、ディスパッチループの呼び出しの最下段のフレームが JIT の関数を末尾呼び出しで呼んだことを表す。
///
/// 一様表現の値とも番兵とも重ならないワードである。
pub const TAILREQ: Word = Word(0x4);

/// 引数の受け渡し用のバッファのワード数。
///
/// これより多い引数を受け取る関数と、これより多いワードをこのバッファで渡す命令を含む関数は、JIT でコンパイルしない([`jit_eligible`])。
pub const ARG_WORDS: usize = 1024;

/// JIT のコードとランタイム関数が共有するコンテキスト。JIT のコードは、[`off`] のオフセットでフィールドを読み書きする。
#[repr(C)]
pub struct NativeCtx {
    /// インタプリタへのポインタ(`*mut Interp`)。
    pub interp: *mut std::ffi::c_void,
    /// 呼び出し先の関数の番号。
    pub callee: u64,
    /// 呼び出し先の環境(クロージャ)。なければ 0 である。
    pub env: u64,
    /// 呼び出し先のフレームの論理的な位置。
    pub cur_pos: u64,
    /// 呼び出し先のフレームヘッダへのポインタ。
    pub cur_fp: *mut u64,
    /// コンテキストの深さ `ctx.depth` へのポインタ。
    pub depth: *mut u64,
    /// 現在の深さの上限へのポインタ。
    pub limit: *mut u64,
    /// ネイティブスタックの下限のアドレス。スタック残量の検査に使い、0 なら検査しない。
    pub stack_low: u64,
    /// 引数の受け渡し用のバッファ([`ARG_WORDS`] ワード)。
    pub args: *mut u64,
    /// ランタイム関数が返す値。
    ///
    /// [`krt_dispatch`] が [`dispatch::VALUE`] を返したときと、[`krt_fx`] が [`fx::VALUE`] か [`fx::REDELIVER`] を返したときに、JIT のコードが読む。
    pub result: u64,
    /// グローバルの値の配列。
    pub globals: *mut u64,
    /// グローバルの初期化済みフラグの配列(1 バイトずつ)。
    pub global_init: *mut u8,
    /// 文字列定数のオブジェクトの配列。
    pub strings: *const u64,
    /// ネイティブスタックのアドレスで、スタックがこれを下回ったら JIT の関数をインタプリタで実行する([`SOFT_MARGIN`])。0 なら検査しない。
    pub stack_soft: u64,
    /// `interp_stub` のアドレス。
    pub stub: u64,
    /// 0 でなければ、[`krt_interp_run`] は関数をコンパイルせずにインタプリタで実行する。
    ///
    /// 1 はネイティブスタックの残りが少ないこと、2 は `alternating` で選ばれたことを表す。
    pub force_interp: u64,
    /// `alternating` の擬似乱数の状態(xorshift64)。`alternating` 以外の設定では 0 で、JIT の関数の入口でティアを選ばない。
    pub alt: u64,
    /// 値スタックの使用中の最後のチャンクの範囲([`keleut_rt::stack::StackWindow`])。
    ///
    /// JIT のコードは、呼び出し先のフレームがこの範囲に収まるとき、[`krt_prep`] を呼ばずにその場でフレームを置く。
    pub win: *mut keleut_rt::stack::StackWindow,
    /// 関数の番号ごとに 2 ワードずつ並べた配列。2 ワードは、フレームのワード数と、フレームヘッダの 2 番のワードに書く値である。
    ///
    /// 呼び出し先が実行時に決まる呼び出し(クロージャ)でも、JIT のコードがその場でフレームを置けるように、この配列を読む。
    pub frames: *const u64,
    /// GC のヒープ(`keleut_rt::heap::Heap`)。
    ///
    /// JIT のコードの確保の高速経路が、`keleut_rt::heap::fast` のオフセットでフィールドを読み書きする。
    pub heap: *mut keleut_rt::heap::Heap,
}

/// [`NativeCtx`] のフィールドのオフセット(バイト)。
#[allow(missing_docs)]
pub mod off {
    pub const CALLEE: u64 = 8;
    pub const ENV: u64 = 16;
    pub const CUR_POS: u64 = 24;
    pub const CUR_FP: u64 = 32;
    pub const DEPTH: u64 = 40;
    pub const LIMIT: u64 = 48;
    pub const STACK_LOW: u64 = 56;
    pub const ARGS: u64 = 64;
    pub const RESULT: u64 = 72;
    pub const GLOBALS: u64 = 80;
    pub const GLOBAL_INIT: u64 = 88;
    pub const STRINGS: u64 = 96;
    pub const STACK_SOFT: u64 = 104;
    pub const STUB: u64 = 112;
    pub const FORCE_INTERP: u64 = 120;
    pub const ALT: u64 = 128;
    pub const WIN: u64 = 136;
    pub const FRAMES: u64 = 144;
    pub const HEAP: u64 = 152;
}

/// ネイティブスタックのアドレスが、スタック残量の検査の下限(コンテキストの `stack_low`)にこのバイト数を足したアドレス(`stack_soft`)を下回ったら、関数を JIT のコードでなくインタプリタで実行する。
///
/// JIT の関数の非末尾の再帰はネイティブスタックを使うが、インタプリタの中の呼び出しは使わない。
/// そのため、小さな `--native-stack` でも、スタック残量の検査で終了する前に、論理的な深さの上限まで再帰できる。
/// JIT の関数は、入口でスタックが `stack_soft` を下回っていたら、置いてあるフレームのままインタプリタでの実行に切り替える(入口での脱最適化)。
pub const SOFT_MARGIN: u64 = 64 * 1024;

/// [`krt_dispatch`] に渡す操作の番号と、[`krt_dispatch`] が返す結果。
pub mod dispatch {
    /// 関数値を呼ぶ(`call_value`)。
    pub const OP_VALUE: u64 = 0;
    /// 辞書からメソッドを選んで呼ぶ(`call_method`)。
    pub const OP_METHOD: u64 = 1;
    /// 辞書を渡す(`dict_app`)。
    pub const OP_DICTAPP: u64 = 2;
    /// 末尾呼び出しであることを表すフラグ。操作の番号に足す。
    pub const TAIL: u64 = 8;
    /// 呼び出し先のフレームを置いた。JIT のコードは `entry[ctx.callee]` を呼ぶ。
    pub const CALL: i64 = 0;
    /// メソッドの実装の関数のフレームを置いた。JIT のコードは非末尾呼び出しで呼び、返った値を `OP_VALUE` でもう一度渡す。
    pub const PHASE: i64 = 1;
    /// 結果の値が `ctx.result` にある。構造的な `Eq` の比較の結果か、`extern` の値への `dict_app` の結果である。
    pub const VALUE: i64 = 2;
    /// 状態ワードを設定したか、巻き戻さずに終了するエラーを保存した。JIT のコードは番兵を返す。
    pub const ERR: i64 = 3;
}

/// [`krt_fx`] と [`krt_fx_term`] が返す結果。
pub mod fx {
    /// 結果を書き込み先のスロットに書いた。JIT のコードは次の命令へ進む。
    pub const NEXT: i64 = 0;
    /// 状態ワードを設定したか、巻き戻さずに終了するエラーを保存した。JIT のコードは番兵を返す。
    pub const ERR: i64 = 1;
    /// 呼び出し先のフレームを置いた。JIT のコードは `entry[ctx.callee]` を呼ぶ([`super::krt_fx_term`] の結果なら `musttail` で呼ぶ)。
    pub const CALL: i64 = 2;
    /// 呼び出しの結果が `ctx.result` にある。継続の再開(`cont_resume`)の結果である。
    pub const VALUE: i64 = 3;
    /// フレームヘッダの再開点が指す呼び出しの結果が `ctx.result` にある。
    ///
    /// `cont_make` が継続を作れなかったときの結果で、JIT のコードはフレームヘッダの再開点から続ける。
    pub const REDELIVER: i64 = 4;
}

/// Rust から tailcc の関数を呼ぶスタブ `enter_tailcc` の型。
pub type EnterFn = unsafe extern "C" fn(*mut NativeCtx, usize) -> u64;

/// コンパイルスレッドの起動が済むのを待ち、その結果(`interp_stub` と `enter_tailcc` のアドレス)を受け取る関数。
pub type PendingInit = Box<dyn FnMut() -> Result<(u64, EnterFn), String> + Send>;

/// ティアの設定(`--tier`)。
///
/// `interp-only` に相当するバリアントはない。
/// `interp-only` のときは、インタプリタにネイティブバックエンドを付けない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierMode {
    /// JIT でコンパイルできる関数はどれも、初めて呼ぶときにコンパイルする。
    JitOnly,
    /// インタプリタで `N` 回呼んだ関数のコンパイルを、コンパイルスレッドに依頼する。完了は待たない。
    Threshold(u64),
    /// `jit-only` と同じようにコンパイルし、JIT の関数に入るたびに、シードから決まる擬似乱数でティアを選ぶ。
    Alternating(u64),
    /// シードで決まるおよそ半分の関数([`mixed_selects`])だけを、初めて呼ぶときにコンパイルする。
    Mixed(u64),
}

/// コンパイルスレッドにコンパイルを依頼する関数と、失敗の通知。
pub struct Translator {
    /// 関数のコンパイルを依頼する。
    ///
    /// 第 2 引数が真なら、完了を待ってエントリのアドレスを返す(完了を待つ依頼)。
    /// 偽なら、待たずに `None` を返す(完了を待たない依頼)。
    /// コンパイルスレッドは、コンパイルを終えたらエントリテーブルの項目を書き換える。
    ///
    /// 次の 2 つの場合、コンパイルスレッドはその関数をコンパイルしない。
    /// 完了を待つ依頼にはアドレス 0 を返し、完了を待たない依頼ではエントリテーブルを書き換えない。
    /// どちらの依頼でも、その関数はインタプリタで実行を続ける。
    ///
    /// - コンパイルスレッドがコンパイルをやめた後。
    ///   コンパイルスレッドは、グローバルアロケータの予備領域を自分が使ったときと、コンパイルの前に予備領域がなく、確保し直すこともできないときに、コンパイルをやめる(`keleut_rt::alloc`)。
    ///   やめる前にコンパイルした関数は、JIT のコードのまま実行する。
    /// - `RLIMIT_AS` が有限で、仮想メモリの残りが `JIT_AS_ROOM`(64 MiB)を下回るとき。
    ///   その関数だけをコンパイルせず、次の依頼でまた残りを調べる。
    pub request: Box<dyn FnMut(FuncId, bool) -> Result<Option<usize>, String>>,
    /// 完了を待たない依頼のコンパイルが失敗したことを表すフラグと、そのエラーメッセージ。
    pub failure: Arc<(AtomicBool, Mutex<Option<String>>)>,
}

/// 関数ごとのティアの状態。
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// まだ決めていない。
    Unknown,
    /// インタプリタで実行する。
    Interp,
    /// コンパイルを依頼した(完了を待たない依頼)。エントリテーブルの項目が書き換わったらコンパイル済みになる。
    Requested,
    /// コンパイル済みである。エントリのアドレスはエントリテーブルにある。
    Compiled,
}

/// splitmix64 の 1 回分の計算。
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// `mixed=SEED` で関数 `f` をコンパイルするなら真を返す。
///
/// 結果はシードと関数の番号だけで決まり、およそ半分の関数で真になる。
pub fn mixed_selects(seed: u64, f: FuncId) -> bool {
    mix(mix(seed) ^ u64::from(f.0)) & 1 == 1
}

/// `alternating=SEED` の擬似乱数の初期状態を返す。0 にはならない。
pub fn alternating_state(seed: u64) -> u64 {
    mix(seed ^ 0x5eed) | 1
}

/// ネイティブバックエンドの状態。インタプリタが持つ。
pub struct Native {
    /// JIT のコードとランタイム関数に渡すコンテキスト。
    pub ctx: NativeCtx,
    /// エントリテーブル。関数の番号ごとにエントリのアドレスを持つ。JIT のコードは、この表のアドレスを定数として持つ。
    pub entry: Arc<[AtomicUsize]>,
    enter: EnterFn,
    /// コンパイルスレッドの起動を待たずに実行を始めたときに、起動の結果(`interp_stub` と `enter_tailcc` のアドレス)を受け取る関数。
    ///
    /// エントリテーブルの項目は、起動が済むまでは 0 で、起動が済むと `interp_stub` を指す。
    /// ミューテータは、コンパイルを依頼した関数の項目が 0 でなくなったのを見たときに、この関数を 1 回だけ呼ぶ([`Native::set_pending_init`])。
    pending: Option<PendingInit>,
    state: Vec<State>,
    eligible: Vec<bool>,
    counts: Vec<u64>,
    /// ティアの設定。
    pub mode: TierMode,
    translator: Translator,
    frames: Vec<(usize, u64)>,
    /// `ctx.frames` が指す配列。
    _frame_tab: Box<[u64]>,
    /// 巻き戻さずに終了するエラー。ランタイム関数が保存し、インタプリタがネイティブの呼び出しから戻ったときに取り出す。
    pub fatal: Option<Stop>,
    /// 引数の受け渡し用のバッファ。`ctx.args` が指す。
    _args: Box<[u64]>,
    /// コンパイルした関数の個数。ドロップするときにエントリテーブルから数え直す。
    pub compiled: usize,
    /// JIT の関数の入口でネイティブスタックの残りが少なく、インタプリタでの実行に切り替えた回数。
    pub low_stack_entries: u64,
    /// JIT の関数の入口で、`alternating` で選ばれてインタプリタでの実行に切り替えた回数。
    pub alt_entries: u64,
    /// `interp_stub` から [`krt_interp_run`] に入った回数。
    pub stub_entries: u64,
    /// JIT の関数への末尾呼び出しの依頼([`TAILREQ`])の回数(計測用)。
    ///
    /// [`krt_interp_run`] が依頼を返したときと、`Interp::call_native_request` が依頼を実行したときに数える。
    pub tail_requests: u64,
    /// ランタイム関数とネイティブの呼び出しが観測した、ネイティブスタックの最も深いアドレス(計測用)。
    pub min_sp: usize,
    /// 同じ時点で観測した、コンテキストの深さの最大値(計測用)。
    pub max_depth_seen: u64,
    /// 最初のネイティブの呼び出しの直前のスタックのアドレス(計測用)。
    pub sp0: usize,
}

/// 直近の実行の [`Native::low_stack_entries`](テスト用)。
pub static LAST_LOW_STACK_ENTRIES: AtomicU64 = AtomicU64::new(0);
/// 直近の実行の [`Native::compiled`](テスト用)。
pub static LAST_COMPILED: AtomicU64 = AtomicU64::new(0);
/// 直近の実行の [`Native::alt_entries`](テスト用)。
pub static LAST_ALT_ENTRIES: AtomicU64 = AtomicU64::new(0);
/// 直近の実行の [`Native::stub_entries`](テスト用)。
pub static LAST_STUB_ENTRIES: AtomicU64 = AtomicU64::new(0);
/// 直近の実行の [`Native::tail_requests`](テスト用)。
pub static LAST_TAIL_REQUESTS: AtomicU64 = AtomicU64::new(0);
/// 直近の実行で、最初のネイティブの呼び出しの直前から測った、ネイティブスタックの使用量(バイト。テスト用)。
pub static LAST_STACK_USED: AtomicU64 = AtomicU64::new(0);

/// JIT でコンパイルする関数に許す、コンパイルの重み([`jit_cost`] の値)の上限。
///
/// これを超える関数はインタプリタで実行する。
/// コンパイルの時間は重みのおよそ 1.3 乗に比例して増え、実測では、重み 4 万から 9 万の関数で 0.6 秒から 1.7 秒かかった(`default<O2>`)。
/// 上限は、重みが上限に近い関数のコンパイルが 1 秒ほどで終わるように選んだ。
/// `conformance/` のテストの関数の重みは 600 に満たない。
pub const JIT_COST_MAX: u64 = 50_000;

/// 関数のコンパイルの重みを見積もる。
///
/// 安全点と再開点の個数の合計にスロット数を掛け、それを命令と終端命令の個数に足した値である。
/// JIT のコードは、安全点と再開点のそれぞれで、生存しているレジスタをスロットに書く。
/// そのため、生成する LLVM IR のサイズは、およそ積の項に比例する。
/// 生存しているレジスタの個数はスロット数を超えないので、積の項は、スロットに書く回数の上限である。
/// フレームの情報(`frame`)がない関数の重みは 0 である。
pub fn jit_cost(f: &Func) -> u64 {
    let Some(fr) = f.frame.as_ref() else {
        return 0;
    };
    let insts: u64 = f.blocks.iter().map(|b| b.insts.len() as u64 + 1).sum();
    let points = (fr.safepoints.len() + fr.resume_points.len()) as u64;
    let slots = u64::from(fr.any_slots) + u64::from(fr.raw_slots);
    insts.saturating_add(points.saturating_mul(slots))
}

/// 関数を JIT でコンパイルできるなら真を返す。
///
/// 次の関数は偽で、インタプリタで実行する。
///
/// - フレームの情報(`frame`)がない関数。
/// - 引数の個数が、受け渡し用のバッファのワード数([`ARG_WORDS`])を超える関数と、オペランドがこのバッファに入らない命令か終端命令を含む関数。
/// - コンパイルの重みが上限([`JIT_COST_MAX`])を超える関数。
///
/// エフェクトの命令と終端命令を含む関数は、JIT でコンパイルできる(モジュールの説明の「番兵とエフェクト」)。
pub fn jit_eligible(f: &Func) -> bool {
    if f.frame.is_none() || f.params.len() > ARG_WORDS {
        return false;
    }
    if jit_cost(f) > JIT_COST_MAX {
        return false;
    }
    for b in &f.blocks {
        for i in &b.insts {
            match i {
                Inst::CallMethod { own, args, .. } if own.len() + args.len() > ARG_WORDS => {
                    return false;
                }
                Inst::CtorNew { fields, .. } | Inst::RecNew { fields, .. }
                    if fields.len() > ARG_WORDS =>
                {
                    return false;
                }
                Inst::ClosureNew { caps: v, .. }
                | Inst::MethodNew { own: v, .. }
                | Inst::DictInst { premises: v, .. }
                | Inst::CallValue { args: v, .. }
                | Inst::CallDirect { args: v, .. }
                | Inst::DictApp { dicts: v, .. }
                    if v.len() > ARG_WORDS =>
                {
                    return false;
                }
                Inst::DictRecord { fields, .. } | Inst::DictVariant { fields, .. }
                    if 2 * fields.len() > ARG_WORDS =>
                {
                    return false;
                }
                _ => {}
            }
        }
        match &b.term {
            Term::TailCallMethod { own, args, .. } if own.len() + args.len() > ARG_WORDS => {
                return false;
            }
            Term::TailCallDirect { args, .. } | Term::TailCallValue { args, .. }
                if args.len() > ARG_WORDS =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

impl Native {
    /// コンパイルスレッドの起動を待たずに実行を始めるときに、起動の結果を受け取る関数 `init` を設定する(`threshold=N`)。
    ///
    /// エントリテーブルの項目は起動が済むまで 0 で、その間はコンパイル済みの関数がない。
    /// コンパイルを依頼した関数の項目が 0 でなくなったら、`init` で起動の結果を受け取ってから JIT のコードを呼ぶ。
    pub fn set_pending_init(&mut self, init: PendingInit) {
        self.pending = Some(init);
    }
}

impl Drop for Native {
    /// テスト用の統計(`LAST_*`)を保存する。環境変数 `KELEUT_JIT_STATS` があれば、コンパイルした関数の個数などの統計を標準エラーに書く。
    fn drop(&mut self) {
        let used = self.sp0.saturating_sub(self.min_sp);
        // 完了を待たない依頼でコンパイルした関数は、ミューテータがエントリテーブルの項目を調べる前に JIT のコードから呼ばれることがあるので、エントリテーブルで数える。
        let stub = self.ctx.stub as usize;
        self.compiled = self
            .entry
            .iter()
            .filter(|e| {
                let e = e.load(Ordering::Acquire);
                e != stub && e != 0 && self.pending.is_none()
            })
            .count();
        LAST_LOW_STACK_ENTRIES.store(self.low_stack_entries, Ordering::Relaxed);
        LAST_COMPILED.store(self.compiled as u64, Ordering::Relaxed);
        LAST_ALT_ENTRIES.store(self.alt_entries, Ordering::Relaxed);
        LAST_STUB_ENTRIES.store(self.stub_entries, Ordering::Relaxed);
        LAST_TAIL_REQUESTS.store(self.tail_requests, Ordering::Relaxed);
        LAST_STACK_USED.store(used as u64, Ordering::Relaxed);
        if std::env::var_os("KELEUT_JIT_STATS").is_some() {
            let interp = self.state.iter().filter(|s| **s == State::Interp).count();
            eprintln!(
                "keleut: jit: コンパイルした関数 {}、インタプリタで実行した関数 {interp}、スタックの下限で切り替えたエントリ {}、\
                 alternating で切り替えたエントリ {}、interp_stub からの起動 {}、末尾呼び出しの依頼 {}、\
                 ネイティブスタック {used} バイト(コンテキストの深さ {})",
                self.compiled,
                self.low_stack_entries,
                self.alt_entries,
                self.stub_entries,
                self.tail_requests,
                self.max_depth_seen
            );
        }
    }
}

impl Native {
    /// プログラム `prog` のネイティブバックエンドの状態を作る。
    ///
    /// `entry` は、関数の個数と同じ長さのエントリテーブルである。
    /// どの項目も、はじめは `interp_stub` を指す。
    /// コンパイルスレッドの起動を待たないときは、起動が済むまで 0 である([`Native::set_pending_init`])。
    pub fn new(
        prog: &Program,
        entry: Arc<[AtomicUsize]>,
        enter: EnterFn,
        mode: TierMode,
        translator: Translator,
    ) -> Native {
        let frames: Vec<(usize, u64)> = prog
            .funcs
            .iter()
            .map(|f| match &f.frame {
                Some(fr) => (
                    HEADER_WORDS + (fr.any_slots + fr.raw_slots) as usize,
                    u64::from(fr.any_slots) | (u64::from(fr.raw_slots) << 32),
                ),
                None => (HEADER_WORDS, 0),
            })
            .collect();
        let frame_tab: Box<[u64]> = frames
            .iter()
            .flat_map(|&(fw, sizes)| [fw as u64, sizes])
            .collect();
        let mut args = vec![0u64; ARG_WORDS].into_boxed_slice();
        let alt = match mode {
            TierMode::Alternating(seed) => alternating_state(seed),
            _ => 0,
        };
        Native {
            ctx: NativeCtx {
                interp: std::ptr::null_mut(),
                callee: 0,
                env: 0,
                cur_pos: 0,
                cur_fp: std::ptr::null_mut(),
                depth: std::ptr::null_mut(),
                limit: std::ptr::null_mut(),
                stack_low: 0,
                args: args.as_mut_ptr(),
                result: 0,
                globals: std::ptr::null_mut(),
                global_init: std::ptr::null_mut(),
                strings: std::ptr::null(),
                stack_soft: 0,
                stub: entry
                    .first()
                    .map_or(0, |e| e.load(Ordering::Relaxed) as u64),
                force_interp: 0,
                alt,
                win: std::ptr::null_mut(),
                frames: frame_tab.as_ptr(),
                heap: std::ptr::null_mut(),
            },
            entry,
            enter,
            pending: None,
            state: vec![State::Unknown; prog.funcs.len()],
            eligible: prog.funcs.iter().map(jit_eligible).collect(),
            counts: vec![0; prog.funcs.len()],
            mode,
            translator,
            frames,
            _frame_tab: frame_tab,
            fatal: None,
            _args: args,
            compiled: 0,
            low_stack_entries: 0,
            alt_entries: 0,
            stub_entries: 0,
            tail_requests: 0,
            min_sp: usize::MAX,
            max_depth_seen: 0,
            sp0: 0,
        }
    }
}

impl<'p> Interp<'p> {
    /// インタプリタにネイティブバックエンドを付ける。
    pub fn set_native(&mut self, n: Box<Native>) {
        self.native = Some(n);
    }

    /// 関数 `f` を JIT のコードで実行するなら、エントリのアドレスを返す。
    ///
    /// インタプリタが関数に入るたびに呼ぶ(モジュールの説明の「ティアの切り替え」)。
    /// `threshold` では呼び出しの回数を数え、しきい値に達したらコンパイルを依頼する。
    /// それ以外の設定では、コンパイルする関数を初めて呼ぶときにコンパイルを依頼し、完了を待つ。
    /// ネイティブスタックの残りが少ないときは、コンパイル済みの関数でも `None` を返す([`SOFT_MARGIN`])。
    pub(crate) fn native_target(&mut self, f: FuncId) -> Result<Option<usize>, Stop> {
        let Some(n) = self.native.as_mut() else {
            return Ok(None);
        };
        if self.stack_low != 0 && crate::interp::approx_sp() < self.stack_low + SOFT_MARGIN as usize
        {
            // ネイティブスタックの残りが少ない。JIT の関数もインタプリタで実行する([`SOFT_MARGIN`])。
            return Ok(None);
        }
        let i = f.index();
        match n.state[i] {
            State::Compiled => return Ok(Some(n.entry[i].load(Ordering::Relaxed))),
            State::Interp => return Ok(None),
            State::Requested => {
                let e = n.entry[i].load(Ordering::Acquire);
                if e != 0
                    && let Some(mut init) = n.pending.take()
                {
                    // コンパイルスレッドの起動が済んだ。`interp_stub` と `enter_tailcc` のアドレスを受け取る。
                    let (stub, enter) = init().map_err(|m| {
                        Stop::Internal(format!("LLVM の JIT を始められません: {m}"))
                    })?;
                    n.ctx.stub = stub;
                    n.enter = enter;
                }
                if e != 0 && e as u64 != n.ctx.stub {
                    n.state[i] = State::Compiled;
                    n.compiled += 1;
                    return Ok(Some(e));
                }
                if n.translator.failure.0.load(Ordering::Relaxed) {
                    let m = n
                        .translator
                        .failure
                        .1
                        .lock()
                        .map(|m| m.clone().unwrap_or_default())
                        .unwrap_or_default();
                    return Err(Stop::Internal(format!("関数をコンパイルできません: {m}")));
                }
                return Ok(None);
            }
            State::Unknown => {}
        }
        if !n.eligible[i] {
            n.state[i] = State::Interp;
            return Ok(None);
        }
        match n.mode {
            TierMode::Threshold(t) => {
                n.counts[i] += 1;
                if n.counts[i] < t {
                    return Ok(None);
                }
                (n.translator.request)(f, false).map_err(|e| {
                    Stop::Internal(format!(
                        "関数 {} のコンパイルを依頼できません: {e}",
                        self.prog.funcs[i].sym
                    ))
                })?;
                n.state[i] = State::Requested;
                return Ok(None);
            }
            TierMode::Mixed(seed) if !mixed_selects(seed, f) => {
                n.state[i] = State::Interp;
                return Ok(None);
            }
            _ => {}
        }
        let addr = (n.translator.request)(f, true)
            .and_then(|a| a.ok_or_else(|| "アドレスが返りませんでした".to_string()))
            .map_err(|e| {
                Stop::Internal(format!(
                    "関数 {} をコンパイルできません: {e}",
                    self.prog.funcs[i].sym
                ))
            })?;
        if addr == 0 {
            // コンパイルスレッドがこの関数をコンパイルしなかった。この関数はインタプリタで実行を続ける。
            n.state[i] = State::Interp;
            return Ok(None);
        }
        n.state[i] = State::Compiled;
        n.compiled += 1;
        Ok(Some(addr))
    }

    /// コンテキストのポインタのフィールドを、現在の `self` から書き直す。
    ///
    /// ネイティブの呼び出しの直前に呼ぶ。
    /// JIT のコードとランタイム関数は、その呼び出しの間、ここで書いたポインタを通してインタプリタとランタイムを読み書きする。
    fn native_ctx(&mut self) -> *mut NativeCtx {
        let me = self as *mut Interp<'p> as *mut std::ffi::c_void;
        let low = self.stack_low as u64;
        let soft = if low == 0 { 0 } else { low + SOFT_MARGIN };
        let rt = &mut self.rt;
        let n = self
            .native
            .as_mut()
            .expect("ネイティブのバックエンドがある");
        if n.sp0 == 0 {
            n.sp0 = crate::interp::approx_sp();
        }
        n.ctx.interp = me;
        n.ctx.depth = &mut rt.depth;
        n.ctx.limit = &mut rt.limit;
        n.ctx.stack_low = low;
        n.ctx.stack_soft = soft;
        n.ctx.globals = rt.globals.as_mut_ptr().cast::<u64>();
        n.ctx.global_init = rt.global_init.as_mut_ptr().cast::<u8>();
        n.ctx.strings = rt.strings.as_ptr().cast::<u64>();
        n.ctx.win = rt.stack.window_ptr();
        n.ctx.heap = rt.heap.fast_ptr();
        &mut n.ctx
    }

    /// 関数 `func` の JIT のコードを、エントリ `addr` から呼ぶ。
    ///
    /// フレームは、位置 `pos`(フレームヘッダは `fp`)に置いてある。
    /// 呼び出しの間に、巻き戻さずに終了するエラーが保存されていたら、それを `Err` で返す。
    pub(crate) fn call_native(
        &mut self,
        addr: usize,
        func: FuncId,
        env: Word,
        pos: u64,
        fp: *mut u64,
    ) -> Result<Word, Stop> {
        sample(self);
        let ctx = self.native_ctx();
        // SAFETY: ctx は self.native の中のコンテキストを指し、ここで書いたフィールドは呼び出しの間有効である。
        let r = unsafe {
            (*ctx).callee = u64::from(func.0);
            (*ctx).cur_pos = pos;
            (*ctx).cur_fp = fp;
            (*ctx).env = env.0;
            let enter = self
                .native
                .as_ref()
                .expect("ネイティブのバックエンドがある")
                .enter;
            enter(ctx, addr)
        };
        let n = self
            .native
            .as_mut()
            .expect("ネイティブのバックエンドがある");
        if let Some(f) = n.fatal.take() {
            return Err(f);
        }
        Ok(Word(r))
    }

    /// ディスパッチループの呼び出しの最下段のフレームが JIT の関数 `func` を末尾呼び出しで呼ぶという依頼を、コンテキストに書く。
    ///
    /// 関数 `func` のフレームは、位置 `pos` に置いてある。
    pub(crate) fn native_request(&mut self, func: FuncId, env: Word, pos: u64, fp: *mut u64) {
        let n = self
            .native
            .as_mut()
            .expect("ネイティブのバックエンドがある");
        n.ctx.callee = u64::from(func.0);
        n.ctx.cur_pos = pos;
        n.ctx.cur_fp = fp;
        n.ctx.env = env.0;
    }

    /// ディスパッチループの呼び出しの最下段のフレームが残した末尾呼び出しの依頼を実行する。
    ///
    /// 依頼された JIT の関数を呼び、その結果を返す。
    pub(crate) fn call_native_request(&mut self) -> Result<Word, Stop> {
        let n = self
            .native
            .as_mut()
            .expect("ネイティブのバックエンドがある");
        n.tail_requests += 1;
        let func = FuncId(n.ctx.callee as u32);
        let (env, pos, fp) = (Word(n.ctx.env), n.ctx.cur_pos, n.ctx.cur_fp);
        let addr = n.entry[func.index()].load(Ordering::Acquire);
        self.call_native(addr, func, env, pos, fp)
    }
}

// ---- ランタイム関数(JIT のコードが呼ぶ) ----

/// ランタイム関数の本体 `f` を実行し、パニックが JIT のフレームに伝わらないようにする。
///
/// JIT のフレームはスタックのアンワインド用の表(unwind table)を持たず、`extern "C"` の関数からパニックが外に出ると、Rust はプロセスを abort する。
/// そこでパニックを捕捉し、標準出力のバッファを flush し、インタプリタバックエンドで実行スレッドがパニックしたときと同じ診断を出して、終了コード 70 で終了する。
/// パニックの後のインタプリタの状態は信用できないので、JIT のコードには戻らない。
fn guarded<R>(ctx: *mut NativeCtx, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // SAFETY: ctx はネイティブの呼び出しの最中のコンテキストで、この後 JIT のコードには戻らない。
                let it = unsafe { interp(ctx) };
                let _ = it.rt.out.flush();
            }));
            eprintln!("keleut: 内部エラー: 実行スレッドが異常終了しました");
            std::process::exit(70)
        }
    }
}

/// コンテキストからインタプリタへの参照を得る。
///
/// # Safety
///
/// `ctx` は [`Interp::native_ctx`] が書いたコンテキストで、ネイティブの呼び出しの最中である。
unsafe fn interp<'a>(ctx: *mut NativeCtx) -> &'a mut Interp<'static> {
    // SAFETY: 事前条件により、ctx.interp は有効な Interp を指す。Interp は呼び出しの間移動しない。
    unsafe { &mut *((*ctx).interp as *mut Interp<'static>) }
}

fn native<'a>(it: &'a mut Interp<'_>) -> &'a mut Native {
    it.native.as_mut().expect("ネイティブのバックエンドがある")
}

/// ネイティブスタックのアドレスとコンテキストの深さを記録する(計測用。`KELEUT_JIT_STATS`)。
#[inline]
fn sample(it: &mut Interp<'_>) {
    let sp = crate::interp::approx_sp();
    let depth = it.rt.depth;
    let n = native(it);
    n.min_sp = n.min_sp.min(sp);
    n.max_depth_seen = n.max_depth_seen.max(depth);
}

/// 巻き戻さずに終了するエラーを保存する。すでに保存してあれば、最初のものを残す。
fn set_fatal(it: &mut Interp<'_>, s: Stop) {
    let n = native(it);
    if n.fatal.is_none() {
        n.fatal = Some(s);
    }
}

/// エラーを状態ワードに設定する。巻き戻さずに終了するエラーなら、[`set_fatal`] で保存する。
fn fail(it: &mut Interp<'_>, s: Stop) {
    if let Err(f) = it.rt.raise(s) {
        set_fatal(it, f);
    }
}

fn bug(m: &str) -> Stop {
    Stop::Internal(m.to_string())
}

/// 関数 `fid` のフレームを位置 `at` に置き、フレームヘッダとコンテキストのフィールドを書く。
///
/// 呼び出し元の現在のフレームは位置 `me` にある。
/// 値スタックを拡張できなければ、メモリ不足のエラーを状態ワードに設定して `None` を返す。
fn place(
    it: &mut Interp<'_>,
    ctx: *mut NativeCtx,
    fid: u64,
    me: u64,
    at: u64,
    caller: u64,
    env: u64,
) -> Option<*mut u64> {
    it.rt.stack.ptr_at(me);
    let (fw, sizes) = native(it).frames[fid as usize];
    match it.rt.stack.place(at, fw) {
        Ok(pos) => {
            let p = it.rt.stack.ptr_at(pos);
            // SAFETY: p からフレームのワード数の分はチャンクの中にあり、ctx はネイティブの呼び出しの最中のコンテキストである。
            unsafe {
                *p = fid;
                *p.add(1) = 0;
                *p.add(2) = sizes;
                *p.add(3) = caller;
                (*ctx).callee = fid;
                (*ctx).cur_pos = pos;
                (*ctx).cur_fp = p;
                (*ctx).env = env;
            }
            Some(p)
        }
        Err(_) => {
            fail(it, Stop::OutOfMemory);
            None
        }
    }
}

/// JIT の関数が関数 `fid` を呼ぶ前に、そのフレームを位置 `at` に置く。フレームヘッダの 3 番のワードには `caller` を書く。
///
/// 呼び出し元の現在のフレームは位置 `me` にある。
/// 置いたフレームのヘッダへのポインタを返し、コンテキストの `callee`、`cur_pos`、`cur_fp`、`env` を書く。
/// 値スタックを拡張できなければ、メモリ不足のエラーを状態ワードに設定して null ポインタを返す。
/// これは、呼び出し元の命令の失敗として扱う。
///
/// # Safety
///
/// JIT のコードが、ネイティブの呼び出しの最中に、その呼び出しのコンテキストを渡して呼ぶ。
pub unsafe extern "C" fn krt_prep(
    ctx: *mut NativeCtx,
    fid: u64,
    me: u64,
    at: u64,
    caller: u64,
    env: u64,
) -> *mut u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        sample(it);
        place(it, ctx, fid, me, at, caller, env).unwrap_or(std::ptr::null_mut())
    })
}

/// レジスタで渡された引数を、置いてあるフレーム(コンテキストの `cur_fp`)のスロットに書き、環境をコンテキストに書く。
///
/// レジスタ渡しのエントリのスタブ `interp_stub_r` が呼ぶ。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_store_regs(
    ctx: *mut NativeCtx,
    env: u64,
    x0: u64,
    x1: u64,
    x2: u64,
    x3: u64,
) {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        // SAFETY: ctx は有効なコンテキストで、cur_fp は置いてあるフレームを指す。
        unsafe {
            let fid = (*ctx).callee as usize;
            let n = it.prog.funcs[fid].params.len().min(REG_ARGS);
            let fp = (*ctx).cur_fp;
            for (i, x) in [x0, x1, x2, x3].into_iter().take(n).enumerate() {
                *fp.add(HEADER_WORDS + i) = x;
            }
            (*ctx).env = env;
        }
    })
}

/// レジスタで渡す引数の個数。`ctx` と環境を合わせて、x86-64 の引数レジスタ 6 個に収まる。
pub const REG_ARGS: usize = 4;

/// JIT のフレーム(フレームヘッダは `fp`)の非末尾呼び出しから番兵が返ったときに呼ぶ。
///
/// コンテキストの深さをこのフレームの深さに書き戻し、状態ワードがこのフレームに届いてよいものかを検査する。
/// 次の場合は、内部エラーを巻き戻さずに終了するエラーとして保存する(`doc/lir.md` の 16.1 節、17 節)。
///
/// - `yields` が 0 の再開点に `YIELD` が届いた。
/// - 操作節の関数とその呼び出し元の間だけで使う目印(`clause_matched`、`clause_value`、`tail_resume`)が、通常の呼び出しに届いた。
///
/// 巻き戻さずに終了するエラーがすでに保存されていれば、何もしない。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_sentinel(ctx: *mut NativeCtx, fp: *mut u64, yields: u64) {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        if native(it).fatal.is_some() {
            return;
        }
        // SAFETY: fp はこのフレームのヘッダを指す。
        it.rt.depth = unsafe { *fp.add(1) };
        let e = match &it.rt.pending {
            keleut_rt::effect::Pending::Yield { .. } if yields == 0 => Some(bug(
                "yield が noyield の再開点に達しました(may_yield の印の誤り)",
            )),
            keleut_rt::effect::Pending::Matched(_)
            | keleut_rt::effect::Pending::ClauseValue(_)
            | keleut_rt::effect::Pending::TailResume(_) => {
                Some(bug("節の通知が通常の呼び出しに達しました"))
            }
            _ => None,
        };
        if let Some(e) = e {
            set_fatal(it, e);
        }
    })
}

/// `interp_stub` から入り、インタプリタのディスパッチループを呼び出す。
///
/// フレームは呼び出し元が置き、コンテキストの `callee`、`cur_pos`、`cur_fp`、`env` を書いてある。
/// `force_interp` が 0 で、関数を JIT のコードで実行できるなら、ディスパッチループを呼ばずに [`TAILREQ`] を返し、スタブにその関数を呼ばせる。
/// `jit-only` などの設定では、このときに関数をコンパイルする。
/// それ以外は、関数をインタプリタで実行し、最下段のフレームが返した値か番兵、または [`TAILREQ`] を返す。
/// 巻き戻さずに終了するエラーが起きたら、それを保存して番兵を返す。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_interp_run(ctx: *mut NativeCtx) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        // SAFETY: ctx は有効なコンテキストである。
        let (func, env, pos, fp) = unsafe {
            (
                FuncId((*ctx).callee as u32),
                Word((*ctx).env),
                (*ctx).cur_pos,
                (*ctx).cur_fp,
            )
        };
        // SAFETY: ctx は有効なコンテキストである。
        let forced = unsafe { std::mem::replace(&mut (*ctx).force_interp, 0) };
        native(it).stub_entries += 1;
        match forced {
            0 => match it.native_target(func) {
                Ok(Some(_)) => {
                    native(it).tail_requests += 1;
                    return TAILREQ.0;
                }
                Ok(None) => {}
                Err(e) => {
                    set_fatal(it, e);
                    return Word::SENTINEL.0;
                }
            },
            2 => native(it).alt_entries += 1,
            _ => native(it).low_stack_entries += 1,
        }
        let r = it.run_activation(func, env, pos, fp);
        if matches!(r, Ok(v) if v == TAILREQ) {
            native(it).tail_requests += 1;
        }
        // ディスパッチループの中のネイティブの呼び出しがコンテキストのポインタを書き直しているが、
        // どのフィールドも同じ Interp と Rt を指すので、戻る前に書き戻す必要はない。
        match r {
            Ok(v) => v.0,
            Err(e) => {
                set_fatal(it, e);
                Word::SENTINEL.0
            }
        }
    })
}

/// [`krt_dispatch`] の処理の状態。呼び出し先を決めて、そのフレームを置く。
struct Disp<'a, 'p> {
    it: &'a mut Interp<'p>,
    ctx: *mut NativeCtx,
    me: u64,
    row: u32,
    tail: bool,
    args: Vec<Word>,
}

impl Disp<'_, '_> {
    /// 自分のフレームについて、フレームヘッダへのポインタ、フレームの直後の位置、フレームヘッダの 3 番のワード(呼び出し元のフレームの位置)を返す。
    fn frame(&mut self) -> (*mut u64, u64, u64) {
        let fp = self.it.rt.stack.ptr_at(self.me);
        // SAFETY: fp は現在のフレームのヘッダを指す。
        let (fid, w3) = unsafe { (*fp & 0xffff_ffff, *fp.add(3)) };
        let fw = native(self.it).frames[fid as usize].0 as u64;
        (fp, self.me + fw, w3)
    }

    fn set_row(&mut self, row: u32) {
        let (fp, _, _) = self.frame();
        // SAFETY: fp は現在のフレームのヘッダを指す。
        unsafe { *fp = (*fp & 0xffff_ffff) | (u64::from(row) << 32) };
    }

    /// 関数 `func` のフレームを置き、環境 `env` を設定して引数を書く。末尾呼び出しなら、自分のフレームの位置に置く。
    fn call(&mut self, func: FuncId, env: Word) -> i64 {
        let (_, top, w3) = self.frame();
        let (at, caller) = if self.tail {
            (self.me, w3)
        } else {
            (top, self.me)
        };
        let Some(p) = place(
            self.it,
            self.ctx,
            u64::from(func.0),
            self.me,
            at,
            caller,
            env.0,
        ) else {
            return dispatch::ERR;
        };
        for (i, a) in self.args.iter().enumerate() {
            // SAFETY: 置いたフレームは、関数の引数の個数以上のスロットを持つ。
            unsafe { *p.add(HEADER_WORDS + i) = a.0 };
        }
        dispatch::CALL
    }

    fn err(&mut self, s: Stop) -> i64 {
        fail(self.it, s);
        dispatch::ERR
    }

    fn value(&mut self, w: Word) -> i64 {
        // SAFETY: ctx は有効なコンテキストである。
        unsafe { (*self.ctx).result = w.0 };
        dispatch::VALUE
    }

    fn callee_value(&mut self, f: Word) -> i64 {
        match self.it.rt.callee_view(f) {
            CalleeView::Closure(func) => self.call(func, f),
            CalleeView::ExternUnimpl(g) => {
                let s = unimpl_prim(self.it.prog, g);
                self.err(s)
            }
            CalleeView::Method => {
                let (c, m, d, own) = self.it.rt.method_parts(f);
                self.select(c, m, d, &own)
            }
            CalleeView::Other => {
                let s = Stop::Internal(format!(
                    "呼び出せない値を呼びました: {}",
                    String::from_utf8_lossy(&self.it.rt.show(f))
                ));
                set_fatal(self.it, s);
                dispatch::ERR
            }
        }
    }

    fn select(&mut self, class: ClassId, m: u32, d: Word, own: &[Word]) -> i64 {
        match self.it.rt.dict_view(d) {
            DictView::Builtin(c, t) => match self.it.builtin_method(c, t, m) {
                Ok(body) => self.call(body, Word(0)),
                Err(s) => {
                    set_fatal(self.it, s);
                    dispatch::ERR
                }
            },
            DictView::Inst(i, premises) => {
                if let Err(s) = self.it.check_inst(i) {
                    return self.err(s);
                }
                let prog = self.it.prog;
                let im = &prog.instances[i.index()].methods[m as usize];
                if im.dicts == 0 {
                    let v = self.it.rt.inst_values[i.index()][m as usize];
                    return self.callee_value(v);
                }
                let func = im.func;
                let mut dicts = premises;
                dicts.extend_from_slice(own);
                // メソッドの実装の関数を非末尾呼び出しで評価する。その間は、フレームヘッダの再開点に PHASE フラグを付け、
                // 呼び出しの命令のオペランド(JIT のコードがスロットに書いてある)をルートにする(`doc/lir.md` の 1.8 節)。
                self.set_row(self.row | PHASE);
                let (_, top, _) = self.frame();
                let Some(p) = place(
                    self.it,
                    self.ctx,
                    u64::from(func.0),
                    self.me,
                    top,
                    self.me,
                    0,
                ) else {
                    return dispatch::ERR;
                };
                for (i, a) in dicts.iter().enumerate() {
                    // SAFETY: 置いたフレームは、関数の引数の個数以上のスロットを持つ。
                    unsafe { *p.add(HEADER_WORDS + i) = a.0 };
                }
                dispatch::PHASE
            }
            DictView::Record(_) | DictView::Variant(_) => {
                if self.args.len() != 2 {
                    set_fatal(self.it, bug("構造的な Eq の引数の数が 2 でありません"));
                    return dispatch::ERR;
                }
                let (a, b) = (self.args[0], self.args[1]);
                // 構造的な Eq の比較は、メモリを確保し、ユーザのコードを呼ぶ。フレームヘッダの再開点には、この呼び出しの再開点を書いてある(set_row)。
                self.it.rt.gc.at_frame(self.me);
                let (_, top, _) = self.frame();
                match self.it.eq_struct(class, d, a, b, top) {
                    Ok(r) => self.value(Word::bool(r)),
                    Err(s) => self.err(s),
                }
            }
            DictView::Other => {
                let s = Stop::Internal(format!(
                    "辞書でない値からメソッドを選びました: {}",
                    String::from_utf8_lossy(&self.it.rt.show(d))
                ));
                set_fatal(self.it, s);
                dispatch::ERR
            }
        }
    }
}

/// JIT の関数の、呼び出し先が実行時に決まる呼び出しを処理する。
///
/// 対象は、`call_value`、`call_method`、`dict_app` と、末尾呼び出し版の `tail_call_value`、`tail_call_method` である。
/// `op` は [`dispatch`] の操作の番号で、末尾呼び出しなら [`dispatch::TAIL`] を足す。
/// `me` は現在のフレームの位置である。
/// `row` はフレームヘッダに書く再開点で、末尾呼び出し版では、再開点の表の末尾呼び出し版の項目である。
/// `x` は呼ぶ値か辞書、`cm` は `クラス << 32 | メソッド` である。
/// 引数 `nargs` 個とメソッド自身の辞書 `nown` 個は、この順に、コンテキストの引数の受け渡し用のバッファに並べてある。
/// 結果は [`dispatch`] の `CALL`、`PHASE`、`VALUE`、`ERR` のどれかである。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn krt_dispatch(
    ctx: *mut NativeCtx,
    op: u64,
    me: u64,
    row: u64,
    x: u64,
    cm: u64,
    nown: u64,
    nargs: u64,
) -> i64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        sample(it);
        let (nargs, nown) = (nargs as usize, nown as usize);
        // SAFETY: 引数の受け渡し用のバッファは ARG_WORDS ワードで、JIT のコードは nargs + nown ワードを書いてある。
        let (args, own): (Vec<Word>, Vec<Word>) = unsafe {
            let p = (*ctx).args;
            (
                (0..nargs).map(|i| Word(*p.add(i))).collect(),
                (0..nown).map(|i| Word(*p.add(nargs + i))).collect(),
            )
        };
        let mut d = Disp {
            it,
            ctx,
            me,
            row: row as u32,
            tail: op & dispatch::TAIL != 0,
            args,
        };
        d.set_row(row as u32);
        let x = Word(x);
        match op & 7 {
            dispatch::OP_VALUE => d.callee_value(x),
            dispatch::OP_METHOD => d.select(ClassId((cm >> 32) as u32), cm as u32, x, &own),
            _ => {
                if d.it.rt.is_extern_value(x) {
                    return d.value(x);
                }
                match d.it.rt.callee_view(x) {
                    CalleeView::Closure(func) => d.call(func, x),
                    _ => {
                        set_fatal(d.it, bug("辞書を受け取らない値に辞書を渡しました"));
                        dispatch::ERR
                    }
                }
            }
        }
    })
}

/// 論理的な深さが上限を超えたときに呼ぶ(`depth_check`、`depth_push`)。`depth_limit` のエラーを状態ワードに設定する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_depth_error(ctx: *mut NativeCtx) {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let s = it.rt.depth_error();
        fail(it, s);
    })
}

/// 関数 `fid` のブロック `blk` を終える終端命令 `error` が表す実行時エラーを、状態ワードに設定する。`opnd` はオペランドのワードである。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_error(ctx: *mut NativeCtx, fid: u64, blk: u64, opnd: u64) {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let prog = it.prog;
        let s = match &prog.funcs[fid as usize].blocks[blk as usize].term {
            Term::Error { kind, .. } => runtime_error(prog, &it.rt, kind, opnd),
            _ => bug("error でない終端命令"),
        };
        fail(it, s);
    })
}

/// 未実装の機能に達したときに呼ぶ(`unimplemented`、`unimplemented_value`)。
///
/// `inst` はブロック `blk` の中の命令の番号で、`u64::MAX` なら終端命令を表す。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_unimpl(ctx: *mut NativeCtx, fid: u64, blk: u64, inst: u64) {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let b = &it.prog.funcs[fid as usize].blocks[blk as usize];
        let what = if inst == u64::MAX {
            match &b.term {
                Term::Unimplemented { what } => what.clone(),
                _ => String::new(),
            }
        } else {
            match &b.insts[inst as usize] {
                Inst::UnimplementedValue { what, .. } => what.clone(),
                _ => String::new(),
            }
        };
        set_fatal(it, Stop::Unimplemented(what));
    })
}

/// メモリを確保するランタイム関数に共通の処理。
///
/// 安全点(フレームの位置 `me` と安全点の番号 `sp`)を記録してから `f` を呼ぶ。
/// `f` が失敗したら、エラーを状態ワードに設定して 0 を返す。
fn alloc_with(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    f: impl FnOnce(&mut Interp<'static>, &[u64]) -> Result<Word, Stop>,
) -> u64 {
    guarded(ctx, || {
        // SAFETY: JIT のコードが、ネイティブの呼び出しの最中に呼ぶ。
        let it = unsafe { interp(ctx) };
        it.rt.gc.at_safepoint(me, sp as u32);
        // SAFETY: 引数の受け渡し用のバッファは ARG_WORDS ワードある。
        let args = unsafe { std::slice::from_raw_parts((*ctx).args, ARG_WORDS) };
        match f(it, args) {
            Ok(w) => w.0,
            Err(s) => {
                fail(it, s);
                0
            }
        }
    })
}

/// `box` を実行する。`kind` が 0 なら `i64`、1 なら `f64` をボックス化する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_box(ctx: *mut NativeCtx, me: u64, sp: u64, kind: u64, v: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        if kind == 0 {
            it.rt.box_i64(v as i64)
        } else {
            it.rt.box_f64(v)
        }
    })
}

/// `str.concat` を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_str_concat(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    a: u64,
    b: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| it.rt.str_concat(Word(a), Word(b)))
}

/// `str.sub` を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_str_sub(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    s: u64,
    pos: u64,
    len: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        it.rt.str_sub(Word(s), pos as i32, len as i32)
    })
}

/// `op` に応じて、`show.i32`(0)、`show.i64`(1)、`show.f64`(2)、`show.bool`(3)を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_show(ctx: *mut NativeCtx, me: u64, sp: u64, op: u64, x: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        let s = match op {
            0 => (x as i32).to_string(),
            1 => (x as i64).to_string(),
            2 => keleut_rtcore::show_f64(f64::from_bits(x)),
            _ => (if x != 0 { "true" } else { "false" }).to_string(),
        };
        it.rt.new_str(s.as_bytes())
    })
}

fn words(a: &[u64], n: u64) -> Vec<Word> {
    a[..n as usize].iter().map(|&w| Word(w)).collect()
}

/// `ctor_new` を実行する。フィールドは、引数の受け渡し用のバッファに `n` 個ある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_ctor_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    ty: u64,
    ctor: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.ctor_new(TypeId(ty as u32), ctor as u32, &words(a, n))
    })
}

/// `variant_new` を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_variant_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    label: u64,
    p: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        it.rt.variant_new(LabelId(label as u32), Word(p))
    })
}

/// `rec_new` を実行する。フィールドは、引数の受け渡し用のバッファに `n` 個ある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_rec_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    shape: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.rec_new(ShapeId(shape as u32), &words(a, n))
    })
}

/// `op` に応じて、`rec_extend`(0)、`rec_restrict`(1)、`rec_update`(2)を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn krt_rec_op(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    op: u64,
    v: u64,
    label: u64,
    x: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        let l = LabelId(label as u32);
        match op {
            0 => it.rt.rec_extend(Word(v), l, Word(x)),
            1 => it.rt.rec_restrict(Word(v), l),
            _ => it.rt.rec_update(Word(v), l, Word(x)),
        }
    })
}

/// `closure_new` を実行する。キャプチャは、引数の受け渡し用のバッファに `n` 個ある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_closure_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    func: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.closure_new(FuncId(func as u32), &words(a, n))
    })
}

/// `method_new` を実行する。メソッド自身の辞書は、引数の受け渡し用のバッファに `n` 個ある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn krt_method_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    class: u64,
    method: u64,
    dict: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt.method_new(
            ClassId(class as u32),
            method as u32,
            Word(dict),
            &words(a, n),
        )
    })
}

/// `dict_inst` を実行する。前提の辞書は、引数の受け渡し用のバッファに `n` 個ある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_dict_inst(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    inst: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        it.rt
            .dict_inst(keleut_lir::InstanceId(inst as u32), &words(a, n))
    })
}

/// `kind` が 0 なら `dict_record`、1 なら `dict_variant` を実行する。
///
/// 項目は、引数の受け渡し用のバッファに、ラベル、辞書の順で `n` 組ある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_dict_struct(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    kind: u64,
    n: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, a| {
        let fs: Vec<(LabelId, Word)> = (0..n as usize)
            .map(|i| (LabelId(a[2 * i] as u32), Word(a[2 * i + 1])))
            .collect();
        let k = if kind == 0 {
            Kind::DictRecord
        } else {
            Kind::DictVariant
        };
        it.rt.dict_struct(k, &fs)
    })
}

/// `ref_new` を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_ref_new(ctx: *mut NativeCtx, me: u64, sp: u64, x: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| it.rt.ref_new(Word(x)))
}

/// `marray_new` を実行する。`n` は `i32` として読み、負なら 0 として扱う。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_marray_new(
    ctx: *mut NativeCtx,
    me: u64,
    sp: u64,
    n: u64,
    x: u64,
) -> u64 {
    alloc_with(ctx, me, sp, |it, _| {
        let n = (n as i32).max(0) as usize;
        it.rt.array_new(Kind::MArray, n, Word(x))
    })
}

/// `marray_freeze` を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_marray_freeze(ctx: *mut NativeCtx, me: u64, sp: u64, a: u64) -> u64 {
    alloc_with(ctx, me, sp, |it, _| it.rt.array_freeze(Word(a)))
}

/// `str.eq` などの文字列の比較を実行する。
///
/// `op` は [`crate::bytecode::cmp_sub`] が返すサブコードである。
/// 比較の結果が真なら 1、偽なら 0 を返す。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_str_cmp(ctx: *mut NativeCtx, op: u64, a: u64, b: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let o = keleut_rtcore::compare_bytes(it.rt.str_bytes(Word(a)), it.rt.str_bytes(Word(b)));
        use std::cmp::Ordering::*;
        u64::from(match op {
            0 => o == Equal,
            1 => o == Less,
            2 => o != Greater,
            3 => o == Greater,
            _ => o != Less,
        })
    })
}

/// `has` が 0 なら `rec_field`、1 なら `rec_has` を実行する。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_rec_field(
    ctx: *mut NativeCtx,
    has: u64,
    v: u64,
    label: u64,
    nth: u64,
) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let l = LabelId(label as u32);
        if has == 0 {
            it.rt.rec_field(Word(v), l, nth as u32).0
        } else {
            u64::from(it.rt.rec_has(Word(v), l, nth as u32))
        }
    })
}

/// インスタンスの命令を実行する。`op` は 0 が `inst_is_init`、1 が `inst_mark_init`、2 が `inst_set_value`、3 が `inst_get_value` である。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_inst(ctx: *mut NativeCtx, op: u64, inst: u64, m: u64, x: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let (i, m) = (inst as usize, m as usize);
        match op {
            0 => u64::from(it.rt.inst_init[i]),
            1 => {
                it.rt.inst_init[i] = true;
                0
            }
            2 => {
                it.rt.inst_values[i][m] = Word(x);
                0
            }
            _ => it.rt.inst_values[i][m].0,
        }
    })
}

/// 配列の要素を読み書きする。`op` は 0 が `array_get` と `marray_get`、1 が `marray_set` である。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_array(ctx: *mut NativeCtx, op: u64, a: u64, i: u64, x: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        if op == 0 {
            it.rt.array_get(Word(a), i as i32).0
        } else {
            it.rt.array_set(Word(a), i as i32, Word(x));
            0
        }
    })
}

/// `console_write` を実行する。書けたら 0 を返し、失敗したらエラーを状態ワードに設定して 1 を返す。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_console_write(ctx: *mut NativeCtx, s: u64) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let bytes = it.rt.str_bytes(Word(s));
        // SAFETY: 文字列オブジェクトは移動せず、書いている間に GC は起きない。
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
        match it.rt.out.write(bytes) {
            Ok(()) => 0,
            Err(e) => {
                fail(it, Stop::from(e));
                1
            }
        }
    })
}

/// フレーム(位置 `fpos`、フレームヘッダ `fp`)の上に関数 `func` のフレームを置き、引数を書く。
///
/// 末尾呼び出し(`tail`)なら、現在のフレームの位置に置き、呼び出し元と深さを引き継ぐ。
/// これはインタプリタの末尾呼び出しと同じである。
/// フレームを置けなければ偽を返す。
#[allow(clippy::too_many_arguments)]
fn place_with_args(
    it: &mut Interp<'_>,
    ctx: *mut NativeCtx,
    fpos: u64,
    fp: *mut u64,
    func: FuncId,
    env: Word,
    args: &[Word],
    tail: bool,
) -> bool {
    // SAFETY: fp は現在のフレームのヘッダを指す。
    let (fid, caller, depth) = unsafe { (*fp & 0xffff_ffff, *fp.add(3), *fp.add(1)) };
    let (at, caller) = if tail {
        (fpos, caller)
    } else {
        (fpos + native(it).frames[fid as usize].0 as u64, fpos)
    };
    let Some(p) = place(it, ctx, u64::from(func.0), fpos, at, caller, env.0) else {
        return false;
    };
    // SAFETY: 置いたフレームは、関数の引数の個数以上のスロットを持つ。
    unsafe {
        if tail {
            *p.add(1) = depth;
        }
        for (i, a) in args.iter().enumerate() {
            *p.add(HEADER_WORDS + i) = a.0;
        }
    }
    true
}

/// JIT のフレーム(位置 `fpos`、フレームヘッダ `fp`)のエフェクトの命令を実行する。`idx` は、関数の中でのエフェクトの命令の番号である。
///
/// JIT のコードは、オペランドをすべてスロットに書いてから呼ぶ。
/// メモリを確保する命令なら、`sp` はその安全点の番号で、JIT のコードは、生存している、`any` の領域のレジスタもスロットに書いてある。
/// それ以外の命令では、`sp` は `u64::MAX` である。
/// 呼び出しの一種である命令なら、JIT のコードは、再開点で生存しているレジスタもスロットに書き、フレームヘッダに再開点を書いてある。
/// 返り値は [`fx`] の定数のどれかである。
/// 書き込み先のある命令では、命令の結果の値は書き込み先のスロットにある。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_fx(
    ctx: *mut NativeCtx,
    fpos: u64,
    fp: *mut u64,
    idx: u64,
    sp: u64,
) -> i64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        sample(it);
        if sp != u64::MAX {
            it.rt.gc.at_safepoint(fpos, sp as u32);
        }
        match it.native_fx(fpos, fp, idx as u32) {
            Ok(NativeFx::Next) => fx::NEXT,
            Ok(NativeFx::Enter(func, env, args)) => {
                let placed = place_with_args(it, ctx, fpos, fp, func, env, &args, false);
                it.give_args(args);
                if placed { fx::CALL } else { fx::ERR }
            }
            Ok(NativeFx::Value(v)) => {
                // SAFETY: ctx は有効なコンテキストである。
                unsafe { (*ctx).result = v.0 };
                fx::VALUE
            }
            Ok(NativeFx::Redeliver(v)) => {
                // SAFETY: ctx は有効なコンテキストである。
                unsafe { (*ctx).result = v.0 };
                fx::REDELIVER
            }
            Err(e) => {
                fail(it, e);
                fx::ERR
            }
        }
    })
}

/// JIT のフレームのエフェクトの終端命令を実行する。`idx` は、関数の中でのエフェクトの終端命令の番号である。
///
/// オペランドはスロットにある。
/// 番兵を返す終端命令(`propagate` と、状態ワードに目印を設定する `clause_value`、`clause_matched`、`tail_resume`)のときと、エラーのときは、[`fx::ERR`] を返す。
/// 末尾呼び出しの終端命令のときは、呼び出し先のフレームを現在のフレームの位置に置いて [`fx::CALL`] を返し、置けなければ [`fx::ERR`] を返す。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_fx_term(
    ctx: *mut NativeCtx,
    fpos: u64,
    fp: *mut u64,
    idx: u64,
) -> i64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        sample(it);
        match it.native_fx_term(fpos, fp, idx as u32) {
            Ok(None) => fx::ERR,
            Ok(Some((func, env, args))) => {
                let placed = place_with_args(it, ctx, fpos, fp, func, env, &args, true);
                it.give_args(args);
                if placed { fx::CALL } else { fx::ERR }
            }
            Err(e) => {
                fail(it, e);
                fx::ERR
            }
        }
    })
}

/// JIT のフレームの `perform` の呼び出しが `r` を返したときに呼ぶ。処理は `Interp::native_perform_back` が行う。
///
/// 1 を返したら、JIT のコードは `r` を `perform` の結果にして続ける。
/// 0 を返したら、JIT のコードは番兵を返す。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_perform_back(
    ctx: *mut NativeCtx,
    fpos: u64,
    fp: *mut u64,
    r: u64,
) -> u64 {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        if native(it).fatal.is_some() {
            return 0;
        }
        match it.native_perform_back(fpos, fp, Word(r)) {
            Ok(b) => u64::from(b),
            Err(e) => {
                set_fatal(it, e);
                0
            }
        }
    })
}

/// JIT のコードが、実行されないはずの経路に入ったときに呼ぶ。内部エラーを保存する。
///
/// `what` は、その箇所を表す番号である。
///
/// # Safety
///
/// 事前条件は [`krt_prep`] と同じである。
pub unsafe extern "C" fn krt_bug(ctx: *mut NativeCtx, fid: u64, what: u64) {
    guarded(ctx, || {
        // SAFETY: 事前条件により、ctx はネイティブの呼び出しの最中のコンテキストである。
        let it = unsafe { interp(ctx) };
        let sym = &it.prog.funcs[fid as usize].sym;
        set_fatal(
            it,
            Stop::Internal(format!(
                "JIT のコードが起きないはずの経路に入りました({sym}、{what})"
            )),
        );
    })
}

/// JIT のコードが呼ぶランタイム関数の名前とアドレスを返す。LLJIT に、絶対アドレスのシンボルとして登録する。
pub fn helper_symbols() -> Vec<(&'static str, usize)> {
    vec![
        ("krt_prep", krt_prep as *const () as usize),
        ("krt_store_regs", krt_store_regs as *const () as usize),
        ("krt_sentinel", krt_sentinel as *const () as usize),
        ("krt_interp_run", krt_interp_run as *const () as usize),
        ("krt_dispatch", krt_dispatch as *const () as usize),
        ("krt_depth_error", krt_depth_error as *const () as usize),
        ("krt_error", krt_error as *const () as usize),
        ("krt_unimpl", krt_unimpl as *const () as usize),
        ("krt_box", krt_box as *const () as usize),
        ("krt_str_concat", krt_str_concat as *const () as usize),
        ("krt_str_sub", krt_str_sub as *const () as usize),
        ("krt_show", krt_show as *const () as usize),
        ("krt_ctor_new", krt_ctor_new as *const () as usize),
        ("krt_variant_new", krt_variant_new as *const () as usize),
        ("krt_rec_new", krt_rec_new as *const () as usize),
        ("krt_rec_op", krt_rec_op as *const () as usize),
        ("krt_closure_new", krt_closure_new as *const () as usize),
        ("krt_method_new", krt_method_new as *const () as usize),
        ("krt_dict_inst", krt_dict_inst as *const () as usize),
        ("krt_dict_struct", krt_dict_struct as *const () as usize),
        ("krt_ref_new", krt_ref_new as *const () as usize),
        ("krt_marray_new", krt_marray_new as *const () as usize),
        ("krt_marray_freeze", krt_marray_freeze as *const () as usize),
        ("krt_str_cmp", krt_str_cmp as *const () as usize),
        ("krt_rec_field", krt_rec_field as *const () as usize),
        ("krt_inst", krt_inst as *const () as usize),
        ("krt_array", krt_array as *const () as usize),
        ("krt_console_write", krt_console_write as *const () as usize),
        ("krt_fx", krt_fx as *const () as usize),
        ("krt_fx_term", krt_fx_term as *const () as usize),
        ("krt_perform_back", krt_perform_back as *const () as usize),
        ("krt_bug", krt_bug as *const () as usize),
        ("krt_sin", keleut_rtcore::sys::krt_sin as *const () as usize),
        ("krt_cos", keleut_rtcore::sys::krt_cos as *const () as usize),
        ("krt_exp", keleut_rtcore::sys::krt_exp as *const () as usize),
        ("krt_log", keleut_rtcore::sys::krt_log as *const () as usize),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn 文脈の欄の位置は_jit_のものと同じ() {
        assert_eq!(offset_of!(NativeCtx, callee) as u64, off::CALLEE);
        assert_eq!(offset_of!(NativeCtx, env) as u64, off::ENV);
        assert_eq!(offset_of!(NativeCtx, cur_pos) as u64, off::CUR_POS);
        assert_eq!(offset_of!(NativeCtx, cur_fp) as u64, off::CUR_FP);
        assert_eq!(offset_of!(NativeCtx, depth) as u64, off::DEPTH);
        assert_eq!(offset_of!(NativeCtx, limit) as u64, off::LIMIT);
        assert_eq!(offset_of!(NativeCtx, stack_low) as u64, off::STACK_LOW);
        assert_eq!(offset_of!(NativeCtx, args) as u64, off::ARGS);
        assert_eq!(offset_of!(NativeCtx, result) as u64, off::RESULT);
        assert_eq!(offset_of!(NativeCtx, globals) as u64, off::GLOBALS);
        assert_eq!(offset_of!(NativeCtx, global_init) as u64, off::GLOBAL_INIT);
        assert_eq!(offset_of!(NativeCtx, strings) as u64, off::STRINGS);
        assert_eq!(offset_of!(NativeCtx, stack_soft) as u64, off::STACK_SOFT);
        assert_eq!(offset_of!(NativeCtx, stub) as u64, off::STUB);
        assert_eq!(
            offset_of!(NativeCtx, force_interp) as u64,
            off::FORCE_INTERP
        );
        assert_eq!(offset_of!(NativeCtx, alt) as u64, off::ALT);
        assert_eq!(offset_of!(NativeCtx, win) as u64, off::WIN);
        assert_eq!(offset_of!(NativeCtx, frames) as u64, off::FRAMES);
        assert_eq!(offset_of!(NativeCtx, heap) as u64, off::HEAP);
        assert_eq!(TAILREQ.0 & 7, 4);
    }
}
