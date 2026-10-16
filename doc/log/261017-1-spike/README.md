# 261017-1 の計画の材料

`doc/log/261017-1-plan.md` の 3 節が引く、試作の差分、例、実測の出力、cram の下書きである。
筆者が作業用の一時領域に diktor 31b3c1d の写し（`git archive` で取り出した木）を作り、そこで試作してビルドし、実測した結果を写した。
写しの diktor、ビルドの成果物、実行形は保存していない。

| 場所 | 内容 | 計画の節 |
|---|---|---|
| `never/never.patch` | 試作の差分。`lib/tree.ml`、`lib/elab.ml`、`lib/interp.ml` の 3 ファイル。diktor 31b3c1d の木の直下で `patch -p1` で当てる | 3 節 (5) |
| `never/examples/*.kel` | 例 19 本。`a_` から `t_` の接頭辞が計画の本文の名前で、`results.*.txt` の見出しと同じ | 3 節 (2)、(3) |
| `never/results.head.txt` | 例を HEAD（31b3c1d）のビルドで実行した出力。`f_retry` だけ `OCAMLRUNPARAM=l=100k` を付けて最後の 2 行を取った | 3 節 (2) |
| `never/results.never.txt` | 同じ例を試作のビルドで実行した出力 | 3 節 (3) |
| `never/never_ops.t` | cram の下書き。試作の写しで `dune runtest` を通した期待値（`dune promote` の結果）。実施では `retry` のブロックを `test/tail_calls.t` へ移す | 3 節 (5)、7 節 |
