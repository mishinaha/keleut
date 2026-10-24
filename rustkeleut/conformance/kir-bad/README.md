# kir-bad

不正な KIR の試料である。
`keleut kir verify` がどれも落ちずに 65 で拒否することを確かめる。
走らせ方は次のとおりである(`rustkeleut/` で)。

```sh
cargo build --release
target/release/kconform expect conformance/kir-bad/*.t
```

各 `.t` には、`keleut kir verify` の標準エラーの文面と `[65]` を記録してある。
文面は Diktor の `diktor --run-ir` と同じで、違うのは先頭の `keleut:` だけである(v09-resume-cyclic-handlebody を除く。後述)。
kconform は比べる前に行を正規化する。
診断の種別の語(型エラーなど)に `: ` が続けばその後ろを切り、先頭が `⚠ ` の行は `⚠` だけにする。
KIR の診断の文面はどちらにも当たらないので、正規化しても文面は全体のまま比べられ、各試料がどの検査で拒否されたかまで固定される。
正規化の前の不一致の件数も要約に出る。

## 作り方

試料は、`crates/kir/tests/fixtures/` の Diktor 2f9300a の書き出した正しいプログラムを 1 か所ずつ壊して作った。

- `bNN-*.kir`: `ex_loop.kir` の 1 ビットを反転したもの、切り詰めたもの、見出しや節を書き換えたもの。
- `tNN-*.kirt`: `ex_loop.kirt` のテキストを書き換えたもの。
- `vNN-*.kirt`: `ex_handle.kirt` か `ex_dict.kirt` のテキストを書き換えたもの。
  書き換えは `crates/kir/tests/verify.rs` の `MUTATIONS` から、IRSpec 13.8 節の各規則に 1 つ以上選んだ。
  どれも読み込みを通り、検証で拒否される。
- `deep-base.kirt`: `ex_loop.kirt` そのもの(正しいプログラム)である。
  `deep.t` が、これをもとに深い入力をその場で作る。

## 試料と検査

### バイナリの読み込み(IRSpec 13.7 節。`binary.t`)

| 試料 | 13.7 節の項目 | 作り方 | 診断 |
| --- | --- | --- | --- |
| b01-header-short | 1 見出し | 先頭の 10 バイトだけ | 見出しの 16 バイトがありません |
| b02-version | 1 見出し | 位置 4 のビット 0 | 版 1.3 は読めません |
| b03-feature-bit | 1 見出し | 位置 8 のビット 0 | 必要な機能のビット 0x00000001 を処理できません |
| b04-section-count | 2 節の並び | 位置 12 のビット 0 | 節の数が見出し(10)と違います(11) |
| b05-section-undefined | 2 節の並び | 位置 16 のビット 0 | 節 0x00 は定義されていません |
| b06-section-order | 2 節の並び | 位置 16 のビット 1 | 節 0x02 が id の昇順に並んでいません |
| b07-section-missing | 2 節の並び | 位置 815 のビット 7 | 必須の節 0x0b がありません |
| b08-section-size | 2 節の並び | 位置 18 のビット 1 | 節 0x01 の大きさがファイルの残りを超えます |
| b09-section-trailing | 3 符号 | 位置 19 のビット 0 | 節 0x01 の中身の後ろにバイトが残っています |
| b10-read-past-end | 3 符号 | 位置 19 のビット 4 | 節の終わりを越えて読もうとしました |
| b11-leb128-overlong | 3 符号 | 最初の節の大きさを 6 バイトの LEB128 にした | uLEB128 が u32 の範囲を超えます |
| b12-count-overflow | 3 符号 | 位置 604 のビット 4 | 個数が節の残りのバイト数を超えます |
| b13-bad-tag | 3 符号 | 位置 746 のビット 5 | 式の番号 0x34 は定義されていません |
| b14-bool-value | 3 符号 | 位置 734 のビット 0 | yield の印の値が 0 でも 1 でもありません |
| b15-flag-bits | 3 符号 | 位置 769 のビット 2 | 呼び出しの印に定義されていないビットがあります |
| b16-strings-order | 4 表 | 位置 21 のビット 1 | stringsの表が厳密な昇順に並んでいません |
| b17-strings-unused | 4 表 | 位置 617 のビット 0 | strings の項目 1 がどこからも使われていません |
| b18-strings-utf8 | 4 表 | 位置 22 のビット 7 | strings の表に UTF-8 でない名前があります |
| b19-index-range | 5 添字 | 位置 772 のビット 1 | 大域の添字 14 が範囲外です |
| b20-count-lower-bound | 6 個数の下限 | 位置 822 のビット 0 | def_fns の大域がありません |
| b21-truncated | 2 節の並び | 先頭の 400 バイトだけ | 節 0x01 の大きさがファイルの残りを超えます |
| b22-bad-magic | 1 見出し | マジックを `00 4B 49 53` にした | マジックが KIR のものではありません |
| b23-verify-tail-mark | (13.8 節の 8) | 位置 805 のビット 0 | call の末尾の印が、再計算した結果と違います |
| b24-strings-order-trailing | 4 表 | b16 の strings の個数(位置 19)を 1 減らした | stringsの表が厳密な昇順に並んでいません |

