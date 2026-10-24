(* keleut-front の LR(1) の表 src/tables.rs を、Diktor の文法から作る。

   使い方(rustkeleut/crates/keleut-front で):

     mkdir -p /tmp/kf
     menhir --cmly --table --explain --strict ../../../diktor/lib/parser.mly --base /tmp/kf/parser
     cp tools/gen_tables.ml /tmp/kf/
     ocamlfind ocamlopt -package menhirSdk -linkpkg /tmp/kf/gen_tables.ml -o /tmp/kf/gen
     /tmp/kf/gen /tmp/kf/parser.cmly ../../../diktor/lib/parser.mly > src/tables.rs
     cargo fmt

   ocamlopt は .cmi、.cmx、.o を元のファイルの隣に書くので、写しを /tmp/kf で翻訳する。
   Diktor の dune と同じ旗で menhir を走らせ、出てきた .cmly(menhirSdk の形式)から、
   状態ごとの動作、goto、既定の還元と、規則の一覧を Rust の定数に書き出す。
   表は menhir の表の形の後段(--table)と同じ自動機械を表すので、
   受理する言語、誤りに気づくトークン、トークンを読む時機が Diktor と一致する。
   規則の意味の動作は src/actions.rs に手で写し、規則の綴り(KEY)で結びつける。 *)

module G = MenhirSdk.Cmly_read.Read (struct
  let filename = Sys.argv.(1)
end)

let mly = if Array.length Sys.argv > 2 then Sys.argv.(2) else ""

let pr = Printf.printf

let sym_name = function G.T t -> G.Terminal.name t | G.N n -> G.Nonterminal.name n

let prod_key p =
  let lhs = G.Nonterminal.name (G.Production.lhs p) in
  let rhs = Array.to_list (Array.map (fun (s, _, _) -> sym_name s) (G.Production.rhs p)) in
  String.concat " " ((lhs ^ ":") :: rhs)

let escape s =
  let b = Buffer.create (String.length s) in
  String.iter (fun c -> match c with '"' -> Buffer.add_string b "\\\"" | '\\' -> Buffer.add_string b "\\\\" | c -> Buffer.add_char b c) s;
  Buffer.contents b

let is_sharp t = G.Terminal.name t = "#"

