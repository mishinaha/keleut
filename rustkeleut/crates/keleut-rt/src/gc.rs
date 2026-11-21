//! GC。オブジェクトを移動しないマークスイープである。
//!
//! GC は確保の中でだけ起こる([`Rt::alloc`])。
//! 確保の関数を呼ぶ前に、バックエンドは、現在の最上段のフレームの位置と、そのフレームが止まっている点(再開点か安全点)を
//! [`GcState::at_safepoint`] か [`GcState::at_frame`] で設定する(`doc/lir.md` の 1.8 節)。
//! 最上段の安全点はヘッダに書かない。
//! ヘッダの点は、確保に失敗して巻き戻すときに、呼び出し元の再開点として読まれることがあるからである。
//! たとえば、`cont_make` が継続を作れずに、区間(継続にするフレームの範囲)の最上段のフレームへ制御を移し、そこからメモリ不足のエラーを伝播して巻き戻すとき、
//! ハンドラのフレームは、本体を呼び出した再開点でエラーを受け取る。
//!
//! GC ルートは次のものである。
//!
//! | GC ルート | 走査するもの |
//! | --- | --- |
//! | 値スタック | [`GcState::top`] から呼び出し元を順にたどったフレームと、ネイティブからの入口([`GcState::enter_native`])ごとに [`GcState::outer`] に積んだ外側の最上段から同じようにたどったフレーム。状態ワードが `YIELD` なら、その `perform` のフレームから現在の最上段の手前までのフレームも走査する。各フレームでは、ヘッダの点の集合に入る `any` のスロットだけを走査する |
//! | ルートスタック([`crate::heap::Roots`]) | 積んだすべてのワード |
//! | グローバルとインスタンス | グローバルの値、`inst_set_value` が記録した値、文字列定数 |
//! | エフェクト | ハンドラの連鎖の先頭、状態ワードのペイロード、表に入れたがまだ添字を持つオブジェクトのない項目 |
//!
//! ハンドラ記録、継続、退避した状態ワードの表は GC ルートではない。
//! 添字を持つオブジェクトをマークしたときに項目のワードを走査し、継続が持つフレームのコピーも値スタックのフレームと同じ規則で走査する。
//! マークの後に、オブジェクトがマークされなかった項目を表から外す。
//!
//! マークは再帰を使わず、明示的なマークスタックを使うループで行うので、ネイティブスタックを消費しない。
//! マークスタックを拡張できないとき(システムのアロケータが確保に失敗したとき)は、オーバーフローのフラグを立て、それ以上はマークスタックに積まない。
//! マークスタックが空になったら、ヒープのマーク済みのオブジェクトをすべて走査し直す。
//! これを、オーバーフローが起きなくなるまで繰り返す。
//!
//! ## GC の中の確保
//!
//! GC の全体は、確保の失敗を呼び出し元が処理する範囲([`crate::alloc::handled`])で実行する。
//! GC の中の確保は、どれも `try_reserve` か `try_reserve_exact` で行う。
//! 失敗したときの扱いは、その確保に代替の経路があるかどうかで分かれる。
//!
//! | 確保 | 失敗したとき |
//! | --- | --- |
//! | マークスタック | オーバーフローのフラグを立て、上に書いた走査のやり直しに切り替える |
//! | スイープの中の確保(ページの配列の作り直し、プールのページ) | スイープがその場で代替の経路に切り替える([`crate::heap::Heap::sweep`]) |
//! | 関数ごとのルート情報のキャッシュ(`FuncRoots`)、再開点の表の項目ごとのルートのスロット、生存しているレジスタの集合を作る作業用バッファ | マークを中断する |
//!
//! キャッシュと項目ごとのスロットは、どの関数のどの項目をたどるかがマークの途中で決まるので、GC の前に確保しておけない。
//! キャッシュと項目は、確保できたときだけ記録する。
//! 途中まで作った項目を記録すると、次の GC がその項目のルートを見落とすからである。
//!
//! マークを中断したら、付けたマークを確保なしの走査で消し、表の項目のマーク済みの状態も元に戻す。
//! スイープをせずに [`Stop::OutOfMemory`] を返すので、ヒープの内容は GC の前と同じである。
//! そのあと `cancel` 節の確保が続けて GC を起こさないように、ヒープが次に GC を始める時点を、間隔 1 つ分だけ後にずらす
//! ([`crate::heap::Heap::defer_collect`])。
//! `--gc-stress` の規則 1(初めて通る確保箇所で GC を実行する)も、次に GC を終えるまで止める。
//!
//! ## フレームの点
//!
//! ヘッダの 0 番のワードの上位 32 ビットは、フレームが止まっている点である。
//! 最上段のフレームが確保の命令で止まっているときは、ヘッダの代わりに [`GcState::top_point`] を使う。
//! 再開点の番号は、バックエンドが持つ再開点の表での、項目の番号である。
//!
//! | 点 | ルートのスロット |
//! | --- | --- |
//! | [`SAFEPOINT`] フラグと安全点の番号 | 安全点の集合(LIR の [`keleut_lir::Frame::safepoints`] の番号) |
//! | 再開点の項目の番号(LIR の再開点の数より小さい) | 再開点の集合。`perform` の項目なら `perform` のオペランドも |
//! | それ以上の項目の番号 | 末尾呼び出し版の項目。終端命令が `tail_call_value` か `tail_call_method` のブロックだけを関数のブロックの順に数えて、0 始まりで `k` 番目のブロックに対応する。項目の番号は再開点の数 + `k` である。集合は空 |
//!
//! [`PHASE`] フラグが付いていれば、その項目の呼び出しの命令(末尾呼び出し版の項目ならブロックの終端命令)のオペランドもルートにする。
//! [`YIELDED`] フラグは取り除いて読む(`perform` の項目なので、オペランドはルートになる)。
//! `$rt_handle` のフレームは、どの点でも `any` の領域の 0 番のスロット(ハンドラ記録)をルートにする。

