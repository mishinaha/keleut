import sys
p=sys.argv[1]; s=open(p).read()
a='binder_id: lower_id { $1 } | reserved_word { raise (Syntax_error_at ($startpos, "reserved")) }'
assert a in s
s=s.replace(a,'binder_id: lower_id { $1 } | error { raise (Syntax_error "reserved") }')
open(p,'w').write(s)
