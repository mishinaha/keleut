import sys
p = sys.argv[1] + '/lib/unify.ml'
s = open(p, encoding='utf-8').read()
def rep(a, b):
    global s
    assert s.count(a) == 1, a
    s = s.replace(a, b)
rep('''let show2 a b = !show2_ref a b
''', '''let show2 a b = !show2_ref a b

let show_pair_ref : (ty -> ty -> string * string) ref = ref (fun _ _ -> ("<型>", "<型>"))

(* 2 つの型を 1 つの名前の表で印字し、別々の文字列として返す *)
let show_pair a b = !show_pair_ref a b
''')
rep('''  let fail ?advice msg =
    let origin =
      match via with
      | None -> ""
      | Some (vt, vc) -> "。" ^ show vt ^ " が " ^ display_of vc ^ " であるための前提として要求されました"
    in
    type_error (msg ^ origin ^ match advice with None -> "" | Some a -> "。" ^ a)
  in
''', '''  (* ty は文言に現れる型で、mk はその印字から文言を作る。
     出どころを添えるときは、ty と出どころの型を 1 つの名前の表で印字する *)
  let fail ?advice ?ty mk =
    let origin vt vc = "。" ^ vt ^ " が " ^ display_of vc ^ " であるための前提として要求されました" in
    let shown, suffix =
      match (ty, via) with
      | Some ty, None -> (show ty, "")
      | None, None -> ("", "")
      | Some ty, Some (vt, vc) ->
          let s, o = show_pair ty vt in
          (s, origin o vc)
      | None, Some (vt, vc) -> ("", origin (show vt) vc)
    in
    type_error (mk shown ^ suffix ^ match advice with None -> "" | Some a -> "。" ^ a)
  in
''')
rep('''              fail
                ~advice:("[A: " ^ display_of c ^ "] のように制約を書いてください")
                ("型パラメータ " ^ show (TVar v) ^ " は " ^ display_of c ^ " のインスタンスではありません")
      | _ -> fail (show t ^ " に " ^ display_of c ^ " 制約を付けられません"))''',
'''              fail
                ~advice:("[A: " ^ display_of c ^ "] のように制約を書いてください")
                ~ty:(TVar v)
                (fun s -> "型パラメータ " ^ s ^ " は " ^ display_of c ^ " のインスタンスではありません")
      | _ -> fail ~ty:t (fun s -> s ^ " に " ^ display_of c ^ " 制約を付けられません"))''')
rep('''      | None -> fail (display_of n ^ " は " ^ display_of c ^ " のインスタンスではありません"))''',
'''      | None -> fail (fun _ -> display_of n ^ " は " ^ display_of c ^ " のインスタンスではありません"))''')
rep('''      | _ -> fail ("行変数を含む型 " ^ show t ^ " に " ^ display_of c ^ " の構造的導出は適用できません(行が閉じていません)"))
  | TApp _ -> fail ("制約 " ^ display_of c ^ " を " ^ show t ^ " に付けられません(頭が型変数の適用には制約を貼れません)")
  | other -> fail (show other ^ " は " ^ display_of c ^ " のインスタンスではありません")''',
'''      | _ ->
          fail ~ty:t (fun s ->
              "行変数を含む型 " ^ s ^ " に " ^ display_of c ^ " の構造的導出は適用できません(行が閉じていません)"))
  | TApp _ -> fail ~ty:t (fun s -> "制約 " ^ display_of c ^ " を " ^ s ^ " に付けられません(頭が型変数の適用には制約を貼れません)")
  | other -> fail ~ty:other (fun s -> s ^ " は " ^ display_of c ^ " のインスタンスではありません")''')
open(p, 'w', encoding='utf-8').write(s)

p = sys.argv[1] + '/lib/show.ml'
s = open(p, encoding='utf-8').read()
rep('''  match strs with [ x; y ] -> x ^ " と " ^ y | _ -> bug "show2"

let () =
  Unify.show_ref := show;
  Unify.show2_ref := show2''', '''  match strs with [ x; y ] -> x ^ " と " ^ y | _ -> bug "show2"

let show_pair a b =
  (* ctx は捨てる。2 つの型を別々の文に埋め込むので、show2 と同じく前置しない *)
  let strs, _ = show_all [ a; b ] in
  match strs with [ x; y ] -> (x, y) | _ -> bug "show_pair"

let () =
  Unify.show_ref := show;
  Unify.show2_ref := show2;
  Unify.show_pair_ref := show_pair''')
open(p, 'w', encoding='utf-8').write(s)
