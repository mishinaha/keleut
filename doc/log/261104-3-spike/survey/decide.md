# 調査 decide: V224、V218、V223、REPL の表示の書式、条件つきの既知の差の登録

- 日付: 2026-11-04
- 対象: 計画 261104-3 の推奨順の (4)。判断を要する項目である。
- 版: 親 69febfd、diktor cccd8cb。`rustkeleut/target/release/keleut` と `diktor/_build/install/default/bin/diktor` は HEAD から作ったもの。
- 測定の材料: `doc/log/261104-3-spike/probe/decide/`(以下 `probe/`)。
  台本は `*.sh` と `*.py`、結果は `*.out`、diktor の試作の差分は `proto-driver.diff` と `proto2-driver.diff` である。
- diktor の試作は、`git archive` で作業域に写した diktor の木で作った。
  `diktor/` の作業木には触れていない。
- 番号: 本調査は新しい番号を使わない。
  下の「案」は、計画を書く者が D564 以降の番号を振る。

## 0. 要約

| 項目 | 何か | 測った振る舞い | 推す扱い |
| --- | --- | --- | --- |
| V224 | `keleut repl` が入れ子 10 万段以上の入力を受理しない | keleut は 100,000 段で「KIR の書き出しで入れ子が深すぎます」、199,999 段で構文解析の限度。`diktor --repl` は 199,999 段まで受理 | 限度として残し、仕様に合う振る舞いとして閉じる。IRSpec 1 節に REPL の入力も同じであることを 1 文足す |
| V218 | Diktor が非ブロッキングの標準出力で `Sys_blocked_io` になり 2 で終わる | 木、`--run-ir`、`--via-ir`、`--repl` のすべてで 2。keleut は 4 つの道で 0 | diktor を直す(起動時に `O_NONBLOCK` を外す 1 行)。既知の差 2 項目を消す |
| V223 | `diktor --repl` が標準入力の読み誤りを「標準出力に書き出せません」と報告する | 閉じた標準入力、ディレクトリの標準入力でそう書いて 74 | diktor を直す(3 行と cram 1 本)。新しい cram に V217 の接頭辞の項目を足す |
| REPL の表示の書式 | LangSpec に書くか | 15 種の値の表示が、レコードの順を除いて両処理系でバイト列まで一致 | 書式は定めず、§6.6 に「表示の書式は処理系が定める」と 1 文足す |
| 条件つきの登録 | `diktor/test` の `--repl` のブロックが表示の順の差に当たったら登録する | いまは 15 本、816 個のコマンドで差 0 件 | 規則を計画の外の文書(`conformance/README.md`)にも書く。いまは登録しない |

2 つの diktor の修正を入れた写しで `dune runtest` が終了状態 0、出力 0 行で通り、既存の期待値は 1 つも変わらなかった【実測】(`probe/proto-runtest.out`。V218-2 の写しも同じ)。

## 1. V224: `keleut repl` の入れ子の限度

### 1.1 記録

- 記録 261024-3 の 5113〜5127 行(M100 の 3 回目の検証の指摘)が V224 を立てた。
  `run_input` が、検証が入れ子の限度で止まったときに書き手を走らせ、`<stdin>: 処理系の限度: KIR の書き出しで入れ子が深すぎます` を標準出力の 1 行に書き、入力を受理せずに続ける。
  「いまの試験群に当たるコマンドが無いので、`known-diffs.toml` には項目を足していない」(5127 行)。
- 記録 5288 行は、足した単位だけを確かめる検証の入口が V224 の判定も行うことを書く。
- 台帳の表(5660 行)は「残す(限度は I)」、残した項目(5671 行)は「振る舞いの差として残すもの」に入れている。

### 1.2 コード

- `rustkeleut/crates/keleut-cli/src/repl_cmd.rs:458-464` が `Ran::Limit` を定める。
- 同 `:494-520` の `Live::run` が `kir::Limits::default()`(`N_nest = 100,000`)で `kir::verify_extension` を呼ぶ。
  限度の誤り(`ErrorKind::Limit`)なら `kir::write_kir_with` を走らせ、書き手の文面を優先して `Ran::Limit` を返す。
