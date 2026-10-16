# 261017-3 の計画の材料

`doc/log/261017-3-plan.md` の各項目の節が引く、試作の差分、例、実測の出力、反証者の報告である。
項目ごとの担当と反証者、分野ごとの担当と反証者、全分野の合成の確認者が、作業用の一時領域に作ったものを、計画の作成のときに写した。
写しの diktor、ビルドの成果物、実行形、60 KB を超える出力（差分と分野の `runtest.log` は 120 KB まで）、担当の下書きと最終稿（計画の本文に取り込んだ）、反証者の作業用の写しは保存していない。
一時領域の絶対パスは `<一時領域>/` に置き換えた。

## 置き場所

- `<項目>/`（`V8`、`V22`、…、`X-B11` など 65 個）：項目ごとの材料。
  `<項目>.patch` が試作の差分、`ex/` が例、`results*.txt` が HEAD と試作の出力、`runtest*.log` が `dune runtest` の出力、`verify/` が反証者の報告（`refute.md`）と例である。
  差分が 2 つ以上ある項目は、計画の節の (3) がどれを採るかを書いている。
- `<分野>/`（`spec`、`syntax`、`numeric`、`eval`、`effects`、`never`、`classes`、`region`、`diagtext`、`kinds`、`extra`）：分野ごとの合成差分 `<分野>.patch`、当てた順 `order.txt`、合成後の `dune runtest` の出力 `runtest.log`、反証者の報告 `verify/refute.md`。
- `all/`：全分野（`extra` を除く 10 分野）の合成差分 `all.patch`、順に当てた記録 `order.txt`、段ごとの `runtest` の差分 `runtest.log`、報告 `report.md`、生の記録 `logs/`。
- `inventory/ledger.md`：台帳 V1〜V138 の番号ごとの最新の扱い（計画 0.1 節の根拠）。

## 当て方

差分は diktor 39d4091 の木の直下で `patch -p1` で当てる。
見出しは `a/` と `b/` の形にそろえてある（反証者の作業用の差分には、写しどうしの比較で作ったため `<一時領域>/…` の見出しを持つものがある）。
分野の合成差分は、計画 15 節の順（spec、syntax、numeric、eval、effects、never、classes、region、diagtext、kinds）で前の分野を当てた木に当てる。
`extra/extra.patch` と `V44`、`V47`、`V48`、`V49`、`V50`、`V53` の差分は、10 分野を当てて期待値を更新した木（`all/all.patch` を当てた木）に対するものである。
項目の差分は HEAD の木に対するもので、分野の合成の上では当たらないことがある（計画の各分野の「合成の確認」に手直しを書いた）。
