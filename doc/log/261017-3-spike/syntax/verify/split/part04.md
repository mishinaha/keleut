### N.4 V123：字句エラーの文面を日本語にそろえる

#### (1) 判定、規模、依存、出典

判定は fix である。
LangSpec は字句エラーの文面を定めていないので、仕様の文面は変えない。

規模は S である。
試作の差分は `lib/lexer.ml` と `lib/driver.ml` で約 40 行、cram は既存 3 ファイルの期待値(コマンド 17、ブロック 7)と新しいファイル 1 本である。

依存は無い。
適用の順は V121 の後を勧める(理由は下の V121 の項)。
重なりは次のとおりである。
V121、V115、V118、X-F5b とは差分が機械的に当たらない箇所があり、後に当てる側で手で合わせる。

- V121(不正な UTF-8 の位置)：V121 の差分も `lib/lexer.ml` を変える。冒頭の記事の 54〜58 行の段落を書き換え、§2.7 の 552〜553 行の直後に `read_raw_token` の包みを足し、そこで `Lex_error ("不正な UTF-8 バイト列です", ...)` を投げる。V123 を当てた写しに `V121.patch` を `patch --dry-run` で当てると、lexer.ml の hunk 1(51 行、冒頭の記事)が失敗し、hunk 3(552 行の後)は fuzz 2 で当たる(実測)。driver.ml の hunk はすべて当たる。したがって「衝突しない」は成り立たない。V121 の後では lexer.ml の `Lex_error` の文面は 9 種になり、9 種目は初めから日本語である。冒頭の記事の段落は、V121 の加筆(不正な UTF-8 も位置つきの `Lex_error` になること)と V123 の加筆(文面が日本語の文であることと 2 つの定数)を合わせて書き直す。`test/run_exit.t` でも exitutf8(exitlex の 4 行下)を共に変える。
- V115(「パースエラー」と「構文エラー」)：`lib/driver.ml` の `try_parse` の 769 行と `test/run_exit.t` の exitparse(50 行、exitlex の 4 行上)、`test/repl.t` を変える。V123 を当てた写しに V115 の差分を当てると、driver.ml の 766 行の hunk が失敗する。文面の方針(診断の本文を日本語の文にする)は同じ向きである。
- V118(構文エラーの位置)：`lib/driver.ml` の `try_parse`(769〜772 行)と `lib/lexer.ml`(字句解析器の状態)を変える。V123 を当てた写しで driver.ml の 771 行の hunk が失敗する。
- X-F5b(数値の綴り間違いの診断)：`lib/driver.ml` 769 行、`lib/lexer.ml`、`test/tokens.t` を変える。V123 を当てた写しで driver.ml の 774 行の hunk が失敗する。
- V88(予約語の診断)：driver.ml で `Lex_error` を文面によらず受けるので、文面の変更とは独立である。
- V114(数値の接尾辞)：`lib/lexer.ml` の §2.1 の記事を変えるが、V123 の差分と同時に当たる。

V115、V118、X-F5b の 3 件で衝突するのは、どれも `try_parse` の `match` の中の隣り合う行である。
V123 がこの関数で変えるのは 770 行の照合の 1 行だけなので、後に当てる側で 1 行を合わせ直せば済む。
分野の合成では、V115、X-E8、V121 の後に本項目を当て、当たらなかった 2 か所(lexer.ml の冒頭の記事と `try_parse` の照合の 1 行)を手で合わせた。
後に当てた V118 と X-F5b の `try_parse` の hunk も、照合の行には触れずに合わせられた(実測。N.z)。

出典は台帳 `doc/log/261010-2-m53.md:713`(延期、261010-2)である。

#### (2) 現状(根拠つき)

台帳の要約は「不正なエスケープだけが英語」だが、ソースを読むと誤りである。
`lib/lexer.ml` が投げる `Lex_error` の文面 8 種がすべて英語で、日本語なのは driver が出す「不正な UTF-8 バイト列です」だけである。

| 行(HEAD) | § | 関数 | 文面 |
| --- | --- | --- | --- |
| 324, 326 | 2.5 | `read_block_comment` | `unterminated block comment` |
| 372 | 2.6 | `read_unicode_escape` | `invalid unicode escape (out of range or surrogate)` |
| 381 | 2.6 | `read_unicode_escape` | `unexpected end of unicode escape` |
| 399 | 2.6 | `read_escape` | `invalid escape sequence` |
| 406, 416 | 2.6 | `read_text` | `unterminated string literal` |
| 492 | 2.7 | `read_raw_token` | `stray '#!'` |
| 552 | 2.7 | `read_raw_token` | `unexpected character: ` の後に文字 |
| 553 | 2.7 | `read_raw_token` | `unexpected input` |

