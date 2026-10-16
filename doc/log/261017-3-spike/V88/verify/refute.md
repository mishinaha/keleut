# V88 下書きの反証

## 裏取り(写しと HEAD のビルド)

`lib/driver.ml` の `parse_unit_with`(264-274 行、`Parser'.Error` の分岐は 270-271 行)、`try_parse`(757-775 行、汎用の文面は 769 行)、`lib/dune` 10 行、`lib/lexer.ml` の `parse`(943 行)と §2.11(927-930 行)、`lib/parser.mly` 104-109 行と 513 行は、どれも HEAD の写しと一致した。
LangSpec §2.2 の予約語の文は 56-62 行、§15.5 の終了状態 2 の行は 1621 行、§5.4 の `{` の表は 371-379 行で、下書きと一致した。
sample.kel 20-29 行は `{` の読み分けの表である。
261010-1-plan.md の 1489 行は D266 の中、1891-1893 行は V88 の台帳、5878 行は表の行で、261010-2-m53.md の 682 行は延期のままの行である。
`test/nonfeatures.t` の kwlabel の前書きは 107-112 行で、下書きの 108-112 行より 1 行早い。
「`test/repl.t` の 8 行目の `and`」は、ファイルの 74 行目(対話的な実行の入力の 8 行目)である。

## 実測の再現

試作の差分は写しにそのまま当たり、dune build は通った(警告は HEAD と同じ `Warning 60 unused module Diktor` だけ)。
試作の差分を当てたままの dune runtest は差分なしである。
HEAD の test/*.t を写して dune runtest を走らせると、差分は `test/if.t` の kwthen/kwelse/kwlabel(1 ブロック、3 行)、`test/import.t` の letfrom(1 行)、`test/nonfeatures.t` の kwlabel(1 行)だけで、どれも文面だけが変わり位置は同じだった。
下書きの「3 ブロック 5 行」は正しい。
`test/reserved_word.t` は 7 ブロック、45 コマンド、`printf` で作るプログラム 22 本で、汎用の文面は 7 行である。
`ex/` の全例を両方のビルドで走らせ直した結果は `results.head.txt` と `results.proto.txt` に一字一句一致した。
30000 行の `--dump-ast` は、同じ既定の profile でビルドした HEAD が 674 ms、試作が 823 ms(7 回の最小)で、下書きの値を再現した。
V60 の 7 形は HEAD でどれも汎用の文面になった(`ex/v60/`)。
conflict の数は再計測していない(`conflicts/summary.txt` と b1 の `parser.conflicts` の 18 状態の記録が一致することだけを見た)。

## 退行(受理と出力)

HEAD が受理するプログラムで、試作が拒否するか出力を変える形は見つからなかった。
試したのは、import と module と pub とガード(`ex/r1_import.kel`)、入れ子のハンドラ(`r2_handlers.kel`)、型クラスと制約付きの型パラメータとエイリアス(`r3_classes.kel`)、レコードの更新・パンニング・末尾ブロック(`r4_records.kel`)、プレリュードの差し替え(`pre_ok.kel`)、20 万段の括弧と 30 万項の演算子の列と 10 万文のブロック(`l=100k` でも両方通る)、対話的な実行の 2 本(`repl1.txt`、`repl2.txt`。続きの行の待ち、セッションの継続、`:type`)である。
`--dump-ast` は上の 4 本と `test/sample/sample.kel` で両方一致した。
意味アクションが投げる構文エラー(`r5_semact.kel`、`r6_semact2.kel`)の文面と位置も同じだった。

## 診断の誤爆(下書きに無い)

位置違いの予約語で、試作が「識別子に使えません」と出す形が 3 つある(`ex/fp/`)。

- `let r = handle { f() } with { x => x }`(前置の `handle`。§13.4 は後置)。HEAD は 2:9 で汎用、試作は「予約語 handle は識別子に使えません」。
- `let y = match(x) { case 1 => 2, case _ => 3 }`(前置の `match`)。試作は「予約語 match は識別子に使えません」。
- `import { b } from "./a"`(順序の誤り)。試作は 1:1 で「予約語 import は識別子に使えません」。

どれも、予約語を識別子にすると呼び出しや末尾ブロックとして読み進められるので、条件 2 と 3 を通る。
下書きの「条件 2 と 3 で位置違いの予約語をほぼ除ける」(P[V88-3] の根拠)は、この 3 形を見ていない。
また `ex/fp_case_if.kel` は試作で専用の文面になるので、(7) の「`fp_*.kel` が汎用の文面のままであること」は結果と食い違う(この形自体はパターン変数の位置なので専用の文面で妥当である)。

## 拾える範囲

28 語の予約語を 4 つの位置に置いて測った(`ex/lab/`)。

- `let w = 1`:27 語が専用、`rec` だけ汎用(`let rec` が正しい形なので `=` で詰まる)。
- 型のラベル `{w: Int32}`:27 語が専用、`extends` だけ汎用。
- 呼び出しの引数 `g(w)`:21 語が専用、`fn`、`if`、`perform`、`resume`、`run` は汎用、`true` と `false` は正しいプログラム(型エラー)。
- レコード式のラベル `{w = 1}`:11 語(and class else from handle import instance match rec then val)だけが専用で、17 語(case effect extends extern false fn if let module newtype perform pub resume run true type with)は汎用のままである。

汎用のままの形は、予約語がその位置で式か文を始められるので、予約語ではなく次の `=` や `)` で詰まる形である。
下書きの残る限界は「宣言を始められるとき」(`{type = 1}`)だけを書くので、式を始める語(`{if = 1}`、`{true = 1}`、`g(if)`)を含むように直す必要がある。
`test/reserved_word.t` の前書き「レコードのラベル。…のどれでも同じ文面になる」も、ラベルの語によっては汎用のままであることと合わない。

## ソースコメント

`lib/driver.ml` 169-172 行の「menhir が投げる `Parser'.Error` は情報を持たず、どこで詰まったかは、字句解析器が覚えている最後のトークンの位置からしか取れない」は、試作では `parse_program` が詰まる直前の状態を持つので古くなる。
同じく §16.3 の `parse_with` の箇条(HEAD の 252 行)の「menhir が次に進む節を選べなかった」も、試作では `parse_program` が投げる。
下書きの (5) のソースコメントの一覧に 169-172 行が無い。
litwrap.py は試作の 3 ファイルを変えず、litcheck.py --old は driver.ml と lexer.ml で code の不一致(コードの変更による)、parser.mly で一致、marker は 0 件だった。

## 判断事項

P[V88-3] は、上の誤爆 3 形があるので、推奨の根拠が弱い。
代案の文面(「予約語 W はここには書けません(予約語は識別子に使えません)」)なら、3 形でも誤りにならない。
推奨案を保つなら、誤爆の 3 形を cram に足して既知の限界として固定するか、識別子を予約語の前に補うと予約語を受け付ける(後置の `match` と `handle`)ときは汎用の文面にする条件を足す。
P[V88-2] の「約 2 割」は `--dump-ast` の値で、`--type-check` は 820 ms から 1017 ms の約 24% である。
P[V88-1] に異論は無い。
