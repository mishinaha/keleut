//! WasmGC バックエンド。
//!
//! LIR のプログラム(パス 12 を済ませたもの)から、WasmGC のモジュール `program.wasm` を作る。
//! モジュールは、次の 2 つを import する。
//!
//! - rtcore をビルドした `rt.wasm`。線形メモリ、`show(Float64)`、libm、シェイプ表を持つ。
//! - Node 側のランタイム `host/keleut-host.mjs`。起動、入出力、診断メッセージの作成を受け持つ。
//!
//! | モジュール | 内容 |
//! | --- | --- |
//! | [`emit`] | モジュールの組み立て(型の rec グループ、関数、グローバル、データ、名前セクション) |
//! | `rt` | ランタイム関数(trap することがある命令を置く補助関数 `safe.*`、エフェクト、メソッドの選択) |
//! | `func` | LIR の関数のコード生成(ブロックの並べ方、状態ワードの伝播、大きなフレーム方式) |
//! | [`meta`] | ホストに渡す表(診断メッセージのテンプレートと名前) |
//! | [`check`] | wasmparser による検証と trap の検査 |
//! | [`launch`] | `keleut run --backend wasm` と `keleut build` の出力の書き出しと Node の起動 |
//!
//! LIR の命令の意味と、エフェクトの規約は `rustkeleut/doc/lir.md`(以下 `doc/lir.md`)にある。
//! 値の表現と制御の構造は、`rt` と `func` の冒頭に書く。
//! 分類 G の操作節(G の節)の継続は、フレームの記録の並びとして持つ(`func` の冒頭の「継続のキャプチャと再開」)。

pub mod check;
pub mod emit;
mod func;
pub mod launch;
pub mod meta;
mod rt;

use std::collections::{BTreeSet, HashMap, VecDeque};

use keleut_lir::{ClauseClass, FuncId, FuncKind, FxOp, GlobalKind, Inst, Program};
use wasm_encoder::{ConstExpr, ExportKind, Instruction, ValType};

use emit::{EQ, I32, I64, Mb, rv};
use meta::Templates;

/// 論理的な深さの 1 段に必要と見積もる、スタックのバイト数 `B_wasm`。
///
/// 実測に基づいて決めた値である。
/// `B_prog` の計算([`b_prog`])、スタックサイズを切り詰めるときの下限([`stack_size_kib`])、Wasm の関数 1 段に許す見積もりバイト数の上限に使う。
pub const B_WASM: u64 = 1536;

/// Wasm の関数 1 段のバイト数を、`8 × K + 24 × P + 64` と見積もって返す。
///
/// `K` はローカルの個数(`locals`)、`P` は引数の個数(`params`)である。
pub fn frame_estimate(locals: u64, params: u64) -> u64 {
    8 * locals + 24 * params + 64
}

/// 呼び出しで Wasm の引数として直接渡す、LIR の引数の個数の上限。
///
/// 引数が `P_DIRECT` 個を超える呼び出しは、先頭の `P_DIRECT` 個を直接渡し、残りを 1 つの配列(`$anyarr`)にまとめて渡す。
/// 呼び出し先は入口で配列を読み、残りの引数を通常のレジスタに置く。
/// こうして、引数の多い関数でも 1 段が [`B_WASM`] に収まるようにする。
/// ローカルを除いた見積もりは、最大で `24 × (32 + 3) + 64 = 904` バイトである。
pub const P_DIRECT: u32 = 32;

/// 1 つの呼び出しで配列にまとめて渡せる引数の個数の上限。
///
/// まとめる引数の個数は `array.new_fixed` の要素数になる。
/// V8 が許す要素数の上限は 10,000 である。
pub const PACKED_MAX: u32 = 1000;

/// LIR の引数が `n` 個の呼び出しで、Wasm の引数として渡す値の個数を返す。
pub fn wire_arity(n: u32) -> u32 {
    if n > P_DIRECT { P_DIRECT + 1 } else { n }
}

/// ハンドラのフレームになる関数 `$rt_handle` の 1 段に許す、見積もりバイト数の上限 `E_rth`。
///
/// 生成した `$rt_handle` の見積もりがこの値を超えたら、コード生成のエラーにする。
pub const E_RTH: u64 = 320;

/// `rt_resume` の補助関数の 1 段の見積もりバイト数 `E_rres`。
///
/// ローカル 5 個、引数 3 個として [`frame_estimate`] の式で計算した値である。
pub const E_RRES: u64 = 8 * 5 + 24 * 3 + 64;

/// プログラムの 1 段に必要なバイト数 `B_prog` を返す。
///
/// `B_prog = B_wasm + k × (B_wasm − E_rres) + R(r)` である。
/// `R(0) = 0` で、`r ≥ 1` なら `R(r) = r × B_wasm − E_rth` である。
/// `k` と `r` は [`clause_path`] の値で、同じ深さに重なる節の 1 本の経路に沿って数える。
/// 以下では、分類 N の操作節を「N の節」と書く。
///
/// `cancel` 節の中にハンドラ式があると、論理的な深さの 1 段に、ハンドラ式を書いた関数と `$rt_handle` に加えて、`cancel` 節と内側の `$rt_handle` が重なる。
/// 重なる 1 組(`cancel` 節と内側の `$rt_handle`)の上限は `(B_wasm − E_rth − E_rres) + E_rth = B_wasm − E_rres` である。
/// `rt_resume` は 1 つの深さに 1 つしか重ならないので、組ごとには足さない。
///
/// `return` 節と N の節は、`$rt_handle` が末尾呼び出しで呼んで自分のフレームと置き換えるので、ハンドラ式を書いた関数と同じ深さに重なる。
/// これらの節の連鎖の長さが `r` なら、深さ `h` には、ハンドラ式を書いた関数(上限 `B_wasm − E_rth − E_rres`)、
/// `r` 個の節(どれも上限 `B_wasm`)、`rt_resume`(`E_rres`)が重なる。
/// 節を呼んだ `$rt_handle` のフレームは、末尾呼び出しでその節のフレームに置き換わっているので、数えない。
/// 最後の節が `$rt_handle` を非末尾呼び出しで呼ぶ場合は、その節の上限が `B_wasm − E_rth − E_rres` になるので(`func` の `budget_of`)、節とその `$rt_handle` を合わせても、節 1 個に割り当てた `B_wasm` に収まる。
/// 深さ `h` に重なるものの和は `(B_wasm − E_rth − E_rres) + r × B_wasm + E_rres = B_wasm + r × B_wasm − E_rth` で、`R(r)` はこの和から `B_wasm` を引いたものである。
/// 連鎖の 2 つ目以降の節は、前の節が非末尾呼び出しで呼んだ `$rt_handle` の位置に重なる。
/// その `$rt_handle` は上限に入っていないので、節ごとに `B_wasm` を足す。
///
/// `cancel` 節と、`return` 節や N の節が 1 本の経路に交互に並ぶときは、2 種類の重なりを足し合わせる。
/// 経路の 2 つ目以降の節は、前の節が非末尾呼び出しで呼んだ `$rt_handle` の上(`cancel` 節の場合)か、その `$rt_handle` の位置(`return` 節と N の節の場合)に重なる。
/// どちらの場合も前の節のフレームは残るので、節ごとのバイト数をすべて足す必要がある。
/// `k = 0` かつ `r = 0` なら `B_prog = B_wasm` である。
pub fn b_prog(k: u32, r: u32) -> u64 {
    let rr = if r == 0 {
        0
    } else {
        u64::from(r).saturating_mul(B_WASM) - E_RTH
    };
    B_WASM
        .saturating_add(u64::from(k).saturating_mul(B_WASM - E_RRES))
        .saturating_add(rr)
}

