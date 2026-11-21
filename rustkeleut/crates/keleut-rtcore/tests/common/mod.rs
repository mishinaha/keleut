//! テストで共用する関数。並列に実行する map と、Node での検査に渡すファイルの書き出しである。

use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// `items` の各要素に `f` を適用し、結果を元の順序で返す。
/// 要素は複数のスレッドに分けて処理する。
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
            .flat_map(|h| h.join().expect("テストのスレッドが異常終了した"))
            .collect()
    })
}

/// `bytes` を `<target>/rtcore/<name>` に書き、そのパスを返す。
/// 一時ファイルに書いてから、名前を `<name>` に変える。
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

/// f64 のビット列(`u64`)を、リトルエンディアンのバイト列にして連結する。
pub fn bits_le(xs: impl IntoIterator<Item = u64>) -> Vec<u8> {
    xs.into_iter().flat_map(u64::to_le_bytes).collect()
}

/// バイト列の FNV-1a ハッシュ(64 ビット)を返す。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Node での検査に渡すファイルがどのビルドの結果かを示す、JSON のフィールドの並びを返す。
///
/// `rt.wasm` の FNV-1a、profile、ターゲットトリプルを書く。
/// `tests/wasm_check.mjs` は `<target>/rtcore/` の `rt.wasm` から FNV-1a を計算し直し、この値と比較する。
pub fn provenance_json() -> String {
    format!(
        "\"rt_wasm_fnv1a64\": \"{:016x}\", \"profile\": \"{}\", \"target\": \"{}\"",
        fnv1a64(keleut_rtcore::RT_WASM),
        keleut_rtcore::BUILD_PROFILE,
        keleut_rtcore::BUILD_TARGET
    )
}
