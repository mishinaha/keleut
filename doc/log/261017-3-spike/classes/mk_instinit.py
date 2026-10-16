import sys; sys.path.insert(0,sys.argv[1]); from mkcram import blk, rd
I=sys.argv[2]; V=I+'/V110'; W=I+'/V97/run'
out=[]
out.append("""インスタンスのメソッドを値束縛で書いたときの初期化式(LangSpec §12.2、§6.4)。
宣言の実行で 1 回だけ評価する初期化式は、トップレベルの評価と同じエフェクトを
起こせる。前提を持つインスタンスと制約を持つメソッドの初期化式はメソッドを使う
たびに評価するので、エフェクトを起こせない。署名の行と照らすのは初期化式の値の
関数の行で、初期化式が起こすエフェクトは含まない。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"
""")
def add(pre, name, src, cmds):
    out.append(pre.rstrip('\n')+'\n\n'+blk(name, src, cmds))
add("初期化式は Console を起こせる。前提を持たないインスタンスでは宣言の実行で 1 回だけ\n評価する:", 'instinitcon', rd(V+'/ex/e8.kel'), ['diktor instinitcon.kel'])
add("エフェクトを起こさない値でない初期化式は受理する。返す無名関数の決まっていない行は\n空の行に決まる。組み込みのクラスでも、利用者のクラスの @ E のメソッドでも同じ:", 'instweakrow', rd(V+'/ex/b4.kel'), ['diktor instweakrow.kel'])
out[-1]+=blk('instweakrowuser', rd(V+'/ex/c7.kel'), ['diktor instweakrowuser.kel'])
add("初期化式の中で処理した操作は、初期化式の行に残らない:", 'instinithandled', rd(V+'/ex/c6.kel'), ['diktor instinithandled.kel'])
add("効果多相の関数を初期化式で呼ぶ形は、その関数の行変数が返す関数の行と共有されても\n受理する。初期化式の中の局所の束縛で呼んでも同じ:", 'instinitpoly', rd(V+'/verify/ex/r6.kel'), ['diktor instinitpoly.kel'])
out[-1]+=blk('instinitpolylocal', rd(V+'/verify/ex/r12.kel'), ['diktor instinitpolylocal.kel'])
add("トップレベルで起こせないエフェクトを初期化式で起こすと、エフェクトの名前とメソッドの\n名前を挙げて拒否する。位置はメソッドの束縛である:", 'instinitask', rd(V+'/ex/u4.kel'), ['diktor instinitask.kel'])
out[-1]+=blk('instinitheap', rd(V+'/ex/u3.kel'), ['diktor instinitheap.kel'])
add("前提を持つインスタンスの初期化式はメソッドを使うたびに評価するので、Console も\n起こせない:", 'instinitperuse', rd(V+'/ex2/q7.kel'), ['diktor instinitperuse.kel'])
# timing blocks from V97
import re
t=rd(I+'/V97/diktor/test/inst_init_pure.t')
def src_of(name):
    m=re.search(r"  \$ cat > "+name+r"\.kel <<'KEL'\n(.*?)  > KEL\n", t, re.S)
    return ''.join(l[4:]+'\n' for l in m.group(1).rstrip('\n').split('\n'))
add("ゼロ除算は行に現れないので、初期化式に書いても型検査を通る。前提を持たない\nインスタンスでは、インスタンス宣言を実行する時点で実行時エラーになる(LangSpec §6.4):", 'inidiv', src_of('inidiv'), ['diktor inidiv.kel'])
add("前提を持つインスタンスでは初期化式をメソッドを使うたびに評価するので、実行時エラーは\nメソッドを使う時点で起きる(LangSpec §6.4):", 'inipremdiv', src_of('inipremdiv'), ['diktor inipremdiv.kel'])
add("エフェクトを起こさない初期化式は、構文上の値でなくても受理する。組み込みのクラスでも\n利用者のクラスと同じである:", 'inipure', src_of('inipure'), ['diktor inipure.kel'])
out.append("対話的な実行でも同じ規則で、トップレベルの行が Print を含むので、Print を起こす\n初期化式も受理する:\n\n  $ diktor --repl <<'EOF2'\n"+''.join('  > '+l+'\n' for l in rd(V+'/verify/ex/repl1.txt').rstrip('\n').split('\n'))+"  > EOF2\n")
open(sys.argv[3],'w').write('\n'.join(out))
