### N.1 V47：module の外の非修飾の `@ W[E]` が、案内の無い「未知の型: W」になる

#### (1) 判定、規模、依存、出典

判定: fix(診断の文面だけが変わる。受理と拒否と終了コードは変わらない)。

規模: S。
コードは `lib/elab.ml` の 3 か所(補助関数 `unknown_type` の新設と呼び出し 2 か所、`elab_eff` の分岐 1 つ)、記事 2 か所(§11.3 と §11.6)、`unknown_type` の頭のコメント(5 行)、`elab_eff` の新しい分岐の中のコメント(7 行)、最後の分岐の中のコメント 1 か所。
cram は `test/visibility.t` に 2 ブロック、コマンド 16 行、プログラム 8 本。
差分 `doc/log/261017-3-spike/V47/V47.patch` は 85 行の増加と 19 行の減少(見出しを除く。見出しの行を除いて `^+` の行を数えた。空行を含む)。
下書きの時点では 83 行で、反証者の補足を受けて新しい分岐の中のコメントに 2 行を足した。

依存: V114(`absent_numeric`。新しい `unknown_type` は V114 の後の `elab_type` の分岐をそのまま移す)。
重なり: V50 が同じ `elab_eff` の `EApply` の分岐の条件(`not (SMap.mem n env.types)`)と §11.6 の記事を触りうる。
V50 が型パラメータと EffectRow エイリアスの優先を変えるなら、本項の新しい分岐の条件も合わせる。
合成では V50 を本項の後に当てた。V50 が変えるのは `EBraceRow` の `T.BLabel` の中だけで、`@ W[E]` の型パラメータの扱い(`not (SMap.mem n env.types)`)は変えないので、本項の条件は合わせなくてよい(N.z、実測)。
V56 と V57 が `test/visibility.t` の visrow 系と §11.6 の記事を触る。
本項のブロック名(vis4c〜vis4e、visrow4b〜visrow4e、visrow5b)は V56 と V57 の名前(visrow16〜visrow21、visrow16b〜visrow16e)と重ならない。
変更する行も別である(本項は `elab_type` と、`elab_eff` の `EBraceRow` より前の分岐)。

出典: diktor の `doc/log/260926-1-plan.md:89`(表)、`:243-245`(2.2 節の延期)、`:2716-2717`(§5 の (6) の 7)、`:4408`(台帳)、`260926-2-m34.md:164`(M36 の検証の指摘 1。visrow4 と visrow5 の前書き)、`:339`(残す)。
台帳が挙げる位置 `lib/elab.ml:529` は当時のもので、HEAD 39d4091 では `:604` である。
本項目は 260926-1 の 2.2 節が延期するもの(`:243-245`。案内を足すのは小さな後続の項目にする、とした)に挙げ、260926-2 が「残す」とした(`:339`)もので、本計画の第 1 回の検証で、「直さないと裁定済み」とした分類を 260926-2 の「残す」に合わせて正し、範囲に加えた。

#### (2) 現状(根拠つき)

仕様: LangSpec §15.1(`doc/LangSpec.md:1447-1481`)は、module の外からは修飾名で参照すること、エフェクト位置で module の宣言が名前を覆うのは EffectRow の別名のときだけであることを書く。
診断の文面は書いていない。

実装(ソースを読んで導いたこと。行は HEAD):
- `@ W[E]` は `elab_eff`(§11.6、`lib/elab.ml:1165`)で、`find_row_alias` の分岐(`:1180`)にも effect の略記の分岐(`:1188`)にも当たらない。module の外では `Decls.resolve_con` が `M.W` に届かないからである。
- 最後の分岐(`:1239`)が `elab_type` に渡し、`EApply` の分岐(`:568-604`)が「未知の型: W」で落とす(`:604`)。この分岐は修飾の候補を探さない。
- 引数の無い `@ W` は `unknown_effect`(`:941`)で「未知のエフェクト: W(M.W と修飾してください)」になり(visrow4)、行の要素の `{W[E]}` も `unknown_effect` を通る。
- 型の位置でも同じ欠けがある。引数の無い `V` は `EIdent` の分岐(`:530-542`)が pub の候補を案内するが、`V[Int32]` は `:604` で案内が無い。

