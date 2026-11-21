//! 分割値スタック。
//!
//! フレームは、チャンクをつないだ値スタックに置く。
//! チャンクの既定のサイズは 1 MiB で、それに入らないフレームには、そのフレームが入るサイズのチャンクを確保する。
//! チャンクの中のワードには、値スタックの底を 0 として数えた**論理的な位置**を振る。
//! チャンク `i` の先頭の位置は、前のチャンクの先頭の位置に、前のチャンクのワード数を足したものである。
//! フレームはチャンクをまたがない。
//! チャンクの残りに入らないフレームは次のチャンクの先頭に置き、前のチャンクの残りは使わない。
//!
//! フレームのレイアウトは次のとおりで、インタプリタと JIT で同じにする。
//! 0 番から 3 番までのワードがフレームヘッダである。
//!
//! | ワード | 内容 |
//! | --- | --- |
//! | 0 | 関数の番号(下位 32 ビット)と再開点(上位 32 ビット) |
//! | 1 | フレームの深さ |
//! | 2 | `any` の領域のスロット数(下位 32 ビット)と、ボックス化していない値の領域のスロット数(上位 32 ビット) |
//! | 3 | 呼び出し元のフレームの論理的な位置。呼び出し元がネイティブのときは [`NATIVE_CALLER`] |
//! | 4〜 | `any` の領域のスロット、続けてボックス化していない値の領域のスロット |
//!
//! ヘッダはアドレスではなく論理的な位置を持つ。
//! そのため、継続が区間(継続にするフレームの範囲)をコピーし、後で値スタックに復元しても、ヘッダを読める。
//! ただし、論理的な位置はチャンクの境界がどこにあるかで決まるので、区間を別の底へ一様にずらすと、フレームがチャンクをまたぐことがある。
//! そのため、継続を値スタックに復元するときは、フレームを 1 つずつ [`ValueStack::place`] で配置し直し、ヘッダの 3 番のワード(呼び出し元の位置)を書き直す。
//! 1 番のワード(深さ)も、再開する側の深さを足して書き直す。
//! **注入**は、継続のコピーをチャンクのまま値スタックにつなぎ、エラーを送り込む操作である。
//! 注入するときは、フレームを配置し直さずに、継続が持つフレームのコピー領域をそのまま 1 つのチャンクとしてつなぐ([`ValueStack::link_copy`])。
//! チャンクを確保しないので、メモリ不足を注入するときにも、継続の中の `cancel` 節を実行できる。
//! こうしてつないだチャンクを**コピーチャンク**と呼ぶ。
//! コピーチャンクのワード数はコピーの長さと同じなので、チャンクのサイズはチャンクごとに異なることがある。
//!
//! フレームを捨てて呼び出し元へ戻るときには、チャンクを解放しない。
//! GC の後に、生きているフレームのうち最も上のものを含むチャンクと、その次の 1 つを残し、それより後ろのチャンクを解放する([`ValueStack::release_after_collect`])。
//! 値スタックのサイズは実行の上限にならない。
//! 上限になるのは論理的な深さである。
//!
//! チャンクを確保できないとき(システムのアロケータが失敗したとき)は、プロセスを止めずに [`OutOfMemory`] を返す。
//! 呼び出し元はこれを、ヒープの確保の失敗と同じく、メモリ不足の実行時エラー(IRSpec 付録 B の 19 番)にする。
//! 最初のチャンクも最初のフレームを置くときに確保するので、空の値スタックを作る操作は失敗しない。
//! 最初のチャンクを確保するときに、予備チャンク(1 MiB)も確保しておく。
//! チャンクを確保できなかったら、予備チャンクを次に使うチャンクにしてから [`OutOfMemory`] を返す。
//! メモリ不足のエラーを巻き戻す間に実行される `cancel` 節は、この予備チャンクにフレームを置ける。
//!
//! チャンクはグローバルアロケータから確保する([`crate::alloc`])。
//! グローバルアロケータが自分の予備領域を解放して確保をやり直したときは、確保に成功しても実行スレッドのフラグが立つ。
//! そこで、チャンクを確保した後にこのフラグを調べ、立っていればメモリ不足にする。

use std::alloc::{Layout, alloc_zeroed};

use crate::heap::OutOfMemory;

