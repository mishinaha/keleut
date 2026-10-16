import sys
src, dst = sys.argv[1], sys.argv[2]
t = open(src, encoding='utf-8').read()
def rep(old, new):
    global t
    n = t.count(old)
    if n != 1:
        sys.exit('count %d for: %s' % (n, old[:60]))
    t = t.replace(old, new)

# A: (2) の深さの表に nontail_N.kel を足す
rep("""| `doc/log/261017-3-spike/V138/ex/retryresumepath.tmpl`(同じく retryresumepath) | 6431 |

""", """| `doc/log/261017-3-spike/V138/ex/retryresumepath.tmpl`(同じく retryresumepath) | 6431 |
| `doc/log/261017-3-spike/V137/ex/nontail_N.kel`(`ask` の節で `resume(1)` の戻り値に 1 を足す形。`doc/log/261017-3-spike/V137/results.head.txt` 120〜125 行) | 5403 |

3 行目は V137 の材料で、1 行目とはプログラムが違うので数が 1 段ずれる。
この節で「5403 段」と書くのは `nontail_N.kel` の数、「5404」と書くのは `nontail_resume.tmpl` の数である。

""")
# B
rep("この変更の後の深さは、nontail_resume が 5404 から 7946、retryresumepath が 6431 から 7944 である(実測、`doc/log/261017-3-spike/V138/results.new.txt`。反証者も再現)。",
    "この変更の後の深さは、`nontail_resume.tmpl` が 5404 から 7946、`retryresumepath.tmpl` が 6431 から 7944 である(実測、`doc/log/261017-3-spike/V138/results.new.txt`。反証者も再現)。\n`nontail_N.kel` は 5403 から 7946 である(実測、`doc/log/261017-3-spike/V137/results.head.txt`)。")
# C
rep("で `dune build` と `dune runtest` が 0 差分で通り、末尾でない `resume` の深さは HEAD と同じ 5403 段のままである。",
    "で `dune build` と `dune runtest` が 0 差分で通り、末尾でない `resume` の深さは `nontail_N.kel` で HEAD と同じ 5403 段のままである(V137 の反証者が `l=100k` の二分探索で実測した)。")
# D
rep("- 末尾でない `resume` の入れ子の深さは `l=100k` で 5403 段から 7946 段になった(`doc/log/261017-3-spike/V137/results.head.txt`、反証者も再現)。261017-2 の 7110 段(66a6de2 の前)より深い。",
    "- 末尾でない `resume` の入れ子の深さは、`l=100k` で、`doc/log/261017-3-spike/V137/ex/nontail_N.kel` が 5403 段から 7946 段に、`doc/log/261017-3-spike/V138/ex/nontail_resume.tmpl` が 5404 段から 7946 段になった(`doc/log/261017-3-spike/V137/results.head.txt`、`doc/log/261017-3-spike/V138/results.new.txt`、反証者も再現)。`nontail_resume.tmpl` の 261017-2 の 7110 段(66a6de2 の前)より深い。")
# E
rep("- V128 との重なり: N.z のほかの分野への要望に書いた。\n",
    "- V128 との重なり: N.z のほかの分野への要望に書いた。\n  V138 の最終稿の台帳の候補 V[V138-2](V128 の文面を V137 の後の形に合わせる)は、番号を置かず、N.z の V128 への要望に置き換えた。\n")
# H: 親のコミットの順
rep("3. 親: `spec: §5.7 から打ち切られたハンドラの除外を消す`(サブモジュールを進めるコミットは分野の最後にまとめる。N.z)",
    "3. 親: `spec: §5.7 から打ち切られたハンドラの除外を消す`(親では、分野の最後のサブモジュールを進めるコミットの後に置く。N.z)")
# G
rep("`doc/log/261017-3-spike/V109/V109.patch` の追加と削除は合わせて 47 行)",
    "`doc/log/261017-3-spike/V109/V109.patch` の追加と削除は、空行を除いて 47 行、空行を含めて 52 行)")