報告の側は `lib/driver.ml` §16.8 の 1151〜1153 行(ファイルの実行)と §16.6b の 771 行(対話的な実行)で、どちらも `字句エラー: ` の後に文面をそのまま付ける。
§16.6b の 770 行(`try_parse`)は、英語の文面 2 つを文字列の照合で見分けて、続きの行を待つ(`Incomplete`)。
したがって文面を訳すときは、この照合も一緒に変えないと、対話的な実行で閉じていない文字列が字句エラーとして報告される退行になる(ソースから導いた性質)。

LangSpec は §2.1(26、29 行)と §4.4(295 行)で「字句エラーになる」とだけ書き、文面を定めない。
§15.5 の 1621 行は終了状態 2 を定める。

HEAD での実測(`doc/log/261017-3-spike/V123/ex/` と `doc/log/261017-3-spike/V123/results.head.txt`)：

```
$ diktor badesc.kel        # let x = "\q"
badesc.kel:1:11: 字句エラー: invalid escape sequence
$ diktor short.kel         # let s = "\u12"
short.kel:1:14: 字句エラー: unexpected end of unicode escape
$ diktor untermc.kel       # let a = 1 /* abc
untermc.kel:1:17: 字句エラー: unterminated block comment
$ diktor stray.kel         # 2 行目が #!x
stray.kel:2:1: 字句エラー: stray '#!'
$ diktor char.kel          # 1 の後に ¤
char.kel:1:11: 字句エラー: unexpected character: ¤
```

どれも終了状態 2 である。
cram で文面を固定しているのは `test/tokens.t`(err、bom 群、surr、oor、badesc)、`test/run_exit.t`(exitlex)、`test/errloc.t`(mb3、mb4)で、`unexpected end of unicode escape`、`unterminated block comment`、`stray '#!'` はどの cram にも現れない。

#### (3) 判断と確定

D[V123-1]：直す範囲は不正なエスケープだけでなく、`Lex_error` の文面 8 種すべてとする。
台帳の要約の前提(英語は 1 種だけ)が実測と合わないためで、1 種だけ訳すと残り 7 種で同じ不ぞろいが残る。

判断事項 P[V123-1]：文面をどう訳すか。

- 推奨案：次の表のとおり、短い日本語の文にする。位置と `字句エラー: ` の前置きは変えない。

| 英語(HEAD) | 日本語(確定) |
| --- | --- |
| `invalid escape sequence` | `不正なエスケープです` |
| `invalid unicode escape (out of range or surrogate)` | `Unicode エスケープの値がサロゲートか、U+10FFFF を超えています` |
| `unexpected end of unicode escape` | `Unicode エスケープの 16 進の桁が足りません` |
| `unterminated string literal` | `文字列リテラルが閉じていません` |
| `unterminated block comment` | `ブロックコメントが閉じていません` |
| `stray '#!'` | `#! はファイルの先頭にだけ書けます` |
| `unexpected character: X` | `予期しない文字です: X` |
| `unexpected input` | `予期しない入力です` |

- 代案 A：不正なエスケープの文面に、使えるエスケープの一覧(`\u \U \a \b \f \n \r \t \v \' \" \\`)を添える。親切だが、逆斜線でファイルが終わる形(esceof)も同じ分岐に来るので、一覧が的外れになる場合がある。
- 代案 B：不正なエスケープの文面に、逆斜線の後の文字を出す。同じ理由で EOF の場合の分岐が要り、規模が増える。
- 推奨の根拠：位置が文字を指すので、文字を文面に繰り返さなくても原因の箇所に届く。既存の日本語の文面(「不正な UTF-8 バイト列です」)の形に合う。

判断事項 P[V123-2]：対話的な実行が、閉じていない文字列とコメントをどう見分けるか。

