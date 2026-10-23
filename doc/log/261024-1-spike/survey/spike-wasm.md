# スパイク報告: Node v26.9.0 の WasmGC 対応と wasm-encoder / wasmparser

調べたのは 2026-10-24 である。
スパイクのコード、手順、出力の全文は `doc/log/261024-1-spike/wasm/README.md` にある。
作ったファイルは `doc/log/261024-1-spike/wasm/` の下だけである。追跡済みのファイルは変えておらず、コミットもしていない。
`Cargo.lock` は版を固定するために残した(`.gitignore` で除くのは `target/`、`node_modules/`、`out/`)。

各節で、**測定**(実行して得た事実)と**推論**(測定から導いた判断)を分ける。

## 0. 環境

- 測定: `node --version` → `v26.9.0`、`node -p process.versions.v8` → `14.6.202.34-node.32`。
- 測定: `cargo info` の最新版は wasm-encoder 0.262.0、wasmparser 0.262.0、wat 1.262.0、wast 262.0.0、wasm-tools 1.262.0 で、どれも rust-version 1.88.0 である。
- 方法: モジュールは WAT で書き、`wat` crate(内部で wast と wasm-encoder を使う)でバイナリにし、wasmparser で検証した。
  あわせて wasm-encoder の API だけで 2 つのモジュールを組んだ(`src/main.rs` の `build_encoder_module` と `build_fx_encoder_module`)。

## 1. GC の構造体と配列、i31ref、ref.test / ref.cast

- 測定(フラグなし、`node run.mjs`):
  - `struct.new` / `struct.get` / `struct.set` → 42。
  - i8 の配列で `array.new` / `array.new_fixed` / `array.copy` / `array.get_u` / `array.get_s` / `array.len` → 10359(期待値どおり)。`array.new_data` も `print.wat` で動いた。
  - `ref.i31` / `i31.get_s` / `i31.get_u`: -5 → -5、2^30 → -1073741824(31 ビットの符号拡張)、-1 → 2147483647。
  - `ref.test` による分類 → 1230(i31、部分型、上位型、null を区別した)。
  - `ref.cast` は成功時に 42、失敗時は `RuntimeError: illegal cast` で trap する。`br_on_cast` → 77,5。
  - 構造体 5000 万個の確保 → 378〜391 ms(2 回)。プロセスは落ちなかった。
- 推論: Keleut の値表現(ヒープの構造体、バイト列 `(array (mut i8))`、小さな整数の i31ref、`ref.test` / `br_on_cast` による型の判定)は、フラグなしでそのまま WasmGC に写せる。

## 2. 型付きの関数参照と末尾呼び出し

- 測定(`tail.wat`、フラグなし):

  | 呼び出し | n = 1e7 | 1e8 | 1e9 |
  | --- | --- | --- | --- |
  | `return_call` の相互再帰 even/odd | 3.1 ms | 21.5 ms | 213.2 ms |
  | `return_call_ref` の相互再帰 | 6.5 ms | 56.2 ms | 560.0 ms |

  どれも正しい値(偶数で 1、`even(10_000_001)` で 0)を返した。
- 測定: 構造体に入れた関数参照の `call_ref` → 正しい値。
- 測定: 末尾でない相互再帰 `even_nt(1e7)` → `RangeError: Maximum call stack size exceeded`。
  `depth.mjs` の二分探索で、成功する最大の深さは約 15689 だった。`--stack-size=7800` では約 126952、`--stack-size=65500` では終了コード 139(SIGSEGV)。
- 測定(`tier.mjs`、`even(1e7)`): `--liftoff-only` で 39.0 / 44.2 ms、`--no-liftoff` で 2.6〜2.8 / 31.8〜32.1 ms(3 回)、既定で 3.2〜3.3 / 6.5 ms。
  どの層でもスタックは溢れない。
- 推論: 時間が n にほぼ比例し、どの層でも溢れないので、V8 は Liftoff と TurboFan の両方でフレームを再利用している。
  既定の 1e7 が `--no-liftoff` と同程度の速さなので、ループの途中で TurboFan への動的な昇格が起きている。
- 推論(確かめていない): `--no-liftoff` で `return_call_ref` だけが遅いのは、TurboFan の投機的なインライン展開が Liftoff の集めた呼び出し先の情報を使うためと考えられる。この説明は検証していない。
- 推論: 末尾呼び出しに頼る Keleut の再帰はトランポリンなしで WasmGC に出せる。一方、末尾でない再帰の深さは既定で 1 万 5 千段程度しかない。

## 3. exnref による例外処理