HEAD と合成の木での実測(`doc/log/261017-3-spike/V47/ex/` と `doc/log/261017-3-spike/V47/exb/`。HEAD は `doc/log/261017-3-spike/V47/results.head.txt`、合成の木は `doc/log/261017-3-spike/V47/results.all.txt`。`doc/log/261017-3-spike/V47/results.head.txt` と `doc/log/261017-3-spike/V47/results.all.txt` は一致する):

| 例 | 形 | HEAD の診断 |
|---|---|---|
| a1 | `module M { pub type W[E]: EffectRow = ... }` と `@ W[Console]` | 未知の型: W |
| a2 | a1 の非 pub | 未知の型: W |
| a3 | a1 で `@ {W[Console]}` | 未知のエフェクト: W(M.W と修飾してください) |
| a10 | 引数の無い `pub type W: EffectRow` と `@ W`(visrow4 と同じ) | 未知のエフェクト: W(M.W と修飾してください) |
| a7、a8、a9 | extends の右、EffectRow エイリアスの本体、`Cb[W[Console]]` | 未知の型: W |
| a18 | 2 つの module の pub の `W[E]` | 未知の型: W |
| a5 | `@ Nope[Console]` | 未知の型: Nope |
| a12、a13 | 型の位置の `V[Int32]`(pub newtype)、`T[Int32]`(pub Type エイリアス) | 未知の型: V / 未知の型: T |
| a14 | 型の位置の引数の無い `V`(pub newtype) | 未知の型: V(M.V と修飾してください) |

どれも終了コード 1 である。
観測点 visrow4 と visrow5(`test/visibility.t:198-205`)は引数の無い `@ W` だけを固定しており、前書きは「引数の無い W」に限ってある(M36 の検証で限定した)。
引数つきの形を固定するブロックは無い。

#### (3) 判断と確定

D[V47-1]: `elab_eff` に、頭が型パラメータでなく、`Decls.resolve_con` で引いた先がエイリアス表にも宣言表にも無い適用 `n[...]` の分岐を足し、`unknown_effect n` で落とす。
置き場は effect の略記の分岐の後、`EBraceRow` の分岐の前である。
最後の分岐へ回す形は、宣言のある頭への適用と module 修飾の名前だけになる。

D[V47-2]: `elab_type` の未知の型の文面を補助関数 `unknown_type n oid` にまとめ、`EIdent` と `EApply` の両方の分岐から呼ぶ。
型の位置の引数つきの名前にも、pub の候補の修飾を案内する。

判断事項 P[V47-1]: エフェクト位置の文面を「未知のエフェクト」にするか。
推奨案: する(D[V47-1])。
代案: 台帳の案どおり `elab_type` の `EApply` の分岐に案内だけを足し、「未知の型: W(M.W と修飾してください)」にする。
推奨の根拠: 引数の無い `@ W` と行の要素の `{W[E]}` はすでに「未知のエフェクト」で、エフェクト位置の 3 形が揃う。
代案はエフェクト位置で「型」の語が残り、候補も pub の型全体(newtype や Type エイリアスを含む)になるので、修飾しても行にならない名前を案内しうる。
推奨案の候補は `unknown_effect` の pub の EffectRow エイリアスだけである(a11 の pub newtype `V[Int32]` には案内しない。実測)。

判断事項 P[V47-2]: 型の位置の引数つきの名前(a12、a13)にも案内を広げるか。
推奨案: 広げる(D[V47-2])。
代案: エフェクト位置だけを直す。
推奨の根拠: 原因は同じ `:604` で、補助関数にまとめれば分岐の重複も減る。既存の期待値は 1 つも変わらない(実測)。

