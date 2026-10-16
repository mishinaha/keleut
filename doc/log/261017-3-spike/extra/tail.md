
### N.z 合成の確認

#### 基点と当て方

基点は合成の木(10 分野の合成差分を当て、期待値を promote した木)である。
6 件の差分は、見出しの形が項目ごとに違う(合成の木の名前、`base/diktor`、`diktor/` など)ので、見出しを同じ形に揃えてから、合成の木の写しに `patch -p1` で順に当てた。
差分の中身は各項目の `doc/log/261017-3-spike/<id>/<id>.patch` と同じで、各段の前に `--dry-run` で確かめた。
写しの `_build` は消してからビルドし直した。

#### 順序と当たり方

| 順 | 項目 | 差分 | 当たり方 | 手直し |
|---|---|---|---|---|
| 1 | V47 | `doc/log/261017-3-spike/V47/V47.patch` | offset 0、fuzz 0 | 無し |
| 2 | V50 | `doc/log/261017-3-spike/V50/V50.patch` | `lib/elab.ml` の 2 hunk が +6 行と +25 行、`test/visibility.t` が +40 行の offset(V47 の追加分)。fuzz 0 | 無し |
| 3 | V44 | `doc/log/261017-3-spike/V44/V44.patch` | `lib/elab.ml` の hunk #4〜#8 が +26 行の offset(V47 の `unknown_type` と記事の分)。`test/pub_effect_note.t` は新設。fuzz 0 | 無し |
| 4 | V49 | `doc/log/261017-3-spike/V49/V49.patch` | `lib/elab.ml` が +68 行の offset、`lib/unify.ml` は offset 0、`test/scope_escape_tparam.t` は新設。fuzz 0 | 無し |
| 5 | V53 | `doc/log/261017-3-spike/V53/V53.patch` | `lib/elab.ml` が +68 行の offset、`test/eval.t` と `test/verify_fixes.t` は offset 0。fuzz 0 | 無し |
| 6 | V48 | `doc/log/261017-3-spike/V48/V48.patch`(`V48-head.patch` は使わない) | 5 ファイルとも offset 0、fuzz 0。`test/uninhabited_pattern.t` は新設 | 無し |

失敗した hunk は無く、手直しは 0 件である。
順序の根拠は次のとおりである。
- V47 を V50 より先に当てる。V50 の visrow30 の「未知のエフェクト: E」は V47 が案内を足す `unknown_effect` を通るので、V47 の後で期待値を確かめる必要があった。
- V44 を V49 より先に当てる。V49 の注記が V44 の誤った言い換えの括弧と重ならない(V49 の (6))。
- V48 は V129 と V136 の後が前提で、本分野では最後に当てる。

#### 項目どうしの重なりの確認

- V47 と V50: 同じ `elab_eff` を触る。
  V47 が足す `@ W[E]` の分岐の条件 `not (SMap.mem n env.types)` は、V50 の後も `@ W[E]` で型パラメータを先に引く向きのままで、V50 の §11.6 の記事の最後の文「`@ W[E]` の形も、型パラメータと同名のエイリアスを引かない」と合う。
  V50 は `EBraceRow` の `T.BLabel` の中だけを変えるので、V47 の条件は合わせなくてよい(ソースから導いた性質。合成の木の runtest で確かめた)。
  visrow30 は合成の木で「visrow30.kel:2:32: 型エラー: 未知のエフェクト: E」のままである(実測)。トップレベルのエイリアスには pub の修飾の候補が無いので、V47 の案内は付かない。
- V44 と V49: V49 は脱出の文面の後ろに注記を足すだけで、V44 の `escape_failure` が見る部分文字列「がスコープの外に漏れています」を保つ。
  V44 の例 a4 は、合成の木で V49 の注記が付く(下の例の出力)。
  V44 の cram は a4 を固定していないので、期待値は変わらない。
- V49 と ml5c: `doc/log/261017-3-spike/V49/V49.patch` は `test/typecheck.t` の ml5c の期待値を含まない。
  合成の木の runtest で予告どおりこの 1 行だけが変わったので promote し、`extra.patch` に含めた。
  実施のときは V49 のコミット 2 で promote する。
