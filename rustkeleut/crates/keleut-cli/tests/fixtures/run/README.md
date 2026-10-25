# keleut run の fixture

`tests/run_cmd.rs` が使う。
各 `NAME.kel` を Diktor(diktor ecb7cc0)で `diktor --emit-ir NAME.kir NAME.kel` として KIR にし、`diktor --run-ir NAME.kir` の標準出力、標準エラー、終了状態を `NAME.stdout`、`NAME.stderr`、`NAME.code` に書いた。
`deep` だけは両方に `--max-depth 100` を足す。

`effect` はハンドラを使うので、記録は Diktor のものでなく、M87 の keleut の結果(`before` の後に未実装で 4)である。
M89 でエフェクトを入れたら、Diktor の結果に書き直す。

`num`、`order`、`order2`、`div0`、`lit`、`inst`、`fwd`、`inst2`、`hole`、`matchfail` は `doc/log/261024-1-spike/probe/fid/` の同名のファイルを写したものである。

`noyield.kirt` は、生成器(G の節の末尾でない resume)の `noyield.kel` を `diktor --no-prelude --emit-ir-text` でテキストにし、ハンドラ式の本体の `gen` の呼び出しの印を手で `noyield` に書き換えたものである。
印が誤っているので、yield の刈り込み(パス 11)を行う既定の `keleut run` は 70 で終わり、`--no-yield-prune` では 0 で終わる。