- 同 `:784-787` の `process` が、その行を標準出力に書いて `Finish::Unrun` で入力を捨てる。
- 構文解析の限度は `keleut-front` の `DEFAULT_MAX_NEST`(20 万。`repl_cmd.rs:58`、`:109`)である。
- 試験は `crates/keleut-cli/tests/repl_cmd.rs:396-408` と `rustkeleut/conformance/frontend-depth/repl.t:48-60` にある。
- 書き手の限度の文面は `crates/kir/src/error.rs:81` の `WRITE_LIMIT_MESSAGE` である。

### 1.3 測った振る舞い

【実測】`probe/v224.sh`、結果は `probe/v224.out`。
式は `1 + (` を N 回重ねたもので、REPL には `let y = 1`、`let x = 式`、`echoln(show(y))`、`x + y` の 4 行を与えた。

| N | `keleut repl` | `diktor --repl` | `keleut run` | `diktor`(木) | `diktor --emit-ir` |
| --- | --- | --- | --- | --- | --- |
| 99,999 | 受理 | 受理 | 0 | 0 | 0 |
| 100,000 | 書き出しの限度の行、x は未束縛、0 | 受理、0 | 書き出しの限度、1 | 0 | 書き出しの限度、1 |
| 150,000 | 同上 | 受理、0 | 1 | 0 | 1 |
| 199,999 | 構文解析の限度(20 万)の行、0 | 受理、0 | 構文解析の限度、2 | 0 | 書き出しの限度、1 |

読み取れることは次のとおりである。

- 境目は `keleut run`、`diktor --emit-ir`、`keleut repl` で同じ 99,999 段と 100,000 段の間にある。
  REPL だけの限度ではなく、KIR を通すすべての道の限度である。
- `keleut repl` は限度を超えても 199,999 段まではセッションを続ける。
  20 万段を超える入力は、`keleut run` と同じく構文解析の限度で止まる。
  V224 を「KIR の限度」とだけ書くと、この 2 つめの限度が台帳に現れない。
- `diktor --repl` は KIR を作らないので、どちらの限度も持たない(199,999 段を受理した)。

### 1.4 仕様の位置

- IRSpec 1 節(`doc/IRSpec.md:52-54`)は「ノードの深さが `N_nest` を超える Keleut のプログラムは、KIR を持たない」とし、ソースから直接に実行する処理系(Diktor の木の評価器)はそのまま実行しうると書く。
  REPL の入力については書いていない。
- IRSpec 13.9 節(`doc/IRSpec.md:1668-1677`)は、書き手が限度を超える KIR を書かずに 1 で終えることを定める。
- LangSpec §15.5(`doc/LangSpec.md:1765-1803`)の表は、型検査での拒否(1)の条件を「型検査に要する領域が処理系の限度を超えた」とする。
  `keleut run` の書き出しの限度は 1 で終わるが、KIR の書き出しは厳密には型検査ではない。
  この点は REPL に限らない、文言の小さな隙間である【推論】。
- LangSpec §6.6(`doc/LangSpec.md:737-739`)は、字句と構文の誤りのある入力、型検査に通らなかった入力、実行時エラーで止まった入力を受理しないとする。
  §15.5 の表が読み込みと型検査の限度をそれぞれの誤りの種類に含めるので、構文解析の限度による不受理は §6.6 に合う。
  書き出しの限度による不受理は、§15.5 の文言を「型検査と、実行の前の変換」に広げれば同じ形で §6.6 に合う【推論】。

### 1.5 直すなら

RustKeleut の側:

- 案 V224-B(REPL だけ限度を上げる): `Live::run` の `kir::Limits::default()` を、フロントエンドの `max_nest` から作る `Limits::for_nest` に替え、書き手の検査を外す。
  REPL の KIR は書き出さないので、ファイルの形式の限度に縛られる理由は無い。
  ただし、検証、`keleut_lir::Extender` の下ろし、実行が 10 万段を超える木で落ちないかは測っていない【推論】。
  書き手が拒むので 10 万段を超える KIR を作る道が無く、試すにはコードを変える必要がある。
  直しても構文解析の 20 万段の限度は残るので、V224 は閉じずに狭まるだけである。
  `keleut run` と REPL の限度が違う形になる。
- 案 V224-A(残す): コードは変えない。

Diktor の側: 変えるものは無い。
`diktor --repl` が受理することは仕様に合う。