# V109 (5) numeric
rep("V106(分野 classes)も `test/dictionary_passing.t` を触るが、合成後の木に V106 の差分は fuzz なしで当たる(N.z)。",
    "V106(分野 classes)も `test/dictionary_passing.t` を触るが、合成後の木に V106 の差分は fuzz なしで当たる(N.z)。\n分野 numeric の V91 はこのファイルの末尾の既存のブロック negremf の期待値を変えるので、どちらの順でも末尾の塊が fuzz 2 で当たる(N.z。実測)。")
# F: V108 (1) と (6)
rep("  本項目の新しいブロックはパースエラーを含まないので、V115 の文言の変更の影響を受けない。\n",
    "  本項目の新しいブロックはパースエラーを含まないので、V115 の文言の変更の影響を受けない。\n- V97、V103(分野 classes): 本項目の 1 つ目の新しいブロックの `t` の行の型が変わる(N.z の実測)。\n")
rep("- newtype の `Show` の経路で、引数を見ない `show` の穴を含む値の表示が `U!` から `<unevaluated>` に変わる(P[V108-3])。受理は変わらない。\n",
    "- newtype の `Show` の経路で、引数を見ない `show` の穴を含む値の表示が `U!` から `<unevaluated>` に変わる(P[V108-3])。受理は変わらない。\n- 1 つ目の新しいブロックの `t : (Int32) => String @ {Console, Blocking, Print} = <fn>` の行は、型の行を表示する。分野 classes の V97 または V103 が入ると、この行は `@ {}` に変わる(V97 と V103 の試作の処理系、分野 classes の合成の処理系で `let t = idf(show); t(2i32)` を走らせて実測)。値の表示は変わらない。後に入るほうのコミットで期待値を直す(N.z)。\n")
# I: N.z の見出しの説明
rep("HEAD の diktor(39d4091)の写しに、次の順で差分を `patch -p1` で当てた(各差分の見出しは `diktor/lib/...` の形)。",
    "HEAD の diktor(39d4091)の写しに、次の順で差分を `patch -p1` で当てた。\n各項目の差分の見出しは、`+++` 行が `diktor/lib/...` の形、`---` 行が試作の一時領域の絶対パスで、`patch -p1` は `+++` 行の `lib/...` を使う。")
rep("合成差分は `doc/log/261017-3-spike/eval/eval.patch`(810 行、8 ファイル)である。",
    "合成差分は `doc/log/261017-3-spike/eval/eval.patch`(810 行、8 ファイル)である。\n合成差分の見出しは `a/lib/...` と `b/lib/...` の相対の形に作り直した(本体は作り直す前と同一)。\ndiktor のディレクトリの中で `patch -p1` で当たり、当てた木は合成の写しと `diff -r` で一致し、`dune runtest` は 0 バイトだった(実測)。")
# H in N.z
rep("""親は 2 本とする。
`spec: §5.7 から打ち切られたハンドラの除外を消す`(V137)と、`diktor: 評価器と対話的な実行の延期項目を直す(サブモジュールを … に進める)` である。""",
"""親は 2 本とし、次の順に置く。

1. `diktor: 評価器と対話的な実行の延期項目を直す(サブモジュールを … に進める)`
2. `spec: §5.7 から打ち切られたハンドラの除外を消す`(V137)

spec を先に置くと、その間の親は仕様が末尾位置を保証し、記録された diktor の版がそれを守らない状態になるので、サブモジュールを進めるコミットの後に置く。""")
# K
rep("- 分野 classes と diagtext(V97、V103、V33、V106): `test/classes.t` と `test/dictionary_passing.t` の差分は合成後の木に fuzz なしで当たる。V37 の pshowsplit が `test/classes.t` の pshow と pshowop の間に 37 行足すので、先にこの分野を当てる順を要望する。",
    "- 分野 classes と diagtext(V97、V103、V33、V106): 当てる順は問わない。分野の合成差分どうしで、どちらの順でも fuzz なしで当たり、同じ木になる(下の「ほかの分野の合成差分との組み合わせ」)。ただし分野 classes の V97 と V103 は V108 の新しいブロックの `t` の行の型を `@ {}` に変えるので、後に入るほうのコミットで `test/repl.t` の期待値を直す。")