/// フレームヘッダのワード数。
pub const HEADER_WORDS: usize = 4;

/// 呼び出し元がネイティブであることを表す、ヘッダの 3 番のワードの値。
///
/// Rust で書いたランタイム関数が呼んだフレームと、プログラムの開始時に置くフレームでは、呼び出し元がネイティブである。
pub const NATIVE_CALLER: u64 = u64::MAX;

/// 既定のチャンクのワード数(1 MiB)。
const CHUNK_WORDS: usize = 1 << 17;

/// 値スタックにつないだチャンク 1 つ。
///
/// `mem` の長さがチャンクのワード数で、`base` は先頭のワードの論理的な位置である。
/// コピーチャンク([`ValueStack::link_copy`])のワード数は、コピーの `Vec` の長さと同じで、[`CHUNK_WORDS`] より小さいことがある。
struct Chunk {
    mem: Vec<u64>,
    base: u64,
}

/// 使用中のチャンク(次のフレームを置く位置を決めるときに基準にするチャンク)の範囲。
/// JIT のコードが、フレームを置く高速経路(fast path)で読む。
///
/// `lo` と `hi` はチャンクの先頭と終わりの論理的な位置、`ptr` はチャンクの先頭のワードのアドレスである。
/// チャンクがないときは `lo == hi == 0` で、高速経路ではどのフレームも置けない。
/// [`ValueStack`] は、使用中のチャンクを変えるたびにこれを書き直すので、この値は常に使用中のチャンクを表す。
#[repr(C)]
#[derive(Debug, Default)]
pub struct StackWindow {
    /// チャンクの先頭の論理的な位置。
    pub lo: u64,
    /// チャンクの終わり(最後のワードの次)の論理的な位置。
    pub hi: u64,
    /// チャンクの先頭のワードのアドレス。
    pub ptr: usize,
}

/// 分割値スタック。
/// チャンクの並びと予備チャンクを持つ。
pub struct ValueStack {
    chunks: Vec<Chunk>,
    /// 使用中のチャンクの添字。
    cur: usize,
    /// 確保しておいた予備チャンク。
    spare: Option<Box<[u64]>>,
    /// チャンクの確保がメモリ不足になった後に、次のチャンクとして使う領域。
    /// 予備チャンクか、実行スレッドのフラグが立った確保で得た領域である。
    armed: Option<Box<[u64]>>,
    /// 使用中のチャンクの範囲。
    win: StackWindow,
}

impl Default for ValueStack {
    fn default() -> Self {
        Self::new()
    }
}

/// `words` ワードの、0 で埋めたチャンクの領域を確保する。
/// 確保できなければ [`OutOfMemory`] を返す。
///
/// 確保はグローバルアロケータを通り、確保の失敗を呼び出し元が処理する範囲(`crate::alloc::handled`)の外で行う。
/// そのため、確保に失敗するとグローバルアロケータが予備領域を解放してやり直し、その後は実行スレッドのフラグが立っていることがある。
/// フラグは呼び出し元([`ValueStack::take_mem`] と [`ValueStack::place`])が調べる。
fn new_mem(words: usize) -> Result<Box<[u64]>, OutOfMemory> {
    let _site = crate::alloc::site_if_none(crate::alloc::Site::Chunk);
    let layout = Layout::array::<u64>(words).map_err(|_| OutOfMemory)?;
    // SAFETY: words ≧ CHUNK_WORDS なので layout のサイズは 0 でない。
    let p = unsafe { alloc_zeroed(layout) }.cast::<u64>();
    if p.is_null() {
        return Err(OutOfMemory);
    }
    // SAFETY: p は Layout::array::<u64>(words) で確保した、0 で埋めた words ワードの領域であり、Box<[u64]> が解放に使うレイアウトと同じである。
    Ok(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, words)) })
}

impl ValueStack {
    /// チャンクを持たない空の値スタックを作る。
    /// 最初のチャンクは最初の [`ValueStack::place`] で確保する。
    pub fn new() -> ValueStack {
        ValueStack {
            chunks: Vec::new(),
            cur: 0,
            spare: None,
            armed: None,
            win: StackWindow::default(),
        }
    }

