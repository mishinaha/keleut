//! バイナリの読み込みの検査(IRSpec 13.1 節、13.3 節、13.7 節)の試験。
//!
//! 文面は Diktor の読み手(`ir_bin.ml`)と同じである。

mod common;

use kir::ErrorKind;

fn base() -> Vec<u8> {
    std::fs::read(common::fixtures().join("ex_loop.kir")).unwrap()
}

fn err(b: &[u8]) -> String {
    let e = kir::read_kir(b).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Malformed, "{e}");
    assert_eq!(e.exit_code(), 65);
    e.message().to_string()
}

#[test]
fn header() {
    let b = base();
    assert_eq!(err(&b[..10]), "位置 0: 見出しの 16 バイトがありません");
    let mut x = b.clone();
    x[1] = 1;
    assert_eq!(err(&x), "位置 0: マジックが KIR のものではありません");
    let mut x = b.clone();
    x[6] = 2;
    assert_eq!(
        err(&x),
        "位置 8: 版 0.2 は読めません(読めるのは 0.3 だけです)"
    );
    let mut x = b.clone();
    x[4] = 1;
    assert_eq!(
        err(&x),
        "位置 8: 版 1.3 は読めません(読めるのは 0.3 だけです)"
    );
    let mut x = b.clone();
    x[8] = 1;
    assert_eq!(
        err(&x),
        "位置 12: 必要な機能のビット 0x00000001 を処理できません"
    );
}

#[test]
fn sections() {
    let b = base();
    let secs = common::sections(&b);
    assert_eq!(secs.len(), 11);
    // 後ろに 0 のバイト(定義しない id 0x00 の節の始まり)
    let mut x = b.clone();
    x.push(0);
    assert_eq!(
        err(&x),
        format!("位置 {}: 節 0x00 は定義されていません", b.len() + 1)
    );
    // 定義しない id 0x0C
    let mut s = secs.clone();
    s.push((0x0C, vec![]));
    let mut x = common::assemble(&b, &s);
    x[12] = 12;
    assert!(err(&x).contains("節 0x0c は定義されていません"));
    // 節の数が見出しと違う
    let mut x = b.clone();
    x[12] = 12;
    assert!(err(&x).contains("節の数が見出し(12)と違います(11)"));
    // 必須の節が無い
    let mut s = secs.clone();
    s.remove(3);
    let mut x = common::assemble(&b, &s);
    x[12] = 10;
    assert!(err(&x).contains("必須の節 0x04 がありません"));
    // 昇順でない
    let mut s = secs.clone();
    s.swap(3, 4);
    let x = common::assemble(&b, &s);
    assert!(err(&x).contains("id の昇順に並んでいません"));
    // 節の大きさがファイルの残りを超える
    let mut x = b.clone();
    x.truncate(x.len() - 1);
    assert!(err(&x).contains("の大きさがファイルの残りを超えます"));
}

#[test]
fn skippable_section_and_padded_leb() {
    let b = base();
    let canonical = kir::write_kir(&kir::read_kir(&b).unwrap()).unwrap();
    assert_eq!(canonical, b);
    // 数えた 0x80 の節は読み飛ばす
    let mut s = common::sections(&b);
    s.push((0x80, b"anything".to_vec()));
    let mut x = common::assemble(&b, &s);
    x[12] = 12;
    let p = kir::read_kir(&x).unwrap();
    assert_eq!(kir::write_kir(&p).unwrap(), b);
    // 数えない 0x80 の節は拒否する
    let x = common::assemble(&b, &s);
    assert!(err(&x).contains("節の数が見出し(11)と違います(12)"));
    // 節の大きさの冗長な LEB128 は受け付ける
    let s = common::sections(&b);
    let mut x = b[..16].to_vec();
    for (id, payload) in &s {
        x.push(*id);
        let n = payload.len();
        x.extend_from_slice(&[
            (n & 0x7f) as u8 | 0x80,
            ((n >> 7) & 0x7f) as u8 | 0x80,
            ((n >> 14) & 0x7f) as u8 | 0x80,
            ((n >> 21) & 0x7f) as u8 | 0x80,
            ((n >> 28) & 0x0f) as u8,
        ]);
        x.extend_from_slice(payload);
    }
    let p = kir::read_kir(&x).unwrap();
    assert_eq!(kir::write_kir(&p).unwrap(), b);
    // 6 バイトの LEB128 は拒否する
    let mut y = b[..16].to_vec();
    y.push(s[0].0);
    y.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]);
    assert!(err(&y).contains("uLEB128 が u32 の範囲を超えます"));
}

#[test]
fn tables() {
    let b = base();
    let s = common::sections(&b);
    // strings の表の 2 つの項目を入れ替えると、厳密な昇順でなくなる
    let p = kir::read_kir(&b).unwrap();
    let mut strs: Vec<Vec<u8>> = Vec::new();
    {
        let payload = &s[0].1;
        let mut pos = 0;
        let n = payload[pos] as usize;
        pos += 1;
        assert!(n < 128);
        for _ in 0..n {
            let len = payload[pos] as usize;
            pos += 1;
            strs.push(payload[pos..pos + len].to_vec());
            pos += len;
        }
    }
    assert!(strs.len() > 2);
    let mut swapped = strs.clone();
    swapped.swap(0, 1);
    let mut payload = vec![swapped.len() as u8];
    for x in &swapped {
        payload.push(x.len() as u8);
        payload.extend_from_slice(x);
    }
    let mut s2 = s.clone();
    s2[0].1 = payload;
    let x = common::assemble(&b, &s2);
    let m = err(&x);
    assert!(m.contains("stringsの表が厳密な昇順に並んでいません"), "{m}");
    // 使われない項目を足す
    let mut extra = strs.clone();
    extra.push(b"~~~unused".to_vec());
    let mut payload = vec![extra.len() as u8];
    for x in &extra {
        payload.push(x.len() as u8);
        payload.extend_from_slice(x);
    }
    let mut s3 = s.clone();
    s3[0].1 = payload;
    let x = common::assemble(&b, &s3);
    assert!(err(&x).contains(&format!(
        "strings の項目 {} がどこからも使われていません",
        strs.len()
    )));
    drop(p);
}
