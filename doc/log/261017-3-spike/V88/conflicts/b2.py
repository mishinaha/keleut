import sys,re
src, dst, sites = sys.argv[1], sys.argv[2], sys.argv[3].split(',')
s=open(src).read()
kw='''  | AND { "and" } | CASE { "case" } | CLASS { "class" } | EFFECT { "effect" } | ELSE { "else" }
  | EXTENDS { "extends" } | EXTERN { "extern" } | FN { "fn" } | FROM { "from" } | HANDLE { "handle" }
  | IF { "if" } | IMPORT { "import" } | INSTANCE { "instance" } | LET { "let" } | MATCH { "match" }
  | MODULE { "module" } | NEWTYPE { "newtype" } | PERFORM { "perform" } | PUB { "pub" } | REC { "rec" }
  | RESUME { "resume" } | RUN { "run" } | THEN { "then" } | TYPE { "type" } | VAL { "val" } | WITH { "with" }
  | BOOL { string_of_bool $1 }'''
s=s.replace("lower_id: LOWER_IDENTIFIER { $1 }\n","lower_id: LOWER_IDENTIFIER { $1 }\nbinder_id: lower_id { $1 } | reserved_word { raise (Syntax_error_at ($startpos, \"reserved\")) }\nreserved_word:\n"+kw+"\n",1)
rep={
 'binding': ("  | lower_id typarams_opt LPAREN params RPAREN sig_tail EQ exp","  | binder_id typarams_opt LPAREN params RPAREN sig_tail EQ exp"),
 'bindpat': ("  | lower_id                           { mk $sloc (PVar $1) }","  | binder_id                          { mk $sloc (PVar $1) }"),
 'val': ("  | VAL lower_id typarams_opt","  | VAL binder_id typarams_opt"),
 'extern': ("extern_sig: lower_id","extern_sig: binder_id"),
 'effop': ("eff_op: lower_id COLON ty","eff_op: binder_id COLON ty"),
 'field': ("  | lower_id EQ exp { ($1, $3) }","  | binder_id EQ exp { ($1, $3) }"),
 'patfield': ("  | lower_id EQ pat { ($1, $3) }","  | binder_id EQ pat { ($1, $3) }"),
 'pat': ("  | LOWLINE                           { mk $sloc PWildcard }\n  | NUMBER","  | LOWLINE                           { mk $sloc PWildcard }\n  | reserved_word { raise (Syntax_error_at ($startpos, \"reserved\")) }\n  | NUMBER"),
 'patarg': ("pat_arg: lower_id EQ pat","pat_arg: binder_id EQ pat"),
 'arg': ("arg: lower_id EQ exp","arg: binder_id EQ exp"),
 'braceitem': ("  | lower_id COLON ty { BField","  | binder_id COLON ty { BField"),
 'ctorfield': ("| lower_id COLON ty { { fd_label","| binder_id COLON ty { { fd_label"),
 'dot': ("  | postfix_exp DOT lower_id            {","  | postfix_exp DOT binder_id           {"),
 'lambda': ("  | LBRACE_BLOCK lower_id EQ_GREATER items RBRACE","  | LBRACE_BLOCK binder_id EQ_GREATER items RBRACE"),
 'typaram': ("ty_ident: lower_id { $1 }","ty_ident: binder_id { $1 }"),
}
for site in sites:
    if not site: continue
    a,b=rep[site]
    assert s.count(a)==1,(site,s.count(a))
    s=s.replace(a,b)
open(dst,'w').write(s)