rep("- 台帳の扱い: V138 と B11 は、この分野のコミットで解消したと実施記録に書く。\n",
"""- 分野 numeric(V91): `test/dictionary_passing.t` の末尾の塊がどちらの順でも fuzz 2 で当たる。期待値は変わらない。
- 台帳の扱い: V138 と B11 は、この分野のコミットで解消したと実施記録に書く。
- 材料を写すとき: 各項目の差分の `---` 行には試作の一時領域の絶対パスが残っている。親の `doc/log/261017-3-spike/` に写すときに相対の形に直す(この分野の合成差分は直した)。

#### ほかの分野の合成差分との組み合わせ(実測)

HEAD の写しにこの分野の合成差分を当て、その上にほかの分野の合成差分を 1 つずつ当てて、`dune build` と `dune runtest` を走らせた。

| 分野 | 当たり方 | 組み合わせた木の `dune runtest` |
|---|---|---|
| syntax | fuzz なし | 0 バイト |
| numeric | `test/dictionary_passing.t` の 1 塊が fuzz 2 | 0 バイト |
| region | fuzz なし | 0 バイト |
| effects | fuzz なし | 5 ファイル 7 ブロックの差分で、分野 effects だけを当てた木の差分と同一(期待値の変化は分野 effects のもので、この分野は増やさない) |
| never | fuzz なし | 0 バイト |
| kinds | fuzz なし | 0 バイト |
| spec | fuzz なし | 0 バイト |
| diagtext | fuzz なし | 0 バイト |
| classes | fuzz なし | `test/repl.t` の 1 ブロック(V108 の 1 つ目)の 1 行が `@ {Console, Blocking, Print}` から `@ {}` に変わる |

逆の順(classes、diagtext、numeric を先に当て、この分野を後に当てる)も試した。
classes と diagtext は `test/classes.t` の V37 の塊が offset 45 行と 39 行だけで当たり、どちらの順でも同じ木になった。
numeric は `test/dictionary_passing.t` の V109 の塊が fuzz 2 で当たった。

#### 検証の反映

- 反証者の must_fix 1(末尾でない `resume` の深さが 5404 と 5403 の 2 通りに見える): 反映した。(2) の表に `nontail_N.kel` の行を足し、D[V137-2]、P[V137-3]、(6) にプログラム名を書いた。
- must_fix 2(classes と diagtext への順の要望が読めず、要らない): 反映した。「当てる順は問わない」に書き換え、分野の合成差分で両方の順を実測した。あわせて、反証者が見ていなかった期待値の衝突(V97、V103 による `test/repl.t` の `t` の行)を見つけたので、V108 の (1)(6) と要望に書いた。
- must_fix 3(合成差分の `---` 行の一時領域のパス): 反映した。合成差分の見出しを `a/`、`b/` の相対の形に作り直し、本文の見出しの説明も直した。各項目の差分の `---` 行は写すときに直すよう要望に書いた。
- 直すとよいもの 1(親のコミットの順): 反映した。サブモジュールを進めるコミットの後に spec を置く順と理由を書いた。
- 直すとよいもの 2(V[V138-2] の置き換え): 反映した。V137 の (8) に 1 文足した。
- 直すとよいもの 3(V109 の 47 行の数え方): 反映した。空行を除く数と含む数を併記した。
- 直すとよいもの 4 と文章規範(第13章の引用): 引用のまま残す。
- 文章規範(合成差分の見出しのパス): must_fix 3 と同じく反映した。
""")
open(dst, 'w', encoding='utf-8').write(t)
