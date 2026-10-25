//! 回収(計画 261024-1 の 6.5 節、D526)。動かさないマークスイープである。
//!
//! 回収は確保の中でだけ起こる([`Rt::alloc`])。
//! 確保の関数を呼ぶ前に、後段は今の最上段のフレームの位置と、そのフレームが止まっている点を
//! [`GcState::at_safepoint`] か [`GcState::at_frame`] で置く(`doc/lir.md` の 1.8 節)。
//! 最上段の安全点は見出しに書かない。
//! 見出しの点は、確保に失敗して巻き戻すときに呼び出し元の行として読まれうるからである
//! (継続を作れずに区間の最上段へ移ったとき、ハンドラのフレームは本体の呼び出しの行で誤りを受け取る)。
//!
//! 根は次のものである。
//!
//! | 根 | 走査するもの |
//! | --- | --- |
//! | 値スタック | [`GcState::top`] から呼び出し元の鎖をたどったフレームと、ネイティブの入口([`GcState::enter_native`])ごとに [`GcState::outer`] に積んだ外側の最上段から同じくたどったフレーム。状態語が `YIELD` なら、その `perform` のフレームから今の最上段の手前まで。各フレームは見出しの点の集合の `any` のスロットだけ |
//! | `Root`([`crate::heap::Roots`]) | 積んだすべての語 |
//! | 大域とインスタンス | 大域の値、`inst_set_value` が覚えた値、文字列の定数 |
//! | エフェクト | ハンドラの連鎖の先頭、状態語の積載値、表に入れたがまだ添字の箱の無い項目 |
//!
//! ハンドラ記録、継続、退避した状態語の表は根ではない。
//! 添字の箱に印を付けたときに項目の語を走査し、継続の中のフレームの写しも値スタックのフレームと同じ規則で走査する。
//! 印付けの後に、箱に印の付かなかった項目を表から外す。
//!
//! 印付けは明示の印付けの列で反復し、ネイティブのスタックを使わない。
//! 列を広げられないとき(システムの確保の失敗)は、溢れの印を立てて列に積むのをやめ、
//! 列が空になったら、ヒープの印の付いた箱をすべて走査し直す(溢れが無くなるまで繰り返す)。
//!
//! ## フレームの点
//!
//! 見出しの 0 番の語の上位 32 ビットは、フレームが止まっている点である。
//! 最上段のフレームが確保の命令で止まっているときは、見出しの代わりに [`GcState::top_point`] を使う。
//!
//! | 点 | 根のスロット |
//! | --- | --- |
//! | [`SAFEPOINT`] の印と安全点の番号 | 安全点の集合(LIR の [`keleut_lir::Frame::safepoints`] の番号) |
//! | 再開点の行の番号(LIR の再開点の数より小さい) | 再開点の集合。`perform` の行なら `perform` の被演算子も |
//! | それ以上の行の番号 | 末尾の形の行(関数のブロックの順で 0 から数えて `k` 番目の、終わりが `tail_call_value` か `tail_call_method` のブロック。行の番号は再開点の数 + `k`)。集合は空 |
//!
//! 印 [`PHASE`] が付いていれば、その行の呼び出しの命令(末尾の形の行ならブロックの終わり)の被演算子も根にする。
//! 印 [`YIELDED`] は取り除いて読む(`perform` の行なので被演算子は根になる)。
//! `$rt_handle` のフレームは、どの点でも `any` の区画の 0 番のスロット(ハンドラ記録)を根にする。

use std::collections::HashSet;
use std::time::{Duration, Instant};

use keleut_lir::{FuncId, FxOp, Inst, Program, Reg, Slot, Term};

use crate::Rt;
use crate::effect::{Cont, HandlerRec, Pending};
use crate::error::Stop;
use crate::heap::{MARK, Roots};
use crate::stack::{HEADER_WORDS, NATIVE_CALLER, ValueStack};
use crate::value::{Kind, Word};

/// 再開点の番号に付けて、実装の関数を評価している途中であることを表す印(`doc/lir.md` の 1.8 節)。
pub const PHASE: u32 = 1 << 31;

/// `perform` の再開点の番号に付けて、そのフレームが G の節に取り出されたこと(継続の最上段)を表す印。
pub const YIELDED: u32 = 1 << 30;

/// 見出しの点が安全点(確保する命令)であることを表す印。下位のビットは安全点の番号である。
pub const SAFEPOINT: u32 = 1 << 29;

