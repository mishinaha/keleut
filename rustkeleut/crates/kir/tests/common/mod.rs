//! 試験の共通の部品。

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// `tests/fixtures/` のパス。
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// ディレクトリの中の `.kir` と、同じ名前の `.kirt` の組を名前の順に返す。
pub fn pairs(dir: &Path) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "kir"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|p| {
            let kir = std::fs::read(&p).unwrap();
            let kirt = std::fs::read(p.with_extension("kirt")).unwrap();
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                kir,
                kirt,
            )
        })
        .collect()
}

/// 往復の性質をすべて確かめ、違えば理由を返す。
pub fn roundtrip(kir: &[u8], kirt: &[u8]) -> Result<(), String> {
    let p = kir::read_kir(kir).map_err(|e| format!("read_kir: {e}"))?;
    kir::verify(&p).map_err(|e| format!("verify: {e}"))?;
    let b = kir::write_kir(&p).map_err(|e| format!("write_kir: {e}"))?;
    if b != kir {
        return Err("バイナリ → 型 → バイナリが元と違います".into());
    }
    let t = kir::write_kirt(&p).map_err(|e| format!("write_kirt: {e}"))?;
    if t != kirt {
        let line = t
            .split(|&c| c == b'\n')
            .zip(kirt.split(|&c| c == b'\n'))
            .position(|(a, b)| a != b)
            .map_or(0, |i| i + 1);
        return Err(format!(
            "バイナリ → テキストが Diktor のテキストと違います({line} 行目)"
        ));
    }
    let q = kir::read_kirt(kirt).map_err(|e| format!("read_kirt: {e}"))?;
    let b2 = kir::write_kir(&q).map_err(|e| format!("write_kir(text): {e}"))?;
    if b2 != kir {
        return Err("テキスト → バイナリが Diktor のバイナリと違います".into());
    }
    let t2 = kir::write_kirt(&q).map_err(|e| format!("write_kirt(text): {e}"))?;
    if t2 != kirt {
        return Err("テキスト → テキストが元と違います".into());
    }
    Ok(())
}

/// 節を切り出す。見出しの後の (id, 中身) の列。
pub fn sections(bin: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 16;
    while pos < bin.len() {
        let id = bin[pos];
        pos += 1;
        let (mut size, mut shift) = (0usize, 0);
        loop {
            let b = bin[pos];
            pos += 1;
            size |= usize::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        out.push((id, bin[pos..pos + size].to_vec()));
        pos += size;
    }
    out
}

pub fn assemble(head: &[u8], secs: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = head[..16].to_vec();
    for (id, payload) in secs {
        out.push(*id);
        let mut n = payload.len();
        loop {
            if n < 0x80 {
                out.push(n as u8);
                break;
            }
            out.push((n & 0x7f) as u8 | 0x80);
            n >>= 7;
        }
        out.extend_from_slice(payload);
    }
    out
}
