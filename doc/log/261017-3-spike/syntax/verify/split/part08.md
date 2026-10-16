### N.8 V118：構文エラーの位置が次のトークンを指す

#### (1) 判定、規模、依存、出典

判定は fix である。
LangSpec は診断の位置を規定していないので、仕様の文面は変えない。

規模は S である。
試作の差分のうち、コードは 5 ファイルで追加 58 行、削除 31 行(`lib/parser.mly` 46 行、`lib/driver.ml` 16 行、`lib/lexer.ml` 15 行、`lib/syntax.ml` 10 行、`lib/tree.ml` 2 行。どれも差分の行数)である。
ほかに、ソースコメントを 12 か所で改め、cram を 1 本足す。

依存と重なりは次のとおりである。

- V115(「パースエラー」と「構文エラー」の用語)：本項目が足す文面と `parse_error_message` も、V115 の言い換えの対象になる。同じ `lib/driver.ml` の `parse_unit_with`(264 行)と `try_parse`(767〜773 行)を変え、本項目が変える exitparse と nl の期待値の行も V115 が変える。最終稿は V115 を後に置くとしていたが、分野の順序では V115 を先に入れる(統括者の指示と V115 の (5))。本項目の文面ははじめから V115 の形(`構文エラー: <本文>`)で書く(P[V118-1] の文面を改めた)。
- V60(末尾ブロックのパースエラーの案内)、V88(予約語の診断)：どちらも `Parser'.Error` の報告を変える。本項目の `parse_error_message` を共通の出口にし、案内の文面をそこへ足す形にそろえる。V60 の 7 形と V88 の `let from = 1` は同じ行で合わなくなる形なので、主な位置は HEAD のままである(反証者の実測。sameline と同じ分岐)。V60 や V88 が menhir の incremental API などで `Lexer'.parse` を置き換えるときは、トークンの供給を `Lexer'.read` のまま保つ(`before_ep` と `before_cont` を更新するのが `read` なので、別の供給にすると位置が食い違う)。V88 は `parse_program` で `Lexer'.read` から供給するので、この条件を満たす。ただし V88 の予約語の判定は次のトークンを余分に読むので、`before_ep` と `before_cont` を読む前の値に戻す(D[V88-3])。V88 の後は `Syntax.Syntax_error` を照合する V88 の 2 か所も `Syntax_error_at` だけにする。
- X-F5b(数値の綴り間違い)：同じ `lib/lexer.ml` §2.8 の状態と §2.11 の `read`、同じ名前の関数 `parse_error_message` を足す。1 つの関数にまとめ、数値の判定を先、本項目の位置の判定を後に置く(D[V118-3])。
- V121(不正な UTF-8 の位置)、V123(エスケープの文面)：`test/run_exit.t` の exitutf8、exitlex は本項目が変える exitparse(45〜48 行)の隣のブロックである。`driver.ml` の `MalFormed` の節(268〜269 行)は `last_sp.pos_fname` を読むが、本項目は変えない(V121 がこの節を消す)。衝突は隣の行だけである。
- X-E13(読み込みの段の限度)：`parse_unit_with` と `try_parse` に `Stack_overflow` と `Out_of_memory` の節を足す。本項目の節とは独立で、`read_limit_message` の後ろに `parse_error_message` を置く。
- `Tree.ElabData`(tree.ml §5.2)に関数 `loc` を足すので、同じ節を触る項目があれば合わせる。一覧の項目(V97、V101、V102 など elab 側)とはコードの衝突は無い。

出典は台帳の 261010-1-plan.md 5277 行(replfail の実測)、5577〜5578 行(台帳の記述)、5908 行(延期)、261010-2-m53.md 705 行(延期のまま)である。

#### (2) 現状(根拠つき)

構文の誤りの報告には 2 つの経路があり、どちらも位置が次のトークンに流れる。
例と結果は `doc/log/261017-3-spike/V118/ex/`、`doc/log/261017-3-spike/V118/verify/ex/`、`doc/log/261017-3-spike/V118/results.head.txt` にある。

