# 261024-1 の計画の材料

`doc/log/261024-1-plan.md` が引く、調査報告、試作、探査のプログラム、設計案と審査である。
どれも 2026-10-24 に、親 bcb16f1 と diktor 6e49bbb の上で作った。
作るときに、追跡しているファイルは変更していない。
計画の作成の手順は、計画の 15 節に書いた。

## 置き場所

### survey/: 調査報告

計画が略号で引く 7 本の報告である。
報告の筆者は、diktor 6e49bbb のソースと cram を読み、いま `probe/` にあるプログラムを Diktor と手元の道具で実行して書いた。
各報告は、実測と推論を分けて書き、探査に使ったプログラムを `probe/` の下に置いている。

| ファイル | 計画の略号 | 中身 | 探査のプログラム |
|---|---|---|---|
| `diktor-tree.md` | tree | Diktor の精緻化木と、精緻化の後の IR が運ぶべき事実 | `probe/tree-probe/` |
| `spec-fidelity.md` | fid | すべての実行系が一致させる意味規則の目録と、標準環境の在庫 | `probe/fid/`、`probe/fltcmp/` |
| `effects-impl.md` | fx | Diktor のエフェクトと巻き戻しの実装と、コンパイル方式への要件 Q1〜Q10 | `probe/fx/` |
| `spike-llvm.md` | sl | LLVM ORCv2 を Rust から使う試作の報告(`llvm/README.md` に計画への示唆を足したもの) | `llvm/`、`probe/become/` |
| `spike-wasm.md` | sw | Node 26.9.0 の WasmGC の対応と、wasm-encoder と wasmparser の試作の報告 | `wasm/` |
| `frontend-port.md` | fp | フロントエンドを Diktor から Rust へ移す見積もり | `probe/fp/` |
| `tiering-gc.md` | tg | 段の切り替えとメモリ管理の設計案と試作 | `probe/tg/` |

### llvm/: LLVM ORCv2 の試作

llvm-sys 221.1.1 を通して、システムの LLVM 22.1.8 の LLJIT を呼ぶ試作である(crate の名前は `orc-spike`)。
`src/main.rs` が実験の本体で、`inkwell-check/` は inkwell 0.11.0 が LLVM 22 で組めるかを見る別の crate である。
`gc/` と `sig/` は、`llc` と `opt` で GC の戦略と `musttail` の型の規則を見るための LLVM IR である。
組み方、実行の手順、出力は `llvm/README.md` にある。

### wasm/: WasmGC の試作

Node 26.9.0 の V8 が、WasmGC の後段に要る機能をどこまで受け付けるかを確かめた試作である。
`src/main.rs` は `wat/*.wat` を組み立てて wasmparser で検証し、`out/*.wasm` に書く。
`run.mjs`、`fx*.mjs`、`depth.mjs`、`tier.mjs`、`utf8.mjs` は、Node で動かす検査である。
手順と出力は `wasm/README.md` にある。

### probe/: 探査のプログラム

調査報告と設計案の作成で、仮説を確かめるために書いた小さなプログラムである。

- `tree-probe/`: tree が使った `p1.kel`〜`p4.kel`。
- `fid/`: fid が Diktor で実行した `.kel`(評価順、数値、初期化、エフェクト、深さなど)、出力 `out.txt`、REPL の入力 `repl.txt`、import の例 `imp/`。
- `fltcmp/`: fid の 2 節の Float64 の表示の比較。
  `src/main.rs` は、乱数のビット列 2,000,000 個と 2 の冪と境界の値について、glibc の `%.*g` の探索と Rust の `{:.*e}` の探索と最短往復表記を比べる。
  `js.mjs` は、同じ探索を JS の `toPrecision` で組んで、`cases.txt` の C の結果と比べる。
- `fx/`: fx が使った `.kel`(末尾と非末尾の resume、入れ子の resume、cancel、性能)と、手で組んだ Wasm のモジュールを動かす `wasmprobe.js`、`jspi.js`、`sofx.js`。
- `tg/`: tg の試作。
  `*.ll` は末尾呼び出し、ステートポイント、段の切り替えの LLVM IR で、`interp.c` は解釈器の代役である。
  `gc.mjs` と `wasm.mjs` は、手で組んだ Wasm のモジュール(GC の構造体、末尾呼び出し)を Node で動かす確認、`become.rs` は Rust の `become` の確認、`llv/` と `fib/` は小さな cargo の crate である。