#### (4) 仕様側の改訂案

変えない。
LangSpec は未知の名前の診断の文面を書いておらず、受理と拒否も変わらない。

#### (5) diktor 側の変更

試作: `doc/log/261017-3-spike/V47/V47.patch`(合成の木に対する差分。V114 の `absent_numeric` を前提にするので、HEAD の写しにはそのまま当たらない)。
写しで `dune build` が通り、`dune runtest` は緑(`doc/log/261017-3-spike/V47/runtest.log` は空)。
新しい cram を合成の木のコードで走らせると、6 コマンドが落ち(vis4c、vis4e、visrow4b、visrow5b、visrow4c、visrow4d)、対照の vis4d と visrow4e は通る(`doc/log/261017-3-spike/V47/runtest.base-code-new-test.log`)。
例の前後の対照は `doc/log/261017-3-spike/V47/results.all.txt` と `doc/log/261017-3-spike/V47/results.new.txt`、`doc/log/261017-3-spike/V47/results.b.all.txt` と `doc/log/261017-3-spike/V47/results.b.new.txt`。
変わるのは a1、a2、a5、a7、a8、a9、a11、a12、a13、a17、a18、a20、b2、b3 の 14 例の文面だけで、終了コードと位置は同じである。

コード(`lib/elab.ml`):
- `unknown_type n oid` を §11.3 の記事の後、`speculating` と `elab_type` の間に置く(試作の `lib/elab.ml:586` が §11.3 の見出し、`:720` が `speculating`、`:727` が `unknown_type`、`:738` が `elab_type`。行は合成の木に差分を当てた後のもの)。中身は `EIdent` の分岐にあった候補の絞り込み(pub だけ)と `absent_numeric` の案内で、文字列を返す。
- `elab_type` の `EIdent (LongId [n])` と `EApply ((_, EIdent (LongId [n])), args)` の未知の分岐を `type_error (unknown_type n oid)` にする。副作用として `Int8[Int32]` にも数値型の一覧が付く(b2)。
- `elab_eff` に次の分岐を足す。

```ocaml
  | T.EApply ((_, T.EIdent (LongId [ n ])), _)
    when (not (SMap.mem n env.types))
         &&
         let oid = Decls.resolve_con (intern n) in
         not (Hashtbl.mem Decls.aliases oid || Hashtbl.mem Decls.con_kinds oid) ->
      type_error (unknown_effect n)
```

cram(`test/visibility.t`):
- vis4b の直後に 1 ブロック(コマンド 6 行、プログラム 3 本)。前書き: 引数つきの型を修飾せずに書いても、同じく pub の型だけを候補として案内する。newtype でも Type エイリアスでも同じである。vis4c(pub newtype `V[A]`、案内)、vis4d(非 pub、案内なし。対照)、vis4e(pub Type エイリアス `T[A]`、案内)。
- visrow5 の直後に 1 ブロック(コマンド 10 行、プログラム 5 本)。前書き: 引数つきの `W[E]` を module の外から修飾せずに `@` の後ろに書いても未知のエフェクトとして拒否し、引数の無い W と同じく pub なら修飾を案内し、pub でなければ案内しない。extends の右と、行を取る型引数の位置でも同じである。案内どおり修飾すると通る。visrow4b(pub)、visrow5b(非 pub)、visrow4c(extends の右)、visrow4d(`Cb[W[Console]]`)、visrow4e(`@ M.W[Console]` が受理。対照)。
- 期待値が変わる既存のブロック: 無い(実測)。

