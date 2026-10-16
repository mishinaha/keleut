型パラメータの行変数と空の行の衝突(LangSpec §13.2)。行変数は何が入るか決まって
いないので、@ {} や入れ子の矢印で省略した @(どちらも空の行)とは一致しない。
単一化の報告は原因だけを述べ、直し方を書かない。直し方は束縛の種類と、
空の行がどちらの側にあるかで変わる。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

関数束縛の返り値の注釈が空の行の矢印だと、原因だけを報告する。返り値の
矢印に @ E と書けば通る:

  $ cat > fnrigid.kel <<'KEL'
  > let h[E](g: () => Int32 @ E): () => Int32 @ {} = g
  > KEL
  $ diktor --type-check fnrigid.kel
  ! fnrigid.kel:1:5: 型エラー: 注釈された返り値型を満たしません(行 ς1 は注釈で固定された行変数なので、空の行と一致させられません)
  [1]
  $ cat > fnrigidok.kel <<'KEL'
  > let h[E](g: () => Int32 @ E): () => Int32 @ E = g
  > KEL
  $ diktor --type-check fnrigidok.kel
  h : (() => Int32) => () => Int32

局所の値束縛は型パラメータを取れない。注釈の空の行が外側の型パラメータの
行変数とぶつかっても、行変数を新しく導入するよう促す助言は出さない:

  $ cat > valrigid.kel <<'KEL'
  > let f[E](g: () => Int32 @ E): Int32 @ E = {
  >   let k: () => Int32 @ {} = g
  >   k()
  > }
  > KEL
  $ diktor --type-check valrigid.kel
  ! valrigid.kel:2:7: 型エラー: 注釈された型を満たしません(行 ς1 は注釈で固定された行変数なので、空の行と一致させられません)
  [1]

空の行が初期化式の側(呼ぶ関数の仮引数)にあるときも、同じ文言になる。
注釈には行変数が既に書かれており、直すのは初期化式の関数の側である:

  $ cat > valinitrigid.kel <<'KEL'
  > let ap(h: () => Int32 @ {}): Int32 = h()
  > let f[E](g: () => Int32 @ E): Int32 @ E = {
  >   let k: (() => Int32 @ E) => Int32 = ap
  >   k(g)
  > }
  > KEL
  $ diktor --type-check valinitrigid.kel
  ap : (() => Int32 @ {}) => Int32
  ! valinitrigid.kel:3:7: 型エラー: 注釈された型を満たしません(行 ς1 は注釈で固定された行変数なので、空の行と一致させられません)
  [1]

インスタンスの実装は、値束縛、let rec の値束縛、関数束縛のどれでも原因だけを
報告する。宣言の行変数 E は実装の側から名指しできない:

  $ cat > clsrigid.kel <<'KEL'
  > type class Run[A] {
  >   val run1[E]: (A, () => Int32 @ E) => Int32 @ E
  > }
  > type instance Run[Int32] {
  >   let run1: (Int32, () => Int32) => Int32 = fn(a, g) => g()
  > }
  > KEL
  $ diktor --type-check clsrigid.kel
  ! clsrigid.kel:5:7: 型エラー: インスタンスメソッド run1 がクラス宣言の型を満たしません(行 ς1 は注釈で固定された行変数なので、空の行と一致させられません)
  [1]
  $ cat > clsrecrigid.kel <<'KEL'
  > type class Run[A] {
  >   val run1[E]: (A, () => Int32 @ E) => Int32 @ E
  > }
  > type instance Run[Int32] {
  >   let rec run1: (Int32, () => Int32) => Int32 = fn(a, g) => g()
  > }
  > KEL
  $ diktor --type-check clsrecrigid.kel
  ! clsrecrigid.kel:5:11: 型エラー: インスタンスメソッド run1 がクラス宣言の型を満たしません(行 ς1 は注釈で固定された行変数なので、空の行と一致させられません)
  [1]
  $ cat > clsfnrigid.kel <<'KEL'
  > type class Run[A] {
  >   val run1[E]: (A, () => Int32 @ E) => Int32 @ E
  > }
  > type instance Run[Int32] {
  >   let run1(a: Int32, g: () => Int32): Int32 = g()
  > }
  > KEL
  $ diktor --type-check clsfnrigid.kel
  ! clsfnrigid.kel:5:7: 型エラー: インスタンスメソッド run1 がクラス宣言の型を満たしません(行 ς1 は注釈で固定された行変数なので、空の行と一致させられません)
  [1]