### 1.6 推す扱い

案 V224-A を推す。
理由は次のとおりである。

- 限度は KIR を通る道すべてに共通で、IRSpec 1 節が既にファイルのプログラムについて同じ差を認めている。
- 案 B は 20 万段の限度を残すので差を消せず、`keleut run` との不揃いを足す。
- 利用者が書く入力で 10 万段に届くものは生成した入力に限られる【推論】。

あわせて、文書を 2 か所直す案を出す。

1. IRSpec 1 節の 54 行の後に「対話的な実行の入力も同じで、KIR を通して実行する処理系は、深さが `N_nest` を超える入力を処理系の限度として受理しない」の趣旨の 1 文を足す。
   KIR の版は変わらない(14 節の規則で、意味も符号も変えない文書の改訂である)。
2. LangSpec §15.5 の表の 1 の行の条件を「型検査か、実行の前の変換に要する領域が処理系の限度を超えた」に改める(所有者の判断。LangSpec の改訂になる)。

台帳の V224 は「仕様に合う限度として閉じる」とし、構文解析の 20 万段の限度も同じ項に書き添える。
いまの試験群に当たるコマンドは無いので、`known-diffs.toml` の項目は要らない。

## 2. V218: Diktor の `Sys_blocked_io`

### 2.1 記録と登録

- 記録 2387〜2392 行(M88 の計画から外れたことの 3)が V218 を立て、「後の Diktor のマイルストーンで直す(fix)」とした。
  直し方として「Diktor の出力の書き込みで `EAGAIN` を待って書き直す」と書く。
- 記録 2490 行「V218 は直していない」、4929 行(既知の差の内訳)、5654 行(台帳の表「Diktor の不具合として残す(既知の差 2 項目)」)、5674 行。
- `rustkeleut/conformance/known-diffs.toml:416-436` の 2 項目(`conformance/bytes/bytes.t:50`、組 `dk`、`dk-ir`、`kf`、`x-ir` は `against = "record"`、`ki` から `kr-kw` の 12 組は `against = "pair"`)。
- `rustkeleut/conformance/bytes/bytes.t:31-50` は、非ブロッキングのパイプの試験と「Diktor は Sys_blocked_io の例外で止まる(この試験は Diktor と一致しない)」の注釈を持つ。

### 2.2 Diktor のコード

- `diktor/lib/driver.ml:272-280` の `safe_exit` は、`flush stdout` の `Sys_error` だけを握りつぶす。
- `driver.ml:1715-1717` の受け皿の最後の節が `Sys_blocked_io` を受けて「内部エラー: 予期しない例外です: Sys_blocked_io」を書き、`safe_exit 3` を呼ぶ。
  `safe_exit` の中の `flush stdout` が再び `Sys_blocked_io` を投げ、これは `Sys_error` でないので外へ出て、OCaml の「Fatal error」で 2 になる【推論。出力の 2 行と終了状態 2 がこの道に合う】。
- 標準出力への書き込みは `print_string` の sink(`driver.ml:1124`、`:1313`、`:1384`、`:1527`)のほか、REPL の `print_endline`、`Printf.printf`、`Format` など多くの所にある。
- `driver.ml:262` のコメントは「`Unix._exit` を使うには依存に unix を足す必要があり」と書くが、`lib/dune` は既に `unix` に依存している(`driver.ml:563` の `Unix.realpath`)。
  コメントが古い。

### 2.3 測った振る舞い

【実測】`probe/v218.sh`(読み手が 1 秒遅れて読む非ブロッキングのパイプ。`probe/nb.py` は bytes.t:35-48 と同じ)、結果は `probe/v218.out`。

| 道 | 終了状態 | 読めたバイト数 |
| --- | --- | --- |
| `diktor`(木) | 2(内部エラーと Fatal error の 2 行) | 65,536 |
| `diktor --run-ir` | 2 | 65,536 |
| `diktor --via-ir` | 2 | 65,536 |
| `diktor --repl` | 2 | 61,479 |
| `keleut run`(既定) | 0 | 4,194,313 |
| `keleut run --backend wasm` | 0 | 4,194,313 |
| `keleut repl` | 0 | 4,194,365 |

REPL も同じ不具合を持つ。
記録は bytes.t:50 の `diktor` の実行だけを挙げている。

