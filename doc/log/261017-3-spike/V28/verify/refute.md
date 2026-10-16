# V28 の下書きの反証

## 結論

下書きの事実の主張は、出典の置き場所を除いてすべて裏が取れた。
V28 が diktor 0ab39c6 で解消したという判定は、実測で再現した。
試作の差分は無いので、退行は構成上ありえない。
直すべき点は 1 件で、出典の 3 つのファイルが親ではなく diktor の `doc/log/` にあることである。

## 1. 行番号、名前、件数の裏取り

- `doc/LangSpec.md` §15.1 の 1478〜1481 行は、module の EffectRow エイリアスがエフェクト行の位置で名前を覆う規則で、下書きの要約どおりである(親の写しで確認)。
- `lib/elab.ml` の `elab_eff` は 1165 行、`find_row_alias` は 895 行、`find_alias` は 912 行にある(写しで確認)。
- `elab_eff` は §11.6(1021 行〜)にある。
  `find_row_alias` は §11.5(759〜1020 行)にあり、下書きはこれに § 番号を付けていないので誤りではない。
  ただし `lib/elab.ml:4695` のコメントは「§11.6 の find_row_alias」と書いており、こちらは既存のコメントの誤りである(V28 の範囲外)。
- `test/visibility.t` の visrow1 のコマンドは 152〜175 行、visrow7 は 231〜254 行で、下書きどおりである(前書きはそれぞれ 147 行と 224 行から)。
- 出典の `260919-3-sync.md:108`、`260919-2-m29.md:115`、`260926-1-plan.md:84` は行の内容が下書きどおりだが、3 つとも diktor の `doc/log/` にあり、親の `doc/log/` には無い。
  親の写しの `doc/log/` を列挙して確かめた。
- 26c9057 と 0ab39c6 が M36 の 1 段目と 2 段目であることは、diktor の `doc/log/260926-2-m34.md:134-135` の表で確かめた。
  `git log 26c9057^..0ab39c6` は 2 件で、0ab39c6 は 26c9057 の直後である。
- 29f23be で visrow1 の `M.at`、`M.rhs`、`M.say` の表示が `{Print extends R1}` から `{Print}` に変わったことは、`git show 29f23be -- test/visibility.t` と `git log -S` で確かめた。
- V28 の台帳の行は `260919-3-sync.md:108` で「残る」のままで、M36 の記録(`260926-2-m34.md:335`)は V42 だけを解消としている。
  V28 が台帳の上で閉じられていないという下書きの前提は正しい。

## 2. 実測の再現

HEAD(`diktor-head`)で `a_at_W`、`b_at_WE`、`c_brace_WE`、`d_run` を走らせ、`results.head.txt` と同じ出力を得た(`d_run` は `a`、`b`、`c`、終了コード 0)。

26c9057 と 0ab39c6 を `git archive` で取り出して自分でビルドし(`verify/old/`)、`results.old.txt` と同じ出力を得た。
26c9057 では `a_at_W` が `3:28: 型エラー: 未知のエフェクト: W`、`c_brace_WE` が `3:31: 型エラー: 未知のエフェクト: WE` で落ち、`b_at_WE` だけ通る。
0ab39c6 では 3 つとも通る。
加えて、`d_run` は 26c9057 で `4:32: 型エラー: 未知のエフェクト: W`、0ab39c6 で受理だった。

HEAD の写し(`verify/diktor`)の `dune runtest` は緑だった(出力 0 行、`verify/runtest.out`)。

## 3. 試作の差分

無い。
担当の分類は「コードの変更なし」で、当てる差分が無い。

## 4. 退行の探索

差分が無いので、HEAD と試作の出力は一致する。
代わりに、V28 の解消が台帳の 3 形より広い形で成り立つかを、境界の形で HEAD と 26c9057 に走らせた(`verify/ex/`)。

