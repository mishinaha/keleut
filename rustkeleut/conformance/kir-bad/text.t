テキストの読み込み(IRSpec 12 節)で拒否する KIR である。

  $ keleut kir verify t01-syntax.kirt
  keleut: KIR を読み込めません: 59 行目: ) がありません
  [65]
  $ keleut kir verify t02-unknown-name.kirt
  keleut: KIR を読み込めません: 58 行目: グローバルが表にありません: main/nosuch
  [65]
  $ keleut kir verify t03-count-lower-bound.kirt
  keleut: KIR を読み込めません: 58 行目: seq の要素が 2 個より少なくなっています
  [65]
  $ keleut kir verify t04-version.kirt
  keleut: KIR を読み込めません: 1 行目: バージョン 0.2 は読めません(読めるのはメジャーバージョン 1 のバージョンだけです)
  [65]
  $ keleut kir verify t05-version-major.kirt
  keleut: KIR を読み込めません: 1 行目: バージョン 2.0 は読めません(読めるのはメジャーバージョン 1 のバージョンだけです)
  [65]
  $ keleut kir verify t06-version-minor-range.kirt
  keleut: KIR を読み込めません: 1 行目: マイナーバージョン 65536 が u16 の範囲にありません
  [65]