use std::collections::HashSet;
use std::time::{Duration, Instant};

use keleut_lir::{FuncId, FxOp, Inst, LiveWork, Program, Reg, Slot, Term};

use crate::Rt;
use crate::alloc::{self, Role, Site, handled};
use crate::effect::{Cont, HandlerRec, Pending};
use crate::error::Stop;
use crate::heap::{MARK, Roots};
use crate::stack::{HEADER_WORDS, NATIVE_CALLER, ValueStack};
use crate::value::{Kind, Word};

/// フレームの点の項目の番号(再開点の番号か、末尾呼び出し版の項目の番号)に付けるフラグ。
///
/// その項目の呼び出しの命令(末尾呼び出し版の項目ならブロックの終端命令)が、メソッドの実装の関数(インスタンスがメソッドごとに持つ関数)を評価している途中であることを表す(`doc/lir.md` の 1.8 節)。
pub const PHASE: u32 = 1 << 31;

/// `perform` の再開点の番号に付けて、そのフレームが分類 G の操作節(IRSpec 9.2 節)の継続にキャプチャされたこと(継続の最上段であること)を表すフラグ。
pub const YIELDED: u32 = 1 << 30;

/// ヘッダの点が安全点(確保する命令)であることを表すフラグ。下位のビットは安全点の番号である。
pub const SAFEPOINT: u32 = 1 << 29;

/// ヘッダの点を使うことを表す [`GcState::top_point`] の値。
pub const NO_POINT: u32 = u32::MAX;

/// 点から安全点の番号を取り出すマスク。
const POINT_MASK: u32 = SAFEPOINT - 1;

/// `--gc-stress` で GC の間隔を決める比。前の GC の処理量が `W` なら、確保の回数がおよそ `W / STRESS_RATIO` に達するまで GC を実行しない。
///
/// `--gc-stress` は次の 2 つの規則で GC を実行する([`Rt::alloc`])。
///
/// 1. 確保箇所を初めて通る確保では、必ず GC を実行する。
///    確保箇所は、最上段のフレームの関数と点の組である。最上段のフレームがないときは、全体で 1 つの確保箇所として扱う。
///    この規則によって、実行したどの確保箇所でも、その確保箇所のルートを使う GC が少なくとも 1 回起こる。
///    確保箇所の数はプログラムの大きさで決まるので、この規則による GC の処理量の合計は、確保箇所の数とスタックの深さの積で抑えられる。
///    深い再帰の確保箇所も、通常はスタックがまだ浅いうちに初めて通る。
/// 2. それ以外の確保では、前の GC の後の確保の回数が間隔に達したときに GC を実行する。
///    間隔は、`W / STRESS_RATIO` に 0.5 倍以上 1.5 倍未満の擬似乱数の係数を掛けて切り上げたもので、1 以上である。
///    係数は固定のシードから作るので、同じプログラムでは同じタイミングで GC を実行する。
///    係数を変動させるのは、ループ 1 回あたりの確保の回数が間隔の倍数のときに、同じ確保箇所ばかりで GC を実行するのを避けるためである。
///
/// 前の GC の処理量には、グローバルの値、文字列定数、インスタンスの値のマークと、ページのスイープの分が毎回入る。
/// そのため、小さなプログラムでも `W` は `STRESS_RATIO` を超え、規則 2 だけでは確保のたびには GC を実行しない。
/// コーパスでの実測では、平均で確保 2.5 回に 1 回ほどである。
/// 深いスタックや大きなヒープで確保のたびに GC を実行すると、処理量がスタックの深さと確保の回数の積になり、
/// 10 万段の再帰のテストが終わらない。
/// 規則 2 によって、GC の処理量の合計は確保の回数の `STRESS_RATIO` 倍ほどに収まる。
///
/// 規則 1 が保証するのは、最上段のフレームの確保箇所だけである。
/// 呼び出し元のフレームが止まっている再開点のルートを GC が使うのは、その再開点で止まっている間に GC が起こったときだけである。
/// 規則 2 の間隔の変動によってそうなる可能性は高いが、保証はしない。
pub const STRESS_RATIO: u64 = 64;

/// `--gc-stress` の間隔の係数を作る擬似乱数のシード。
const STRESS_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// 最上段のフレームがないときの確保箇所。
const NATIVE_SITE: u64 = u64::MAX;

/// 走査したフレーム 1 枚あたり、次の GC までの間隔に足すワード数。
///
/// GC のたびにスタックのすべてのフレームを走査し直すので、深いスタックでは 1 回の GC の処理量がフレームの数に比例する。
/// 間隔を GC の後に残った量だけで決めると、GC の処理量の合計がスタックの深さと確保の量の積になる。
/// そこで、フレームの数に比例して間隔を広げ、処理量の合計を確保の量に比例させる。
/// フレーム 1 枚の走査の処理量は、オブジェクト数個のマークと同じくらいである。
pub const FRAME_TRIGGER_WORDS: u64 = 4;

/// GC が項目を管理する表の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// ハンドラ記録。
    Handler = 0,
    /// 継続。
    Cont = 1,
    /// 退避した状態ワード。
    Pending = 2,
}

/// 表の項目の状態。`LIVE` は使用中、`MARKED` はマーク済み、`FREE` は空きである。
const LIVE: u8 = 0;
const MARKED: u8 = 1;
const FREE: u8 = 2;

/// 表 1 つ分の GC の状態。
#[derive(Default)]
pub struct TableGc {
    /// 項目ごとの状態(使用中、マーク済み、空き)。
    state: Vec<u8>,
    /// 空いた添字。GC の中で確保しないように、容量を項目の数以上に保つ。
    free: Vec<usize>,
}

