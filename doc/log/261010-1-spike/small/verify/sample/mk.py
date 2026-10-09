import sys
src, dst = sys.argv[1], sys.argv[2]
L = open(src, encoding='utf-8').read().split('\n')
def rep(a, b, new):
    global L
    assert len(new) == b - a + 1, (a, b, len(new))
    L[a-1:b] = new
rep(35, 35, ["//   型位置で `)` の直後が `=>`         → 関数型。それ以外は式位置と同じ（`,` があるか空ならタプル型、なければグループ化）"])
rep(94, 94, ["let d = -1                           // 数値リテラルに直接付けた - は負のリテラル（前置の - は Neg.neg を呼ぶ）"])
rep(366, 367, ["//   +  Add.add      -  Sub.sub      *  Mul.mul      /  Div.div      %  Rem.rem",
               "//   == Eq.eq        != Eq.eq の否定                 前置の -  Neg.neg"])
rep(523, 526, ["//            候補が 2 つ以上なら、その位置のエフェクト行に現れる候補が 1 つだけのとき、それに解決する。",
               "//            行に現れる候補が 0 個または 2 個以上なら、修飾を要求する（推測はしない）。",
               "//            同じエフェクトが行に 2 回以上現れても、候補としては 1 個と数える。行の中の位置は使わない。",
               "//            そのため、ラベルの順序だけが違う行（型として等しい行）の下では、解決の結果も同じになる。"])
rep(532, 534, ["// 異なるラベルの順序は、型の等価性にも操作名の解決にも使わない（§3 の scoped labels と同じ）。",
               "// 同じラベルどうしの順序は区別し、同じエフェクトを二重に積んだときにどのハンドラが捕まえるかを決める。",
               "// 下の copy の 2 つ目の perform は、行に File と Console の 2 つの候補が現れるので、File.write と修飾する。"])
rep(624, 624, ["  perform File.write(text)                    // 最も左の File は dst のもの"])
open(dst, 'w', encoding='utf-8').write('\n'.join(L))
