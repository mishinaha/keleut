import sys
p=sys.argv[1]
s=open(p,encoding='utf-8').read()
def rep(old,new):
    global s
    assert s.count(old)==1, old[:60]
    s=s.replace(old,new)
rep('''          let bare = match row_fields se with [], tail -> ( match repr tail with TVar _ | TRowEmpty -> true | _ -> false) | _ -> false in
          (try Unify.unify ie se
           with Type_error msg ->
             if bare && row_failure msg then
               type_error
                 ("型クラスのメソッドの実装は純粋でなければなりません(宣言の最外の行にラベルがありません — 仕様 §9)。インスタンスメソッド " ^ mname
                ^ " の本体がエフェクトを起こしています。元の報告: " ^ msg)
''','''          let bare = match row_fields se with [], tail -> ( match repr tail with TVar _ | TRowEmpty -> true | _ -> false) | _ -> false in
          (* 実装が最外の @ を自分で書いていれば、空でない行はその注釈に由来し、
             本体が何も起こしていなくても落ちる。そのときは本体ではなく注釈を指す *)
          let culprit =
            if outer_eff_written (snd bnode) then " の注釈の最外の行が空ではありません。元の報告: "
            else " の本体がエフェクトを起こしています。元の報告: "
          in
          (try Unify.unify ie se
           with Type_error msg ->
             if bare && row_failure msg then
               type_error
                 ("型クラスのメソッドの実装は純粋でなければなりません(宣言の最外の行にラベルがありません — 仕様 §9)。インスタンスメソッド " ^ mname
                ^ culprit ^ msg)
''')
rep('''   判定の材料が宣言の側だけなので、名指しは本来の対象より少し広く働く。
   実装が自分の注釈に `@ Print` と書き、その本体は何も起こしていないときも、
   宣言の行にラベルが無ければ同じ文言で落ちる。
   この形で実際に起きているのは、実装の最外の行が空でないことで、
   文言の後半の「本体がエフェクトを起こしています」は字義どおりには成り立たない。
   それでも、直し方(実装の側の注釈を外す)には辿り着けるので、本実装はこの文言を使う。
''','''   名指しするかどうかは宣言の側の行だけで決まるが、
   名指しの後半で何が空でない行を作ったかを述べる部分は、実装の側の束縛を見て決める。
   実装が最外の `@` を自分で書いていれば(`outer_eff_written`。§11.28)、
   空でない行はその注釈に由来する。
   実装が自分の注釈に `@ Print` と書き、その本体は何も起こしていないときも包摂は落ちるので、
   このときは「本体がエフェクトを起こしています」ではなく
   「注釈の最外の行が空ではありません」と述べる(clsannotrow)。
   本体が実際に `Print` を起こしていても、注釈を外せば本体の行が残って前者の文言になるので、
   注釈の側を先に述べて嘘にならない。
   最外の `@` を書いていなければ、実装の行は本体から推論したものなので、
   空でない行は本体が何かを起こしたことを意味する。
   型エイリアスを頭に書いた値束縛の注釈は、`outer_eff_written` が `@` を書いた側に倒すので、
   エイリアスの中の `@ Print` も注釈の側として述べる。
''')
open(p,'w',encoding='utf-8').write(s)