### 2.4 直し方の案と試作

OCaml の標準のチャネルは、書き込みの途中で `Sys_blocked_io` を投げたとき、文字列のどこまでをバッファに受けたかを呼び手に返さない【推論。OCaml の実行時の `caml_putblock` の作り】。
そのため、例外を受けて `output_string` をやり直すと、バイト列が重なりうる。
記録 2392 行の「待って書き直す」を素直に入れるには、標準出力の書き込みをすべて自前の書き手に移す必要がある。

| 案 | 中身 | 大きさ | 測った結果 | 欠点 |
| --- | --- | --- | --- | --- |
| V218-1 | `main` の始めで `Unix.clear_nonblock Unix.stdout` | 1 行(と古いコメントの手直し) | 4 道とも 0、REPL は 4,194,365 バイトで keleut と同じ。`dune runtest` 通過。閉じた標準出力と `/dev/full` は従来どおり 74【実測】`probe/proto.out`、`probe/proto-runtest.out` | 開いたファイルの記述を共有する親の `O_NONBLOCK` も外れる【実測】`probe/nbflag.out`(keleut は親の旗を変えない) |
| V218-2 | 標準出力が FIFO か文字装置なら `/proc/self/fd/1` を開き直して `dup2` | 8 行 | 木と REPL で 0、親の旗は残る、`/dev/full` と閉じた標準出力は 74、追記のリダイレクトも壊れない【実測】`probe/proto2.out`、読み手のいないパイプは従来どおり 74【実測】`probe/noreader.out` | Linux に依る。端末や権限の違う装置での振る舞いを測っていない |
| V218-3 | 標準出力を 64 KiB のバッファと `Unix.single_write`、`EAGAIN` で `Unix.select` を待つ書き手に移す | 数十行と、標準出力に書くすべての所の置き換え | 作っていない | 変更が広い。オラクルの出力の道を広く触る |
| V218-0 | 直さない | 0 | — | 既知の差 2 項目と、REPL の同じ不具合が残る |

### 2.5 直すときの作業

- diktor: `lib/driver.ml` の `main`(1644 行の SIGPIPE の行の後)に案の行を足し、262 行の古いコメントを直す。
  コメントは literate の規約に従い、`python3 tools/litcheck.py` で確かめる(コード片が変わるので、litcheck はコメントだけの変更の確認にはならない)。
- diktor の試験: 既存の期待値は変わらない【実測】。
  `diktor/test` の cram は python3 を使うものが無いので、非ブロッキングのパイプの試験を足すには試験の道具が要る。
  `conformance/bytes/bytes.t:50` の組 `dk` が、記録(keleut の結果)と Diktor を比べるので、この試験が回帰の試験を兼ねる。
  `diktor/test` に cram を足さない案を推す。
- RustKeleut: `known-diffs.toml:416-436` の 2 項目を消し、`bytes.t:31-34` の注釈から「Diktor は止まる」を消す(期待値は変わらない)。
  【実測】試作 1 の diktor で `kconform --pairs dk,dk-ir,kf,x-ir,ki conformance/bytes/bytes.t` を走らせると、既知の差が 25 件から 20 件に減り、使われなかった既知の差が 2 件になった(`probe/kconform-bytes.out`)。
  消した後は 0 件になるはずである【推論】。
- 親: diktor の版を記録し、台帳の V218 を「直した」にする。
  `AGENTS.md` には V218 の記述が無いので、直すものは無い。

### 2.6 推す扱い

案 V218-1 を推す。
1 行で 4 つの道の不具合が消え、`dune runtest` と bytes.t の両方で測れている。
ただし副作用の範囲を正しく書くと、子が非ブロッキングの標準出力を受け継ぐ現実の場面は、Node(libuv)や Python の asyncio のようなイベントループの親が自分の記述を渡す場面である。
その親は、旗が外れると後の書き込みが止まりうるので、V218-1 の副作用を受けるのはまさにこの場面である【推論】。
Diktor を kconform と dune の下でオラクルとして走らせる使い方では、パイプは子ごとに作られるので効かない【推論】。
Diktor の利用者がオラクルとしての使い方に限られるとみて V218-1 を推すが、イベントループの親の下で Diktor を走らせる使い方を所有者が重く見るなら V218-2 に替える。
V218-2 も `dune runtest` は通った【実測】(`probe/proto-runtest.out`)が、Linux の `/proc` に依る。
いずれの案も Diktor の実装の変更なので、計画が diktor の変更を範囲に入れることを所有者が認める必要がある(計画 261024-1 は範囲の外とした。記録 5674 行)。

