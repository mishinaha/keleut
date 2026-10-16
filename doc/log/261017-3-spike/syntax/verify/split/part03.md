### N.3 V121：不正な UTF-8 のバイト列の字句エラーに位置を付ける

#### (1) 判定、規模、依存、出典

判定は fix である。
LangSpec は変えない。

規模は S である。
コードの変更は `lib/lexer.ml` に 3 行の関数を足し、`lib/driver.ml` から `Sedlexing.MalFormed` の 4 つの分岐(13 行)を消すだけで、ほかはコメントと cram である(`doc/log/261017-3-spike/V121/V121.patch`、230 行のうち新しい cram が 82 行)。

依存は無い。
同じ箇所に触れる項目がある。
V115 は `parse_unit_with` の 271 行と `try_parse` の 768 行の「パースエラー」の文面を変える。本項目が消す 266〜269 行と 774 行に隣接する。
V123 は lexer.ml の英語の字句エラーの文面と、driver.ml 770 行の `try_parse` の `Incomplete` の照合の文字列を変える。`test/run_exit.t` の exitlex(55 行)は本項目の exitutf8(63 行)と同じ節にある。
V114 は §16.8 の終了コードの表の 1052 行(未実装、`1u8`、`Int8`)を変える見込みで、本項目が変える 1050 行と 2 行しか離れていない。
V118 は `parse_unit_with` の `last_sp` を使う位置を変える。本項目は `last_sp.pos_fname` の唯一の利用を消すが、`last_sp` そのものには触れない。
V88 の `misused_reserved_word` は次のトークンを読むときに `Sedlexing.MalFormed` を握りつぶす。本項目の後はこの例外が字句解析器の外へ出ないので、照合から外す(N.6)。
X-E13 は `parse_unit_with` の記事の失敗の一覧に `Stack_overflow` と `Out_of_memory` を足す。本項目が `MalFormed` を消した後の一覧に足す(N.5)。
どの順に入れても、衝突は文脈のずれだけである。
分野の順序では V115 と X-E8 の後、V123 の前に置く(V123 が lexer.ml の冒頭の記事を本項目の後の形から書き足すため。N.z)。

出典は `doc/log/261010-2-m53.md:711`(実施の途中で見つけ、延期)。
詳述はそこの 1 行だけで、候補の直し方と観測点の記述は無い。
観測点は `test/run_exit.t` の exitutf8(56〜64 行)で、もう 1 か所 `test/verify_fixes.t` の badutf8(343〜346 行)が同じ診断を固定している。

#### (2) 現状(根拠つき)

仕様の文面: LangSpec §2.1(25〜26 行)は「UTF-8 として正しくないバイト列を含むソースは、字句エラーになる」とだけ定め、§15.5(1621 行)は字句エラーの終了状態を 2 とする。
診断の書式と位置は LangSpec のどこも定めていない(1782〜1783 行は、診断の文面に名前と位置が現れることを前提にするだけ)。

実装(HEAD 39d4091)。

- `lib/lexer.ml` §2.7 `read_raw_token`(466〜553 行)は sedlex の lexbuf から生トークンを読む。不正なバイトに当たると、sedlex のデコーダが位置を持たない例外 `Sedlexing.MalFormed` を投げ、本章はそれを素通しする(章の冒頭 54〜62 行と §2.6 の 357〜359 行がそう書いている)。
- `lib/driver.ml` は 4 か所でこの例外を受け、どれも位置を付けない。
  §16.3 `dump_tokens_file`(243〜245 行)はファイル名だけを付ける。
  §16.3 `parse_unit_with`(266〜269 行)は `lexer.Lexer'.last_sp.Lexing.pos_fname`、つまり直前に渡したトークンのファイル名だけを付ける。
  §16.6b `try_parse`(774 行)は `<stdin>` だけを付ける。
  §16.8 の受け皿(1163〜1165 行)はファイル名も付けない。