- 測定(フラグなし、`eh.wat`):
  - `try_table` + `catch $e` → 投げないとき 5、投げたとき 1042。
  - `catch_ref` で受けた exnref を外側へ `throw_ref` → 3050。
  - JS の例外を `catch_all_ref` で受ける → 1。`WebAssembly.JSTag` を import して `catch $jstag` で受けると、JS の例外オブジェクトそのものを得る(`=== lastJsError` が true)。
  - JS の例外を `catch_all_ref` してから `throw_ref` すると、同じオブジェクトが JS に届く(true)。
  - JS 側の `new WebAssembly.Exception(tag, [7])` → `is(e)=true`、`getArg` → 7。
  - 複数値を持つタグ `(param i64 i32)` → 5。
- 測定: `node --v8-options` によると、`--experimental-wasm-exnref` と `--experimental-wasm-legacy-eh` はどちらも既定で有効である。
- 推論: Keleut の例外(261017-1 の Never 返しの操作)は、フラグなしで exnref に写せる。古い EH 命令を出す必要はない。

## 4. バイト列の stdout への印字

- 測定: JS から見た GC 配列は中身の読めない物体である(`typeof=object keys=[] [0]=undefined length=undefined`)。
- 測定: 次の 2 つの方法で、`"Hello, WasmGC bytes あ\n"`(UTF-8 で 24 バイト)を `fs.writeSync(1, ...)` で正しく出せた。
  - A: GC 配列を import した JS 関数に渡し、JS が wasm の書き出した `bytes_len` / `bytes_get` を呼んで 1 バイトずつ `Uint8Array` に写す。
  - B: wasm がループで線形メモリに写し、`(ptr, len)` を渡す。JS は `new Uint8Array(mem.buffer, p, n)` を書く。
- 測定: `wasm:js-string` の組み込み(`{ builtins: ["js-string"] }`、`fromCharCodeArray` は i16 配列から JS 文字列を作る)はフラグなしで動いた(`len=3 str="Kel"`)。
- 測定: i8 配列を UTF-8 として解く `wasm:text-decoder` の `decodeStringFromUTF8Array` は、フラグなしでは `Import #0 "wasm:text-decoder": module is not an object or function` で失敗した。`--experimental-wasm-imported-strings-utf8` を付けると `"Kあ"` を得た。
- 推論: 今の Node で入出力をつなぐ実用的な方法は B(線形メモリの小さなバッファを経由する)である。A は 1 バイトごとに wasm と JS の境界を越えるので遅い(速さは測っていない)。
  UTF-8 の組み込みは、フラグが外れれば B を置き換える候補になる。

## 5. JSPI

- 測定: `WebAssembly.Suspending` と `WebAssembly.promising` はフラグなしで関数として存在する。
- 測定: 非同期の JS 関数を `Suspending` で包んで import し、export を `promising` で包んで呼ぶと Promise が返り、242 に解決した(10 ms の `setTimeout` を 2 回待った)。
- 測定: `--v8-options` に JSPI のフラグは無い。`--experimental-wasm-growable-stacks`(JSPI の伸びるスタック)は既定で無効である。
- 推論: Node の非同期 API(ファイル入出力など)を wasm から同期的に呼ぶ手段として、JSPI はフラグなしで使える。

## 6. スタック切り替え(`--experimental-wasm-wasmfx`)

- 測定: フラグなしでは `CompileError: ... core stack switching not enabled (enable with --experimental-wasm-wasmfx) @+14` で拒まれる。
- 測定(フラグあり):
  - `fx.wat` と `fx_encoder.wasm`(1 から n を `suspend` で返す生成器を `resume` で回す)→ `sum(10)=55`、`sum(1e6)=1784293664`(期待値どおり)。100 万回の往復で 60〜64 ms。
  - `cont.bind` → 42。`resume_throw`(止まった継続に例外を投げ込み、継続の側の `try_table` が受ける)→ 507。
  - `resume` の処理表に `(on $sw switch)` を書くだけで、プロセスが `# Fatal error in , line 0 / # unimplemented code` で落ちた(終了コード 133)。
  - `switch` 命令(`fx3.wat`)も同じ落ち方をした。`--no-wasm-lazy-compilation` では `compiled` が出る前に落ちるので、関数のコンパイルの段階で落ちている。
  - `--liftoff-only` では `unimplemented Liftoff instruction: cont.new` で落ちる。`--no-liftoff` では `fx` と `fx2` の前半が動き、`switch` は同じく落ちる。