## 3. V223: `diktor --repl` の標準入力の読み誤りの文面

### 3.1 記録

- 記録 5041〜5049 行(M100 の計画から外れたことの 9)が V223 を立てた。
  keleut は `keleut: 標準入力を読めません: 理由`、Diktor は `diktor: 標準出力に書き出せません: 理由`、どちらも 74。
  「いまの試験群に当たるコマンドが無いので、`known-diffs.toml` には項目を足していない(接頭辞の差は V217 と同じである)」。
- 台帳の表 5659 行「Diktor の不具合として残す(当たる試験が無い)」。

### 3.2 コード

- Diktor: `diktor/lib/driver.ml:1433` の `In_channel.input_line stdin` が `Sys_error` を投げると、REPL の中に受け手が無く、`main` の受け皿 `driver.ml:1671-1673` の `Sys_error msg -> die_output msg` に届く。
  `die_output`(`driver.ml:282-284`)が「標準出力に書き出せません」を書いて 74 で終わる。
  受け皿の注釈「入力側の誤りは Input_error に閉じ込めてあるので、ここへ来るのは出力側の誤りである」(1672 行)の前提を、REPL の標準入力が破っている。
- keleut: `rustkeleut/crates/keleut-cli/src/repl_cmd.rs:287-293`(起動時に閉じていた標準入力。`:76`、`:90` の `STDIN_CLOSED`)と `:307-317`(読みの誤り)。
  試験は `crates/keleut-cli/tests/repl_cmd.rs:349-363`。

### 3.3 測った振る舞い

【実測】`probe/v223.sh`、結果は `probe/v223.out`。

| 入力 | `diktor --repl` | `keleut repl` |
| --- | --- | --- |
| 閉じた標準入力 | `diktor: 標準出力に書き出せません: Bad file descriptor`、74 | `keleut: 標準入力を読めません: Bad file descriptor`、74 |
| ディレクトリの標準入力 | `diktor: 標準出力に書き出せません: Is a directory`、74 | `keleut: 標準入力を読めません: Is a directory`、74 |
| `--no-prelude` で閉じた標準入力 | 同上 | 同上 |

### 3.4 直すときの作業

- diktor: `driver.ml:1433` の `match` に `| exception Sys_error msg -> Printf.eprintf "diktor: 標準入力を読めません: %s\n" msg; safe_exit 74` の 3 行を足す(`probe/proto-driver.diff`)。
  【実測】試作で、2 つの入力とも `diktor: 標準入力を読めません: 理由` と 74 になり、keleut との違いは接頭辞だけになった(`probe/proto.out`)。
  `dune runtest` は通り、既存の期待値は変わらなかった(`probe/proto-runtest.out`)。
- diktor の試験: `diktor/test/repl_stdin.t`(新設。閉じた標準入力 `<&-` とディレクトリの標準入力の 2 コマンド)を足す。
  ディレクトリは cram の作業ディレクトリ `< .` にし、`/tmp` の有無に依らないようにする(probe では `< /tmp` を使った)。
  `repl.t` の末尾に足してもよいが、新しいファイルの方が既知の差の行番号に触れない。
  Diktor の不具合を直すための試験なので、「RustKeleut のために diktor/test の期待値を変えない」規則には当たらない。
- RustKeleut: 新しい cram は `--suite all` に入り、shim が `keleut repl` に送る組(`ki` から `kw`、`kr-*`)で接頭辞が違う。
  その 2 コマンドに、V217 の項目(診断の接頭辞)を、既存の V217 の項目と同じ 12 組で足す。
  cram は `.kel` を含まないので、`kconform extract-corpus --check` は変わらないはずである【推論】。
- 親: 台帳の V223 を「直した」にする。

### 3.5 推す扱い

直すことを推す。
変更は 3 行と cram 1 本で、副作用は測った範囲で無い。
直さない場合は、いまのまま当たる試験が無く、既知の差も要らない。

