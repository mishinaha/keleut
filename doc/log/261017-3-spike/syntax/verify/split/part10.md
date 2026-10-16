### N.z 合成の確認

#### 順序

当てた順は V115、X-E8、V121、V123、X-E13、V88、V60、V118、X-F5b である。
実施のマイルストーン M[syntax] でも、この順にコミットする。
決め方は次のとおりである。

- V115 を最初に置く。V60、V88、V118、X-F5b が足す診断と期待値を、はじめから `構文エラー: <本文>` の形で書けるようにするためである。
- X-E8 は V118 より前に置く(D[X-E8-2]。`lib/tree.ml` §5.2 の段落の最終形を V118 が 1 回で書く)。
- V121 は V123 より前に置く(`lib/lexer.ml` の冒頭の記事を V123 が V121 の後の形から書き足す)。
- X-E13 は `parse_unit_with` と `try_parse` に限度の節を足すだけなので、駆動を変える V88 より前に置いた。後に置いても、手直しの量は変わらない見込みである(推測)。
- V88 は V60 と V118 より前に置く。V88 がパーサの生成と駆動を替えるので、V60 の文法の規則と V118 の状態の更新を、新しい駆動の上で確かめられる。
- V118 と X-F5b は `lib/lexer.ml` §2.8 の状態、§2.11 の `read`、`lib/driver.ml` の `parse_error_message` を共有する。V118 を先にし、X-F5b が `parse_error_message` を 1 つにまとめる(D[V118-3])。

#### 衝突と手直し

どの手直しも、試作の差分の意図を変えない最小のものにした。
詳しい記録は `doc/log/261017-3-spike/syntax/order.txt` にある。

| 段 | 項目 | 当たらなかった箇所 | 手直し |
|---|---|---|---|
| 1 | V115 | 無し | 無し |
| 2 | X-E8 | 無し | 無し |
| 3 | V121 | 無し(driver.ml の 1 hunk は fuzz 2) | 無し |
| 4 | V123 | lexer.ml の冒頭の記事、driver.ml の `try_parse` の照合 | V121 の段落に 4 行を差し込む。照合の 1 行を V115 の後の文脈で置き換える |
| 5 | X-E13 | driver.ml の `parse_with` の記事、`try_parse` の節 | 記事を V121 の後の 3 種類に足して 4 種類にする。節を末尾に足す |
| 6 | V88 | driver.ml の §16.3a の挿入と `parse_unit_with`、if.t、import.t、nonfeatures.t の期待値 | §16.3a をそのまま差し込む。`misused_reserved_word` の照合から `Sedlexing.MalFormed` を外す。期待値は promote |
| 7 | V60 | trailing_block.t の前書き 1 か所と追加の 3 コマンド | 手で足し、ctor0、tag0、paren、nlarm は promote |
| 8 | V118 | driver.ml の 2 hunk、import.t の nl、run_exit.t の exitparse | `parse_error_message` を V115 の文面で足す。V88 の `misused_reserved_word` に `before_ep` と `before_cont` の退避と復元を足し、`Syntax_error` の照合を消す |
| 9 | X-F5b | driver.ml の 2 hunk、lexer.ml の状態の型と初期値、§2.11 の記事、末尾の節 | 節を §2.14 に繰り下げる。`parse_error_message` を 1 つにまとめる。`misused_reserved_word` に `recent` の退避と復元を足す。「パースエラー」を「構文エラー」にする |

項目の意味が変わった手直しは 3 つある。

- V88 の退避と復元の対象に、V118 の `before_ep`、`before_cont` と X-F5b の `recent` を加えた(D[V88-3])。
  戻さない変種では、`let x =` ⏎ `let y = 2` が V118 の位置を、`let a = (1else)` が X-F5b の文面を失った(実測。`doc/log/261017-3-spike/syntax/ex/results.norestore.txt`)。
- `parse_error_message` の判定の順を、V88 の予約語、X-F5b の数値、V118 の位置の順にした(D[V118-3])。
- V118 と X-F5b の文面の頭を、最終稿の「パースエラー」から V115 の `構文エラー:` に改めた(P[V118-1] の文面と D[X-F5b-1])。

#### 合成後の build と runtest

合成後の写しで `dune build` は通った(`lib/parser.mly` 88 行の Warning 60 は HEAD からある)。
promote と前書きの手直しの後、`dune runtest` の差分は 0 行だった。
段ごとの promote の前の差分は `doc/log/261017-3-spike/syntax/runtest.log` の C 節にある。