- V53 と V48: V48 は操作節の総和性の検査に触れず、V53 の期待値も変わらない(合成の木の runtest で確かめた)。
- V44、V47、V49、V50、V53 は `lib/elab.ml` の別々の関数を触り、合成の木で hunk の衝突は無かった。

#### 合成後の dune build と dune runtest

`dune build` は通る(警告は合成の木と同じ `parser.mly:88` の Warning 60 だけ)。
`dune runtest` の 1 回目は、`test/typecheck.t` の ml5c の 1 ブロック 1 行だけが変わった(V49 の予告どおり)。
promote した後の `dune runtest` は差分 0 である。
新しく落ちるブロックは無い。

既存のブロックで期待値が変わるのは 6 ファイル 8 ブロックである。
ml5c を除く 7 ブロックは、各項目の差分がすでに期待値を改めている。

| ファイル | ブロック | 項目 |
|---|---|---|
| `test/visibility.t` | visrow26(V57 が足したブロック) | V50 |
| `test/eval.t` | solelit と soleopt のブロック、solevar2 のブロック、opcover のブロック(3 ブロック 4 コマンド) | V53 |
| `test/verify_fixes.t` | guardonly | V53 |
| `test/typecheck.t` | ml5c | V49(runtest で promote) |
| `test/typecheck_m7.t` | v10h | V48 |
| `test/instance_param_types.t` | inswild(V136 が足したブロック) | V48 |

新しいブロックは 6 ファイルに 25 ブロックである。

| ファイル | ブロック | 項目 |
|---|---|---|
| `test/visibility.t` | 2(vis4c〜vis4e、visrow4b〜visrow4e と visrow5b) | V47 |
| `test/visibility.t` | 2(visrow29、visrow30。visrow27 と visrow28 は visrow26 のブロックに足す) | V50 |
| `test/pub_effect_note.t`(新設) | 8 | V44 |
| `test/scope_escape_tparam.t`(新設) | 5 | V49 |
| `test/eval.t` | 1(opnone0、opnone2、opnone2ok) | V53 |
| `test/uninhabited_pattern.t`(新設) | 7 | V48 |

`extra.patch` は 12 ファイルで、`lib/elab.ml` が +148/-73、`lib/exhaust.ml` が +109/-19、`lib/unify.ml` が +23/-2 である(実測)。

#### 各項目の主張の確かめ

各項目の `ex/`、`exb/`、`ex2/`、`verify/ex/` の `.kel` 327 本を `--type-check` にかけ、合成の木、各項目の試作、本分野の合成の木の 3 つの処理系で出力を比べた(実測)。
- 合成の木と各項目の試作の差の件数は、各小節の主張と一致した。V47 は `ex/` の 12 本と `exb/` の 2 本(14 例)、V50 は `ex/` の 10 本、V44 は `ex/` の 8 本、V49 は `ex/` の 6 本、V53 は `ex/` の 7 本である。
- 各項目の試作と本分野の合成の木で出力が違うのは、V44 の `ex/a4.kel` の 1 本だけである(V49 の注記。上の重なりの確認のとおり)。
- V48 の `doc/log/261017-3-spike/V48/ex/run.kel` は `--strict-exhaustive` でも `42` を出し、終了状態 0 である。`doc/log/261017-3-spike/V48/ex2/valued.kel` の出力は試作の出力(`doc/log/261017-3-spike/V48/ex2/valued.out` の試作の段)と同じである。`ex2/deep.kel` と 200 段の 3 形(`blow_c_200`、`blow_f_200`、`blow_t_200`)の型検査はどれも 0.01 秒以下だった。

例の出力(本分野の合成の木。`--type-check`):