## 4. REPL の表示の書式を LangSpec に書くか

### 4.1 いまの仕様と計画の扱い

- 計画 261104-1 の 2.2 節(`doc/log/261104-1-plan.md:224-227`)は、これを範囲の外とした。
  理由は「LangSpec は REPL の表示の書式をどこにも定めておらず、§18.5 は表示が変わらないことだけを求める。書式を定めるなら、レコード以外の値も含めた別の改訂になる」。
- LangSpec §6.6(`doc/LangSpec.md:728-753`)は対話的な実行を提供してよいとし、入力の区切り方を処理系に任せるが、表示には触れない。
- §15.5 の末尾(`doc/LangSpec.md:1802-1803`)は、対話的な実行での診断の出力先を処理系に任せる。
- §18(`doc/LangSpec.md:1958-1961`)は、表示する型、診断、警告の文面を評価の結果に含めない。
- §18.5(`doc/LangSpec.md:2052-2056`)は「受理した各入力の受理、表示、評価の結果は変わらない」とし、「表示」を書式の定め無しに使う。
- D558(計画 261104-1 の 4.1 節、`261104-1-plan.md:335-364`)は、レコードのフィールドの順を処理系の選択とした。

### 4.2 測った振る舞い

【実測】`probe/display.sh` と `probe/display.txt`(Int32、String、Float64、List、Option、タプル、文字列を持つレコード、Unit、関数、ラベルの順の違うレコード、拡張、同じラベル 2 つ、コンストラクタ、`:type`、式)。
結果 `probe/display.out` では、両処理系の出力がバイト列まで一致した。
【実測】`probe/order.txt`、`probe/order.out`: 注釈で型の行の順と作る順を違えると、Diktor は作った順(`{y = 1, x = 5}`)、keleut は型の行の順(`{x = 5, y = 1}`)で書く(D558 の差)。

書式を定めるなら決めることになる点が、測定から 1 つ見つかった。
文字列は、トップレベル、タプル、レコードの中では引用符付きで書かれる(`"a\"b\n"`、`(1, "a")`、`{name = "n"}`)が、`List` と `Option` の中では `show` のまま引用符無しで書かれる(`[p, q]`、`Some(x)`)。
両処理系はこの綴りまで同じである。

### 4.3 案

| 案 | 中身 | 大きさ | 欠点 |
| --- | --- | --- | --- |
| 表示-A | 書式を定めず、§6.6 に「受理した入力の結果を表示するかどうかと、その書式は処理系が定める」の 1 文を足す。§18.5 の「表示」はこの表示を指すと読める | LangSpec に 1 文 | 2 つの処理系の綴りの一致は、仕様でなく `tests/repl_cmd.rs` のコーパスの比較と kconform が保つままになる |
| 表示-B | `名前 : 型 = 値`、`_` の名前、値の綴り(文字列の引用と上の不揃い、`<fn>`、`()`、レコードの順)を LangSpec に定める | 新しい節。`List` と `Option` の引用の不揃いをどちらに決めるかの判断と、Diktor か keleut の変更が要りうる | 改訂が大きく、D558 で処理系に任せた順とも整合をとり直す |
| 表示-0 | 何もしない | 0 | §18.5 の「表示」が何を指すかが文書から読めないままになる |

### 4.4 推す扱い

表示-A を推す。
書式の一致は既にコーパスの比較で保たれていて、仕様で縛る利用者の需要が記録に無い。
§6.6 に 1 文を足すと、§18.5 と D558 が拠る前提(書式は処理系が定める)が LangSpec に現れる。

## 5. REPL の表示の順の差を `known-diffs.toml` に登録する条件

### 5.1 計画の規則

計画 261104-1 の 2.3 節(`doc/log/261104-1-plan.md:233-236`)は、`known-diffs.toml` に V228 や V221 の項目を足さないとし、「後で `diktor/test` の `--repl` のブロックが REPL の表示の順の差に当たったときは、そのコマンドを D558 と V221 の番号で登録する」とした。
理由は、鍵がコマンドの単位で、足すと同じコマンドのほかの差まで隠すことである。
この規則は計画の中にだけあり、`conformance/README.md`、`known-diffs.toml` の注釈、`AGENTS.md` には書かれていない(grep で確かめた)。