1 つ目は、文法には合うが意味アクションが形を拒否する `Syntax.Syntax_error` である(ソースを読んで導いた性質)。
この例外は位置を持たず、`lib/driver.ml` §16.3 の `parse_unit_with`(264〜274 行)と §16.6b の `try_parse`(772 行)が、字句解析器の `last_sp`(最後に渡したトークンの開始位置)で補う。
menhir は規則を還元するために次のトークンを先読みするので、`last_sp` は拒否した構文の後ろのトークンになる。
`lib/parser.mly` の投げる箇所は 13 か所ある。
`dot_select`(172 行)、`variant_payload`(227 行)、§3.7 の `block_of_items`(313、315、318 行)、§3.8 の `append_last_arg`(357 行)と `with_splice`(366 行)、`variant_pat_payload`(412 行)、§3.10 の `set_pub`(455、456 行)、`resume` の規則(1021、1022 行)、レコード更新の規則(1093 行)である。
このうち 2 か所は到達できない(ソースを読んで導いた性質)。
`set_pub` の `DExp` の分岐(456 行)は、`decl_body`(664〜688 行)が `DExp` を作らないので届かない(`pub 1` は 1:5 の `Parser'.Error` になる)。
`append_last_arg` の分岐(357 行)は、`record_of_args` が `RecordEmpty` と `RecordExtend` だけを組むので届かない。
§3.7 の 303 行のコメントは「位置は最後の分岐と同じく、パーサが最後に読んだトークンになる」と書き、この振る舞いを認めている。
`block_of_items` が位置を出せないのは、§1.13 の `Syntax.Data` の署名が `allocate` しか持たず、パーサから宣言のノードの位置を読めないからである(parser.mly 291〜292 行のコメント)。

HEAD での実測(HEAD の処理系にファイルを渡す)を示す。

| 例 | 誤りの位置 | HEAD の報告 |
|---|---|---|
| `doc/log/261017-3-spike/V118/ex/s10_blkdecl_long.kel`(2 行目のブロックの中の `newtype`、ブロックは 7 行目まで) | 2:3 | 7:2 |
| `doc/log/261017-3-spike/V118/ex/s11_pubrec.kel`(2 行目の `pub let rec`、ファイルの末尾が 4 行目) | 2:3 | 5:1 |
| `doc/log/261017-3-spike/V118/ex/s3_withnc.kel`(2 行目の `with x = 1`) | 2:12 | 4:1 |
| `doc/log/261017-3-spike/V118/ex/s7_reslbl.kel`(6 行目の `resume(x = 1)`、空行とコメントの後ろの 9 行目に次の節) | 6:17 | 9:3 |
| `doc/log/261017-3-spike/V118/ex/s5_pubinst.kel`(2〜4 行目の `pub type instance`) | 2:1 | 4:2 |
| `doc/log/261017-3-spike/V118/ex/s8_upd.kel`(`{Foo with a = 2}` の `Foo`) | 2:10 | 2:25 |

対話的な実行では、入力が確定するのはその行の終わりの `EOF` を読んだ後なので、位置は次の入力の先頭になる。
`doc/log/261017-3-spike/V118/ex/repl_b.txt` の 6 行目の入力は `<stdin>:7:1`、`doc/log/261017-3-spike/V118/ex/repl_c.txt` の 2 行目の入力は `<stdin>:3:1` と報告される(`doc/log/261017-3-spike/V118/results.repl.txt`)。
既存の cram もこの位置を期待値に固定している(`test/import.t` の pb1、pb2、`test/verify_fixes.t` の reslabel、res2、`test/premise.t` の pr15、`test/nonfeatures.t` の updbase3、`test/invariants.t` の repl_b)。

2 つ目は、menhir が次に進めなかった `Parser'.Error` である。
報告の位置は文法に合わなかったトークン(`last_sp`)の開始位置で、それ自体は正しい。
ただし ASI(`lib/lexer.ml` §2.10 の `read_token`、882〜897 行)が改行を捨てると、合わなかったトークンは空行やコメントを越えた先の行にある。
改行を捨てる理由は 3 つある。
抑制の region(丸括弧、角括弧、レコード、`case` の節)の中にいる、直前のトークンが `can_end_statement` を満たさない(行末の二項演算子、カンマ、開き括弧)、次のトークンが `can_begin_statement` を満たさない(行頭の `)`、`}`、演算子)、である。
実測では、`doc/log/261017-3-spike/V118/ex/parse_err.kel`(`let x = 1 +`、空行 2 行、`let b = 2`)は 4:1、`doc/log/261017-3-spike/V118/ex/parse_err2.kel`(`let x = (1, 2`)は 3:1、`doc/log/261017-3-spike/V118/ex/p_comma.kel`(括弧の中の引数の間のカンマの抜け)は次の引数の 4:3、`doc/log/261017-3-spike/V118/ex/p_stray.kel`(`let x = 1` の次の行頭の `)`)は 2:1、`doc/log/261017-3-spike/V118/verify/ex/e5.kel`(`let f() = { … }` の次の行頭の余った `}`)は 5:1 と報告される。
前の 3 つは誤りが前の行にあり、後の 2 つは合わなかったトークン自体が誤りである。
対話的な実行でも同じ形が起きる。
`let x = (1 +` ⏎ `)` と入力すると、1 行目は括弧の中で続きを待ち(`Incomplete`)、2 行目の `)` で合わなくなって `<stdin>:2:1` と報告される(`doc/log/261017-3-spike/V118/verify/ex/r1.txt`)。
字句解析器の状態(§2.8)は直前のトークンの終了位置 `last_ep` を控えているが、どこからも読まれていない(564 行のコメント)。

