## X-E8 の反証

### 裏取り

`lib/tree.ml` の 295 行は `let data (d, _) = d` で、§5.4 アクセサ(279 行から)の中にある。
§5.2 の小見出し「loc と oid の読み手」は 227 行で、236-237 行が `data` を残す理由の 2 文である。
`Tree.loc_of` は 9 件、`Tree.oid_of` は 17 件で、下書きの件数と一致した(写しを grep で数えた)。
`Tree.data` の参照は lib、bin、test に 0 件で、`\bdata\b` の当たりは 236 行と 295 行のほかは別の意味の `data`(decls.ml の `prelude_owned "data"`、elab.ml 5367 行の局所変数など)だけである。
下書きの「外側の `Tree` を開く箇所は無い」は正しいが、`lib/unify.ml:955` に別名 `module TT = Tree` がある。
`TT.data` と `T.data` の参照は 0 件で、試作のビルドが通ることもこれを裏付ける。
lib に `.mli` は無い(bin に `main.mli` があるだけ)。
出典の行(`260912-1-status.md:503`、`260829-4-plan.md` の 3972、4806、5799 行、`260829-3-literate.md:98`)は写しのとおりである。

### 実測の再現

試作の差分を写しに当てた。当たりは 2 hunk ともずれなく当たった。
`dune build` は通り、出る警告は HEAD の写しでも出る `parser.mly` の Warning 60 だけである。
`dune runtest` の出力は 0 行である。
`litcheck.py`(写しに git を作り HEAD を基点にした)は `lib/tree.ml` だけを不一致(206 トークン目)とし、差は `let data (d, _) = d` の削除だけで、marker は 0 件である。
`litwrap.py` は HEAD の `lib/tree.ml` を変えない。
試作で折り返しが起きたのは、HEAD の 234 行(表示幅 98)の末尾に ` *)` が付いて幅 101 になり、既定の 100 を超えたためである。
下書きはこの因果を書いていないが、記述に誤りは無い。

### 退行の探索

HEAD と試作のビルドで次を走らせ、標準出力、標準エラー、終了コードを比べた(`ex/`)。
すべて同一だった。

- `e1_alias.kel`: 型エイリアス経由の Never 返し操作、ガード付きの節。`caught none`、`-1`、`5`、終了 0。
- `e2_nested.kel`: 入れ子のハンドラと 2 つのエフェクト。`log a`、`40`、終了 0。
- `e3_typeerr.kel`: 型パラメータと注釈の不一致。2:5 の型エラー、終了 1。
- `e4_runtime.kel`: リストの show とゼロ除算。`[1, 2]` と実行時エラー、終了 3。
- `e5_module.kel`: module の中の pub newtype と pub 関数。`7`、終了 0。
- `repl.in`: 対話的な実行。束縛 3 つと 4:9 の型エラー。
- `--prelude` で差し替えたプレリュード、`--dump-ast`、`--strict-exhaustive`、`test/sample/sample.kel` の `--type-check` も同一。

ソースを読んで導いた性質: 消すのは副作用の無い最上位の関数定義で、参照が無いことはビルドが通ることで確かめられるので、振る舞いは変わりえない。
退行は見つからなかった。

### 重なり

下書きの「`lib/tree.ml` を触る項目はほかに無い」は誤りである。
`items/V118/V118.patch` は `lib/tree.ml` の 245 行の後(§5.2 の `ElabData`)に `let loc d = d.loc` を足す。
`items/V118/draft.md` の 100 行と 114 行は、§5.2 の記事(`loc` の読み手にパーサを足す)を改めるとしている。
これは本項が末尾 2 文を消す小見出し「loc と oid の読み手」(227-237 行)と同じ段落である。
2 つの差分を続けて当てると当たり(X-E8 の 2 つ目の hunk が 2 行ずれる)、`dune build` が通り、`dune runtest` の出力は 0 行だった。
記事の文面は V118 が後から書き足すので、どちらを先にするかを決め、後のほうで litwrap と litcheck を走らせ直す必要がある。

### 判断事項

推奨案(消す)に異論は無い。
D23 候補が同じ受け口として並べた `Location.show_span` と `dummy_span` は HEAD の `lib/location.ml` に既に無く、`data` だけが残っている。
同じ基準で `data` を消すのは一貫している。
