# kir の試験の fixture

各 `.kir` と `.kirt` の組は、Diktor(サブモジュール `diktor/` の 2f9300a)の `--emit-ir` と `--emit-ir-text` の出力である。
`tests/roundtrip.rs` は、この組で読み書きの往復がバイト単位で一致することを確かめる。
`tests/verify.rs` と `tests/fuzz.rs` は、これらを書き換えた入力を使う。

| 組 | 元 | 書き出した場所と引数 |
| --- | --- | --- |
| `ex_closure`、`ex_dict`、`ex_handle`、`ex_loop` | `doc/ir/examples/*.kel` | `doc/ir/examples/` で `diktor --no-prelude --emit-ir… X.kel` |
| `sample` | `diktor/test/sample/stubs.kel`、`sample.kel` | `diktor/test/` で `diktor --emit-ir… sample/stubs.kel sample/sample.kel` |
| `strings` | `src/strings.kel`(本 crate のために書いたもの) | `src/` で `diktor --emit-ir… strings.kel` |
| `ir_emit_001` など、`<t>_<k>` の名前の組 | `diktor/test/<t>.t` の k 番目の `cat > … <<` のヒアドキュメント(`src/<t>_<k>.kel` に写した) | ヒアドキュメントのファイル名だけを渡す |
| `import_020` | `diktor/test/import.t` の `dia/` のファイル群 | 同じ |

`sample` と `import_020` のほかは、単位の `paths` がファイル名だけになる場所で書き出した。
Diktor の書き手を変えたときは、同じ引数で書き出し直し、差分を確かめてから入れ替える。