/// 見出しの点を使うことを表す [`GcState::top_point`] の値。
pub const NO_POINT: u32 = u32::MAX;

/// 点の番号を取り出す覆い。
const POINT_MASK: u32 = SAFEPOINT - 1;

/// `--gc-stress` の予算。前の回収の手間が `W` なら、確保の回数がおよそ `W / STRESS_RATIO` に届くまで回収しない。
///
/// `--gc-stress` は次の 2 つの規則で回収する([`Rt::alloc`])。
///
/// 1. 確保の場所(最上段のフレームの関数と点の組。最上段のフレームが無ければ 1 つの場所)を初めて通る確保では、必ず回収する。
///    実行したどの場所でも、少なくとも 1 回はその場所の根で回収を試す。
///    場所の数はプログラムの大きさで決まるので、この規則による回収の手間の合計は場所の数とスタックの深さの積で抑えられる。
///    深い再帰の場所も、ふつうはスタックがまだ浅いうちに初めて通る。
/// 2. それ以外の確保では、前の回収の後の確保の回数が間隔に届いたときに回収する。
///    間隔は `W / STRESS_RATIO` に 2 分の 1 倍から 2 分の 3 倍の擬似乱数の係数を掛けて切り上げたもので、1 以上である。
///    係数は固定の種から作るので、同じプログラムでは同じ時機に回収する。
///    係数を揺らすのは、ループの 1 回あたりの確保の回数が間隔の倍数のとき、同じ場所ばかりで回収するのを避けるためである。
///
/// 前の回収の手間には、大域の値、文字列の定数、インスタンスの値の印付けと、ページの掃除の分がいつも入る。
/// そのため、小さなプログラムでも `W` は `STRESS_RATIO` を超え、規則 2 だけでは確保のたびには回収しない
/// (コーパスでは平均で確保 2.5 回に 1 回ほどだった)。
/// 深いスタックや大きなヒープで確保のたびに回収すると、手間がスタックの深さと確保の回数の積になり、
/// 10 万段の再帰の試験が終わらない。
/// 規則 2 で、回収の手間の合計は確保の回数の `STRESS_RATIO` 倍ほどに収まる。
///
/// 規則 1 が保証するのは、最上段のフレームの場所だけである。
/// 呼び出し元のフレームが止まっている再開点の根は、その再開点で止まっている間に回収が起こったときにだけ試される。
/// 規則 2 の揺らぎでそうなる見込みは高いが、保証はしない。
pub const STRESS_RATIO: u64 = 64;

/// `--gc-stress` の間隔の擬似乱数の種。
const STRESS_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// 最上段のフレームが無いときの確保の場所。
const NATIVE_SITE: u64 = u64::MAX;

/// 走査したフレーム 1 枚あたり、次の回収までの間隔に足す語の数。
///
/// 回収のたびにスタックのすべてのフレームを走査し直すので、深いスタックでは 1 回の回収の手間がフレームの数に比例する。
/// 間隔を生き残った量だけで決めると、回収の手間の合計がスタックの深さと確保の量の積になる。
/// フレームの数に比例して間隔を広げ、手間の合計を確保の量に比例させる(フレーム 1 枚の走査の手間は、箱の数個の印付けほどである)。
pub const FRAME_TRIGGER_WORDS: u64 = 4;

/// 表の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// ハンドラ記録。
    Handler = 0,
    /// 継続。
    Cont = 1,
    /// 退避した状態語。
    Pending = 2,
}

/// 表の項目の状態。
const LIVE: u8 = 0;
const MARKED: u8 = 1;
const FREE: u8 = 2;

/// 表の回収の状態。
#[derive(Default)]
pub struct TableGc {
    /// 項目ごとの状態(生きている、印付け済み、空き)。
    state: Vec<u8>,
    /// 空いた添字。容量は常に項目の数以上にしておく(回収の中で確保しない)。
    free: Vec<usize>,
}

impl TableGc {
    /// 次に使う添字を決める。使い回せる添字があればそれを、無ければ `len`(新しい項目)を返す。
    /// 新しい項目の分の記録の領域を、確保の失敗を返す形で先に取る。
    pub fn next_index(&mut self, len: usize) -> Result<usize, Stop> {
        if let Some(i) = self.free.pop() {
            self.state[i] = LIVE;
            return Ok(i);
        }
        debug_assert_eq!(self.state.len(), len);
        self.state.try_reserve(1).map_err(|_| Stop::OutOfMemory)?;
        let want = len + 1;
        if self.free.capacity() < want {
            self.free
                .try_reserve(want - self.free.len())
                .map_err(|_| Stop::OutOfMemory)?;
        }
        self.state.push(LIVE);
        Ok(len)
    }