impl TableGc {
    /// 次に使う添字を決める。再利用できる添字があればそれを返し、なければ `len`(新しい項目の添字)を返す。
    /// 新しい項目の状態を記録する領域を先に確保し、確保できなければ [`Stop::OutOfMemory`] を返す。
    ///
    /// `ahead` は、新しい項目の後にさらに空けておく項目の数である([`TableGc::keep_spare`])。
    pub fn next_index(&mut self, len: usize, ahead: usize) -> Result<usize, Stop> {
        if let Some(i) = self.free.pop() {
            self.state[i] = LIVE;
            return Ok(i);
        }
        debug_assert_eq!(self.state.len(), len);
        self.keep_spare(1 + ahead).map_err(|_| Stop::OutOfMemory)?;
        self.state.push(LIVE);
        Ok(len)
    }

    /// 項目を `n` 個追加しても状態を記録する領域を確保し直さずに済むように、容量を先に確保する。
    /// 確保は、確保の失敗を呼び出し元が処理する範囲([`crate::alloc::handled`])で行う。
    ///
    /// 退避した状態ワードの表は、常に 1 個分を空けておく。
    /// メモリ不足を巻き戻す途中の退避が、表を拡張せずに済むようにするためである(`Rt::pending_box`)。
    pub fn keep_spare(&mut self, n: usize) -> Result<(), std::collections::TryReserveError> {
        let len = self.state.len();
        handled(|| self.state.try_reserve(n))?;
        let want = len + n;
        if self.free.capacity() < want {
            handled(|| self.free.try_reserve(want - self.free.len()))?;
        }
        Ok(())
    }

    /// 添字 `i` を空きにする。
    pub fn release(&mut self, i: usize) {
        self.state[i] = FREE;
        self.free.push(i);
    }

    /// 状態を記録している項目の数を返す。表の長さと同じである。
    pub fn state_len(&self) -> usize {
        self.state.len()
    }

    /// 使用中の項目の数を返す。
    pub fn live(&self) -> usize {
        self.state.len() - self.free.len()
    }
}

/// 関数ごとのルート情報のキャッシュ。再開点の表の項目ごとに、ルートになるスロットを持つ。
struct FuncRoots {
    /// LIR の再開点の数。
    nrp: usize,
    /// 末尾呼び出し版の項目に対応するブロックの番号。ブロックの順に並べる。
    tails: Vec<u32>,
    /// 項目ごとの、ルートになるスロット(`any` の領域の中の番号)。まだ作っていない項目は `None` である。
    rows: Vec<Option<Box<[u32]>>>,
    /// 項目ごとの、`PHASE` フラグが付いているときに追加するスロット。
    phase: Vec<Option<Box<[u32]>>>,
    /// 関数が `$rt_handle` なら真。
    rt_handle: bool,
}

/// GC の状態。最上段のフレームの位置、表ごとの状態、統計、GC の間で再利用するバッファを持つ。
pub struct GcState {
    /// 現在の最上段のフレームの論理的な位置。フレームがなければ [`NATIVE_CALLER`] である。
    pub top: u64,
    /// 最上段のフレームが止まっている安全点(`SAFEPOINT` フラグと番号)。ヘッダの点を使うなら [`NO_POINT`] である。
    pub top_point: u32,
    /// [`GcState::enter_native`] の呼び出しごとに積んだ、外側の最上段のフレームの位置。
    pub outer: Vec<u64>,
    /// `--gc-stress` なら真。真のとき、通常の条件に加えて [`STRESS_RATIO`] の規則でも GC を始める。
    pub stress: bool,
    /// 前の GC の処理量。走査したフレーム、マークしたオブジェクト、スイープで調べたセルと表の項目の数から求める。
    pub last_work: u64,
    /// 前の GC の後の確保の回数。
    pub allocs_since: u64,
    /// `--gc-stress` で、次に GC を実行する確保の回数([`STRESS_RATIO`] の規則 2)。
    pub stress_due: u64,
    /// `--gc-stress` の間隔の係数を作る擬似乱数の状態(xorshift)。
    stress_rng: u64,
    /// `--gc-stress` で GC を試した確保箇所。上位 32 ビットが関数の番号、下位 32 ビットが点である。
    tested: HashSet<u64>,
    /// 最後まで終えた GC の回数。
    pub collections: u64,
    /// GC にかかった時間の合計。中断した GC の時間も含む。
    pub time: Duration,
    /// GC が表から外した項目の数の合計。
    pub freed_entries: u64,
    /// 表に入れたが、まだ添字を持つオブジェクトのない項目。
    pub(crate) unboxed: Option<(Table, usize)>,
    /// 表ごとの状態。
    pub(crate) tables: [TableGc; 3],
    /// マークスタック。
    mark: Vec<u64>,
    /// マークスタックのオーバーフローのフラグ。
    overflow: bool,
    /// 関数ごとのルート情報のキャッシュ。フレームを初めて走査したときに作る。
    funcs: Vec<Option<FuncRoots>>,
    /// スロットの番号を集める作業用バッファ。
    tmp: Vec<u32>,
    /// 生存しているレジスタの集合の出力先。GC のたびに再利用する。
    live: Vec<Reg>,
    /// [`keleut_lir::Points::live_into`] の作業用バッファ。
    live_work: LiveWork,
    /// 前の GC がマークを中断していれば真。真の間は、次に GC を終えるまで `--gc-stress` の規則 1 を止める。
    aborted: bool,
    /// マークを中断した GC の回数。
    pub aborts: u64,
}

impl GcState {
    /// 関数ごとのルート情報のキャッシュを `nfuncs` 個に拡張する。プログラムに単位を追加したときに [`Rt::grow`] が呼ぶ。
    pub fn grow_funcs(&mut self, nfuncs: usize) {
        if self.funcs.len() < nfuncs {
            self.funcs.resize_with(nfuncs, || None);
        }
    }

