# 261009-3 のスパイク：閉じた行を使用時に開く

`doc/log/261009-3-proposal.md` の 3 節と 3.10 節が使った材料である。

- `callsite-open.patch`：`diktor` 69a3679 の `lib/elab.ml` への差分（提案 3-1 の最初の形）。
  §11.12 の `T.Apply` で呼び出し先の閉じた行を開く変更と、§11.29 の `elab_rec_bindings` で署名を先に置く変更の 2 か所、追加 22 行。
- `callsite-open-v2.patch`：案 U の差分（3.10 節）。
  上の 2 か所に加えて、`open_explicit_eff` を恒等にして公開時の開きを外し、補助関数 `subsume_closed` を `elab_check` の fallback と注釈の照合 4 か所に置く。
  8 か所、追加 42 行、削除 12 行。
- `examples/`：変更前の処理系と変更後の写しで型検査した 20 個の例。
  `a` で始まるものは受理に変わるべき形、`r` で始まるものは拒否のままであるべき形、`o` で始まるものは順序依存を示す形、`p` で始まるものは関数を値として置く形である。
  結果の表は提案の 3.5 節と 3.10 節にある。

写しの作り方は、`diktor` の作業ツリーを `.git` と `_build` を除いて作業用の場所へ写し、どちらかの patch を `patch -p1` で当てて `dune build` する。
写しで `dune runtest` を走らせると、`callsite-open.patch` では `test/annot_rows.t` の 5 ブロックと `test/verify_fixes.t` の 1 ブロックの期待値が変わる。
`callsite-open-v2.patch` では 19 ファイルの期待値が変わるが、新たに落ちるブロックは無い。
