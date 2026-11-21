バイナリの読み込みの検査(IRSpec 13.7 節)で拒否される KIR である。
b23 だけは読み込みの検査に通り、検証(IRSpec 13.8 節)で拒否される。

  $ keleut kir verify b01-header-short.kir
  keleut: KIR を読み込めません: 位置 0: 見出しの 16 バイトがありません
  [65]
  $ keleut kir verify b02-version.kir
  keleut: KIR を読み込めません: 位置 8: 版 0.0 は読めません(読めるのは主版 1 の版だけです)
  [65]
  $ keleut kir verify b03-feature-bit.kir
  keleut: KIR を読み込めません: 位置 12: 必要な機能のビット 0x00000001 を処理できません
  [65]
  $ keleut kir verify b04-section-count.kir
  keleut: KIR を読み込めません: 位置 827: 節の数が見出し(10)と違います(11)
  [65]
  $ keleut kir verify b05-section-undefined.kir
  keleut: KIR を読み込めません: 位置 17: 節 0x00 は定義されていません
  [65]
  $ keleut kir verify b06-section-order.kir
  keleut: KIR を読み込めません: 位置 603: 節 0x02 が id の昇順に並んでいません
  [65]
  $ keleut kir verify b07-section-missing.kir
  keleut: KIR を読み込めません: 位置 827: 必須の節 0x0b がありません
  [65]
  $ keleut kir verify b08-section-size.kir
  keleut: KIR を読み込めません: 位置 19: 節 0x01 の大きさがファイルの残りを超えます
  [65]
  $ keleut kir verify b09-section-trailing.kir
  keleut: KIR を読み込めません: 位置 598: 節 0x01 の中身の後ろにバイトが残っています
  [65]
  $ keleut kir verify b10-read-past-end.kir
  keleut: KIR を読み込めません: 位置 602: 節の終わりを越えて読もうとしました
  [65]
  $ keleut kir verify b11-leb128-overlong.kir
  keleut: KIR を読み込めません: 位置 22: uLEB128 が u32 の範囲を超えます
  [65]
  $ keleut kir verify b12-count-overflow.kir
  keleut: KIR を読み込めません: 位置 605: 個数が節の残りのバイト数を超えます
  [65]
  $ keleut kir verify b13-bad-tag.kir
  keleut: KIR を読み込めません: 位置 747: 式の番号 0x34 は定義されていません
  [65]
  $ keleut kir verify b14-bool-value.kir
  keleut: KIR を読み込めません: 位置 740: yield の印の値が 0 でも 1 でもありません
  [65]
  $ keleut kir verify b15-flag-bits.kir
  keleut: KIR を読み込めません: 位置 770: 呼び出しの印に定義されていないビットがあります
  [65]
  $ keleut kir verify b16-strings-order.kir
  keleut: KIR を読み込めません: 位置 602: stringsの表が厳密な昇順に並んでいません
  [65]
  $ keleut kir verify b17-strings-unused.kir
  keleut: KIR を読み込めません: 位置 827: strings の項目 1 がどこからも使われていません
  [65]
  $ keleut kir verify b18-strings-utf8.kir
  keleut: KIR を読み込めません: 位置 602: strings の表に UTF-8 でない名前があります
  [65]
  $ keleut kir verify b19-index-range.kir
  keleut: KIR を読み込めません: 位置 773: 大域の添字 14 が範囲外です(表の大きさは 14)
  [65]
  $ keleut kir verify b20-count-lower-bound.kir
  keleut: KIR を読み込めません: 位置 823: def_fns の大域がありません
  [65]
  $ keleut kir verify b21-truncated.kir
  keleut: KIR を読み込めません: 位置 19: 節 0x01 の大きさがファイルの残りを超えます
  [65]
  $ keleut kir verify b22-bad-magic.kir
  keleut: KIR を読み込めません: 位置 0: マジックが KIR のものではありません
  [65]
  $ keleut kir verify b23-verify-tail-mark.kir
  keleut: KIR の検証に失敗しました: 関数 main/total: call の末尾の印が、再計算した結果と違います
  [65]
  $ keleut kir verify b24-strings-order-trailing.kir
  keleut: KIR を読み込めません: 位置 598: stringsの表が厳密な昇順に並んでいません
  [65]
  $ keleut kir verify b25-version-major.kir
  keleut: KIR を読み込めません: 位置 8: 版 2.0 は読めません(読めるのは主版 1 の版だけです)
  [65]
  $ keleut kir verify b26-version-draft.kir
  keleut: KIR を読み込めません: 位置 8: 版 0.3 は読めません(読めるのは主版 1 の版だけです)
  [65]
