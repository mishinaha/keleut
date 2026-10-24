テキストの読み込み(IRSpec 12 節)で拒否する KIR である。

  $ keleut kir verify t01-syntax.kirt
  keleut: KIR を読み込めません: 59 行目: ) がありません
  [65]
  $ keleut kir verify t02-unknown-name.kirt
  keleut: KIR を読み込めません: 58 行目: 大域が表にありません: main/nosuch
  [65]
  $ keleut kir verify t03-count-lower-bound.kirt
  keleut: KIR を読み込めません: 58 行目: seq の要素が 2 個より少なくなっています
  [65]
  $ keleut kir verify t04-version.kirt
  keleut: KIR を読み込めません: 1 行目: 版 0.2 は読めません(読めるのは 0.3 だけです)
  [65]