    /// 確保の前に呼ぶ。最上段のフレームの位置を `pos` に、そのフレームが止まっている点を安全点 `sp`(LIR の安全点の番号)に設定する。
    #[inline]
    pub fn at_safepoint(&mut self, pos: u64, sp: u32) {
        self.top = pos;
        self.top_point = sp | SAFEPOINT;
    }

    /// ランタイムが確保する前に呼ぶ。最上段のフレームの位置を `pos` に設定し、点にはそのフレームのヘッダの点(再開点)を使うように設定する。
    #[inline]
    pub fn at_frame(&mut self, pos: u64) {
        self.top = pos;
        self.top_point = NO_POINT;
    }

    /// Rust(ネイティブの呼び出し元)から、値スタックのフレームの連鎖に入る。
    ///
    /// 外側の最上段の位置を [`GcState::outer`] に積み、[`GcState::leave_native`] に渡す値を返す。
    /// 最下段のヘッダの 3 番のワードが [`NATIVE_CALLER`] になる連鎖を始めるときは、どのバックエンドでもこの関数を使う。
    /// 使わないと、GC は入った位置より下のフレームをたどらない。
    /// 入れ子になった `Interp::run` の呼び出し(トップレベルの初期化コードの実行と、構造的な `Eq` の比較の入れ子)がこれにあたる。
    /// JIT とインタプリタの境界では、ヘッダの 3 番のワードに実際の呼び出し元の位置を書くので、この関数を使わない。
    /// 値スタックのフレームどうしの通常の呼び出しも、ヘッダの 3 番のワードに呼び出し元の位置を書くので、この関数を使わない。
    #[must_use]
    pub fn enter_native(&mut self) -> (u64, u32) {
        let saved = (self.top, self.top_point);
        self.outer.push(saved.0);
        saved
    }

    /// [`GcState::enter_native`] で入った連鎖から戻る。積んだ位置を外し、最上段の位置と点を元に戻す。
    pub fn leave_native(&mut self, saved: (u64, u32)) {
        self.outer.pop();
        self.top = saved.0;
        self.top_point = saved.1;
    }

    /// 関数が `nfuncs` 個あるプログラムの、最初の状態を作る。
    pub fn new(nfuncs: usize) -> GcState {
        GcState {
            top: NATIVE_CALLER,
            top_point: NO_POINT,
            outer: Vec::new(),
            stress: false,
            last_work: 0,
            allocs_since: 0,
            stress_due: 0,
            stress_rng: STRESS_SEED,
            tested: HashSet::new(),
            collections: 0,
            time: Duration::ZERO,
            freed_entries: 0,
            unboxed: None,
            tables: Default::default(),
            mark: Vec::new(),
            overflow: false,
            funcs: (0..nfuncs).map(|_| None).collect(),
            tmp: Vec::new(),
            live: Vec::new(),
            live_work: LiveWork::default(),
            aborted: false,
            aborts: 0,
        }
    }
}

/// マークの状態。マークスタックと、処理量を数えるカウンタを持つ。
struct Marker<'a> {
    /// マークスタック。
    stack: &'a mut Vec<u64>,
    /// マークスタックのオーバーフローのフラグ。
    overflow: &'a mut bool,
    /// 処理量。マークしたオブジェクトの数と、値スタックで走査したフレームの数の和である。
    work: u64,
    /// 走査したフレームの数。値スタックのフレームと、継続が持つフレームのコピーを数える。
    frames: u64,
    /// 代替の経路を持たない確保が失敗し、マークを中断するなら真。
    failed: bool,
}

impl Marker<'_> {
    /// ワードがポインタなら、指すオブジェクトをマークする。初めてマークしたときはマークスタックに積む。
    #[inline]
    fn word(&mut self, w: u64) {
        if w & 7 != 0 || w == 0 {
            return;
        }
        let p = w as *mut u64;
        // SAFETY: GC ルートとオブジェクトの値のフィールドにあるポインタのワードは、生きているオブジェクトを指す(ルートの不変条件)。
        unsafe {
            let h = *p;
            if h & MARK != 0 {
                return;
            }
            *p = h | MARK;
        }
        self.work += 1;
        if self.stack.len() == self.stack.capacity()
            && handled(|| self.stack.try_reserve(1)).is_err()
        {
            *self.overflow = true;
            return;
        }
        self.stack.push(w);
    }
}

/// GC の中で、代替の経路を持たない確保が失敗したことを表す。
struct NoRoom;

/// `regs` のうち `any` の領域に置かれたレジスタのスロットを `out` に追加する。
fn any_slots(prog: &Program, f: FuncId, regs: &[Reg], out: &mut Vec<u32>) -> Result<(), NoRoom> {
    let fr = prog.funcs[f.index()]
        .frame
        .as_ref()
        .expect("パス 12 を済ませた関数");
    out.try_reserve(regs.len()).map_err(|_| NoRoom)?;
    for r in regs {
        if let Slot::Any(n) = fr.slot[r.index()] {
            out.push(n);
        }
    }
    Ok(())
}

/// `visit` が渡すレジスタのうち `any` の領域に置かれたもののスロットを `out` に追加する。
fn any_uses(
    prog: &Program,
    f: FuncId,
    visit: impl FnOnce(&mut dyn FnMut(Reg)),
    out: &mut Vec<u32>,
) -> Result<(), NoRoom> {
    let fr = prog.funcs[f.index()]
        .frame
        .as_ref()
        .expect("パス 12 を済ませた関数");
    let mut ok = true;
    visit(&mut |r| {
        if let Slot::Any(n) = fr.slot[r.index()] {
            if out.try_reserve(1).is_ok() {
                out.push(n);
            } else {
                ok = false;
            }
        }
    });
    if ok { Ok(()) } else { Err(NoRoom) }
}

/// `v` を、容量が長さと等しい領域にコピーする。`into_boxed_slice` が領域を確保し直さないようにするためである。
fn boxed(v: &[u32]) -> Result<Box<[u32]>, NoRoom> {
    let mut b = Vec::new();
    b.try_reserve_exact(v.len()).map_err(|_| NoRoom)?;
    b.extend_from_slice(v);
    Ok(b.into_boxed_slice())
}