/// 関数ごとに、直接呼び出しをたどると深い再帰に入ることがあるかどうかを求める。
///
/// 直接呼び出しには末尾呼び出しも含める。
/// 関数 `f` から直接呼び出しをたどって到達できる関数(`f` 自身も含む)の中に、次のどちらかがあれば、`f` の結果は真である。
///
/// - 関数値の呼び出し、メソッドの呼び出し、`perform` のどれかを持つ関数。
///   節と本体を関数値として呼ぶ `$rt_handle` も、これに含める。
/// - 直接呼び出しの循環に入っている関数。
///
/// どちらもなければ、`f` から始まる呼び出しの入れ子の深さは有限で、`f` の上に論理的な段が際限なく重なることはない。
/// 直接呼び出しのグラフの強連結成分を Tarjan のアルゴリズムで求め、呼び出し先の成分から順に結果を決める。
fn reaches_recursion(prog: &Program) -> Vec<bool> {
    let n = prog.funcs.len();
    // 関数ごとに、(関数値やメソッドの呼び出しか perform を持てば真、直接呼び出す関数の並び)の組を作る。
    let info: Vec<(bool, Vec<usize>)> = prog
        .funcs
        .iter()
        .map(|f| {
            let mut open = f.kind == FuncKind::Handler;
            let mut direct = Vec::new();
            for b in &f.blocks {
                for i in &b.insts {
                    match i {
                        Inst::CallDirect { func, .. } => direct.push(func.index()),
                        Inst::CallValue { .. }
                        | Inst::CallMethod { .. }
                        | Inst::Fx {
                            op: FxOp::Perform { .. },
                            ..
                        } => open = true,
                        _ => {}
                    }
                }
                match &b.term {
                    keleut_lir::Term::TailCallDirect { func, .. } => direct.push(func.index()),
                    keleut_lir::Term::TailCallValue { .. }
                    | keleut_lir::Term::TailCallMethod { .. } => open = true,
                    _ => {}
                }
            }
            (open, direct)
        })
        .collect();
    // Tarjan のアルゴリズムを、再帰を使わずに明示的なスタックで行う。
    // 呼び出しの連鎖が長くても、Rust のスタックを使い切らないようにするためである。
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on = vec![false; n];
    let mut st: Vec<usize> = Vec::new();
    let mut reach = vec![false; n];
    let mut next = 0usize;
    for s in 0..n {
        if index[s] != usize::MAX {
            continue;
        }
        let mut work = vec![(s, 0usize)];
        index[s] = next;
        low[s] = next;
        next += 1;
        st.push(s);
        on[s] = true;
        while let Some(&mut (v, ref mut pos)) = work.last_mut() {
            if let Some(&w) = info[v].1.get(*pos) {
                *pos += 1;
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    st.push(w);
                    on[w] = true;
                    work.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(u, _)) = work.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] != index[v] {
                continue;
            }
            // v を根とする強連結成分を取り出す。
            // 呼び出し先の成分の結果は、先に決まっている。
            let mut comp = Vec::new();
            loop {
                let w = st.pop().unwrap();
                on[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            let cyclic = comp.len() > 1 || info[v].1.contains(&v);
            let r = cyclic
                || comp.iter().any(|&w| {
                    info[w].0 || info[w].1.iter().any(|&g| !comp.contains(&g) && reach[g])
                });
            for &w in &comp {
                reach[w] = r;
            }
        }
    }
    reach
}

/// 同じ深さに重なる節の経路について、`k` と `r` を返す。
///
/// `k` はハンドラ式を持つ `cancel` 節の数、`r` は深い再帰に入ることがある `return` 節と分類 N の操作節(N の節)の数である。
/// `k` と `r` は、`B_prog` の計算に使う([`b_prog`])。
///
/// `k` と `r` は LIR から求める。
/// 対象の節は、種類 `cancelclause` の関数、種類 `retclause` の関数、N の節の関数の 3 種類である。
/// 節の関数値は、ハンドラ式を書いた関数の中で `closure_new` が作り、`handler_new` に渡す(`doc/lir.md` の 16.2 節)。
/// そのため、節 `f` が `closure_new` で作る節 `g` は、`f` の中に直接書いたハンドラ式の節である。
/// 節 `f` から、`f` が作る節 `g` へ辺を引き、辺をたどる経路ごとに、1 つの深さに重なるバイト数を見積もる。
///
/// 3 種類の節は、どれもハンドラ式を書いた関数と同じ深さに重なる。
/// `cancel` 節は `$rt_handle` と同じ深さで実行され、その中のハンドラ式の `$rt_handle` を非末尾呼び出しで呼ぶ(IRSpec 5.3 節)。
/// `return` 節と N の節は、`$rt_handle` が末尾呼び出しで呼んで自分のフレームと置き換える(`doc/lir.md` の 16.3 節)。
/// そのため、`cancel` 節と、`return` 節や N の節が交互に入れ子になった経路でも、経路の上の節はすべて同じ深さに重なる。
/// ハンドラ式の本体、無名関数や関数の定義(種類 `lambda` の関数)、分類 T と G の操作節は別の関数になり、深さを 1 つ増やして実行される。
/// そのため、それらの中に書いたハンドラ式の節へは、外側の節から辺を引かない。
/// そのような節は、外側の節の経路には入らず、そこから別の経路が始まる。
///
/// 経路に入れる節と、その重みは次のとおりである。
///
/// - `handler_new` を持つ `cancel` 節は、`k` に 1 を足す(重み `B_wasm − E_rres`)。
///   `handler_new` を持たない `cancel` 節は経路の終わりで、何も足さない。
/// - 直接呼び出しをたどると深い再帰に入ることがある `return` 節と N の節は、`r` に 1 を足す(重み `B_wasm`)。
///   経路にこの種類の節が 1 つでもあれば、経路全体の重みから `E_rth` を 1 度だけ引く。
///   深い再帰に入ることのない節(`echoln` のような葉の関数だけを呼ぶ節)は経路の終わりで、何も足さない。
/// - ただし、`return` 節がなく N の節が 1 つだけの経路では、その N の節を数えない(`r = 0`)。
///   N の節が 1 つだけ重なる場合は、実測した 1 段のバイト数が `B_wasm` に収まったからである。
///
/// 返すのは、重み([`b_prog`] の値)が最大の経路の `(k, r)` である。
/// `k` を `cancel` 節だけの連鎖で、`r` を `return` 節と N の節だけの連鎖で別々に数えて最大を求めると、`cancel` 節と `return` 節や N の節が交互に並ぶ連鎖を数え漏らすので、1 本の経路に沿って数える。
/// 節の入れ子はソース上の入れ子なので、辺は循環しない。
/// それでも念のため、探索の途中にある節へ戻る辺は、経路の終わりとして扱う。
pub fn clause_path(prog: &Program) -> (u32, u32) {
    let n = prog.funcs.len();
    let reach = reaches_recursion(prog);
    // N の節の関数なら真。
    // handler_new のオペランドは、本体、引き継ぐ継続、操作節の関数値(書いた順)、return 節、cancel 節の順に並ぶ。
    let mut never = vec![false; n];
    // handler_new を持つ関数なら真。
    let mut has_handler = vec![false; n];
    for (fi, f) in prog.funcs.iter().enumerate() {
        let mut made: HashMap<keleut_lir::Reg, FuncId> = HashMap::new();
        for i in f.blocks.iter().flat_map(|b| &b.insts) {
            match i {
                Inst::ClosureNew { dst, func, .. } => {
                    made.insert(*dst, *func);
                }
                Inst::Fx {
                    op: FxOp::HandlerNew { ops, .. },
                    args,
                    ..
                } => {
                    has_handler[fi] = true;
                    for (j, (_, class)) in ops.iter().enumerate() {
                        if *class == ClauseClass::N
                            && let Some(g) = args.get(2 + j).and_then(|a| made.get(a))
                        {
                            never[g.index()] = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let kind = |i: usize| prog.funcs[i].kind;
    let is_cancel = |i: usize| kind(i) == FuncKind::Kir(keleut_lir::FnKind::Cancelclause);
    let is_ret = |i: usize| kind(i) == FuncKind::Kir(keleut_lir::FnKind::Retclause);
    let in_set = |i: usize| is_cancel(i) || is_ret(i) || never[i];
    // 対象の節ごとに、その節が closure_new で作る対象の節の並びを求める。
    let kids: Vec<Vec<usize>> = prog
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if !in_set(i) {
                return Vec::new();
            }
            f.blocks
                .iter()
                .flat_map(|b| &b.insts)
                .filter_map(|x| match x {
                    Inst::ClosureNew { func, .. } if in_set(func.index()) => Some(func.index()),
                    _ => None,
                })
                .collect()
        })
        .collect();
    // 節から始まる経路のうち重みが最大のものを、経路の種類ごとに持つ。
    // p0 は return 節も N の節もない経路の k、p1 は N の節が 1 つだけで return 節のない経路の k、
    // pc は r を数える経路の (k, r) である。
    // None は、その種類の経路がないことを表す。
    #[derive(Clone, Copy, Default)]
    struct Best {
        p0: Option<u32>,
        p1: Option<u32>,
        pc: Option<(u32, u32)>,
    }
    let weight_c = |(k, r): (u32, u32)| b_prog(k, r);
    let better = |a: Option<(u32, u32)>, b: (u32, u32)| match a {
        Some(x) if weight_c(x) >= weight_c(b) => Some(x),
        _ => Some(b),
    };
    let leaf = Best {
        p0: Some(0),
        p1: None,
        pc: None,
    };
    let mut memo: Vec<Option<Best>> = vec![None; n];
    let mut open = vec![false; n];
    let mut best = (0u32, 0u32);
    for s in 0..n {
        if !in_set(s) {
            continue;
        }
        // 再帰を使わずに、明示的なスタックで深さ優先にたどる。
        // 節の入れ子が深くても、Rust のスタックを使い切らないようにするためである。
        let mut stack = vec![(s, false)];
        while let Some((v, done)) = stack.pop() {
            if memo[v].is_some() {
                continue;
            }
            let counts = if is_cancel(v) {
                has_handler[v]
            } else {
                reach[v]
            };
            if !counts {
                memo[v] = Some(leaf);
                continue;
            }
            if !done {
                if !open[v] {
                    open[v] = true;
                    stack.push((v, true));
                    for &g in &kids[v] {
                        if memo[g].is_none() && !open[g] {
                            stack.push((g, false));
                        }
                    }
                }
                continue;
            }
            // 子から始まる経路のうち、重みが最大のものを種類ごとに求める。
            // 探索の途中にある子は、経路の終わりとして扱う。
            // 経路はどの節でも終えられるので、初期値は leaf にする。
            let mut c = leaf;
            for &g in &kids[v] {
                let b = memo[g].unwrap_or(leaf);
                c.p0 = c.p0.max(b.p0);
                c.p1 = c.p1.max(b.p1);
                if let Some(x) = b.pc {
                    c.pc = better(c.pc, x);
                }
            }
            let mut me = Best::default();
            if is_cancel(v) {
                me.p0 = c.p0.map(|k| k + 1);
                me.p1 = c.p1.map(|k| k + 1);
                me.pc = c.pc.map(|(k, r)| (k + 1, r));
            } else {
                // 経路に 2 つ目の return 節か N の節が加わると、1 つだけだった N の節も数える。
                if let Some(k) = c.p1 {
                    me.pc = better(me.pc, (k, 2));
                }
                if let Some((k, r)) = c.pc {
                    me.pc = better(me.pc, (k, r.saturating_add(1)));
                }
                let k0 = c.p0.unwrap_or(0);
                if is_ret(v) {
                    me.pc = better(me.pc, (k0, 1));
                } else {
                    me.p1 = Some(k0);
                }
            }
            memo[v] = Some(me);
        }
        let b = memo[s].unwrap_or(leaf);
        for x in [b.p0.map(|k| (k, 0)), b.p1.map(|k| (k, 0)), b.pc]
            .into_iter()
            .flatten()
        {
            if weight_c(x) > weight_c(best) {
                best = x;
            }
        }
    }
    best
}

/// スタックサイズの上限 `STACK_CAP`。単位は KiB で、値は 16 GiB を表す。
///
/// `B_prog` は `k` と `r` に応じて大きくなるので、`k` や `r` の大きいプログラムのスタックを、この上限で切り詰める。
/// 切り詰めないと、ホストが `Worker` を作れず、再帰の浅いプログラムでも起動時に終了コード 70 で終了する。
/// 1 台のマシンでの実測では、Node 26.9.0 は `resourceLimits.stackSizeMb` が 16,448 と 32,000 の `Worker` を作れ、64,000 の `Worker` を作れなかった。
/// 既定の `L` では、`r = 0` なら `k` が 10 まで、`k = 0` なら `r` が 9 までのプログラムのスタックは切り詰めない。
/// `B_prog = B_wasm` のときのサイズ(`k = 0` かつ `r = 0` のプログラムのサイズ)より小さくは切り詰めない([`stack_size_kib`])。
/// ホストの `STACK_CAP_KIB` と同じ値である。
pub const STACK_CAP_KIB: u64 = 16 << 20;

/// 上限で切り詰める前のスタックサイズを、KiB 単位で返す。
///
/// 値は `(L + R) × B_prog + 32 MiB` で、`R = max(1024, L / 64)` は予備の深さである(IRSpec 11.5 節)。
pub fn stack_need_kib(max_depth: u64, b_prog: u64) -> u64 {
    let r = (max_depth / 64).max(1024);
    (max_depth.saturating_add(r))
        .saturating_mul(b_prog)
        .saturating_add(32 << 20)
        .div_ceil(1024)
}

/// `--stack-size` と `Worker` のスタックに使うサイズを、KiB 単位で返す。
///
/// [`stack_need_kib`] が [`STACK_CAP_KIB`] を超えるときは、`STACK_CAP_KIB` に切り詰める。
/// ただし、`B_prog = B_wasm` のときのサイズ(`k = 0` かつ `r = 0` のプログラムのサイズ)より小さくはしない。
/// `k = 0` かつ `r = 0` のプログラムの動作を、上限によって変えないためである。
/// そのようなプログラムが確保できないサイズのスタックを必要とするときは、上限がない場合と同じく、起動時に終了コード 70 で終了する。
pub fn stack_size_kib(max_depth: u64, b_prog: u64) -> u64 {
    let need = stack_need_kib(max_depth, b_prog);
    if need <= STACK_CAP_KIB {
        need
    } else {
        STACK_CAP_KIB.max(stack_need_kib(max_depth, B_WASM))
    }
}

/// V8 の `--stack-size` に渡せる最大の値(KiB)。
pub const STACK_SIZE_MAX_KIB: u64 = 2_097_151;

/// 状態ワードの種類。グローバル `$p_kind` に置く値である。
pub mod pk {
    pub const NONE: i32 = 0;
    pub const YIELD: i32 = 1;
    pub const DISCARD_VALUE: i32 = 2;
    pub const DISCARD_NEVER: i32 = 3;
    pub const ERROR: i32 = 4;
    pub const MATCHED: i32 = 5;
    pub const CLAUSE_VALUE: i32 = 6;
    pub const TAIL_RESUME: i32 = 7;
}

/// 操作節の分類の番号。ハンドラ記録の `classes` に置く値である。
pub mod cls {
    pub const T: i32 = 0;
    pub const N: i32 = 1;
    pub const G: i32 = 2;
}

/// モジュールに置く Wasm の関数を区別するキー。
///
/// 同じキーの関数は 1 つだけ作る([`Cg::func`])。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FKey {
    /// LIR の関数。値は関数の番号である。
    Lir(u32),
    /// 内容の決まっているランタイム関数。値は `rt::H` で、どの関数かを表す。
    H(rt::H),
    /// 関数値を `n` 個の引数で呼ぶ関数。
    CallV(u32),
    /// 辞書からメソッドを選んで呼ぶ関数。値は(クラス、メソッド、メソッド自身の辞書の個数、引数の個数)である。
    CallM(u32, u32, u32, u32),
    /// 辞書の抽象(種類 `dictabs` の関数のクロージャ)に、`k` 個の辞書を引数として渡して呼ぶ関数。
    DictApp(u32),
    /// 退避した `DISCARD(NeverBody)` の節の本体を末尾呼び出しで呼ぶ関数。
    TailNever,
    /// 継続が持つ G の節の本体を、継続を引数にして末尾呼び出しで呼ぶ関数(`call_clause`)。
    ClauseK,
    /// 構造的な `Eq` の比較を行う関数。値はクラスである。
    EqStruct(u32),
}

/// どのプログラムでも作るグローバルの番号。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gl {
    pub depth: u32,
    pub limit: u32,
    pub lmax: u32,
    pub lr: u32,
    pub next_hid: u32,
    pub chain: u32,
    pub p_kind: u32,
    pub p_hr: u32,
    pub p_val: u32,
    pub p_clause: u32,
    pub p_args: u32,
    pub p_msg: u32,
    /// `YIELD` のときの、連鎖の切り出した部分の先頭。
    pub p_cut: u32,
    /// `YIELD` の伝播中に追加したフレームの記録を並べたリストの、先頭(最上段)と末尾(最下段)。
    pub yf_head: u32,
    pub yf_tail: u32,
    /// 再開するフレームに渡す値と、分割関数をまたぐ呼び出しの結果を置く。
    pub rv: u32,
    pub unit: u32,
    pub true_: u32,
    pub false_: u32,
    pub str_true: u32,
    pub str_false: u32,
    pub empty: u32,
}

/// コード生成のコンテキスト。
///
/// 組み立て中のモジュールと、LIR のプログラムの各項目に対応する Wasm の関数やグローバルの番号を持つ。
pub(crate) struct Cg<'p> {
    pub prog: &'p Program,
    pub m: Mb,
    fmap: HashMap<FKey, u32>,
    pending: VecDeque<(FKey, u32)>,
    pub g: Gl,
    /// LIR のグローバルごとの、(値、初期化済みのフラグ)を置く Wasm のグローバルの番号。
    pub pglobals: Vec<(u32, u32)>,
    /// インスタンスごとの、初期化済みのフラグを置く Wasm のグローバルの番号。
    pub inst_init: Vec<u32>,
    /// インスタンスのメソッドごとの、値を置く Wasm のグローバルの番号。
    pub inst_vals: Vec<Vec<u32>>,
    /// 文字列定数ごとの(Wasm のグローバルの番号、データの中のオフセット、長さ)。
    pub strs: Vec<(u32, u32, u32)>,
    ctor0: HashMap<(u32, u32), u32>,
    dictb: HashMap<(u32, u32), u32>,
    dicti0: HashMap<u32, u32>,
    /// 診断メッセージのテンプレートの表。
    pub tpl: Templates,
    /// メソッドの値を作る命令に現れる(クラス、メソッド、メソッド自身の辞書の個数)の集合。
    pub method_vals: BTreeSet<(u32, u32, u32)>,
    /// `extern` の本体になる LIR の関数の番号。
    pub extern_funcs: Vec<u32>,
    /// 操作節の関数の引数の個数の集合。
    pub clause_arities: BTreeSet<u32>,
    /// 実装のない `extern` のグローバルの番号。
    pub extern_unimpl: Vec<u32>,
    /// G の節を持つハンドラがプログラムのどこかにあれば真。
    ///
    /// なければ、状態ワードが `YIELD` になることはない。
    /// `YIELD` を設定するのは、G の節を選んだ `perform` だけだからである。
    /// その場合は、どの関数も、継続のキャプチャのための区切り、フレームの記録、再開用エントリを持たない。
    pub has_g: bool,
    /// コード生成の統計。
    pub stats: Stats,
}

/// コード生成の統計。
///
/// 環境変数 `KELEUT_WASM_STATS` が設定されているときに、標準エラーに書く。
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// LIR の関数の個数。
    pub funcs: usize,
    /// 大きなフレーム方式にした関数の個数。
    pub big_frames: usize,
    /// 1 段の見積もりバイト数の最大値。大きなフレーム方式にした関数は、その方式にした後の見積もりを最大値の計算に使う。
    pub max_estimate: u64,
    /// 引数の個数の最大値。
    pub max_params: usize,
    /// 1 つの関数が持つブロックの個数の最大値。
    pub max_blocks: usize,
    /// 呼び出しの引数の個数の最大値。
    pub max_args: u32,
    /// 複数の Wasm の関数に分割した LIR の関数の個数。
    pub split_funcs: usize,
    /// 1 つの LIR の関数を分割してできた分割関数の個数の最大値。
    pub max_parts: usize,
    /// 重みが最大の節の経路にある、`cancel` 節の数 `k`([`clause_path`])。
    pub cancel_nest: u32,
    /// 重みが最大の節の経路で `r` に数えた、`return` 節と N の節の数([`clause_path`])。
    pub ret_chain: u32,
    /// プログラムの 1 段に必要なバイト数 `B_prog`([`b_prog`])。
    pub b_prog: u64,
}

fn cexpr(insns: Vec<Instruction<'static>>) -> ConstExpr {
    ConstExpr::extended(insns)
}

impl<'p> Cg<'p> {
    fn new(prog: &'p Program) -> Cg<'p> {
        let mut m = Mb::new();
        let t = m.t;
        let i64g = |m: &mut Mb, n: &str| m.global(I64, true, ConstExpr::i64_const(0), n);
        let i32g = |m: &mut Mb, n: &str| m.global(I32, true, ConstExpr::i32_const(0), n);
        let eqg = |m: &mut Mb, n: &str| m.global(EQ, true, ConstExpr::ref_null(emit::HEQ), n);
        let depth = i64g(&mut m, "depth");
        let limit = i64g(&mut m, "limit");
        let lmax = i64g(&mut m, "lmax");
        let lr = i64g(&mut m, "lr");
        let next_hid = m.global(I64, true, ConstExpr::i64_const(1), "next_hid");
        let chain = eqg(&mut m, "chain");
        let p_kind = i32g(&mut m, "p_kind");
        let p_hr = eqg(&mut m, "p_hr");
        let p_val = eqg(&mut m, "p_val");
        let p_clause = i32g(&mut m, "p_clause");
        let p_args = eqg(&mut m, "p_args");
        let p_msg = i32g(&mut m, "p_msg");
        let p_cut = eqg(&mut m, "p_cut");
        let frg = |m: &mut Mb, n: &str| {
            m.global(
                rv(true, t.frame),
                true,
                ConstExpr::ref_null(wasm_encoder::HeapType::Concrete(t.frame)),
                n,
            )
        };
        let yf_head = frg(&mut m, "yf_head");
        let yf_tail = frg(&mut m, "yf_tail");
        let rv_ = eqg(&mut m, "rv");
        let unit = m.global(
            rv(false, t.unit),
            false,
            cexpr(vec![Instruction::StructNew(t.unit)]),
            "unit",
        );
        let true_ = m.global(
            rv(false, t.bool_),
            false,
            cexpr(vec![
                Instruction::I32Const(1),
                Instruction::StructNew(t.bool_),
            ]),
            "true",
        );
        let false_ = m.global(
            rv(false, t.bool_),
            false,
            cexpr(vec![
                Instruction::I32Const(0),
                Instruction::StructNew(t.bool_),
            ]),
            "false",
        );
        let empty = m.global(
            rv(false, t.anyarr),
            false,
            cexpr(vec![Instruction::ArrayNewFixed {
                array_type_index: t.anyarr,
                array_size: 0,
            }]),
            "empty",
        );
        let strg = |m: &mut Mb, n: &str| {
            m.global(
                rv(true, t.str_),
                true,
                ConstExpr::ref_null(wasm_encoder::HeapType::Concrete(t.str_)),
                n,
            )
        };
        let str_true = strg(&mut m, "str_true");
        let str_false = strg(&mut m, "str_false");
        let g = Gl {
            depth,
            limit,
            lmax,
            lr,
            next_hid,
            chain,
            p_kind,
            p_hr,
            p_val,
            p_clause,
            p_args,
            p_msg,
            p_cut,
            yf_head,
            yf_tail,
            rv: rv_,
            unit,
            true_,
            false_,
            str_true,
            str_false,
            empty,
        };
        let mut pglobals = Vec::new();
        for (i, gl) in prog.globals.iter().enumerate() {
            let v = eqg(&mut m, &format!("g{i}"));
            let f = i32g(&mut m, &format!("g{i}.init"));
            let _ = gl;
            pglobals.push((v, f));
        }
        let mut inst_init = Vec::new();
        let mut inst_vals = Vec::new();
        for (i, inst) in prog.instances.iter().enumerate() {
            inst_init.push(i32g(&mut m, &format!("inst{i}.init")));
            inst_vals.push(
                (0..inst.methods.len())
                    .map(|k| eqg(&mut m, &format!("inst{i}.m{k}")))
                    .collect(),
            );
        }
        let mut strs = Vec::new();
        for (i, s) in prog.strings.iter().enumerate() {
            let gi = strg(&mut m, &format!("str{i}"));
            let off = m.add_data(s);
            strs.push((gi, off, s.len() as u32));
        }
        let mut extern_funcs = Vec::new();
        let mut extern_unimpl = Vec::new();
        for (i, gl) in prog.globals.iter().enumerate() {
            match &gl.kind {
                GlobalKind::Extern { func: Some(f), .. } => extern_funcs.push(f.0),
                GlobalKind::Extern { func: None, .. } => extern_unimpl.push(i as u32),
                _ => {}
            }
        }
        let mut clause_arities = BTreeSet::new();
        let mut method_vals = BTreeSet::new();
        let mut has_g = false;
        for f in &prog.funcs {
            if f.kind == FuncKind::Kir(keleut_lir::FnKind::Opclause) {
                clause_arities.insert(f.params.len() as u32);
            }
            for b in &f.blocks {
                for i in &b.insts {
                    match i {
                        Inst::MethodNew {
                            class, method, own, ..
                        } => {
                            method_vals.insert((class.0, *method, own.len() as u32));
                        }
                        Inst::Fx {
                            op: FxOp::HandlerNew { ops, .. },
                            ..
                        } => {
                            has_g |= ops.iter().any(|(_, c)| matches!(c, ClauseClass::G));
                        }
                        _ => {}
                    }
                }
            }
        }
        Cg {
            prog,
            m,
            fmap: HashMap::new(),
            pending: VecDeque::new(),
            g,
            pglobals,
            inst_init,
            inst_vals,
            strs,
            ctor0: HashMap::new(),
            dictb: HashMap::new(),
            dicti0: HashMap::new(),
            tpl: Templates::default(),
            method_vals,
            extern_funcs,
            clause_arities,
            extern_unimpl,
            has_g,
            stats: Stats::default(),
        }
    }

    /// キー `k` の関数の番号を返す。
    ///
    /// まだなければ、型を決めて番号を確保する。
    /// 本体は後で作る。
    pub fn func(&mut self, k: FKey) -> u32 {
        if let Some(&i) = self.fmap.get(&k) {
            return i;
        }
        let (params, results, name, safe) = self.sig_of(&k);
        let ty = match &k {
            FKey::Lir(f) => {
                let n = self.prog.funcs[*f as usize].params.len() as u32;
                self.m.fn_types(n).0
            }
            _ => self.m.sig(&params, &results),
        };
        let idx = self.m.declare_func(ty, name, safe);
        self.fmap.insert(k.clone(), idx);
        self.pending.push_back((k, idx));
        idx
    }

    /// オペランドスタックに積んだ `n` 個の引数のうち、[`P_DIRECT`] 個を超える分を 1 つの配列にまとめる命令を生成する。
    pub fn pack_args(&mut self, fb: &mut emit::Fb, n: u32) {
        self.stats.max_args = self.stats.max_args.max(n);
        if n > P_DIRECT {
            fb.i()
                .array_new_fixed(self.m.t.anyarr, (n - P_DIRECT).min(PACKED_MAX));
        }
    }

    /// LIR の関数 `f` に対応する Wasm の関数の番号を返す。
    pub fn lir(&mut self, f: FuncId) -> u32 {
        self.func(FKey::Lir(f.0))
    }

    /// LIR の関数 `f` の引数の個数を返す。
    pub fn arity(&self, f: FuncId) -> u32 {
        self.prog.funcs[f.index()].params.len() as u32
    }

    fn sig_of(&mut self, k: &FKey) -> (Vec<ValType>, Vec<ValType>, String, bool) {
        match k {
            FKey::Lir(f) => {
                let func = &self.prog.funcs[*f as usize];
                (Vec::new(), Vec::new(), format!("f{f}:{}", func.sym), false)
            }
            FKey::H(h) => {
                let (p, r, safe) = rt::sig(self, *h);
                (p, r, format!("{h:?}"), safe)
            }
            FKey::CallV(n) => {
                let mut p = vec![EQ];
                p.extend(self.m.arg_types(*n));
                (p, vec![EQ], format!("callv{n}"), false)
            }
            FKey::CallM(c, m, no, n) => {
                let mut p = vec![EQ];
                p.extend(std::iter::repeat_n(EQ, *no as usize));
                p.extend(self.m.arg_types(*n));
                (p, vec![EQ], format!("callm{c}.{m}.{no}.{n}"), false)
            }
            FKey::DictApp(n) => {
                let mut p = vec![EQ];
                p.extend(self.m.arg_types(*n));
                (p, vec![EQ], format!("dictapp{n}"), false)
            }
            FKey::TailNever => (vec![EQ], vec![EQ], "tail_never".to_string(), false),
            FKey::ClauseK => (vec![EQ], vec![EQ], "clause_k".to_string(), false),
            FKey::EqStruct(c) => (vec![EQ, EQ, EQ], vec![I32], format!("eq_struct{c}"), false),
        }
    }

    /// 引数のないコンストラクタの値を置くグローバルの番号を返す。
    /// そのグローバルがまだなければ、ここで作る。
    pub fn ctor0(&mut self, ty: u32, c: u32) -> u32 {
        if let Some(&g) = self.ctor0.get(&(ty, c)) {
            return g;
        }
        let t = self.m.t;
        let g = self.m.global(
            rv(false, t.data),
            false,
            cexpr(vec![
                Instruction::I32Const(ty as i32),
                Instruction::I32Const(c as i32),
                Instruction::ArrayNewFixed {
                    array_type_index: t.anyarr,
                    array_size: 0,
                },
                Instruction::StructNew(t.data),
            ]),
            &format!("ctor0.{ty}.{c}"),
        );
        self.ctor0.insert((ty, c), g);
        g
    }

    /// 組み込みのインスタンスの辞書を置くグローバルの番号を返す。
    /// そのグローバルがまだなければ、ここで作る。
    pub fn dictb(&mut self, class: u32, ty: u32) -> u32 {
        if let Some(&g) = self.dictb.get(&(class, ty)) {
            return g;
        }
        let t = self.m.t;
        let g = self.m.global(
            rv(false, t.dictb),
            false,
            cexpr(vec![
                Instruction::I32Const(class as i32),
                Instruction::I32Const(ty as i32),
                Instruction::StructNew(t.dictb),
            ]),
            &format!("dictb.{class}.{ty}"),
        );
        self.dictb.insert((class, ty), g);
        g
    }

    /// 前提を持たないユーザ定義のインスタンスの辞書を置くグローバルの番号を返す。
    /// そのグローバルがまだなければ、ここで作る。
    pub fn dicti0(&mut self, inst: u32) -> u32 {
        if let Some(&g) = self.dicti0.get(&inst) {
            return g;
        }
        let t = self.m.t;
        let g = self.m.global(
            rv(false, t.dicti),
            false,
            cexpr(vec![
                Instruction::I32Const(inst as i32),
                Instruction::ArrayNewFixed {
                    array_type_index: t.anyarr,
                    array_size: 0,
                },
                Instruction::StructNew(t.dicti),
            ]),
            &format!("dicti0.{inst}"),
        );
        self.dicti0.insert(inst, g);
        g
    }
}

/// コード生成のオプション。
#[derive(Clone, Debug)]
pub struct Options {
    /// 論理的な深さの上限 `L`。
    ///
    /// ホストに渡す表に書くので、`keleut build` の出力を単独で実行するときの上限にもなる。
    pub max_depth: u64,
}

/// コード生成の結果。
pub struct Output {
    /// `program.wasm` の内容。
    pub program: Vec<u8>,
    /// ホストに渡す表(JSON)。
    pub meta: String,
    /// コード生成の統計。
    pub stats: Stats,
}

/// LIR のプログラムから WasmGC のモジュールを生成する。
///
/// パス 12 を済ませていない関数があるときと、引数が `P_DIRECT + PACKED_MAX` 個を超える呼び出しがあるときは、エラーを返す。
/// 関数のコード生成が返すエラーも、そのまま返す。
pub fn compile(prog: &Program, opts: &Options) -> Result<Output, String> {
    if prog.funcs.iter().any(|f| f.frame.is_none()) {
        return Err("パス 12 を済ませていない関数があります".into());
    }
    let mut cg = Cg::new(prog);
    for i in 0..prog.funcs.len() {
        cg.lir(FuncId(i as u32));
    }
    let main = cg.func(FKey::H(rt::H::Main));
    cg.m.export("main", ExportKind::Func, main);
    for (name, h) in rt::EXPORTED {
        let f = cg.func(FKey::H(*h));
        cg.m.export(name, ExportKind::Func, f);
    }
    cg.m.export("p_kind", ExportKind::Global, cg.g.p_kind);
    cg.m.export("p_msg", ExportKind::Global, cg.g.p_msg);
    while let Some((k, idx)) = cg.pending.pop_front() {
        let body = match &k {
            FKey::Lir(f) => func::generate(&mut cg, FuncId(*f))?,
            _ => rt::generate(&mut cg, &k),
        };
        cg.m.set_body(idx, body);
    }
    cg.stats.funcs = prog.funcs.len();
    if cg.stats.max_args > P_DIRECT + PACKED_MAX {
        return Err(format!(
            "引数が {} 個の呼び出しは扱えません(上限は {} 個)",
            cg.stats.max_args,
            P_DIRECT + PACKED_MAX
        ));
    }
    (cg.stats.cancel_nest, cg.stats.ret_chain) = clause_path(prog);
    cg.stats.b_prog = b_prog(cg.stats.cancel_nest, cg.stats.ret_chain);
    let meta = meta::meta_json(
        prog,
        &cg.tpl,
        opts.max_depth,
        cg.stats.b_prog,
        cg.stats.cancel_nest,
    );
    let stats = cg.stats.clone();
    let program = cg.m.finish();
    Ok(Output {
        program,
        meta,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn スタックの大きさは_m79_の記録の値() {
        // 既定の L = 2^20 で B_prog = B_wasm なら、--stack-size は 1,630,208 KiB で、V8 の上限に収まる。
        assert_eq!(stack_size_kib(1 << 20, B_WASM), 1_630_208);
        assert!(stack_size_kib(1 << 20, B_WASM) <= STACK_SIZE_MAX_KIB);
        assert!(stack_size_kib(2_000_000, B_WASM) > STACK_SIZE_MAX_KIB);
        // ローカル 178 個、引数 2 個の関数の見積もりは、ちょうど B_wasm になる。
        assert_eq!(frame_estimate(178, 2), 1536);
        assert!(frame_estimate(179, 2) > B_WASM);
    }

    /// `tests/fixtures/cancel_nest/NAME.kir` を読んで LIR に変換する。
    ///
    /// `.kir` は、同じ名前の `.kel` から `keleut emit-kir -o NAME.kir NAME.kel` で書いたものである。
    /// 各プログラムの構造は次のとおりである。
    ///
    /// - `a`、`c`: `cancel` 節の中に直接ハンドラ式を書く。
    /// - `b`: `cancel` 節の中のハンドラ式の本体に、`cancel` 節を持つハンドラ式をもう 1 つ書く。
    /// - `d`: `cancel` 節は関数を呼ぶだけで、ハンドラ式を持たない。
    /// - `e`: `cancel` 節を持たない。
    /// - `f`: `cancel` 節の中に直接書いたハンドラ式が G の節を持ち、その節が末尾 `resume` で再開する。
    /// - `n0`: `cancel` 節は値を表示するだけで、ハンドラ式を持たない。
    /// - `n2`: `cancel` 節の中に直接、`cancel` 節を持つハンドラ式を書き、その `cancel` 節の中にさらにハンドラ式を書く。
    /// - `n2nc`: `n2` と同じ構造で、どちらの `cancel` 節も変数をキャプチャしない。
    /// - `r`: `return` 節が `go` を再帰的に呼ぶ。
    /// - `r0`: `return` 節が `echoln` だけを呼ぶ。
    /// - `r3`: `return` 節の中に直接書いたハンドラ式の `return` 節を、3 段重ねる。
    /// - `nc3`、`nc1`: 分類 N の操作節(N の節)を、それぞれ 3 段、1 段重ねる。
    /// - `nc2leaf`: N の節を 2 段重ね、内側の N の節は `echoln` だけを呼ぶ。
    /// - `mix`: N の節の中に直接書いたハンドラ式の `return` 節が、`go` を再帰的に呼ぶ。
    /// - `alt3`: `cancel` 節と N の節が交互に 3 組重なり、その内側にもう 1 つ `cancel` 節がある。
    /// - `alt3r`: `alt3` の N の節の代わりに `return` 節を使う。
    /// - `alt1n`: `cancel` 節、N の節、`cancel` 節の順に重なる。
    fn lowered(name: &str) -> Program {
        let path = format!(
            "{}/tests/fixtures/cancel_nest/{name}.kir",
            env!("CARGO_MANIFEST_DIR")
        );
        let k = kir::load(&std::fs::read(&path).unwrap()).unwrap();
        keleut_lir::lower(&k).unwrap()
    }

    #[test]
    fn cancel_節の中のハンドラ式の入れ子の段の数() {
        // cancel 節の中に直接ハンドラ式を書くプログラム(a、c)と、
        // そのハンドラ式の G の節が末尾 resume で再開するプログラム(f)は、k = 1 である。
        // b は、cancel 節の中のハンドラ式の本体に、cancel 節を持つハンドラ式をもう 1 つ書く。
        // 本体は深さを 1 つ増やして実行されるので、どちらの cancel 節から始まる経路も k = 1 である。
        for name in ["a", "b", "c", "f"] {
            assert_eq!(clause_path(&lowered(name)), (1, 0), "{name}");
        }
        // cancel 節の中に直接、cancel 節を持つハンドラ式を書き、その cancel 節の中にさらにハンドラ式を書くと、k = 2 である。
        assert_eq!(clause_path(&lowered("n2")), (2, 0));
        // どちらの cancel 節も変数をキャプチャしないプログラムでも、cancel 節の関数値はハンドラ式を書いた関数の中で作るので、k = 2 である。
        assert_eq!(clause_path(&lowered("n2nc")), (2, 0));
        // cancel 節がハンドラ式を持たないプログラム(d は関数を呼ぶだけ、n0 は値を表示するだけ)と、
        // 比較のための cancel 節のないプログラム e は、k = 0 である。
        for name in ["d", "e", "n0"] {
            assert_eq!(clause_path(&lowered(name)), (0, 0), "{name}");
        }
    }

    #[test]
    fn 同じ深さに重なる節の鎖() {
        // return 節が go を再帰的に呼ぶプログラム r は r = 1 である。
        // return 節が echoln だけを呼ぶプログラム r0 と、cancel 節だけを重ねるプログラムは r = 0 である。
        assert_eq!(clause_path(&lowered("r")), (0, 1));
        for name in ["r0", "a", "c", "e", "f", "n2"] {
            assert_eq!(clause_path(&lowered(name)).1, 0, "{name}");
        }
        // return 節の中に直接書いたハンドラ式の return 節を 3 段重ねたプログラム r3 は、r = 3 である。
        assert_eq!(clause_path(&lowered("r3")), (0, 3));
        // 分類 N の操作節(N の節)を 3 段重ねたプログラム nc3 は、r = 3 である。
        // N の節が 1 つだけのプログラム(nc1)と、内側の N の節が葉の関数だけを呼ぶ 2 段のプログラム(nc2leaf)では、N の節を数えない。
        assert_eq!(clause_path(&lowered("nc3")), (0, 3));
        for name in ["nc1", "nc2leaf"] {
            assert_eq!(clause_path(&lowered(name)), (0, 0), "{name}");
        }
        // N の節の中に直接書いたハンドラ式の return 節が go を再帰的に呼ぶプログラム mix は、r = 2 である。
        assert_eq!(clause_path(&lowered("mix")), (0, 2));
        // cancel 節と N の節が交互に並ぶプログラム alt3(cancel 節 4 つと N の節 3 つ)は、1 本の経路に沿って数えて k = 4、r = 3 である。
        // k を cancel 節だけの入れ子で、r を return 節と N の節だけの連鎖で別々に数えると (1, 0) になる。
        // その値では B_prog が足りず、深さが既定の L に達する前に終了コード 70 で終了する。
        assert_eq!(clause_path(&lowered("alt3")), (4, 3));
        assert_eq!(b_prog(4, 3), 1536 + 4 * 1360 + 3 * 1536 - 320);
        // N の節の代わりに return 節を使うプログラム alt3r も、同じ値である。
        assert_eq!(clause_path(&lowered("alt3r")), (4, 3));
        // cancel 節の間にある N の節が 1 つだけのプログラム alt1n では、N の節を数えず、k = 2(cancel 節 2 つ)である。
        assert_eq!(clause_path(&lowered("alt1n")), (2, 0));
        // r ≥ 1 では、B_wasm に r × B_wasm − E_rth を足す。
        // r = 1 なら 1,040 + 1,536 + 176 = 2,752 バイトである。
        assert_eq!(b_prog(0, 1), 2752);
        assert_eq!(b_prog(0, 2), 2752 + 1536);
        assert_eq!(b_prog(0, 6), 1536 + 6 * 1536 - 320);
        assert_eq!(b_prog(1, 1), 2896 + 1216);
        let out = compile(&lowered("r"), &Options { max_depth: 600_000 }).unwrap();
        assert_eq!(
            (out.stats.cancel_nest, out.stats.ret_chain, out.stats.b_prog),
            (0, 1, 2752)
        );
        assert!(
            out.meta
                .starts_with("{\"maxDepth\":600000,\"bProg\":2752,\"cancelNest\":0,"),
            "{}",
            &out.meta[..80]
        );
        // 既定の L では、k = 0 なら r が 9 まで、スタックを STACK_CAP で切り詰めない(r = 0 なら k が 10 までである)。
        assert!(stack_need_kib(1 << 20, b_prog(0, 9)) <= STACK_CAP_KIB);
        assert!(stack_need_kib(1 << 20, b_prog(0, 10)) > STACK_CAP_KIB);
    }

    #[test]
    fn b_prog_とスタックの大きさ() {
        // k = 0 かつ r = 0 では B_prog = B_wasm で、スタックサイズは B_wasm で計算した値と同じである。
        assert_eq!(b_prog(0, 0), B_WASM);
        assert_eq!(stack_size_kib(1 << 20, b_prog(0, 0)), 1_630_208);
        // k = 1 では 1,040 + 320 + 1,040 + 320 + 176 = 2,896 バイトである。
        assert_eq!(b_prog(1, 0), 2896);
        assert_eq!(b_prog(2, 0), 2896 + 1360);
        // L = 600,000、k = 1 のスタックサイズは、V8 の --stack-size の上限に収まる。
        assert_eq!(stack_size_kib(600_000, b_prog(1, 0)), 1_756_157);
        assert!(stack_size_kib(600_000, b_prog(1, 0)) <= STACK_SIZE_MAX_KIB);
        // 既定の L では、k = 1 で V8 の上限を超えるので Worker の経路になる。
        // k = 10 までは STACK_CAP で切り詰めない。
        assert!(stack_size_kib(1 << 20, b_prog(1, 0)) > STACK_SIZE_MAX_KIB);
        assert!(stack_need_kib(1 << 20, b_prog(10, 0)) <= STACK_CAP_KIB);
        assert!(stack_need_kib(1 << 20, b_prog(11, 0)) > STACK_CAP_KIB);
        // それを超えると、STACK_CAP で切り詰める。
        assert_eq!(stack_size_kib(1 << 20, b_prog(11, 0)), STACK_CAP_KIB);
        assert_eq!(stack_size_kib(1 << 20, b_prog(u32::MAX, 0)), STACK_CAP_KIB);
        // B_prog = B_wasm のときのサイズより小さくは切り詰めない。
        // そのため、k = 0 かつ r = 0 のプログラムのスタックサイズは、STACK_CAP を超えていても変わらない。
        let l = 100_000_000;
        assert!(stack_need_kib(l, B_WASM) > STACK_CAP_KIB);
        assert_eq!(stack_size_kib(l, B_WASM), stack_need_kib(l, B_WASM));
        assert_eq!(stack_size_kib(l, b_prog(1, 0)), stack_need_kib(l, B_WASM));
        assert_eq!(
            stack_size_kib(u64::MAX, b_prog(u32::MAX, 0)),
            stack_need_kib(u64::MAX, B_WASM)
        );
        // ホストの Worker の経路も、同じ上限 STACK_CAP_KIB と、下限の計算に使う同じ B_WASM を持つ。
        for line in [
            format!("const STACK_CAP_KIB = {STACK_CAP_KIB};"),
            format!("const B_WASM = {B_WASM};"),
        ] {
            assert!(
                launch::HOST_JS.contains(&line),
                "host/keleut-host.mjs に {line} がない"
            );
        }
    }

    #[test]
    fn meta_に_b_prog_と_k_を書く() {
        let out = compile(&lowered("c"), &Options { max_depth: 600_000 }).unwrap();
        assert_eq!((out.stats.cancel_nest, out.stats.b_prog), (1, 2896));
        assert!(
            out.meta
                .starts_with("{\"maxDepth\":600000,\"bProg\":2896,\"cancelNest\":1,"),
            "{}",
            &out.meta[..80]
        );
        let out = compile(&lowered("e"), &Options { max_depth: 600_000 }).unwrap();
        assert_eq!((out.stats.cancel_nest, out.stats.b_prog), (0, B_WASM));
    }
}
