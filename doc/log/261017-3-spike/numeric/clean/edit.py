import sys
p = sys.argv[1]
s = open(p, encoding='utf-8').read()
def rep(old, new):
    global s
    n = s.count(old)
    if n != 1:
        sys.exit('count %d: %s' % (n, old[:60]))
    s = s.replace(old, new)

rep("`generalize`(675〜681 行)", "`generalize`(674〜681 行)")

rep("""  `let b: Float64 = 1.5 % 2.0` は HEAD も試作も 2:5(束縛の名前)に「注釈された型を満たしません(Float64 は Rem のインスタンスではありません)」、`let f(): Float64 = 1.5 % 2.0` は両方とも 2:5 に「注釈された返り値型を満たしません(…)」と出る(実測、`doc/log/261017-3-spike/V91/verify/ex/g1_annot.kel`、`g2_annot.kel`)。""",
"""  `let b: Float64 = 1.5 % 2.0` は HEAD も試作も 2:5(束縛の名前)に「注釈された型を満たしません(Float64 は Rem のインスタンスではありません)」と出る(実測、`doc/log/261017-3-spike/V91/verify/ex/g2_annot.kel`)。
  `let f(): Float64 = 1.5 % 2.0` は両方とも 2:5 に「注釈された返り値型を満たしません(…)」と出る(実測、`doc/log/261017-3-spike/V91/verify/ex/g1_annot.kel`)。""")

rep("""- V109 は `test/dictionary_passing.t` の negremf のすぐ後ろ(419 行から)にブロックを足し、その差分の文脈に negremf の `2:1` の行を含む。
  本項目の後に V109 を当てると文脈が合わないので、V109 の差分の文脈の 1 行を `2:13` に直す(N.z の要望)。""",
"""- V109 は `test/dictionary_passing.t` の negremf のすぐ後ろ(419 行から)にブロックを足し、その差分の文脈に negremf の `2:1` の行を含む。
  本項目の後に V109 を当てても、そのハンクは 419 行に fuzz 2 で当たる(合成の木に `V109.patch` を `patch -p1 --dry-run` で当てた実測)。
  fuzz に頼らないよう、V109 の差分の文脈の 1 行を `2:13` に直しておく(N.z の要望)。""")

rep("LangSpec §11.1 の表の直後(873 行の後の空行の次)に、次の段落を足す。",
    "LangSpec §11.1 の表の直後(873 行の空行の後、874 行の前)に、次の段落を足す。")

rep("- 付録 A.2 の 1927 行は V114 だけ、§11.1 の 873 行の後ろは V8 だけが触る。",
    "- 付録 A.2 の 1927 行は V114 だけ、§11.1 の 873 行の空行の後ろは V8 だけが触る。")

rep("""  `lib/lexer.ml` は V8(74 行の段落)と V114(91 行と 129 行)で別の箇所、""",
    """  `lib/lexer.ml` は V8(76 行の後に 1 行を足す)と V114(94 行と 132 行を書き換える。132 行はコメント 131〜132 行の 2 行目)で別の箇所、""")

rep("""- 合成の差分は `doc/log/261017-3-spike/numeric/numeric.patch` で、HEAD の写しに `patch -p1 --dry-run` で当たることを確かめた。
  V91 の 2 ブロックの昇格を含む。""",
"""- 合成の差分は `doc/log/261017-3-spike/numeric/numeric.patch` で、HEAD の写しに `patch -p1 --dry-run` で当たることを確かめた。
  V91 の 2 ブロックの昇格を含む。
  この差分と `V91.patch`、`V114.patch`、`V8.patch` の見出しは `a/lib/elab.ml` と `b/lib/elab.ml` の形にそろえてあり、どれも HEAD に `-p1` で当たる(実測)。
  見出しを直した後の `numeric.patch` を HEAD の写しに当てて `dune build` と `dune runtest` を走らせ直し、差分なしで、昇格後の合成の木と同じ内容になることを確かめた。""")

rep("""  V127 の差分は `lib/unify.ml` の 497〜577 行を触り、V91 の差分(289 行、668〜747 行、954 行、1062 行)とは重ならない。""",
    """  V127 の差分は `lib/unify.ml` の 497〜577 行を触り、V91 の差分(31 行、289 行、668〜747 行、954 行、1062 行)とは重ならない。""")

rep("""- V109(分野 eval): `test/dictionary_passing.t` の negremf の直後にブロックを足し、差分の文脈に negremf の `2:1` の行を含む。
  本節の後に当てるなら、その文脈の行を `2:13` に直す。""",
"""- V109(分野 eval): `test/dictionary_passing.t` の negremf の直後にブロックを足し、差分の文脈に negremf の `2:1` の行を含む。
  本節の後に当てても、そのハンクは 419 行に fuzz 2 で当たる(合成の木に `V109.patch` を `patch -p1 --dry-run` で当てた実測)。
  fuzz に頼らないよう、その文脈の行を `2:13` に直しておく。""")

s = s.rstrip('\n') + """

検証の反映(分野の節の反証者の報告に対する扱い):

- must_fix 1(N.z の `lib/lexer.ml` の行番号がハンクの見出しの開始行で、小節の変更行と食い違う): 反映した。
  N.z を変更行にそろえ、V8 は 76 行の後に 1 行、V114 は 94 行と 132 行(コメントは 131〜132 行)と書いた。
  HEAD の写しと合成の木の `diff` で 76a77、94c95、132c133 を確かめた。
- must_fix 2(V109 は合成の木に fuzz 2 で当たる): 反映した。
  V91 の (6) と N.z の要望を「fuzz 2 で当たるが、文脈の 1 行を `2:13` に直しておく」に改めた。
  合成の木に `V109.patch` を `patch -p1 --dry-run` で当て、dictionary_passing.t のハンクが 419 行に fuzz 2 で当たることを確かめた。
- must_fix 3(写す差分の見出しに一時領域の絶対パスが残る): 反映した。
  `numeric.patch` の 28 行を `a/` と `b/` の形に直して上書きし、本体の行が変わらないこと、HEAD の写しに `-p1` で当たること、`dune build` と `dune runtest` が差分なしで通ることを確かめた。
  `V91.patch`(8 行)、`V8.patch`(8 行)、相対パスの見出しの `V114.patch`、X-premise との試し合わせの `default_to-via.patch` も同じ形に直したものを作り、統括者に写す元として渡す。
- 軽微(V91 の (2) の `generalize` の行): 反映した。
  674〜681 行に直した。
- 軽微(V8 の (4) の 873 行): 反映した。
  「873 行の空行の後、874 行の前」に直し、N.z の LangSpec の調整の行も合わせた。
- 軽微(V91 の (6) の注釈の例の対応): 反映した。
  文を 2 つに分け、`let b` に `g2_annot.kel`、`let f` に `g1_annot.kel` を添えた(HEAD で実測し直した)。
- 軽微(N.z の V91 の `lib/unify.ml` のハンクの列挙に 31 行が抜けている): 反映した。
  31 行を足した。
  V127 の 497〜577 行と重ならない結論は変わらない。
- 文章規範(一時領域のパス、英語の文、仮の番号、禁止語、である調の崩れは無い): 変更は要らない。
  書き足した文も同じ規範で書いた。
"""
open(p, 'w', encoding='utf-8').write(s)
