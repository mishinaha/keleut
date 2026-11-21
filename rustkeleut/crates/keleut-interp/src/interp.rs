//! LIR を符号化したレジスタバイトコードを実行するインタプリタ。
//!
//! ディスパッチは、Rust の `match` を 1 つ持つ平坦なループで行う。
//! フレームは分割値スタックに置き、レイアウトは `keleut_rt::stack` が定める。
//! インタプリタの中の呼び出しは、ネイティブスタックを使わない。
//! 末尾呼び出しはフレームを再利用する。
//!
//! 非末尾呼び出しでは、呼び出し元のフレームヘッダに再開点を書いてから、呼び出し先のフレームを積む。
//! 呼び出し先が戻ったら、呼び出し元の再開点の番号で [`crate::bytecode::Code::resume`] を引き、結果の書き込み先と、実行を続ける位置を決める。
//!
//! メソッドを実行時に選ぶ呼び出しでは、辞書がユーザのインスタンスで、メソッドの実装の関数が辞書を受け取ることがある。
//! このときは、実装の関数を先に評価してから、評価で得た関数を引数で呼ぶ(`doc/lir.md` の 13 節)。
//! 実装の関数が受け取る辞書は、インスタンスの前提の辞書と、メソッド自身の辞書である。
//! 実装の関数は通常のフレームとして積み、呼び出し元の再開点に [`PHASE`] フラグを付ける。
//! 実装の関数が戻ったら、呼び出しの命令を読み直し、戻った関数を同じ引数で呼ぶ。
//! 末尾呼び出し版の命令なら、このときにフレームを再利用する。
//! 引数は呼び出し元のフレームのスロットに残っている。
//! 実装の関数のフレームは呼び出し元のフレームの上に積むので、そのスロットを上書きしない。
//!
//! Rust で書いたランタイム関数がユーザのコードを呼ぶのは、構造的な `Eq` の比較(`dict_record`、`dict_variant`)だけである。
//! この比較は [`Interp::run`] を入れ子に呼ぶ。
//! 入れ子は、比較する値の中でレコードかヴァリアントの入れ子が 1 段深くなるごとに 1 つ増える。
//! 再帰する名目的な型(`Node({k: T})` のような型)では、値の入れ子の深さと同じ数だけ入れ子になる。
//! 入れ子 1 段は、論理的な深さを 2 つと、ネイティブスタックを約 1.5 KiB 使う。
//! そのため、既定のネイティブスタックのサイズ `S_native` では、ネイティブスタックを使い切る前に、論理的な深さが上限に達する。
//! 入れ子の入口では、スタック残量の検査でネイティブスタックの残りを調べる。
//! 切り替え先のスタックを設定していなければ([`Interp::set_stack_hop`])、残りが足りないときに内部エラーとして終了コード 70 で終了する。
//! 設定していれば、項目を比較する入口でネイティブスタックの残りが下限に近いときに、新しいスタックを持つスレッドに実行を切り替えて続ける。
//! そのため、`--native-stack` が小さくても、入れ子は論理的な深さの上限まで進む。
//!
//! ## エフェクト(`doc/lir.md` の 16 節)
//!
//! 実行時エラー、`depth_limit`、メモリ不足、出力エラーは、状態ワード `ctx.pending` に `ERROR` として設定して巻き戻す。
//! 状態ワードと番兵は、`doc/lir.md` の 16.1 節が定める。
//! 巻き戻しは [`Interp::unwind`] の 1 か所で行う。
//! 現在のフレームは番兵を返して戻る。
//! その後の動作は、呼び出し元が止まっている再開点の種類(再開点の表の項目が持つ)で決まる。
//!
//! - 通常の呼び出しなら、呼び出し元も番兵を返して戻る。
//!   状態ワードが `YIELD` ならフレームを値スタックに残して継続に含め、それ以外ならフレームを捨てる。
//!   どちらの場合も、`ctx.depth` を呼び出し元のフレームの深さに書き戻す。
//! - 番兵を受け取る呼び出し(`$rt_handle` の中の呼び出し)なら、結果のスロットに番兵を書いて実行を続ける。
//! - `perform` なら、節の選択の結果を状態ワードに設定する([`Interp::perform_unwound`])。
//!
//! 継続は、`perform` のフレームからハンドラのフレームのすぐ上のフレームまでを、呼び出し元を順にたどってコピーして作る。
//! 継続を値スタックに復元するときは、フレームがチャンクの境界をまたがないように、フレームを 1 つずつ配置し直して、ヘッダの 3 番のワード(呼び出し元の位置)を書き直す。
//! ヘッダの深さのワードは、継続をキャプチャしたハンドラ記録の `h` からの差で持ち、復元するときに復元先の深さを足す。
//! 復元したフレームが `$rt_handle` のフレームなら、そのハンドラ記録の `h` を新しい深さに書き直す。
//! `$rt_handle` の 0 番の引数がハンドラ記録で、`any` の領域の 0 番のスロットにある。
//!
//! 構造的な `Eq` の比較が入れ子に呼ぶ [`Interp::run`] の境界は、`DISCARD` と `ERROR` だけを外へ通す。
//! `Eq` のメソッドはエフェクトを持たないので、境界の外へ `YIELD` が届くことはない。
//! 届いたら、内部エラーとして終了コード 70 で終了する。
//!
//! ## GC(`keleut_rt::gc`)
//!
//! GC は、ランタイムがメモリを確保するときに起こる。
//! インタプリタは、確保の前に、現在のフレームの位置を `rt.gc.top` に設定し、そのフレームが止まっている点(再開点か安全点)を GC が正しく読めるようにしておく。
//!
//! - メモリを確保する命令では、その前に置いた命令 `safepoint` が、フレームの位置と安全点の番号を `rt.gc` に設定する。
//!   安全点はヘッダには書かない。
//! - ランタイムが命令の外で確保するとき(`perform` の引数の配列を作るとき)は、フレームが `perform` の再開点で止まっているので、位置だけを設定する。
//! - 構造的な `Eq` の比較の前には、ヘッダの点をその呼び出しの再開点にし、位置を設定してから、[`Interp::run`] を入れ子に呼ぶ。
//!
//! [`Interp::run`] は、入口で外側の最上段の位置を `rt.gc.outer` に積み、戻るときに外して `rt.gc.top` を元に戻す。
//! GC は、最上段から呼び出し元を順にたどり、呼び出し元がネイティブであることを表す値 `NATIVE_CALLER` に着いたら、積んである外側の位置から続ける。
//!
//! `YIELD` を伝播する間、`perform` のフレームから現在のフレームのすぐ上までのフレームは値スタックに残っていて、まだ継続にコピーしていない。
//! GC はこれらのフレームを、状態ワードの `YIELD` が持つ `perform` のフレームの位置からたどる。

use keleut_lir::{ClassId, ConvOp, FuncId, InstanceId, LabelId, Program, RtError, ShapeId, TypeId};
use keleut_rt::alloc::{self, Site};
use keleut_rt::stack::{HEADER_WORDS, NATIVE_CALLER};
use keleut_rt::value::Kind;
use keleut_rt::{CalleeView, DictView, Rt, Stop, Word};
use std::collections::HashMap;

use crate::bytecode::{self, Code, ResumeEntry, op, rk};
use keleut_lir::{ClauseClass, FxOp, FxTermOp, HandlerPart};
use keleut_rt::effect::{Cont, HandlerRec, Pending};

pub use keleut_rt::gc::{PHASE, YIELDED};

/// エフェクトの処理で見つけた不整合を、メッセージ `m` の内部エラー(`Stop::Internal`)にする。
fn bug(m: &str) -> Stop {
    Stop::Internal(m.to_string())
}

/// スタック残量の検査が、ネイティブスタックの終わりの手前に残す余裕のバイト数。
pub const NATIVE_MARGIN: usize = 256 * 1024;

/// 呼び出しの引数をどこから読むかを表す。
#[derive(Clone, Copy)]
enum Args {
    /// 現在のフレームのスロット番号の並び。ポインタは命令のワードを指す。
    Slots(*const u32, usize),
    /// 一様表現の値の並び。
    Words(*const Word, usize),
}

impl Args {
    fn len(self) -> usize {
        match self {
            Args::Slots(_, n) | Args::Words(_, n) => n,
        }
    }
}

/// 実行中のフレームの状態。
///
/// フレームの位置 `fpos`、ヘッダのアドレス `fp`、関数のバイトコード `code`、次に実行する命令の位置 `pc`、環境 `env` を持つ。
struct St {
    fpos: u64,
    fp: *mut u64,
    code: *const Code,
    pc: usize,
    env: Word,
}

impl St {
    #[inline]
    fn get(&self, i: u32) -> u64 {
        // SAFETY: i はこのフレームのスロットの番号で、フレームは生きている。
        unsafe { *self.fp.add(HEADER_WORDS + i as usize) }
    }

    #[inline]
    fn set(&mut self, i: u32, v: u64) {
        // SAFETY: get と同じ。
        unsafe { *self.fp.add(HEADER_WORDS + i as usize) = v }
    }

    #[inline]
    fn w(&self, i: u32) -> Word {
        Word(self.get(i))
    }

    #[inline]
    fn code(&self) -> &Code {
        // SAFETY: Code は Interp::codes の Box の中にあり、Interp が生きている間は移動も解放もされない。
        unsafe { &*self.code }
    }

    #[inline]
    fn depth(&self) -> u64 {
        // SAFETY: fp はこのフレームのヘッダを指す。
        unsafe { *self.fp.add(1) }
    }

    fn set_resume(&mut self, rp: u32) {
        // SAFETY: fp はこのフレームのヘッダを指す。
        unsafe {
            let h = *self.fp;
            *self.fp = (h & 0xffff_ffff) | (u64::from(rp) << 32);
        }
    }
}

/// インタプリタ。ランタイムコンテキスト、符号化済みのバイトコード、再利用するバッファを持つ。
pub struct Interp<'p> {
    /// ランタイムコンテキスト。
    pub rt: Rt<'p>,
    pub(crate) prog: &'p Program,
    codes: Vec<Option<Box<Code>>>,
    builtin_methods: HashMap<(u32, u32, u32), FuncId>,
    pub(crate) stack_low: usize,
    words: Vec<Word>,
    /// 呼び出しを行うエフェクトの命令と、末尾呼び出しを行うエフェクトの終端命令が、引数の並びに再利用するバッファ([`Interp::fx_call`]、[`Interp::fx_term_call`])。
    ///
    /// 使う間は `std::mem::take` で取り出し、呼び出しの後に [`Interp::give_args`] で戻す。
    /// 取り出している間に入れ子の呼び出しがこのバッファを使っても、空の `Vec` から作り直すだけなので、引数の並びは混ざらない。
    fx_args: Vec<Word>,
    /// `cont_resume` が配置したフレームの位置を入れる、再利用するバッファ。
    placed: Vec<u64>,
    /// 再開した継続から外したコピー領域。フレームのワードの `Vec` と、フレームごとのワード数の `Vec` の組である。
    /// `cont_make` が再利用する。
    cont_bufs: Vec<(Vec<u64>, Vec<u32>)>,
    rt_handle: Option<FuncId>,
    /// ネイティブのバックエンド(JIT)のエントリテーブルとコンテキスト。`None` なら、インタプリタだけで実行する([`crate::native`])。
    pub native: Option<Box<crate::native::Native>>,
    /// 現在のループの呼び出しで最下段にあるフレームの位置。
    ///
    /// ここでは、この [`Interp`] のディスパッチループを実行しているネイティブの呼び出し 1 回を、**ループの呼び出し**と呼ぶ。
    /// ループの呼び出しを 1 回始めるのは、[`Interp::run`]、JIT の関数が呼ぶ `run_activation`、`run_from` である。
    /// `run_from` は、JIT のフレームが継続を再開したときと、JIT のフレームの `cont_make` が継続を作れなかったときに呼ばれる。
    /// 最下段のフレームが、値か番兵を返して戻ると、ディスパッチループを抜けて、ネイティブの呼び出し元へその値を返す。
    /// 最下段のフレームヘッダの 3 番のワードは、[`Interp::run`] が始めたループの呼び出しでは [`NATIVE_CALLER`] である。
    /// JIT の関数が始めたループの呼び出し([`crate::native::krt_interp_run`])では、呼び出し元の JIT のフレームの位置である。
    /// `run_from` が始めたループの呼び出しでも、呼び出し元の JIT のフレームの位置である。
    pub(crate) bottom: u64,
    /// 構造的な `Eq` の比較の入れ子で、ネイティブスタックの残りが下限に近づいたときに実行を切り替える先の、新しいスタックの設定。
    ///
    /// スタックのサイズと、そのスタックでのスタック残量の検査の余裕の組で、どちらもバイト数である。
    /// `None` なら切り替えない。
    /// そのときは、入れ子の入口のスタック残量の検査で、内部エラーとして終了コード 70 で終了する([`Interp::set_stack_hop`])。
    stack_hop: Option<(usize, usize)>,
}

/// 構造的な `Eq` の項目を比較する入口で、ネイティブスタックの残りが、この量(バイト数)とスタック残量の検査の余裕の和を下回ったら、新しいスタックを持つスレッドに実行を切り替えて続ける([`Interp::set_stack_hop`])。
///
/// この量は、JIT の関数がエントリでインタプリタに切り替える下限([`crate::native::SOFT_MARGIN`])より大きくする。
/// 項目の比較 1 段(インタプリタで約 1.5 KiB)と、その中で JIT の関数が下限に達するまでに使う量を残すためである。
pub const HOP_MARGIN: usize = 2 * crate::native::SOFT_MARGIN as usize;

/// 再開した継続から外して再利用するコピー領域を、何組まで残すかの上限([`Interp::keep_cont_bufs`])。
const CONT_BUFS: usize = 4;

/// 再利用するコピー領域のワード数の上限。これより大きい領域は、再利用せずに解放する。
const CONT_BUF_WORDS: usize = 1 << 16;

/// `cont_make` が再利用する領域を選ぶときに、フレームのワードの `Vec` の容量に許す上限の最小値。
///
/// 容量の上限は、必要なワード数の 2 倍とこの値の大きいほうである。
const CONT_FIT_WORDS: usize = 64;

/// `cont_make` が再利用する領域を選ぶときに、フレームごとのワード数の `Vec` の容量に許す上限の最小値。
const CONT_FIT_FRAMES: usize = 16;

