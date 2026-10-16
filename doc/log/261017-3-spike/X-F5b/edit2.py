import sys
D=sys.argv[1]
p=D+'/lib/lexer.ml'; s=open(p).read()
def rep(a,b):
    global s
    assert s.count(a)==1,a
    s=s.replace(a,b)
rep("""   - `last_sp`：直前に渡したトークンの開始位置。
     第16章(driver.ml)が構文エラーの報告に使う。
   - `recent`""","""   - `last_sp` / `last_ep`：直前に渡したトークンの開始位置と終了位置。
     第16章(driver.ml)が `last_sp` を構文エラーの報告に使う。
     `last_ep` はどこからも読まれていない
   - `recent`""")
rep("""    mutable recent : entry list;""","""    mutable last_ep : Lexing.position;
    mutable recent : entry list;""")
rep("""      recent = [];""","""      last_ep = Lexing.dummy_pos;
      recent = [];""")
rep("""   `read` がその関数で、あわせて直前に渡したトークンの開始位置と、
   直近の 3 個のトークンを状態に控える""","""   `read` がその関数で、あわせて直前に渡したトークンの開始位置と終了位置と、
   直近の 3 個のトークンを状態に控える""")
rep("""    t.last_sp <- e.sp;
    t.recent""","""    t.last_sp <- e.sp;
    t.last_ep <- e.ep;
    t.recent""")
rep("""   - 詰まったトークンの 2 つ前が数値で、1 つ前が `.` である(`1.2.3` の `3` で詰まる形)
""","""   - 詰まったトークンが数値で、1 つ前が `.`、2 つ前が数値である(`1.2.3` の `3` で詰まる形)

   2 つ目の形で詰まったトークンを数値に限るのは、浮動小数の後ろの `.` が、
   `1.2.foo` のようにフィールド選択として文法に合うからである。
   `.` の後ろで詰まるのは数値か予約語のときで、`1.2.match` のような予約語の形は数値の綴りの誤りではない。
   2 つ目の形では、`.` とその後ろの数値をつなげて、数値の直後に続く綴りとして返す(`1.2.3` なら `.3`)。
""")
rep("""   返すのは、数値、その開始位置、数値の直後のトークンの 3 つである。""","""   返すのは、数値、その開始位置、数値の直後に続く綴りの 3 つである。""")
rep("""    | e :: ({ tok = NUMBER n; _ } as p) :: _ when touches p e && wordlike e.tok -> Some (n, p.sp, e.tok)
    | e :: ({ tok = DOT; _ } as d) :: ({ tok = NUMBER n; _ } as p) :: _
      when touches p d && touches d e && wordlike e.tok ->
        Some (n, p.sp, DOT)""","""    | ({ tok = NUMBER m; _ } as e) :: ({ tok = DOT; _ } as d) :: ({ tok = NUMBER n; _ } as p) :: _
      when touches p d && touches d e ->
        Some (n, p.sp, "." ^ show_number m)
    | e :: ({ tok = NUMBER n; _ } as p) :: _ when touches p e && wordlike e.tok -> Some (n, p.sp, show_token e.tok)""")
open(p,'w').write(s)
p=D+'/lib/driver.ml'; s=open(p).read()
rep("""        (show_pos sp) (Lexer.show_number n) (Lexer'.show_token next)""","""        (show_pos sp) (Lexer.show_number n) next""")
rep("""   `parse_error_message` が数値の開始位置を指し、数値とその直後のトークンを名指す。""","""   `parse_error_message` が数値の開始位置を指し、数値とその直後に続く綴りを名指す。""")
open(p,'w').write(s)
