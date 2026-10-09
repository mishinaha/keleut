# 261009-3 のスパイク：呼び出し時に閉じた行を開く

`doc/log/261009-3-proposal.md` の 3 節が使った材料である。

- `callsite-open.patch`：`diktor` 69a3679 の `lib/elab.ml` への差分。
  §11.12 の `T.Apply` で呼び出し先の閉じた行を開く変更と、§11.29 の `elab_rec_bindings` で署名を先に置く変更の 2 か所、追加 22 行。
- `examples/`：変更前の処理系と変更後の写しで型検査した例。
  `a` で始まるものは受理に変わるべき形、`r` で始まるものは拒否のままであるべき形、`o` で始まるものは順序依存を示す形である。
  結果の表は提案の 3.5 節にある。

写しの作り方は、`diktor` の作業ツリーを `.git` と `_build` を除いて作業用の場所へ写し、`patch -p1 < callsite-open.patch` を当てて `dune build` する。
写しで `dune runtest` を走らせると、`test/annot_rows.t` の 5 ブロックと `test/verify_fixes.t` の 1 ブロックの期待値が変わる。