#### (3) 判断と確定

D[V118-1]：位置を持たない `Syntax.Syntax_error` を廃し、13 か所をすべて `Syntax_error_at` にする。
各箇所が指す位置は、拒否した構文の始まりとする。
到達できない 2 か所(`set_pub` の `DExp`、`append_last_arg`)も、関数を全域に保つために同じく位置付きの構文エラーにする(内部の誤りの例外に変える案は、到達しない分岐の扱いを変えるだけで利用者に見える違いが無いので採らない)。

| 箇所 | 指す位置 |
|---|---|
| `block_of_items` の 3 分岐 | 拒否した宣言の始まり(`pub` を含む) |
| `with_splice` | `with` の右辺の式の始まり |
| `append_last_arg`(到達しない) | `with` の文、または末尾ブロックを付けた呼び出しの始まり(`sloc`) |
| `dot_select` | 大文字のラベルの始まり(呼び出し元から `$startpos($3)` を渡す) |
| `variant_payload`、`variant_pat_payload` | `#Foo(` の始まり |
| `set_pub`(`DExp` の分岐は到達しない) | `pub` の始まり(`decl` の規則から `$startpos` を渡す) |
| `resume` の 2 分岐 | `resume` の始まり |
| レコード更新 | 基底の始まり |

D[V118-2]：`Syntax.Data` の署名に `val loc : t -> Location.span` を足す。
`Tree.ElabData` は `loc` フィールドを返し、`EmptyData` は `t = Location.span` にする。
パーサは `start_of (d, _) = (Data.loc d).Location.start` でノードの始まりを読む。
ほかの方法(`items` の要素に位置を対で持たせる)は、`items` を共有する module の本体と instance の本体まで型を変えることになる。

判断事項 P[V118-1]：`Parser'.Error` で、合わなかったトークンが直前のトークンより後ろの行で始まるとき、どの位置を報告するか。

- 推奨案(代案 4 を採る)：改行が「続き」として捨てられた形、すなわち直前のトークンを読んだ後の region が抑制の region であるか、直前のトークンが `can_end_statement` を満たさないときに限り、直前のトークンの終わりを主な位置にし、合わなかったトークンの位置を括弧の中に添える。文面は `ファイル:行:桁: 構文エラー: この位置の後ろ、行:桁 のトークンが文法に合いません` とする(最終稿の `パースエラー(この位置の後ろ、…)` を V115 の形に改めた)。それ以外(同じ行、区切りに昇格した改行の後ろ、次のトークンが文を始められないことだけで改行が捨てられた形)は HEAD と同じく合わなかったトークンを指す。
- 代案 1：行をまたぐときはどの形でも、直前のトークンの終わりを主な位置にする(下書きの推奨案)。
- 代案 2：行をまたぐときはどの形でも、合わなかったトークンを主な位置にし、直前のトークンの終わりを括弧の中に添える。
- 代案 3：直前のトークンの終わりだけを報告する。
- 代案 5：`Parser'.Error` は変えず、`Syntax_error` の側だけ直す。
- 推奨の根拠：改行を捨てた理由が、どちらのトークンが誤りに近いかを分ける。直前のトークンが続きを求めている形(行末の `+`、カンマ、開いた括弧の中)では、誤りは前の行にある(`doc/log/261017-3-spike/V118/ex/parse_err.kel`、`doc/log/261017-3-spike/V118/ex/parse_err2.kel`、`doc/log/261017-3-spike/V118/verify/ex/e1.kel`〜`e4.kel`、`test/run_exit.t` の exitparse)。直前のトークンが文を終えられ、次のトークンが文を始められないことだけで改行を捨てた形では、行頭の余った `)` や `}` のように合わなかったトークン自体が誤りである(`doc/log/261017-3-spike/V118/ex/p_stray.kel`、`doc/log/261017-3-spike/V118/ex/p_stray2.kel`、`doc/log/261017-3-spike/V118/verify/ex/e5.kel`)。代案 1 は後者で主な位置を前の行へ外す。閉じ括弧の数え違いはよくある誤りなので、この外れは小さくない(頻度の実測は無い)。代案 2 は前者で外す。代案 3 は後者で手がかりを失い、代案 5 は台帳の「次のトークンを指す」の半分を残す。区切りに昇格した改行の後ろを除くのは、文の区切りの後ろで合わなかったトークンはそれ自体が誤りだからであり、`prev = Some NL` を「続きでない」と数えれば自然に除かれる。