/// JIT のフレームがエフェクトの命令を実行した結果([`Interp::native_fx`])。
pub(crate) enum NativeFx {
    /// 結果を書き込み先のスロットに書いた。書き込み先のない命令では、何も書いていない。
    Next,
    /// このフレームの上で、関数を環境と引数で呼ぶ。番兵を受け取る呼び出しか `perform` である。
    Enter(FuncId, Word, Vec<Word>),
    /// 継続の再開によって、呼び出しの結果(値か番兵)が決まった。
    Value(Word),
    /// `cont_make` が継続を作れなかった。区間の最上段から巻き戻した結果を、ヘッダの再開点の呼び出し(本体の呼び出し)の結果として返す。
    Redeliver(Word),
}

/// 呼び出しを行うエフェクトの命令が、何を呼ぶかを表す([`Interp::fx_call`])。
enum FxCall {
    /// 呼ばずに結果を書いた。`call_clause` で深さが上限を超え、番兵を結果に書いた場合である。
    Done,
    /// 関数を、環境と引数で呼ぶ。
    Enter(FuncId, Word, Vec<Word>),
    /// 関数値を、引数で呼ぶ(`call_value` の規則に従う)。
    Value(Word, Vec<Word>),
    /// 継続を、値で再開する。
    Resume(Word, Word),
}

/// 構造的な `Eq` の比較の入れ子が実行を切り替える先の、新しいスタックの最小のサイズ(バイト数)。
///
/// 切り替える先のスタックのサイズは、ネイティブスタックのサイズ(`--native-stack`)とこの値の大きいほうにする。
/// `--native-stack 512k` と同じサイズにすると、スタック残量の検査の余裕と [`HOP_MARGIN`] を除いた残りでは、1 つのスレッドで 80 段ほどしか進まない。
/// 20 万段の入れ子では、2,600 個ほどのスレッドを作ることになる。
/// その状態でテストを並列に実行すると、ユーザごとのスレッド数の上限に達してスレッドを作れなくなり、内部エラーとして終了コード 70 で終了する。
pub const HOP_STACK_MIN: usize = 8 << 20;

/// フレームヘッダの 2 番のワード(2 つの領域のスロット数)から、ヘッダを含むフレームのワード数を求める。
fn frame_words(sz: u64) -> usize {
    HEADER_WORDS + (sz & 0xffff_ffff) as usize + (sz >> 32) as usize
}

/// 現在のスタックポインタに近いアドレスを返す。ローカル変数のアドレスで代用する。
pub(crate) fn approx_sp() -> usize {
    let x = 0u8;
    std::hint::black_box(&x) as *const u8 as usize
}

/// 未実装のプリミティブの実行時エラーを作る。
/// IRSpec 付録 B の 17 番(表示名と実装名が同じ)か、18 番(表示名と実装名が違い、メッセージに実装名も書く)である。
pub(crate) fn unimpl_prim(prog: &Program, g: keleut_lir::GlobalId) -> Stop {
    let gl = &prog.globals[g.index()];
    let name = match &gl.kind {
        keleut_lir::GlobalKind::Extern { name, .. } => name.as_str(),
        _ => gl.display.as_str(),
    };
    if prog.error_number(&RtError::UnimplPrim(g)) == 18 {
        Stop::runtime(format!(
            "未実装のプリミティブ: {}(実装名 {name} が見つかりません)",
            gl.display
        ))
    } else {
        Stop::runtime(format!("未実装のプリミティブ: {}", gl.display))
    }
}

fn conv_name(op: &ConvOp) -> &'static str {
    op.prim_name()
}

