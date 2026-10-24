# 取り込んだ musl の出所

このディレクトリのファイルは、musl 1.2.6 の配布物から、改変せずに写したものである。
上流での相対パスをそのまま保っている。
ライセンスは MIT で、本文は `COPYRIGHT` にある。

## 版と入手先

- 版: 1.2.6(2026-10-24 に https://musl.libc.org/ で最新の版であることを確かめた)
- 配布物: https://musl.libc.org/releases/musl-1.2.6.tar.gz(1,082,499 バイト)
- sha256: `d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a`
- 署名: https://musl.libc.org/releases/musl-1.2.6.tar.gz.asc
  - 同じサイトの https://musl.libc.org/musl.pub の鍵(指紋 `8364 8929 0BB6 B70F 99FF DA05 56BC DB59 3020 450F`)で `gpg --verify` が `Good signature` を出した。
  - 署名の日時は 2026-03-21 00:41:25 JST である。
  - 鍵も配布物と同じ HTTPS のサイトから取ったので、これはサイトとの整合の確認であり、鍵の持ち主を独立に確かめたものではない。

配布物そのものはリポジトリに入れていない。

## 取り込んだファイル

`sin cos exp log` と、それらが呼ぶ関数とデータである。
`floor.c` と `scalbn.c` は `__rem_pio2_large.c` が呼び、`__math_*.c` は `exp.c` と `log.c` が誤りの値を作るのに呼ぶ。

| ファイル | sha256 |
|---|---|
| `COPYRIGHT` | `b870108ec5e7790e9f9919064f1b9421d62d5f9b0e6c230c6adf7ea2da62e97b` |
| `VERSION` | `5844ffcc346f89c07b13ba7596bfb3788ed73f4755e541182d7822d43b7c7a24` |
| `src/internal/libm.h` | `d5a11757541471030d8f4bf7a39a29de35732ecbb291b2f45bf03296be4f25a2` |
| `src/math/sin.c` | `24e53a9d85f196f66919cc19247be5480bb607ef38844b1a24bc47c4fe4974ef` |
| `src/math/cos.c` | `3a74e054057d636fa494bdd76a53a40db5cee618264f6ab31cb0538ba63c6bda` |
| `src/math/__sin.c` | `f3a37ef9bcfb8a2f9c809d8c7f1aa2c17c8c3319ad7fb2fe7c0a6284ea8859d2` |
| `src/math/__cos.c` | `3f0f774aab60a18d7ef935ffdf121bcda17cbadabff14096c614da22e31a42c1` |
| `src/math/__rem_pio2.c` | `938329e0939cd83ee2185a52154b41242f618d4a6df1b08395f344da33d4f1e6` |
| `src/math/__rem_pio2_large.c` | `b97f19fb5951c83a0c10506fab35646caefde13bd03100ad2af55ef00af0b9fb` |
| `src/math/exp.c` | `7db56f21ca9b8915d36dd5099b707aafeb6a736eae798e19b76b074764e75f34` |
| `src/math/exp_data.c` | `c55dff64e369fe816cd8ce367064989bb119f85783599680d1a329031c660689` |
| `src/math/exp_data.h` | `df77fca837edd02b260dd09a2914202775909ae67a059218ae38cfab104e6d16` |
| `src/math/log.c` | `a0694d8587a0d145d0f8ae87cbffba014d7988bc2ebcbfad94cea73b394c9ecc` |
| `src/math/log_data.c` | `e96bcd99d88fc57e0f3686299014f4b949a6212603ce237f39e16cedc155617a` |
| `src/math/log_data.h` | `a8dced6f0821f30cad040ba326b03e419fbe0a750ed8f3b0d7145992bee4bccc` |
| `src/math/floor.c` | `b9ccb9363719a84be218d60c56717f4fb0234e170c305e36ce21c30e1f42f8e1` |
| `src/math/scalbn.c` | `a2e766283ed30c2b30c71e01dbdabef58faf92cc9aa84a01c1ce1069b6625696` |
| `src/math/__math_oflow.c` | `74669de53bc4b250183cfa262a8a03ee8106f33a83e3aca6b217bafc7363b83c` |
| `src/math/__math_uflow.c` | `9a841197bbe9ade9bbd684f8bc65424a97c481d37dabb9fa1790bafe7959034a` |
| `src/math/__math_xflow.c` | `5feafe10347636884a071a5096f912d5ca4a0939844804ca09b88241405648ad` |
| `src/math/__math_divzero.c` | `84aa910bdc5e7ccfed42098c37c44e278aa942d46e5a51d5ee8273336c79e719` |
| `src/math/__math_invalid.c` | `b56440ed59fa1e1aaaf6bf8b672c8bb71eaf844457906f7773da1d67d7ef013b` |

22 個のファイルは、どれも展開した配布物の同じパスのファイルと `cmp` で一致した。
上流のヘッダのうち `math.h`、`endian.h`、`features.h`(`src/include/` のもの)、`fp_arch.h` は取り込まず、`../shim/` の最小のものに替えた。