- 推奨案：`Lexer` の最上位に文面の定数 `unterminated_text_msg` と `unterminated_comment_msg` を置き、字句解析器はその定数を投げ、`try_parse` は `when msg = Lexer.unterminated_text_msg || msg = Lexer.unterminated_comment_msg` で照合する。
- 代案 A：`Lex_error` の引数を文面ではなく種類のヴァリアントにし、文面は表示の関数で作る。照合が型で守られるが、`Lex_error` を受ける 3 か所と、文面を組み立てる関数を変える必要があり、規模が倍になる。
- 代案 B：`Lex_error` の位置が入力の終わりかどうかで継続を決める。`"abc\` で入力が終わる形(HEAD は不正なエスケープとして報告し、続きを待たない)を継続に変えてしまうので採らない。
- 推奨の根拠：照合する文面が 2 つだけで、定数を共有すれば訳し直しても照合がずれない。試作で確かめた。

#### (4) 仕様側の改訂案

変えない。
LangSpec は字句エラーの文面を定めず、§18 の 1782〜1783 行は診断の文面を評価の結果に含めないとしている。
終了状態 2 も変わらない。

#### (5) diktor 側の変更

コード(試作 `doc/log/261017-3-spike/V123/V123.patch`)：

- `lib/lexer.ml` の最上位(`exception Lex_error` の直後)：`unterminated_text_msg` と `unterminated_comment_msg` を足す。
- `lib/lexer.ml` §2.5 `read_block_comment`、§2.6 `read_unicode_escape`、`read_escape`、`read_text`、§2.7 `read_raw_token`：(3) の表の文面に置き換える。
- `lib/driver.ml` §16.6b `try_parse`：770 行の文字列の照合を、定数との `when` 照合に置き換える。

ソースコメント(改める §)：

- `lib/lexer.ml` 冒頭の記事(54〜56 行の `Lex_error` の段落)：文面が日本語の文であることと、2 つの定数を置く理由を 3 文足す。
- `lib/lexer.ml` §2.5(292 行)と §2.7(432、439 行)：記事が引く英語の文面を日本語の文面に置き換える。
- 写しで `tools/litwrap.py` を走らせ、`tools/litcheck.py` で marker が 0 件であることを確かめた(code の不一致はコードを変えたため)。

cram：

- 期待値が変わる既存の cram は、コマンド 17、ブロック 7 である(`doc/log/261017-3-spike/V123/runtest.log`)。内訳は `test/tokens.t` のブロック 5(err、bom、bom2、c0〜wj、surr〜badesc。コマンド 14)、`test/run_exit.t` のブロック 1(exitlex)、`test/errloc.t` のブロック 1(mb3、mb4。コマンド 2)である。どれも文面だけが変わり、位置と終了状態は変わらない。
- 新しく落ちるブロックは無い。
- 新しいファイル `test/lexerr.t`(コマンド 12、ブロック 6。.kel は cat ではなく printf で 5 本作る)。前書きの要点は次のとおり。
  - `\u12` と `\U0011`：16 進の桁が足りない(1:14、1:16、終了状態 2)。
  - 逆斜線でファイルが終わる形：閉じていない文字列ではなく不正なエスケープになる(1:11)。
  - 閉じていないブロックコメント(1:17)。
  - ファイルの先頭以外の `#!`(2:1)。
  - 対話的な実行で、閉じていないブロックコメントは続きの行を待ち、不正なエスケープは報告して次の入力へ進む。閉じていない文字列の継続は既存の `test/repl.t`(71〜78 行)が固定している。

コミットの分け方(diktor、`ja/` ブランチなので日本語)：

1. `diktor: 字句エラーの文面を日本語にする`(lexer.ml と driver.ml)
2. `test: 字句エラーの文面の期待値を日本語に更新する`(tokens.t、run_exit.t、errloc.t)
3. `test: 字句エラーの文面と対話的な実行での継続を lexerr.t で固定する`

親：`diktor: 字句エラーの文面を日本語にそろえる(サブモジュールを進める)` と、台帳の V123 を解消にする `doc:` のコミット。

合成での手直し(実測): V121 の後の写しでは、lexer.ml の冒頭の記事の hunk が当たらない。
V121 が書き直した段落(「外へ出す例外は `Lex_error` だけ」と、`MalFormed` を §2.7 の終わりで `Lex_error` に変えること)の、終了コード 2 の文の直後に、本項目の 4 行(日本語の文であることと 2 つの定数の理由)を差し込んだ。
`try_parse` の照合の 1 行は、V115 の後の文脈で `when` 照合に置き換えた。
V121 の `read_raw_token` の包みの後ろの hunk(英語の文面 2 つの置き換え)は fuzz 2 で当たった。
この写しで `dune runtest` は差分なしで通った。
合成後の lexer.ml の `Lex_error` の文面は 9 種ですべて日本語である(`grep -n 'Lex_error ("' lib/lexer.ml`)。

#### (6) リスク

