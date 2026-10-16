import sys
p=sys.argv[1]; s=open(p,encoding='utf-8').read()
anchor='''名指しは最外の行の単一化で失敗したときのものなので、同じ宣言でも実装しだいで'''
add='''名指しの後半は実装の側の束縛で言い分ける。実装が最外の @ を自分で書いていれば、
本体が何も起こしていなくても注釈の行で落ちるので、本体ではなく注釈を指す
(本体が何かを起こした形は上の clsimpure):

  $ cat > clsannotrow.kel <<'KEL'
  > newtype Box = Box(Int32)
  > type class Sizer[T] { val size: (T) => Int32 }
  > type instance Sizer[Box] { let size(b: Box): Int32 @ Print = b match { case Box(x) => x } }
  > KEL
  $ diktor --type-check clsannotrow.kel

値束縛の注釈の頭に型エイリアスを書いた形も、エイリアスの中の @ を注釈の側として述べる:

  $ cat > clsannotalias.kel <<'KEL'
  > newtype Box = Box(Int32)
  > type Fn = (Box) => Int32 @ Print
  > type class Sizer[T] { val size: (T) => Int32 }
  > type instance Sizer[Box] { let size: Fn = fn(b) => 1 }
  > KEL
  $ diktor --type-check clsannotalias.kel

'''
assert s.count(anchor)==1; s=s.replace(anchor, add+anchor)
open(p,'w',encoding='utf-8').write(s)
