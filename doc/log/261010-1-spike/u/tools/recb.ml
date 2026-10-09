and elab_rec_bindings ?(top = false) env level eff bs : env =
  (* 事前に割り当てた単相の変数で束縛 → 本体を推論 → unify → 一般化。多相再帰はできない *)
  let lvl = level + 1 in
  (* 型パラメータの剛定数は、署名を先に置くために、群のどの本体よりも前に作る(§11.29) *)
  let tps = List.map (fun ((_, b) as bnode : T.let_binding) -> at_node bnode (fun () -> make_rigids lvl b.T.lb_tparams)) bs in
  let types_with env rigids = List.fold_left (fun m (n, t, _) -> SMap.add n t m) env.types rigids in
  (* 束縛の名前と、先に置く型 pre。注釈で決まる部分(関数束縛の最外の行、値束縛の注釈)は
     本体の前に pre に置き、先行する本体の呼び出しから見えるようにする。sig_eff は関数束縛の
     本体を検査する行で、pre の行と同じものを使う *)
  let names, sigs =
    List.split
      (List.map2
         (fun ((_, b) as bnode : T.let_binding) rigids ->
           at_node bnode @@ fun () ->
           (match (b.T.lb_params, snd b.T.lb_body) with
           | None, T.Lambda _ | Some _, _ -> ()
           | None, _ -> type_error "let rec の右辺は関数でなければなりません");
           match snd b.T.lb_name with
           | T.PVar x -> (
               let env_ty = { env with types = types_with env rigids } in
               let arrow params e = TArrow (TRecord (closed_item_row (List.map (fun _ -> new_var lvl) params)), new_var lvl, e) in
               match (b.T.lb_params, b.T.lb_eff, b.T.lb_ret) with
               | Some params, Some e, _ ->
                   let e = elab_eff env_ty lvl e in
                   ((x, arrow params e), (Some e, None))
               | Some params, None, _ when b.T.lb_pub ->
                   let e = new_pub_pure_row () in
                   ((x, arrow params e), (Some e, None))
               | None, _, Some t ->
                   let ty = elab_value_type_outer env_ty lvl "型注釈" t in
                   let ty =
                     match (snd t, repr ty) with
                     | T.EArrow (_, _, None), TArrow (a, r, _) when b.T.lb_pub -> TArrow (a, r, TRowEmpty)
                     | _ -> ty
                   in
                   ((x, ty), (None, Some ty))
               | _ -> ((x, new_var lvl), (None, None)))
           | _ -> type_error "let rec の束縛はパターンにできません")
         bs tps)
  in
  (* トップレベルの let rec の自己参照と相互参照は、トップレベルの実体を指す *)
  let bind_names env names =
    List.fold_left2
      (fun env bn (x, t) -> if top then add_top env x t (Tree.GDecl (Tree.oid_of bn, x)) else add_local env x t)
      env bs names
  in
  let env_rec = bind_names env names in
  (* 型パラメータの剛定数のうち、群のほかの束縛の pre に入り込んだものは群の終わりに、
     それ以外は束縛ごとに解放する(§11.29) *)
  let group_rigids = ref [] in
  let rec mentions r t =
    match repr t with
    | TVar v -> v == r
    | TCon (_, args) -> List.exists (mentions r) args
    | TApp (f, a) -> mentions r f || mentions r a
    | TArrow (a, ret, e) -> mentions r a || mentions r ret || mentions r e
    | TRecord row | TVariant row -> mentions r row
    | TRowEmpty -> false
    | TRowExtend (_, f, rest) -> mentions r f || mentions r rest
  in
  let rec_arg_queue = ref [] in
  List.iter2
    (fun ((_, b) as bnode) (((_, pre), (sig_eff, sig_vty)), rigids) ->
      at_node bnode @@ fun () ->
      (if b.T.lb_pub then check_pub_annots ~value_head_outer:true ~params:b.T.lb_params ~ret:b.T.lb_ret);
      let env_ty = { env_rec with types = types_with env_rec rigids } in
      let fn_ty =
        match b.T.lb_params with
        | Some params ->
            let seen = ref [] in
            let param_tys = List.map (fun _ -> new_var lvl) params in
            let env2 = List.fold_left2 (fun env p t -> elab_pat env lvl seen t p) env_ty params param_tys in
            let fn_eff = match sig_eff with Some e -> e | None -> new_row_var lvl in
            let ret_ty = match b.T.lb_ret with Some t -> elab_value_type env_ty lvl "返り値の型注釈" t | None -> new_var lvl in
            let body_ty = elab_exp env2 lvl fn_eff b.T.lb_body in
            Unify.unify ret_ty (subsume_closed lvl body_ty ret_ty);
            rec_arg_queue := (params, param_tys) :: !rec_arg_queue;
            TArrow (TRecord (closed_item_row param_tys), ret_ty, fn_eff)
        | None ->
            let vty = match sig_vty with Some t -> t | None -> new_var lvl in
            Unify.unify vty (subsume_closed lvl (elab_exp env_ty lvl eff b.T.lb_body) vty);
            vty
      in
      Unify.unify pre fn_ty;
      Tree.set_ty bnode fn_ty;
      let shared, own =
        List.partition (fun (_, _, r) -> List.exists (fun (_, other) -> other != pre && mentions r other) names) rigids
      in
      release_rigids own;
      group_rigids := shared @ !group_rigids)
    bs
    (List.combine (List.combine names sigs) tps);
  release_rigids !group_rigids;
  List.iter
    (fun (params, param_tys) ->
      List.iter2 (fun p t -> if not (irrefutable_pat p) then Exhaust.queue [ (p, false) ] t) params param_tys)
    (List.rev !rec_arg_queue);
  List.iter warn (Exhaust.drain ());
  Unify.check_ambiguity ~all:false ~level (List.map snd names);
  List.iter (fun (_, t) -> Unify.generalize level t) names;
  bind_names env names

