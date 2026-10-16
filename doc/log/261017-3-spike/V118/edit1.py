import sys,re
def sub(path, old, new, count=1):
    s=open(path,encoding='utf-8').read()
    n=s.count(old)
    if n!=count: sys.exit(f"{path}: {n} matches for {old[:60]!r}")
    s=s.replace(old,new)
    open(path,'w',encoding='utf-8').write(s)
P='lib/parser.mly'
# syntax.ml: Data signature gains loc
sub('lib/syntax.ml','''  val allocate : Location.span -> t
end

module EmptyData : Data = struct
  type t = unit

  let allocate _ = ()
end''','''  val allocate : Location.span -> t

  val loc : t -> Location.span
end

module EmptyData : Data = struct
  type t = Location.span

  let allocate loc = loc

  let loc d = d
end''')
sub('lib/tree.ml','''  let allocate loc = { oid = new_oid (); loc; ty_field = None; resolved = None; dict = DNone }
end''','''  let allocate loc = { oid = new_oid (); loc; ty_field = None; resolved = None; dict = DNone }

  let loc d = d.loc
end''')
# helper in parser: start of a node
sub(P,'''let mk (sp, ep) x = (Data.allocate { Location.start = sp; Location.finish = ep }, x)''',
'''let mk (sp, ep) x = (Data.allocate { Location.start = sp; Location.finish = ep }, x)

let start_of (d, _) = (Data.loc d).Location.start

let syntax_error_at pos msg = raise (Syntax_error_at (pos, msg))''')
sub(P,'''let dot_select sloc e id =''','''let dot_select sloc e (id_pos, id) =''')
sub(P,'''      if is_upper id then raise (Syntax_error ("レコードのラベルに大文字識別子は使えません: " ^ id))''',
'''      if is_upper id then syntax_error_at id_pos ("レコードのラベルに大文字識別子は使えません: " ^ id)''')
sub(P,'''        raise (Syntax_error "#Foo(...) にラベル付き引数は使えません")''','''        syntax_error_at (fst sloc) "#Foo(...) にラベル付き引数は使えません"''')
sub(P,'''      raise (Syntax_error "ブロックの中の let には pub を付けられません(with より後ろの宣言は公開できません)")''',
'''      syntax_error_at (start_of b_node) "ブロックの中の let には pub を付けられません(with より後ろの宣言は公開できません)"''')
s=open(P,encoding='utf-8').read()
s=s.replace('''  | (_, DLet (_, b)) :: _ when b.lb_pub ->''','''  | ((_, DLet (_, b)) as b_node) :: _ when b.lb_pub ->''')
s=s.replace('''  | (_, DLetRec bs) :: _ when List.exists (fun (_, b) -> b.lb_pub) bs ->
      raise (Syntax_error "ブロックの中の let rec には pub を付けられません(with より後ろの宣言は公開できません)")''',
'''  | ((_, DLetRec bs) as b_node) :: _ when List.exists (fun (_, b) -> b.lb_pub) bs ->
      syntax_error_at (start_of b_node) "ブロックの中の let rec には pub を付けられません(with より後ろの宣言は公開できません)"''')
s=s.replace('''  | _ :: _ -> raise (Syntax_error "この宣言はブロック内では使えません(let / let rec / 式のみ)")''',
'''  | b_node :: _ -> syntax_error_at (start_of b_node) "この宣言はブロック内では使えません(let / let rec / 式のみ)"''')
s=s.replace('''    | _ -> raise (Syntax_error "with: 呼び出しの引数形が不正です")''','''    | _ -> syntax_error_at (fst sloc) "with: 呼び出しの引数形が不正です"''')
s=s.replace('''  | _ -> raise (Syntax_error "with の右辺は関数呼び出しでなければなりません")''','''  | _ -> syntax_error_at (start_of call) "with の右辺は関数呼び出しでなければなりません"''')
s=s.replace('''        raise (Syntax_error "#Foo(...) パターンにラベル付き引数は使えません")''','''        syntax_error_at (fst sloc) "#Foo(...) パターンにラベル付き引数は使えません"''')
s=s.replace('''let set_pub = function''','''let set_pub pos = function''')
s=s.replace('''  | DInstance _ -> raise (Syntax_error "type instance に pub は付けられません")
  | DExp _ -> raise (Syntax_error "式文に pub は付けられません")''','''  | DInstance _ -> syntax_error_at pos "type instance に pub は付けられません"
  | DExp _ -> syntax_error_at pos "式文に pub は付けられません"''')
s=s.replace('''decl: PUB decl_body { mk $sloc (set_pub $2) }''','''decl: PUB decl_body { mk $sloc (set_pub $startpos $2) }''')
s=s.replace('''  | postfix_exp DOT lower_id            { dot_select $sloc $1 $3 }
  | postfix_exp DOT upper_id            { dot_select $sloc $1 $3 }''','''  | postfix_exp DOT lower_id            { dot_select $sloc $1 ($startpos($3), $3) }
  | postfix_exp DOT upper_id            { dot_select $sloc $1 ($startpos($3), $3) }''')
s=s.replace('''(dot_select $sloc $1 $3)''','''(dot_select $sloc $1 ($startpos($3), $3))''')
s=s.replace('''        | [ (Some _, _) ] -> raise (Syntax_error "resume の引数にラベルは付けられません")
        | _ -> raise (Syntax_error "resume の引数は高々1個です") }''','''        | [ (Some _, _) ] -> syntax_error_at $startpos "resume の引数にラベルは付けられません"
        | _ -> syntax_error_at $startpos "resume の引数は高々1個です" }''')
s=s.replace('''        | _ -> raise (Syntax_error "レコード更新の基底は小文字始まりの識別子 1 個です"));''','''        | _ -> syntax_error_at $startpos($2) "レコード更新の基底は小文字始まりの識別子 1 個です");''')
open(P,'w',encoding='utf-8').write(s)