    /// `win` を、使用中のチャンクの範囲に書き直す。
    #[inline]
    fn sync_win(&mut self) {
        let c = &mut self.chunks[self.cur];
        self.win = StackWindow {
            lo: c.base,
            hi: c.base + c.mem.len() as u64,
            ptr: c.mem.as_mut_ptr() as usize,
        };
    }

    /// 使用中のチャンクの範囲を持つ [`StackWindow`] へのポインタを返す。
    /// JIT のコンテキストに置くために使う。
    ///
    /// [`StackWindow`] は [`ValueStack`] のフィールドなので、返したポインタは [`ValueStack`] が移動しない間だけ使える。
    pub fn window_ptr(&mut self) -> *mut StackWindow {
        &mut self.win
    }

    /// 新しいチャンクに使う、`words` ワード以上の領域を返す。
    ///
    /// 次に使うチャンク(`armed`)があり、そこに `words` ワードが入れば、それを返す。
    /// そうでなければ、`words` と既定のワード数のうち大きいほうのサイズで領域を確保する。
    /// 確保の後に、実行スレッドのフラグを調べて下ろす(`crate::alloc`)。
    /// フラグが立っていれば、グローバルアロケータが予備領域を解放してやり直したか、ほかの確保が先に予備領域を使ったので、メモリ不足を返す。
    /// このとき、確保できた領域は次に使うチャンクにする。
    /// したがって、メモリ不足に備えた領域は、グローバルアロケータの予備領域、フラグが立った確保で得た領域、値スタックの予備チャンクの順に使う。
    /// 確保できなければ、値スタックの予備チャンクを次に使うチャンクにして、メモリ不足を返す。
    /// フラグは立っていてもいなくても下ろすので、1 回のメモリ不足には 1 回だけ応じる。
    fn take_mem(&mut self, words: usize) -> Result<Box<[u64]>, OutOfMemory> {
        if let Some(m) = self.armed.take() {
            if m.len() >= words {
                return Ok(m);
            }
            self.armed = Some(m);
        }
        let r = new_mem(words.max(CHUNK_WORDS));
        let flagged = crate::alloc::take_flag(crate::alloc::Role::Exec);
        match r {
            Ok(m) if !flagged => Ok(m),
            Ok(m) => {
                self.armed = Some(m);
                Err(OutOfMemory)
            }
            Err(e) => {
                if let Some(sp) = self.spare.take() {
                    self.armed = Some(sp);
                }
                Err(e)
            }
        }
    }

    /// GC の後に、使っていないチャンクを解放する。
    ///
    /// `live` は、生きているフレームのうち最も上にあるものの論理的な位置である。
    /// 生きているフレームがなければ [`NATIVE_CALLER`] を渡し、そのときは何もしない。
    /// `live` のフレームを含むチャンクと、その次の 1 つのチャンクを残し、それより後ろのチャンクを解放する。
    /// 次の 1 つを残すのは、チャンクの境界で呼び出しと戻りを繰り返すときに、そのたびにチャンクを確保し直すのを避けるためである。
    /// 次の 1 つが `CHUNK_WORDS` より小さいコピーチャンクで、後ろに既定のサイズ以上のチャンクがあれば、コピーチャンクを解放してそのチャンクを残す。
    /// ただし、使用中のチャンクが次の 1 つなら、置いたばかりのフレームがそこにあるかもしれないので、コピーチャンクのまま残す。
    /// 予備チャンクは解放しない。
    ///
    /// 使用中のチャンクを解放したときは、残した最後のチャンクを使用中のチャンクにして、範囲を書き直す。
    /// JIT のコードは戻るときにランタイムを呼ばないので、使用中のチャンクが最上段のフレームより後ろに残っていることがある。
    /// JIT のコードは、フレームを置く位置を決めるたびに範囲をコンテキストの `win` から読むので、書き直した範囲は次にフレームを置くときから有効になる。
    /// 置いたばかりでまだ最上段になっていないフレームは、`live` のフレームを含むチャンクかその次のチャンクにあり、どちらも残る。
    pub fn release_after_collect(&mut self, live: u64) {
        if self.chunks.is_empty() || live == NATIVE_CALLER {
            return;
        }
        let i = self.chunks.partition_point(|c| c.base <= live);
        let n = i.max(1);
        let keep = n + 1;
        if self.chunks.len() <= keep {
            return;
        }
        // 使用中のチャンクが次のチャンクなら、置いたばかりのフレームがそこにあるかもしれないので、コピーチャンクでも残す。
        let moved = self.cur != n && self.drop_copies_before_full(n);
        self.chunks.truncate(keep);
        if moved && self.cur > n {
            self.cur = n;
        }
        if self.cur >= keep {
            self.cur = keep - 1;
        }
        if moved || self.cur == keep - 1 {
            self.sync_win();
        }
    }