```
V47 ex/a1.kel
! a1.kel:2:26: 型エラー: 未知のエフェクト: W(M.W と修飾してください)
V50 ex/a03.kel
f : (Int32) => Int32
! a03.kel:3:29: 型エラー: 行変数 W は extends の位置にのみ書けます
V44 ex/a9.kel
! a9.kel:4:32: 型エラー: スコープ付きの型 ς1 がスコープの外に漏れています
V44 ex/a4.kel(V49 の注記が付く)
! a4.kel:4:29: 型エラー: スコープ付きの型 ς1 がスコープの外に漏れています(ς1 は型パラメータ A です。A のスコープの外の型と共有された、型の決まっていない変数(注釈の無い引数の型や、一般化されない束縛の型など)に A を入れようとしています)
V49 ex/v49_top.kel
! v49_top.kel:3:23: 型エラー: スコープ付きの型 ς1 がスコープの外に漏れています(ς1 は型パラメータ A です。A のスコープの外の型と共有された、型の決まっていない変数(注釈の無い引数の型や、一般化されない束縛の型など)に A を入れようとしています)
V53 ex/zero.kel
! zero.kel:3:9: 型エラー: 操作 ask には、ガードがなく、どんな引数にも一致する節がありません(どの節にも一致しなかった操作を外側のハンドラへ回す規則はありません)。ガードの無い case ask() を最後に置いてください
V48 ex/params.kel
pc : (Absurd) => Int32
pvar : (#V(Never)) => Int32
(警告なし。終了状態 0)
```

V44 の a9 は合成の木では「pub な宣言はエフェクトを起こせません(…)」の言い換えが付いていた。
V48 の params の出力は抜粋で、型の行は 10 行、警告は 0 行である。

#### ソースコメントの検査

本分野の合成の木で、`tools/litcheck.py --old <合成の木の同じファイル>` を `lib/elab.ml`、`lib/unify.ml`、`lib/exhaust.ml` に掛け、3 ファイルとも marker は 0 件だった(code はコードを変えたので不一致になる)。
`tools/litwrap.py` は 3 ファイルとも変更を出さなかった(既存の表の幅の報告だけ)。
写しは git を持たないので、実施のときに本体で `litcheck.py` を走らせ直す。

#### LangSpec の同じ行を触る項目の調整

- V50 は §13.1 の `doc/LangSpec.md:1116` の後に段落を足す。
  M[classes] の V97 は §13.1 の 1121 行の後に 1 文を足す。
  行が別で、内容(処理系の表示で `@` を省いた矢印の行の読み方)も V50 の段落(型パラメータが同名の別名を覆うこと)と重ならない。
- V48 は §11.2 の 916 行の後に段落を足し、M[never] の V134 も同じ位置に段落を足す。
  V134 が先に入るので、V48 の段落を V134 の段落の後に置く。
  V134 の段落は節のない `match {}` の照合対象の型変数を `Never` に決める規則で、V48 の段落は節が 1 つ以上の位置で受ける型に値が無いときに警告しない規則である。
  対象(節が 0 個と 1 個)が違い、矛盾しない。
  V48 の D[V48-4] は、コンストラクタを持つ値の無い型に対する `match {}` を変えないとしており、916 行の規則とも V134 の段落とも合う。
- V47、V44、V49、V53 は LangSpec を変えない。

#### 最終稿からの訂正

- V53 の (1) の出典で、`260926-1-plan.md:271-274` を §7.4 の注記としていたのは誤りで、この範囲は 2.2 節の延期の一覧である。初出は台帳の出典の欄のとおり §7.4 とし、2.2 節の延期を経緯の文に書いた。
- V49 の P[V49-3] に、反証者の挙げた句点で続ける形を代案 2 として足した(最終稿は統括者に委ねていた)。推奨案は変えない。
- V50 のコミットの見出しの scope を、リポジトリの慣行に合わせて `fix:` から `diktor:` に改めた。

#### ほかの分野への要望

- M[kinds] へ: 台帳の候補 V[V56-1] と V[V57-1] は、V50 で決着する。M[kinds] の台帳の扱いを「V50(M[extra])で解消」とする。
- M[diagtext] へ: V49 の `rigid_tparams`(剛定数の識別子から型パラメータの名前を引く表)は、V111 が群の束縛から型パラメータの名前を引く仕組みと寄せられる。寄せるかどうかは統括者が決め、寄せるなら V49 の後の別のコミットにする。
- M[never] へ: LangSpec §11.2 の 916 行の後の段落は、V134 の段落、V48 の段落の順に置く。
- 統括者へ: M[extra] の 6 件の差分は 10 分野の合成の木を前提にするので、M[extra] は 10 分野の後に置く。M[extra] の中の順は上の表のとおりである。