- `parse_unit_with` の付け方には、もう 1 つ欠陥がある。
  最初のトークンを渡す前は `last_sp` が `Lexing.dummy_pos` で、ファイル名が空文字列なので、診断が `: 字句エラー: …` になる。

sedlex 3.7 の振る舞い(ソース `sedlexing.ml` を読んで導いた性質): `from_gen` の `refill`(134〜152 行)は、正しく復号できた前置部分を先にバッファへ入れて返し、不正なバイトに読み進もうとした次の `refill` で `MalFormed` を投げる。
そのため、例外が出た時点の現在位置(`lexing_position_curr`)は不正なバイトそのものを指す。
行番号は sedlex が改行を数えて持つので、行も正しい。

HEAD での実測(`doc/log/261017-3-spike/V121/ex/` のプログラム、全出力は `doc/log/261017-3-spike/V121/results.head.txt`):

| 入力 | HEAD の診断 |
| --- | --- |
| `echoln("before")⏎let x = "\377"`(exitutf8 と同じ) | `u1_string.kel: 字句エラー: 不正な UTF-8 バイト列です` |
| `let ab\377c = 1` | `u2_ident.kel: 字句エラー: …` |
| `/* \377 */⏎let x = 1` | `: 字句エラー: …`(ファイル名が空) |
| `\377` だけのファイル(`ok.kel` の後ろに渡す) | `: 字句エラー: …`(どのファイルか分からない) |
| import 先 `lib1.kel` の `"\377"` | `lib1.kel: 字句エラー: …` |
| `--repl` に `let x = "\377"` | `<stdin>: 字句エラー: …` |

どれも終了状態は 2 で、REPL はセッションを続ける。
`--type-check`、`--dump-tokens`、`--dump-ast` も同じ診断である。

#### (3) 判断と確定

D[V121-1]: 字句解析器の章(lexer.ml)が外へ出す例外を `Lex_error` と `Sys_error` に限る。
§2.7 の終わりで `read_raw_token` を同名の関数で包み、`Sedlexing.MalFormed` を受けて `Lex_error ("不正な UTF-8 バイト列です", Sedlexing.lexing_position_curr lexbuf)` を投げ直す。
層 2 と層 3 が生トークンを読む経路は `peek`(646 行)と `pop_reclassified`(746 行)の 2 か所だけで、どちらも包んだ関数を呼ぶ。
位置にはファイル名が lexbuf から入るので、最初のトークンで失敗してもファイル名は失われない。
診断の文面は、ほかの字句エラーと同じ受け皿(§16.8 の `Lex_error` の節、REPL の `try_parse` の節)が `ファイル:行:桁: 字句エラー: 不正な UTF-8 バイト列です` と書く。

判断事項 P[V121-1]: 例外を位置つきに変える場所はどこか。
推奨案: 字句解析器(lexer.ml §2.7)で変える。
代案 1: driver の 4 か所の分岐で、`Lexer'.t` の lexbuf から位置を取る。
代案 2: 3 つの入口(`from_filename`、`parse_string`、`try_parse`)で原文を `String.is_valid_utf_8` などで先に検査し、位置を自分で数える。
推奨の根拠: 代案 1 は `dump_tokens_file` と受け皿の分岐に lexbuf が届かず、4 か所に同じ処理を書くことになる。lexbuf を持つのは lexer.ml だけで、そこで変えれば driver は既存の `Lex_error` の経路をそのまま使える。
代案 2 は sedlex の `refill` の内部の振る舞いに依らない利点があるが、入口が 3 つに分かれ、行と桁を sedlex と同じ数え方(コードポイントで桁を数え、改行で行を進める)で再現する手間がかかる。(6) の sedlex の版への依存が現実に問題になったときの代わりの手段として残す。

判断事項 P[V121-2]: driver の `Sedlexing.MalFormed` の 4 つの分岐を残すか。
推奨案: 4 つとも消す。
代案: 防御として §16.8 の受け皿の分岐だけ残す。
推奨の根拠: D[V121-1] の後は、`Sedlexing` の lexbuf を読む関数が lexer.ml の §2.7 に閉じ、どの分岐にも届かない。届かない分岐は cram で固定できず、残すと §16.3 の記事が実際より多くの失敗を説明することになる。万一漏れたときは受け皿の最後の節が「内部エラー: 予期しない例外です」と終了状態 3 で報告するので、漏れたことは分かる。

