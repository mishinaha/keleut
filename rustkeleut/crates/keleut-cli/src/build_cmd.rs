//! サブコマンド `keleut build` である。
//!
//! `keleut build --target wasm -o DIR [OPTIONS] FILE.kel… | FILE.kir` は、WasmGC バックエンドの出力 `program.wasm`、`rt.wasm`、`main.mjs` と起動スクリプト `run` を `DIR` に書く。
//! 書いたプログラムは、`DIR/run` か `node DIR/main.mjs` だけで実行できる。
//! スタックは `Worker` の `resourceLimits.stackSizeMb` で確保する。
//! `DIR/run` は、標準出力が閉じているかどうかを Node を起動する前に調べる。
//! 閉じた標準出力にプログラムが書いたときは、`keleut run --backend wasm` と同じく終了コード 74 で終了する。
//! `node DIR/main.mjs` を直接実行すると、標準出力が閉じていることを検出できない。
//!
//! 入力の読み方とオプション(`--frontend`、`--max-depth`、`--prelude`、`--no-prelude`、`--import-path`、`--strict-exhaustive`、`--no-yield-prune`)は `keleut run` と同じである(既定のフロントエンドは `rust`)。
//! `keleut run` のオプションのうち `--backend`、`--tier`、`--native-stack`、`--gc-stress` は、未知のオプションとして拒否する。
//! `--max-depth` は、`main.mjs` が使う論理的な深さの上限になる。
//! 終了コードは、成功が 0、使い方のエラーが 64、KIR の不正が 65、Wasm のコード生成、検証、ファイルの書き出しの失敗が 70 である。
//! 入力を読み込む段階でそれ以外のエラーが起きたときは、`keleut run` と同じ終了コードで終了する。

use crate::{exit, run_cmd};

/// `keleut build` の使い方の説明(usage)である。
pub const USAGE: &str = "\
usage: keleut build --target wasm -o DIR [OPTIONS] FILE.kel... | FILE.kir

writes program.wasm, rt.wasm, main.mjs and the launcher run to DIR
(run with: DIR/run; node DIR/main.mjs also works but cannot detect a closed stdout)

options:
  --max-depth N               logical depth limit L used by main.mjs (default 1048576)
  --no-yield-prune            treat every resume point as able to receive a yield (pass 11 off)
  --frontend diktor|rust      front end for .kel files (default: rust; diktor runs
                              $KELEUT_DIKTOR, else diktor on PATH)
  --prelude FILE, --no-prelude, --import-path DIR   passed to the front end
";

/// `keleut build …` のエントリポイントである。引数の先頭は `build` である。
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