/// 項目のルートを作るときに再利用するバッファ。
struct Work<'a> {
    tmp: &'a mut Vec<u32>,
    live: &'a mut Vec<Reg>,
    live_work: &'a mut LiveWork,
}

impl FuncRoots {
    /// 関数 `f` のキャッシュを作る。項目ごとのスロットは、初めて使うときに作る。
    fn new(prog: &Program, f: FuncId) -> Result<FuncRoots, NoRoom> {
        let func = &prog.funcs[f.index()];
        let fr = func.frame.as_ref().expect("パス 12 を済ませた関数");
        let is_tail = |b: &keleut_lir::Block| {
            matches!(
                b.term,
                Term::TailCallValue { .. } | Term::TailCallMethod { .. }
            )
        };
        let mut tails = Vec::new();
        tails
            .try_reserve_exact(func.blocks.iter().filter(|b| is_tail(b)).count())
            .map_err(|_| NoRoom)?;
        tails.extend(
            func.blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| is_tail(b))
                .map(|(i, _)| i as u32),
        );
        let nrp = fr.resume_points.len();
        let n = nrp + tails.len();
        let mut rows = Vec::new();
        rows.try_reserve_exact(n).map_err(|_| NoRoom)?;
        rows.resize_with(n, || None);
        let mut phase = Vec::new();
        phase.try_reserve_exact(n).map_err(|_| NoRoom)?;
        phase.resize_with(n, || None);
        Ok(FuncRoots {
            nrp,
            tails,
            rows,
            phase,
            rt_handle: prog.rt_handle == Some(f),
        })
    }

    /// 項目 `row` のルートになるスロットを返す。まだ作っていなければ作って記録する。
    fn row(
        &mut self,
        prog: &Program,
        f: FuncId,
        row: usize,
        w: &mut Work<'_>,
    ) -> Result<&[u32], NoRoom> {
        if self.rows[row].is_none() {
            w.tmp.clear();
            if row < self.nrp {
                let func = &prog.funcs[f.index()];
                let fr = func.frame.as_ref().expect("パス 12 を済ませた関数");
                w.live.clear();
                fr.resume_points
                    .live_into(row, w.live, w.live_work)
                    .map_err(|_| NoRoom)?;
                any_slots(prog, f, w.live, w.tmp)?;
                let pt = fr.resume_points.point(row);
                let inst = &func.blocks[pt.block.index()].insts[pt.index as usize];
                if matches!(
                    inst,
                    Inst::Fx {
                        op: FxOp::Perform { .. },
                        ..
                    }
                ) {
                    any_uses(prog, f, |g| inst.for_each_use(g), w.tmp)?;
                }
            }
            if self.rt_handle {
                w.tmp.try_reserve(1).map_err(|_| NoRoom)?;
                w.tmp.push(0);
            }
            // 作り終えた項目だけを記録する。
            self.rows[row] = Some(boxed(w.tmp)?);
        }
        Ok(self.rows[row].as_deref().unwrap_or(&[]))
    }

    /// 項目 `row` で `PHASE` フラグが付いているときに追加するスロットを返す。まだ作っていなければ作って記録する。
    fn phase(
        &mut self,
        prog: &Program,
        f: FuncId,
        row: usize,
        w: &mut Work<'_>,
    ) -> Result<&[u32], NoRoom> {
        if self.phase[row].is_none() {
            w.tmp.clear();
            let func = &prog.funcs[f.index()];
            if row < self.nrp {
                let fr = func.frame.as_ref().expect("パス 12 を済ませた関数");
                let pt = fr.resume_points.point(row);
                let inst = &func.blocks[pt.block.index()].insts[pt.index as usize];
                any_uses(prog, f, |g| inst.for_each_use(g), w.tmp)?;
            } else {
                let b = self.tails[row - self.nrp] as usize;
                let term = &func.blocks[b].term;
                any_uses(prog, f, |g| term.for_each_use(g), w.tmp)?;
            }
            self.phase[row] = Some(boxed(w.tmp)?);
        }
        Ok(self.phase[row].as_deref().unwrap_or(&[]))
    }
}

/// フレームの走査に必要なものをまとめた構造体。
struct Frames<'a, 'p> {
    prog: &'p Program,
    funcs: &'a mut Vec<Option<FuncRoots>>,
    work: Work<'a>,
}

impl Frames<'_, '_> {
    /// ヘッダが `p` にあるフレームの、ルートになるスロットの値をマークする。
    /// `point` が [`NO_POINT`] でなければ、ヘッダの点の代わりにそれを使う(最上段の安全点)。
    /// ルートを作る確保が失敗したら、`m.failed` を立てて戻る。
    fn scan(&mut self, m: &mut Marker<'_>, p: *const u64, point: u32) {
        if m.failed {
            return;
        }
        if self.scan_inner(m, p, point).is_err() {
            m.failed = true;
        }
    }

