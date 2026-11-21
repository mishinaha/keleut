入れ子の上限(IRSpec 13.9 節)を超える KIR である。
deep-base.kirt(正しいプログラム)の main/total の本体を、深さ n の連鎖に置き換えて作る。
正準なインデントで書くとファイルのサイズが深さの 2 乗に比例するので、連鎖は 1 行に書く。

  $ chain() {
  >   awk -v n="$1" -v kind="$2" '
  >     index($0, "(call ntail noyield (global \"main/loop\")") == 0 { print; next }
  >     {
  >       printf "  "
  >       if (kind == "exp") {
  >         for (i = 1; i < n; i++) printf "(not "
  >         printf "(lit i64 0)"
  >         for (i = 1; i < n; i++) printf ")"
  >       } else if (kind == "pat") {
  >         printf "(match (lit i64 0) (clause "
  >         for (i = 2; i < n; i++) printf "(variant a "
  >         printf "(wild)"
  >         for (i = 2; i < n; i++) printf ")"
  >         printf " (lit i64 0)))"
  >       } else {
  >         printf "(opcall \"builtin/Eq\" eq "
  >         for (i = 2; i < n; i++) printf "(drecord (field a "
  >         printf "(dbuiltin \"builtin/Eq\" \"builtin/Int32\")"
  >         for (i = 2; i < n; i++) printf "))"
  >         printf " (lit i32 0) (lit i32 0))"
  >       }
  >       print ")"
  >     }' deep-base.kirt
  > }

上限ちょうどの深さ 100,000(N_nest)の KIR は受理する。

  $ chain 100000 exp > ok.kirt
  $ keleut kir verify ok.kirt

深さ 100,001 の式、パターン、辞書のテキストは、読み込みで拒否する。

  $ chain 100001 exp > exp.kirt
  $ keleut kir verify exp.kirt
  keleut: KIR を読み込めません: 58 行目: 処理系の限度: KIR の読み込みで入れ子が深すぎます
  [65]
  $ chain 100001 pat > pat.kirt
  $ keleut kir verify pat.kirt
  keleut: KIR を読み込めません: 58 行目: 処理系の限度: KIR の読み込みで入れ子が深すぎます
  [65]
  $ chain 100001 dict > dict.kirt
  $ keleut kir verify dict.kirt
  keleut: KIR を読み込めません: 58 行目: 処理系の限度: KIR の読み込みで入れ子が深すぎます
  [65]

深さ 100,001 のバイナリは、上限を上げた keleut kir asm で作る。
既定の上限では、読み込みで拒否する。

  $ keleut kir asm --max-nest 100001 exp.kirt -o exp.kir
  $ keleut kir verify exp.kir
  keleut: KIR を読み込めません: 位置 100806: 処理系の限度: KIR の読み込みで入れ子が深すぎます
  [65]
  $ keleut kir asm --max-nest 100001 dict.kirt -o dict.kir
  $ keleut kir verify dict.kir
  keleut: KIR を読み込めません: 位置 300809: 処理系の限度: KIR の読み込みで入れ子が深すぎます
  [65]

上限を下げると、浅いプログラムも同じ診断で拒否する。

  $ keleut kir verify --max-nest 3 deep-base.kirt
  keleut: KIR を読み込めません: 50 行目: 処理系の限度: KIR の読み込みで入れ子が深すぎます
  [65]
