# X-B11 の下書きの反証

## 結論

下書きの主張の骨格(B11 の枝は CLI、REPL、`Driver` の API のどこからも到達せず、試作は runtest を変えない)は正しい。
試作を自分の写しに当て、`dune build` と `dune runtest` を走らせた。
runtest の出力は空で、分類(落ちるブロック 0、変わるブロック 0)は正しい。
退行は見つからなかった。
直すべき点は、出典と根拠の行番号 3 か所と、経路の本数の食い違い 1 か所である。

## 裏取り

- `lib/interp.ml:1416` の枝、`:1322` の `exec_decl`、`:1302` の前書き、`§14.12`(1274 行で始まる)は正しい。
- `lib/elab.ml:5087-5089` の `bug` の枝と文面、`:5276` の `flatten_modules`、`:5383` の展開、`:5468` の入れ子の `noimpl`、`:5897` の単位ごとの平坦化は正しい。
- `lib/driver.ml:546-547`、`:568`、`:576`、`:648`、`:825`、`:870`、`:1199-1201` は正しい。
- `prelude :: flat` は 686 行ではなく `lib/driver.ml:688` である。
- `bug` の定義は「同ファイル 82 行」ではなく `lib/aux.ml:82` である(`lib/interp.ml:82` は §の前書きの本文)。
- 親の出典 `doc/log/260912-1-status.md:520` は F5b の箇条で、B11 の箇条は 523 行である。
- diktor の `doc/log/260829-4-plan.md:79`、`:442`、`:2088-2110` は正しい。
- 929a255 の本文の E10 の記述は下書きの引用どおりである。
- 「`exec_decl` へ宣言を渡す経路は 4 本」と書いて 3 本の箇条を並べている。担当の要約は 3 種類と書く。実体は CLI(`Interp.run_units`)、API(`Interp.run`)、REPL(`Interp.start_session` と `Interp.exec_input`)で、数え方を揃える必要がある。
- `exec_decl` は `let rec` でなく、`DInstance` の本体を再帰で `exec_decl` に渡さない(ソースを読んで導いた性質)。したがって、インスタンスの本体を経由して `DModule` が届く経路も無い。
- LangSpec に `DModule`、平坦化、`[BUG]` の記述は無く、§15.5 の終了状態の表の実行時エラー(終了コード 3)とも、§18 とも矛盾しない。仕様を変えないという判断は正しい。

## 実測の再現

担当の例(top、nested、imp、prel、repl)を HEAD と試作の両方で走らせ、担当の `results.head.txt` と同じ出力を得た(`out.head.txt`、`out.patched.txt`、両者は同一)。
probe を自分の写し(HEAD と試作)にもつないで走らせ、`probe.head.txt`、`probe.patched.txt` と同じ 3 行を得た。
probe に入れ子の module を `eval_string` へ渡す例を足すと、両方のビルドで `Diktor.Aux.NotImplemented_at` が `eval_string` の外へ漏れた。
これは B11 の枝ではなく平坦化の例外で、試作は変えない。
API の `eval_string` が平坦化の例外を `Error` に包まない点は B11 の範囲外だが、記録してよい。

## 退行の探索

`ex/` に次を書き、HEAD と試作で走らせた。
出力と終了コードはすべて同じだった。

- `m1.kel`: 2 つの module、module の中の pub な型エイリアス、別の module からの呼び出し。`10`。
- `m2.kel`: module の中のガードつきの `match`。`big`、`small`。
- `m3.kel`: module の中の入れ子のハンドラ。`31`。
- `m4.kel`: module の中の型パラメータつきの関数と相互再帰の `let rec`。`3`、`true`。
- `m5.kel`: import した pub module の関数を、自分の module の中から呼ぶ。`13`。
- `prel2.kel` と `usepz.kel`: `--prelude` で差し替えたプレリュードの module に `@ Console` の関数を置く。`hi` を 2 行。
- `r2.in`: REPL で module、入れ子の module(拒否)、その後の続行。両方で同じ出力。
- `--type-check m1.kel`: 両方で同じ。

## 判断事項への意見

推奨案に異論は無い。
到達条件は `Interp` を平坦化なしで呼ぶ誤用だけで、型検査側の同じ枝と分類と文面を揃えるのは筋が通る。
終了コードも 3 のままである。