    fn scan_inner(&mut self, m: &mut Marker<'_>, p: *const u64, point: u32) -> Result<(), NoRoom> {
        // SAFETY: p はフレームのヘッダを指し、フレームはヘッダの 2 番のワードが表す大きさを持つ。
        let h0 = unsafe { *p };
        let f = FuncId(h0 as u32);
        let pt = if point != NO_POINT {
            point
        } else {
            (h0 >> 32) as u32
        };
        let slot = |n: u32| -> u64 {
            // SAFETY: n は関数のフレームの any の領域のスロットである。
            unsafe { *p.add(HEADER_WORDS + n as usize) }
        };
        let entry = &mut self.funcs[f.index()];
        if entry.is_none() {
            // 作り終えたキャッシュだけを記録する。
            *entry = Some(FuncRoots::new(self.prog, f)?);
        }
        let fr = entry.as_mut().expect("直前に入れた");
        let w = &mut self.work;
        if pt & SAFEPOINT != 0 {
            let i = (pt & POINT_MASK) as usize;
            let frame = self.prog.funcs[f.index()]
                .frame
                .as_ref()
                .expect("パス 12 を済ませた関数");
            w.tmp.clear();
            w.live.clear();
            frame
                .safepoints
                .live_into(i, w.live, w.live_work)
                .map_err(|_| NoRoom)?;
            any_slots(self.prog, f, w.live, w.tmp)?;
            if fr.rt_handle {
                w.tmp.try_reserve(1).map_err(|_| NoRoom)?;
                w.tmp.push(0);
            }
            for &n in w.tmp.iter() {
                m.word(slot(n));
            }
            return Ok(());
        }
        let row = (pt & !(PHASE | YIELDED)) as usize;
        for &n in fr.row(self.prog, f, row, w)? {
            m.word(slot(n));
        }
        if pt & PHASE != 0 {
            for &n in fr.phase(self.prog, f, row, w)? {
                m.word(slot(n));
            }
        }
        Ok(())
    }

    /// 位置 `pos` の最上段から、ネイティブの呼び出し元か位置 `stop` のフレームの手前まで、呼び出し元を順にたどって走査する。
    /// 最初のフレームには点 `point` を使う(ヘッダの点を使うなら [`NO_POINT`])。
    fn walk(
        &mut self,
        m: &mut Marker<'_>,
        stack: &ValueStack,
        mut pos: u64,
        stop: u64,
        mut point: u32,
    ) {
        while pos != NATIVE_CALLER && pos != stop && !m.failed {
            let p = stack.ptr_at_ro(pos);
            m.work += 1;
            m.frames += 1;
            self.scan(m, p, point);
            point = NO_POINT;
            // SAFETY: p はフレームのヘッダを指す。
            pos = unsafe { *p.add(3) };
        }
    }

    /// 継続が持つフレームのコピーを走査する。
    fn cont(&mut self, m: &mut Marker<'_>, c: &Cont) {
        let mut off = 0usize;
        for &sz in &c.sizes {
            if m.failed {
                return;
            }
            m.frames += 1;
            self.scan(m, c.frames[off..].as_ptr(), NO_POINT);
            off += sz as usize;
        }
    }
}

/// 状態ワード `p` のペイロードをマークする。
fn mark_pending(m: &mut Marker<'_>, p: &Pending) {
    match p {
        Pending::Yield { hr, args, cut, .. } => {
            m.word(hr.0);
            m.word(args.0);
            m.word(cut.0);
        }
        Pending::DiscardValue { hr, v } => {
            m.word(hr.0);
            m.word(v.0);
        }
        Pending::DiscardNever { hr, args, .. } => {
            m.word(hr.0);
            m.word(args.0);
        }
        Pending::ClauseValue(v) | Pending::TailResume(v) => m.word(v.0),
        Pending::None | Pending::Error(_) | Pending::Matched(_) => {}
    }
}

/// ハンドラ記録 `r` が持つワードをマークする。
fn mark_handler(m: &mut Marker<'_>, r: &HandlerRec) {
    m.word(r.body.0);
    m.word(r.res.0);
    for o in &r.ops {
        m.word(o.2.0);
    }
    if let Some(w) = r.ret {
        m.word(w.0);
    }
    if let Some(w) = r.cancel {
        m.word(w.0);
    }
    m.word(r.parent.0);
}

/// マークの間に読む表と、表ごとの GC の状態。
struct Tables<'a> {
    handlers: &'a [HandlerRec],
    conts: &'a [Cont],
    saved: &'a [Pending],
    gc: &'a mut [TableGc; 3],
}

impl Tables<'_> {
    /// 表 `t` の項目 `i` を走査する。すでに走査した項目は走査しない。
    fn entry(&mut self, m: &mut Marker<'_>, fr: &mut Frames<'_, '_>, t: Table, i: usize) {
        let st = &mut self.gc[t as usize].state[i];
        if *st != LIVE || m.failed {
            return;
        }
        *st = MARKED;
        match t {
            Table::Handler => mark_handler(m, &self.handlers[i]),
            Table::Cont => {
                let c = &self.conts[i];
                m.word(c.hr.0);
                m.word(c.cut.0);
                m.word(c.args.0);
                fr.cont(m, c);
            }
            Table::Pending => mark_pending(m, &self.saved[i]),
        }
    }

    /// オブジェクト `p` の値のフィールドが指すオブジェクトをマークする。
    fn children(&mut self, m: &mut Marker<'_>, fr: &mut Frames<'_, '_>, p: *mut u64) {
        // SAFETY: p は生きているオブジェクトを指し、レイアウトはヘッダの種類で決まる。
        unsafe {
            let h = *p;
            let n = (h >> 32) as usize;
            let at = |i: usize| *p.add(i);
            match (h & 0xff) as u8 {
                k if k == Kind::Record as u8
                    || k == Kind::Data as u8
                    || k == Kind::Closure as u8
                    || k == Kind::DictInst as u8 =>
                {
                    for i in 0..n {
                        m.word(at(2 + i));
                    }
                }
                k if k == Kind::Variant as u8 => m.word(at(2)),
                k if k == Kind::Method as u8 => {
                    for i in 0..=n {
                        m.word(at(2 + i));
                    }
                }
                k if k == Kind::DictRecord as u8 || k == Kind::DictVariant as u8 => {
                    for i in 0..n {
                        m.word(at(2 + 2 * i));
                    }
                }
                k if k == Kind::Ref as u8 => m.word(at(1)),
                k if k == Kind::Array as u8 || k == Kind::MArray as u8 => {
                    for i in 0..n {
                        m.word(at(1 + i));
                    }
                }
                k if k == Kind::Handler as u8 => self.entry(m, fr, Table::Handler, at(1) as usize),
                k if k == Kind::Cont as u8 => self.entry(m, fr, Table::Cont, at(1) as usize),
                k if k == Kind::Pending as u8 => self.entry(m, fr, Table::Pending, at(1) as usize),
                // Int64、Float64、文字列、実装のない extern の値は、値のフィールドを持たない。
                _ => {}
            }
        }
    }
}

