//! KIR のフィクスチャとコーパスの全件から WasmGC のコードを生成し、wasmparser の検証と trap の検査に通す。
//!
//! - `crates/kir/tests/fixtures/*.kir` は常に検査する。
//! - 環境変数 `KELEUT_DIKTOR` に Diktor の実行ファイルを指定すると、`conformance/corpus/` の各 `.kel` を
//!   `diktor --emit-ir` で KIR にし、`diktor` が終了コード 0 で終了したものすべてについて、コードを生成して検査する。
//!   `diktor` には、`index.tsv` のその行に書いてあるオプションを渡す。
//!   オプションの列が `?` ならオプションなしで試し、終了コード 1 で終了したら `--no-prelude` を付けてやり直す。
//!   コード生成の統計(大きなフレーム方式にした関数の個数、フレーム 1 段の見積もりバイト数の最大値など)を標準エラーに書く。

use std::path::{Path, PathBuf};
use std::process::Command;

use keleut_wasm::{Options, Stats, check, compile};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// KIR のバイト列からコードを生成して検証と trap の検査に通し、統計と `program.wasm` のバイト数を返す。
fn translate(path: &Path, bytes: &[u8]) -> Result<(Stats, usize), String> {
    let k = kir::load(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let p = keleut_lir::lower(&k).map_err(|e| format!("{}: {e}", path.display()))?;
    let out = compile(&p, &Options { max_depth: 1 << 20 })
        .map_err(|e| format!("{}: {e}", path.display()))?;
    check::validate(&out.program).map_err(|e| format!("{}: {e}", path.display()))?;
    check::trap_check(&out.program).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((out.stats, out.program.len()))
}

#[test]
fn fixture_をすべて翻訳して検査する() {
    let dir = root().join("../kir/tests/fixtures");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let path = e.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) == Some("kir") {
            translate(&path, &std::fs::read(&path).unwrap()).unwrap();
            n += 1;
        }
    }
    assert!(n >= 16);
}

#[test]
fn コーパスの全件を_diktor_で書き出して翻訳する() {
    let Ok(diktor) = std::env::var("KELEUT_DIKTOR") else {
        return;
    };
    let corpus = root().join("../../conformance/corpus");
    let index = std::fs::read_to_string(corpus.join("index.tsv")).unwrap();
    let tmp = std::env::temp_dir().join(format!("keleut-wasm-corpus-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let rows: Vec<(String, String)> = index
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let path = it.next().unwrap_or("").to_string();
            let _src = it.next();
            (path, it.next().unwrap_or("").to_string())
        })
        .collect();
    let (mut pass, mut skip) = (0, 0);
    let mut fails = Vec::new();
    let mut total = Stats::default();
    let (mut bytes_max, mut bytes_sum) = (0usize, 0usize);
    for (i, (path, flags)) in rows.iter().enumerate() {
        let kel = corpus.join(path);
        let out = tmp.join(format!("{i}.kir"));
        let tries: Vec<Vec<&str>> = match flags.as_str() {
            "?" => vec![vec![], vec!["--no-prelude"]],
            "" => vec![vec![]],
            f => vec![f.split_whitespace().collect()],
        };
        let mut code = None;
        for fl in tries {
            let st = Command::new(&diktor)
                .args(&fl)
                .arg("--emit-ir")
                .arg(&out)
                .arg(&kel)
                .current_dir(kel.parent().unwrap())
                .output()
                .unwrap();
            code = st.status.code();
            if code != Some(1) {
                break;
            }
        }
        if code != Some(0) {
            skip += 1;
            continue;
        }
        match translate(&kel, &std::fs::read(&out).unwrap()) {
            Ok((s, b)) => {
                pass += 1;
                total.funcs += s.funcs;
                total.big_frames += s.big_frames;
                total.split_funcs += s.split_funcs;
                total.max_estimate = total.max_estimate.max(s.max_estimate);
                total.max_params = total.max_params.max(s.max_params);
                total.max_blocks = total.max_blocks.max(s.max_blocks);
                bytes_max = bytes_max.max(b);
                bytes_sum += b;
            }
            Err(m) => fails.push(m),
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "コーパス: {} 本のうち、コード生成した {pass} 本、対象外 {skip} 本、失敗 {} 本",
        rows.len(),
        fails.len()
    );
    eprintln!(
        "統計: 関数 {} 個、大きなフレーム方式 {} 個、分けた関数 {} 個、見積もりの最大 {} バイト、引数の最大 {} 個、ブロックの最大 {} 個、program.wasm の最大 {} バイト、平均 {} バイト",
        total.funcs,
        total.big_frames,
        total.split_funcs,
        total.max_estimate,
        total.max_params,
        total.max_blocks,
        bytes_max,
        bytes_sum / pass.max(1)
    );
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(pass > 0);
}