    /// 添字 `i` を空きにする。
    pub fn release(&mut self, i: usize) {
        self.state[i] = FREE;
        self.free.push(i);
    }

    /// 状態を持つ項目の数(表の長さと同じ)。
    pub fn state_len(&self) -> usize {
        self.state.len()
    }

    /// 使っている項目の数。
    pub fn live(&self) -> usize {
        self.state.len() - self.free.len()
    }
}

/// 関数ごとの根のスロットの覚え書き。
struct FuncRoots {
    /// LIR の再開点の数。
    nrp: usize,
    /// 末尾の形の行のブロック(ブロックの順)。
    tails: Vec<u32>,
    /// 行ごとの根のスロット(`any` の区画の番号)。
    rows: Vec<Option<Box<[u32]>>>,
    /// 行ごとの、印 PHASE のときに足すスロット。
    phase: Vec<Option<Box<[u32]>>>,
    /// `$rt_handle` か。
    rt_handle: bool,
}

/// 回収の状態。
pub struct GcState {
    /// 今の最上段のフレームの論理的な位置(無ければ [`NATIVE_CALLER`])。
    pub top: u64,
    /// 最上段のフレームが止まっている安全点(`SAFEPOINT` の印と番号)。見出しの点を使うなら [`NO_POINT`]。
    pub top_point: u32,
    /// 入れ子の入口ごとの、外側の最上段のフレームの位置。
    pub outer: Vec<u64>,
    /// `--gc-stress` か(回収の時機は [`STRESS_RATIO`] の規則に従う)。
    pub stress: bool,
    /// 前の回収の手間(走査したフレーム、印を付けた箱、掃除で見た区画と表の項目の数)。
    pub last_work: u64,
    /// 前の回収の後の確保の回数。
    pub allocs_since: u64,
    /// `--gc-stress` で、次に回収する確保の回数([`STRESS_RATIO`] の規則 2)。
    pub stress_due: u64,
    /// `--gc-stress` の間隔の係数を作る擬似乱数の状態(xorshift)。
    stress_rng: u64,
    /// `--gc-stress` で回収を試した確保の場所(上位 32 ビットが関数の番号、下位 32 ビットが点)。
    tested: HashSet<u64>,
    /// 回収の回数。
    pub collections: u64,
    /// 回収にかかった時間の合計。
    pub time: Duration,
    /// 回収で外した表の項目の数の合計。
    pub freed_entries: u64,
    /// 表に入れたが、まだ添字の箱の無い項目。
    pub(crate) unboxed: Option<(Table, usize)>,
    /// 表ごとの状態。
    pub(crate) tables: [TableGc; 3],
    mark: Vec<u64>,
    overflow: bool,
    funcs: Vec<Option<FuncRoots>>,
    tmp: Vec<u32>,
}

impl GcState {
    /// 最上段のフレーム `pos` が、安全点 `sp`(LIR の安全点の番号)の確保をする。
    #[inline]
    pub fn at_safepoint(&mut self, pos: u64, sp: u32) {
        self.top = pos;
        self.top_point = sp | SAFEPOINT;
    }

    /// 最上段のフレーム `pos` が、見出しの点(再開点)で止まったまま実行時が確保する。
    #[inline]
    pub fn at_frame(&mut self, pos: u64) {
        self.top = pos;
        self.top_point = NO_POINT;
    }

    /// Rust(ネイティブの呼び出し元)から管理されたフレームの鎖に入る。
    ///
    /// 外側の最上段の位置を [`GcState::outer`] に積み、[`GcState::leave_native`] に渡す値を返す。
    /// 最下段の見出しの 3 番の語が [`NATIVE_CALLER`] になる鎖を始める入口は、どの後段でもこれを使う。
    /// 入れ子のインタプリタの起動、LLVM の後段の `enter_tailcc` と初期化の駆動、
    /// `interp_stub` から入る `interp_run` がこれにあたる。
    /// 使わないと、回収は入口より下のフレームをたどらない。
    /// 管理されたフレームどうしのふつうの呼び出し(見出しの 3 番の語に呼び出し元の位置を書く)は使わない。
    #[must_use]
    pub fn enter_native(&mut self) -> (u64, u32) {
        let saved = (self.top, self.top_point);
        self.outer.push(saved.0);
        saved
    }