判断事項 P[V121-3]: 位置はトークンの開始位置か、不正なバイトの位置か。
推奨案: 不正なバイトの位置(`lexing_position_curr`)。
代案: トークンの開始位置(ほかの字句エラーが使う `cur_pos`)。
推奨の根拠: `cur_pos`(lexer.ml 216 行)は `fst (Sedlexing.lexing_positions lexbuf)`、つまり直前の照合の開始位置である(ソースを読んで導いた性質)。
文字列リテラルとブロックコメントは 1 文字ずつ照合するので、`cur_pos` も不正なバイトそのものを指す。
1 回の照合で長く読む形(識別子、行コメント、数値)では、照合の開始位置が不正なバイトから離れる。
`cur_pos` を使う変種での実測は次のとおりである。
`let s = "abcdefghij\377"` はどちらも 1:20、`/* abcdefghij \377 */` はどちらも 1:15 で一致する。
`let x = 1 // abcdefghij \377` は `cur_pos` が 1:11(`//` の位置)、現在位置が 1:25 で、`let ab\377c` は `cur_pos` が 1:5、現在位置が 1:7 で離れる。
現在位置はどの形でも不正なバイトを指す。

判断事項 P[V121-4]: 文面に不正なバイトの値(たとえば `0xFF`)を出すか。
推奨案: 出さない。
代案: `lexing_bytes_position_curr` のバイト位置と、`from_filename` が読んだ原文からバイトを取り出して出す。
推奨の根拠: sedlex の lexbuf は原文のバイト列を持たないので、代案は原文を `Lexer'.t` に持たせる変更が要る。行と桁が分かれば、エディタや `od` でバイトを確かめられる。

#### (4) 仕様側の改訂案

変えない。
LangSpec §2.1 は不正なバイト列を字句エラーにすることだけを定め、§15.5 は終了状態だけを定める。
診断の書式と位置は処理系が決めることで、終了状態(2)も初期化を行わないことも変わらない。

#### (5) diktor 側の変更

コード(試作は `doc/log/261017-3-spike/V121/V121.patch`)。

- `lib/lexer.ml` §2.7 の終わり(553 行の後ろ、§2.8 の前): `read_raw_token` を包む同名の関数を足す。
  ```ocaml
  let read_raw_token lexbuf =
    try read_raw_token lexbuf
    with Sedlexing.MalFormed -> raise (Lex_error ("不正な UTF-8 バイト列です", Sedlexing.lexing_position_curr lexbuf))
  ```
- `lib/driver.ml` §16.3 `dump_tokens_file`: `try … with Sedlexing.MalFormed` を外し、`with_input` だけにする。
- `lib/driver.ml` §16.3 `parse_unit_with`: `Sedlexing.MalFormed` の節を消す。
- `lib/driver.ml` §16.6b `try_parse`: `exception Sedlexing.MalFormed` の節を消す。既存の `Lex_error (msg, pos)` の節が `<stdin>:行:桁` で報告する。
- `lib/driver.ml` §16.8 `main` の受け皿: `Sedlexing.MalFormed` の節を消す。

cram。

