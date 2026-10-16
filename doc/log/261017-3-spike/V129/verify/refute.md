# V129 の下書きの反証

## 1. 裏取り(写しと HEAD のビルド)

`lib/exhaust.ml` の `complete_sig` は 452 行、`missing` は 642 行、`check_entry` は 888〜916 行、反例の文面は 901 行で、下書きのとおりである。
§10.7 は 395 行、§10.10 は 570 行、§10.13 は 803 行から始まる。
`lib/elab.ml` 1862 行の `Exhaust.queue` は下書きのとおりである。
`perform` の式を新しい型変数にする行は 1924 行で、下書きの 1923 行は 1 行ずれている。
`Exhaust.queue` のほかの呼び出しは 1656、2180、2708、3109、3173、3417 行の 6 か所で、どれも 1 行の項目を積む。
節が 0 個の項目は `match {}` からしか積まれないという主張は正しい。
LangSpec 913〜916 行(§11.2)、1633〜1634 行(§15.5)、452 行は下書きのとおりである。
ただし 452 行は §5 の中の §5.6(穴)である。
`doc/log/261017-1-plan.md:760` と `doc/log/261017-2-m63.md:258` も下書きのとおりである。
`test/never_ops.t` の 77〜78 行の前書きと 92 行の期待値も下書きのとおりである。
`V129.patch` は 118 行である。
`Unify.is_predicate` は `Integral` と `Fractional` だけを真にする(`lib/unify.ml:275`)。

## 2. 実測の再現

担当の `ex/` の 17 本を HEAD と、自分の写しに試作を当てたビルドで走らせ直した(`rerun.txt`)。
新しい文面になるのは absurd、fnlit、helper、late、outer の 1 行目、param、showvar、strict で、従来の文面のままなのは emptyrec、guard、int、lit、openvar、rigid、outer の 2 行目である。
never は警告なし、asc は構文エラー(終了状態 2)で、どちらのビルドも同じである。
`--strict-exhaustive strict.kel` はどちらも終了状態 1、`--repl < repl.in` の文面も試作で新しい文面になる。
分類は下書きのとおりである。

ただし `results.spike.txt` の前半 12 件(absurd から strict --strict-exhaustive まで)は、古い文面「照合対象の型が決まっていないので、節のない match {} を網羅的と判定できません(… になるのは … ときです)」で記録されている。
試作の `undetermined_msg` の文面と違うので、下書き (5) と (7) が参照する結果の写しとして古い。

## 3. 試作の差分

`V129.patch` は写しにそのまま当たった。
`dune build` は通り、`dune runtest` は緑である(`runtest.log` は空)。
`test/never_ops.t` を HEAD に戻して `lib/exhaust.ml` の変更だけで走らせると、差分は absurd の 92 行の 1 行だけである(`runtest.codeonly.log`)。
担当の分類は正しい。
`tools/litwrap.py lib/exhaust.ml` は何も変えず、`tools/litcheck.py --old` は marker 0 件(code の不一致はコードを変えたため)である。

cram の数え方は合わない。
足すのはプログラム 3 本(absurdhelper、absurdparam、absurdother)、コマンド 7 行(`cat` が 3 行、`diktor` が 4 行)、前書きは 2 段落なのでブロックは 2 つである。
下書き (1) の「3 ブロックの追加」と (5) の「4 コマンド」は数え方の定義に合わない。

## 4. 退行の候補(`verify/ex/`、出力は `probe.txt`)

試作は、検査の時点で型変数のままでも、同じ宣言の後ろで型が決まる照合対象にも新しい文面を出す。
下書き (6) は後で `Never` に決まる形(`late.kel`)だけを挙げ、新しい文面は HEAD より原因に近いとする。
後でコンストラクタを持つ型に決まる形では、「照合対象の型が決まっていません」は利用者から見て偽である。