- 対話的な実行の継続の退行：文面と照合の対応がずれると、閉じていない文字列やコメントが続きを待たずに字句エラーになる。定数の共有で防ぎ、`test/repl.t` の 71〜78 行と `test/lexerr.t` の最後のブロックで固定する。
- 他の項目の差分との衝突：V121 とは `lib/lexer.ml` の冒頭の記事が機械的に当たらない。V115、V118、X-F5b とは `lib/driver.ml` の `try_parse` の hunk が当たらない。V115 と V121 は `test/run_exit.t` の隣り合う行も変える。(1) のとおり V121 の後に当て、記事の段落を両方の加筆で書き直す。`try_parse` は後に当てる側で 770 行の照合を合わせ直す。
- 文面を照合する外部の道具：diktor の中には、`try_parse` のほかに `Lex_error` の文面を照合する箇所は無い(`grep -rn` で確かめた)。

試作中に見つけたこと(V123 の範囲外、実測)：対話的な実行では、各入力の先頭が字句解析器のオフセット 0 になるため、2 行目以降の入力の行頭の `#!x` も shebang として黙って読み飛ばされる(`printf 'let a = 1\n#!x\n' | diktor --repl` は `a : Int32 = 1` だけを出し、エラーにならない)。ファイルの実行では同じ入力が `stray.kel:2:1` の字句エラーになる。台帳の候補 V[V123-1] に挙げる。

#### (7) 検証の観点

- `lib/lexer.ml` に英語の `Lex_error` の文面が残っていないこと(`grep -n 'Lex_error ("' lib/lexer.ml` の結果がすべて日本語であること)。
- `doc/log/261017-3-spike/V123/ex/` の 10 本を変更後の処理系で走らせ、`doc/log/261017-3-spike/V123/results.patched.txt` と同じ文面、位置、終了状態 2 になること。位置が HEAD(`doc/log/261017-3-spike/V123/results.head.txt`)から変わっていないこと。
- 対話的な実行で、閉じていない文字列(`"x` の次の行に `y"`)と閉じていないブロックコメントが続きの行を待ち、不正なエスケープは報告されて次の入力へ進むこと。
- `dune runtest` の差分が、(5) のコマンド 17(ブロック 7)の文面だけであること。
- `tools/litcheck.py` で marker が 0 件であること。記事の中に英語の文面への参照が残っていないこと。

参照：差分 `doc/log/261017-3-spike/V123/V123.patch`、例 `doc/log/261017-3-spike/V123/ex/`、HEAD の結果 `doc/log/261017-3-spike/V123/results.head.txt`、変更後の結果 `doc/log/261017-3-spike/V123/results.patched.txt`、期待値の差分 `doc/log/261017-3-spike/V123/runtest.log`(promote 前、既存ブロック)と `doc/log/261017-3-spike/V123/runtest.prepromote.log`(lexerr.t を含む)。

#### (8) 検証の反映

反証者は doc/log/261017-3-spike/V123/ex/ の 10 本と r1〜r12 で HEAD と試作を比べ、字句エラーの文面のほかに出力と終了状態の違いが無いことを確かめた(`doc/log/261017-3-spike/V123/verify/reproduce.txt`、`doc/log/261017-3-spike/V123/verify/regress.txt`)。
退行の候補は無いので、試作 `doc/log/261017-3-spike/V123/V123.patch` は変えていない。

- LangSpec の節番号(§0 と §2 は誤りで §2.1 と §4.4)：反映した。(2) を直した。写しの 23 行が `### 2.1.`、287 行が `### 4.4.` であることを確かめた。
- V121 との重なり(lexer.ml の hunk 1 が失敗、hunk 3 は fuzz 2、文面が 9 種になる)：反映した。V123 を当てた写しで `V121.patch` を `patch --dry-run` で当て直し、同じ結果を得た。(1) の「衝突しない」を消し、冒頭の段落を両方の加筆で書き直すこと、V121 の後では文面が 9 種になること、適用の順を V121 の後にすることを書いた。(6) も合わせた。
- driver.ml の `try_parse` の重なり(V115、V118、X-F5b)：反映した。(1) に 3 件を挙げ、(6) に合わせ直しの手順を書いた。3 件の dry-run の結果は反証者の実測に拠る。分野の合成で、本項目の後に 3 件を当てて合わせ直せることを確かめた(N.z)。
- 件数の言い方(コマンド 17、ブロック 7)：反映した。(1)、(5)、(7) を直した。
- 推奨案への異論：無し。P[V123-2] の見落とした代案(`Lex_error` の位置が入力の終わりかどうかで継続を決める)は、`"abc\` で入力が終わる形を継続に変えるので採らない。この理由を P[V123-2] の代案に足した。
- 整合の指摘の V88(独立)と V114(同時に当たる)：反映した。(1) に書いた。