- 新しいファイル `test/utf8_position.t`(試作に同梱、printf で作るプログラム 12 本、コマンド 25、ブロック 8)。
  前書きの要点と内容は次のとおりで、どれも試作の処理系で実測した出力を期待値にしている。
  1. 文字列の中、識別子の途中、行コメントの中で、不正なバイトそのものを指す(2:10、1:7、1:14)
  2. 最初のトークンの前で当たってもファイル名を示す(`/* \377 */` で 1:4、2 つ目のファイルの先頭で 1:1)。HEAD ではファイル名が空になる
  3. 途中で切れた多バイト列(`\343\201` で終わるファイル)と、サロゲートを符号化した `\355\240\200` は、その先頭のバイトを指す(2:1、1:10)
  4. 前に 3 バイトの文字が 2 つあっても、桁はコードポイントで数える(1:12)
  5. sedlex の 512 コードポイントの読み込みの境界をまたいでも、桁が正しい(1:513)。境界の前後 510〜513 と 1024 は `doc/log/261017-3-spike/V121/results.new.txt` で確かめた
  6. import 先の誤りは、そのファイルの名前と位置で示す
  7. `--type-check` と `--dump-tokens` も同じ診断を出す
  8. `--repl` は `<stdin>:行:桁` で示し、セッションを続ける。`:type` の後ろの式でも行の中の桁で示す(4:8)
- 期待値が変わる既存のブロック(`doc/log/261017-3-spike/V121/runtest.log`): `test/run_exit.t` の exitutf8(63 行が `exitutf8.kel:2:10: …` になる)と、`test/verify_fixes.t` の badutf8(345 行が `badutf8.kel:2:4: …` になる)の 2 つ。ほかのブロックは変わらない。
- `test/run_exit.t` の前書き(42 行「構文エラー、字句エラー、不正な UTF-8 のバイト列は 2」)はそのままでよい。

ソースコメント。

- `lib/lexer.ml` 章の冒頭(54〜62 行): 外へ出す例外は `Lex_error` だけで、`MalFormed` は §2.7 の終わりで変えると書き換える。
- `lib/lexer.ml` §2.6(357 行): 不正な UTF-8 バイト列も §2.7 の終わりで `Lex_error` に変えると書き換える。
- `lib/lexer.ml` §2.7 の終わり: 包む関数の記事を足す(現在位置を使う理由、ファイル名が失われない理由)。
- `lib/driver.ml` §16.3(249〜263 行): 受ける失敗を 3 種類にし、不正な UTF-8 も位置つきの `Lex_error` として §16.8 の受け皿に届くと書く。
- `lib/driver.ml` §16.8 の表(1050 行): 出所の欄を「`Parse_error`、`Lex_error`(不正な UTF-8 を含む)」にする。
- 試作で `python3 tools/litwrap.py` を 2 ファイルに走らせた。`python3 tools/litcheck.py --old <HEAD の写し> <file>` は code だけが不一致(コードの変更なので想定どおり)で、lex、block、width は通り、marker は 0 件である。

コミットの分け方(diktor、ja/ ブランチなので日本語)。

1. `diktor: 不正な UTF-8 のバイト列の字句エラーに位置を付ける`(lexer.ml と driver.ml のコードとコメント)
2. `test: 不正な UTF-8 の字句エラーの位置の期待値を改める`(run_exit.t と verify_fixes.t の 2 ブロック)
3. `test: 不正な UTF-8 の字句エラーの位置を utf8_position.t で固定する`

その後、親で `diktor: 不正な UTF-8 の字句エラーに位置を付ける(サブモジュールを進める)` を記録する。

合成での確認(実測): V115 と X-E8 の後の写しに本項目の差分を当てると、`lib/driver.ml` の最初の hunk(`dump_tokens_file`)が fuzz 2 で当たり、ほかはそのまま当たった。
当たった結果は試作と同じで、`dune runtest` は差分なしで通った。
期待値が変わる既存の行は、合成後も exitutf8 と badutf8 の 2 行だけだった。

#### (6) リスク

- §16.5 の API(`parse_string` を使う関数)は、不正な UTF-8 で `Parse_error` ではなく `Lexer.Lex_error` を投げるようになる。ほかの字句エラーは HEAD でも `Lex_error` のまま出ているので、そろう方向の変化である。リポジトリの中にこの API の呼び出し側は無い(driver.ml 540 行の記事)。
  driver.ml 530〜532 行の記事は、この API が `Parse_error` などを例外のまま出すと書く。本項目の後は不正な UTF-8 で `Lex_error` が出るが、ほかの字句エラーは HEAD でも `Lex_error` で、記事は既に `Lex_error` を挙げていない。試作はこの記事に触れていない。実施のときに `Lex_error` を書き足すかは任意で、書き足すならコメントだけの変更なので litcheck で確かめる。