推奨案で確定する。
実装は、字句解析器の `read` が、次のトークンを読む前に `last_ep` を `before_ep` に写し、region の先頭と `prev` から `before_cont`(直前のトークンの後ろの改行を続きとして捨てる状態だったか)を決め、`Lexer'.error_positions` が `before_cont` と行番号の比較で 2 つの位置を決める。
推奨案の実測(試作)は、parse_err 1:12、parse_err2 1:14、p_comma 3:4、e1 2:12、e2 1:12、e3 3:4、e4 2:8、e6 2:6 で、いずれも括弧の中に HEAD の位置を添える。
p_stray 2:1、p_stray2 3:3、e5 5:1、e7 1:9 は HEAD と同じである。
括弧の中のカンマの抜け(p_comma、`test/import.t` の nl)は 2 つのトークンの間の誤りで、どちらを指しても近さは同じだが、抑制の region の中なので前の行を指す。

D[V118-3]：`Parser'.Error` の報告は `lib/driver.ml` §16.3 の `parse_error_message` 1 つにまとめ、`parse_unit_with` と `try_parse` の両方がそれを呼ぶ。
判定の順は次のとおりとする。

1. V88 の予約語の誤用(`parse_program` の中で判定し、当たれば `Syntax_error_at` を投げるので、`parse_error_message` には来ない)。
2. X-F5b の数値の綴り間違い(`Lexer'.glued_number`。当たれば数値の開始位置と数値を名指す文面)。
3. 本項目の位置(`Lexer'.error_positions`。続きの形で行をまたげば直前のトークンの終わりと、括弧の中に合わなかったトークンの位置)。

数値の判定は隙間の無いトークンの並びでだけ成り立ち、本項目の 2 つの位置は行をまたぐときだけ分かれるので、2 と 3 は同じ入力で競合しない(ソースから導いた性質)。
1 と 3 は競合しうる。
行をまたいだ続きの後ろに予約語を書いた `let x = 1 +` ⏎ ⏎ `then` は、予約語の文面と予約語の位置(3:1)になり、本項目の位置(1:12)にはならない(合成で実測。`doc/log/261017-3-spike/syntax/ex/results.txt`)。
予約語の誤用の文面は誤りの種類を名指すので、こちらを先にする。

#### (4) 仕様側の改訂案

変えない。
LangSpec §15.5(1603 行から)は字句エラーと構文エラーの終了状態と出力先を決めるが、診断の位置と文面は規定していない。

#### (5) diktor 側の変更

コード(試作 `doc/log/261017-3-spike/V118/V118.patch` のとおり):

- `lib/syntax.ml` §1.13：`Data` に `loc` を足し、`EmptyData` を `t = Location.span` にする。§1.1 の `exception Syntax_error` を消す。
- `lib/tree.ml` §5.2：`ElabData.loc` を足す。
- `lib/parser.mly` §3.3：`start_of` と `syntax_error_at` を足す。§3.4 `dot_select` は `(id_pos, id)` を受ける(呼び出し 3 か所)。§3.5、§3.7、§3.8、§3.9、§3.10、§3.19、§3.20 の 13 か所を D[V118-1] の表のとおりにする。`decl` の規則は `set_pub $startpos $2` と書く。
- `lib/lexer.ml` §2.8 と §2.11：状態に `before_ep` と `before_cont` を足し、`read` で更新する。`error_positions` は `parse` の後ろ、`--dump-tokens` のコメントと `all_tokens` の前に置く。
- `lib/driver.ml` §16.3：`parse_error_message` を足し、`parse_unit_with` の `Parser'.Error` の節をそれにする。`Syntax_error` の節を消す。§16.6b の `try_parse` も同じにする。X-F5b と同じマイルストーンに入れるので、`parse_error_message` は D[V118-3] の順で 1 つにまとめる。
- `lib/driver.ml` §16.3a(V88)：`misused_reserved_word` で `before_ep` と `before_cont` を退避して戻し、`Syntax.Syntax_error` の照合を消す。

