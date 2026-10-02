open Diktor
let () =
  Decls.reset ();
  let pre = Driver.embedded_prelude () in
  let files = List.tl (Array.to_list Sys.argv) in
  let decls = Elab.flatten_modules (List.concat_map Driver.parse_file files) in
  let (_, err) = Elab.type_check ~prelude:pre decls in
  (match err with Some e -> print_endline (Driver.render_error e) | None -> ());
  let l name n = Printf.printf "%-22s %d\n" name n in
  let open Decls in
  l "prelude_keys" (Hashtbl.length prelude_keys);
  l "type_namespace" (Hashtbl.length type_namespace);
  l "user_type_redecls" (Hashtbl.length user_type_redecls);
  l "externs" (Hashtbl.length externs);
  l "con_kinds" (Hashtbl.length con_kinds);
  l "reserved_type_names" (Hashtbl.length reserved_type_names);
  l "con_synonyms" (Hashtbl.length con_synonyms);
  l "module_con_synonyms" (Hashtbl.length module_con_synonyms);
  l "module_val_synonyms" (Hashtbl.length module_val_synonyms);
  l "con_hints" (Hashtbl.length con_hints);
  l "val_synonyms" (Hashtbl.length val_synonyms);
  l "value_visibility" (Hashtbl.length value_visibility);
  l "con_visibility" (Hashtbl.length con_visibility);
  l "decl_module" (Hashtbl.length decl_module);
  l "aliases" (Hashtbl.length aliases);
  l "datas" (Hashtbl.length datas);
  l "ctor_owner" (Hashtbl.length ctor_owner);
  l "effects" (Hashtbl.length effects);
  l "op_index" (Hashtbl.length op_index);
  l "classes" (Hashtbl.length classes);
  l "instances" (Hashtbl.length instances);
  l "builtin_redecls" (Hashtbl.length builtin_redecls);
  l "reserved_predicates" (Hashtbl.length reserved_predicates);
  l "builtin_ops(list)" (List.length !builtin_ops);
  let total = ref 0 in
  let snap () =
    let c t = Hashtbl.copy t in
    ignore (c prelude_keys, c type_namespace, c user_type_redecls, c externs, c con_kinds, c reserved_type_names,
            c con_synonyms, c module_con_synonyms, c module_val_synonyms, c con_hints, c val_synonyms);
    ignore (c value_visibility, c con_visibility, c decl_module, c aliases, c datas, c ctor_owner, c effects,
            c op_index, c classes, c instances, c builtin_redecls, c reserved_predicates);
    incr total
  in
  let n = 10000 in
  let t0 = Sys.time () in
  for _ = 1 to n do snap () done;
  let t1 = Sys.time () in
  Printf.printf "snapshot x%d: %.3f s (%.1f us/each)\n" n (t1 -. t0) ((t1 -. t0) /. float n *. 1e6);
  (* interp globals size *)
  let g = Hashtbl.create 512 in
  Interp.register_builtin_values g;
  Interp.register_class_methods g;
  l "globals(builtins+methods)" (Hashtbl.length g);
  let t0 = Sys.time () in
  for _ = 1 to n do ignore (Hashtbl.copy g) done;
  let t1 = Sys.time () in
  Printf.printf "globals copy x%d: %.3f s (%.1f us/each)\n" n (t1 -. t0) ((t1 -. t0) /. float n *. 1e6)
let () =
  Decls.reset ();
  let pre = Driver.embedded_prelude () in
  let _ = Elab.start_session ~prelude:pre () in
  let s = Interp.start_session ~sink:ignore pre in
  Printf.printf "globals after prelude: %d, versions: %d\n" (Hashtbl.length s.Interp.s_env.Value.globals) (List.length !(s.Interp.s_versions))
