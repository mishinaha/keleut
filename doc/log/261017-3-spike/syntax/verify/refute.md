# syntax の節と合成差分の反証

対象は clusters/syntax/section.md と syntax.patch である。
作業の写しは verify/diktor(HEAD 39d4091 の写しに syntax.patch を patch -p1 で当てた)。
途中で /tmp の空きが無くなったので、項目ごとの試作の再ビルドはやめ、各項目の結果ファイルと合成後の処理系の出力を、V115 の文面の置き換えだけを正規化して突き合わせた。

## 1. build と runtest(実測)

- patch -p1 --dry-run で 25 ファイルとも当たり、そのまま当てた。
- dune build は通る。Warning 60(parser.mly 88 行)は HEAD の写しのビルドにも出る。
- dune runtest の差分は 0 行。
- 合成後の lib と HEAD の test/ を組み合わせて dune runtest を走らせると、14 ファイル、- 行 72、+ 行 72。終了状態の行([n])の変化は 0。
- 72 行の分類(ファイルと文面の種類で数えた): 付近のトークン 36、予約語 5(if.t 3、import.t 1、nonfeatures.t 1)、末尾ブロック 3(trailing_block.t)、この位置の後ろ 2(import.t の nl、run_exit.t の exitparse)、Syntax_error の位置だけ 7(import.t 2、invariants.t 1、nonfeatures.t 1、premise.t 1、verify_fixes.t 2)、字句エラーの日本語化 17(tokens.t 14、errloc.t 2、run_exit.t 1)、UTF-8 の位置 2。
  N.z の表のファイル別内訳と全部一致した。
