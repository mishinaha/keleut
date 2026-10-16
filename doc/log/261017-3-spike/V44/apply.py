import sys
p = sys.argv[1]
s = open(p, encoding='utf-8').read()
def rep(old, new):
    global s
    assert s.count(old) == 1, old[:60]
    s = s.replace(old, new)

# 1. escape_failure after row_failure
rep('''  has "行型ではありません" msg || has "スコープ付きの型" msg || has "ラベル " msg || has "は注釈で固定された行変数" msg
''', '''  has "行型ではありません" msg || has "スコープ付きの型" msg || has "ラベル " msg || has "は注釈で固定された行変数" msg

(* 失敗が、剛定数がスコープの外に漏れたことによるかどうか。
   pub の言い換えのうち、呼び出し先の行の単一化(§11.12)は、この失敗を言い換えから除く。
   pub で @ を省略した本体の行は空の行で、剛定数を含まない。
   そこで漏れが起きるのは、run の本体の行のヒープのような、本体の中で作った剛定数を、
   外側の単相の型変数に入れようとしたときである。
   その原因は本体がエフェクトを起こすことではないので、pub の規則を名指さない *)
let escape_failure msg = has_sub "がスコープの外に漏れています" msg
''')

# 2. helper after outer_eff_written
rep('''  | None -> ( match b.T.lb_ret with Some (_, T.EArrow (_, _, eff)) -> eff <> None | Some _ -> true | None -> false)
''', '''  | None -> ( match b.T.lb_ret with Some (_, T.EArrow (_, _, eff)) -> eff <> None | Some _ -> true | None -> false)

(* pub で頭の @ を省略した値束縛の、注釈と初期化式の型の単一化(§11.28、§11.29)。
   注釈の頭の行は空の行である。
   初期化式の型の頭も矢印なら、頭の行を除いた部分を先に単一化し、頭の行どうしを最後に単一化する。
   pub の規則を名指す文言にするのは、頭の行どうしの失敗だけである。
   引数、返り値、入れ子の矢印の行の失敗は other に渡す。
   レコードのフィールドやヴァリアントの選択肢の失敗も、エフェクトの行の失敗と同じ
   「ラベル … がありません」の文言になるので、文言からは見分けられないからである。
   単一化する組と順序は、型全体を 1 度に単一化するときと同じである *)
let unify_pub_value_annot vty body ~other =
  match (repr vty, repr body) with
  | TArrow (p1, r1, e1), (TArrow (_, _, e2) as tb) ->
      (try Unify.unify (TArrow (p1, r1, e2)) tb with Type_error msg -> other msg);
      (try Unify.unify e1 e2
       with Type_error msg ->
         type_error ("pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: " ^ msg ^ ")"))
  | _ -> ( try Unify.unify vty body with Type_error msg -> other msg)
''')

# 3. Apply path
rep('''         if pub_pure && row_failure msg then
           type_error ("pub な宣言はエフェクトを起こせません''',
    '''         if pub_pure && row_failure msg && not (escape_failure msg) then
           type_error ("pub な宣言はエフェクトを起こせません''')

# 4. value binding (§11.28)
rep('''        (try Unify.unify vty (subsume_closed lvl body_ty vty)
         with Type_error msg when b.T.lb_ret <> None ->
           (* pub の省略 @ の値束縛では、失敗の原因は本体が純粋でないこと。
              関数束縛(§11.12)と同じ言い換えをここでも置く *)
           if b.T.lb_pub && (not (outer_eff_written b)) && row_failure msg then
             type_error ("pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: " ^ msg ^ ")")
           else type_error ("注釈された型を満たしません(" ^ msg ^ ")"));
''', '''        let body_ty = subsume_closed lvl body_ty vty in
        (* pub の省略 @ の値束縛では、頭の行の失敗の原因は、初期化式が返す関数が純粋でないこと。
           関数束縛(§11.12)と同じ言い換えを、頭の行の失敗にだけ置く *)
        if b.T.lb_pub && not (outer_eff_written b) then
          unify_pub_value_annot vty body_ty ~other:(fun msg -> type_error ("注釈された型を満たしません(" ^ msg ^ ")"))
        else (
          try Unify.unify vty body_ty
          with Type_error msg when b.T.lb_ret <> None -> type_error ("注釈された型を満たしません(" ^ msg ^ ")"));
''')

# 5. let rec (§11.29)
rep('''            (try Unify.unify vty (subsume_closed lvl (elab_exp env_ty lvl eff b.T.lb_body) vty)
             with Type_error msg when b.T.lb_pub && (not (outer_eff_written b)) && row_failure msg ->
               type_error ("pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: " ^ msg ^ ")"));
''', '''            let body_ty = subsume_closed lvl (elab_exp env_ty lvl eff b.T.lb_body) vty in
            if b.T.lb_pub && not (outer_eff_written b) then unify_pub_value_annot vty body_ty ~other:type_error
            else Unify.unify vty body_ty;
''')
open(p, 'w', encoding='utf-8').write(s)