let () =
  let nt = G.Terminal.count and nn = G.Nonterminal.count and np = G.Production.count and ns = G.Lr1.count in
  let digest = if mly = "" then "" else Digest.to_hex (Digest.file mly) in
  pr "//! Diktor の文法(diktor/lib/parser.mly)の LR(1) の表である。\n";
  pr "//!\n";
  pr "//! tools/gen_tables.ml が menhir の .cmly から作った。手で書き換えない。\n";
  pr "//! 元の parser.mly の MD5 は %s である。\n\n" digest;
  pr "#![allow(clippy::all)]\n\n";
  pr "/// 表を作った元の parser.mly の MD5 である(試験が今の parser.mly と比べる)。\n";
  pr "pub const PARSER_MLY_MD5: &str = \"%s\";\n\n" digest;
  pr "/// 終端記号の数である(`error` と `#` を含む)。\n";
  pr "pub const N_TERMINALS: usize = %d;\n" nt;
  pr "/// 非終端記号の数である。\n";
  pr "pub const N_NONTERMINALS: usize = %d;\n" nn;
  pr "/// 状態の数である。\n";
  pr "pub const N_STATES: usize = %d;\n\n" ns;
  pr "/// 終端記号の名前である(添字は menhir の番号)。\n";
  pr "pub const TERMINALS: [&str; N_TERMINALS] = [\n";
  G.Terminal.iter (fun t -> pr "    \"%s\",\n" (escape (G.Terminal.name t)));
  pr "];\n\n";
  pr "/// 非終端記号の名前である。\n";
  pr "pub const NONTERMINALS: [&str; N_NONTERMINALS] = [\n";
  G.Nonterminal.iter (fun n -> pr "    \"%s\",\n" (escape (G.Nonterminal.name n)));
  pr "];\n\n";
  pr "/// 規則である。\n";
  pr "pub struct Prod {\n";
  pr "    /// 左辺の非終端記号。\n    pub lhs: u16,\n";
  pr "    /// 右辺の長さ。\n    pub len: u8,\n";
  pr "    /// 開始の規則か(還元すると受理する)。\n    pub start: bool,\n";
  pr "    /// `左辺: 右辺の記号…` の綴り。意味の動作を引く鍵である。\n    pub key: &'static str,\n";
  pr "}\n\n";
  pr "/// 規則の一覧である(添字は menhir の番号)。\n";
  pr "pub const PRODS: [Prod; %d] = [\n" np;
  G.Production.iter (fun p ->
      pr "    Prod { lhs: %d, len: %d, start: %b, key: \"%s\" },\n"
        (G.Nonterminal.to_int (G.Production.lhs p))
        (Array.length (G.Production.rhs p))
        (G.Production.kind p = `START)
        (escape (prod_key p)));
  pr "];\n\n";
  (* 動作の表。0 は誤り、1..=0x7fff は (状態 + 1) への shift、0x8000 | p は規則 p の還元 *)
  pr "/// 動作の表(状態 × 終端記号)である。\n";
  pr "/// 0 は誤り、`1..=0x7fff` は値から 1 を引いた状態への shift、`0x8000 | p` は規則 p の還元である。\n";
  pr "pub static ACTION: [u16; N_STATES * N_TERMINALS] = [\n";
  G.Lr1.iter (fun s ->
      let row = Array.make nt 0 in
      List.iter
        (fun (sym, s') -> match sym with G.T t -> row.(G.Terminal.to_int t) <- G.Lr1.to_int s' + 1 | G.N _ -> ())
        (G.Lr1.transitions s);
      List.iter
        (fun (t, p) ->
          let i = G.Terminal.to_int t in
          if row.(i) <> 0 then failwith "conflict";
          row.(i) <- 0x8000 lor G.Production.to_int p)
        (G.Lr1.get_reductions s);
      pr "    ";
      Array.iter (fun v -> pr "%d," v) row;
      pr "\n");
  pr "];\n\n";
  pr "/// goto の表(状態 × 非終端記号)である。`u16::MAX` は無いことを表す。\n";
  pr "pub static GOTO: [u16; N_STATES * N_NONTERMINALS] = [\n";
  G.Lr1.iter (fun s ->
      let row = Array.make nn 0xffff in
      List.iter
        (fun (sym, s') -> match sym with G.N n -> row.(G.Nonterminal.to_int n) <- G.Lr1.to_int s' | G.T _ -> ())
        (G.Lr1.transitions s);
      pr "    ";
      Array.iter (fun v -> pr "%d," v) row;
      pr "\n");
  pr "];\n\n";
  pr "/// 既定の還元(先読みを見ずに還元する規則)である。`u16::MAX` は無いことを表す。\n";
  pr "pub static DEFAULT_REDUCTION: [u16; N_STATES] = [\n    ";
  G.Lr1.iter (fun s ->
      match G.Lr1.default_reduction s with Some p -> pr "%d," (G.Production.to_int p) | None -> pr "65535,");
  pr "\n];\n\n";
  pr "/// shift でこの状態に入ったとき、次のトークンを読まないか(`#` での既定の還元を持つ)である。\n";
  pr "pub static NO_DISCARD: [bool; N_STATES] = [\n    ";
  G.Lr1.iter (fun s ->
      let v =
        G.Lr1.default_reduction s <> None && List.exists (fun (t, _) -> is_sharp t) (G.Lr1.get_reductions s)
      in
      pr "%b," v);
  pr "\n];\n\n";
  (match G.Grammar.entry_points with
  | [ (n, _, s) ] when G.Nonterminal.name n = "program" ->
      pr "/// 開始記号 `program` の初期状態である。\n";
      pr "pub const ENTRY: u16 = %d;\n" (G.Lr1.to_int s)
  | _ -> failwith "entry point");
  ()
