# 261010-1 の計画の材料

`doc/log/261010-1-plan.md` の各項目の節が引く、試作の差分、例、実測の出力である。
下書きを書いた担当と、それを検証した検証者が、作業用の一時領域に作ったものを、計画の作成のときに写した。
写しの diktor、ビルドの成果物、実行形は保存していない。
差分は、特に断らない限り diktor 69a3679 の作業ツリーを `.git` と `_build` を除いて写した木の直下で `patch -p1`（見出しが `a/` と `b/` のもの）か `patch -p0`（見出しが `lib/...` のもの）で当てる。

| 場所 | 内容 | 計画の節 |
|---|---|---|
| `u/` | 案 U の差分（`u-elab.diff` が全体、`u-stageA.diff` と `u-stageBC.diff` が段階、`u-refopen.diff` が名前の参照で開く 2 行、`u-elab-over-v2.diff` が 261009-3 の v2 からの差分）、LangSpec の改訂案（`langspec.diff`、`spec/sec132.md`）、`doc/sample.kel` のコメントの改訂（`sample-kel.diff`）、例 48 個（`examples/`）と 4 つのビルドでの結果（`results.*.txt`）、期待値の差分の分類（`classify.txt`、`tools/`）、最終形の cram の期待値（`test-final/`）、検証者の追加の例と分類（`verify/`） | 9 節 |
| `if/` | 条件式の差分（`if.patch`）、LangSpec の改訂案（`langspec.patch`）、`doc/sample.kel` の任意の 1 行（`sample-comment.patch`）、対照の差分（`ablation-noif.patch`）、例 36 本（`cases/`、`run.sh`、`cases.*.out`）、仕様の例（`specex/`）、検証者の材料（`verify/`。案 U との合流の例 `ux/`、`else` を省略できる規則の差分、パターンの位置を `$sloc` にした差分） | 6 節 |
| `small/` | 操作名の解決（`6-1-*.patch`）、到達しない分岐（`6-2-*.patch`）、`Neg` と `Rem`（`6-5-*.patch`）、型位置の括弧（`9-8-*.patch`）、`with f(a)`（`g-with-call.lib.patch`）の差分、例（`examples/`）、改訂前後の sample の型検査の出力（`base_on_*.txt`）、検証者の材料（`verify/`。6 か所を当てた sample と生成のスクリプト、計測用の差分、案 U と重ねたときの差分） | 3 節、4 節、5 節、7 節、8 節 |
| `dict/` | 辞書渡しの試作の差分（`dict-prototype.patch`）、試作の `dune runtest` の差分（`runtest-prototype.diff`）、例（`ex/`、`ex2/`）と両方のビルドでの出力（`examples.out`）、測定（`bench/`）、sample の型検査の出力（`sample_*.out`）、検証者の材料（`verify/`。反例 `ex/`、対話的な実行の比較 `repl/`、再測定、案 U の上に当てたときの記録） | 10 節 |
| `doc/` | 文法の書けない形と優先順位の例（`gram/`）、終了状態と警告と UTF-8 の例（`x/`）、保存する性質の組（`inv/`、`rec/`）、cram の案（`invariants.t`、`run_exit.t`、`string_bytes.t`）と再生の結果（`*.head.t`、`invariants.u.t`）、簡易な再生器（`tools/minicram.py`）、検証者の材料（`verify/`。反例 `t/`、5 つのビルドでの再生 `replay/`、一覧 `counterexamples.txt`） | 11 節、12 節 |
