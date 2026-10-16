let src = "module M {\n  pub let f(x: Int32): Int32 = x + 1\n}\necholn(show(M.f(41)))\n"
let () =
  let buf = Buffer.create 16 in
  (match Diktor.Driver.eval_string ~sink:(Buffer.add_string buf) src with
   | Ok () -> Printf.printf "eval_string: Ok, out=%S\n" (Buffer.contents buf)
   | Error e -> Printf.printf "eval_string: Error %s\n" e);
  let lines, err = Diktor.Driver.type_check_string src in
  Printf.printf "type_check_string: %s / %s\n" (String.concat "; " lines) (Option.value ~default:"None" err);
  (* 型検査も平坦化も通さずに Interp.run へ直接渡す誤用 *)
  let raw = Diktor.Driver.parse_string ~filename:"<raw>" src in
  (try Diktor.Interp.run ~sink:print_string raw; print_endline "Interp.run raw: ok"
   with ex -> Printf.printf "Interp.run raw: %s\n" (Diktor.Driver.runtime_message ex))
