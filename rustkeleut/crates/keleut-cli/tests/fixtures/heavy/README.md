# 翻訳の重い関数の fixture

`tests/jit_cmd.rs` が使う。
`consN.kel` の関数 `f` は `Cons` を `N` 段入れ子にしたリストを返し、`loop` が `f` を 3000 回呼ぶ。
どちらも `diktor --emit-ir consN.kir consN.kel` で KIR にした(diktor 15ddd62)。
どちらのプログラムも `4504500` を印字して 0 で終わる。

`cons1000` の `f` は翻訳の重さが上限(`keleut_interp::native::JIT_COST_MAX`)を超えるので、`--tier jit-only` でもインタプリタに残る。
上限を入れる前は、`--tier jit-only` の実行に 110 秒ほどかかった(インタプリタでは 0.1 秒に届かない)。

`cons200` の `f` は上限より軽く、LLVM の翻訳に 0.6 秒ほどかかる。
試験は、`--tier threshold=1` で `f` の翻訳の途中に実行が終わっても、翻訳を待たずに終わることを確かめる。