ソースコメント:
- §11.3 の記事: 名前の優先順位の 3 項目の後に、どれにも無ければ未知の型で、pub の候補を引数の有無によらず案内する、の 3 行。
- §11.6 の記事: 「未知のエフェクト」の言い分けの段落の後に、頭が宣言されていない適用も未知のエフェクトとして報告する段落(6 行)。「ここへ届く形は 2 つある」の 2 つ目を「宣言のある頭への適用」に限る。
- `unknown_type` の頭のコメント(5 行): 修飾しない名前がエイリアスにも宣言表にも無いときの診断であること、引数の有無によらず同じ文面にすること、pub の型だけを案内すること、引数の個数は見ないこと。
- `elab_eff` の新しい分岐の中のコメント(7 行): 最後の分岐へ回すと未知の型として落ちるのでここで未知のエフェクトとして報告すること、引数の無い `@ W` や `{W[E]}` と同じ案内をすること、条件は `elab_type` の `EApply` の分岐が名前を引く順(型パラメータ、エイリアス表、宣言表)を写したもので、`elab_type` の側で未知の名前に検査を足すときはこの条件も合わせること。
- 最後の分岐の中のコメントを同じく限る。
- 上の 2 つも OCaml のコメントの規則(二重引用符なし、番号と経緯なし)の検査対象である。試作の 2 つに二重引用符は無い。
- 写しで `tools/litwrap.py lib/elab.ml`(既存の表の幅の報告だけ)と `tools/litcheck.py --old <合成の木の elab.ml> lib/elab.ml`(marker 0 件、code の不一致はコードを変えた位置)を走らせた。
分岐の中のコメントに 2 行を足した後も、写しで `dune build`、`dune runtest`(緑、`doc/log/261017-3-spike/V47/runtest.log` は空)、litwrap(既存の表の幅の報告だけ)、litcheck(marker 0 件)を走らせ直した。

コミットの分け方(diktor は ja/ ブランチなので日本語):
1. `diktor: 修飾しない型名の適用が未知のときも、pub の候補への修飾を案内する`(`unknown_type` と `elab_type`、§11.3)
2. `diktor: エフェクト位置の未知の頭への適用を、未知のエフェクトとして報告する`(`elab_eff`、§11.6)
3. `test: 引数つきの名前を module の外から修飾せずに書いたときの案内を固定する`(`test/visibility.t`)
4. 親: `diktor: 引数つきの名前の未知の診断に修飾の案内を足す(サブモジュールを進める)`

M[extra] では本項を最初に当てる(V114 は M[numeric]、V56 と V57 は M[kinds] で先に入っている)。

#### (6) リスク

- 受理と拒否の変化: 新しい分岐は `type_error` だけを出し、HEAD で最後の分岐から `elab_type` の「未知の型」に落ちていた形だけを拾う(エイリアス表にも宣言表にも無い頭は、`elab_type` の `EApply` の分岐で例外なく `:604` に落ちる。ソースから)。
- 頭が型パラメータの適用(`F[E]`)は分岐の条件から外れ、HEAD と同じ経路に進む(b10 は HEAD と同じ)。
- 新しい分岐は可視性を検査しない。`Decls.check_con_visible`(`lib/decls.ml:491`)は `con_visibility` に項目がある oid だけを拒否する。エイリアス表にも宣言表にも無い oid に `con_visibility` の項目が入る経路は見つけていない(ソースを読んだ範囲。網羅は確かめていない)。そのような経路があれば、HEAD の可視性の誤りが未知のエフェクトに変わる。
- 行カインドのパラメータへの型引数(`~check_row:false`、a9)でも文面が「未知のエフェクト」になる。投機の最中も型エラーの扱いは変わらない。
- `Int8[Int32]`(b2)に数値型の一覧が付く。V114 の前書き(`test/numeric.t` の int8)は引数の無い形だけを固定しており、矛盾はしない。固定するなら int8 のブロックに 1 コマンドを足す(本試作では足していない)。
- V114 が先に入らないと差分は当たらない(反証者が HEAD への dry-run で hunk 3 の失敗を確かめた)。
- V50 が型パラメータとエイリアスの優先を変えると、新しい分岐の条件 `not (SMap.mem n env.types)` が `elab_type` と食い違いうる。本計画の V50 は `@ W[E]` を型パラメータ優先のまま保つので食い違わない(V50 の §11.6 の記事の最後の文「`@ W[E]` の形も、型パラメータと同名のエイリアスを引かない」と同じ向きである)。V50 の visrow30 は本項の後も「未知のエフェクト: E」のままで、案内は付かない(合成の木で実測)。
- `elab_type` の `EApply` の分岐の未知の名前に検査が増えると(たとえば数値型の案内)、エフェクト位置ではその前に未知のエフェクトで落ちる。分岐の中のコメントにこの対応を書いた。
- P[V47-2] の後、型の位置の `W[Console]`(pub の EffectRow エイリアス)にも「M.W と修飾してください」が付き、修飾するとカインドの誤りになる(反証者の実測 e3、e2)。引数の無い `x: W` で HEAD から同じ案内が出ている(e7)ので退行ではなく、既存の振る舞いの延長である。
- V56、V57 と同じ記事と cram の周辺を触る。V56 と V57 は M[kinds] で先に入り、本項の差分は合成の木に offset 0 で当たった(N.z)。