| プログラム | 形 | HEAD | 26c9057 |
|---|---|---|---|
| `r1_import.kel` + `r1_import_lib.kel` | import した pub module の `@ W`、`@ {WE[E]}`、`{Console extends WE[E]}` と、import 側の `@ {M.WE[{}]}` | 受理、`a c d g` を出力 | パースエラー(import の構文が当時と違う) |
| `r2_nested_lambda.kel` | let の注釈の入れ子の矢印に `@ W` と `@ {WE[{}]}` | 受理、`x` を 2 回出力 | `未知のエフェクト: W` |
| `r3_handler.kel` | 利用者の effect Ask を含む `{WA[E]}` と `@ W` と入れ子のハンドラ | 受理、`211` | `未知のエフェクト: WA` |
| `r5_letrec.kel` | let rec の群の両方に `@ {WE[{}]}` | 受理、`o` | パースエラー(当時の構文) |
| `r6_shadow_type_param.kel` | `{Console extends WE[E]}` と `{WE[{Console extends E}]}` | 受理 | 前者は受理、後者は `未知のエフェクト: WE` |
| `r7_nested_alias.kel` | 別名の右辺の `{W extends E}` と、別名を返す別名 `WW[E]` | 受理 | `未知のエフェクト: W` |
| `r8_repl.in` | REPL で同じ module を入力 | 受理、`a c` | `--repl` が無い |
| `r9_guard_where.kel` | 引数の関数型とガードつき match の中の再帰で `{WE[E]}` | 受理、`1 3` | `未知のエフェクト: WE` |

HEAD は 8 本とも受理し、期待どおりに実行した。

境界で 1 つ落ちる形を見つけたが、V28 ではなく V29 の形である。
`type WE[E]: EffectRow = {Print extends E}` の後に `newtype Cb = Cb(() => Unit @ {WE[{}]})` を書くと、HEAD は `1:40: 型エラー: 行カインドではない型パラメータです: E` で落ちる(`r4c_top_WE.kel`、トップレベル)。
module の中でも同じで(`r4_newtype_field.kel`、`r4d_mod_WE_at.kel`)、26c9057 と 0ab39c6 でも同じだった。
同じ別名を let の注釈に書く形(`r4h_let.kel`)と、パラメータを渡す `newtype Cb[E] = Cb(() => Unit @ {WE[E]})`(`r4g_param.kel`)は通る。
module の中の非修飾の `@ W` を newtype のフィールドに書く形(`r4b_newtype_W.kel`)は HEAD で通る。
したがって module と非修飾名の解決(V28)は関係せず、newtype のフィールドで EffectRow エイリアスに具体的な行を渡す形の問題(V29)である。

## 5. 仕様の文面

下書きは仕様を変えないので、§18、§15.5、付録 A との矛盾は生じない。
§18.4 は、型の宣言群の中で後ろで宣言するエイリアスに `{}` を渡す位置を除外として挙げている。
上の `r4c_top_WE.kel` はエイリアスを前で宣言していても落ちるので、§18.4 の除外の書き方は V29 の担当が確かめるべき点である(V28 の範囲外)。

## 6. 判断事項 P[V28-1]

推奨案(足さない)に異論は無い。
`elab_eff` の `EBraceRow` の分岐は、名前が組み込みと衝突するかに関係なく、最初に `find_alias` を呼び、`find_alias` は最初に `find_row_alias` を呼ぶ(`lib/elab.ml:915`、`:1218`)。
したがって visrow7 の `{Heap[E]}` と `{WE[E]}` は同じ分岐を通る、という担当の導出はソースの上で正しい。
ただし、衝突する場合の条件が厳しいという言い方は、分岐が同じである以上、どちらの条件も同じであり、強弱の比較は要らない。
代案の費用は 1 ブロックで小さいので、台帳の文面の 3 形をそのまま観測点に残したい場合は代案でもよい。

## 7. ほかの項目との重なり

- V56 と V57 は、同じ `find_alias`(`lib/elab.ml:912`)と `find_row_alias`(`:895`)と、`test/visibility.t` の visrow 系のブロックを触る可能性がある。
  V28 は変更を持たないので衝突はしないが、V56/V57 の修正後に visrow1 と visrow7 が緑であることを V28 の観測点として確かめる必要がある。
- V29: 上の 4 節のとおり、newtype のフィールドで `{WE[{}]}` を書く形は、V28 の解消後も V29 の原因で落ちる。
- X-effsuffix: 下書きの実測の表示(`@ {Print}` と `@ {Print extends R1}`)が変わりうる。
  統括者が V28 の決着に実測を写すときは、版の名前を付けること。
- `lib/elab.ml:4695` のコメントの「§11.6 の find_row_alias」は §11.5 が正しい。
  find_alias の周辺を触る項目(V56)で一緒に直せる。