/// ルートスタック、グローバルの値、インスタンスの値をマークする。
fn mark_roots(m: &mut Marker<'_>, roots: &Roots, globals: &[Word], insts: &[Vec<Word>]) {
    for &w in roots.as_slice() {
        m.word(w);
    }
    for w in globals {
        m.word(w.0);
    }
    for vs in insts {
        for w in vs {
            m.word(w.0);
        }
    }
}

impl Rt<'_> {
    /// `words` ワードのオブジェクトの領域を確保する。GC を始める条件を満たしていれば、先に GC を実行する。
    /// `--gc-stress` のときは、[`STRESS_RATIO`] の規則でも GC を実行する。
    ///
    /// 確保できなければ GC を実行してから試し直し、それでも確保できなければ [`Stop::OutOfMemory`] を返す。
    /// 呼ぶ前に、バックエンドは [`GcState::top`] と最上段のフレームの点を設定しておく(このモジュールの冒頭)。
    pub fn alloc(&mut self, words: usize) -> Result<*mut u64, Stop> {
        if alloc::take_flag(Role::Exec) {
            // ほかの確保がグローバルアロケータの予備領域を使った(`crate::alloc`)。
            return Err(Stop::OutOfMemory);
        }
        self.alloc_after_flag(words)
    }

    /// [`Rt::alloc`] から、先頭の検査を除いた部分。
    /// 先頭の検査は、実行スレッドの確保がグローバルアロケータの予備領域を使ったことを表すフラグ(`crate::alloc`)を調べる。
    ///
    /// メモリ不足を巻き戻す途中の退避(`Rt::pending_box`)は、このフラグを調べずにこの関数を呼ぶ。
    /// そのフラグは、いま報告しているメモリ不足と同じメモリ不足を表すからである。
    #[inline(always)]
    pub(crate) fn alloc_after_flag(&mut self, words: usize) -> Result<*mut u64, Stop> {
        let mut collected = false;
        let stress = self.gc.stress && self.stress_now();
        if stress || self.heap.should_collect() {
            self.collect()?;
            collected = true;
            if alloc::take_flag(Role::Exec) {
                return Err(Stop::OutOfMemory);
            }
        }
        if let Ok(p) = self.heap.try_alloc(words) {
            return Ok(p);
        }
        if !collected {
            if self.collect().is_err() {
                // ヒープから確保できなくなった後の GC を中断した。GC を終えても確保できなかったときと同じく、
                // GC のヒープの予備のページに切り替えてから、メモリ不足を返す。
                return Err(self.heap.exhausted(words).into());
            }
            if alloc::take_flag(Role::Exec) {
                return Err(Stop::OutOfMemory);
            }
            if let Ok(p) = self.heap.try_alloc(words) {
                return Ok(p);
            }
        }
        Err(self.heap.exhausted(words).into())
    }

    /// `--gc-stress` で、この確保の前に GC を実行するなら真を返す([`STRESS_RATIO`] の規則)。
    fn stress_now(&mut self) -> bool {
        let g = &mut self.gc;
        g.allocs_since += 1;
        let site = if g.top == NATIVE_CALLER {
            NATIVE_SITE
        } else {
            // SAFETY: 確保の前に、バックエンドは top を最上段のフレームのヘッダの位置にしておく(このモジュールの冒頭)。
            let h = unsafe { *self.stack.ptr_at_ro(g.top) };
            let point = if g.top_point == NO_POINT {
                (h >> 32) as u32
            } else {
                g.top_point
            };
            ((h & 0xffff_ffff) << 32) | point as u64
        };
        if !g.aborted && !g.tested.contains(&site) {
            // 記録する領域を確保できなくても GC は実行する。記録しなければ、次に通るときも GC を実行する。
            if handled(|| g.tested.try_reserve(1)).is_ok() {
                g.tested.insert(site);
            }
            return true;
        }
        g.allocs_since >= g.stress_due
    }

    /// GC を実行する。
    ///
    /// 代替の経路を持たない確保が失敗したら、マークを中断し、ヒープを GC の前のままにして
    /// [`Stop::OutOfMemory`] を返す(このモジュールの冒頭の「GC の中の確保」)。
    ///
    /// GC の後に、生きているフレームより後ろにある値スタックのチャンクを解放する([`ValueStack::release_after_collect`])。
    /// 生きているフレームのうち最も上のものは、状態ワードが `YIELD` ならその `perform` のフレーム、そうでなければ [`GcState::top`] である。
    /// マークを中断したときも解放する。
    /// 捨てるチャンクのフレームは GC ルートとしてたどっておらず、チャンクの解放は確保を伴わないからである。
    pub fn collect(&mut self) -> Result<(), Stop> {
        let r = {
            let _site = alloc::site(Site::Collect);
            handled(|| self.collect_in_scope())
        };
        let mut live = self.gc.top;
        if let Pending::Yield { top: y, .. } = self.pending
            && (live == NATIVE_CALLER || y > live)
        {
            live = y;
        }
        self.stack.release_after_collect(live);
        r
    }

    /// [`Rt::collect`] の本体。確保の失敗を呼び出し元が処理する範囲の中で、マークとスイープを行う。
    fn collect_in_scope(&mut self) -> Result<(), Stop> {
        let t0 = Instant::now();
        let Rt {
            prog,
            heap,
            roots,
            stack,
            handlers,
            conts,
            saved,
            chain,
            pending,
            globals,
            inst_values,
            strings,
            gc,
            ..
        } = self;
        let GcState {
            top,
            top_point,
            outer,
            unboxed,
            tables,
            mark,
            overflow,
            funcs,
            tmp,
            live,
            live_work,
            ..
        } = gc;
        *overflow = false;
        mark.clear();
        let mut m = Marker {
            stack: mark,
            overflow,
            work: 0,
            frames: 0,
            failed: false,
        };
        let mut fr = Frames {
            prog,
            funcs,
            work: Work {
                tmp,
                live,
                live_work,
            },
        };
        let mut tabs = Tables {
            handlers,
            conts,
            saved,
            gc: tables,
        };
        mark_roots(&mut m, roots, globals, inst_values);
        for w in strings.iter() {
            m.word(w.0);
        }
        m.word(chain.0);
        mark_pending(&mut m, pending);
        if let Some((t, i)) = *unboxed {
            tabs.entry(&mut m, &mut fr, t, i);
        }
        if let Pending::Yield { top: y, .. } = pending {
            // まだ継続にコピーしていない、perform のフレームから現在の最上段の手前までのフレーム。
            fr.walk(&mut m, stack, *y, *top, NO_POINT);
        }
        fr.walk(&mut m, stack, *top, NATIVE_CALLER, *top_point);
        for &o in outer.iter().rev() {
            fr.walk(&mut m, stack, o, NATIVE_CALLER, NO_POINT);
        }
        while !m.failed {
            while let Some(w) = m.stack.pop() {
                if m.failed {
                    break;
                }
                tabs.children(&mut m, &mut fr, w as *mut u64);
            }
            if m.failed || !*m.overflow {
                break;
            }
            // オーバーフローした。マーク済みのオブジェクトをすべて走査し直す(子がマーク済みなら何もしない)。
            *m.overflow = false;
            heap.for_each_object(|p| {
                // SAFETY: p は生きているオブジェクトのヘッダを指す。
                if !m.failed && unsafe { *p } & MARK != 0 {
                    tabs.children(&mut m, &mut fr, p);
                    while let Some(w) = m.stack.pop() {
                        if m.failed {
                            break;
                        }
                        tabs.children(&mut m, &mut fr, w as *mut u64);
                    }
                }
            });
        }
        if m.failed {
            // マークを中断する。付けたマークを確保なしで消し、スイープをしない。
            m.stack.clear();
            *m.overflow = false;
            heap.for_each_object(|p| {
                // SAFETY: p は生きているオブジェクトのヘッダを指す。
                unsafe { *p &= !MARK };
            });
            for g in tables.iter_mut() {
                for st in g.state.iter_mut() {
                    if *st == MARKED {
                        *st = LIVE;
                    }
                }
            }
            // `cancel` 節の確保がすぐまた GC を起こさないように、次に GC を始める時点をずらす。
            heap.defer_collect();
            gc.allocs_since = 0;
            gc.aborted = true;
            gc.aborts += 1;
            gc.time += t0.elapsed();
            return Err(Stop::OutOfMemory);
        }
        // オブジェクトがマークされなかった表の項目を外す。
        let mut freed = 0u64;
        let skip = *unboxed;
        let mut swept = 0u64;
        let mut work = m.work;
        for (ti, g) in tables.iter_mut().enumerate() {
            swept += g.state.len() as u64;
            for i in 0..g.state.len() {
                match g.state[i] {
                    MARKED => g.state[i] = LIVE,
                    LIVE if skip != Some((table_of(ti), i)) => {
                        match ti {
                            0 => handlers[i] = HandlerRec::dead(),
                            1 => conts[i] = Cont::default(),
                            _ => saved[i] = Pending::None,
                        }
                        g.release(i);
                        freed += 1;
                    }
                    _ => {}
                }
            }
        }
        heap.set_root_words(m.frames.saturating_mul(FRAME_TRIGGER_WORDS));
        swept += heap.sweep().scanned;
        // スイープは 1 項目あたりの処理が軽いので、処理量を 8 分の 1 に数える。
        work += swept / 8;
        gc.last_work = work;
        gc.allocs_since = 0;
        let mut x = gc.stress_rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        gc.stress_rng = x;
        // 係数は 32/64 から 95/64 までで、平均でほぼ 1 倍である。
        gc.stress_due = work
            .saturating_mul(32 + x % 64)
            .div_ceil(STRESS_RATIO * 64)
            .max(1);
        gc.freed_entries += freed;
        gc.collections += 1;
        gc.aborted = false;
        gc.time += t0.elapsed();
        Ok(())
    }

    /// GC の統計の 1 行を返す。`KELEUT_GC_STATS` を設定したときに標準エラーに書く行である。
    pub fn gc_stats(&self) -> String {
        format!(
            "keleut: gc: 回収 {} 回、{:.3} 秒、外した表の項目 {}、持っている語 {}、表の項目 {}/{}/{}、試した確保の場所 {}、打ち切った回収 {}\n",
            self.gc.collections,
            self.gc.time.as_secs_f64(),
            self.gc.freed_entries,
            self.heap.held_words(),
            self.gc.tables[0].live(),
            self.gc.tables[1].live(),
            self.gc.tables[2].live(),
            self.gc.tested.len(),
            self.gc.aborts,
        )
    }
}

/// [`GcState::tables`] の添字 `i` に対応する表の種類を返す。
fn table_of(i: usize) -> Table {
    match i {
        0 => Table::Handler,
        1 => Table::Cont,
        _ => Table::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 表の添字を使い回す() {
        let mut t = TableGc::default();
        assert_eq!(t.next_index(0, 0), Ok(0));
        assert_eq!(t.next_index(1, 0), Ok(1));
        t.release(0);
        assert_eq!(t.live(), 1);
        assert_eq!(t.next_index(2, 0), Ok(0));
        assert_eq!(t.next_index(2, 0), Ok(2));
        assert!(t.free.capacity() >= 3);
    }
}
