# V83 の反証

## 裏取り

HEAD(diktor 39d4091)の `lib/prelude.kel` を行頭で数え直した。
`^effect ` 2(188-189 行)、`^extern ` 48(248-297 行)、`^type instance` 4(434、447、464、477 行)、`^newtype ` 2、`^type [A-Z]` 1、`^let ` 4 で、下書きの表と一致する(実測)。
11-15 行の「4 つの `effect`」「52 本の `extern` 署名」「`Show` インスタンス 2 つ」も HEAD の文面どおりである。

ずれの由来を `git show <rev>:lib/prelude.kel` で数えて確かめた(実測)。
`adddab4^` で effect 4 / extern 52、`adddab4` で effect 3、`7bea47d` で effect 2 / extern 48、`7532998` で instance 2 から 4 になり、どの版でも冒頭の 2 行は「4 つ」「52 本」のままである。
下書きの由来の記述は正しい。

節の対応(§15.3 Unit / Option / List、§15.4 2 つのエフェクト、§15.5 extern、§15.6 出力の 3 つの入口、§15.7 with_stdout、§15.8 Show、§15.9 Eq)は見出し(95、127、191、299、326、391、454 行)と一致する。

`lib/decls.ml` 1140-1143 行で予約述語 `Integral` / `Fractional` を `def_class` で登録すること、出典の `doc/log/261010-1-plan.md:689-691`、`:5873`、`doc/log/261010-2-m53.md:677` も確かめた。

`litwrap.py --check` が HEAD で終了状態 1 になるのは diktor の lib と bin のうち `lib/prelude.kel` だけである(実測。全ファイルに走らせた)。
幅を超える 3 行は HEAD の 82 行(幅 109)、90 行(幅 103)、231 行(幅 102)である。

## 試作の再現

`V83.patch` を写しに当て、`dune build` は通り、`dune runtest` の出力は 0 バイトだった。
`litcheck.py --old <HEAD の prelude.kel>` は「code: 一致(641 トークン) / marker: 0 件」、終了状態 0。
試作の後の `litwrap.py --check` は終了状態 0 で、32 行(表の行)を「表・見出し・コード(幅 126)」と報告する。
weave.awk の出力で冒頭の箇条書きと表は崩れない。

担当の例 `eqlist.kel`(true / false、終了状態 0)と `fs.kel`(未知のエフェクト: Fs、終了状態 1)は HEAD で同じ出力を再現した。

## 退行の探索

コードのトークンは変わらないが、埋め込みのプレリュードの行番号は 4 行ずれるので、プレリュードの位置が診断に出る形を中心に試した。
`ex/` の 10 本(標準環境の型の再宣言、`Show[List[_]]` のコヒーレンス違反、関数の `Eq`、`println` の行の不足、`Show` のメソッドの panic、module の中の入れ子のハンドラと `with_stdout`、prelude の extern の再宣言、入れ子の `List` / `Option` の `==`、import した pub 関数、REPL)と `--type-check`、`--strict-exhaustive`、`--dump-ast`、`--prelude <HEAD と試作の prelude.kel>`、`--no-prelude` で、HEAD と試作の出力と終了状態はすべて一致した(実測)。
`<prelude>` の位置を出す診断は cram にも無い(grep で 0 件)。
退行は見つからなかった。

## 指摘

1. 38 行の改訂案(予約述語を足す)は、下書き自身の確定「組み込み表の一覧は現状に合わせる」を満たさない。
`register_builtins` は基本型 `Boolean` / `Int32` / `Int64` / `Float64` / `String` / `Never`(`lib/decls.ml` 1100-1102 行、`Never` のデータ宣言は 1146 行)と `pinned`(1026-1035 行、本章 227 行も組み込みと書く)も登録するが、38-39 行の一覧にも 33 行の表にも無い。
網羅する形にするか、「主なもの」と範囲を断る形にするかを決める必要がある。
2. 下書き (5) の「表の行は `litcheck` の幅の検査から外れる(幅 126 は報告だけで、終了状態は 0)」の報告は `litcheck.py` ではなく `litwrap.py --check` の出力である。
`litcheck.py` は表の行を幅の検査から除く(tools/litcheck.py 16 行)。
また HEAD の 32 行は幅 118 で、試作で 126 になり、`--hard` の既定 120 を超える。
「`Show` / `Eq`」のように縮めれば 120 以内に収まる。
3. 33 行の表の「演算子クラス」は、P[V83-2] で 38 行に予約述語を足すなら同じ表の行と食い違う。
32 行を直すなら 33 行も同じ扱いにする。

## 重なり

V37 の下書きが `lib/prelude.kel` §15.8(413-423 行の記事と 434-445 行のコード)を変える。
V83 の試作は 231 行より前で 4 行増やすので、V37 の行番号はどちらが先かで 4 ずれる。
差分の塊は重ならない。
V97 が組み込みの署名を `lib/decls.ml` §6.12 で動かしても、推奨案(数を書かない)なら冒頭は直さずに済む。
X-D7 と V135 は `lib/prelude.kel` を読むだけで変えない。