    /// チャンク `i` が [`CHUNK_WORDS`] より小さいコピーチャンクで、後ろに既定のサイズ以上のチャンクがあれば、その手前までのチャンクを解放する。
    ///
    /// チャンク `i` 以降がどれも使っていないチャンクであるときに呼ぶ。
    /// 解放したら、既定のサイズ以上のチャンクをチャンク `i` にし、その先頭の位置を前のチャンクの終わりにして、真を返す。
    /// 解放しなければ偽を返す。
    /// コピーチャンクを残して既定のサイズのチャンクを解放すると、次にチャンクの境界を越えるフレームを置くときにチャンクを確保し直すので、注入のたびにチャンクを確保することになる。
    /// この関数はチャンクの領域を解放するだけで、確保はしない。
    fn drop_copies_before_full(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.chunks.len() || self.chunks[i].mem.len() >= CHUNK_WORDS {
            return false;
        }
        let Some(k) = self.chunks[i + 1..]
            .iter()
            .position(|c| c.mem.len() >= CHUNK_WORDS)
        else {
            return false;
        };
        self.chunks.drain(i..i + 1 + k);
        let p = &self.chunks[i - 1];
        self.chunks[i].base = p.base + p.mem.len() as u64;
        true
    }

    /// 継続のコピー `mem` を、位置 `top`(現在のフレームの終わり)を含むチャンクの次のチャンクとしてつなぎ、その先頭の論理的な位置を返す。
    ///
    /// 継続に注入するときに、[`ValueStack::place`] でフレームを 1 つずつ配置し直す代わりに使う。
    /// コピーのフレームは最下段から順に隙間なく並んでいるので、チャンクの中の位置がそのままフレームの位置になる。
    /// ヘッダの 1 番と 3 番のワードは呼び出し元が書き直す。
    /// 使用中のチャンクの、`top` から後ろの残りは使わない。
    ///
    /// コピー領域をそのままチャンクにするので、チャンクの領域は確保しない。
    /// そのため、メモリ不足を注入するときにも使える。
    /// つなぐ前に次のチャンクだったチャンクは、コピーチャンクの次に移して残す。
    /// 巻き戻す間に実行される `cancel` 節のフレームがコピーチャンクに入らないときは、このチャンクに置く。
    /// つなぐ前の次のチャンクが、前の注入でつないだ `CHUNK_WORDS` より小さいコピーチャンクで、後ろに既定のサイズ以上のチャンクがあれば、コピーチャンクを解放してそのチャンクを移す。
    /// 移したチャンクより後ろにあったチャンクは解放する。
    ///
    /// 確保が必要になることがあるのは、チャンクの並びを拡張するときだけである。
    /// コピーチャンクをつなぐための拡張ができなければ、`mem` を返し、チャンクの並びを変えない。
    /// 次のチャンクを移すための拡張ができなければ、そのチャンクを解放する。
    pub fn link_copy(&mut self, top: u64, mem: Vec<u64>) -> Result<u64, (OutOfMemory, Vec<u64>)> {
        assert!(
            !self.chunks.is_empty() && !mem.is_empty(),
            "空の値スタックか空のコピーをつなごうとしました"
        );
        while self.cur > 0 && top < self.chunks[self.cur].base {
            self.cur -= 1;
        }
        let i = self.cur + 1;
        if self.chunks.len() <= i
            && self.chunks.len() == self.chunks.capacity()
            && crate::alloc::handled(|| self.chunks.try_reserve(1)).is_err()
        {
            self.sync_win();
            return Err((OutOfMemory, mem));
        }
        self.drop_copies_before_full(i);
        let base = {
            let c = &self.chunks[self.cur];
            c.base + c.mem.len() as u64
        };
        let next = (i < self.chunks.len()).then(|| self.chunks.swap_remove(i));
        self.chunks.truncate(i);
        let end = base + mem.len() as u64;
        self.chunks.push(Chunk { mem, base });
        if let Some(mut n) = next
            && crate::alloc::handled(|| self.chunks.try_reserve(1)).is_ok()
        {
            n.base = end;
            self.chunks.push(n);
        }
        self.cur = i;
        self.sync_win();
        Ok(base)
    }