cram:

- 新しいファイル `test/syntax_error_pos.t`(試作のとおり、138 行)。前書きのブロックが 9 個、コマンドが 39(`export` 1、`.kel` を作るコマンド 17、`sed` で `reslabel.kel` から `res2.kel` を派生させるもの 1、`diktor` の実行 20)。プログラムは 17 本(`cat` 2 本、`printf` 15 本)で、ほかに派生の `res2.kel` と、対話的な実行への入力 2 本がある。前書きの要点は、形を拒否する構文エラーは構文の始まりを指すこと、続きを求める形で行をまたいで合わなくなったトークンは直前のトークンの終わりを主な位置にすること(opend、paren、comma)、行頭の余った閉じ括弧は合わないトークンそのものを指すこと(stray、strayb)、同じ行と区切りの後ろでは合わないトークンを指すこと(sameline、afternl)、対話的な実行でも次の入力の先頭を指さないこと(ブロックの中の宣言と、括弧の中で続きを待った入力の 2 本)。HEAD の処理系で走らせると、stray、strayb、sameline、afternl 以外の 16 の実行が落ちる(`doc/log/261017-3-spike/V118/newtest.at_head.log`)。
- 期待値が変わる既存のブロック(実装だけを当てて HEAD の期待値で走らせた差分)：`test/run_exit.t` の exitparse(3:1 から 2:13)、`test/import.t` の pb1(2:1 から 1:11)、pb2(3:1 から 2:1)、nl(3:3 から 2:4)、`test/verify_fixes.t` の reslabel と res2(7:3 から 6:17)、`test/premise.t` の pr15(3:1 から 2:1)、`test/nonfeatures.t` の updbase3(3:1 から 2:10)、`test/invariants.t` の repl_b(`<stdin>:7:1` から `<stdin>:6:14`)。6 ファイル 9 行で、ほかの `パースエラー` の期待値は変わらない。
- 前書きを改める既存のブロック：`test/import.t` の 84〜85 行の「位置はパーサが最後に読んだトークンである」を、宣言の始まりを指す旨に改める。

ソースコメント(改める箇所。どれも実測した位置に合わせて書き、litwrap と litcheck を走らせる):

- `lib/syntax.ml` §1.1(114〜127 行。`Syntax_error` の説明を消し、`Syntax_error_at` だけにする)、§1.13(807〜811 行。`loc` と `EmptyData` の形)。
- `lib/tree.ml` §5.2(`loc` の読み手にパーサを足す)。
- `lib/parser.mly` 54 行(`EmptyData` を渡すとノードの注釈は unit になる、を `Location.span` に改める)、§3.2(87 行)、§3.3(`start_of`)、§3.7(285〜303 行。「パーサからは `Data.t` の中身を読めない」は成り立たなくなるので、`Seq` に `mk sloc` を使う理由を共有の問題だけで書き直し、末尾の「位置はパーサが最後に読んだトークン」を消す)、§3.12(507〜511 行。捕まえる例外を数え直す)、§3.20(1065〜1069 行)。
- `lib/lexer.ml` §2.8(557 行の「状態が 5 つある」と一覧を `before_ep`、`before_cont` を含めて数え直し、562〜564 行の「`last_ep` はどこからも読まれていない」を消す)、§2.11(`read` が控える位置と `error_positions`)。
- `lib/driver.ml` 169〜172 行(`Parser'.Error` の位置は最後のトークンの位置からしか取れない、を 2 つの位置の決め方に改める)、§16.3(249〜262 行の 4 種類の一覧と位置の決め方)、§16.6b。
- `lib/elab.ml` §11.43(5497 行と 5534 行の `Syntax_error` への言及)。

コミットの分け方(diktor、ブランチ ja/bleeding なので日本語):