b23 は、ビットの反転が読み込みを通って検証で拒否される例である。
b24 は、strings の節が並びの誤りと残りのバイトの両方を持つ例である。
Diktor と同じく、並びの誤りを先に報告する。
b22 は、拡張子が `.kir` なのでバイナリとして読む。
`keleut kir` は拡張子が `.kir` でも `.kirt` でもないファイルだけをマジックで振り分ける。

### テキストの読み込み(IRSpec 12 節。`text.t`)

| 試料 | 書き換え | 診断 |
| --- | --- | --- |
| t01-syntax | 閉じ括弧を 1 つ消した | ) がありません |
| t02-unknown-name | 表に無い大域を参照した | 大域が表にありません: main/nosuch |
| t03-count-lower-bound | 要素が 1 つの `seq` | seq の要素が 2 個より少なくなっています |
| t04-version | `(kir 0 2 0)` | 版 0.2 は読めません |

Diktor はテキストを読まないので、この 4 本は RustKeleut だけの試料である。

### 検証(IRSpec 13.8 節。`verify.t`)

| 試料 | 13.8 節の規則 | 書き換え |
| --- | --- | --- |
| v01-symbol-form | 1 記号 | 大域の記号 `main/r3` を `r3` にした |
| v02-creation-site | 2 関数を作る場所 | `lambda` の関数を `thunk` にした |
| v03-function-fields | 3 関数の欄 | `lambda` の `dicts` を 1 にした |
| v04-slot-range | 4 スロット | `local` の番号を範囲の外にした |
| v05-capture-count | 5 捕獲 | `handle` の本体の捕獲を消した |
| v06-rep | 6 Rep | `def_value` の大域の Rep を `any` にした |
| v07-construct-fields | 7 構文 | `construct` のフィールドを消した |
| v08-tail-mark | 8 末尾の印 | 末尾の `handle` を `ntail` にした |
| v09-resume-tail-off-spine | 9 resume | 背骨の外の `resume` を `tail` にした |
| v09-resume-in-lambda | 9 resume | ラムダの本体に `resume` を書いた |
| v09-resume-cyclic-handlebody | 9 resume | 互いを作る 2 つの `handlebody` の関数の一方に `resume` を書いた |
| v10-classification | 10 分類 | 分類 G の操作節を T と書いた |
| v11-instance-methods | 11 インスタンス | クラスにメソッドを足し、インスタンスが実装しないようにした |
| v12-init-twice | 12 単位と初期化 | 大域を 2 回定義した |
| v13-builtin-tables | 13 組み込みの表 | 組み込みの型の名前を変えた |
| v14-runtime-mark | 14 操作の印 | `prelude/Console` でない操作に `runtime` の印を付けた |

v09-resume-cyclic-handlebody は、Diktor 2f9300a の検証器が止まらない入力である(持ち主をたどる鎖が輪になる)。
`diktor --run-ir` には渡さない。
ほかの v の試料は、`keleut kir asm --no-verify` でバイナリにして `diktor --run-ir` に渡すと、同じ文面で 65 になる。

### 入れ子の限度(IRSpec 13.9 節。`deep.t`)

`deep.t` は、`deep-base.kirt` の `main/total` の本体を深さ n の鎖に置き換えた入力を awk で作る。

| 入力 | 終了状態 | 診断 |
| --- | --- | --- |
| 深さ 100,000 の `not` の鎖(対照) | 0 | なし |
| 深さ 100,001 の `not`、`variant` のパターン、`drecord` の辞書の鎖(テキスト) | 65 | 処理系の限度: KIR の読み込みで入れ子が深すぎます |
| 同じ `not` と `drecord` の鎖を `asm --max-nest 100001` で書いたバイナリ | 65 | 同じ |
| `deep-base.kirt` を `--max-nest 3` で読む | 65 | 同じ |

深さ 100,000 の対照は、限度がちょうど `N_nest` であることを示すために置く。
これだけは 65 でなく 0 で終わる。
深さ 100,001 のバイナリの診断は、位置まで Diktor の `--run-ir` と同じである。