- sedlex を上げて `refill` の振る舞い(前置部分を返してから投げる)が変わると、桁がずれうる。cram の 1:513 と 1:12 のブロックが検出する。検出したときは P[V121-1] の代案 2(入口で原文を先に検査する)に切り替えられる。
- `read_raw_token` を同名で包むので、後から §2.7 の中で生トークンを読む関数を足すと、包む前の関数を呼んでしまう。包む関数の記事に「層 2 と層 3 はこの関数を通してだけ読む」と書いた。`grep -n MalFormed lib bin` が記事の 2 か所と包む関数の 1 か所だけになることを確かめられる。
- REPL の `try_parse` は `Lex_error` のうち未閉の文字列とコメントだけを続きの行の待ちにする。不正な UTF-8 はその 2 つの文面に一致しないので `Failed` になる(実測で確かめた)。V123 で字句エラーの文面を日本語にするときは、この照合の文字列も合わせる必要があり、本項目の新しい文面はその照合に入れない。
- 試作のビルドで出る `lib/parser.mly` 88 行の Warning 60 は、HEAD の写しのビルドでも出る既存のものである。

#### (7) 検証の観点

- `doc/log/261017-3-spike/V121/ex/` の 10 本と境界の 5 本(`b510.kel` など)を変更後の処理系で走らせ、`doc/log/261017-3-spike/V121/results.new.txt` と同じ位置になるか。桁が不正なバイトのコードポイントの位置に 1 を足した値か(`od -c` で数える)。
- HEAD でファイル名が空になる 2 形(`u8_blockcomment.kel`、`ok.kel u10_first.kel`)が、変更後にファイル名を示すか。
- `dune runtest` の差分が run_exit.t の exitutf8 と verify_fixes.t の badutf8 の 2 ブロックだけか(`doc/log/261017-3-spike/V121/runtest.log`)。
- `grep -rn MalFormed lib bin` が lexer.ml の記事 2 か所と包む関数 1 か所だけか。driver.ml に残っていないか。
- 終了状態が 2 のまま、REPL がセッションを続けるか。`--prelude` に不正なファイルを渡した形(`badprelude.kel:2:10`)も位置が出るか。
- 4 バイト文字の後ろの不正なバイトでも、桁がコードポイントで数えられるか(試作では `u11_4byte.kel` で 1:11 を確かめたが、cram には入れていない)。

#### (8) 検証の反映

反証者は試作の差分を HEAD の写しに当て、ビルドと `dune runtest` が通ること、変わる既存のブロックが 2 つだけであること、正しい UTF-8 の入力 11 本などで出力が変わらないことを確かめた。
退行の候補は無かったので、試作(`doc/log/261017-3-spike/V121/V121.patch`)は変えていない。

- P[V121-3] の根拠の誤り(`cur_pos` は文字列とブロックコメントの中でも不正なバイトを指し、離れるのは識別子と行コメントと数値のように 1 回の照合で長く読む形だけ): 反映した。(3) の根拠を、lexer.ml 216 行の `cur_pos` の定義と反証者の 4 形の実測に置き換えた。推奨案(現在位置)は変わらない。
- (6) の「§16.5 の記事 538 行」: 反映した。driver.ml 540 行に直した。
- P[V121-1] の見落とした代案(入口で原文を先に検査する): 反映した。(3) に代案 2 として足し、(6) の sedlex の版への依存の対策として書き添えた。推奨案は変わらない。
- 整合の指摘(V115、V123、V114、V118 との隣接): 反映した。(1) の依存の段落に行番号つきで書いた。
- 整合の指摘(driver.ml 530〜532 行の §16.5 の記事): 反映した。(6) に、触れるかは任意であることと、試作は触れていないことを書いた。

