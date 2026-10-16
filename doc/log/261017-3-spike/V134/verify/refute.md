# V134 の下書きの反証

## 1. 裏取り

- 親 doc/log/261017-1-plan.md の 125〜127 行は D342 である(正しい)。
- doc/log/261017-2-m63.md の 176 行は退行の表の V134 の行、266 行は台帳の V134 である(正しい)。
- LangSpec §11.2 は 889〜916 行で、行を閉じる規則は 907〜910 行、`match {}` の網羅は 916 行である(正しい)。
- lib/exhaust.ml の complete_sig は 452 行、missing は 642 行、check_entry は 888 行、drain は 918〜921 行である(正しい)。
- ただし §10.13 の記事は 803 行から始まる。下書きの「§10.13、888〜921 行」は check_entry と drain のコードの範囲である。
- lib/elab.ml の Exhaust.queue は 1862 行、drain の呼び出しは 3140、3153、3174、3419、5010、5590 行の 6 か所で、これで全数である(grep で確認)。
- test/never_ops.t の前書きは 77〜78 行だが、`$ diktor absurd.kel` のコマンドは 91 行である。下書きの「89 行のコマンド」は誤り(89 は runtest の差分のハンクの開始行)。
- 試作の差分は 99 行で、追加行は 25 行(exhaust.ml が 19 行、elab.ml が 6 行)である。
- 既存の cram の `match {}` は never_ops.t、typecheck_m5.t、typecheck_m7.t の 6 行で、後の 2 つは `n: Never` と注釈した absurd である。

## 2. 実測の再現

担当の ex/ の全プログラムを HEAD と試作で走らせ直した(verify/r.head.txt、r.new.txt、r.repl.txt)。
出力は担当の results.head.txt、results.new.txt と、並び順を除いて一致した。

## 3. 試作の差分

verify/diktor に当てて dune build と dune runtest を走らせた。
runtest の差分は never_ops.t の absurd の警告 1 行だけで、担当の runtest.log と一致した(verify/runtest.log)。

## 4. 退行(verify/ex/、出力は verify/reg.txt と reg2.txt)

HEAD が警告付きで受理し正しく実行するプログラムを、試作は型エラー(終了状態 1)で拒否する形が 9 本ある。

- r11_void_helper.kel:`newtype Void` と `let elim(x) = x match {}` と `let use(v: Void): Int32 = elim(v)`。HEAD は警告と ok、試作は「3:32: 型が一致しません: Void と Never」。
- r12_void_inst.kel:`type instance Describe[Void] { let describe(n) = n match {} }`。HEAD は警告と ok、試作は「インスタンスメソッド describe がクラス宣言の型を満たしません(型が一致しません: Never と Void)」。
- r13_void_lambda.kel:`let h = id(fn(v) => v match {})` を Void に使う。HEAD は警告と ok、試作は型エラー。
- r1_nongen_top.kel:値制限で一般化しない `let h = id(fn(y) => y match {})` を、実行しない枝で `h(3)` に使う。HEAD は警告と 0、試作は「Never は Integral のインスタンスではありません」。
- r5_local_nongen.kel:同じ形を関数の中の局所の let で書く。HEAD は警告と 0、試作は型エラー。
- r2_weak_dexp.kel:弱い型変数を持つ `k` に対する式文 `fn(z) => k(z) match {}` の後で `k(3)`。HEAD は警告と 3、試作は型エラー。REPL でも同じ(r10.repl)。
- r3_inst_int.kel、r9_inst_generic.kel:Int32 のインスタンスのメソッドの中の `n match {}`。HEAD は警告と ok、試作は型エラー。
- r6_module.kel:module の中の `let g(x) = x match {}` を、実行しない枝で `g(3)` に使う。HEAD は警告と 1、試作は型エラー。

出力が変わる形が 1 本ある。

- r4_order.kel:`let f(x) = { let a = x match {}; let b = 3 match { case 1 => 1 }; x + 1 }`。HEAD は「例えば _」「例えば 0」の順、試作は逆の順に警告を出す。

