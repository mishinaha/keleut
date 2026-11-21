//! グローバルアロケータのラッパ([`keleut_rt::alloc`])のテスト。
//!
//! ラッパは、`#[global_allocator]` でグローバルアロケータとして宣言したバイナリの中でしか有効にならないので、このテストターゲットで宣言する。
//! 予備領域、スレッドの役割ごとのフラグ、テスト用の失敗の指定は、どれもプロセス全体で共有するので、テストはロックを取って 1 つずつ実行する。

use std::sync::Mutex;

use keleut_rt::alloc::{self, Role, Site};
use keleut_rt::heap::{Heap, MARK};

#[global_allocator]
static ALLOC: alloc::Wrap = alloc::Wrap;

static LOCK: Mutex<()> = Mutex::new(());

/// ロックを取り、起動時の処理を済ませ、現在のスレッドを実行スレッド(役割 `exec`)にする。
///
/// 前のテストの影響を消すために、テスト用の失敗の指定を空にし、予備領域を確保し直し、実行スレッドのフラグを下ろす。
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    alloc::init().unwrap();
    alloc::set_role(Role::Exec);
    alloc::configure("").unwrap();
    alloc::reacquire();
    while alloc::take_flag(Role::Exec) {}
    g
}

#[test]
fn 範囲の中の失敗は予備に触れず印も立てない() {
    let _g = setup();
    alloc::configure("exec:collect:hard:0").unwrap();
    assert!(alloc::has_reserve());
    let r = {
        let _s = alloc::site(Site::Collect);
        let mut v: Vec<u64> = Vec::new();
        alloc::handled(|| v.try_reserve(1024))
    };
    assert!(r.is_err());
    assert_eq!(alloc::injected_count(), 1);
    assert!(alloc::has_reserve());
    assert!(!alloc::flagged(Role::Exec));
    alloc::configure("").unwrap();
}

#[test]
fn 範囲の外の_soft_の失敗は予備を返して印を立て確保をやり直す() {
    let _g = setup();
    alloc::configure("exec:rec_fields:soft:1").unwrap();
    let v = {
        let _s = alloc::site(Site::RecFields);
        let a: Vec<u64> = Vec::with_capacity(8);
        assert!(alloc::has_reserve(), "1 回目は失敗させない");
        let b: Vec<u64> = Vec::with_capacity(16);
        (a, b)
    };
    assert_eq!(v.1.capacity(), 16);
    assert_eq!(alloc::injected_count(), 1);
    assert!(!alloc::has_reserve());
    assert!(alloc::take_flag(Role::Exec));
    assert!(!alloc::take_flag(Role::Exec), "フラグは 1 回だけ応じる");
    // `soft` の指定は 1 回だけ失敗させるので、次の確保は失敗しない。
    let _s = alloc::site(Site::RecFields);
    let c: Vec<u64> = Vec::with_capacity(32);
    assert_eq!(c.capacity(), 32);
    assert!(!alloc::flagged(Role::Exec));
    assert!(alloc::reacquire());
    assert!(alloc::has_reserve());
    alloc::configure("").unwrap();
}

/// 実行スレッドが予備領域を使った後でも、コンパイルスレッドの確保が失敗すると、ラッパは予備領域を確保し直してから確保をやり直す。
/// やり直した後に、もう一度予備領域を確保し直す。
#[test]
fn 予備が無いときの翻訳の役割の失敗は予備を取り直してやり直す() {
    let _g = setup();
    alloc::configure("exec:rec_fields:soft:0,jit:jit:soft:0").unwrap();
    {
        let _s = alloc::site(Site::RecFields);
        let a: Vec<u64> = Vec::with_capacity(8);
        assert_eq!(a.capacity(), 8);
    }
    assert!(!alloc::has_reserve(), "実行スレッドが予備領域を使った");
    assert!(alloc::take_flag(Role::Exec));
    alloc::set_role(Role::Jit);
    let b = {
        let _s = alloc::site(Site::Jit);
        let b: Vec<u64> = Vec::with_capacity(16);
        b
    };
    alloc::set_role(Role::Exec);
    assert_eq!(b.capacity(), 16);
    assert_eq!(alloc::injected_count(), 2);
    assert!(
        alloc::take_flag(Role::Jit),
        "コンパイルスレッドのフラグが立つ"
    );
    assert!(alloc::has_reserve(), "やり直しの後に予備領域を確保し直す");
    assert!(!alloc::flagged(Role::Exec));
    alloc::configure("").unwrap();
}

#[test]
fn 役割と場所の違う確保は失敗させない() {
    let _g = setup();
    alloc::configure("jit:collect:hard:0,exec:chunk:hard:0").unwrap();
    {
        let _s = alloc::site(Site::Collect);
        let mut v: Vec<u64> = Vec::new();
        assert!(alloc::handled(|| v.try_reserve(16)).is_ok());
    }
    let mut v: Vec<u64> = Vec::new();
    assert!(alloc::handled(|| v.try_reserve(16)).is_ok());
    assert!(alloc::has_reserve());
    alloc::configure("").unwrap();
}

#[test]
fn 回収の_keep_の確保が失敗しても掃除は続く() {
    let _g = setup();
    let mut h = Heap::new();
    // スイープで残すオブジェクトと解放させるオブジェクトを交互に作る。
    // 種類は 1 で、ヘッダにマークを立てたものがスイープの後に残る。
    let mut boxes = Vec::new();
    for i in 0..10_000 {
        let p = h.try_alloc(4).unwrap();
        // SAFETY: p は 4 ワードの新しい領域である。
        unsafe {
            *p = 1 | if i % 2 == 0 { MARK } else { 0 };
            *p.add(1) = 0;
            *p.add(2) = 0;
            *p.add(3) = 0;
        }
        boxes.push(p);
    }
    alloc::configure("exec:collect:hard:0").unwrap();
    let st = {
        let _s = alloc::site(Site::Collect);
        h.sweep()
    };
    assert!(alloc::injected_count() >= 1, "keep の確保に当たる");
    alloc::configure("").unwrap();
    assert!(alloc::has_reserve());
    assert!(!alloc::flagged(Role::Exec));
    assert_eq!(st.live_words, 5_000 * 4);
    for (i, p) in boxes.iter().enumerate() {
        // SAFETY: 残したオブジェクトはスイープの後もオブジェクトのままで、マークだけが消える。
        let hd = unsafe { **p };
        if i % 2 == 0 {
            assert_eq!(hd, 1, "生かしたオブジェクトのマークが消える");
        }
    }
    // スイープの後も確保できる。
    assert!(h.try_alloc(4).is_ok());
}

#[test]
fn 指定の誤りは文面を返す() {
    let _g = setup();
    for s in [
        "exec",
        "exec:collect:soft",
        "user:collect:soft:0",
        "exec:nosuch:soft:0",
        "exec:collect:maybe:0",
        "exec:collect:soft:x",
    ] {
        assert!(alloc::configure(s).is_err(), "{s}");
    }
    assert!(alloc::configure("exec:collect:soft:0,conv:kir:hard:3").is_ok());
    alloc::configure("").unwrap();
}