- HEAD の test/*.t の「パースエラー」は 11 ファイル 52 行、期待値 46 行(45 行が同じ文面、1 行が <stdin>)、前書き 6 行(ast.t 147、if.t 75、import.t 42、spec_gaps.t 146、trailing_block.t 148、187)、「構文エラー: 」の期待値 24 行。V115 の (2) と一致。
- menhir --explain --strict --table を合成後の parser.mly に単独で走らせ、parser.conflicts は 0 バイト(menhir 20260209)。
- 合成後の lib/bin/test/tools に「パースエラー」は 0 件。MalFormed は lexer.ml の 3 か所だけ。Lex_error の文字列は 7 種と定数 2 つで 9 種、すべて日本語。

## 2. 新しい cram の数え直し(合成後)

- syntax_error_pos.t: 138 行、コマンド 39、diktor の実行 20、cat 2、printf 17(うち 2 本は対話的な実行への入力)。汎用の文面 4 行と「この位置の後ろ」4 行で 8 行。
- reserved_word.t: コマンド 55、予約語の文面 19 行、汎用 8 行と norhs の V118 の形 1 行。
- utf8_position.t: コマンド 25、プログラム 12(printf は 13、1 本は対話的な実行の入力)。
- lexerr.t: コマンド 12、プログラム 5。
- tokens.t への追加: コマンド 24、cat 2。X-F5b の試作の tokens.t の「パースエラー」を含む期待値は 14 行(HEAD 2 行)で、12 行の主張に合う。
- run_exit.t への追加: コマンド 14。
- 項目の試作の cram と合成の cram の差(文面を正規化した後): reserved_word.t は norhs の 1 行、tokens.t は V123 の字句エラーの 11 行と gluedkw の 1 行と前書き 2 か所、run_exit.t は exitparse、exitlex、exitutf8 の 3 行、syntax_error_pos.t、utf8_position.t、lexerr.t は 0 行。どれも N.z の説明どおり。

## 3. 例の実行(合成後の処理系)

- V115: letnone、runh、twoblk、REPL の let = 1 は results.new.txt どおり。paren.kel は V118 で 2:13 の「この位置の後ろ、3:1」に変わる(results.new.txt は 3:1)。
- V121: 19 本。results.new.txt にある 10 本は一致。境界の b510〜b513 は 1:511〜1:514、b1024 は 1:1025。
- V123: 10 本すべて results.patched.txt と一致。REPL の閉じていない文字列は続きを待つ。行頭の #!x の REPL での読み飛ばしも再現。
- X-E13: deep.kel(l=100k)は実行と --type-check で 1、--dump-ast で 3、既定で 0。seq5000、seqexp5000、topwith、tup5000、rec5000、imp/main.kel(l=16k)は 2 で標準出力 0 バイト。seqrepl.txt は続行。results.spike.txt、results.final.txt と一致。
- V88: 35 本。out.final.txt と一致し、違いは fp_missing_rhs.kel だけ(V118 で 1:8 の「この位置の後ろ、2:1」。norhs と同じ形)。
- V60: results.txt の spike 側 60 行と完全一致。{ x => } は受理、f { } はレコードの診断。
- V118: s3、s5、s7、s7b、s8、s10、s11 は final.md の表どおり。parse_err 1:12、parse_err2 1:14、p_comma 3:4、p_stray 2:1。results.compare.txt は代案 4 に改める前の古い結果で、final.md も参照していない。
- X-F5b: run.sh の 34 本は results.final.txt と一致(文面の頭を正規化)。e_*.kel も主張どおり。syntax/ex の k_match、k_if、k_with、k_then(次の行に let b = 2)は汎用、k_true と fif は予約語の文面。

## 4. 指摘

### 要修正

1. 合成差分の tokens.t の前書き(X-F5b の 3 つ目のブロック)が実測に反する。
   「その後ろの予約語で詰まる形は数値の綴りの誤りとはせず、予約語の誤用として報告する」と書くが、合成後の処理系で 1.2.match、1.2.if、1.2.with、1.2.then(次の行に let b = 2)は汎用の文面になる(syntax/ex/results.kw.txt、本検証でも再現)。
   N.9 の (6) はこの分かれ方を書いているのに、(5) の手直しの記述と cram の前書きは「予約語の誤用として報告する」と言い切っている。
   前書きを「数値の綴りの誤りとはしない(後ろのトークンによって予約語の誤用か汎用の文面になる)」の形に限り、N.9 (5) と N.z の記述も合わせる。
2. N.3(V121)の (1) に「`try_parse` の 768 行」とある(最終稿から写した誤り)。HEAD の driver.ml で「パースエラー」の文面は 769 行である(N.1 も 769 行と書く)。
3. 合成後の driver.ml §16.3 の parse_with の記事は、受ける失敗の 4 種類に `Syntax.Syntax_error` を挙げ、`Parser'.Error` には「最後に読んだトークンの位置を使う」と書いたままである。
   V118 の後は Syntax_error が無く、Parser'.Error の位置は error_positions が決める。
   N.z の「そのほかの確認」は parse_with の記事に Syntax_error の説明が残ることだけを書き、位置の文は挙げていない。N.5 の「記事を 4 種類にした」も、存在しない例外を 1 種類に数えた形である。
   実施の V118 のコミットで直す対象に、この 2 点(4 種類の一覧と位置の文)を明記する。
4. N.8(V118)の (6) に「`<stdin>:1:13: パースエラー(この位置の後ろ、2:1 …)` になり、新しい cram に固定した」が残る。合成の cram(syntax_error_pos.t 136 行)は `構文エラー: この位置の後ろ、2:1 のトークンが文法に合いません` である。

### 文章(軽微)

- N.8 (5) のコミットの題「行をまたぐパースエラーの位置の期待値を改める」と、N.9 (5) の 3 つのコミットの題が「パースエラー」の語を使う。V115 を先に入れる順では処理系から消える語なので、「構文エラー」にそろえる。
- N.4(V123)は「：」(全角のコロン)と「:」が混ざる(最終稿の写し)。
- X-E8 の「差分は 6 行」は削除 6 行、追加 2 行の差分である(最終稿の写し)。

## 5. 順序と要望

- 当てた順(V115、X-E8、V121、V123、X-E13、V88、V60、V118、X-F5b)は、各最終稿の依存(V115 を V60、V88、V118 より前、X-E8 を V118 より前、V121 を V123 より前、V60 を V118 より前)と矛盾しない。V118 と V88 の最終稿は V115 を後に置くとしていたが、節は逆にした理由を書いている。
- V114 の driver.ml §16.8 の 1046 行の hunk が当たらない原因が V121 の表の 2 の行の変更であることは、両方の差分を読んで確かめた。V114 は numeric.t を変え、X-F5b は numeric.t も LangSpec も変えないので、「numeric.t と §4.2 で重なる」が成り立たないという指摘は正しい。
- 退避と復元(D[V88-3])の要否: recent は不変のリストで、退避は参照の保存で足りる。戻さない変種の結果(results.norestore.txt)は、ソースから導いた動き(before_ep が次のトークンの終わりに進み、行をまたぐ判定が外れる)と合う。
- REPL で、if で終わる行の続き、空行をはさむ演算子の続き、予約語、括弧の続き、数値の綴りをまとめて流し、各文面とセッションの継続を確かめた。
