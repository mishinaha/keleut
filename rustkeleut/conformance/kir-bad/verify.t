読み込みには通り、検証(IRSpec 13.8 節)で拒否する KIR である。
ファイル名の vNN は、13.8 節の規則の番号である。

  $ keleut kir verify v01-symbol-form.kirt
  keleut: KIR の検証に失敗しました: 大域の記号 r3 が 単位の鍵/名前 の形ではありません
  [65]
  $ keleut kir verify v02-creation-site.kirt
  keleut: KIR の検証に失敗しました: 関数 main/r2$0 の種類 thunk が、作る場所の求める lambda と違います
  [65]
  $ keleut kir verify v03-function-fields.kirt
  keleut: KIR の検証に失敗しました: 関数 main/r2$0(lambda)の辞書の引数の数が規則と合いません
  [65]
  $ keleut kir verify v04-slot-range.kirt
  keleut: KIR の検証に失敗しました: 関数 main/once$h0.op.fail: スロット 1 が範囲外です
  [65]
  $ keleut kir verify v05-capture-count.kirt
  keleut: KIR の検証に失敗しました: 関数 main/once: main/once$h0.body の捕獲の数が違います
  [65]
  $ keleut kir verify v06-rep.kirt
  keleut: KIR の検証に失敗しました: 大域 main/r1 の Rep が初期化の関数の結果の Rep と違います
  [65]
  $ keleut kir verify v07-construct-fields.kirt
  keleut: KIR の検証に失敗しました: 関数 main/n: コンストラクタ Box のフィールドが足りません
  [65]
  $ keleut kir verify v08-tail-mark.kirt
  keleut: KIR の検証に失敗しました: 関数 main/once: handle の末尾の印が、再計算した結果と違います
  [65]
  $ keleut kir verify v09-resume-cyclic-handlebody.kirt
  keleut: KIR の検証に失敗しました: 関数 main/cyc$h0.body: resume を書けない関数に resume があります
  [65]
  $ keleut kir verify v09-resume-in-lambda.kirt
  keleut: KIR の検証に失敗しました: 関数 main/r2$0: resume を書けない関数に resume があります
  [65]
  $ keleut kir verify v09-resume-tail-off-spine.kirt
  keleut: KIR の検証に失敗しました: 関数 main/twice$h0.op.ask: resume の末尾の印が、再計算した背骨と違います
  [65]
  $ keleut kir verify v10-classification.kirt
  keleut: KIR の検証に失敗しました: 関数 main/twice$h0.op.ask: 操作節の分類 T が、再計算した G と違います
  [65]
  $ keleut kir verify v11-instance-methods.kirt
  keleut: KIR の検証に失敗しました: インスタンス main/instance:Size[Int32] がクラス main/Size のメソッド other を実装しません
  [65]
  $ keleut kir verify v12-init-twice.kirt
  keleut: KIR の検証に失敗しました: 大域 main/once を 2 回定義します
  [65]
  $ keleut kir verify v13-builtin-tables.kirt
  keleut: KIR の検証に失敗しました: 型の表の先頭が、付録 A の組み込みの型の並びと合いません
  [65]
  $ keleut kir verify v14-runtime-mark.kirt
  keleut: KIR の検証に失敗しました: エフェクト main/Ask の操作 ask の runtime の印が規則と合いません
  [65]
