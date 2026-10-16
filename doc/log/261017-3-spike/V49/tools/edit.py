import sys
root = sys.argv[1]
def sub(path, old, new):
    p = root + "/" + path
    s = open(p, encoding="utf-8").read()
    assert s.count(old) == 1, (path, old[:60], s.count(old))
    s = s.replace(old, new)
    open(p, "w", encoding="utf-8").write(s)

# unify.ml: table of type-parameter names for rigids, and the note in occurs_adjust
sub("lib/unify.ml",
"""   利用者のプログラムの誤りではなく実装の誤りである。 *)

let rec occurs_adjust tv lvl t =""",
"""   利用者のプログラムの誤りではなく実装の誤りである。

   脱出の報告には、剛定数が型パラメータなら、その名前と漏れの原因を添える。
   型パラメータの剛定数が浅いレベルの変数に入り込むのは、
   その変数が、型パラメータを導入した束縛より外側で作られ、まだ型が決まっていないときである。
   外側の注釈の無い引数や、一般化しない束縛の型がそれに当たる。
   剛定数の表示 `ς1` だけでは、どの型パラメータがどこへ漏れたのかが読めない。
   名前は `rigid_tparams` に、第11章の `make_rigids` が剛定数を作るときに登録する。
   `run` のヒープとインスタンス検査の skolem は登録しないので、報告は元のままである。 *)

let rigid_tparams : (oid, string) Hashtbl.t = Hashtbl.create 16

let escape_message v (i : var_info) =
  let base = "スコープ付きの型 " ^ show (TVar v) in
  match Hashtbl.find_opt rigid_tparams i.vid with
  | None -> base ^ " がスコープの外に漏れています"
  | Some n ->
      base ^ "(型パラメータ " ^ n ^ ")がスコープの外に漏れています(" ^ n
      ^ " を導入した束縛より外側で作られた、型の決まっていない変数の型に " ^ n
      ^ " を入れようとしています。外側の注釈の無い引数や、一般化されない束縛の型などがこれに当たります)"

let rec occurs_adjust tv lvl t =""")
sub("lib/unify.ml",
"""          if i.vlevel > lvl then type_error ("スコープ付きの型 " ^ show (TVar v) ^ " がスコープの外に漏れています")""",
"""          if i.vlevel > lvl then type_error (escape_message v i)""")

# elab.ml make_rigids registers the names
sub("lib/elab.ml",
"""      let r = new_rigid_ref ~kind ~classes level in
      (tp.tp_name, TVar r, r))""",
"""      let r = new_rigid_ref ~kind ~classes level in
      (match !r with Rigid i -> Hashtbl.replace Unify.rigid_tparams i.vid tp.tp_name | _ -> ());
      (tp.tp_name, TVar r, r))""")