原因(ソースを読んで導いた性質):
D[V134-1] の条件 (c)「vlevel > level なら一般化される」は、gen が偽の束縛(値制限、パターン束縛)では成り立たない。
lib/elab.ml 3146〜3148 行は、gen が偽のとき変数を一般化せず lower_levels で外側のレベルに下げ、後の使用で決まるようにする。
試作はその前に Never に決めてしまう(r1、r5)。
式文の drain のレベル -1 は、前の最上位の束縛の弱い型変数(レベル 0)も決める(r2)。
インスタンスのメソッドは期待型なしに推論され、drain の後で宣言の型と照らす(V136 の下書きの 4576 行と 4514 行)ので、Never 以外の型のインスタンスでは決めた Never が宣言と衝突する(r3、r9、r12)。
コンストラクタを持たない型は Never だけでなく、右辺を省略した newtype(LangSpec 834 行)もあるので、注釈のない引数の型を Never に決めると Void に使えなくなる(r11、r13)。
r6 は担当が意図した変化(unannparam_call)と同じ類だが、HEAD で呼び出しを実行しない形は「警告と実行時エラー」ではなく正しく実行するので、(6) のリスクの書き方が狭い。

## 5. 仕様の文面

案の 2 文目「後者の引数の型は Never と推論される」は、LangSpec 834 行と 916 行の、Never 以外のコンストラクタを持たない型に対する `match {}` を、注釈のない補助関数では書けなくする(r11)。
§18.2 の並べ替え、§15.5 の終了状態、付録 A の文法とは矛盾を見つけていない。
§15.5 の 1636 行「警告があっても、プログラムは実行する」は、警告付きで実行できたプログラムが型エラーになる r1〜r13 の変化を、利用者には後退として見せる。

## 6. 推奨案への異論

代案 F を見落としている。
決める変数を、節のない match の照合対象で制約の無い未定の型変数のうち、drain を呼んだ束縛の型(式文では式の型)に現れないものに限る。
そのような変数は束縛の外から観測できないので、Never に決めても受理と型の表示は変わらない(LangSpec §6.2 598〜600 行の曖昧な型変数と同じ考え方)。
束縛の型に現れて gen が偽なら、局所では外側の drain に送り、最上位では従来どおり警告する。
試作(verify/altF.patch、115 行)で実測した。

- direct、helper、valbind、abort、laterlet、laterlet2、dexp は警告なしで --strict-exhaustive も終了状態 0(A と同じ)。
- okarm は A と同じく到達不能の警告。
- r1〜r3、r5、r6、r9、r11〜r13 は HEAD と同じく警告付きで受理し、同じ出力。
- unannparam(`let f(x) = x match {}`)、lamdefer、rowkind、v136 は HEAD と同じ警告のまま(f の型は (A) => B)。
- dune runtest の差分は A と同じ 1 行。
- r4 の警告の順序の変化は F でも残る(送りによる)。

F は台帳の V134 の形をすべて解き、HEAD の受理を狭めない。
V136 は F では解けないが、V136 には期待型を渡す独自の修正案がある。
推奨を F に替えるか、A を採るなら r1〜r13 の拒否を意図した変化として仕様とリスクに書くべきである。

## 7. ほかの項目との重なり

- V136:同じ elab_binding の drain の前後を扱う。V136 の修正(期待型を渡す)が先に入ると、r3、r9、r12 の A の拒否は消え、v136.kel が V134 に依存しなくなる。下書きの「V136 は本項に含めて閉じられる」は V136 の下書きの修正案と食い違う。
- V129:同じ check_entry の文面を変える。A でも F でも V129 の文面が出る範囲が変わるので、V129 の cram の例(helper.kel)は V134 の後で警告が消える。順序を決める必要がある。
- V135:同じ規則 1 の perform の型変数を扱う。制約の付いた変数は V134 で決めないので振る舞いは独立だが、移行の書き方の文面(`: Never` の注釈)を揃える。
- never_ops.t:V135 が同じファイルにブロックを足し、V128、V133、V137 の下書きも同じファイルを参照する。absurd の前書き(77〜78 行)と期待値の変更をほかの項目と同じコミットに混ぜない。
- D342 と P86:P86(261017-1-plan.md 780 行、261017-2-m63.md 284 行)の答え「言及しない」も覆る。
