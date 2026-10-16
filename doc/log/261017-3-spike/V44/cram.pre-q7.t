pub で @ を省略した宣言の本体は純粋で(LangSpec §13.2)、本体がエフェクトを起こすと
「pub な宣言はエフェクトを起こせません」と報告する。この文言を出すのは、原因が
本体のエフェクトである失敗だけで、レコードのフィールドやヴァリアントの選択肢の
失敗と、run のヒープがスコープの外に漏れる失敗には出さない。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

値束縛の注釈の頭の @ を省略すると、初期化式が返す関数がエフェクトを起こす形は
pub の規則を名指して拒否する。入れ子の矢印の行に @ {} と書いた形と、返り値の
レコードのフィールドが違う形は、pub でない束縛と同じく「注釈された型を満たしません」
で拒否する:

  $ cat > pubvaleff.kel <<'KEL'
  > module M {
  >   pub let g: (Int32) => Int32 = fn(x) => { echoln("a"); x }
  > }
  > KEL
  $ diktor --type-check pubvaleff.kel
  ! pubvaleff.kel:2:11: 型エラー: pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: ラベル Console がありません(行は閉じています))
  [1]
  $ cat > pubvalinner.kel <<'KEL'
  > module M {
  >   pub let g: (Int32) => (Int32) => Int32 @ {} = fn(x) => fn(y) => { echoln("a"); y }
  > }
  > KEL
  $ diktor --type-check pubvalinner.kel
  ! pubvalinner.kel:2:11: 型エラー: 注釈された型を満たしません(ラベル Console がありません(行は閉じています))
  [1]
  $ cat > pubvalrec.kel <<'KEL'
  > module M {
  >   pub let g: (Int32) => {a: Int32} = fn(x) => {b = x}
  > }
  > KEL
  $ diktor --type-check pubvalrec.kel
  ! pubvalrec.kel:2:11: 型エラー: 注釈された型を満たしません(ラベル a がありません(行は閉じています))
  [1]

引数のレコードのフィールドと、返り値のヴァリアントの選択肢でも同じである:

  $ cat > pubvalarg.kel <<'KEL'
  > module M {
  >   pub let g: ({a: Int32}) => Int32 = fn(r: {b: Int32}) => r.b
  > }
  > KEL
  $ diktor --type-check pubvalarg.kel
  ! pubvalarg.kel:2:11: 型エラー: 注釈された型を満たしません(ラベル a がありません(行は閉じています))
  [1]
  $ cat > pubvalvariant.kel <<'KEL'
  > module M {
  >   pub let g: (Int32) => #A | #B = fn(x) => #C
  > }
  > KEL
  $ diktor --type-check pubvalvariant.kel
  ! pubvalvariant.kel:2:11: 型エラー: 注釈された型を満たしません(ラベル C がありません(行は閉じています))
  [1]

返り値のフィールドが違い、本体がエフェクトも起こす形は、フィールドの失敗を報告する:

  $ cat > pubvalboth.kel <<'KEL'
  > module M {
  >   pub let g: (Int32) => {a: Int32} = fn(x) => { echoln("a"); {b = x} }
  > }
  > KEL
  $ diktor --type-check pubvalboth.kel
  ! pubvalboth.kel:2:11: 型エラー: 注釈された型を満たしません(ラベル a がありません(行は閉じています))
  [1]

let rec の値束縛も同じである。フィールドの失敗は、pub でない let rec と同じ文言になる:

  $ cat > pubrecvaleff.kel <<'KEL'
  > module M {
  >   pub let rec g: (Int32) => Int32 = fn(x) => { echoln("a"); x }
  > }
  > KEL
  $ diktor --type-check pubrecvaleff.kel
  ! pubrecvaleff.kel:2:15: 型エラー: pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: ラベル Console がありません(行は閉じています))
  [1]
  $ cat > pubrecvalrec.kel <<'KEL'
  > module M {
  >   pub let rec g: (Int32) => {a: Int32} = fn(x) => {b = x}
  > }
  > KEL
  $ diktor --type-check pubrecvalrec.kel
  ! pubrecvalrec.kel:2:15: 型エラー: ラベル a がありません(行は閉じています)
  [1]

関数束縛の本体の run の中で、単相のフィールドの関数を呼ぶと、ヒープのラベルが
フィールドの関数の行に入ろうとして、スコープの外に漏れる。pub の有無によらず、
漏れとして報告する:

  $ cat > pubrunleak.kel <<'KEL'
  > let idf[A](x: A): A = x
  > module M {
  >   let r = idf({go = fn(u: Int32) => u + 1})
  >   pub let f(): Int32 = run h { r.go(1) }
  > }
  > KEL
  $ diktor --type-check pubrunleak.kel
  idf : (A) => A
  M.r : {go: (Int32) => Int32}
  ! pubrunleak.kel:4:32: 型エラー: スコープ付きの型 ς1 がスコープの外に漏れています
  [1]
  $ sed 's/pub let f/let f/' pubrunleak.kel > runleak.kel
  $ diktor --type-check runleak.kel
  idf : (A) => A
  M.r : {go: (Int32) => Int32}
  ! runleak.kel:4:28: 型エラー: スコープ付きの型 ς1 がスコープの外に漏れています
  [1]

run の本体で、エフェクトを起こす引数の関数を呼ぶ形は、pub の規則を名指して拒否する:

  $ cat > pubruneff.kel <<'KEL'
  > module M {
  >   pub let f(g: () => Int32 @ {Console}): Int32 = run h { g() }
  > }
  > KEL
  $ diktor --type-check pubruneff.kel
  ! pubruneff.kel:2:58: 型エラー: pub な宣言はエフェクトを起こせません(@ を明示するか pub を外してください。元の報告: ラベル Console がありません(行は閉じています))
  [1]
