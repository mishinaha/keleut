# V138 の下書きの反証

## 裏取り

試作の差分 V138.patch は写し verify/diktor にそのまま当たった。
dune build は通り、dune runtest の出力は空である(verify/runtest.out)。
期待値が変わるブロックは無いという担当の分類は正しい。

行番号は次のとおりである。
lib/interp.ml の T.Resume の分岐は 406-412 行で正しい。
exnc は 995 行から 1004 行(raise ex); まで)で、下書きの 995-1003 は 1 行短い。
再開しない操作の分岐は 1049 行の Some (locals, c, `Never) から 1063 行までで、下書きの 1047 行は直前の None の分岐の discontinue である。
value.ml:481 の <pending> の枝はある。
doc/log/261017-2-m63.md の 221 行(退行(閉包の版))と 269 行(V138 の台帳)は正しい。
LangSpec の §5.7 は 455 行から 515 行で、479 行と 511 行の文面は下書きの引用どおりである。
下書きの 455-520 の 516 行以降は §5.8 である。

差分の行数 85 は、patch の +/- の行の数と一致した。
コードの行は、削除が 5 行(T.Resume の match 1 行、let tok、match、2 つの腕)、追加が 4 行(continue、exnc の腕、discontinue の 2 行)である。
差し引きは 1 行減で、「差し引き約 6 行」は数として合わない。

## 実測の再現

OCAMLRUNPARAM=l=100k の二分探索(verify/depth.sh)で、次の値を得た。
nontail_resume は HEAD で 5404、試作で 7946、66a6de2 の前(diktor-pre66)で 7110、V137 の試作で 7946 である。
retryresumepath は HEAD で 6431、試作で 7944 である。
どれも下書きの値と一致した。

## 退行の探索

verify/ex/ に次のプログラムを書き、HEAD と試作で走らせて、標準出力と標準エラーと終了状態を比べた。
すべて一致した。

- r1_three.kel: cancel 節が同じハンドラの再開しない操作を起こす入れ子を 3 段重ねた形。
- r2_bodyerr.kel: 打ち切りの後に先の破棄の節の本体がゼロ除算で脱出する形。外側の cancel 節が走り、終了状態 3。
- r3_bodyperform.kel: 先の破棄の節の本体が外側のハンドラの操作を起こし、外側は末尾でない resume で再開する形。
- r4_guard.kel: ガードの付いた再開しない操作の節と、末尾でない resume の節が混ざる形。
- r5_poly.kel: module の中の pub な型パラメータ付きの関数(@ Console を明示)。
- r6_errinresume.kel: 末尾でない resume の戻り値として届く節の本体がゼロ除算で脱出する形。
- r7_tailafter.kel、r8_nestedcancel_tail.kel、r10_resumepath_tail.kel、r11_same_rec.kel: 10 万回の再試行(l=100k)。どちらも完走する。
- r9_rethrow.kel、r12_outerask.kel、r13_twonever.kel: 節の本体からの再送出、外側のハンドラの ask を経る形、2 つの再開しない操作を使い分ける形。
- REPL(repl_in.txt)、import(imp/main.kel)、--prelude で写したプレリュードに差し替えた r3。

退行は見つからなかった。

## 記事の取り残し

試作は lib/interp.ml の冒頭の対応表(30 行)を変えていない。
その行は「再開しない操作の節 | 節の本体の前に discontinue、本体は effc の末尾で評価」で、試作の後は本体を exnc で評価するので事実と合わない。
同じ冒頭の 54 行(継続を捨てた後に包みの外で評価する)は試作の後も成り立つ。

## 打ち切りの段落について

試作は「打ち切られたハンドラの effc は」を「打ち切られたハンドラの discontinue の呼び出しは、それが戻るまで…枠を残す」と書き換える。
この文はソースから導いた性質として書かれているが、実測の裏付けが無い。
V137 の例(items/V137/ex の retrycancel、retrycancelvia、retrycancelsame、grow_tick)は、l=100k で 10 万回を HEAD でも試作でも V137 の試作でも完走した(出力 0)。
items/V138/ex の same、grow_outer、grow_via は、l=10k で 30 万回を HEAD でも試作でも完走した。
したがって、台帳 269 行と同じ文書の V137 の行の「l=100k で 10 万回がスタックオーバーフロー」は、これらの例では再現しない。
この段落は V137 の担当の結論に従わせ、V138 の差分では書き換えないほうがよい。

## 判断事項

P[V138-1] の推奨は V137 の扱いに依存させているが、型を分ける利点(VPending が show、==、match に届かないことを型が保証する。value.ml:481 の枝が不要になる)は V137 の領域の主張と独立である。
V137 の前提が上のとおり再現しないので、V137 が延期や取り下げになっても V138 で型を分けられるよう、代案 2 を推奨にするか、V138 を V137 に統合するかを統括者が決めるのがよい。

P[V138-2] の推奨(見張らない)に異論は無い。
既存の nontail_resume(10 万回で落ちる)と retryresumepath(10 万回で落ちる)の前書きは、試作の後も成り立つ(上限は 7946 と 7944)。
