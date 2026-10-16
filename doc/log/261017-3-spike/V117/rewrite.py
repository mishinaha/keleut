import sys,re
p=sys.argv[1]
s=open(p,encoding='utf-8').read()
start=s.index("(* 注釈の中の矢印のうち、@ を省略したものを探す。")
end=s.index("let pub_scope tparams =")
new='''(* 注釈の中の矢印のうち、@ を省略したものを探す。pub の完全注釈検査
   (check_pub_annots と、§11.31 の pub newtype のフィールド)が使う。
   パス 1c の前方参照の署名は、頭の矢印だけを見る別の判定を使う(§11.36)。

   エイリアスは透過なので、注釈に書いたエイリアスの本体の矢印も、注釈の中の矢印として数える。
   数えなければ、矢印をエイリアスに包むだけで要求を逃れられる。
   仕様 §9 も、型エイリアスを展開した矢印を入れ子の矢印に数える(sample.kel:487、:500)。
   本体は宣言の位置の名前で読む。
   そこで、本体を見る間は §11.5 の expand_alias と同じく単位と module を立て直し、
   エイリアスのパラメータを、同じ名前のエイリアスを隠す名前として扱う。
   型引数は、使う側の位置で、注釈に直接書いた型として調べる。
   本体でパラメータを使うかどうかに関わらず調べるので、展開で消える型引数の矢印も数える。
   束縛の型パラメータと、外側で束縛した型パラメータ(tparams)は、§11.3 の優先順位のとおり、
   同じ名前のエイリアスを隠す。
   EffectRow エイリアスの本体は見ない。
   エフェクト行の中は、注釈に直接書いたものも見ないからである。

   見つからなければ None、注釈に直接書いた矢印なら Some None、
   エイリアスの本体の矢印なら、その矢印を本体に書いたエイリアスの名前を添えて Some (Some 名前) を返す。
   エイリアスの再帰、引数の個数が合わない使用、使う位置から見えないエイリアスは、本体を見ずに打ち切る。
   その診断は、後で注釈を精緻化するときに §11.3 と §11.5 が出す。 *)
type pub_scope = {
  ps_module : string option;
  ps_unit : Decls.unit_id;
  ps_tparams : string list;
  ps_alias : string option;
}

let rec unwritten_effect ~expanding sc ((_, te) : T.type_exp) =
  let nested t = unwritten_effect ~expanding sc t in
  let first f l = List.find_map f l in
  let lookup_alias name =
    let saved = !Decls.current_module and saved_unit = !Decls.current_unit in
    Decls.current_module := sc.ps_module;
    Decls.current_unit := sc.ps_unit;
    Fun.protect
      ~finally:(fun () ->
        Decls.current_module := saved;
        Decls.current_unit := saved_unit)
      (fun () ->
        let oid = Decls.resolve_con (intern name) in
        match (Hashtbl.find_opt Decls.aliases oid, Hashtbl.find_opt Decls.con_visibility oid) with
        | _, Some v when not (Decls.visible_here v) -> None
        | Some info, _ -> Some (oid, info)
        | None, _ -> None)
  in
  let alias_body li nargs =
    match li with
    | LongId [ n ] when List.mem n sc.ps_tparams -> None
    | _ -> (
        match lookup_alias (show_long_id li) with
        | None -> None
        | Some (_, info) when info.Decls.al_kind = Some "EffectRow" -> None
        | Some (oid, _) when List.mem oid expanding -> None
        | Some (_, info) when nargs <> List.length info.Decls.al_params -> None
        | Some (oid, info) ->
            let sc' =
              {
                ps_module = info.Decls.al_module;
                ps_unit = info.Decls.al_unit;
                ps_tparams = List.map (fun (tp : type_param) -> tp.tp_name) info.Decls.al_params;
                ps_alias = Some (display_of oid);
              }
            in
            unwritten_effect ~expanding:(oid :: expanding) sc' info.Decls.al_body)
  in
  match te with
  | T.EArrow (params, ret, eff) -> if eff = None then Some sc.ps_alias else first nested (params @ [ ret ])
  | T.EIdent li -> alias_body li 0
  | T.EApply ((_, T.EIdent li), args) -> (
      match first nested args with Some _ as r -> r | None -> alias_body li (List.length args))
  | T.EApply (f, args) -> first nested (f :: args)
  | T.EBraceRow (elems, ext) -> (
      match
        first (function T.BField (_, t) -> nested t | T.BLabel (_, ts) -> first nested ts) elems
      with
      | Some _ as r -> r
      | None -> Option.bind ext nested)
  | T.EVariantCase (_, Some t) -> nested t
  | T.EUnion ts -> first nested ts
  | T.EVariantCase (_, None) | T.EHole -> None

'''
s=s[:start]+new+s[end:]
s=s.replace("""let pub_scope tparams =
  { ps_module = !Decls.current_module; ps_unit = !Decls.current_unit; ps_tparams = tparams; ps_args = []; ps_alias = None }""","""let pub_scope tparams =
  { ps_module = !Decls.current_module; ps_unit = !Decls.current_unit; ps_tparams = tparams; ps_alias = None }""")
old_art="""   値束縛の注釈は head を真にして `unwritten_effect` に渡し、頭の `@` の有無は問わない。
   関数束縛の `lb_ret` は返り値の型、つまり入れ子の位置なので、すべての矢印に `@` が要る。"""
new_art="""   値束縛の注釈の頭が矢印なら、引数と返り値だけを `unwritten_effect` に渡し、頭の `@` の有無は問わない。
   頭が矢印でない注釈(型エイリアスなど)には最外の矢印が無いので、そのまま渡す。
   エイリアスを展開して頭に現れた矢印は、最外ではなく入れ子の矢印である(sample.kel:487)。
   関数束縛の `lb_ret` は返り値の型、つまり入れ子の位置なので、すべての矢印に `@` が要る。"""
assert old_art in s
s=s.replace(old_art,new_art)
old_tail="""  | Some te -> pub_effect_error what (unwritten_effect ~head:(params = None) ~expanding:[] sc te)"""
new_tail="""  | Some te -> (
      let check t = pub_effect_error what (unwritten_effect ~expanding:[] sc t) in
      match (params, te) with
      | None, (_, T.EArrow (ps, r, _)) -> List.iter check (ps @ [ r ])
      | _ -> check te)"""
assert old_tail in s
s=s.replace(old_tail,new_tail)
s=s.replace("unwritten_effect ~head:false ~expanding:[]","unwritten_effect ~expanding:[]")
# tparams at call sites
s=s.replace("""  (if b.T.lb_pub then
    check_pub_annots ~tparams:(List.map (fun (tp : type_param) -> tp.tp_name) b.T.lb_tparams) ~params:b.T.lb_params ~ret:b.T.lb_ret);
  let is_fun""","""  (if b.T.lb_pub then
    check_pub_annots ~tparams:(pub_tparams env b.T.lb_tparams) ~params:b.T.lb_params ~ret:b.T.lb_ret);
  let is_fun""")
s=s.replace("""      (if b.T.lb_pub then
    check_pub_annots ~tparams:(List.map (fun (tp : type_param) -> tp.tp_name) b.T.lb_tparams) ~params:b.T.lb_params ~ret:b.T.lb_ret);""","""      (if b.T.lb_pub then check_pub_annots ~tparams:(pub_tparams env_rec b.T.lb_tparams) ~params:b.T.lb_params ~ret:b.T.lb_ret);""")
assert "List.map (fun (tp : type_param) -> tp.tp_name) b.T.lb_tparams" not in s
open(p,'w',encoding='utf-8').write(s)
