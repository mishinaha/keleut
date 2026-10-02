tb (末尾ブロック) の試作。基準は diktor 6a8dab7 の写し(../../../tb/diktor.orig)。
- trailing_block.diff : 試作の全差分(lib/parser.mly, lib/syntax.ml, lib/driver.ml)。../../../tb/diktor で dune build / dune runtest が通る(差分 0、新しい test/trailing_block.t を含む)
- parser.diff         : 上の parser.mly 部分だけ
- trailing_block.t    : cram 案(試作で dune promote した期待出力つき)
- paren_position_variant.diff : 試作に (e) { ... } の位置を足す変種(採らない案。衝突 0、runtest 差分 0)
- layer_*.diff : menhir --explain の衝突の実測に使った文法(意味アクションは計測用で、P は型が合わない)
    A0  : 3 位置 x 3 形だけ                    -> 衝突 0
    A   : A0 + { _ => } の拒否 + 2 個目の拒否    -> 衝突 0
    AB  : A + 位置の外の { x => / { case の拒否  -> 衝突 0
    ABC : AB + レコードの形 / perform / resume の拒否 -> 衝突 0
    P   : ABC + (e) { ... }                      -> 衝突 0
  対照: scratchpad/d/v6_both(任意の後置式 + 呼び出しの後ろ)は shift/reduce 1 件(同じ手順で再現)
- AST 同一性の比較: ../../../tb/cmp/ の 460 ファイル(test/*.t の heredoc 457 個 + test/sample/*.kel 2 個 + lib/prelude.kel)で、
  diktor.orig と diktor の --dump-ast(stderr と終了コード込み)が全件一致。