    /// [`GcState::enter_native`] で入った鎖から戻る。積んだ位置を外し、最上段の位置と点を元に戻す。
    pub fn leave_native(&mut self, saved: (u64, u32)) {
        self.outer.pop();
        self.top = saved.0;
        self.top_point = saved.1;
    }

    /// 関数の数 `nfuncs` のプログラムの状態。
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
        }
    }
}

/// 印付けの列。
struct Marker<'a> {
    stack: &'a mut Vec<u64>,
    overflow: &'a mut bool,
    work: u64,
    /// 走査したフレーム(値スタックと継続の写し)の数。
    frames: u64,
}

impl Marker<'_> {
    /// 語がポインタなら箱に印を付け、初めてなら列に積む。
    #[inline]
    fn word(&mut self, w: u64) {
        if w & 7 != 0 || w == 0 {
            return;
        }
        let p = w as *mut u64;
        // SAFETY: 根と箱の値の欄のポインタの語は、生きている箱を指す(根の規律)。
        unsafe {
            let h = *p;
            if h & MARK != 0 {
                return;
            }
            *p = h | MARK;
        }
        self.work += 1;
        if self.stack.len() == self.stack.capacity() && self.stack.try_reserve(1).is_err() {
            *self.overflow = true;
            return;
        }
        self.stack.push(w);
    }
}

fn any_slots(prog: &Program, f: FuncId, regs: &[Reg], out: &mut Vec<u32>) {
    let fr = prog.funcs[f.index()]
        .frame
        .as_ref()
        .expect("パス 12 を済ませた関数");
    for r in regs {
        if let Slot::Any(n) = fr.slot[r.index()] {
            out.push(n);
        }
    }
}

impl FuncRoots {
    fn new(prog: &Program, f: FuncId) -> FuncRoots {
        let func = &prog.funcs[f.index()];
        let fr = func.frame.as_ref().expect("パス 12 を済ませた関数");
        let tails: Vec<u32> = func
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                matches!(
                    b.term,
                    Term::TailCallValue { .. } | Term::TailCallMethod { .. }
                )
            })
            .map(|(i, _)| i as u32)
            .collect();
        let nrp = fr.resume_points.len();
        let n = nrp + tails.len();
        FuncRoots {
            nrp,
            tails,
            rows: (0..n).map(|_| None).collect(),
            phase: (0..n).map(|_| None).collect(),
            rt_handle: prog.rt_handle == Some(f),
        }
    }

    fn row(&mut self, prog: &Program, f: FuncId, row: usize) -> &[u32] {
        if self.rows[row].is_none() {
            let mut v = Vec::new();
            if row < self.nrp {
                let func = &prog.funcs[f.index()];
                let fr = func.frame.as_ref().expect("パス 12 を済ませた関数");
                any_slots(prog, f, &fr.resume_points.live(row), &mut v);
                let pt = fr.resume_points.point(row);
                let inst = &func.blocks[pt.block.index()].insts[pt.index as usize];
                if matches!(
                    inst,
                    Inst::Fx {
                        op: FxOp::Perform { .. },
                        ..
                    }
                ) {
                    any_slots(prog, f, &inst.uses(), &mut v);
                }
            }
            if self.rt_handle {
                v.push(0);
            }
            self.rows[row] = Some(v.into_boxed_slice());
        }
        self.rows[row].as_deref().unwrap_or(&[])
    }

    fn phase(&mut self, prog: &Program, f: FuncId, row: usize) -> &[u32] {
        if self.phase[row].is_none() {
            let mut v = Vec::new();
            let func = &prog.funcs[f.index()];
            if row < self.nrp {
                let fr = func.frame.as_ref().expect("パス 12 を済ませた関数");
                let pt = fr.resume_points.point(row);
                let inst = &func.blocks[pt.block.index()].insts[pt.index as usize];
                any_slots(prog, f, &inst.uses(), &mut v);
            } else {
                let b = self.tails[row - self.nrp] as usize;
                any_slots(prog, f, &func.blocks[b].term.uses(), &mut v);
            }
            self.phase[row] = Some(v.into_boxed_slice());
        }
        self.phase[row].as_deref().unwrap_or(&[])
    }
}

/// フレームの走査に要るもの。
struct Frames<'a, 'p> {
    prog: &'p Program,
    funcs: &'a mut Vec<Option<FuncRoots>>,
    tmp: &'a mut Vec<u32>,
}