1. `diktor: 構文の形を拒否する誤りに拒否した構文の始まりの位置を付ける`(syntax.ml、tree.ml、parser.mly、driver.ml の `Syntax_error` の節と、関わるコメント)
2. `test: 構文の形を拒否する誤りの位置の期待値を改める`(既存の 5 ファイル 7 行と import.t の前書き)
3. `diktor: 続きを求める形で行をまたいで合わなくなったトークンを直前のトークンの終わりで報告する`(lexer.ml、driver.ml の `Parser'.Error` の節とコメント)
4. `test: 行をまたぐパースエラーの位置の期待値を改める`(run_exit.t の exitparse、import.t の nl)
5. `test: 構文エラーの位置の cram を足す`(`test/syntax_error_pos.t`)

V88 の `misused_reserved_word` の退避と照合の手直しは、コミット 1(`Syntax_error` を消す)と 3(`before_ep` と `before_cont` を足す)にそれぞれ含める。
`reserved_word.t` の norhs の期待値の変化(下)は、コミット 4 に含める。

合成での手直し(実測): V115、X-E8、V121、V123、X-E13、V88、V60 の後の写しでは、`lib/driver.ml` の 2 つの hunk と、`test/import.t` の nl、`test/run_exit.t` の exitparse の hunk が当たらない(V115 の文面と V88 の駆動のため)。
`lib/lexer.ml` の 4 つの hunk は当たり、`error_positions` は V88 が `parse` を消した後の `read` の後ろに入った。
`lib/parser.mly`、`lib/syntax.ml`、`lib/tree.ml`(X-E8 の後で 2 行ずれ)と、ほかの期待値の hunk はそのまま当たった。
driver.ml は上のとおり V115 の文面で `parse_error_message` を足し、V88 の 2 か所を手直しした。
promote の前の差分は、nl と exitparse の 2 行(位置と文面)、`syntax_error_pos.t` の 8 行(文面だけ)、`reserved_word.t` の norhs の 1 行だった。
norhs(`let x =` ⏎ `let y = 2`)は `norhs.kel:2:1: 構文エラー: 付近のトークンを確認してください` から `norhs.kel:1:8: 構文エラー: この位置の後ろ、2:1 のトークンが文法に合いません` に変わる。
`=` は文を終えられないので、P[V118-1] の推奨案のとおり続きの形になる。
既存の期待値の 9 行(最終稿の (5) の一覧)は、合成後も同じ位置に変わった(N.z の表)。

本項目の試作の差分はコードと cram だけで、上に挙げたソースコメントの改訂は含まない。
合成差分にも含まれないので、実施のときに書く(`Syntax_error` を説明するコメントは合成後も syntax.ml の 114、120 行、parser.mly の 87、530、1125 行、driver.ml の `parse_with` の記事に残っている)。

親では、サブモジュールの版を進めるコミットを 1 つ作る。

#### (6) リスク

- 対話的な実行の入力の終わりの判定(`prev = Some EOF`)は `prev` を見るので変わらない。`doc/log/261017-3-spike/V118/ex/repl_c.txt` の開いた括弧と行末の演算子の続きは、試作でも続きを待った(`doc/log/261017-3-spike/V118/results.repl.txt`)。`doc/log/261017-3-spike/V118/verify/ex/r1.txt` は試作で `<stdin>:1:13: パースエラー(この位置の後ろ、2:1 …)` になり、新しい cram に固定した。
- 抑制の region の中で次の行の行頭に余った閉じ括弧を書いた形(`f(1 ⏎ ))` の類)は、推奨案では前の行の終わりを主な位置にする。括弧の中に合わなかったトークンの位置を添えるので手がかりは残る(ソースを読んで導いた性質。未実測)。
- `with x = e ⏎ 残り` の規則では、OCaml の引数の評価順(右から左)により、`block_of_items` が `with_splice` より先に走る。`with` の右辺と後ろのブロックの両方に誤りがあると、後ろの誤りが先に報告される。HEAD でも同じ順で、位置が付いたことで目に見えるようになるだけである(ソースを読んで導いた性質。未実測)。
- 推奨案から代案 1 または代案 2 に切り替えるときは、`error_positions` の条件か対を変えるだけで済む。
- V88 の予約語の判定が次のトークンを余分に読むので、`before_ep` と `before_cont` を戻し忘れると、予約語で詰まった入力で位置がずれる。合成で戻さない変種を測り、norhs が 2:1 に戻ることを確かめた(`doc/log/261017-3-spike/syntax/ex/results.norestore.txt`)。
- 行をまたいだ続きの後ろの予約語は、本項目の位置ではなく V88 の文面になる(D[V118-3])。
- `Data.loc` を足すと、§3.7 の `Seq` に span を組む道が開くが、本項目では `Seq` の作り方を変えない。変えると `--dump-ast` と型エラーの位置が動く。

