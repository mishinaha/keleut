import sys
p = sys.argv[1]
s = open(p, encoding='utf-8').read()
old1 = """  let member_marks = ref [] in
  Fun.protect ~finally:(fun () -> group_stack := List.filter (fun g' -> g' != g) !group_stack) @@ fun () ->
  List.iter2
    (fun ((_, b) as bnode) (((_, pre), (sig_eff, sig_vty)), rigids) ->
      at_node bnode @@ fun () ->
"""
new1 = """  let member_marks = ref [] in
  (* 剛定数の衝突に、群の中の参照が単相であることから来る事実を添える(§11.29) *)
  let group_note cur =
    let rigid r = match !r with Rigid _ -> true | _ -> false in
    let flowed =
      List.concat
        (List.map2
           (fun (x, pre) rigids ->
             List.filter_map
               (fun (tp, _, r) ->
                 if not (rigid r) then None
                 else
                   match List.find_opt (fun (_, other) -> other != pre && mentions r other) names with
                   | Some (y, _) -> Some (Printf.sprintf "%s の型パラメータ %s が %s の型に入り込んでいます" x tp y)
                   | None -> None)
               rigids)
           names tps)
    in
    let fixed =
      match List.assq_opt cur (List.combine bs (List.combine names tps)) with
      | Some ((x, _), _ :: _) ->
          let callers =
            List.sort_uniq compare
              (List.filter_map
                 (fun (_, from, tgt, _) ->
                   match from with
                   | Some f when tgt == cur && f != cur -> Some (fst (List.assq f (List.combine bs names)))
                   | _ -> None)
                 g.g_refs)
          in
          List.map (fun y -> Printf.sprintf "型パラメータを持つ %s を、前にある %s の本体が参照しています" x y) callers
      | _ -> []
    in
    match flowed @ fixed with
    | [] -> None
    | facts ->
        Some
          ("(再帰群の中で、前にある束縛から後ろにある束縛への参照は単相です。" ^ String.concat "。" facts
         ^ "。前にある束縛の本体が後ろの束縛の型パラメータを型に決める形は、型検査に通りません。§6.3)")
  in
  let has_sub sub s =
    let n = String.length sub and m = String.length s in
    let rec go i = i + n <= m && (String.sub s i n = sub || go (i + 1)) in
    go 0
  in
  let clash msg = has_sub "スコープ付きの型が一致しません" msg && not (has_sub "再帰群の中で、前にある束縛" msg) in
  let at_group_ref (loc : Location.span) =
    List.exists (fun (node, _, _, _) -> (Tree.loc_of node).Location.start.Lexing.pos_cnum = loc.Location.start.Lexing.pos_cnum) g.g_refs
  in
  Fun.protect ~finally:(fun () -> group_stack := List.filter (fun g' -> g' != g) !group_stack) @@ fun () ->
  List.iter2
    (fun ((_, b) as bnode) (((_, pre), (sig_eff, sig_vty)), rigids) ->
      (try
      at_node bnode @@ fun () ->
"""
assert s.count(old1) == 1
s = s.replace(old1, new1)
old2 = """      Unify.unify pre fn_ty;
      Tree.set_ty bnode fn_ty;
"""
new2 = """      (try Unify.unify pre fn_ty
       with Type_error msg when clash msg -> (
         match group_note bnode with Some note -> type_error (msg ^ note) | None -> type_error msg));
      Tree.set_ty bnode fn_ty;
"""
assert s.count(old2) == 1
s = s.replace(old2, new2)
old3 = """      release_rigids own;
      group_rigids := shared @ !group_rigids)
    bs
"""
new3 = """      release_rigids own;
      group_rigids := shared @ !group_rigids
      with Type_error_at (loc, msg) when clash msg && at_group_ref loc -> (
        match group_note bnode with Some note -> raise (Type_error_at (loc, msg ^ note)) | None -> raise (Type_error_at (loc, msg))))
    bs
"""
assert s.count(old3) == 1
s = s.replace(old3, new3)
open(p, 'w', encoding='utf-8').write(s)
