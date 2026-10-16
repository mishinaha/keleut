pub newtype のフィールドと effect の操作の診断は、宣言の先頭ではなく、
そのフィールドや操作の型の位置を指す。

  $ export PATH="$TESTDIR/../_build/install/default/bin:$PATH"

pub newtype の完全注釈の診断は、@ の無い矢印を含むフィールドの型を指す。
ラベルつきのフィールドではラベルの右の型を指し、2 つ目のフィールドなら 2 つ目を指す:

  $ cat > ntpos.kel <<'KEL'
  > module P {
  >   pub newtype Parser[A] = Parser(
  >     go: (String) => Option[A] @ {},
  >     fail: (String) => Unit)
  > }
  > KEL
  $ diktor --type-check ntpos.kel
  ! ntpos.kel:4:11: 型エラー: pub な newtype のフィールドには完全な型注釈が必要です(注釈の中の矢印に @ がありません)
  [1]

コンストラクタが複数あるときも、悪いコンストラクタのフィールドの型を指す。
矢印が型引数の中にあるときは、フィールドの型の先頭を指す:

  $ cat > ntpos2.kel <<'KEL'
  > pub newtype T = A((Int32) => Int32 @ {})
  >   | B(Int32, List[(Int32) => Int32])
  > KEL
  $ diktor --type-check ntpos2.kel
  ! ntpos2.kel:2:14: 型エラー: pub な newtype のフィールドには完全な型注釈が必要です(注釈の中の矢印に @ がありません)
  [1]

effect の操作の型が矢印でないときは、その操作の型を指す:

  $ cat > effpos.kel <<'KEL'
  > effect Log = {
  >   log: (String) => Unit,
  >   level: Int32
  > }
  > KEL
  $ diktor --type-check effpos.kel
  ! effpos.kel:3:10: 型エラー: 操作 level の型は矢印型でなければなりません
  [1]

予約された操作名と、英小文字で始まらない操作名は、名前の右にある型の位置を指す:

  $ cat > effpos2.kel <<'KEL'
  > effect Log = {
  >   log: (String) => Unit,
  >   cancel: () => Unit
  > }
  > KEL
  $ diktor --type-check effpos2.kel
  ! effpos2.kel:3:11: 型エラー: 操作名 cancel は予約されています(handle の cancel 節と衝突するため宣言できません)
  [1]

  $ cat > effpos3.kel <<'KEL'
  > effect Log = {
  >   log: (String) => Unit,
  >   _x: () => Unit
  > }
  > KEL
  $ diktor --type-check effpos3.kel
  ! effpos3.kel:3:7: 型エラー: 操作名 _x は英小文字で始めてください(handle の節が操作名として読めません)
  [1]

同じ effect の中での操作名の重複は、2 度目の宣言の型を指す:

  $ cat > effpos4.kel <<'KEL'
  > effect Log = {
  >   log: (String) => Unit,
  >   flush: () => Unit,
  >   log: () => Unit
  > }
  > KEL
  $ diktor --type-check effpos4.kel
  ! effpos4.kel:4:8: 型エラー: 操作 log が二重に宣言されています
  [1]

型パラメータの診断は、操作ではなく宣言全体の誤りなので、宣言の先頭を指す:

  $ cat > effpos5.kel <<'KEL'
  > effect Log[A] = {
  >   log: (A) => Unit
  > }
  > KEL
  $ diktor --type-check effpos5.kel
  ! effpos5.kel:1:1: 型エラー: effect 宣言に型パラメータは書けません(sample.kel §9)
  [1]

type class のメソッドの診断も、操作と同じく、そのメソッドの型を指す。
型が矢印でないときはその型を指し、メソッドの重複は 2 度目の宣言の型を指す:

  $ cat > clspos.kel <<'KEL'
  > type class C[A] {
  >   val g: (A) => Int32
  >   val h: A
  > }
  > KEL
  $ diktor --type-check --no-prelude clspos.kel
  ! clspos.kel:3:10: 型エラー: メソッド h の型は矢印型でなければなりません
  [1]
  $ cat > clspos2.kel <<'KEL'
  > type class C[A] {
  >   val g: (A) => Int32
  >   val f: (A) => Int32
  >   val f: (A) => String
  > }
  > KEL
  $ diktor --type-check --no-prelude clspos2.kel
  ! clspos2.kel:4:10: 型エラー: メソッド f が二重に宣言されています
  [1]