- `p01_laterint.kel`(`let f(x) = { let r: Int32 = x match {}; x + 1i32 }`):HEAD は「例えば _ が漏れています」、試作は新しい文面。`x` の型は `Int32` である。
- `p02_lateropt.kel`(同じ形で、後ろで `x` を `Some`/`None` で照合し `f(Some(2i32))` を呼ぶ):HEAD も試作も実行時エラー「match のどの節にも一致しません: Some(2)」(終了状態 3)で、警告だけが変わる。試作は型が決まっていないと言うが、`x` は `Option[Int32]` である。
- `p14_repl.in`(REPL に p01 の `f` を入れる):試作は新しい文面の直後に `f : (Int32) => Int32 = <fn>` を表示し、文面と表示した型が食い違う。
- `p03_instnever.kel`(`type instance Describe[Never] { let describe(n) = n match {} }`、V136 の形):HEAD は「例えば _」、試作は「照合対象の型が決まっていません(… Never のように…)」。インスタンスの頭は `Never` なので、利用者には矛盾した案内に読める。
- `p10_twomatch.kel`:2 つの局所束縛の `match {}` と `let g(x) = x match { }` の 3 件がすべて新しい文面になる。

文面以外(受理、終了状態、実行の出力)は、試した 14 本(module の中、import、エイリアス `type N = Never`、型パラメータとガード、入れ子のハンドラ、穴 `???`、レコードのフィールドを読んだ引数、`--prelude` と `--no-prelude`、REPL)のどれでも HEAD と同じである。
import したファイルの警告は試作でも `imp/lib.kel:` を前置する。

穴(`??? match {}`、`p04_module.kel`、`p08_hole.kel`)と、注釈のない高階の引数の返り値(`p12_generic_eff.kel`)も新しい文面になる。
下書きの「対象は perform の形、間接の形、注釈のない引数の 3 つ」は、型で決まる条件の例示であって全部ではない。

## 5. 仕様との整合

仕様は変えないので、§15.5(警告は 3 種類で文面は定めない、1633〜1637 行)、§11.2(916 行)、§18 の 1782 行(警告の文面は評価の結果に含めない)と矛盾しない。
付録 A の文法は関わらない。

## 6. ほかの項目との重なり

V134 の試作(`items/V134/V134.patch`)を HEAD に当て、その上に V129 の試作を当てると、どちらも当たり(exhaust.ml の hunk 2 は fuzz 2)、ビルドも通る。
しかし V129 の cram は 5 行で落ちる(`runtest.combo.log`)。
V134 は節のない `match {}` の照合対象で制約の無い未定の変数を `Never` に決めるので、absurd、absurdhelper、absurdparam の `f` の警告が消える。
組み合わせたビルドで新しい文面が残るのは、`Show` などの制約を持つ変数(`showvar.kel`、absurdparam の `h`)だけだった。
p01、p02、p14、late、outer の 1 行目は、組み合わせたビルドでは従来の文面か警告なしになる(p02 は「例えば None が漏れています」)。
したがって V134 を推奨案で採るなら、V129 の対象はクラスの制約を持つ変数に縮み、cram の期待値も作り直しになる。
下書きの「依存は無い」「V134 で範囲を変えても条件は独立に働く」は、コードの条件については正しいが、対象と cram の期待値については成り立たない。

V136 の試作も `n match {}` の警告を消す(V136 の下書き)ので、p03 の文面の食い違いは V136 か V134 を採れば消える。
`test/never_ops.t` の absurd(77〜92 行)は V129 と V134 の両方が変える。
`lib/exhaust.ml` §10.13 の `check_entry` と記事は V129 と V134 の両方が触る。

## 7. 判断事項の推奨案について

P[V129-1]、P[V129-2]、P[V129-3] の推奨案には異論が無い。
見落とした代案が 1 つある。
文面を「照合対象の型が、この時点では Never のようなコンストラクタを持たない型と分かっていません」のように、検査の時点の事実に限って言う形である。
この形は p01、p02、p14、p03 でも偽にならない。
もう 1 つは、V134 を先に採り、V129 をその残り(制約つきの変数)に限って扱う順序である。