- 測定(V8 が受け付けた符号、`xxd -g1 out/fx_encoder.wasm`): 継続の型 `0x5d typeidx`(0x0e: `5d 00`)、`suspend` `0xe2 tag`(0x53: `e2 00`)、`cont.new` `0xe0 typeidx`(0x69: `d2 00 e0 01`)、`resume` `0xe3 typeidx vec(handler)`、処理 `on $tag $label` は `0x00 tag label`(0x75: `e3 01 01 00 00 00`)。
  wasm-encoder の符号は `cont.bind` 0xe1、`resume_throw` 0xe4、`resume_throw_ref` 0xe5、`switch` 0xe6 である(`wasm-encoder-0.262.0/src/core/instructions.rs:4538-4608`)。V8 は wat で書いた `cont.bind` と `resume_throw` を受け付けて正しく実行した。
- 推論: V8 の実装は、wasm-encoder 0.262.0 と wasmparser 0.262.0 が実装している符号(0xe0〜0xe6 の並び)に一致する。少なくとも試した 5 命令は一致した。
- 推論: スタック切り替えは今の Node では出荷に使えない。フラグが要り、Liftoff は未実装で、`switch` はコンパイルで JS から捕まえられない異常終了を起こす。
  `WebAssembly.validate` は `switch` を含むモジュールに true を返したので(`fx3.mjs`)、実行時に使えるかを事前に調べる手段も無い。
  WasmGC バックエンドで継続が要る機能(効果の処理など)には、CPS 変換や JSPI など別の手段を残す必要がある。WasmFX は、`cont.new` / `suspend` / `resume` に限ったフラグ付きの実験経路にとどめるのがよい。

## 7. wasm-encoder 0.262.0 と wasmparser 0.262.0 の対応

- 測定(wasmparser、`wasmparser-0.262.0/src/features.rs`):
  - 既定で有効: `tail_call`(210 行)、`exceptions`(225 行)、`function_references`、`gc`(237 行)、`gc_types`。
  - 既定で無効: `legacy_exceptions`(247 行)、`stack_switching`(266 行)。
  - `WasmFeatures::WASM3` は tail_call と exceptions を含む(420〜428 行)。
  - 検証: 既定の機能集合では `cannot define continuation types when stack switching is disabled` で拒み、`WasmFeatures::default() | WasmFeatures::STACK_SWITCHING` で通る。GC、末尾呼び出し、exnref のモジュールは既定のままで通った。
- 測定(wasm-encoder、`wasm-encoder-0.262.0/src/core/code.rs`): `Instruction` の列挙に `ReturnCallRef`(481 行)、`TryTable`(487 行)、`ThrowRef`(489 行)、`StructNew`(699 行)、`ArrayNewFixed`(722 行)、`RefI31`(769 行)、`ContNew`(1296 行)、`Suspend`(1301 行)、`Resume`(1302 行)、`ResumeThrowRef`(1311 行)がある。
  `types.rs` には `CompositeInnerType::Cont(ContType)`(41 行)と抽象ヒープ型 `cont` / `nocont` がある。wasm-encoder には機能の切り替えが無い。
- 測定: wasm-encoder の API で組んだ `encoder.wasm`(GC の配列と構造体、`try_table`、`throw`、`return_call_ref`、`ref.i31`)は V8 でフラグなしで動いた(`enc_main(3)=8`、`enc_main(11)=1011`)。
  `fx_encoder.wasm` もフラグありで動いた。
- 測定(API の注意): 0.262.0 では `SubType { is_final, supertype_idxs: Vec<u32>, composite_type }`、`CompositeType { inner, shared, descriptor, describes }` である。手元の記憶にあった `supertype_idx: Option<u32>` は通らなかった。
- 推論: RustKeleut の WasmGC バックエンドは、出力を wasm-encoder、検証と試験を wasmparser で書ける。API は版ごとに変わるので、`Cargo.lock` で版を固定する。
  `wat` crate は試験で WAT を書くときに使える。

## 8. 計画への含意(すべて推論)

- フラグなしの Node v26.9.0 で頼れる機能: GC の構造体と配列、i31ref、`ref.test` / `ref.cast` / `br_on_cast`、型付きの関数参照と `call_ref`、`return_call` / `return_call_ref`、exnref、JSPI、`wasm:js-string`。
- フラグが要る機能: スタック切り替え(`--experimental-wasm-wasmfx`)、UTF-8 の文字列組み込み(`--experimental-wasm-imported-strings-utf8`)。
- 使えない機能: `switch` と `(on $tag switch)`(プロセスが落ちる)。
- 入出力は、線形メモリのバッファに写して JS に `(ptr, len)` を渡す方法を既定にするのがよい。
- バックエンドの試験は、Node を子プロセスで起動し、標準出力と終了コードを比べる形で書ける。今回の `run.mjs` がその形である。
