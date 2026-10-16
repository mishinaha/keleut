import sys
def sub(path, old, new):
    s=open(path,encoding='utf-8').read()
    n=s.count(old)
    if n!=1: sys.exit(f"{path}: {n} matches for {old[:70]!r}")
    open(path,'w',encoding='utf-8').write(s.replace(old,new))
L='lib/lexer.ml'
sub(L,'''    mutable last_ep : Lexing.position;
  }''','''    mutable last_ep : Lexing.position;
    mutable before_ep : Lexing.position; (* last_sp のトークンの 1 つ前のトークンの終了位置 *)
    mutable before_nl : bool; (* 1 つ前のトークンが区切りに昇格した NL か *)
  }''')
sub(L,'''      last_ep = Lexing.dummy_pos;
    }''','''      last_ep = Lexing.dummy_pos;
      before_ep = Lexing.dummy_pos;
      before_nl = false;
    }''')
sub(L,'''  let read t =
    let e = read_token t in
    t.last_sp <- e.sp;''','''  let read t =
    t.before_ep <- t.last_ep;
    t.before_nl <- t.prev = Some NL;
    let e = read_token t in
    t.last_sp <- e.sp;''')
sub(L,'''  let all_tokens t =''','''  (* 文法に合わなかったトークンの報告位置。そのトークンが 1 つ前のトークンより後の行で
     始まるなら、1 つ前のトークンの終わりを主な位置にし、合わなかったトークンの位置を添える *)
  let error_positions t =
    if (not t.before_nl) && t.before_ep != Lexing.dummy_pos && t.last_sp.Lexing.pos_lnum > t.before_ep.Lexing.pos_lnum
    then (t.before_ep, Some t.last_sp)
    else (t.last_sp, None)

  let all_tokens t =''')
D='lib/driver.ml'
sub(D,'''  | Parser'.Error ->
      raise (Parse_error (Printf.sprintf "%s: パースエラー(付近のトークンを確認してください)" (show_pos lexer.Lexer'.last_sp)))
  | Syntax.Syntax_error msg ->
      raise (Parse_error (Printf.sprintf "%s: 構文エラー: %s" (show_pos lexer.Lexer'.last_sp) msg))
''','''  | Parser'.Error -> raise (Parse_error (parse_error_message lexer))
''')
sub(D,'''let parse_unit_with lexer =''','''let parse_error_message lx =
  match Lexer'.error_positions lx with
  | pos, None -> Printf.sprintf "%s: パースエラー(付近のトークンを確認してください)" (show_pos pos)
  | pos, Some next ->
      Printf.sprintf "%s: パースエラー(この位置の後ろ、%d:%d のトークンが文法に合いません)" (show_pos pos)
        next.Lexing.pos_lnum
        (next.Lexing.pos_cnum - next.Lexing.pos_bol + 1)

let parse_unit_with lexer =''')
sub(D,'''      else Failed (Printf.sprintf "%s: パースエラー(付近のトークンを確認してください)" (show_pos lx.Lexer'.last_sp))''','''      else Failed (parse_error_message lx)''')
sub(D,'''  | exception Syntax.Syntax_error msg -> Failed (Printf.sprintf "%s: 構文エラー: %s" (show_pos lx.Lexer'.last_sp) msg)
''','')
sub('lib/syntax.ml','''exception Syntax_error of string

''','')
