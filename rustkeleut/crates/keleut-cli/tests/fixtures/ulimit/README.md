# 仮想メモリの上限の下で走らせる fixture

`tests/jit_cmd.rs` が使う。
`gres.kel` は `conformance/gc/gc.t` の同名のプログラムを写したもので、`diktor --emit-ir gres.kir gres.kel` で KIR にした(diktor 15ddd62)。
試験は `ulimit -v 524288` の下で、`--backend` を省いた既定と段の設定のどれでも、`200000` と `G` を印字して 0 で終わることを確かめる。