    /// つないだチャンクの数を返す。
    /// テストと計測に使う。
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// 論理的な位置 `pos` のワードへのポインタを返す。
    /// `pos` は、つないだチャンクのどれかの中になければならない。
    ///
    /// `pos` が使用中のチャンクより前にあれば、使用中のチャンクをそこまで戻す。
    /// フレームを捨てて呼び出し元へ戻るときが、この場合である。
    /// 後ろにあれば、使用中のチャンクをそこまで進める。
    /// どのチャンクにも入らない位置は処理系のバグなので、リリースビルドでもパニックする。
    /// パニックするとスレッドが異常終了し、プロセスは終了コード 70 で終了する。
    #[inline]
    pub fn ptr_at(&mut self, pos: u64) -> *mut u64 {
        let old = self.cur;
        while pos < self.chunks[self.cur].base {
            self.cur -= 1;
        }
        while self.cur + 1 < self.chunks.len() && pos >= self.chunks[self.cur + 1].base {
            self.cur += 1;
        }
        if self.cur != old {
            self.sync_win();
        }
        let c = &mut self.chunks[self.cur];
        let off = (pos - c.base) as usize;
        assert!(
            off < c.mem.len(),
            "値スタックの位置 {pos} がどのチャンクにも入りません"
        );
        // SAFETY: off はチャンクの中にある。
        unsafe { c.mem.as_mut_ptr().add(off) }
    }

    /// 論理的な位置 `pos` のワードへのポインタを、使用中のチャンクを変えずに返す。
    /// GC がフレームをたどるときに使う。
    ///
    /// [`ValueStack::ptr_at`] は使用中のチャンクを動かし、使用中のチャンクは次の [`ValueStack::place`] がフレームを置く位置に影響するので、実行の途中の GC はこちらを使う。
    /// どのチャンクにも入らない位置は処理系のバグなので、パニックする。
    pub fn ptr_at_ro(&self, pos: u64) -> *const u64 {
        let i = self.chunks.partition_point(|c| c.base <= pos);
        assert!(i > 0, "値スタックの位置 {pos} がどのチャンクにも入りません");
        let c = &self.chunks[i - 1];
        let off = (pos - c.base) as usize;
        assert!(
            off < c.mem.len(),
            "値スタックの位置 {pos} がどのチャンクにも入りません"
        );
        // SAFETY: off はチャンクの中にある。
        unsafe { c.mem.as_ptr().add(off) }
    }