impl<'p> Interp<'p> {
    /// インタプリタを作る。`stack_low` はネイティブスタックの下限のアドレスで、0 ならスタック残量を検査しない。
    pub fn new(rt: Rt<'p>, stack_low: usize) -> Interp<'p> {
        let prog = rt.prog;
        let builtin_methods = prog
            .builtin_methods
            .iter()
            .map(|b| ((b.class.0, b.ty.0, b.method), b.func))
            .collect();
        Interp {
            rt,
            prog,
            codes: (0..prog.funcs.len()).map(|_| None).collect(),
            builtin_methods,
            stack_low,
            words: Vec::new(),
            fx_args: Vec::new(),
            placed: Vec::new(),
            cont_bufs: Vec::new(),
            rt_handle: prog.rt_handle,
            native: None,
            bottom: NATIVE_CALLER,
            stack_hop: None,
        }
    }

    /// インタプリタが実行するプログラムを `prog` に差し替える(`keleut repl` が使う。[`Rt::rebind`])。
    ///
    /// `prog` は、現在のプログラムの各表の後ろに項目を追加したもの(`keleut_lir::Extender`)か、使わない間だけ置いておく空のプログラムである。
    /// 符号化済みのバイトコードは、以前からある関数の分を残し、追加された関数の分の空きを作る。
    /// ランタイムコンテキストの表は拡張しないので、項目を追加したプログラムに差し替えた後で [`Rt::grow`] を呼ぶ。
    /// ネイティブのバックエンド(JIT)を付けたインタプリタには使えず、付けていればパニックする。
    /// JIT のエントリテーブルとコンパイルスレッドは前のプログラムに合わせて作ってあり、追加された関数を扱えないからである。
    pub fn rebind<'q>(self, prog: &'q Program) -> Interp<'q> {
        assert!(
            self.native.is_none(),
            "Interp::rebind: ネイティブの後段を付けたインタプリタは付け替えられません"
        );
        let Interp {
            rt,
            prog: _,
            mut codes,
            mut builtin_methods,
            stack_low,
            words,
            fx_args,
            placed,
            cont_bufs,
            rt_handle: _,
            native,
            bottom,
            stack_hop,
        } = self;
        if codes.len() < prog.funcs.len() {
            codes.resize_with(prog.funcs.len(), || None);
        }
        for b in &prog.builtin_methods {
            builtin_methods.insert((b.class.0, b.ty.0, b.method), b.func);
        }
        Interp {
            rt: rt.rebind(prog),
            prog,
            codes,
            builtin_methods,
            stack_low,
            words,
            fx_args,
            placed,
            cont_bufs,
            rt_handle: prog.rt_handle,
            native,
            bottom,
            stack_hop,
        }
    }

    /// 構造的な `Eq` の比較の入れ子が深くなったときに、サイズ `size` のスタックを持つ新しいスレッドに実行を切り替えて続けるようにする。
    ///
    /// `margin` は、新しいスタックでのスタック残量の検査の余裕である。
    /// 新しいスタックの下限は、新しいスレッドの入口でのスタックのアドレスから `size` を引き、`margin` を足したアドレスである。
    /// 構造的な `Eq` の比較は、値の入れ子 1 段ごとに [`Interp::run`] を入れ子に呼び、ネイティブスタックを使う。
    /// 切り替えるようにすると、入れ子を制限するのは論理的な深さの上限だけになる。
    /// `--native-stack` が小さくても、スタック残量の検査で先に終了コード 70 になることはない。
    /// JIT の関数の非末尾の再帰が、ネイティブスタックを下限の近くまで使った後でも同じである。
    pub fn set_stack_hop(&mut self, size: usize, margin: usize) {
        self.stack_hop = Some((size, margin));
    }

    /// ネイティブスタックの残りが下限に近ければ、`f` を新しいスタックを持つスレッドで実行する。そうでなければ、現在のスレッドで実行する。
    ///
    /// 仮想メモリを制限しているとき(`ulimit -v`)に新しいスレッドを作れなければ、スタックを半分ずつ小さくして作り直す(最小は [`HOP_STACK_MIN`])。
    /// 最小のサイズでも作れなければ、メモリ不足([`Stop::OutOfMemory`])を返す。
    /// このときは、`cancel` 節を実行して、終了コード 3 で終了する。
    /// 仮想メモリを制限していないのに作れなければ、内部エラー(終了コード 70)である。
    /// 新しいスレッドが実行している間、元のスレッドは待つだけで、インタプリタに触れない。
    /// スタックの下限と、JIT が読むコンテキストの下限のフィールドは、切り替えている間だけ新しいスタックのものにし、戻ったら元に戻す。
    fn maybe_hop<R, F: FnOnce(&mut Self) -> Result<R, Stop>>(&mut self, f: F) -> Result<R, Stop> {
        let Some((size, margin)) = self.stack_hop else {
            return f(self);
        };
        if self.stack_low == 0 || approx_sp() >= self.stack_low.saturating_add(HOP_MARGIN) {
            return f(self);
        }
        /// スレッドの間で値を渡すためのラッパ。元のスレッドは新しいスレッドが終わるまで待つので、2 つのスレッドが同時に触れることはない。
        struct Pass<T>(T);
        // SAFETY: 上の説明のとおり、ラッパの中の値を使うのは、どの時点でも 1 つのスレッドだけである。
        unsafe impl<T> Send for Pass<T> {}
        impl<T> Pass<T> {
            fn take(self) -> T {
                self.0
            }
        }
        let saved_low = self.stack_low;
        let saved_ctx = self
            .native
            .as_ref()
            .map(|n| (n.ctx.stack_low, n.ctx.stack_soft, n.sp0, n.min_sp));
        // 新しいスレッドを作れなければ、スタックを半分ずつ小さくして作り直す(最小は HOP_STACK_MIN)。
        // スレッドを作れなかったときはクロージャが実行されないので、ジョブ(self と f)はクロージャの外に置き、作れたスレッドがクロージャの中で取り出す。
        let mut job = Some((self as *mut Self, f));
        let mut size = size;
        let r = loop {
            let slot = Pass(&mut job as *mut Option<(*mut Self, F)>);
            let r = std::thread::scope(|s| {
                let h = std::thread::Builder::new()
                    .name("keleut-mutator".to_string())
                    .stack_size(size)
                    .spawn_scoped(s, move || {
                        keleut_rt::alloc::set_role(keleut_rt::alloc::Role::Exec);
                        // SAFETY: 元のスレッドは join で待っていて、その間 job と self に触れない。
                        let (me, f) =
                            unsafe { (*slot.take()).take() }.expect("仕事は 1 回だけ取り出す");
                        // SAFETY: 上と同じ。
                        let it = unsafe { &mut *me };
                        let sp0 = approx_sp();
                        it.stack_low = sp0.saturating_sub(size).saturating_add(margin);
                        if let Some(n) = it.native.as_mut() {
                            // 実行の統計(`KELEUT_JIT_STATS`)に使うスタックのアドレスは、切り替えた先では新しいスタックで測り、戻るときに元の値に戻す。
                            // 切り替えた先での使用量は統計に含めない。
                            n.sp0 = sp0;
                            n.min_sp = usize::MAX;
                        }
                        Pass(f(it))
                    });
                match h {
                    Ok(h) => match h.join() {
                        Ok(r) => Some(r.take()),
                        Err(p) => std::panic::resume_unwind(p),
                    },
                    Err(e) => {
                        if keleut_rt::alloc::address_space_limit().is_none() {
                            // 仮想メモリを制限していないのに作れないのは、処理系の内部エラーである。
                            Some(Err(Stop::Internal(format!(
                                "新しいスタックのスレッドを作れません({size} バイトのスタック): {e}"
                            ))))
                        } else {
                            None
                        }
                    }
                }
            });
            if let Some(r) = r {
                break r;
            }
            if size <= HOP_STACK_MIN {
                // 最小のスタックでも作れない。ネイティブスタックに使うメモリを確保できないので、メモリ不足(`Stop::OutOfMemory`)を返す。
                // このときは、`cancel` 節を実行して、終了コード 3 で終了する。
                break Err(Stop::OutOfMemory);
            }
            size = (size / 2).max(HOP_STACK_MIN);
        };
        self.stack_low = saved_low;
        if let (Some(n), Some((low, soft, sp0, min_sp))) = (self.native.as_mut(), saved_ctx) {
            n.ctx.stack_low = low;
            n.ctx.stack_soft = soft;
            n.sp0 = sp0;
            n.min_sp = min_sp;
        }
        r
    }

    /// 関数 `f` のバイトコードへのポインタを返す。まだ符号化していなければ、符号化する。
    pub(crate) fn code(&mut self, f: FuncId) -> Result<*const Code, Stop> {
        if let Some(c) = &self.codes[f.index()] {
            return Ok(&**c as *const Code);
        }
        let c = bytecode::encode(self.prog, &self.rt, f).map_err(Stop::Internal)?;
        let b = Box::new(c);
        let p = &*b as *const Code;
        self.codes[f.index()] = Some(b);
        Ok(p)
    }

    /// 関数 `f` のバイトコードを返す。まだ符号化していなければ、符号化する(`keleut dump bytecode` が使う)。
    pub fn code_of(&mut self, f: FuncId) -> Result<&Code, Stop> {
        let p = self.code(f)?;
        // SAFETY: Code は self.codes の Box の中にある。
        Ok(unsafe { &*p })
    }

    /// スタック残量の検査。ネイティブスタックのアドレスが下限を下回っていれば、内部エラーを返す。
    pub(crate) fn check_native_stack(&self) -> Result<(), Stop> {
        if self.stack_low != 0 && approx_sp() < self.stack_low {
            return Err(Stop::Internal(
                "ネイティブのスタックの予備を使い切りました".to_string(),
            ));
        }
        Ok(())
    }

    /// 関数 `func` を環境 `env` と引数 `args` で呼び、返り値を返す。
    ///
    /// フレームは論理的な位置 `base`(現在のフレームの終わり)以降に置き、ヘッダの 3 番のワードに、呼び出し元がネイティブであることを表す `NATIVE_CALLER` を書く。
    /// 深さは変えない。呼び出し先は `ctx.depth` の深さで始まる。
    ///
    /// 入口で外側の最上段の位置を `rt.gc.outer` に積み、戻るときに外す([`keleut_rt::gc::GcState::enter_native`])。
    /// 詳しくは、モジュールの説明の GC の節に書いてある。
    pub fn run(&mut self, func: FuncId, env: Word, args: &[Word], base: u64) -> Result<Word, Stop> {
        self.check_native_stack()?;
        let saved = self.rt.gc.enter_native();
        let bottom = self.bottom;
        let r = self.run_inner(func, env, args, base);
        self.bottom = bottom;
        self.rt.gc.leave_native(saved);
        r
    }

    fn run_inner(
        &mut self,
        func: FuncId,
        env: Word,
        args: &[Word],
        base: u64,
    ) -> Result<Word, Stop> {
        let native = self.native_target(func)?;
        let (fw, any, raw) = self.frame_shape(func)?;
        let fpos = self.rt.stack.place(base, fw)?;
        let fp = self.rt.stack.ptr_at(fpos);
        // SAFETY: fp から始まるフレームのワード数の範囲は、値スタックのチャンクの中にある。
        unsafe {
            *fp = u64::from(func.0);
            *fp.add(1) = self.rt.depth;
            *fp.add(2) = u64::from(any) | (u64::from(raw) << 32);
            *fp.add(3) = NATIVE_CALLER;
            for (i, a) in args.iter().enumerate() {
                *fp.add(HEADER_WORDS + i) = a.0;
            }
        }
        self.bottom = fpos;
        if let Some(addr) = native {
            return self.call_native(addr, func, env, fpos, fp);
        }
        let code = self.code(func)?;
        let mut st = St {
            fpos,
            fp,
            code,
            pc: 0,
            env,
        };
        let v = self.exec(&mut st)?;
        if v == crate::native::TAILREQ {
            // 最下段のフレームが JIT の関数を末尾呼び出しで呼んだ。呼び出し先のフレームは配置してある。
            return self.call_native_request();
        }
        Ok(v)
    }

    /// JIT の関数が始めるループの呼び出し([`crate::native::krt_interp_run`])。
    ///
    /// 関数 `func` のフレームは、呼び出し元が位置 `pos`(ヘッダのアドレスは `fp`)に配置してある。
    /// フレームヘッダの 3 番のワードは呼び出し元の JIT のフレームの位置なので、継続をキャプチャするときは、このループの呼び出しの境界をまたいでフレームをたどれる。
    /// このフレームを最下段としてディスパッチループを実行し、最下段のフレームが返した値か番兵、または末尾呼び出しの依頼 [`crate::native::TAILREQ`] を返す。
    pub(crate) fn run_activation(
        &mut self,
        func: FuncId,
        env: Word,
        pos: u64,
        fp: *mut u64,
    ) -> Result<Word, Stop> {
        self.check_native_stack()?;
        let code = self.code(func)?;
        let saved = self.bottom;
        self.bottom = pos;
        let mut st = St {
            fpos: pos,
            fp,
            code,
            pc: 0,
            env,
        };
        let r = self.exec(&mut st);
        self.bottom = saved;
        r
    }

    /// 関数 `f` のフレームの大きさを返す。
    ///
    /// ヘッダを含むワード数、`any` の領域のスロット数、ボックス化していない値の領域のスロット数の組である。
    pub(crate) fn frame_shape(&self, f: FuncId) -> Result<(usize, u32, u32), Stop> {
        let fr = self.prog.funcs[f.index()]
            .frame
            .as_ref()
            .ok_or_else(|| Stop::Internal(format!("関数 {} にフレームがありません", f.0)))?;
        Ok((
            HEADER_WORDS + (fr.any_slots + fr.raw_slots) as usize,
            fr.any_slots,
            fr.raw_slots,
        ))
    }

    /// 現在のフレームの上に呼び出し先のフレームを積み、呼び出し先の先頭に制御を移す。
    ///
    /// 呼び出し先が JIT の関数なら、フレームを積んだ後にネイティブの呼び出しで実行し、戻った値を現在のフレームに渡す([`Interp::native_returned`])。
    /// このとき、`st` は現在のフレームのままである。
    /// `Some` を返すのは、JIT の関数が実装の関数で、それが戻った後に行う末尾呼び出し版の呼び出しが、最下段のフレーム(`self.bottom`)で結果を出したときである。
    /// 結果は、構造的な `Eq` の比較の値か、JIT の関数への末尾呼び出しの依頼 [`crate::native::TAILREQ`] である。
    fn enter(
        &mut self,
        st: &mut St,
        func: FuncId,
        env: Word,
        args: Args,
    ) -> Result<Option<Word>, Stop> {
        let top = st.fpos + st.code().frame_words as u64;
        let native = self.native_target(func)?;
        let (fw, any, raw) = self.frame_shape(func)?;
        let npos = self.rt.stack.place(top, fw)?;
        let nfp = self.rt.stack.ptr_at(npos);
        // SAFETY: nfp から始まるフレームのワード数の範囲はチャンクの中にあり、呼び出し元のフレーム(st.fp)とは重ならない。
        unsafe {
            match args {
                Args::Slots(p, n) => {
                    for i in 0..n {
                        *nfp.add(HEADER_WORDS + i) = st.get(*p.add(i));
                    }
                }
                Args::Words(p, n) => {
                    for i in 0..n {
                        *nfp.add(HEADER_WORDS + i) = (*p.add(i)).0;
                    }
                }
            }
            *nfp = u64::from(func.0);
            *nfp.add(1) = 0;
            *nfp.add(2) = u64::from(any) | (u64::from(raw) << 32);
            *nfp.add(3) = st.fpos;
        }
        if let Some(addr) = native {
            let v = self.call_native(addr, func, env, npos, nfp)?;
            // 呼び出しの間に値スタックの使用中のチャンクが変わることがあるので、使用中のチャンクを現在のフレームの位置に戻す。
            self.rt.stack.ptr_at(st.fpos);
            return self.native_returned(st, v);
        }
        let code = self.code(func)?;
        *st = St {
            fpos: npos,
            fp: nfp,
            code,
            pc: 0,
            env,
        };
        Ok(None)
    }

    /// 現在のフレームを再利用して、呼び出し先の先頭に制御を移す(末尾呼び出し)。
    ///
    /// 呼び出し先が JIT の関数で、現在のフレームが最下段(`self.bottom`)なら、フレームを配置してから、末尾呼び出しの依頼 [`crate::native::TAILREQ`] を返す。
    /// 実際の呼び出しは、ネイティブの呼び出し元が行う。
    /// 最下段でなければ、JIT の関数を通常のネイティブの呼び出しで実行する。
    /// 配置したフレームの呼び出し元は、現在のフレームの呼び出し元である。
    /// 戻った値は、その呼び出し元に渡す。
    fn tail_enter(
        &mut self,
        st: &mut St,
        func: FuncId,
        env: Word,
        args: Args,
    ) -> Result<Option<Word>, Stop> {
        let mut buf = std::mem::take(&mut self.words);
        buf.clear();
        // SAFETY: 引数は、現在のフレームのスロットか、生きている値の並びから読む。
        unsafe {
            match args {
                Args::Slots(p, n) => {
                    for i in 0..n {
                        buf.push(st.w(*p.add(i)));
                    }
                }
                Args::Words(p, n) => {
                    for i in 0..n {
                        buf.push(*p.add(i));
                    }
                }
            }
        }
        // SAFETY: fp は現在のフレームのヘッダを指す。
        let (caller, depth) = unsafe { (*st.fp.add(3), *st.fp.add(1)) };
        let native = self.native_target(func)?;
        let (fw, any, raw) = self.frame_shape(func)?;
        let npos = self.rt.stack.place(st.fpos, fw)?;
        let nfp = self.rt.stack.ptr_at(npos);
        // SAFETY: nfp から始まるフレームのワード数の範囲は、チャンクの中にある。
        unsafe {
            for (i, a) in buf.iter().enumerate() {
                *nfp.add(HEADER_WORDS + i) = a.0;
            }
            *nfp = u64::from(func.0);
            *nfp.add(1) = depth;
            *nfp.add(2) = u64::from(any) | (u64::from(raw) << 32);
            *nfp.add(3) = caller;
        }
        self.words = buf;
        let at_bottom = st.fpos == self.bottom;
        if at_bottom {
            self.bottom = npos;
        }
        if let Some(addr) = native {
            if at_bottom {
                self.native_request(func, env, npos, nfp);
                return Ok(Some(crate::native::TAILREQ));
            }
            let v = self.call_native(addr, func, env, npos, nfp)?;
            self.frame_at(st, caller)?;
            if v == Word::SENTINEL {
                self.rt.depth = st.depth();
            }
            return self.native_returned(st, v);
        }
        let code = self.code(func)?;
        *st = St {
            fpos: npos,
            fp: nfp,
            code,
            pc: 0,
            env,
        };
        Ok(None)
    }

    /// 値 `v` を返して、現在のフレームから戻る。現在のフレームが最下段(`self.bottom`)なら `Some(v)` を返す。
    fn ret(&mut self, st: &mut St, v: Word) -> Result<Option<Word>, Stop> {
        // SAFETY: fp は現在のフレームのヘッダを指す。
        let caller = unsafe { *st.fp.add(3) };
        if caller == NATIVE_CALLER || st.fpos == self.bottom {
            return Ok(Some(v));
        }
        self.frame_at(st, caller)?;
        self.deliver(st, v)
    }

    /// 呼び出し先が値 `v` を返して戻ったときに呼ぶ。現在のフレーム `st` のヘッダの再開点から実行を続ける。
    fn deliver(&mut self, st: &mut St, v: Word) -> Result<Option<Word>, Stop> {
        // SAFETY: fp は現在のフレームのヘッダを指す。
        let rp = (unsafe { *st.fp } >> 32) as u32;
        if rp & PHASE != 0 {
            return self.phase2(st, rp & !PHASE, v);
        }
        let e = st.code().resume[(rp & !YIELDED) as usize];
        if e.kind == rk::PERFORM && rp & YIELDED == 0 {
            // 分類 T の操作節(IRSpec 9.2 節)の末尾 resume(v) である。連鎖を perform の時点のものに戻す。
            let (_, saved) = self.perform_hr(st, e);
            self.rt.chain = saved;
            self.rt.depth = st.depth();
        }
        st.set(e.dst, v.0);
        st.pc = e.pc_after as usize;
        Ok(None)
    }

    /// ネイティブの呼び出しで実行した関数が `v` を返して戻ったときに呼ぶ。`st` はその呼び出し元のフレームである。
    ///
    /// `v` が番兵なら、`ctx.depth` を呼び出し元の深さに書き戻し、[`Interp::receive`] で受け取る。
    /// 番兵を受け取らないフレームなら、`Err(Stop::Pending)` を返して巻き戻しを続ける。
    /// このときは、現在のフレームも番兵を返して戻る。
    fn native_returned(&mut self, st: &mut St, v: Word) -> Result<Option<Word>, Stop> {
        if v == Word::SENTINEL {
            self.rt.depth = st.depth();
            if self.receive(st)? {
                return Ok(None);
            }
            return Err(Stop::Pending);
        }
        self.deliver(st, v)
    }

    /// `st` が位置 `pos` のフレームを指すようにする。`ctx.depth` と `st.pc` は変えない。
    fn frame_at(&mut self, st: &mut St, pos: u64) -> Result<(), Stop> {
        let fp = self.rt.stack.ptr_at(pos);
        // SAFETY: fp はフレームヘッダを指す。
        let h0 = unsafe { *fp };
        let code = self.code(FuncId(h0 as u32))?;
        st.fpos = pos;
        st.fp = fp;
        st.code = code;
        st.env = Word(0);
        Ok(())
    }

    /// `st` が位置 `pos` のフレームを指すようにし、`ctx.depth` をそのフレームの深さにする。
    fn move_to(&mut self, st: &mut St, pos: u64) -> Result<(), Stop> {
        self.frame_at(st, pos)?;
        self.rt.depth = st.depth();
        Ok(())
    }

    /// 現在のフレームから、番兵を返して戻る。この関数を呼ぶ前に、状態ワードを設定しておく。
    ///
    /// 番兵を受け取るフレームに着いたら、`None` を返す。
    /// 実行はそのフレームから続く。
    /// 最下段のフレーム(`self.bottom`)から戻るときは、番兵を `Some` で返す。
    fn unwind(&mut self, st: &mut St) -> Result<Option<Word>, Stop> {
        loop {
            // SAFETY: fp は現在のフレームのヘッダを指す。
            let caller = unsafe { *st.fp.add(3) };
            if caller == NATIVE_CALLER || st.fpos == self.bottom {
                return Ok(Some(Word::SENTINEL));
            }
            self.move_to(st, caller)?;
            if self.receive(st)? {
                return Ok(None);
            }
        }
    }

    /// 呼び出し先が番兵を返して戻り、現在のフレーム `st` に着いたときに呼ぶ。ヘッダに書いてある再開点の種類(再開点の表の項目が持つ)で、次の動作を決める。
    ///
    /// 番兵を受け取る呼び出しなら、結果のスロットに番兵を書いて `true` を返す。
    /// 実行はそこから続く。
    /// それ以外は `false` を返す。
    /// このときは、現在のフレームも番兵を返して戻る。
    fn receive(&mut self, st: &mut St) -> Result<bool, Stop> {
        // SAFETY: fp は現在のフレームのヘッダを指す。
        let rp = (unsafe { *st.fp } >> 32) as u32;
        if rp & PHASE != 0 {
            return Ok(false);
        }
        let e = st.code().resume[(rp & !YIELDED) as usize];
        match e.kind {
            rk::CATCH => {
                st.set(e.dst, Word::SENTINEL.0);
                st.pc = e.pc_after as usize;
                Ok(true)
            }
            rk::PERFORM => {
                if rp & YIELDED == 0 {
                    self.perform_unwound(st, e)?;
                }
                Ok(false)
            }
            _ => match &self.rt.pending {
                Pending::Yield { .. } if !e.yields => Err(bug(
                    "yield が noyield の再開点に届きました(may_yield の印の誤り)",
                )),
                Pending::Matched(_) | Pending::ClauseValue(_) | Pending::TailResume(_) => {
                    Err(bug("節の印がふつうの呼び出しに届きました"))
                }
                _ => Ok(false),
            },
        }
    }

    /// `perform` の再開点の表の項目 `e` から、その命令のオペランドのうち、ハンドラ記録と `perform` の時点の連鎖を読む。
    fn perform_hr(&self, st: &St, e: ResumeEntry) -> (Word, Word) {
        let w = &st.code().words;
        let pc = e.call_pc as usize;
        (st.w(w[pc + 4]), st.w(w[pc + 5]))
    }

    /// `perform` の操作の引数を、`Array` のオブジェクトにする。
    fn perform_args(&mut self, st: &St, e: ResumeEntry) -> Result<Word, Stop> {
        let w = &st.code().words;
        let pc = e.call_pc as usize;
        let n = w[pc + 3] as usize - 2;
        let mut xs = std::mem::take(&mut self.fx_args);
        xs.clear();
        {
            let _site = alloc::site(Site::CallArgs);
            xs.extend((0..n).map(|i| st.w(w[pc + 6 + i])));
        }
        // フレームは perform の再開点で止まっていて、引数はそのオペランドとして GC ルートになっている。
        self.rt.gc.at_frame(st.fpos);
        let a = self.rt.array_new(Kind::Array, n, Word::UNIT)?;
        for (i, x) in xs.iter().enumerate() {
            // SAFETY: a は n 個の要素を持つ配列のオブジェクトである。
            unsafe { keleut_rt::value::set(a, 1 + i, x.0) };
        }
        self.give_args(xs);
        Ok(a)
    }

    /// 節の候補が番兵を返して戻り、`perform` のフレームに着いたときに呼ぶ(`doc/lir.md` の 16.5 節の `perform` の手順 4)。
    /// 状態ワードを読み、節の選択の結果を状態ワードに設定する。
    fn perform_unwound(&mut self, st: &mut St, e: ResumeEntry) -> Result<(), Stop> {
        let (hr, saved) = self.perform_hr(st, e);
        match std::mem::take(&mut self.rt.pending) {
            Pending::Matched(j) => {
                self.rt.chain = saved;
                let class = self
                    .rt
                    .hr(hr)?
                    .ops
                    .get(j as usize)
                    .map(|o| o.1)
                    .ok_or_else(|| bug("範囲の外の節"))?;
                let args = match self.perform_args(st, e) {
                    Ok(a) => a,
                    Err(s) => {
                        self.rt.raise(s)?;
                        return Ok(());
                    }
                };
                if class == ClauseClass::N {
                    self.rt.pending = Pending::DiscardNever {
                        hr,
                        clause: j,
                        args,
                    };
                } else {
                    self.rt.pending = Pending::Yield {
                        hr,
                        clause: j,
                        args,
                        cut: saved,
                        top: st.fpos,
                    };
                    // SAFETY: fp はこのフレームのヘッダを指す。
                    let rp = (unsafe { *st.fp } >> 32) as u32;
                    st.set_resume(rp | YIELDED);
                }
            }
            Pending::ClauseValue(v) => {
                self.rt.chain = saved;
                self.rt.pending = Pending::DiscardValue { hr, v };
            }
            Pending::TailResume(_) => return Err(bug("末尾の resume の印が perform に届きました")),
            p @ Pending::Yield { .. } => self.rt.pending = p,
            Pending::None => return Err(bug("状態語の立っていない巻き戻し")),
            p => {
                self.rt.chain = saved;
                self.rt.pending = p;
            }
        }
        Ok(())
    }

    /// 実装の関数が関数値 `f` を返した後に、呼び出しの命令を読み直して `f` を呼ぶ。
    fn phase2(&mut self, st: &mut St, rp: u32, f: Word) -> Result<Option<Word>, Stop> {
        let e = st.code().resume[rp as usize];
        let pc = e.call_pc as usize;
        let w = &st.code().words;
        let opc = (w[pc] & 0xff) as u8;
        let (start, n, tail) = match opc {
            op::CALL_METHOD | op::TAIL_METHOD => {
                let no = w[pc + 5] as usize;
                (pc + 7 + no, w[pc + 6 + no] as usize, opc == op::TAIL_METHOD)
            }
            op::CALL_VALUE | op::TAIL_VALUE => (pc + 4, w[pc + 3] as usize, opc == op::TAIL_VALUE),
            _ => {
                return Err(Stop::Internal(format!(
                    "再開点 {rp} の命令がメソッドの呼び出しではありません"
                )));
            }
        };
        let args = Args::Slots(w[start..].as_ptr(), n);
        self.call_value(st, rp, f, args, tail)
    }

    /// 関数値 `f` を呼ぶ(`call_value` と `tail_call_value`)。
    fn call_value(
        &mut self,
        st: &mut St,
        rp: u32,
        f: Word,
        args: Args,
        tail: bool,
    ) -> Result<Option<Word>, Stop> {
        match self.rt.callee_view(f) {
            CalleeView::Closure(func) => {
                if tail {
                    self.tail_enter(st, func, f, args)
                } else {
                    st.set_resume(rp);
                    self.enter(st, func, f, args)
                }
            }
            CalleeView::ExternUnimpl(g) => Err(unimpl_prim(self.prog, g)),
            CalleeView::Method => {
                let (c, m, d, own) = self.rt.method_parts(f);
                self.select(st, rp, c, m, d, &own, args, tail)
            }
            CalleeView::Other => Err(Stop::Internal(format!(
                "呼べない値を呼びました: {}",
                String::from_utf8_lossy(&self.rt.show(f))
            ))),
        }
    }

    /// 辞書 `d` からメソッドを選んで呼ぶ(`call_method` と `tail_call_method`)。
    #[allow(clippy::too_many_arguments)]
    fn select(
        &mut self,
        st: &mut St,
        rp: u32,
        class: ClassId,
        m: u32,
        d: Word,
        own: &[Word],
        args: Args,
        tail: bool,
    ) -> Result<Option<Word>, Stop> {
        match self.rt.dict_view(d) {
            DictView::Builtin(c, t) => {
                let body = self.builtin_method(c, t, m)?;
                if tail {
                    self.tail_enter(st, body, Word(0), args)
                } else {
                    st.set_resume(rp);
                    self.enter(st, body, Word(0), args)
                }
            }
            DictView::Inst(i, premises) => {
                self.check_inst(i)?;
                let prog = self.prog;
                let im = &prog.instances[i.index()].methods[m as usize];
                if im.dicts == 0 {
                    let v = self.rt.inst_values[i.index()][m as usize];
                    return self.call_value(st, rp, v, args, tail);
                }
                let func = im.func;
                let mut dicts = premises;
                dicts.extend_from_slice(own);
                st.set_resume(rp | PHASE);
                self.enter(st, func, Word(0), Args::Words(dicts.as_ptr(), dicts.len()))
            }
            DictView::Record(_) | DictView::Variant(_) => {
                if args.len() != 2 {
                    return Err(Stop::Internal(
                        "構造的な Eq の引数の数が 2 でありません".into(),
                    ));
                }
                let (a, b) = self.args2(st, args);
                // 比較はメモリを確保し、ユーザのコードも呼ぶので、先にヘッダの再開点をこの呼び出しのものにする。
                // 末尾呼び出し版の項目(`tail_entry` が作る)は、LIR の生存しているレジスタの集合を持たない。
                // このときのルートは、Root で保持した両辺と辞書だけである。
                st.set_resume(rp);
                self.rt.gc.at_frame(st.fpos);
                let base = st.fpos + st.code().frame_words as u64;
                let r = Word::bool(self.eq_struct(class, d, a, b, base)?);
                if tail {
                    return self.ret(st, r);
                }
                let e = st.code().resume[rp as usize];
                st.set(e.dst, r.0);
                st.pc = e.pc_after as usize;
                Ok(None)
            }
            DictView::Other => Err(Stop::Internal(format!(
                "辞書でない値からメソッドを選びました: {}",
                String::from_utf8_lossy(&self.rt.show(d))
            ))),
        }
    }

    fn args2(&self, st: &St, args: Args) -> (Word, Word) {
        // SAFETY: 引数は 2 つある(呼び出し元が確かめた)。
        unsafe {
            match args {
                Args::Slots(p, _) => (st.w(*p), st.w(*p.add(1))),
                Args::Words(p, _) => (*p, *p.add(1)),
            }
        }
    }

    pub(crate) fn builtin_method(&self, c: ClassId, t: TypeId, m: u32) -> Result<FuncId, Stop> {
        self.builtin_methods
            .get(&(c.0, t.0, m))
            .copied()
            .ok_or_else(|| {
                Stop::Internal(format!(
                    "組み込みのメソッドがありません: クラス {} 型 {} メソッド {m}",
                    c.0, t.0
                ))
            })
    }

    pub(crate) fn check_inst(&self, i: InstanceId) -> Result<(), Stop> {
        if self.rt.inst_init[i.index()] {
            Ok(())
        } else {
            Err(keleut_rt::error::instance_not_ready(
                &self.prog.instances[i.index()].display,
            ))
        }
    }

    // ---- 入れ子の呼び出し(構造的な Eq) ----

    /// [`Interp::run`] を入れ子に呼ぶ。番兵が返ったら、入れ子の境界の外へ巻き戻す。
    fn run_nested(
        &mut self,
        func: FuncId,
        env: Word,
        args: &[Word],
        base: u64,
    ) -> Result<Word, Stop> {
        let v = self.run(func, env, args, base)?;
        if v != Word::SENTINEL {
            return Ok(v);
        }
        if self.rt.pending.is_yield() {
            return Err(bug(
                "継続をネイティブの境目(構造的な Eq の比べ方)の外へ取り出そうとしました",
            ));
        }
        Err(Stop::Pending)
    }

    /// 関数値 `f` を入れ子に呼ぶ。
    fn call_value_nested(&mut self, f: Word, args: &[Word], base: u64) -> Result<Word, Stop> {
        match self.rt.callee_view(f) {
            CalleeView::Closure(func) => self.run_nested(func, f, args, base),
            CalleeView::ExternUnimpl(g) => Err(unimpl_prim(self.prog, g)),
            CalleeView::Method => {
                let (c, m, d, own) = self.rt.method_parts(f);
                self.call_method_nested(c, m, d, &own, args, base)
            }
            CalleeView::Other => Err(Stop::Internal("呼べない値を呼びました".into())),
        }
    }

    /// 辞書からメソッドを選んで入れ子に呼ぶ。
    fn call_method_nested(
        &mut self,
        class: ClassId,
        m: u32,
        d: Word,
        own: &[Word],
        args: &[Word],
        base: u64,
    ) -> Result<Word, Stop> {
        match self.rt.dict_view(d) {
            DictView::Builtin(c, t) => {
                let body = self.builtin_method(c, t, m)?;
                self.run_nested(body, Word(0), args, base)
            }
            DictView::Inst(i, premises) => {
                self.check_inst(i)?;
                let prog = self.prog;
                let im = &prog.instances[i.index()].methods[m as usize];
                let f = if im.dicts == 0 {
                    self.rt.inst_values[i.index()][m as usize]
                } else {
                    let func = im.func;
                    let mut dicts = premises;
                    dicts.extend_from_slice(own);
                    let mark = self.root_all(args);
                    let f = self.run_nested(func, Word(0), &dicts, base);
                    self.rt.roots.truncate(mark);
                    f?
                };
                self.call_value_nested(f, args, base)
            }
            DictView::Record(_) | DictView::Variant(_) => Ok(Word::bool(
                self.eq_struct(class, d, args[0], args[1], base)?,
            )),
            DictView::Other => Err(Stop::Internal(
                "辞書でない値からメソッドを選びました".into(),
            )),
        }
    }

    /// `ws` の値をルートスタックに積み、積む前のルートスタックの長さを返す。
    fn root_all(&mut self, ws: &[Word]) -> usize {
        let mark = self.rt.roots.len();
        for w in ws {
            self.rt.roots.push(w.0);
        }
        mark
    }

    /// 構造的な `Eq` の比較(`doc/lir.md` の `call_method`)。
    ///
    /// 項目ごとに、深さを 1 つ増やし、その項目の辞書の `eq`(クラス `class` の 0 番のメソッド)を呼んで比較する。
    /// 比較する間、両辺と辞書をルートスタックに置く。
    pub(crate) fn eq_struct(
        &mut self,
        class: ClassId,
        d: Word,
        a: Word,
        b: Word,
        base: u64,
    ) -> Result<bool, Stop> {
        let mark = self.root_all(&[d, a, b]);
        let r = self.eq_struct_inner(class, d, a, b, base);
        self.rt.roots.truncate(mark);
        r
    }

    fn eq_struct_inner(
        &mut self,
        class: ClassId,
        d: Word,
        a: Word,
        b: Word,
        base: u64,
    ) -> Result<bool, Stop> {
        match self.rt.dict_view(d) {
            DictView::Record(fs) => {
                // 項目のラベル `l` が、辞書の中の同じラベルのうち 0 始まりで `k` 番目なら、両辺の外側から `k` 番目の `l` のフィールドを比較する。
                // これは、前の項目で同名のフィールドを取り除いた残りから、最も外側のフィールドを取り出すのと同じ結果になり、メモリを確保しない。
                // a、b、d は eq_struct がルートスタックに置いている。
                for (i, &(l, dl)) in fs.iter().enumerate() {
                    let k = fs[..i].iter().filter(|(m, _)| *m == l).count() as u32;
                    let xa = self.rt.rec_field(a, l, k);
                    let xb = self.rt.rec_field(b, l, k);
                    if !self.eq_item(class, dl, xa, xb, base)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            DictView::Variant(fs) => {
                let (ta, tb) = (self.rt.variant_tag(a), self.rt.variant_tag(b));
                if ta != tb {
                    return Ok(false);
                }
                let Some(&(_, dl)) = fs.iter().find(|(l, _)| l.0 as i32 == ta) else {
                    return Ok(false);
                };
                let (pa, pb) = (self.rt.variant_payload(a), self.rt.variant_payload(b));
                self.eq_item(class, dl, pa, pb, base)
            }
            _ => Err(Stop::Internal("構造的な辞書ではありません".into())),
        }
    }

    fn eq_item(
        &mut self,
        class: ClassId,
        d: Word,
        x: Word,
        y: Word,
        base: u64,
    ) -> Result<bool, Stop> {
        let saved = self.rt.depth;
        if self.rt.depth + 1 > self.rt.limit {
            return Err(self.rt.depth_error());
        }
        self.rt.depth += 1;
        let r = self.maybe_hop(|it| it.call_method_nested(class, 0, d, &[], &[x, y], base));
        self.rt.depth = saved;
        Ok(r?.as_bool())
    }

    // ---- エフェクト(`doc/lir.md` の 16 節) ----

    /// 節の候補のうち、番号が `start` 以上で、操作が `op` である最初の節を探す。
    fn next_clause(&self, hr: Word, op: u32, start: usize) -> Result<Option<(usize, Word)>, Stop> {
        let r = self.rt.hr(hr)?;
        Ok(r.ops
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, o)| o.0 == op)
            .map(|(j, o)| (j, o.2)))
    }

    /// IRSpec 付録 B の 9 番(どの節にも一致しない)の実行時エラーを作る。
    fn no_clause(&self, hr: Word, op: u32, args: &[Word]) -> Result<Stop, Stop> {
        let r = self.rt.hr(hr)?;
        let eff = &self.prog.effects[r.effect as usize];
        let shown: Vec<Vec<u8>> = args.iter().map(|&a| self.rt.show(a)).collect();
        let mut m = format!(
            "handle のどの節にも一致しません: {}.{}",
            eff.display, eff.ops[op as usize].0
        )
        .into_bytes();
        m.extend(keleut_rt::effect::args_text(&shown));
        Ok(Stop::Runtime(m))
    }

    /// 配列のオブジェクト `a` の要素を、再利用する引数のバッファ([`Interp::fx_args`])に入れて返す。
    /// `extra` は、呼び出し元が後で追加するワード数である。
    fn array_words_buf(&mut self, a: Word, extra: usize) -> Vec<Word> {
        let mut v = std::mem::take(&mut self.fx_args);
        v.clear();
        let n = self.rt.array_len(a);
        v.reserve(n as usize + extra);
        v.extend((0..n).map(|i| self.rt.array_get(a, i)));
        v
    }

    /// 使い終えた引数の並びを、再利用するバッファとして戻す。
    ///
    /// 取り出している間に入れ子の呼び出しが別のバッファを戻していたら、容量の大きいほうを残す。
    pub(crate) fn give_args(&mut self, mut v: Vec<Word>) {
        if v.capacity() > self.fx_args.capacity() {
            v.clear();
            self.fx_args = v;
        }
    }

    /// 継続 `k` に対応する、継続の表の項目を返す。
    fn cont(&mut self, k: Word) -> Result<&mut Cont, Stop> {
        let i = self.rt.cont_index(k)?;
        Ok(&mut self.rt.conts[i])
    }

    /// エフェクトの命令を実行する。`pc` は命令の位置、`idx` は [`Code::fx`] の添字である。
    // ディスパッチループにインライン展開させない。インライン展開されるとループのコードが変わり、エフェクトを使わないプログラムまで遅くなることがある。
    #[inline(never)]
    fn fx(&mut self, st: &mut St, pc: usize, idx: u32) -> Result<Option<Word>, Stop> {
        // SAFETY: Code は self.codes の Box の中にあり、移動しない。
        let code: &Code = unsafe { &*st.code };
        let w = &code.words;
        let rp = w[pc + 1];
        let dst = w[pc + 2];
        let n = w[pc + 3] as usize;
        let arg = |i: usize| st.w(w[pc + 4 + i]);
        let next = pc + 4 + n;
        let op = &code.fx[idx as usize];
        if op.is_call() {
            st.pc = next;
            return match self.fx_call(st, pc, idx)? {
                FxCall::Done => Ok(None),
                FxCall::Enter(func, env, args) => {
                    st.set_resume(rp);
                    let r = self.enter(st, func, env, Args::Words(args.as_ptr(), args.len()));
                    self.give_args(args);
                    r
                }
                FxCall::Value(f, args) => {
                    let r =
                        self.call_value(st, rp, f, Args::Words(args.as_ptr(), args.len()), false);
                    self.give_args(args);
                    r
                }
                FxCall::Resume(k, v) => self.cont_resume(st, rp, k, v),
            };
        }
        let mut out: Option<u64> = None;
        match op {
            FxOp::HandlerNew {
                effect,
                ops,
                has_ret,
                has_cancel,
            } => {
                let clauses: Vec<(u32, ClauseClass, Word)> = ops
                    .iter()
                    .enumerate()
                    .map(|(i, &(o, c))| (o, c, arg(2 + i)))
                    .collect();
                let mut at = 2 + ops.len();
                let ret = has_ret.then(|| {
                    at += 1;
                    arg(at - 1)
                });
                let cancel = has_cancel.then(|| arg(at));
                let rec = HandlerRec {
                    id: 0,
                    effect: *effect,
                    body: arg(0),
                    res: arg(1),
                    ops: clauses,
                    ret,
                    cancel,
                    parent: Word::UNIT,
                    h: 0,
                };
                out = Some(self.rt.handler_new(rec)?.0);
            }
            FxOp::HandlerFind { effect, op } => {
                let mut c = self.rt.chain;
                loop {
                    if c == Word::UNIT {
                        let e = &self.prog.effects[*effect as usize];
                        return Err(Stop::Unhandled(format!(
                            "{}.{}",
                            e.display, e.ops[*op as usize].0
                        )));
                    }
                    let r = self.rt.hr(c)?;
                    if r.effect == *effect {
                        break;
                    }
                    c = r.parent;
                }
                out = Some(c.0);
            }
            FxOp::HandlersGet => out = Some(self.rt.chain.0),
            FxOp::ContCheck { nontail } => {
                let k = arg(0);
                let depth = st.depth();
                let limit = self.rt.limit;
                let c = self.cont(k)?;
                if *nontail && !c.alive {
                    return Err(Stop::runtime(
                        "resume を節の外で呼び出しました(second-class)",
                    ));
                }
                if c.used {
                    return Err(Stop::runtime("resume は高々1回しか呼べません(アフィン)"));
                }
                if *nontail && depth + c.delta > limit {
                    return Err(self.rt.depth_error());
                }
            }
            FxOp::IsCont => out = Some(u64::from(self.rt.is_kind(arg(0), Kind::Cont))),
            FxOp::ContHr => {
                let k = arg(0);
                out = Some(self.cont(k)?.hr.0);
            }
            FxOp::HandlerInstall => {
                let hr = arg(0);
                let chain = self.rt.chain;
                let h = st.depth();
                let r = self.rt.hr_mut(hr)?;
                r.parent = chain;
                r.h = h;
                self.rt.chain = hr;
            }
            FxOp::HandlerUninstall => self.rt.chain = self.rt.hr(arg(0))?.parent,
            FxOp::IsPending => out = Some(u64::from(arg(0) == Word::SENTINEL)),
            FxOp::PendingClass => {
                use keleut_lir::effect::class::*;
                let hr = arg(0);
                let own = |t: &Word| *t == hr;
                let c = match &self.rt.pending {
                    Pending::DiscardValue { hr: t, .. } if own(t) => OWN_VALUE,
                    Pending::DiscardNever { hr: t, .. } if own(t) => OWN_NEVER,
                    Pending::Yield { hr: t, .. } if own(t) => OWN_YIELD,
                    Pending::Yield { .. } => OTHER_YIELD,
                    Pending::TailResume(_) => TAIL_RESUME,
                    Pending::Error(_)
                    | Pending::DiscardValue { .. }
                    | Pending::DiscardNever { .. } => OTHER_UNWIND,
                    Pending::None | Pending::Matched(_) | Pending::ClauseValue(_) => {
                        return Err(bug("ハンドラのフレームに届いた状態語が不正です"));
                    }
                };
                out = Some(c as i64 as u64);
            }
            FxOp::PendingTake => {
                let p = std::mem::take(&mut self.rt.pending);
                out = Some(self.rt.pending_box(p)?.0);
            }
            FxOp::PendingRestore => self.rt.pending = self.rt.pending_unbox(arg(0))?,
            FxOp::PendingValue => match self.rt.pending_unbox(arg(0))? {
                Pending::DiscardValue { v, .. } | Pending::TailResume(v) => out = Some(v.0),
                _ => return Err(bug("値を持たない状態語")),
            },
            FxOp::PendingDiscard => {
                let p = Pending::DiscardValue {
                    hr: arg(0),
                    v: arg(1),
                };
                out = Some(self.rt.pending_box(p)?.0);
            }
            FxOp::PendingSuppress => {
                let p = std::mem::take(&mut self.rt.pending);
                let mut m = "cancel 節で例外が抑制されました: ".as_bytes().to_vec();
                m.extend(keleut_rt::effect::suppressed_text(&p));
                m.push(b'\n');
                // 抑制の記録を書く前の flush が失敗したら、記録を書かずに、すぐに終了コード 74 で終了する(Diktor と同じ)。
                // flush の失敗には、前の失敗の後でバッファに残したデータや、その後に追記したデータを書き出せなかった場合も含む。
                if let Err(e) = self.rt.out.eprint_after_flush(&m) {
                    return Err(Stop::IoAbort(e.0));
                }
            }
            FxOp::ContMake => {
                let hr = arg(0);
                out = Some(self.cont_make(st, hr)?.0);
            }
            FxOp::ContKill => {
                let k = arg(0);
                self.cont(k)?.alive = false;
            }
            FxOp::ContUsed => {
                let k = arg(0);
                out = Some(u64::from(self.cont(k)?.used));
            }
            FxOp::HandlerHas(part) => {
                let r = self.rt.hr(arg(0))?;
                let x = match part {
                    HandlerPart::Ret => r.ret,
                    HandlerPart::Cancel => r.cancel,
                };
                out = Some(u64::from(x.is_some()));
            }
            FxOp::HandlerGet(part) => {
                let r = self.rt.hr(arg(0))?;
                let x = match part {
                    HandlerPart::Ret => r.ret,
                    HandlerPart::Cancel => r.cancel,
                };
                out = Some(x.ok_or_else(|| bug("無い節を読みました"))?.0);
            }
            FxOp::LimitGet => out = Some(self.rt.limit),
            FxOp::LimitSet => self.rt.limit = st.get(w[pc + 4]),
            FxOp::Perform { .. }
            | FxOp::CallBody
            | FxOp::ContResume
            | FxOp::CallClause
            | FxOp::CallCancel => unreachable!("呼び出しの一種は上で扱った"),
        }
        if let Some(v) = out {
            st.set(dst, v);
        }
        st.pc = next;
        Ok(None)
    }

    /// 呼び出しを行うエフェクトの命令(`FxOp::is_call`)について、何を呼ぶかを決める。
    ///
    /// 呼ぶ前の副作用(`perform` の連鎖と深さの更新、`call_clause` の深さの更新)は、ここで済ませる。
    /// 呼び出しそのものは、この関数の呼び出し元が行う(インタプリタでは [`Interp::fx`]、JIT のフレームでは [`Interp::native_fx`])。
    fn fx_call(&mut self, st: &mut St, pc: usize, idx: u32) -> Result<FxCall, Stop> {
        // 引数の並びは、確保の失敗を呼び出し元が処理する範囲(`keleut_rt::alloc::handled`)の外で確保し、確保箇所を `CallArgs` に設定する。
        // この間に起こる GC は、確保箇所を GC のものに上書きする。
        let _site = alloc::site(Site::CallArgs);
        // SAFETY: Code は self.codes の Box の中にあり、移動しない。
        let code: &Code = unsafe { &*st.code };
        let w = &code.words;
        let dst = w[pc + 2];
        let n = w[pc + 3] as usize;
        let arg = |i: usize| st.w(w[pc + 4 + i]);
        match &code.fx[idx as usize] {
            FxOp::Perform { op, .. } => {
                let hr = arg(0);
                let mut args = std::mem::take(&mut self.fx_args);
                args.clear();
                args.reserve(n);
                args.extend((2..n).map(arg));
                let p = self.rt.depth;
                if p + 1 > self.rt.limit {
                    return Err(self.rt.depth_error());
                }
                let Some((j, clause)) = self.next_clause(hr, *op, 0)? else {
                    return Err(self.no_clause(hr, *op, &args)?);
                };
                self.rt.chain = self.rt.hr(hr)?.parent;
                self.rt.depth = p + 1;
                args.push(hr);
                args.push(Word::i32(j as i32));
                Ok(FxCall::Enter(self.rt.closure_func(clause), clause, args))
            }
            FxOp::CallBody => {
                let r = self.rt.hr(arg(0))?;
                let (body, res) = (r.body, r.res);
                let mut args = std::mem::take(&mut self.fx_args);
                args.clear();
                args.push(res);
                Ok(FxCall::Value(body, args))
            }
            FxOp::ContResume => Ok(FxCall::Resume(arg(0), arg(1))),
            FxOp::CallClause => {
                let k = arg(0);
                let d = st.depth();
                if d + 1 > self.rt.limit {
                    let s = self.rt.depth_error();
                    self.rt.pending = Pending::Error(s);
                    st.set(dst, Word::SENTINEL.0);
                    return Ok(FxCall::Done);
                }
                let c = self.cont(k)?;
                let (hr, j, args) = (c.hr, c.clause as usize, c.args);
                let clause = self.rt.hr(hr)?.ops[j].2;
                let mut a = self.array_words_buf(args, 2);
                a.push(k);
                a.push(Word::i32(-1));
                self.rt.depth = d + 1;
                Ok(FxCall::Enter(self.rt.closure_func(clause), clause, a))
            }
            FxOp::CallCancel => {
                let mut args = std::mem::take(&mut self.fx_args);
                args.clear();
                Ok(FxCall::Value(arg(0), args))
            }
            _ => Err(bug("呼び出しでないエフェクトの命令")),
        }
    }

    /// エフェクトの終端命令を実行する。
    // [`Interp::fx`] と同じ理由で、ディスパッチループにインライン展開させない。
    #[inline(never)]
    fn fx_term(&mut self, st: &mut St, pc: usize, idx: u32) -> Result<Option<Word>, Stop> {
        let (func, env, a) = self.fx_term_call(st, pc, idx)?;
        let r = self.tail_enter(st, func, env, Args::Words(a.as_ptr(), a.len()));
        self.give_args(a);
        r
    }

    /// エフェクトの終端命令を、末尾呼び出しで呼ぶ関数が決まるところまで進める。
    ///
    /// 番兵を返して戻る終端命令では、`Err(Stop::Pending)` を返す。
    /// `clause_value`、`clause_matched`、`tail_resume` は、その前に状態ワードに目印を設定する。`propagate` は状態ワードを変えない。
    /// 末尾呼び出しの終端命令(`tail_call_never`、`tail_call_clause`)では、呼ぶ関数、環境、引数を返す。
    fn fx_term_call(
        &mut self,
        st: &St,
        pc: usize,
        idx: u32,
    ) -> Result<(FuncId, Word, Vec<Word>), Stop> {
        let _site = alloc::site(Site::CallArgs);
        // SAFETY: Code は self.codes の Box の中にあり、移動しない。
        let code: &Code = unsafe { &*st.code };
        let w = &code.words;
        let n = w[pc + 1] as usize;
        let arg = |i: usize| st.w(w[pc + 2 + i]);
        match &code.fx_terms[idx as usize] {
            FxTermOp::Propagate => Err(Stop::Pending),
            FxTermOp::ClauseValue => {
                self.rt.pending = Pending::ClauseValue(arg(0));
                Err(Stop::Pending)
            }
            FxTermOp::ClauseMatched => {
                self.rt.pending = Pending::Matched(arg(0).as_i32() as u32);
                Err(Stop::Pending)
            }
            FxTermOp::TailResume => {
                self.rt.pending = Pending::TailResume(arg(0));
                Err(Stop::Pending)
            }
            FxTermOp::TailCallNever => {
                let Pending::DiscardNever { hr, clause, args } = self.rt.pending_unbox(arg(0))?
                else {
                    return Err(bug("NeverBody でない状態語"));
                };
                let c = self.rt.hr(hr)?.ops[clause as usize].2;
                let mut a = self.array_words_buf(args, 2);
                a.push(Word::UNIT);
                a.push(Word::i32(-1));
                Ok((self.rt.closure_func(c), c, a))
            }
            FxTermOp::TailCallClause { op } => {
                let (hr, sel) = (arg(0), arg(1));
                let mut a = std::mem::take(&mut self.fx_args);
                a.clear();
                a.reserve(n);
                a.extend((2..n).map(arg));
                let start = (sel.as_i32() + 1).max(0) as usize;
                match self.next_clause(hr, *op, start)? {
                    Some((j, c)) => {
                        a.push(hr);
                        a.push(Word::i32(j as i32));
                        Ok((self.rt.closure_func(c), c, a))
                    }
                    None => Err(self.no_clause(hr, *op, &a)?),
                }
            }
        }
    }

    /// `cont_make hr`。自分宛ての `YIELD` から継続を作る。
    ///
    /// コピー領域は、状態ワードを消す前に確保する。確保には、失敗を `Err` で返す `try_reserve_exact` を使う。
    /// 確保できなければ、[`Interp::fail_into_segment`] で、まだ値スタックに残っている区間(継続にするフレームの範囲)の最上段から、メモリ不足のエラーを伝播して巻き戻す。
    /// 継続を表に入れるときに、添字を持つオブジェクトを確保できなかったときも同じである。
    fn cont_make(&mut self, st: &mut St, hr: Word) -> Result<Word, Stop> {
        let (t, top) = match &self.rt.pending {
            Pending::Yield { hr: t, top, .. } => (*t, *top),
            _ => return Err(bug("YIELD でない状態語から継続を作ろうとしました")),
        };
        if t != hr {
            return Err(bug("ほかの宛ての YIELD から継続を作ろうとしました"));
        }
        let h = st.depth();
        // 1 回目の走査: フレームの個数と、ワード数の合計を数える。
        let (mut count, mut total) = (0usize, 0usize);
        let mut pos = top;
        while pos != st.fpos {
            if pos == NATIVE_CALLER {
                return Err(bug("継続のフレームがハンドラのフレームに届きません"));
            }
            let p = self.rt.stack.ptr_at(pos);
            // SAFETY: p はフレームヘッダを指し、フレームのワード数はヘッダの 2 番のワードから求まる。
            let (words, caller) = unsafe { (frame_words(*p.add(2)), *p.add(3)) };
            count += 1;
            total += words;
            pos = caller;
        }
        let (mut frames, mut sizes) = self.take_cont_buf(total, count);
        frames.clear();
        sizes.clear();
        let got = {
            let _site = alloc::site(Site::Cont);
            alloc::handled(|| {
                frames.try_reserve_exact(total).is_ok() && sizes.try_reserve_exact(count).is_ok()
            })
        };
        if !got {
            return self.fail_into_segment(st, top);
        }
        frames.resize(total, 0);
        sizes.resize(count, 0);
        // 2 回目の走査: 最上段から順にたどり、バッファの後ろから詰める。最下段のフレームが先頭に来る。
        let (mut off, mut i) = (total, count);
        let mut delta = 0;
        let mut pos = top;
        while pos != st.fpos {
            let p = self.rt.stack.ptr_at(pos);
            // SAFETY: 1 回目の走査と同じフレームである。
            let words = unsafe { frame_words(*p.add(2)) };
            off -= words;
            i -= 1;
            let f = &mut frames[off..off + words];
            // SAFETY: p から始まる words ワードは、フレームの中にある。
            f.copy_from_slice(unsafe { std::slice::from_raw_parts(p, words) });
            if i == count - 1 {
                delta = f[1].wrapping_sub(h);
            }
            f[1] = f[1].wrapping_sub(h);
            sizes[i] = words as u32;
            pos = f[3];
        }
        // 状態ワードは、継続を表に入れた後で消す。
        // 表に入れるときの確保が GC を起こしても、区間のフレームは、状態ワードの `YIELD` からたどる GC ルートとして値スタックに残る。
        // GC は、生きているフレームより後ろのチャンクを解放する。
        // 先に状態ワードを消すと、継続を作れなかったときに [`Interp::fail_into_segment`] が制御を移す区間の最上段が、解放したチャンクにあることになる。
        let Pending::Yield {
            clause, args, cut, ..
        } = self.rt.pending.clone()
        else {
            unreachable!("上で確かめた");
        };
        let c = Cont {
            hr,
            frames,
            sizes,
            delta,
            cut,
            clause,
            args,
            used: false,
            alive: true,
        };
        match self.rt.cont_new(c) {
            Ok(k) => {
                self.rt.pending = Pending::None;
                Ok(k)
            }
            Err(Stop::OutOfMemory) => self.fail_into_segment(st, top),
            Err(e) => Err(e),
        }
    }

    /// 再開した継続から外したコピー領域のうち、`total` ワードと `count` 個のフレームに容量の近い組を取り出す。
    ///
    /// 容量が必要な量の 2 倍を超える領域は選ばない。
    /// 小さな継続では、2 倍の代わりに [`CONT_FIT_WORDS`] ワードと [`CONT_FIT_FRAMES`] 個を上限にする。
    /// 小さな継続が大きな領域を持ったまま生き続けると、生きている継続ごとに余分なメモリが残るからである。
    /// 合う組がなければ、空の組を返す。
    /// ディスパッチループの機械語が変わらないように、インライン展開させない。
    #[inline(never)]
    fn take_cont_buf(&mut self, total: usize, count: usize) -> (Vec<u64>, Vec<u32>) {
        let fits = |(f, s): &(Vec<u64>, Vec<u32>)| {
            (total..=(2 * total).max(CONT_FIT_WORDS)).contains(&f.capacity())
                && (count..=(2 * count).max(CONT_FIT_FRAMES)).contains(&s.capacity())
        };
        match self.cont_bufs.iter().position(fits) {
            Some(i) => self.cont_bufs.swap_remove(i),
            None => Default::default(),
        }
    }

    /// 再開した継続から外したコピー領域を、次の `cont_make` のために残す。
    ///
    /// 残すのは [`CONT_BUFS`] 組までで、[`CONT_BUF_WORDS`] ワードを超える領域は残さずに解放する。
    /// 組の数が上限に達していれば、最も古い組を解放して入れ替える。
    /// そのため、容量の合わない組が残り続けて、再利用できる組を入れられなくなることはない。
    #[inline(never)]
    fn keep_cont_bufs(&mut self, frames: Vec<u64>, sizes: Vec<u32>) {
        if frames.capacity() > CONT_BUF_WORDS {
            return;
        }
        if self.cont_bufs.len() >= CONT_BUFS {
            self.cont_bufs.remove(0);
        }
        self.cont_bufs.push((frames, sizes));
    }

    /// 継続を作れなかったときに呼ぶ。区間の最上段のフレーム `top`(まだ値スタックにある)に制御を移し、そこからメモリ不足のエラーを伝播して巻き戻す。
    ///
    /// 区間の中にあるハンドラの `cancel` 節は、`depth_limit` のエラーを継続に注入するとき([`Interp::cont_inject`])と同じく、内側から順に実行される。
    /// 巻き戻しがハンドラのフレームに達すると、そのフレームは、本体を呼び出した再開点で、このエラーをほかのハンドラ宛ての `ERROR` として受け取る。
    fn fail_into_segment(&mut self, st: &mut St, top: u64) -> Result<Word, Stop> {
        self.move_to(st, top)?;
        self.rt.pending = Pending::Error(Stop::OutOfMemory);
        Err(Stop::Pending)
    }

    /// `cont_resume k v`。現在のフレームの上に継続のフレームを復元し、最上段の `perform` の結果を `v` にして実行を続ける。
    ///
    /// `v` が退避した状態ワードなら、フレームを配置し直さずに、[`Interp::cont_inject`] で注入して巻き戻す。
    fn cont_resume(
        &mut self,
        st: &mut St,
        rp: u32,
        k: Word,
        v: Word,
    ) -> Result<Option<Word>, Stop> {
        let ki = self.rt.cont_index(k)?;
        if self.rt.is_kind(v, Kind::Pending) {
            return self.cont_inject(st, rp, ki, v);
        }
        let base = st.depth();
        let mut below = st.fpos;
        let mut top = st.fpos + st.code().frame_words as u64;
        // フレームをすべて配置できるまで、継続を変えない。配置できなければ、継続をそのまま残してメモリ不足を返す。
        // 継続はまだ使われていないので、その後は注入の経路(継続の中の cancel 節を実行する)で処理される。
        let mut placed = std::mem::take(&mut self.placed);
        placed.clear();
        alloc::handled(|| placed.try_reserve_exact(self.rt.conts[ki].sizes.len()))
            .map_err(|_| Stop::OutOfMemory)?;
        {
            let _site = alloc::site(Site::Resume);
            let rt = &mut self.rt;
            let c = &rt.conts[ki];
            let mut off = 0usize;
            for &sz in &c.sizes {
                let sz = sz as usize;
                let pos = rt.stack.place(top, sz)?;
                let p = rt.stack.ptr_at(pos);
                // SAFETY: p から始まる sz ワードは、チャンクの中にある。
                unsafe {
                    std::ptr::copy_nonoverlapping(c.frames[off..].as_ptr(), p, sz);
                    *p.add(1) = base.wrapping_add(*p.add(1));
                    *p.add(3) = below;
                }
                placed.push(pos);
                off += sz;
                below = pos;
                top = pos + sz as u64;
            }
        }
        let c = &mut self.rt.conts[ki];
        c.used = true;
        let frames = std::mem::take(&mut c.frames);
        let sizes = std::mem::take(&mut c.sizes);
        let (hr, cut) = (c.hr, c.cut);
        self.keep_cont_bufs(frames, sizes);
        // 復元したフレームが $rt_handle のフレームなら、そのハンドラ記録の深さを新しい深さに書き直す。
        if let Some(rh) = self.rt_handle {
            for &pos in &placed {
                let p = self.rt.stack.ptr_at(pos);
                // SAFETY: p はフレームヘッダを指す。$rt_handle のフレームでは、any の領域の 0 番のスロットがハンドラ記録である。
                let (func, depth, h) =
                    unsafe { (*p as u32, *p.add(1), Word(*p.add(HEADER_WORDS))) };
                if func == rh.0 {
                    self.rt.hr_mut(h)?.h = depth;
                }
            }
        }
        let chain = self.rt.chain;
        let r = self.rt.hr_mut(hr)?;
        r.parent = chain;
        r.h = base;
        self.rt.chain = cut;
        st.set_resume(rp);
        let empty = placed.is_empty();
        self.placed = placed;
        if empty {
            return Err(bug("フレームの無い継続"));
        }
        self.move_to(st, below)?;
        // SAFETY: fp は最上段のフレーム(分類 G の操作節がキャプチャした perform のフレーム)のヘッダを指す。
        let trp = (unsafe { *st.fp } >> 32) as u32;
        if trp & YIELDED == 0 {
            return Err(bug("継続の最上段が取り出された perform でない"));
        }
        let e = st.code().resume[(trp & !YIELDED) as usize];
        st.set(e.dst, v.0);
        st.pc = e.pc_after as usize;
        Ok(None)
    }

    /// 継続 `ki` に、退避した状態ワード `v` を注入する(`cont_resume` の `v` が状態ワードのとき)。
    ///
    /// 注入は、継続が持つフレームのコピーをチャンクのまま値スタックにつなぎ、最上段の `perform` のフレームから、退避した状態ワード(`DISCARD` か `ERROR`)を伝播する操作である。
    /// 使われていない継続にメモリ不足を注入するときに、継続のフレームを [`keleut_rt::stack::ValueStack::place`] で配置し直すと、チャンクの確保が必要になる。
    /// その確保にも失敗すると、継続の中の `cancel` 節を実行できない。
    /// そこで、継続のコピー領域をそのまま 1 つのチャンクとして値スタックにつなぎ([`keleut_rt::stack::ValueStack::link_copy`])、チャンクを確保せずに配置する。
    /// コピーしたフレームのヘッダの 1 番のワード(深さ)と 3 番のワード(呼び出し元の位置)は、通常の再開と同じように書き直す。
    /// チャンクの配列を拡張できなければ、継続を変えずにメモリ不足を返す。
    fn cont_inject(
        &mut self,
        st: &mut St,
        rp: u32,
        ki: usize,
        v: Word,
    ) -> Result<Option<Word>, Stop> {
        let base = st.depth();
        let top = st.fpos + st.code().frame_words as u64;
        if self.rt.conts[ki].sizes.is_empty() {
            return Err(bug("フレームの無い継続"));
        }
        let frames = std::mem::take(&mut self.rt.conts[ki].frames);
        let at = match self.rt.stack.link_copy(top, frames) {
            Ok(at) => at,
            Err((_, frames)) => {
                self.rt.conts[ki].frames = frames;
                return Err(Stop::OutOfMemory);
            }
        };
        let sizes = std::mem::take(&mut self.rt.conts[ki].sizes);
        let rh = self.rt_handle.map(|f| f.0);
        let (mut pos, mut below) = (at, st.fpos);
        for &sz in &sizes {
            let p = self.rt.stack.ptr_at(pos);
            // SAFETY: p は、コピーチャンクの中にあるフレームのヘッダを指す。
            let (func, depth) = unsafe {
                *p.add(1) = base.wrapping_add(*p.add(1));
                *p.add(3) = below;
                (*p as u32, *p.add(1))
            };
            // 復元したフレームが $rt_handle のフレームなら、そのハンドラ記録の深さを新しい深さに書き直す。
            if Some(func) == rh {
                // SAFETY: $rt_handle のフレームでは、any の領域の 0 番のスロットがハンドラ記録である。
                let h = Word(unsafe { *p.add(HEADER_WORDS) });
                self.rt.hr_mut(h)?.h = depth;
            }
            below = pos;
            pos += u64::from(sz);
        }
        let c = &mut self.rt.conts[ki];
        c.used = true;
        let (hr, cut) = (c.hr, c.cut);
        let chain = self.rt.chain;
        let r = self.rt.hr_mut(hr)?;
        r.parent = chain;
        r.h = base;
        self.rt.chain = cut;
        st.set_resume(rp);
        self.move_to(st, below)?;
        self.rt.pending = self.rt.pending_unbox(v)?;
        Err(Stop::Pending)
    }

    // ---- JIT のフレームのエフェクト(`crate::native` のランタイム関数が呼ぶ) ----

    /// JIT のフレーム(位置 `fpos`、ヘッダのアドレス `fp`)を、インタプリタの現在のフレームとして扱うための `St` を作る。
    /// `pc` は命令の位置である。
    fn native_st(&mut self, fpos: u64, fp: *mut u64, pc: usize) -> Result<St, Stop> {
        // SAFETY: fp は JIT のフレームのヘッダを指す。
        let fid = FuncId(unsafe { *fp } as u32);
        let code = self.code(fid)?;
        Ok(St {
            fpos,
            fp,
            code,
            pc,
            env: Word(0),
        })
    }

    /// JIT のフレームの、`idx` 番目のエフェクトの命令を 1 つ実行する。
    ///
    /// JIT のコードは、命令のオペランドをすべてスロットに書いてから、この関数を呼ぶ。
    /// 呼び出しを行う命令なら、再開点で生存しているレジスタもスロットに書き、ヘッダに再開点を書いておく。
    /// 命令の意味はインタプリタと同じで、結果は書き込み先のスロットに書く([`NativeFx::Next`])。
    /// 呼び出しを行う命令では、呼ぶ関数を返す([`NativeFx::Enter`])。
    /// 継続の再開では、値スタックに復元した区間を、新しいループの呼び出し(`run_from`)でインタプリタが実行し、区間の最下段のフレームの結果を返す([`NativeFx::Value`])。
    /// 区間の最上段は、分類 G の操作節がキャプチャした `perform` のフレームであり、JIT のコードはフレームを再開点から続けられないからである。
    /// `cont_make` が継続を作れなかったときは、区間の最上段から巻き戻し、区間の最下段のフレームの結果を、このフレームのヘッダの再開点(本体の呼び出し)の結果として返す([`NativeFx::Redeliver`])。
    pub(crate) fn native_fx(
        &mut self,
        fpos: u64,
        fp: *mut u64,
        idx: u32,
    ) -> Result<NativeFx, Stop> {
        let mut st = self.native_st(fpos, fp, 0)?;
        let pc = st.code().fx_pc[idx as usize] as usize;
        st.pc = pc;
        // SAFETY: Code は self.codes の Box の中にあり、移動しない。
        let code: &Code = unsafe { &*st.code };
        if !code.fx[idx as usize].is_call() {
            return match self.fx(&mut st, pc, idx) {
                Ok(_) => Ok(NativeFx::Next),
                Err(e) if st.fpos != fpos => {
                    let bottom = self.segment_bottom(st.fpos, fpos)?;
                    Ok(NativeFx::Redeliver(self.run_from(
                        &mut st,
                        bottom,
                        Some(e),
                    )?))
                }
                Err(e) => Err(e),
            };
        }
        let rp = st.code().words[pc + 1];
        match self.fx_call(&mut st, pc, idx)? {
            FxCall::Done => Ok(NativeFx::Next),
            FxCall::Enter(func, env, args) => Ok(NativeFx::Enter(func, env, args)),
            FxCall::Value(f, args) => match self.rt.callee_view(f) {
                CalleeView::Closure(func) => Ok(NativeFx::Enter(func, f, args)),
                _ => Err(Stop::Internal(format!(
                    "エフェクトの呼び出しの相手がクロージャでありません: {}",
                    String::from_utf8_lossy(&self.rt.show(f))
                ))),
            },
            FxCall::Resume(k, v) => {
                let inject = match self.cont_resume(&mut st, rp, k, v) {
                    Ok(_) => None,
                    Err(e) if st.fpos != fpos => Some(e),
                    Err(e) => return Err(e),
                };
                let bottom = self.segment_bottom(st.fpos, fpos)?;
                Ok(NativeFx::Value(self.run_from(&mut st, bottom, inject)?))
            }
        }
    }

    /// 値スタックのフレーム `top` から呼び出し元を順にたどり、呼び出し元が `below` であるフレームの位置を返す。
    fn segment_bottom(&mut self, top: u64, below: u64) -> Result<u64, Stop> {
        let mut pos = top;
        loop {
            let p = self.rt.stack.ptr_at_ro(pos);
            // SAFETY: p はフレームヘッダを指す。
            let caller = unsafe { *p.add(3) };
            if caller == below {
                return Ok(pos);
            }
            if caller == NATIVE_CALLER {
                return Err(bug("区間の最下段が JIT のフレームの上にありません"));
            }
            pos = caller;
        }
    }

    /// `st` を現在のフレーム、`bottom` を最下段として、新しいループの呼び出しを始める。
    ///
    /// `inject` があれば、それを状態ワードに設定して、現在のフレームから巻き戻す。
    /// 最下段のフレームが返した値か番兵を返す。
    /// 最下段のフレームが JIT の関数を末尾呼び出しで呼んだら、その関数をネイティブの呼び出しで実行して、その値を返す。
    fn run_from(&mut self, st: &mut St, bottom: u64, inject: Option<Stop>) -> Result<Word, Stop> {
        self.check_native_stack()?;
        let saved = self.bottom;
        self.bottom = bottom;
        let r = (|| {
            let first = match inject {
                Some(e) => {
                    self.rt.raise(e)?;
                    self.unwind(st)?
                }
                None => None,
            };
            match first {
                Some(v) => Ok(v),
                None => self.exec(st),
            }
        })();
        self.bottom = saved;
        let v = r?;
        if v == crate::native::TAILREQ {
            return self.call_native_request();
        }
        Ok(v)
    }

    /// JIT のフレームの `perform` の呼び出しが `r` を返して戻ったときに呼ぶ(`doc/lir.md` の 16.5 節の `perform` の手順 4)。
    ///
    /// `r` が値なら、連鎖と深さを `perform` の時点のものに戻して `true` を返す。
    /// このとき、JIT のコードは `r` を `perform` の結果にする。
    /// `r` が番兵なら、深さを書き戻し、節の選択の結果を状態ワードに設定して `false` を返す。
    /// このとき、JIT のコードは番兵を返して戻る。
    pub(crate) fn native_perform_back(
        &mut self,
        fpos: u64,
        fp: *mut u64,
        r: Word,
    ) -> Result<bool, Stop> {
        let mut st = self.native_st(fpos, fp, 0)?;
        if r == Word::SENTINEL {
            self.rt.depth = st.depth();
            if self.receive(&mut st)? {
                return Err(bug("perform の再開点が番兵を受け取りました"));
            }
            return Ok(false);
        }
        self.deliver(&mut st, r)?;
        Ok(true)
    }

    /// JIT のフレームの、`idx` 番目のエフェクトの終端命令を実行する。
    ///
    /// 番兵を返して戻る終端命令なら、`None` を返す。
    /// このとき、状態ワードは設定済みである。
    /// 末尾呼び出しの終端命令なら、呼ぶ関数、環境、引数を返す。
    pub(crate) fn native_fx_term(
        &mut self,
        fpos: u64,
        fp: *mut u64,
        idx: u32,
    ) -> Result<Option<(FuncId, Word, Vec<Word>)>, Stop> {
        let st = self.native_st(fpos, fp, 0)?;
        let pc = st.code().fx_term_pc[idx as usize] as usize;
        match self.fx_term_call(&st, pc, idx) {
            Ok(t) => Ok(Some(t)),
            Err(Stop::Pending) => Ok(None),
            Err(e) => Err(e),
        }
    }

    // ---- ディスパッチループ ----

    /// `idx` 番目の `error` の命令の実行時エラーを作る。`operand` はオペランドのスロット番号で、`u32::MAX` ならオペランドはない。
    fn error(&self, st: &St, idx: u32, operand: u32) -> Stop {
        let kind = &st.code().errors[idx as usize];
        let opnd = if operand == u32::MAX {
            0
        } else {
            st.get(operand)
        };
        runtime_error(self.prog, &self.rt, kind, opnd)
    }

    /// ディスパッチループを実行する。
    ///
    /// 命令がエラーを返したら、そのエラーを状態ワードに設定して巻き戻し、番兵を受け取るフレームから実行を続ける。
    /// 巻き戻さずに終了するエラーは、そのまま返す。
    fn exec(&mut self, st: &mut St) -> Result<Word, Stop> {
        loop {
            match self.exec_inner(st) {
                Ok(v) => return Ok(v),
                Err(s) => {
                    self.rt.raise(s)?;
                    if let Some(v) = self.unwind(st)? {
                        return Ok(v);
                    }
                }
            }
        }
    }

    fn exec_inner(&mut self, st: &mut St) -> Result<Word, Stop> {
        use op::*;
        loop {
            let code = st.code;
            // SAFETY: Code は self.codes の Box の中にあり、移動しない。
            let w: &[u32] = unsafe { &(*code).words };
            let pc = st.pc;
            let w0 = w[pc];
            let opc = (w0 & 0xff) as u8;
            let sub = w0 >> 8;
            let a = |k: usize| w[pc + k];
            macro_rules! g {
                ($k:expr) => {
                    st.get(a($k))
                };
            }
            macro_rules! gw {
                ($k:expr) => {
                    Word(st.get(a($k)))
                };
            }
            macro_rules! put {
                ($k:expr, $v:expr) => {{
                    let v: u64 = $v;
                    st.set(a($k), v);
                }};
            }
            macro_rules! slots {
                ($at:expr) => {{
                    let n = a($at) as usize;
                    (Args::Slots(w[pc + $at + 1..].as_ptr(), n), n)
                }};
            }
            macro_rules! collect {
                ($at:expr) => {{
                    let n = a($at) as usize;
                    let mut v = std::mem::take(&mut self.words);
                    v.clear();
                    for i in 0..n {
                        v.push(Word(st.get(w[pc + $at + 1 + i])));
                    }
                    (v, n)
                }};
            }
            match opc {
                CONST => {
                    put!(1, u64::from(a(2)) | (u64::from(a(3)) << 32));
                    st.pc += 4;
                }
                MOVE => {
                    put!(1, g!(2));
                    st.pc += 3;
                }
                BOX_BOOL => {
                    put!(1, Word::bool(g!(2) != 0).0);
                    st.pc += 3;
                }
                BOX_I32 => {
                    put!(1, Word::i32(g!(2) as i32).0);
                    st.pc += 3;
                }
                BOX_I64 => {
                    let b = self.rt.box_i64(g!(2) as i64)?;
                    put!(1, b.0);
                    st.pc += 3;
                }
                BOX_F64 => {
                    let b = self.rt.box_f64(g!(2))?;
                    put!(1, b.0);
                    st.pc += 3;
                }
                UNBOX_BOOL => {
                    put!(1, u64::from(gw!(2).as_bool()));
                    st.pc += 3;
                }
                UNBOX_I32 => {
                    put!(1, gw!(2).as_i32() as i64 as u64);
                    st.pc += 3;
                }
                UNBOX_I64 | UNBOX_F64 => {
                    put!(1, self.rt.unbox_word(gw!(2)));
                    st.pc += 3;
                }
                ENV_GET => {
                    put!(1, self.rt.closure_cap(st.env, a(2)).0);
                    st.pc += 3;
                }
                DEPTH_ENTER => {
                    // SAFETY: fp は現在のフレームのヘッダを指す。
                    unsafe { *st.fp.add(1) = self.rt.depth };
                    st.pc += 1;
                }
                DEPTH_CHECK => {
                    if self.rt.depth + 1 > self.rt.limit {
                        return Err(self.rt.depth_error());
                    }
                    st.pc += 1;
                }
                DEPTH_PUSH => {
                    if self.rt.depth + 1 > self.rt.limit {
                        return Err(self.rt.depth_error());
                    }
                    self.rt.depth += 1;
                    st.pc += 1;
                }
                DEPTH_RESTORE => {
                    // SAFETY: fp は現在のフレームのヘッダを指す。
                    self.rt.depth = unsafe { *st.fp.add(1) };
                    st.pc += 1;
                }
                I32_ARITH => {
                    let (x, y) = (g!(2) as i32, g!(3) as i32);
                    let r = match sub {
                        0 => x.wrapping_add(y),
                        1 => x.wrapping_sub(y),
                        2 => x.wrapping_mul(y),
                        3 => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_div(y)
                            }
                        }
                        _ => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_rem(y)
                            }
                        }
                    };
                    put!(1, r as i64 as u64);
                    st.pc += 4;
                }
                I64_ARITH => {
                    let (x, y) = (g!(2) as i64, g!(3) as i64);
                    let r = match sub {
                        0 => x.wrapping_add(y),
                        1 => x.wrapping_sub(y),
                        2 => x.wrapping_mul(y),
                        3 => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_div(y)
                            }
                        }
                        _ => {
                            if y == 0 {
                                0
                            } else {
                                x.wrapping_rem(y)
                            }
                        }
                    };
                    put!(1, r as u64);
                    st.pc += 4;
                }
                I32_NEG => {
                    put!(1, (g!(2) as i32).wrapping_neg() as i64 as u64);
                    st.pc += 3;
                }
                I64_NEG => {
                    put!(1, (g!(2) as i64).wrapping_neg() as u64);
                    st.pc += 3;
                }
                I32_CMP => {
                    put!(1, u64::from(cmp(sub, (g!(2) as i32).cmp(&(g!(3) as i32)))));
                    st.pc += 4;
                }
                I64_CMP => {
                    put!(1, u64::from(cmp(sub, (g!(2) as i64).cmp(&(g!(3) as i64)))));
                    st.pc += 4;
                }
                F64_ARITH => {
                    let (x, y) = (f64::from_bits(g!(2)), f64::from_bits(g!(3)));
                    let r = match sub {
                        0 => x + y,
                        1 => x - y,
                        2 => x * y,
                        _ => x / y,
                    };
                    put!(1, r.to_bits());
                    st.pc += 4;
                }
                F64_NEG => {
                    put!(1, g!(2) ^ (1u64 << 63));
                    st.pc += 3;
                }
                F64_CMP => {
                    let (x, y) = (f64::from_bits(g!(2)), f64::from_bits(g!(3)));
                    let r = match sub {
                        0 => x == y,
                        1 => x < y,
                        2 => x <= y,
                        3 => x > y,
                        _ => x >= y,
                    };
                    put!(1, u64::from(r));
                    st.pc += 4;
                }
                F64_MATH => {
                    let x = f64::from_bits(g!(2));
                    let r = match sub {
                        0 => keleut_rtcore::sin(x),
                        1 => keleut_rtcore::cos(x),
                        2 => x.sqrt(),
                        3 => keleut_rtcore::exp(x),
                        _ => keleut_rtcore::log(x),
                    };
                    put!(1, r.to_bits());
                    st.pc += 3;
                }
                F64_TRUNC => {
                    put!(1, f64::from_bits(g!(2)).trunc().to_bits());
                    st.pc += 3;
                }
                F64_ISNAN => {
                    put!(1, u64::from(f64::from_bits(g!(2)).is_nan()));
                    st.pc += 3;
                }
                CONV => {
                    let x = g!(2);
                    let r = match sub {
                        0 => x as i32 as i64 as u64,
                        1 => (x as i32 as f64).to_bits(),
                        2 => x as i64 as i32 as i64 as u64,
                        3 => (x as i64 as f64).to_bits(),
                        4 => f64::from_bits(x) as i32 as i64 as u64,
                        _ => f64::from_bits(x) as i64 as u64,
                    };
                    put!(1, r);
                    st.pc += 3;
                }
                BOOL_NOT => {
                    put!(1, u64::from(g!(2) == 0));
                    st.pc += 3;
                }
                BOOL_EQ => {
                    put!(1, u64::from(g!(2) == g!(3)));
                    st.pc += 4;
                }
                STR_CONCAT => {
                    let r = self.rt.str_concat(gw!(2), gw!(3))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                STR_CMP => {
                    let o = keleut_rtcore::compare_bytes(
                        self.rt.str_bytes(gw!(2)),
                        self.rt.str_bytes(gw!(3)),
                    );
                    put!(1, u64::from(cmp(sub, o)));
                    st.pc += 4;
                }
                STR_LEN => {
                    put!(1, self.rt.str_bytes(gw!(2)).len() as i64 as u64);
                    st.pc += 3;
                }
                STR_SUB => {
                    let r = self.rt.str_sub(gw!(2), g!(3) as i32, g!(4) as i32)?;
                    put!(1, r.0);
                    st.pc += 5;
                }
                SHOW => {
                    let x = g!(2);
                    let s = match sub {
                        0 => (x as i32).to_string(),
                        1 => (x as i64).to_string(),
                        2 => keleut_rtcore::show_f64(f64::from_bits(x)),
                        _ => (if x != 0 { "true" } else { "false" }).to_string(),
                    };
                    let r = self.rt.new_str(s.as_bytes())?;
                    put!(1, r.0);
                    st.pc += 3;
                }
                CTOR_NEW => {
                    let (v, n) = collect!(4);
                    let r = self.rt.ctor_new(TypeId(a(2)), a(3), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 5 + n;
                }
                CTOR_TAG => {
                    put!(1, self.rt.ctor_tag(gw!(2)) as i64 as u64);
                    st.pc += 3;
                }
                CTOR_FIELD => {
                    put!(1, self.rt.ctor_field(gw!(2), a(3)).0);
                    st.pc += 4;
                }
                VARIANT_NEW => {
                    let r = self.rt.variant_new(LabelId(a(2)), gw!(3))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                VARIANT_TAG => {
                    put!(1, self.rt.variant_tag(gw!(2)) as i64 as u64);
                    st.pc += 3;
                }
                VARIANT_PAYLOAD => {
                    put!(1, self.rt.variant_payload(gw!(2)).0);
                    st.pc += 3;
                }
                REC_NEW => {
                    let (v, n) = collect!(3);
                    let r = self.rt.rec_new(ShapeId(a(2)), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 4 + n;
                }
                REC_GET => {
                    put!(1, self.rt.rec_get(gw!(2), a(3)).0);
                    st.pc += 4;
                }
                REC_EXTEND => {
                    let r = self.rt.rec_extend(gw!(2), LabelId(a(3)), gw!(4))?;
                    put!(1, r.0);
                    st.pc += 5;
                }
                REC_FIELD => {
                    put!(1, self.rt.rec_field(gw!(2), LabelId(a(3)), a(4)).0);
                    st.pc += 5;
                }
                REC_HAS => {
                    put!(1, u64::from(self.rt.rec_has(gw!(2), LabelId(a(3)), a(4))));
                    st.pc += 5;
                }
                REC_RESTRICT => {
                    let r = self.rt.rec_restrict(gw!(2), LabelId(a(3)))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                REC_UPDATE => {
                    let r = self.rt.rec_update(gw!(2), LabelId(a(3)), gw!(4))?;
                    put!(1, r.0);
                    st.pc += 5;
                }
                CLOSURE_NEW => {
                    let (v, n) = collect!(3);
                    let r = self.rt.closure_new(FuncId(a(2)), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 4 + n;
                }
                CLOSURE_SET_CAP => {
                    self.rt.closure_set_cap(gw!(1), a(2), gw!(3));
                    st.pc += 4;
                }
                METHOD_NEW => {
                    let (v, n) = collect!(5);
                    let r = self.rt.method_new(ClassId(a(2)), a(3), gw!(4), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 6 + n;
                }
                DICT_INST => {
                    let (v, n) = collect!(3);
                    let r = self.rt.dict_inst(InstanceId(a(2)), &v);
                    self.words = v;
                    put!(1, r?.0);
                    st.pc += 4 + n;
                }
                DICT_RECORD | DICT_VARIANT => {
                    let n = a(2) as usize;
                    let fs: Vec<(LabelId, Word)> = (0..n)
                        .map(|i| (LabelId(a(3 + 2 * i)), Word(st.get(a(4 + 2 * i)))))
                        .collect();
                    let kind = if opc == DICT_RECORD {
                        Kind::DictRecord
                    } else {
                        Kind::DictVariant
                    };
                    let r = self.rt.dict_struct(kind, &fs)?;
                    put!(1, r.0);
                    st.pc += 3 + 2 * n;
                }
                GLOBAL_GET => {
                    put!(1, self.rt.globals[a(2) as usize].0);
                    st.pc += 3;
                }
                GLOBAL_IS_INIT => {
                    put!(1, u64::from(self.rt.global_init[a(2) as usize]));
                    st.pc += 3;
                }
                GLOBAL_SET => {
                    self.rt.globals[a(1) as usize] = gw!(2);
                    st.pc += 3;
                }
                GLOBAL_MARK_INIT => {
                    self.rt.global_init[a(1) as usize] = true;
                    st.pc += 2;
                }
                INST_IS_INIT => {
                    put!(1, u64::from(self.rt.inst_init[a(2) as usize]));
                    st.pc += 3;
                }
                INST_MARK_INIT => {
                    self.rt.inst_init[a(1) as usize] = true;
                    st.pc += 2;
                }
                INST_SET_VALUE => {
                    self.rt.inst_values[a(1) as usize][a(2) as usize] = gw!(3);
                    st.pc += 4;
                }
                INST_GET_VALUE => {
                    put!(1, self.rt.inst_values[a(2) as usize][a(3) as usize].0);
                    st.pc += 4;
                }
                REF_NEW => {
                    let r = self.rt.ref_new(gw!(2))?;
                    put!(1, r.0);
                    st.pc += 3;
                }
                REF_GET => {
                    // SAFETY: 命令の前提により、値は Ref のオブジェクトである。
                    put!(1, unsafe { keleut_rt::value::get(gw!(2), 1) });
                    st.pc += 3;
                }
                REF_SET => {
                    // SAFETY: 命令の前提により、値は Ref のオブジェクトである。
                    unsafe { keleut_rt::value::set(gw!(1), 1, g!(2)) };
                    st.pc += 3;
                }
                ARRAY_LEN => {
                    put!(1, self.rt.array_len(gw!(2)) as i64 as u64);
                    st.pc += 3;
                }
                ARRAY_GET => {
                    put!(1, self.rt.array_get(gw!(2), g!(3) as i32).0);
                    st.pc += 4;
                }
                MARRAY_NEW => {
                    let n = (g!(2) as i32).max(0) as usize;
                    let r = self.rt.array_new(Kind::MArray, n, gw!(3))?;
                    put!(1, r.0);
                    st.pc += 4;
                }
                MARRAY_SET => {
                    self.rt.array_set(gw!(1), g!(2) as i32, gw!(3));
                    st.pc += 4;
                }
                MARRAY_FREEZE => {
                    let r = self.rt.array_freeze(gw!(2))?;
                    put!(1, r.0);
                    st.pc += 3;
                }
                CALL_DIRECT => {
                    let (args, _) = slots!(3);
                    st.set_resume(a(1));
                    if let Some(v) = self.enter(st, FuncId(a(2)), Word(0), args)? {
                        return Ok(v);
                    }
                }
                CALL_VALUE => {
                    let (args, _) = slots!(3);
                    if let Some(v) = self.call_value(st, a(1), gw!(2), args, false)? {
                        return Ok(v);
                    }
                }
                CALL_METHOD | TAIL_METHOD => {
                    let no = a(5) as usize;
                    let own: Vec<Word> = (0..no).map(|i| Word(st.get(a(6 + i)))).collect();
                    let (args, _) = slots!(6 + no);
                    let tail = opc == TAIL_METHOD;
                    let r = self.select(st, a(1), ClassId(a(2)), a(3), gw!(4), &own, args, tail)?;
                    if let Some(v) = r {
                        return Ok(v);
                    }
                }
                DICT_APP => {
                    let f = gw!(2);
                    let (args, _) = slots!(3);
                    if self.rt.is_extern_value(f) {
                        let e = st.code().resume[a(1) as usize];
                        st.set(e.dst, f.0);
                        st.pc = e.pc_after as usize;
                    } else {
                        match self.rt.callee_view(f) {
                            CalleeView::Closure(func) => {
                                st.set_resume(a(1));
                                if let Some(v) = self.enter(st, func, f, args)? {
                                    return Ok(v);
                                }
                            }
                            _ => {
                                return Err(Stop::Internal(
                                    "辞書を受け取らない値に辞書を渡しました".into(),
                                ));
                            }
                        }
                    }
                }
                CONSOLE_WRITE => {
                    let s = gw!(2);
                    let bytes = self.rt.str_bytes(s);
                    // 文字列オブジェクトは移動せず、解放もされないので、コピーせずに渡す。
                    let bytes: &[u8] =
                        // SAFETY: bytes は生きている文字列オブジェクトの中を指す。
                        unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
                    self.rt.out.write(bytes)?;
                    put!(1, Word::UNIT.0);
                    st.pc += 3;
                }
                UNIMPL_VALUE | UNIMPLEMENTED => {
                    return Err(Stop::Unimplemented(st.code().texts[sub as usize].clone()));
                }
                JUMP => st.pc = a(1) as usize,
                BRANCH => {
                    st.pc = if g!(1) != 0 { a(2) } else { a(3) } as usize;
                }
                RETURN => {
                    let v = gw!(1);
                    if let Some(v) = self.ret(st, v)? {
                        return Ok(v);
                    }
                }
                TAIL_DIRECT => {
                    let (args, _) = slots!(2);
                    if let Some(v) = self.tail_enter(st, FuncId(a(1)), Word(0), args)? {
                        return Ok(v);
                    }
                }
                TAIL_VALUE => {
                    let (args, _) = slots!(3);
                    if let Some(v) = self.call_value(st, a(1), gw!(2), args, true)? {
                        return Ok(v);
                    }
                }
                ERROR => return Err(self.error(st, sub, a(1))),
                FX => {
                    if let Some(v) = self.fx(st, pc, sub)? {
                        return Ok(v);
                    }
                }
                FX_TERM => {
                    if let Some(v) = self.fx_term(st, pc, sub)? {
                        return Ok(v);
                    }
                }
                SAFEPOINT => {
                    self.rt.gc.at_safepoint(st.fpos, sub);
                    st.pc += 1;
                }
                _ => {
                    return Err(Stop::Internal(format!(
                        "知らない命令の番号 {opc}(位置 {pc})"
                    )));
                }
            }
        }
    }
}

/// `error` の命令の実行時エラーを作る(メッセージは IRSpec 付録 B が定める)。
/// `opnd` はオペランドのスロットのワードで、オペランドのないエラーでは使わない。
pub(crate) fn runtime_error(prog: &Program, rt: &Rt<'_>, kind: &RtError, opnd: u64) -> Stop {
    let show = |w: Word| rt.show(w);
    let msg: Vec<u8> = match kind {
        RtError::Hole => "??? に到達しました".into(),
        RtError::Unbound(g) => return rt.unbound(*g),
        RtError::PatternFail => {
            let mut m = "パターンに値が一致しません: ".as_bytes().to_vec();
            m.extend(show(Word(opnd)));
            m
        }
        RtError::MatchFail => {
            let mut m = "match のどの節にも一致しません: ".as_bytes().to_vec();
            m.extend(show(Word(opnd)));
            m
        }
        RtError::LitRange(s) => format!("数値リテラルが範囲外です: {s}").into(),
        RtError::InstanceNotReady(i) => {
            return keleut_rt::error::instance_not_ready(&prog.instances[i.index()].display);
        }
        RtError::DivZero => "ゼロ除算です".into(),
        RtError::F64ToIntNan(op) => format!("{}: NaN は整数に変換できません", conv_name(op)).into(),
        RtError::F64ToIntRange(op) => format!(
            "{}: 変換結果が範囲外です: {}",
            conv_name(op),
            keleut_rtcore::show_f64(f64::from_bits(opnd))
        )
        .into(),
        RtError::StrSubRange => "__string_sub: 範囲外です".into(),
        RtError::Panic => {
            let mut m = b"panic: ".to_vec();
            m.extend_from_slice(rt.str_bytes(Word(opnd)));
            m
        }
        RtError::ArrayIndex => "配列の範囲外です".into(),
        RtError::MArrayNegLen => "MutableArray.new: 長さが負です".into(),
        RtError::UnimplPrim(g) => return unimpl_prim(prog, *g),
        RtError::OutOfMemory => return Stop::OutOfMemory,
    };
    Stop::Runtime(msg)
}

/// 比較のサブコード `sub`(0 から順に eq、lt、le、gt、ge)を、順序 `o` に適用した結果を返す。
#[inline]
fn cmp(sub: u32, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match sub {
        0 => o == Equal,
        1 => o == Less,
        2 => o != Greater,
        3 => o == Greater,
        _ => o != Less,
    }
}
