# 分野 eval の節と合成差分の反証

対象は clusters/eval/section.md と clusters/eval/eval.patch である。
作業領域は clusters/eval/verify/ で、合成後の写しは verify/diktor、実行の出力は verify/out と verify/runtest.out にある。

## 1. 合成の再現(実測)

- HEAD の写しに eval.patch を patch -p1 で当てた。--dry-run で全 8 ファイルが当たり、.orig と .rej は出なかった。
- 別の写しに V137、V109、V108、X-B11、V37 の各差分をこの順で当てた木は、eval.patch を当てた木と diff -r で一致した。offset は order.txt の記述(V109 は interp.ml -14、value.ml -1、V108 は interp.ml -14/-15、value.ml +8/+17、X-B11 は -13)と一致し、fuzz は無かった。
- dune build は通り、Warning は 2 件(lib/parser.mly)だった。
- dune runtest の出力は 0 バイト、終了 0 だった。
- 合成後の 4 つの .t を HEAD の実装の写しで走らせると、repl.t と tail_calls.t だけが落ちた。retrycancel と retrycancelvia は 66229 行と 33771 行、retcancelret は [3] と 33761 行と 1 行、repl.t の 1 つ目は p、_、k、m、v、_、_、q、_ が <fn>、t と h が <prim Show.show>、2 つ目は match の実行時エラーで x が未束縛になり z が U! になった。dictionary_passing.t と classes.t は通った。担当の分類と一致する。
- 新しいブロックの数(tail_calls.t 2、dictionary_passing.t 1、repl.t 2、classes.t 1)、tail_calls.t の 4 プログラム 12 コマンド、repl.t の入力 14 行と 8 行、classes.t の 37 行の追加は、差分を数え直して一致した。
- litcheck.py --old は 4 ファイルとも marker 0 件、litwrap.py は interp.ml、value.ml、driver.ml で変化なし。
- ほかの分野の差分(V128、V103、V106、V114、V115、V118、V121、V123、V33、V83、V88、V97、X-E13、X-F5b)は合成後の木に fuzz なしで当たった。V128-alt は hunk 3 が fuzz 1。V128 を当てた木の dune runtest は 0 バイトだった。eval の 4 ファイルとそのテストを触る差分は、この 14 本と V128-alt で漏れが無い(全項目の差分の見出しを grep した)。

## 2. 例の実行(合成後と HEAD、実測)

- V137: retrycancel、retrycancelvia、grow_tick は合成後 0、終了 0、正常な記録 100000 行。HEAD は 33771 行と Stack overflow 66229 行。retcancelret は合成後 0、終了 0、100000 行、HEAD は出力なし、終了 3、33761 行と 1 行。retrycancelsame はどちらも 100000 行。節の表と一致する。
- V138: nontail_resume.tmpl は HEAD 5404 で通り、合成後 7946 で通り 7947 で落ちる。retryresumepath.tmpl は HEAD 6431、合成後 7944。V137 の nontail_N.kel は HEAD 5403 で通り 5404 で落ちる。
- V109: capdict は 1!true!1!。cap.kel は HEAD と同一。3 回の最小で b2_poly 0.42 秒(HEAD 0.84)、b2b_add 0.39 秒(HEAD 0.67)、b2a_mono 0.38 秒(HEAD 0.38)。
- V108: block.txt と r1〜r5 の --repl の出力が results.proto.txt と空行を除いて一致した。
- V37: final/ps2.kel(cram の pshowsplit と同じ形)の出力 22 行は HEAD と ps2.head.out に一致。edge.kel は HEAD と同一。show80000 は合成後 0.107 秒、HEAD 5.48 秒、show1000000 は合成後 1.39 秒。
- X-B11: ex の 6 本と repl.in は HEAD と出力と終了コードが一致。probe.ml を合成後の木で組むと probe.patched.txt と一致した。

## 3. 最終稿との照合

5 項目と吸収した V138 の最終稿を節と diff で比べた。
判定、判断事項、裁定、差分の内容、cram の数、LangSpec の行(475、479〜482、511、1323〜1329、1629)、interp.ml と value.ml の行(30、406〜412、673、811、954、969〜979、995〜1004、1047〜1063、1093、1109、88、248、481)は HEAD の写しで確かめて正しかった。
変更は、試作を差分のパスに直したもの、合成の結果を足したもの、親のコミットを分野の最後にまとめたものに限られ、内容の取り違えは見つからなかった。
V138 の D[V138-1]、D[V138-2]、P[V138-1] は D[V137-2] と P[V137-1] に吸収したと書いてある。
V[V138-2](V128 の文面を合わせる)は節の番号としては消え、N.z のほかの分野への要望に置き換わっているが、そう明記していない。

## 4. 指摘

### 直すべきもの

1. 末尾でない resume の深さの数が節の中で 2 通りに見える。
   (2) の表は nontail_resume.tmpl の HEAD を 5404 とし、(6) と P[V137-3] は「5403 段」とする。
   どちらも実測で正しいが、前者は V138 の nontail_resume.tmpl、後者は V137 の ex/nontail_N.kel(ask を末尾でない resume で受ける形)で、プログラムが違う。
   (6) と P[V137-3] にプログラム名を書くか、どちらかの数にそろえる。
2. N.z の分野 classes と diagtext への要望の「先にこの分野を当てる順を要望する」は、どちらの分野を先にするのか読めない。
   さらに、合成後の木に V97、V103、V33 が fuzz なしで当たり、逆に HEAD に V97 と V33 を当てた木にも V37 は offset 39 行だけで当たった(実測)ので、順の要望は要らない。
   「どちらの順でも当たる」と書き換える。
3. eval.patch の見出しの --- 行に一時領域の絶対パスが入っている(+++ 行は diktor/lib/...)。
   統括者が doc/log/261017-3-spike/eval/eval.patch として親に写すと、一時領域のパスがリポジトリに入る。
   N.z の「各差分の見出しは diktor/lib/... の形」も --- 行については正しくない。
   見出しを a/、b/ の相対の形で作り直す(各項目の差分も同じ形なので、統括者が一括で直すなら節の文面だけ直す)。

### 直すとよいもの

- 親のコミットの順: spec の §5.7 の改訂を、サブモジュールを進めるコミットより前に置くと、その間の親は仕様が保証を述べ実装が守らない状態になる。サブモジュールを進めるコミットの後に spec を置くか、順を書く。
- V[V138-2] を N.z の要望に置き換えたことを V137 の (8) に 1 文書く。
- V109 の差分の「追加と削除は合わせて 47 行」は、空行の +/- を数えないときの数である(空行を含めると 52 行)。最終稿のままなので、数え方を書くか、そのままにする。
- 第13章(V37 の (3) の台帳の引用)は引用なので空白を入れない扱いでよいが、引用であることは括弧で示されている。

## 5. 文章規範

一時領域のパス、英語の文、禁止の語、仮の番号の崩れは見つからなかった。
番号は D[...]、V[...]、P[...]、M[eval] の形だけで、既存の番号(D339、D340、M10、M30)は引用か既存の記録である。
である調は保たれている。