    /// 位置 `top`(現在のフレームの終わり)の後に `words` ワードのフレームを置く位置を決め、その論理的な位置を返す。
    ///
    /// フレームが使用中のチャンクに入れば、`top` に置く。
    /// 入らなければ、次のチャンクの先頭に置く。
    /// 次のチャンクが `CHUNK_WORDS` より小さいコピーチャンクで、後ろに既定のサイズ以上のチャンクがあれば、先にコピーチャンクを解放してそのチャンクを次のチャンクにする。
    /// 次のチャンクがないか、次のチャンクにフレームが入らなければ、新しいチャンクを確保して次のチャンクにする。
    /// 次のチャンクに置くときは、既存のチャンクを使う場合も新しく確保する場合も、フレームを置いたチャンクより後ろにあったチャンクを解放する。
    /// 使用中のチャンクに置くときは解放しない。
    ///
    /// 新しいチャンクを確保できなければ [`OutOfMemory`] を返す。
    /// このとき、置いてあるフレームは変わらない。
    /// ただし、使っていないコピーチャンクを解放していることと、予備チャンクが次に使うチャンクになっていることがある。
    pub fn place(&mut self, top: u64, words: usize) -> Result<u64, OutOfMemory> {
        if self.chunks.is_empty() {
            debug_assert_eq!(top, 0);
            if self.spare.is_none() && self.armed.is_none() {
                let sp = new_mem(CHUNK_WORDS);
                if crate::alloc::take_flag(crate::alloc::Role::Exec) {
                    self.armed = sp.ok();
                    return Err(OutOfMemory);
                }
                self.spare = Some(sp?);
            }
            let mem = self.take_mem(words)?;
            self.chunks.push(Chunk {
                mem: mem.into_vec(),
                base: 0,
            });
            self.cur = 0;
            self.sync_win();
            return Ok(0);
        }
        let old = self.cur;
        while self.cur > 0 && top < self.chunks[self.cur].base {
            self.cur -= 1;
        }
        if self.cur != old {
            self.sync_win();
        }
        {
            let c = &self.chunks[self.cur];
            if top >= c.base {
                let off = (top - c.base) as usize;
                if off + words <= c.mem.len() {
                    return Ok(top);
                }
            }
        }
        let next_base = {
            let c = &self.chunks[self.cur];
            c.base + c.mem.len() as u64
        };
        let i = self.cur + 1;
        self.drop_copies_before_full(i);
        if i < self.chunks.len() && self.chunks[i].mem.len() >= words {
            self.chunks[i].base = next_base;
            self.chunks.truncate(i + 1);
        } else {
            let mem = self.take_mem(words)?;
            self.chunks.truncate(i);
            self.chunks.push(Chunk {
                mem: mem.into_vec(),
                base: next_base,
            });
        }
        self.cur = i;
        self.sync_win();
        Ok(next_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 範囲 `win` が、使用中のチャンク `chunks[cur]` と一致することを確かめる。
    /// JIT のコードはこの性質に依存する。
    fn check_win(s: &ValueStack) {
        if s.chunks.is_empty() {
            return;
        }
        let c = &s.chunks[s.cur];
        assert_eq!(s.win.lo, c.base);
        assert_eq!(s.win.hi, c.base + c.mem.len() as u64);
        assert_eq!(s.win.ptr, c.mem.as_ptr() as usize);
    }

    #[test]
    fn 入らないフレームは次の塊に置き戻ると前の塊に戻る() {
        let mut s = ValueStack::new();
        assert_eq!(s.chunk_count(), 0);
        let a = s.place(0, 10).unwrap();
        check_win(&s);
        assert_eq!(a, 0);
        assert_eq!(s.chunk_count(), 1);
        let b = s.place(10, CHUNK_WORDS - 5).unwrap();
        check_win(&s);
        assert_eq!(b, CHUNK_WORDS as u64);
        assert_eq!(s.chunk_count(), 2);
        let p = s.ptr_at(b);
        check_win(&s);
        // SAFETY: p はチャンクの中を指す。
        unsafe { *p = 7 };
        let q = s.ptr_at(a);
        check_win(&s);
        // SAFETY: q はチャンクの中を指す。
        unsafe { *q = 3 };
        // チャンク 0 に戻ったので、次のフレームはチャンク 0 の続きに置ける。
        assert_eq!(s.place(10, 10), Ok(10));
        check_win(&s);
        // 既定のサイズに入らないフレームには、そのフレームが入るサイズのチャンクを作る。
        let big = s.place(20, CHUNK_WORDS * 3).unwrap();
        check_win(&s);
        assert_eq!(big, CHUNK_WORDS as u64);
        let p = s.ptr_at(big + (CHUNK_WORDS * 3 - 1) as u64);
        check_win(&s);
        // SAFETY: p はチャンクの中を指す。
        unsafe { *p = 1 };
    }

    #[test]
    fn 塊を取れなければ値スタックを変えずに失敗を返す() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert_eq!(s.chunk_count(), 0);
        assert_eq!(s.place(0, 10), Ok(0));
        check_win(&s);
        assert_eq!(s.place(10, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert_eq!(s.chunk_count(), 1);
        assert_eq!(s.place(10, 10), Ok(10));
        check_win(&s);
    }

    #[test]
    fn 塊を取れなかった後は予備の塊にフレームを置ける() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, 10), Ok(0));
        check_win(&s);
        assert!(s.spare.is_some());
        assert_eq!(s.place(10, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert!(s.armed.is_some() && s.spare.is_none());
        // 使用中のチャンクに入らないフレームは、予備チャンクに置く。
        assert_eq!(s.place(10, CHUNK_WORDS), Ok(CHUNK_WORDS as u64));
        check_win(&s);
        assert!(s.armed.is_none());
        assert_eq!(s.chunk_count(), 2);
    }

    /// 既定のチャンクの半分より 1 ワード大きいフレームを `n` 個置き、各フレームの位置を返す。
    ///
    /// フレームは 1 つのチャンクに 1 個しか入らないので、フレームごとにチャンクが 1 つ増える。
    fn descend(s: &mut ValueStack, n: usize) -> Vec<u64> {
        let fw = CHUNK_WORDS / 2 + 1;
        let mut top = 0;
        let mut ps = Vec::new();
        for _ in 0..n {
            let p = s.place(top, fw).unwrap();
            check_win(s);
            ps.push(p);
            top = p + fw as u64;
        }
        ps
    }

    #[test]
    fn 深く置いて戻った後の回収で塊を返す() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 20);
        assert_eq!(s.chunk_count(), 20);
        // インタプリタの戻りと同じように最下段のフレームに戻ってから、GC の後のチャンクの解放を行う。
        s.ptr_at(ps[0]);
        check_win(&s);
        s.release_after_collect(ps[0]);
        check_win(&s);
        assert!(s.chunk_count() <= 2);
        assert!(s.spare.is_some());
        // 残した次のチャンクは再利用する。
        assert_eq!(s.place(ps[0] + 10, CHUNK_WORDS), Ok(CHUNK_WORDS as u64));
        check_win(&s);
        assert_eq!(s.chunk_count(), 2);
    }