#### (7) 検証の観点

- `doc/log/261017-3-spike/V47/results.all.txt` と `doc/log/261017-3-spike/V47/results.new.txt`(と `doc/log/261017-3-spike/V47/results.b.*`)の差が上の 14 例の文面だけで、位置と終了コードが同じであること。
- visrow4 と visrow5(引数の無い形)、visrow2(`{M.W, Console}`)、a3 と a15(`{W[E]}`)、b5(`@ M.W[E]` と `@ {M.W[E]}` の受理)、b7 と b9(トップレベルと module の中の `@ W[E]` の受理)が変わらないこと。
- 案内が pub の EffectRow エイリアスに限られること(a2、visrow5b は案内なし、a11 の pub newtype にも案内なし)。
- 案内に従った後の誤り(b3 の引数の無い `W` に引数を渡した形は、修飾すると b4 の個数の誤りになる)が既存の `unknown_effect` の規則(引数の個数を見ない)と同じであること。
- 2 つの前書きが実測と一致すること(vis4d と visrow4e の対照を含む)。
- 記事の追記が `tools/litcheck.py` を通ること。

#### (8) 検証の反映

反証者の報告は `doc/log/261017-3-spike/V47/verify/refute.md`。

- must_fix 1(差分の行数): 反映した。見出しを除く増加は下書きの時点で 83 行で、76 行は空行 7 行を数えていなかった。補足 1 を反映して 2 行が増え、最終稿の試作では 85 行の増加と 19 行の減少である((1))。
- must_fix 2(`unknown_type` の置き場): 反映した。§11.3 の記事の後、`speculating` と `elab_type` の間である((5))。
- must_fix 3(コメントの一覧): 反映した。`unknown_type` の頭のコメントと、`elab_eff` の新しい分岐の中のコメントを (1) と (5) に足した。
- 退行の候補: 無し(反証者の r1〜r12、e1〜e7 で受理と出力が HEAD または合成の木と一致)。
- 補足 1(新しい分岐の条件と `elab_type` の名前の引き方の対応を書く): 反映した。新しい分岐の中のコメントに 1 文を足し、試作を作り直して build、runtest、litwrap、litcheck を走らせ直した((5)、(6))。
- 補足 2(型の位置の `W[Console]` への案内): 反映した。退行ではなく既存の振る舞い(e7)の延長として (6) に書いた。推奨案は変えない。
- 整合の指摘(V50 との重なり): 反映した。(1) の重なりと (6) に書いた。順序は統括者が決める。
- 整合の指摘(V114 への依存): 反映した。(1) の依存に既にあり、(6) に dry-run の結果を足した。
- 整合の指摘(V44、V48、V49、V53 と重ならない、V56、V57 と周辺が重なる): 反映した。(1) の記述と一致する。

