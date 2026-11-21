//! 関数の組み立て。
//!
//! 変換器は、作った順に番号を振ったブロックに、命令を追加していく。
//! 合流のブロックは分岐先の内容より先に作るので、番号の順はトポロジカル順にならない。
//! [`Builder::finish`] が、入口からたどれるブロックをトポロジカル順(逆ポストオーダー)に並べ直し、
//! どの分岐も番号の大きいブロックへ向かうようにする。

use crate::ir::*;

/// 組み立て中のブロック。
struct Open {
    insts: Vec<Inst>,
    /// 終端命令。ブロックを終えるまでは `None` である。
    term: Option<Term>,
}

/// 関数のビルダ。
/// レジスタ、ブロック、命令を追加していき、[`Builder::finish`] で関数の内容を取り出す。
pub(crate) struct Builder {
    regs: Vec<Rep>,
    blocks: Vec<Open>,
    cur: Option<BlockId>,
    params: Vec<Reg>,
}

impl Builder {
    /// 入口のブロックだけを持つビルダを作る。
    pub(crate) fn new() -> Builder {
        Builder {
            regs: Vec::new(),
            blocks: vec![Open {
                insts: Vec::new(),
                term: None,
            }],
            cur: Some(BlockId(0)),
            params: Vec::new(),
        }
    }

    /// Rep が `rep` の新しいレジスタを作る。
    pub(crate) fn reg(&mut self, rep: Rep) -> Reg {
        self.regs.push(rep);
        Reg((self.regs.len() - 1) as u32)
    }

    /// 引数を受け取る `any` のレジスタを追加する。
    pub(crate) fn param(&mut self) -> Reg {
        let r = self.reg(Rep::Any);
        self.params.push(r);
        r
    }

    /// これまでに追加した引数のレジスタを返す。
    pub(crate) fn params(&self) -> &[Reg] {
        &self.params
    }

    /// 新しいブロックを作る。
    /// 命令を追加するブロックは切り替えない。
    pub(crate) fn block(&mut self) -> BlockId {
        self.blocks.push(Open {
            insts: Vec::new(),
            term: None,
        });
        BlockId((self.blocks.len() - 1) as u32)
    }

    /// 命令を追加するブロックを `b` に切り替える。
    pub(crate) fn switch_to(&mut self, b: BlockId) {
        debug_assert!(self.blocks[b.index()].term.is_none());
        self.cur = Some(b);
    }

    /// 現在のブロックを返す。
    /// ブロックを終えた後、次に切り替えるまでは `None` を返す。
    pub(crate) fn current(&self) -> Option<BlockId> {
        self.cur
    }

    /// 現在のブロックに命令を追加する。
    pub(crate) fn emit(&mut self, i: Inst) {
        let b = self.cur.expect("終わったブロックに命令を足そうとした");
        self.blocks[b.index()].insts.push(i);
    }

    /// 現在のブロックに終端命令 `t` を置いて、ブロックを終える。
    /// その後は、[`Builder::switch_to`] でブロックを切り替えるまで命令を追加できない。
    pub(crate) fn terminate(&mut self, t: Term) {
        let b = self.cur.take().expect("終わったブロックを終えようとした");
        self.blocks[b.index()].term = Some(t);
    }

    /// 定数 `value` を新しいレジスタに置く命令を追加し、そのレジスタを返す。
    pub(crate) fn konst(&mut self, value: Const) -> Reg {
        let dst = self.reg(value.rep());
        self.emit(Inst::Const { dst, value });
        dst
    }

    /// `bad`(`bool`)が真なら実行時エラー `kind` になり、偽なら続きを実行するように、分岐を置く。
    /// 続きの命令は、新しく作ったブロックに追加される。
    pub(crate) fn fail_if(&mut self, bad: Reg, kind: RtError, operand: Option<Reg>) {
        let err = self.block();
        let ok = self.block();
        self.terminate(Term::Branch {
            cond: bad,
            then: err,
            else_: ok,
        });
        self.switch_to(err);
        self.terminate(Term::Error { kind, operand });
        self.switch_to(ok);
    }

    /// ブロックをトポロジカル順に並べ直し、レジスタの Rep、引数のレジスタ、ブロックを取り出す。
    ///
    /// 入口からたどれないブロックは捨てる。
    /// 入口からたどれるブロックに終端命令のないものが残っていれば、変換器のバグなので、パニックする。
    pub(crate) fn finish(self) -> (Vec<Rep>, Vec<Reg>, Vec<Block>) {
        let n = self.blocks.len();
        let succs: Vec<Vec<BlockId>> = self
            .blocks
            .iter()
            .map(|b| b.term.as_ref().map(|t| t.succs()).unwrap_or_default())
            .collect();
        // 再帰を使わない深さ優先の走査で、ポストオーダーを作る。深い入れ子でもネイティブスタックを消費しない。
        let mut state = vec![0u8; n];
        let mut post = Vec::with_capacity(n);
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        state[0] = 1;
        while let Some(top) = stack.last_mut() {
            let (b, k) = *top;
            if k < succs[b].len() {
                top.1 += 1;
                // 後続ブロックを持たないブロック(`error` などで終わるもの)を最後にたどり、それ以外は後に書いた分岐先から先にたどる。
                // 逆ポストオーダーでは、最後にたどった後続ブロックが `branch` の直後に来る。
                // そのため、`branch c then else` の `then`(検査が失敗したときの分岐先)や、照合が失敗した先にある `error` のブロックが直後に来る。
                // 先行ブロックを処理し終えていないブロックが遠くに残らないので、検証と生存解析が同時に保持する集合が少なくなる。
                let n = succs[b].len();
                let leaf = |s: BlockId| succs[s.index()].is_empty();
                let mut order: Vec<BlockId> = (0..n).rev().map(|j| succs[b][j]).collect();
                order.sort_by_key(|&s| leaf(s));
                let s = order[k].index();
                if state[s] == 0 {
                    state[s] = 1;
                    stack.push((s, 0));
                }
            } else {
                state[b] = 2;
                post.push(b);
                stack.pop();
            }
        }
        post.reverse();
        let mut new_of = vec![u32::MAX; n];
        for (i, &b) in post.iter().enumerate() {
            new_of[b] = i as u32;
        }
        let mut old: Vec<Option<Open>> = self.blocks.into_iter().map(Some).collect();
        let mut blocks = Vec::with_capacity(post.len());
        for &b in &post {
            let o = old[b].take().unwrap();
            let mut term = o.term.expect("終わっていないブロックが残った");
            term.map_succs(|s| BlockId(new_of[s.index()]));
            blocks.push(Block {
                insts: o.insts,
                term,
            });
        }
        (self.regs, self.params, blocks)
    }
}