    #[test]
    fn 使っている塊が最上段より後ろに残っていても回収で塊を返す() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 20);
        // JIT のコードの戻りは使用中のチャンクを動かさない。
        // 最上段はチャンク 4 のフレームとする。
        assert_eq!(s.cur, 19);
        s.release_after_collect(ps[4]);
        check_win(&s);
        assert_eq!(s.chunk_count(), 6);
        assert_eq!(s.cur, 5);
        // 最上段とその上の位置には書ける。
        // SAFETY: どちらも残したチャンクの中を指す。
        unsafe {
            *s.ptr_at(ps[4]) = 1;
            *s.ptr_at(ps[5]) = 2;
        }
        check_win(&s);
        // 最上段の上にフレームを置ける。
        let fw = CHUNK_WORDS / 2 + 1;
        assert_eq!(s.place(ps[4] + fw as u64, fw), Ok(ps[5]));
        check_win(&s);
        // 生きているフレームがなければ何もしない。
        s.release_after_collect(NATIVE_CALLER);
        assert_eq!(s.chunk_count(), 6);
    }

    #[test]
    fn 継続の写しを塊としてつなぎ次の塊を後ろに残す() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 3);
        // チャンク 0 のフレームの上に、チャンク 0 の残りより小さいコピーをつなぐ。
        s.ptr_at(ps[0]);
        let fw = (CHUNK_WORDS / 2 + 1) as u64;
        let copy: Vec<u64> = (0..100).collect();
        let at = s.link_copy(ps[0] + fw, copy).unwrap();
        check_win(&s);
        assert_eq!(at, CHUNK_WORDS as u64);
        assert_eq!(s.cur, 1);
        // つなぐ前のチャンク 1 はコピーチャンクの次に移り、チャンク 2 は解放される。
        assert_eq!(s.chunk_count(), 3);
        assert_eq!(s.chunks[2].base, at + 100);
        // SAFETY: コピーチャンクの中を指す。
        assert_eq!(unsafe { *s.ptr_at(at + 99) }, 99);
        // コピーチャンクに入るフレームはそこに置き、入らないフレームは移した次のチャンクに置く。
        assert_eq!(s.place(at + 90, 10), Ok(at + 90));
        assert_eq!(s.place(at + 90, 11), Ok(at + 100));
        check_win(&s);
        assert_eq!(s.chunk_count(), 3);
        // 戻るとチャンク 0 の位置を読める。
        // SAFETY: チャンク 0 の中を指す。
        unsafe { *s.ptr_at(ps[0]) = 5 };
        check_win(&s);
    }

    #[test]
    fn 注入を繰り返しても境目を越えるフレームは既定の大きさの塊を使い直す() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, CHUNK_WORDS - 5), Ok(0));
        let top = (CHUNK_WORDS - 5) as u64;
        assert_eq!(s.place(top, 10), Ok(CHUNK_WORDS as u64));
        let full = s.chunks[1].mem.as_ptr();
        for r in 0..100 {
            // チャンク 0 のフレームの上に 100 ワードのコピーをつなぎ、継続のフレームが戻った後に、境界を越えるフレームを置く。
            s.ptr_at(0);
            let at = s.link_copy(top, (0..100).collect()).unwrap();
            check_win(&s);
            assert_eq!(at, CHUNK_WORDS as u64);
            assert_eq!(s.chunk_count(), 3);
            s.ptr_at(0);
            check_win(&s);
            if r % 3 == 2 {
                // 生きているフレームがチャンク 0 にあるときに GC の後の解放を行うと、コピーチャンクではなく既定のサイズのチャンクが残る。
                s.release_after_collect(0);
                check_win(&s);
                assert_eq!(s.chunk_count(), 2);
                assert_eq!(s.chunks[1].mem.as_ptr(), full);
                assert_eq!(s.chunks[1].base, CHUNK_WORDS as u64);
            }
            // コピーチャンクに入るフレームも入らないフレームも、既定のサイズのチャンクに置く。
            let first = if r % 2 == 0 { 10 } else { 200 };
            assert_eq!(s.place(top, first), Ok(CHUNK_WORDS as u64));
            let b = s.place(CHUNK_WORDS as u64 + first as u64, 95).unwrap();
            assert_eq!(b, CHUNK_WORDS as u64 + first as u64);
            check_win(&s);
            assert_eq!(s.chunk_count(), 2);
            // チャンクを確保し直していない。
            assert_eq!(s.chunks[1].mem.as_ptr(), full);
        }
    }

    #[test]
    fn 次の塊が無い所に写しをつなぐと入らないフレームは予備の塊に置く() {
        let mut s = ValueStack::new();
        let ps = descend(&mut s, 2);
        let fw = (CHUNK_WORDS / 2 + 1) as u64;
        let top = ps[1] + fw;
        // チャンクを確保できずに失敗して、予備チャンクが次に使うチャンクになる(通常の `resume` の失敗に相当する)。
        assert_eq!(s.place(top, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        let sp = s.armed.as_ref().unwrap().as_ptr();
        assert_eq!(s.chunk_count(), 2);
        // 最上段を含む最後のチャンクにコピーをつなぐと、コピーチャンクの後ろにはチャンクがない。
        let at = s.link_copy(top, (0..100).collect()).unwrap();
        check_win(&s);
        assert_eq!(at, 2 * CHUNK_WORDS as u64);
        assert_eq!(s.chunk_count(), 3);
        assert_eq!(s.cur, 2);
        // コピーチャンクに入らないフレームは、新しく確保せずに予備チャンクに置く。
        assert_eq!(s.place(at + 90, 55), Ok(at + 100));
        check_win(&s);
        assert!(s.armed.is_none() && s.spare.is_none());
        assert_eq!(s.chunk_count(), 4);
        assert_eq!(s.chunks[3].mem.as_ptr(), sp);
    }

    #[test]
    fn 範囲は次の塊の使い直しと戻った後の失敗でも使っている塊と一致する() {
        let mut s = ValueStack::new();
        assert_eq!(s.place(0, 10), Ok(0));
        check_win(&s);
        let b = s.place(10, CHUNK_WORDS - 5).unwrap();
        check_win(&s);
        assert_eq!(s.chunk_count(), 2);
        // チャンク 0 に戻ってから入らないフレームを置くと、残っているチャンク 1 を再利用する。
        s.ptr_at(0);
        check_win(&s);
        assert_eq!(s.place(10, CHUNK_WORDS - 5), Ok(b));
        check_win(&s);
        assert_eq!(s.chunk_count(), 2);
        // チャンク 1 からチャンク 0 の位置に戻り、置けずに失敗しても、範囲は戻った先のチャンク 0 を指す。
        assert_eq!(s.place(10, usize::MAX / 4), Err(OutOfMemory));
        check_win(&s);
        assert_eq!(s.cur, 0);
        // 再利用したチャンクにもう一度進む。
        s.ptr_at(b);
        check_win(&s);
        assert_eq!(s.cur, 1);
    }
}