impl Frames<'_, '_> {
    /// 見出しが `p` のフレームの根のスロットに印を付ける。
    /// `point` が [`NO_POINT`] でなければ、見出しの点の代わりにそれを使う(最上段の安全点)。
    fn scan(&mut self, m: &mut Marker<'_>, p: *const u64, point: u32) {
        // SAFETY: p はフレームの見出しを指し、フレームは見出しの 2 番の語の大きさを持つ。
        let h0 = unsafe { *p };
        let f = FuncId(h0 as u32);
        let pt = if point != NO_POINT {
            point
        } else {
            (h0 >> 32) as u32
        };
        let slot = |n: u32| -> u64 {
            // SAFETY: n は関数のフレームの any の区画のスロットである。
            unsafe { *p.add(HEADER_WORDS + n as usize) }
        };
        let fr = self.funcs[f.index()].get_or_insert_with(|| FuncRoots::new(self.prog, f));
        if pt & SAFEPOINT != 0 {
            let i = (pt & POINT_MASK) as usize;
            let frame = self.prog.funcs[f.index()]
                .frame
                .as_ref()
                .expect("パス 12 を済ませた関数");
            self.tmp.clear();
            any_slots(self.prog, f, &frame.safepoints.live(i), self.tmp);
            if fr.rt_handle {
                self.tmp.push(0);
            }
            for &n in self.tmp.iter() {
                m.word(slot(n));
            }
            return;
        }
        let row = (pt & !(PHASE | YIELDED)) as usize;
        for &n in fr.row(self.prog, f, row) {
            m.word(slot(n));
        }
        if pt & PHASE != 0 {
            for &n in fr.phase(self.prog, f, row) {
                m.word(slot(n));
            }
        }
    }

    /// 位置 `pos` の最上段から、ネイティブの呼び出し元か位置 `stop` のフレームの手前まで、呼び出し元の鎖をたどる。
    /// 最初のフレームには点 `point` を使う(見出しの点を使うなら [`NO_POINT`])。
    fn walk(
        &mut self,
        m: &mut Marker<'_>,
        stack: &ValueStack,
        mut pos: u64,
        stop: u64,
        mut point: u32,
    ) {
        while pos != NATIVE_CALLER && pos != stop {
            let p = stack.ptr_at_ro(pos);
            m.work += 1;
            m.frames += 1;
            self.scan(m, p, point);
            point = NO_POINT;
            // SAFETY: p はフレームの見出しを指す。
            pos = unsafe { *p.add(3) };
        }
    }

    /// 継続の中のフレームの写しを走査する。
    fn cont(&mut self, m: &mut Marker<'_>, c: &Cont) {
        let mut off = 0usize;
        for &sz in &c.sizes {
            m.frames += 1;
            self.scan(m, c.frames[off..].as_ptr(), NO_POINT);
            off += sz as usize;
        }
    }
}

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

/// 印付けの間に読む表。
struct Tables<'a> {
    handlers: &'a [HandlerRec],
    conts: &'a [Cont],
    saved: &'a [Pending],
    gc: &'a mut [TableGc; 3],
}