- `become/b.rs`: sl が使った、安定版の Rust に `become` が無いことの確認。
- `fp/`: fp の材料。
  `tools/extract.py` は cram のヒアドキュメントから `.kel` を抜き出し、`tools/codelines.py` はコード行を数える。
  `tc_exit.txt` は、抜き出した各 `.kel` の `diktor --type-check` の終了状態である。
  `shimproj/` は、PATH に置いた `diktor` の shim で cram を無改変で走らせた dune のプロジェクトで、`log.txt` が呼び出しの記録である。
  `c.kel`、`c2.kel`、`c3.kel`、`d.kel`、`e.kel` は、診断を確かめた小さな例である。
- `design-probe/`: native 案の作成で、`clang --target=wasm32 -nostdlib` と `wasm-ld --no-entry` で C の関数を wasm にできることを確かめた例(`a.c` から `a.o` と 295 バイトの `a.wasm`)。
  計画の E14 である。
- `wk.mjs`: 計画の E12 の再測定で、`wasm/depth.mjs` を `Worker` の `resourceLimits: {stackSizeMb: 1024}` で走らせたもの。
  `depth.mjs` は `import.meta.url` からの相対の URL で指す。

### design/: 設計案と審査

- `uniform.md`、`native.md`、`incremental.md`: 優先する観点の違う 3 つの設計案である(意味の一様性、ネイティブの性能と段の切り替え、段階的な納品)。
  各案の裁定と節の番号は、計画の番号とは別のものである。
- `judges.json`: 3 人の審査の結果である。
  審査ごとに、3 案の点(`scores`)、よい案(`best_ideas`)、必須の修正(`must_fix`)を持つ。
- `final.md`: 審査の結果に基づいて 3 案を統合した案である。
  計画 `doc/log/261024-1-plan.md` は、これを計画の書き方に合わせて書き直したもので、違いは計画の 15.3 節に書いた。

## 置いていないものと作り直し方

次のものは、大きいか、ビルドで作り直せるので置いていない。
コマンドはリポジトリの根から実行する。

- `probe/fltcmp/cases.txt`: `fltcmp` が実行したディレクトリに書く、C の結果の表である(399,620 行。候補の先頭 400,000 件から、`1e16` 未満の整数値を除いたもの)。
  次のコマンドで作り直し、JS との比較を行う。

  ```sh
  cd doc/log/261024-1-spike/probe/fltcmp
  cargo build --release && ./target/release/fltcmp
  node js.mjs cases.txt
  ```

  `fltcmp` は約 2,000,000 件を比べて `tested=1998964 diff_vs_rust_precision_e=0 diff_vs_rust_shortest=522` を出す(fid の 2 節)。
  このディレクトリには `.gitignore` が無いので、作った `target/` と `cases.txt` は後で消す。
- `probe/fp/corpus/`: cram から抜き出した 1,046 本の `.kel` である。
  次のコマンドで作り直せる。
  出力は `1046 0`(抜き出した数と、取りこぼした数)である。

  ```sh
  python3 -I doc/log/261024-1-spike/probe/fp/tools/extract.py diktor/test doc/log/261024-1-spike/probe/fp/corpus
  ```

  `tc_exit.txt` を作ったコマンドは記録に無い。
  同じ形の表(終了状態とパス)は、`corpus/` の中で `find . -name '*.kel' | while read f; do diktor --type-check "$f" >/dev/null 2>&1; echo "$? $f"; done` で作れる(行の順は `find` に依る)。
- `probe/fp/diktor-copy/`: fp が時間を測るために使った diktor の写しである。
  diktor 6e49bbb を別の場所に写して `dune build` すれば同じものになる。
- ビルドの成果物: `llvm/target/`(実行のたびに `target/ir/*.ll` も書く)、`llvm/inkwell-check/target/`、`wasm/target/`、`wasm/out/`、`probe/tg/llv/target/`、`probe/tg/fib/target/`。
  作業の木に残っていることがあるが、各ディレクトリの `.gitignore` で除いてあり、材料には含めない。
  `llvm/README.md` と `wasm/README.md` の手順(`cargo build --release`、`cargo run -q`)と、`probe/tg/llv` と `probe/tg/fib` の `cargo build --release` で作り直せる。
