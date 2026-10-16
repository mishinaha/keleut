(* 注釈の中の矢印のうち、@ を省略したものを探す。pub の完全注釈検査
   (check_pub_annots と、§11.31 の pub newtype のフィールド)が使う。
   パス 1c の前方参照の署名は、頭の矢印だけを見る別の判定を使う(§11.36)。

   エイリアスは透過なので、注釈に書いたエイリアスの本体の矢印も、注釈の中の矢印として数える。
   数えなければ、矢印をエイリアスに包むだけで要求を逃れられ、
   型式をそれと同じ型のエイリアスに置き換えると受理が変わる(仕様 §18.4)。
   本体は宣言の位置の名前で読み、型引数は使う側の位置の名前で読む。
   そこで、本体を見る間は §11.5 の expand_alias と同じく単位と module を立て直し、
   パラメータの名前には、型引数の式と、それを読む位置の組を対応させる。
   束縛の型パラメータ(tparams)は、§11.3 の優先順位のとおり、同じ名前のエイリアスを隠す。
   EffectRow エイリアスの本体は見ない。
   エフェクト行の中の型引数は、注釈に直接書いたものも見ないからである。

   head が真のとき、型式の先頭の矢印は値束縛の最外の矢印で、@ の省略を問わない。
   エイリアスを展開して先頭に現れた矢印も、同じく問わない。
   pub の値束縛では、最外の省略も入れ子の省略も @ {} と読むので、型は変わらない。
   展開した矢印だけを問うと、型を変えないエイリアスへの置き換えで受理が変わる。

   見つからなければ None、注釈に直接書いた矢印なら Some None、
   エイリアスの本体の矢印なら、注釈に書いたエイリアスの名前を添えて Some (Some 名前) を返す。
   エイリアスの再帰は、ここでは打ち切るだけにする。
   再帰の診断は、後で注釈を精緻化するときに §11.5 が出す。 *)
type pub_scope = {
  ps_module : string option;
  ps_unit : Decls.unit_id;
  ps_tparams : string list;
  ps_args : (string * (T.type_exp * pub_scope)) list;
}

let rec unwritten_effect ~head ~expanding sc ((_, te) : T.type_exp) =
  let nested t = unwritten_effect ~head:false ~expanding sc t in
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
        match Hashtbl.find_opt Decls.aliases oid with Some info -> Some (oid, info) | None -> None)
  in
  let through_alias name args =
    match lookup_alias name with
    | None -> None
    | Some (_, info) when info.Decls.al_kind = Some "EffectRow" -> Some None
    | Some (oid, _) when List.mem oid expanding -> Some None
    | Some (oid, info) ->
        if List.length args <> List.length info.Decls.al_params then Some None
        else
          let sc' =
            {
              ps_module = info.Decls.al_module;
              ps_unit = info.Decls.al_unit;
              ps_tparams = [];
              ps_args = List.map2 (fun (tp : type_param) a -> (tp.tp_name, (a, sc))) info.Decls.al_params args;
            }
          in
          let found = unwritten_effect ~head ~expanding:(oid :: expanding) sc' info.Decls.al_body in
          Some (Option.map (function None -> Some (display_of oid) | Some n -> Some n) found)
  in
  let named li args =
    match li with
    | LongId [ n ] when List.mem_assoc n sc.ps_args ->
        let a, asc = List.assoc n sc.ps_args in
        if args = [] then Some (unwritten_effect ~head ~expanding asc a)
        else Some (first nested (a :: args) |> fun r -> match r with Some _ -> r | None -> None)
    | LongId [ n ] when List.mem n sc.ps_tparams -> None
    | _ -> through_alias (show_long_id li) args
  in
  match te with
  | T.EArrow (params, ret, eff) ->
      if (not head) && eff = None then Some None else first nested (params @ [ ret ])
  | T.EIdent li -> ( match named li [] with Some r -> r | None -> None)
  | T.EApply (((_, T.EIdent li) as f), args) -> (
      match named li args with
      | Some r -> r
      | None -> first nested (f :: args))
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

let pub_scope tparams =
  { ps_module = !Decls.current_module; ps_unit = !Decls.current_unit; ps_tparams = tparams; ps_args = [] }

(* pub の完全注釈検査の診断。エイリアスの本体の矢印なら、エイリアスの名前を添える *)
let pub_effect_error what = function
  | None -> ()
  | Some None -> type_error (what ^ "には完全な型注釈が必要です(注釈の中の矢印に @ がありません)")
  | Some (Some a) -> type_error (what ^ "には完全な型注釈が必要です(型エイリアス " ^ a ^ " の本体の矢印に @ がありません)")

(* pub の完全注釈検査。引数と返り値に注釈があるだけでなく、注釈の中の矢印にも @ が要る。
   入れ子の矢印の省略 @ は @ {} と読むので、この規則は健全性のためのものではない。
   仕様 §13(sample.kel:825-831)が可読性のために定めた規則で、
   公開 API では、純粋を意図したのか書き忘れたのかを読み手が区別できなければならない。
   pub newtype のフィールド(§11.31)にも同じ規則が掛かる(sample.kel:828-829)。

   値束縛(引数リストを持たない `let`)の注釈の頭の矢印は、この「中の矢印」に入らない。
   頭は束縛の最外だからである。
   仕様 §9 は、「最外の矢印」の指す先を関数束縛と値束縛で書き分けている(sample.kel:486-487)。
   仕様 §13 は入れ子の矢印の `@` も省略できないと定め(sample.kel:825)、
   最外の省略には別の意味を与えている(sample.kel:830-831)。
   推論に任せず `@ {}` と読み、本体に純粋を要求するという意味である。
   値束縛の頭の矢印も同じ扱いであることは、仕様 §9 の表が定めている(:494)。

   値束縛の注釈は head を真にして `unwritten_effect` に渡し、頭の `@` の有無は問わない。
   関数束縛の `lb_ret` は返り値の型、つまり入れ子の位置なので、すべての矢印に `@` が要る。

   頭を数えずに済むのは、呼び出し側が頭の省略に意味を与える分岐を持つからである。
   `let` の値束縛(§11.28)も `let rec` の値束縛(§11.29)も、`pub` で頭の `@` を省略すれば
   `@ {}` と読み、本体が純粋であることを確かめる。 *)
let check_pub_annots ~tparams ~params ~ret =
  let sc = pub_scope tparams in
  let what = "pub な宣言" in
  (match params with
  | Some ps ->
      List.iter
        (fun ((_, p) : T.pat) ->
          match p with
          | T.PAnnot (_, te) -> pub_effect_error what (unwritten_effect ~head:false ~expanding:[] sc te)
          | _ -> type_error "pub な宣言には完全な型注釈が必要です(引数に型注釈がありません)")
        ps
  | None -> ());
  match ret with
  | None -> type_error "pub な宣言には完全な型注釈が必要です(返り値の型注釈がありません)"
  | Some te -> pub_effect_error what (unwritten_effect ~head:(params = None) ~expanding:[] sc te)