### 5.2 測った振る舞い

【実測】`--repl` の呼び出しを含む 15 本の `diktor/test` の cram を `kconform --pairs dk,kr-ki,ki --check-shim` で走らせた。
816 個のコマンドで差 0 件、既知の差 32 件、shim を通らないもの 0 個(`probe/kconform-repl.out`。M104 の受け入れ、記録 261104-2 と同じ数)。
`diktor/test` の REPL の表示でレコードを含む行は `repl.t:204`(`{x = 1}`)と `repl.t:304`(`{f = <fn>, n = 1}`)の 2 行だけで、どちらも型の行の順と作った順が同じである。
いまは条件に当たっていない。

### 5.3 案と推す扱い

- 登録-A: 規則を保ち、`conformance/README.md` の既知の差の説明か `known-diffs.toml` の冒頭の注釈に 2〜3 行で書き写す。
  当たったときの登録の形(組は `ki` から `kw` と `kr-*`、台帳は V221、理由は D558)もあわせて書く。
- 登録-B: kconform の `when` に REPL の表示の行のレコードを並べ替えて比べる条件を足す(`crates/kconform/src/known_diffs.rs:16-21`、`:262` は今は `"unimplemented"` だけを受ける)。
  計画 261104-1 の 2.3 節が、プログラムの出力の行と表示の行を区別できないことを理由に採らなかった正規化に近い。
- 登録-A を推す。
  当たる試験が無いうちに機構を足す理由が無く、規則を計画の外に書けば次の作業者が見落とさない。

V223 を直して `diktor/test/repl_stdin.t` を足すと、その 2 コマンドが V217 の登録を要する(3.4 節)。
これは表示の順の登録とは別で、既存の V217 の形に従う。

## 6. 所有者が決めること

| 問い | 選択肢 | 推す答え |
| --- | --- | --- |
| diktor の変更を本計画の範囲に入れるか | 入れる / 入れない | 入れる(V218 と V223 で合わせて 4 行。既存の期待値は変わらない【実測】) |
| V218 の直し方 | V218-1 `clear_nonblock` / V218-2 開き直し / V218-3 自前の書き手 / V218-0 直さない | V218-1 |
| V223 を直すか | 直して cram を足す / 残す | 直して cram を足す |
| V224 の扱い | A 限度として閉じる / B REPL だけ限度を上げる | A。IRSpec 1 節に 1 文 |
| LangSpec §15.5 の 1 の行の条件を「実行の前の変換」まで広げるか | 広げる / 広げない | 広げる(`keleut run` の書き出しの限度の 1 も同じ行に入る) |
| REPL の表示の書式を LangSpec に書くか | 表示-A / 表示-B / 表示-0 | 表示-A |
| 表示の順の差の登録の規則 | 登録-A / 登録-B | 登録-A |

## 7. 計画に入れるときの作業の大きさの見込み

- diktor の修正(V218、V223): `lib/driver.ml` 4 行と 1 か所のコメント、`test/repl_stdin.t` 新設。
  diktor の中のコミット 2 本(不具合ごと)、親の版の記録 1 本。
  受け入れは `dune runtest`、`kconform --pairs dk ../diktor/test/*.t`、`kconform --suite all` の全組(V217 の 2 項目を足し、V218 の 2 項目を消した後)。
- RustKeleut の変更: `known-diffs.toml`(V218 の 2 項目を消し、V217 の 2 項目を足す)、`conformance/bytes/bytes.t:31-34` の注釈。
  RustKeleut のコードは変えない。
- 文書: IRSpec 1 節に 1 文(V224)と、IRSpec の履歴の表の 1 行(計画 261104-1 の D556 と同じく、版を変えない文書の改訂として書く。記録 261104-2 の 36 行)、LangSpec §6.6 に 1 文(表示-A)、§15.5 の表の 1 行(所有者が認めたとき)、`conformance/README.md` に 2〜3 行(登録-A)。
  `doc/sample.kel` には対話的な実行と処理系の限度に当たる記述が無い(grep で `対話`、`REPL`、`限度` を探し、294 行の補完の注釈だけが当たった)ので、`diktor/test/sample/sample.kel` の同期は要らない【実測】。
- 台帳: V218、V223 は「直した」、V224 は「仕様に合う限度として閉じた」。
