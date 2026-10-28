//! サブコマンド `keleut build` である(計画 261024-1 の 6.2 節)。
//!
//! `keleut build --target wasm -o DIR [OPTIONS] FILE.kel… | FILE.kir` は、WasmGC の後段の出力
//! `program.wasm`、`rt.wasm`、`main.mjs` を `DIR` に書く。
//! `node DIR/main.mjs` だけで走る(スタックは `Worker` の `resourceLimits.stackSizeMb` で取る)。
//!
//! 入力の読み方と選択肢(`--frontend`、`--max-depth`、`--prelude`、`--no-prelude`、`--import-path`、`--no-yield-prune`)は
//! `keleut run` と同じである(既定のフロントエンドは `rust`)。
//! `--max-depth` は `main.mjs` が使う論理的な深さの限度になる。
//! 終了状態は、成功が 0、使い方の誤りが 64、KIR の不正が 65、翻訳と検証と書き出しの失敗が 70 である。

use crate::{exit, run_cmd};

/// `keleut build` の使い方である。
pub const USAGE: &str = "\
usage: keleut build --target wasm -o DIR [OPTIONS] FILE.kel... | FILE.kir

writes program.wasm, rt.wasm and main.mjs to DIR (run with: node DIR/main.mjs)

options:
  --max-depth N               logical depth limit L used by main.mjs (default 1048576)
  --no-yield-prune            treat every resume point as able to receive a yield (pass 11 off)
  --frontend diktor|rust      front end for .kel files (default: rust; diktor runs
                              $KELEUT_DIKTOR, else diktor on PATH)
  --prelude FILE, --no-prelude, --import-path DIR   passed to the front end
";

/// `keleut build …` の入口である。引数の先頭は `build` である。
pub fn main(args: &[String]) -> u8 {
    let rest = &args[1..];
    if matches!(rest.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return 0;
    }
    let mut target = None;
    let mut out = None;
    let mut others = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--target" | "-o" => {
                let Some(v) = rest.get(i + 1) else {
                    eprint!("keleut: {} needs a value\n{USAGE}", rest[i]);
                    return exit::USAGE;
                };
                if rest[i] == "--target" {
                    target = Some(v.clone());
                } else {
                    out = Some(v.clone());
                }
                i += 2;
            }
            "--backend" | "--tier" | "--native-stack" | "--gc-stress" => {
                eprint!("keleut: unknown option: {}\n{USAGE}", rest[i]);
                return exit::USAGE;
            }
            _ => {
                others.push(rest[i].clone());
                i += 1;
            }
        }
    }
    if target.as_deref() != Some("wasm") {
        eprint!("keleut: --target wasm is required\n{USAGE}");
        return exit::USAGE;
    }
    let Some(out) = out else {
        eprint!("keleut: -o DIR is required\n{USAGE}");
        return exit::USAGE;
    };
    let o = match run_cmd::parse(&others) {
        Ok(o) => o,
        Err(msg) => {
            eprint!("keleut: {msg}\n{USAGE}");
            return exit::USAGE;
        }
    };
    let lp = match run_cmd::load(&o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    match keleut_wasm::launch::build(&lp, o.max_depth, std::path::Path::new(&out)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("keleut: 内部エラー: {e}");
            70
        }
    }
}
