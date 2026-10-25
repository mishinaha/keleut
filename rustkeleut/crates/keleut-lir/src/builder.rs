//! 関数の組み立て。
//!
//! 下ろしは、ブロックを作った順の番号で命令を足していく。
//! 合流のブロックは枝の中身より先に作るので、番号の順は位相の順にならない。
//! [`Builder::finish`] が、入口からたどれるブロックを位相の順(逆後順)に並べ直し、
//! どの枝も番号の大きいブロックへ向かう形にする。

use crate::ir::*;

/// 組み立て中のブロック。
struct Open {
    insts: Vec<Inst>,
    term: Option<Term>,
}

/// 関数の組み立て器。
pub(crate) struct Builder {
    regs: Vec<Rep>,
    blocks: Vec<Open>,
    cur: Option<BlockId>,
    params: Vec<Reg>,
}

impl Builder {
    /// 入口のブロックだけを持つ組み立て器を作る。
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

    /// 新しいレジスタ。
    pub(crate) fn reg(&mut self, rep: Rep) -> Reg {
        self.regs.push(rep);
        Reg((self.regs.len() - 1) as u32)
    }

    /// 引数を受け取る `any` のレジスタを足す。
    pub(crate) fn param(&mut self) -> Reg {
        let r = self.reg(Rep::Any);
        self.params.push(r);
        r
    }

    /// これまでに足した引数のレジスタ。
    pub(crate) fn params(&self) -> &[Reg] {
        &self.params
    }

    /// 新しいブロック(まだ移らない)。
    pub(crate) fn block(&mut self) -> BlockId {
        self.blocks.push(Open {
            insts: Vec::new(),
            term: None,
        });
        BlockId((self.blocks.len() - 1) as u32)
    }

    /// 命令を足すブロックを `b` にする。
    pub(crate) fn switch_to(&mut self, b: BlockId) {
        debug_assert!(self.blocks[b.index()].term.is_none());
        self.cur = Some(b);
    }

    /// いまのブロック(終わっていれば `None`)。
    pub(crate) fn current(&self) -> Option<BlockId> {
        self.cur
    }

    /// いまのブロックに命令を足す。
    pub(crate) fn emit(&mut self, i: Inst) {
        let b = self.cur.expect("終わったブロックに命令を足そうとした");
        self.blocks[b.index()].insts.push(i);
    }

    /// いまのブロックを終える。以後、[`Builder::switch_to`] するまで命令を足せない。
    pub(crate) fn terminate(&mut self, t: Term) {
        let b = self.cur.take().expect("終わったブロックを終えようとした");
        self.blocks[b.index()].term = Some(t);
    }

    /// 定数を新しいレジスタに置く。
    pub(crate) fn konst(&mut self, value: Const) -> Reg {
        let dst = self.reg(value.rep());
        self.emit(Inst::Const { dst, value });
        dst
    }

    /// `bad`(`bool`)が真なら実行時エラー `kind` を起こし、偽ならいまのブロックの続きへ進む。
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

    /// 位相の順に並べ直して取り出す(レジスタ、引数、ブロック)。
    ///
    /// 入口からたどれないブロックは捨てる。
    /// 終わっていないブロックが残っていれば、それは組み立ての誤りである。
    pub(crate) fn finish(self) -> (Vec<Rep>, Vec<Reg>, Vec<Block>) {
        let n = self.blocks.len();
        let succs: Vec<Vec<BlockId>> = self
            .blocks
            .iter()
            .map(|b| b.term.as_ref().map(|t| t.succs()).unwrap_or_default())
            .collect();
        // 反復の深さ優先で後順を作る(深い入れ子でもスタックを使わない)。
        let mut state = vec![0u8; n];
        let mut post = Vec::with_capacity(n);
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        state[0] = 1;
        while let Some(top) = stack.last_mut() {
            let (b, k) = *top;
            if k < succs[b].len() {
                top.1 += 1;
                // 後継の無いブロック(`error` などで終わるもの)を最後に、ほかは後の枝から先にたどる。
                // 逆後順では最後にたどった後継が `branch` の直後に来るので、`branch c then else` の
                // `then`(検査の失敗の枝)や、照合の失敗の先の `error` のブロックが直後に来る。
                // 前任を処理し終えていないブロックが遠くに残らないので、検証と生存解析が同時に持つ集合が少なくなる。
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
