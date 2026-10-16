import sys
p=sys.argv[1]
s=open(p,encoding='utf-8').read()
def rep(a,b):
    global s
    assert s.count(a)==1, a
    s=s.replace(a,b)
rep("""    mutable before_ep : Lexing.position; (* last_sp のトークンの 1 つ前のトークンの終了位置 *)
    mutable before_nl : bool; (* 1 つ前のトークンが区切りに昇格した NL か *)
""","""    mutable before_ep : Lexing.position; (* last_sp のトークンの 1 つ前のトークンの終了位置 *)
    mutable before_cont : bool; (* 1 つ前のトークンの後ろの改行を、続きとして捨てる状態だったか *)
""")
rep("""      before_nl = false;
""","""      before_cont = false;
""")
rep("""    t.before_nl <- t.prev = Some NL;
""","""    t.before_cont <-
      (match t.regions with (RSuppress | RClause _) :: _ -> true | _ -> false)
      || (match t.prev with Some NL | None -> false | Some p -> not (can_end_statement p));
""")
rep("""  (* --dump-tokens 用。ASI を適用した後のトークン列を EOF まで集める *)
  (* 文法に合わなかったトークンの報告位置。そのトークンが 1 つ前のトークンより後の行で
     始まるなら、1 つ前のトークンの終わりを主な位置にし、合わなかったトークンの位置を添える *)
  let error_positions t =
    if (not t.before_nl) && t.before_ep != Lexing.dummy_pos && t.last_sp.Lexing.pos_lnum > t.before_ep.Lexing.pos_lnum
    then (t.before_ep, Some t.last_sp)
    else (t.last_sp, None)

""","""  (* 文法に合わなかったトークンの報告位置。1 つ前のトークンが続きを求めていて、
     合わなかったトークンがそれより後の行で始まるなら、1 つ前のトークンの終わりを主な位置にし、
     合わなかったトークンの位置を添える *)
  let error_positions t =
    if t.before_cont && t.last_sp.Lexing.pos_lnum > t.before_ep.Lexing.pos_lnum then (t.before_ep, Some t.last_sp)
    else (t.last_sp, None)

  (* --dump-tokens 用。ASI を適用した後のトークン列を EOF まで集める *)
""")
open(p,'w',encoding='utf-8').write(s)
