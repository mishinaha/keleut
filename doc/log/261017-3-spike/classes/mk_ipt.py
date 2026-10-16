import sys; sys.path.insert(0,sys.argv[1]); from mkcram import blk, rd
V=sys.argv[2]+'/V136'
out=["""インスタンスのメソッドの仮引数のうち、型注釈のない変数と _ の型は、クラス宣言の署名の
対応する引数の型に決まる(LangSpec §12.2)。仮引数に署名の型を注釈した関数と同じく、
本体の数値リテラルの型と網羅性の検査はその型に基づく。返り値の型と最外の行は署名から
決めない。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"
"""]
def add(pre,name,src,cmds): out.append(pre.rstrip('\n')+'\n\n'+blk(name,src,cmds))
never = """type class D1[A] { val d1: (A) => String }
type class D2[A] { val d2: (A) => String }
type class D3[A] { val d3: (A) => String }
type instance D1[Never] { let d1(n) = n match {} }
type instance D2[Never] { let d2 = fn(n) => n match {} }
type instance D3[Never] { let rec d3(n) = n match {} }
"""
add("署名の引数が Never なら、match {} は網羅的である。関数束縛、値束縛の無名関数、let rec\nのどれでも同じ:", 'insnever', never, ['diktor --strict-exhaustive --type-check insnever.kel'])
add("同じ理由で、署名の引数が Never か閉じた構造的ヴァリアントのとき、case _ の節には\n一致する値が無いという警告や冗長の警告が出る:", 'inswild', rd(V+'/ex2/inswild.kel'), ['diktor inswild.kel','diktor --strict-exhaustive --type-check inswild.kel'])
add("本体の数値リテラルは、署名の引数の型に決まる:", 'inslit', rd(V+'/ex/w.kel'), ['diktor inslit.kel'])
add("閉じた構造的ヴァリアントの引数の漏れは、型エラーではなく網羅漏れの警告になる:", 'insvariant', rd(V+'/ex/x.kel'), ['diktor insvariant.kel','diktor --strict-exhaustive insvariant.kel'])
add("渡した型と本体が食い違うと、注釈を書いた関数と同じく、本体の食い違った位置に出る:", 'insbody', rd(V+'/ex/s.kel'), ['diktor --type-check insbody.kel'])
add("返り値の型は署名から決めないので、リテラルは既定の Int32 になり、署名と照らした\n時点で落ちる:", 'insret', rd(V+'/ex/t.kel'), ['diktor insret.kel'])
add("注釈を書いた仮引数には署名の型を渡さない:", 'insannot', rd(V+'/ex/i.kel'), ['diktor --type-check insannot.kel'])
add("前提を持つインスタンスでも、前提のクラスを本体で使う形は変わらない:", 'inspremise', rd(V+'/ex2/inspremise.kel'), ['diktor inspremise.kel','diktor --strict-exhaustive --type-check inspremise.kel'])
open(sys.argv[3],'w').write('\n'.join(out))