合成後のコードを HEAD の既存の cram で走らせた差分は、14 ファイルの 72 行(72 個のコマンドの出力の 1 行ずつ)だった。
新しく落ちるブロックは無い(新しい 4 ファイル `test/reserved_word.t`、`test/syntax_error_pos.t`、`test/utf8_position.t`、`test/lexerr.t` は合成版の期待値で通る)。
72 行を、文面と位置の変わり方で項目に分けた(`doc/log/261017-3-spike/syntax/runtest.log` の B 節)。

| 項目 | 行数 | 内訳 | 最終稿の主張との照合 |
|---|---|---|---|
| V115 | 36 | ast.t 1、if.t 2、import.t 4、nonfeatures.t 18、premise.t 1、repl.t 1、spec_gaps.t 2、tokens.t 2、trailing_block.t 1、trailing_comma.t 4 | 46 行のうち、後続の項目が文面ごと変える 10 行を除いた数で、合う |
| V88(V115 を含む) | 5 | if.t 3、import.t 1、nonfeatures.t 1 | 3 ブロック 5 行で合う |
| V60(V115 を含む) | 3 | trailing_block.t の ctor0、tag0、paren | 3 本で合う |
| V118 | 7 | import.t の pb1、pb2、invariants.t の repl_b、nonfeatures.t の updbase3、premise.t の pr15、verify_fixes.t の reslabel、res2 | 9 行のうち `Syntax_error` の 7 行で合う |
| V118(V115 を含む) | 2 | import.t の nl、run_exit.t の exitparse | 残りの 2 行で合う |
| V123 | 17 | tokens.t 14、errloc.t 2、run_exit.t 1 | コマンド 17 で合う |
| V121 | 2 | run_exit.t の exitutf8、verify_fixes.t の badutf8 | 2 ブロックで合う |
| X-E8、X-E13、X-F5b | 0 | | 既存の期待値は変わらないとの主張に合う |

項目どうしの重なりで、最終稿に無い変化が 2 つあった(どちらも新しい cram の行)。

- `test/reserved_word.t` の norhs(V88 の新しいブロック)が、V118 で `norhs.kel:2:1` から `norhs.kel:1:8: 構文エラー: この位置の後ろ、2:1 のトークンが文法に合いません` になる。
- `test/tokens.t` の gluedkw(X-F5b の新しいブロック、`1.2.match { case _ => 1 }`)が、V88 で汎用の文面から `構文エラー: 予約語 match はここには書けません(予約語は識別子に使えません)` になる。前書きを合わせて直した。

ほかに、新しい cram の汎用の文面と数値の文面は V115 の形で promote した(reserved_word.t 9 行、syntax_error_pos.t 8 行、tokens.t 11 行、trailing_block.t の nlarm 1 行)。

#### menhir の衝突と速さ

V88 は `lib/dune` の menhir の flags に `--table` を足す。
合成後の `lib/parser.mly`(V60 の規則と V118 の変更を含む)を `menhir --explain --strict --table` で単独に生成し、`parser.conflicts` は 0 バイトだった。
HEAD の `parser.mly` も同じく 0 バイトである(menhir 20260209)。
`--strict` の `dune build` も通るので、衝突は 0 件である。

速さは、V88 の `doc/log/261017-3-spike/V88/perf/` の big.kel(30000 行)と tiny.kel で、HEAD の処理系と合成後の処理系を 7 回ずつ走らせた最小で比べた(`doc/log/261017-3-spike/syntax/perf.txt`)。

| 測ったもの | HEAD | 合成後 | 増分 |
|---|---|---|---|
| `--dump-ast big.kel` | 683.1 ms | 849.0 ms | 約 24% |
| `--type-check big.kel` | 820.9 ms | 1030.3 ms | 約 26% |
| `tiny.kel` の実行(プレリュード込み) | 3.0 ms | 2.9 ms | 差なし |

V88 の最終稿の値(約 22% と約 24%)より 2 ポイントずつ大きい。
V118 と X-F5b が `read` に足した状態の更新の分と見るが、内訳は測っていない(推測)。
P[V88-2] の推奨案の根拠(利用者が書く規模では差が見えない)は変わらない。

#### そのほかの確認