#### (7) 検証の観点

- 試作の `dune runtest` で、既存の期待値の差分が (5) に挙げた 9 行だけであること(実装だけを当てて HEAD の期待値で走らせる)。
- 到達できる 11 か所のそれぞれについて、拒否した構文の後ろに空行、コメント、次の宣言、ファイルの末尾を置いた形で、位置が構文の始まりに留まること(`doc/log/261017-3-spike/V118/ex/s*.kel`)。到達できない 2 か所(`set_pub` の `DExp`、`append_last_arg`)は、`decl_body` と `record_of_args` の作る形を読んで、届かないことを確かめる。
- `Parser'.Error` の 4 つの場合(同じ行、区切りの改行の後ろ、続きを求める形で行をまたぐ、次のトークンが文を始められないことだけで行をまたぐ)が推奨案の位置になること。行をまたぐ続きの場合は、括弧の中の位置が合わなかったトークンの開始位置であること。
- 対話的な実行で、`:type` の後ろの式(`try_parse ~col`)の構文エラーの桁がずれないこと。試作では `:type {Foo with a = 1}` が 2:8、`:type r.Foo` が 3:9 で、どちらも `Foo` の桁だった(`doc/log/261017-3-spike/V118/ex/repl_type.txt`)。
- `--dump-tokens` の出力が変わらないこと(`test/tokens.t`)。
- HEAD が受理するプログラム(`doc/log/261017-3-spike/V118/verify/ex/g1〜g7`、`g8_repl.txt`、`test/sample/sample.kel`)の実行、`--type-check`、`--dump-ast` の出力と終了状態が変わらないこと。
- 試作のコードに `Syntax_error` の参照が残らないこと(`grep -n 'Syntax_error\b'` がコメントの改訂後に何も出さないこと)と、`tools/litcheck.py` が通ること。

#### (8) 検証の反映

- 新しい cram の数の誤り：反映した。試作の改訂後の数(138 行、コマンド 39、プログラム 17 本と派生 1 本と対話的な入力 2 本)を (5) に書いた。`res2.kel` は `sed` で作る派生として数えを分けた。
- (2) の「この形はファイルの実行と型検査で起きる」：反映した。対話的な実行の `let x = (1 +` ⏎ `)` を (2) に書き、cram の対話的な実行の節にブロックを足した(試作 `<stdin>:1:13`、HEAD `<stdin>:2:1`)。
- `error_positions` の置き場所：反映した。試作で `parse` の後ろ、`--dump-tokens` のコメントの前に移した。
- 改めるコメントの一覧の漏れ(parser.mly 54 行、driver.ml 169〜172 行、lexer.ml 557 行)：反映した。(5) の一覧に足した。
- 到達できない 2 か所と (7) の「13 か所のそれぞれ」：反映した。(2) と D[V118-1] に到達しないことを書き、(7) は 11 か所を例で、2 か所を読んで確かめる形にした。内部の誤りに変える案は採らない理由を D[V118-1] に書いた。
- 推奨案への異論(代案 4)：反映した。異論のとおり、余った閉じ括弧の形で主な位置が前の行に外れるので、推奨案を代案 4 に改めた(抑制の region の中の改行も「続き」に数える点で、反証者の述べた形より 1 つ広い。その結果 paren と p_comma は前の行の終わりを指す)。試作の `lexer.ml` を改め(`before_nl` を `before_cont` に置き換え)、`dune build` と `dune runtest` が差分なしで通ること、反証者の `doc/log/261017-3-spike/V118/verify/ex/e1〜e8.kel` と `r1.txt` の位置を (3) のとおり確かめた。e5 と同じ形を cram の strayb として足した。
- 整合の指摘(V115、V60、V88、V121、V123、ElabData)：反映した。(1) に書いた。`test/import.t`、`test/invariants.t`、`test/verify_fixes.t` の変わるブロックに重なる項目は一覧に無い。
- 退行の候補：反証者は無しと報告した。推奨案の変更で既存の期待値の差分は 9 行のまま変わらず、新しい cram の stray だけが HEAD の位置に戻った。

