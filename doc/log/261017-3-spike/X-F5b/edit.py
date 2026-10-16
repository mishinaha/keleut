import sys
p = sys.argv[1] + '/lib/lexer.ml'
s = open(p, encoding='utf-8').read()
def rep(old, new):
    global s
    assert s.count(old) == 1, old
    s = s.replace(old, new)
rep("""   - `last_sp` / `last_ep`：直前に渡したトークンの開始位置と終了位置。
     第16章(driver.ml)が `last_sp` を構文エラーの報告に使う。
     `last_ep` はどこからも読まれていない
""", """   - `last_sp`：直前に渡したトークンの開始位置。
     第16章(driver.ml)が構文エラーの報告に使う。
   - `recent`：直前に渡したトークンから遡って高々 3 個(新しいものが先頭)。
     §2.13 が、パースエラーの直前で数値が別のトークンに割れたかを調べるのに使う
""")
rep("""    mutable last_sp : Lexing.position; (* 直前に消費者へ渡したトークンの位置(エラー報告用) *)
    mutable last_ep : Lexing.position;
""", """    mutable last_sp : Lexing.position; (* 直前に消費者へ渡したトークンの位置(エラー報告用) *)
    mutable recent : entry list; (* 直前に消費者へ渡したトークンから遡って高々 3 個(§2.13) *)
""")
rep("""      last_sp = Lexing.dummy_pos;
      last_ep = Lexing.dummy_pos;
""", """      last_sp = Lexing.dummy_pos;
      recent = [];
""")
rep("""   `read` がその関数で、あわせて直前に渡したトークンの開始位置と終了位置を状態に控える
   (そのうち開始位置を構文エラーの報告に使うのは第16章(driver.ml)である)。
""", """   `read` がその関数で、あわせて直前に渡したトークンの開始位置と、直近の 3 個のトークンを状態に控える
   (開始位置を構文エラーの報告に使うのは第16章(driver.ml)である)。
""")
rep("""    t.last_sp <- e.sp;
    t.last_ep <- e.ep;
""", """    t.last_sp <- e.sp;
    t.recent <- (match t.recent with a :: b :: _ -> [ e; a; b ] | l -> e :: l);
""")
rep("""    | WITH -> "with"
end
""", """    | WITH -> "with"

(* ## 2.13 数値の綴り間違いが割ったトークン

   §2.2 の規則は最も長く一致する綴りで切るので、数値の綴り間違いは字句エラーにならない。
   `0b102` は `0b10` と `2` に、`1x` と `0x` は数値と識別子に、`1.2.3` は `1.2` と `.` と `3` に切れる。
   どれも続きのトークンで文法に合わなくなり、パースエラーになる。
   そのとき menhir が詰まるのは続きのトークンなので、`last_sp` は数値の後ろの位置を指し、
   利用者は数値を書き間違えたことを読み取りにくい。

   `glued_number` は、パースエラーのときに直近のトークンを見て、この形を見分ける。
   見分けるのは次の 2 つの形で、どちらも数値とその直後のトークンの間に隙間が無い。

   - 詰まったトークンの直前が数値で、詰まったトークンが英数字か `_` か `.` で始まる
   - 詰まったトークンの 2 つ前が数値で、1 つ前が `.` である(`1.2.3` の `3` で詰まる形)

   英数字で始まるかどうかは `show_token` の表記の先頭の文字で判定する。
   予約語と識別子と数値は綴りのまま表示し、`<NL>` や文字列は記号で始まるからである。
   返すのは、数値、その開始位置、数値の直後のトークンの 3 つである。
   この判定は診断の文面と位置を変えるだけで、字句の切り方も受理するプログラムも変えない。
   `1else` のように数値に予約語が隙間なく続いても文法に合う入力は、従来どおり受理する。 *)

  let glued_number t =
    let touches a b = a.ep.Lexing.pos_cnum = b.sp.Lexing.pos_cnum in
    let wordlike tok =
      let s = show_token tok in
      s <> ""
      && match s.[0] with 'a' .. 'z' | 'A' .. 'Z' | '0' .. '9' | '_' | '.' -> true | _ -> false
    in
    match t.recent with
    | ({ tok = NUMBER n; _ } as p) :: _ :: _ when false && touches p p -> Some (n, p.sp, p.tok)
    | e :: ({ tok = NUMBER n; _ } as p) :: _ when touches p e && wordlike e.tok -> Some (n, p.sp, e.tok)
    | e :: ({ tok = DOT; _ } as d) :: ({ tok = NUMBER n; _ } as p) :: _
      when touches p d && touches d e && wordlike e.tok ->
        Some (n, p.sp, DOT)
    | _ -> None
end
""")
open(p, 'w', encoding='utf-8').write(s)

p = sys.argv[1] + '/lib/driver.ml'
s = open(p, encoding='utf-8').read()
rep("""   `Syntax_error_at` には指定された位置を、ほかの 2 つには最後に読んだトークンの位置を使う。
""", """   `Syntax_error_at` には指定された位置を、ほかの 2 つには最後に読んだトークンの位置を使う。
   ただし `Parser'.Error` の直前で数値が隙間なく別のトークンに割れていたときは(第2章 §2.13)、
   `parse_error_message` が数値の開始位置を指し、数値とその直後のトークンを名指す。
""")
rep("""let parse_unit_with lexer =
""", """let parse_error_message lx =
  match Lexer'.glued_number lx with
  | Some (n, sp, next) ->
      Printf.sprintf "%s: パースエラー: 数値リテラル %s の直後に %s が続いています(数値リテラルの綴りを確認してください)"
        (show_pos sp) (Lexer.show_number n) (Lexer'.show_token next)
  | None -> Printf.sprintf "%s: パースエラー(付近のトークンを確認してください)" (show_pos lx.Lexer'.last_sp)

let parse_unit_with lexer =
""")
rep("""  | Parser'.Error ->
      raise (Parse_error (Printf.sprintf "%s: パースエラー(付近のトークンを確認してください)" (show_pos lexer.Lexer'.last_sp)))
""", """  | Parser'.Error -> raise (Parse_error (parse_error_message lexer))
""")
rep("""      else Failed (Printf.sprintf "%s: パースエラー(付近のトークンを確認してください)" (show_pos lx.Lexer'.last_sp))
""", """      else Failed (parse_error_message lx)
""")
open(p, 'w', encoding='utf-8').write(s)
