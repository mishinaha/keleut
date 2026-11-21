//! バイナリの読み込みの検査(IRSpec 13.1 節、13.3 節、13.7 節)のテスト。
//!
//! 診断メッセージは Diktor のリーダ(`ir_bin.ml`)と同じである。
//! 診断は、ヘッダを `見出し`、セクションを `節`、バージョンを `版`、メジャーバージョンを `主版`、必須機能ビットを `必要な機能のビット` と書く。

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
    // メジャーバージョンが違えば、マイナーバージョンによらず拒否する(IRSpec 14 節)。
    let mut x = b.clone();
    x[4] = 0;
    x[6] = 3;
    assert_eq!(
        err(&x),
        "位置 8: 版 0.3 は読めません(読めるのは主版 1 の版だけです)"
    );
    let mut x = b.clone();
    x[4] = 2;
    assert_eq!(
        err(&x),
        "位置 8: 版 2.0 は読めません(読めるのは主版 1 の版だけです)"
    );
    // メジャーバージョンが同じなら、マイナーバージョンが新しいファイルも読み、書き出すときもそのマイナーバージョンを書く。
    let mut x = b.clone();
    x[6] = 0xff;
    x[7] = 0xff;
    let p = kir::read_kir(&x).unwrap();
    assert_eq!(p.version, (1, 0xffff));
    assert_eq!(kir::write_kir(&p).unwrap(), x);
    // マイナーバージョンが新しくても、未知の必須機能ビットは拒否する。
    let mut x = b.clone();
    x[6] = 1;
    x[11] = 0x80;
    assert_eq!(
        err(&x),
        "位置 12: 必要な機能のビット 0x80000000 を処理できません"
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
    // 末尾に 0 のバイトを追加する。定義されていない id 0x00 のセクションの始まりになる。
    let mut x = b.clone();
    x.push(0);
    assert_eq!(
        err(&x),
        format!("位置 {}: 節 0x00 は定義されていません", b.len() + 1)
    );
    // 定義されていない id 0x0C のセクションを追加する。
    let mut s = secs.clone();
    s.push((0x0C, vec![]));
    let mut x = common::assemble(&b, &s);
    x[12] = 12;
    assert!(err(&x).contains("節 0x0c は定義されていません"));
    // セクション数がヘッダの値と違う。
    let mut x = b.clone();
    x[12] = 12;
    assert!(err(&x).contains("節の数が見出し(12)と違います(11)"));
    // 必須のセクションがない。
    let mut s = secs.clone();
    s.remove(3);
    let mut x = common::assemble(&b, &s);
    x[12] = 10;
    assert!(err(&x).contains("必須の節 0x04 がありません"));
    // セクションが id の昇順に並んでいない。
    let mut s = secs.clone();
    s.swap(3, 4);
    let x = common::assemble(&b, &s);
    assert!(err(&x).contains("id の昇順に並んでいません"));
    // セクションのサイズがファイルの残りを超える。
    let mut x = b.clone();
    x.truncate(x.len() - 1);
    assert!(err(&x).contains("の大きさがファイルの残りを超えます"));
}

#[test]
fn skippable_section_and_padded_leb() {
    let b = base();
    let canonical = kir::write_kir(&kir::read_kir(&b).unwrap()).unwrap();
    assert_eq!(canonical, b);
    // ヘッダのセクション数に含めた id 0x80 のセクションはスキップする。
    let mut s = common::sections(&b);
    s.push((0x80, b"anything".to_vec()));
    let mut x = common::assemble(&b, &s);
    x[12] = 12;
    let p = kir::read_kir(&x).unwrap();
    assert_eq!(kir::write_kir(&p).unwrap(), b);
    // ヘッダのセクション数に含めていない id 0x80 のセクションは拒否する。
    let x = common::assemble(&b, &s);
    assert!(err(&x).contains("節の数が見出し(11)と違います(12)"));
    // セクションのサイズを最短でない 5 バイトの LEB128 で書いても受理する。
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
    // 6 バイトの LEB128 は拒否する。
    let mut y = b[..16].to_vec();
    y.push(s[0].0);
    y.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00]);
    assert!(err(&y).contains("uLEB128 が u32 の範囲を超えます"));
}

#[test]
fn tables() {
    let b = base();
    let s = common::sections(&b);
    // strings の表の 2 つの項目を入れ替えると、厳密な昇順でなくなる。
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
    // どこからも使われない項目を追加する。
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