- `grep -rn 'パースエラー' lib bin test tools` は 0 件である。
- `grep -rn MalFormed lib bin` は lexer.ml の記事 2 か所と包む関数 1 か所だけである(V121)。
- lexer.ml の `Lex_error` の文面は 9 種ですべて日本語である(V123)。
- `tools/litcheck.py --old <HEAD のファイル>` を driver.ml、lexer.ml、parser.mly、syntax.ml、tree.ml、elab.ml に走らせ、marker はどれも 0 件、code の不一致はコードを変えたためだけだった(`doc/log/261017-3-spike/syntax/litcheck.txt`)。
- V118 の試作の差分はソースコメントを含まないので、合成差分にも含まれない。syntax.ml §1.1、parser.mly の 87、530、1125 行、driver.ml の `parse_with` の記事に `Syntax_error` の説明が残る。lexer.ml §2.8 の「`last_ep` はどこからも読まれていない」も、V118 の後は誤りになる。実施の V118 のコミットで直し、`litwrap.py` と `litcheck.py` を走らせる。

#### LangSpec の同じ行を触る項目

分野の中で LangSpec を変えるのは V60 と X-E13 だけで、触る行は重ならない。

- V60: §5.5 の 410 行の直後に 1 文、§2.3 の 87 行の直後に 1 段落を足す。
- X-E13: §15.5 の表の「型検査での拒否」と「字句と構文の誤り」の 2 行と、1629 行の診断の列挙を置き換える。

V115、V118、V121、V123、V88、X-F5b、X-E8 は LangSpec と `doc/sample.kel` を変えない。
V115 は処理系の語を §15.5 の「構文エラー」にそろえる項目で、X-E13 が 1629 行に足す「処理系の限度を超えたことの報告」とも食い違わない。

#### ほかの分野への要望

- numeric(V114、V8、V91): 統括者の指示にある「V114 と X-F5b が test/numeric.t と LangSpec §4.2 で重なる」は、合成差分では成り立たない。X-F5b は `test/tokens.t` と `lib/lexer.ml`、`lib/driver.ml` だけを変え、`test/numeric.t` と LangSpec に触れない。
  重なるのは次の 3 点である。
  (1) V114 の `lib/driver.ml` §16.8 の終了コードの表の hunk(1046 行)は、V121 が同じ表の 2 の行を変えるので、合成差分の上では当たらない(`patch --dry-run` で実測。`doc/log/261017-3-spike/syntax/other_dryrun.txt`)。syntax を先に入れ、V114 の表の行を V121 の後の形に合わせ直すことを求める。
  (2) V114 が §15.5 の表の行を変えるなら、X-E13 の 2 行の置き換えと文面をまとめる。
  (3) `1u8x` のように接尾辞の後ろに英数字が続く形は、X-F5b の `構文エラー: 数値リテラル 1u8 の直後に x が続いています(…)` が V114 の型エラーより先に出る。V114 や V8 が新しい cram に割れた綴りを入れるなら、この文面で書く。
  V8 と V91 の差分は合成差分の上で当たった(V8 は行のずれだけ)。
- すべての分野: 新しい cram に一般の構文エラーを書くなら、V115 の文面 `構文エラー: 付近のトークンを確認してください` と、V118 の位置(続きを求める形で行をまたげば直前のトークンの終わり)で書く。
- すべての分野: `lib/parser.mly` の意味アクションで構文エラーを投げる変更は、`Syntax_error_at`(V118 の `syntax_error_at`)を使う。V118 の後は位置の無い `Syntax.Syntax_error` が無い。`Syntax.Data` を実装するモジュールを足すなら、V118 が署名に足した `loc` を実装する。
- すべての分野: 字句解析器の状態(lexer.ml §2.8 の型 `t`)にフィールドを足し、`read` で更新するなら、V88 の `misused_reserved_word` の退避と復元にも足す(D[V88-3])。
- `lib/parser.mly` を変える項目(classes の V126、effects の V127): 合成差分の上で V126 は fuzz 2 で、V127 は行のずれで当たった。全分野を合わせた文法で `menhir --explain --strict --table` を走らせ、衝突が 0 件のままであることを確かめることを求める。V126 は `test/import.t` も変えるので、V118 の nl と V88 の letfrom の行との重なりを確かめる。
- eval(X-B11、V108、V109): X-E13 は文字列の API の `flatten_modules`(driver.ml 562〜580 行)が型検査の受け手の外で溢れうることを (6) に残した。X-B11 の担当との突き合わせを求める。V108 と V109 の driver.ml の hunk は合成差分の上で 122 行ずれて当たった。
- spec の分野: §15.5、§5.5、§2.3 を触るなら、X-E13 と V60 の改訂と文面を合わせる。