impl Tables<'_> {
    /// 項目 `i` を走査する(初めてのときだけ)。
    fn entry(&mut self, m: &mut Marker<'_>, fr: &mut Frames<'_, '_>, t: Table, i: usize) {
        let st = &mut self.gc[t as usize].state[i];
        if *st != LIVE {
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

    /// 箱 `p` の値の欄に印を付ける。
    fn children(&mut self, m: &mut Marker<'_>, fr: &mut Frames<'_, '_>, p: *mut u64) {
        // SAFETY: p は生きている箱を指し、配置は見出しの種類で決まる。
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
                // Int64、Float64、文字列、実装の無い extern の値は値の欄を持たない。
                _ => {}
            }
        }
    }
}

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
    /// `words` 語の箱の領域を確保する。回収の時機なら(`--gc-stress` なら [`STRESS_RATIO`] の規則に従って)先に回収する。
    ///
    /// 取れなければ回収してから試し直し、それでも取れなければメモリの不足にする。
    /// 呼ぶ前に、後段は [`GcState::top`] と最上段のフレームの点を正しくしておく(このモジュールの冒頭)。
    pub fn alloc(&mut self, words: usize) -> Result<*mut u64, Stop> {
        let mut collected = false;
        let stress = self.gc.stress && self.stress_now();
        if stress || self.heap.should_collect() {
            self.collect();
            collected = true;
        }
        if let Ok(p) = self.heap.try_alloc(words) {
            return Ok(p);
        }
        if !collected {
            self.collect();
            if let Ok(p) = self.heap.try_alloc(words) {
                return Ok(p);
            }
        }
        Err(self.heap.exhausted(words).into())
    }

    /// `--gc-stress` で、この確保の前に回収するかを決める([`STRESS_RATIO`] の規則)。
    fn stress_now(&mut self) -> bool {
        let g = &mut self.gc;
        g.allocs_since += 1;
        let site = if g.top == NATIVE_CALLER {
            NATIVE_SITE
        } else {
            // SAFETY: 確保の前に、後段は top を最上段のフレームの見出しにしておく(このモジュールの冒頭)。
            let h = unsafe { *self.stack.ptr_at_ro(g.top) };
            let point = if g.top_point == NO_POINT {
                (h >> 32) as u32
            } else {
                g.top_point
            };
            ((h & 0xffff_ffff) << 32) | point as u64
        };
        if !g.tested.contains(&site) {
            // 覚える領域を取れなくても回収はする(覚えなければ次も回収する)。
            if g.tested.try_reserve(1).is_ok() {
                g.tested.insert(site);
            }
            return true;
        }
        g.allocs_since >= g.stress_due
    }

    /// 回収する。
    pub fn collect(&mut self) {
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
            ..
        } = gc;
        *overflow = false;
        mark.clear();
        let mut m = Marker {
            stack: mark,
            overflow,
            work: 0,
            frames: 0,
        };
        let mut fr = Frames { prog, funcs, tmp };
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
            // まだ継続に写していない、perform のフレームから今の最上段の上までのフレーム。
            fr.walk(&mut m, stack, *y, *top, NO_POINT);
        }
        fr.walk(&mut m, stack, *top, NATIVE_CALLER, *top_point);
        for &o in outer.iter().rev() {
            fr.walk(&mut m, stack, o, NATIVE_CALLER, NO_POINT);
        }
        loop {
            while let Some(w) = m.stack.pop() {
                tabs.children(&mut m, &mut fr, w as *mut u64);
            }
            if !*m.overflow {
                break;
            }
            // 溢れた。印の付いた箱をすべて走査し直す(子に印が付いていれば何もしない)。
            *m.overflow = false;
            heap.for_each_object(|p| {
                // SAFETY: p は生きている箱の見出しを指す。
                if unsafe { *p } & MARK != 0 {
                    tabs.children(&mut m, &mut fr, p);
                    while let Some(w) = m.stack.pop() {
                        tabs.children(&mut m, &mut fr, w as *mut u64);
                    }
                }
            });
        }
        // 箱に印の付かなかった表の項目を外す。
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
        // 掃除は 1 項目あたりが軽いので、手間を 8 分の 1 に数える。
        work += swept / 8;
        gc.last_work = work;
        gc.allocs_since = 0;
        let mut x = gc.stress_rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        gc.stress_rng = x;
        // 係数は 32/64 から 95/64 まで(平均でほぼ 1 倍)。
        gc.stress_due = work
            .saturating_mul(32 + x % 64)
            .div_ceil(STRESS_RATIO * 64)
            .max(1);
        gc.freed_entries += freed;
        gc.collections += 1;
        gc.time += t0.elapsed();
    }

    /// 回収の統計の 1 行(`KELEUT_GC_STATS` を与えたときに標準エラーに書く)。
    pub fn gc_stats(&self) -> String {
        format!(
            "keleut: gc: 回収 {} 回、{:.3} 秒、外した表の項目 {}、持っている語 {}、表の項目 {}/{}/{}、試した確保の場所 {}\n",
            self.gc.collections,
            self.gc.time.as_secs_f64(),
            self.gc.freed_entries,
            self.heap.held_words(),
            self.gc.tables[0].live(),
            self.gc.tables[1].live(),
            self.gc.tables[2].live(),
            self.gc.tested.len(),
        )
    }
}

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
        assert_eq!(t.next_index(0), Ok(0));
        assert_eq!(t.next_index(1), Ok(1));
        t.release(0);
        assert_eq!(t.live(), 1);
        assert_eq!(t.next_index(2), Ok(0));
        assert_eq!(t.next_index(2), Ok(2));
        assert!(t.free.capacity() >= 3);
    }
}
