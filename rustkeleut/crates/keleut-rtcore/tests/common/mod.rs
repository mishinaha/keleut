//! 試験の共通の部品: 並列の写像と、node の確認へ渡すファイルの書き出し。

use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// `items` の各要素に `f` を適用した結果を、元の順に返す(スレッドで分ける)。
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|c| s.spawn(|| c.iter().map(&f).collect::<Vec<R>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("試験のスレッドが落ちた"))
            .collect()
    })
}

/// `<target>/rtcore/<name>` に書く(一時のファイルに書いてから名前を替える)。
pub fn stage_write(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(keleut_rtcore::STAGE_DIR);
    fs::create_dir_all(&dir).expect("create stage dir");
    let path = dir.join(name);
    let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
    let mut f = fs::File::create(&tmp).expect("create stage file");
    f.write_all(bytes).expect("write stage file");
    drop(f);
    fs::rename(&tmp, &path).expect("rename stage file");
    path
}

/// f64 のビット列をリトルエンディアンで並べる。
pub fn bits_le(xs: impl IntoIterator<Item = u64>) -> Vec<u8> {
    xs.into_iter().flat_map(u64::to_le_bytes).collect()
}

/// バイト列の FNV-1a(64 ビット)。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// node の確認へ渡すファイルがどの組み方の結果かを示す、JSON のフィールドの並び。
///
/// `rt.wasm` の FNV-1a、profile、対象を書く。
/// `tests/wasm_check.mjs` は置き場の `rt.wasm` から FNV-1a を計算し直し、これと比べる。
pub fn provenance_json() -> String {
    format!(
        "\"rt_wasm_fnv1a64\": \"{:016x}\", \"profile\": \"{}\", \"target\": \"{}\"",
        fnv1a64(keleut_rtcore::RT_WASM),
        keleut_rtcore::BUILD_PROFILE,
        keleut_rtcore::BUILD_TARGET
    )
}
