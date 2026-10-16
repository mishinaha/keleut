# V115 の下書きの反証

## 裏取り

`lib/driver.ml` の 160、271、273、274、766、769、772、773 行の内容は下書きのとおりである。
ただし関数の範囲は下書きと違う。
`let parse_unit_with lexer =` は 264 行で、本体は 274 行までである(下書きは 266〜274 行)。
`let try_parse` は 757 行で、最後の分岐は 774 行である(下書きは 760〜775 行)。

「パースエラー」を含む行は lib に 7 か所(driver.ml 160、271、769、lexer.ml 780、834、parser.mly 610、612、1169、elab.ml 648 のうち文字列 2 か所を除くコメント 7 か所)で、bin には無い。
cram では 11 ファイル 52 行で、期待値 46 行、前書き 6 行(ast.t 147、if.t 75、import.t 42、spec_gaps.t 146、trailing_block.t 148、187)である。
ファイルごとの内訳も下書きと一致した。
「構文エラー: 」を含む期待値の行は 24 行である。
差分は 122 行(削除 61、追加 61)である。

LangSpec と doc/sample.kel に「パースエラー」は無い。
LangSpec 1621、1629 行は下書きのとおりである。
LangSpec 397 行は `(g(a)) { ... }` を構文エラーと呼び、diktor はこの形を menhir の段で拒否する(trailing_block.t の paren、HEAD の期待値は「パースエラー」)。
仕様の語が menhir の段の拒否も含むことの裏付けになる。

台帳の出典(261010-1-plan.md 4464、5194〜5196、5905 行、261010-2-m53.md 702 行)は下書きのとおりである。

## 実測の再現

担当の ex/ の 5 本を HEAD と試作で走らせ直した。
letnone、paren、runh は文面だけが変わり、位置と終了状態 2 は同じである。
twoblk は両方で同じ出力である。
`let = 1` の後に `1 + 1` を続けた REPL は、両方で誤りを報告して続行し、`_ : Int32 = 2` を出して終了状態 0 である。

## 試作の差分

V115.patch は写しに当たった。
試作の lib と HEAD の test のまま dune runtest を走らせると、差分は削除 46 行、追加 46 行で、すべて付近のトークンの文面の置き換えだった(runtest.unpromoted.log)。
差分の test を戻すと dune runtest は通った。
試作の test/*.t は、HEAD の各ファイルに文面の置き換えと「パースエラー」から「構文エラー」への置き換えをした結果と一致した。
dune build の Warning 60(parser.mly 88 行)は HEAD の写しでも出る。

litcheck は driver.ml で 2 つの文字列だけが不一致で、HEAD の 2 つの文字列を置き換えた版と比べるとコードが一致した。
elab.ml、lexer.ml、parser.mly はコードが一致し、marker は 0 件である。
litwrap はどのファイルも変えなかった。

## 退行の探索

HEAD で受理して正しく走るプログラムを ex/ に置き、両方で走らせた。
文字列リテラルに診断の文面を含む r1_strlit、相対 import の r2_import、制約付き多相とガードと型エイリアスの r3_module(--type-check も)、入れ子のハンドラの r4_handler、末尾ブロックの r5_trail は、出力と終了状態が同じだった。
--dump-ast と --dump-tokens の出力も一致した。
REPL での複数行の入力、`:type` の途中で終わる式、import の拒否も同じだった。
誤りのある形では、import 先の構文エラー(r6_importbroken)と --prelude に壊れたファイルを渡した形が、文面だけ変わり位置と終了状態 2 は同じだった。
退行は見つからなかった。

## 重なり

V121 は parse_unit_with の 269 行(MalFormed)を変える見込みで、V115 の 271 行と差分の文脈が重なる。
行そのものは違うが、パッチを順に当てるときに文脈の衝突が起きうる。
V60、V88 の期待値(trailing_block.t の ctor0、tag0、paren、前書き 148、187 行、if.t の kwthen、kwlabel、import.t の letfrom、nonfeatures.t の kwlabel)を V115 は書き換える。

## 判断事項

推奨案に異論は無い。
LangSpec 397 行が menhir の段の拒否も構文エラーと呼ぶので、代案 B は仕様の語の使い方と合わない。
