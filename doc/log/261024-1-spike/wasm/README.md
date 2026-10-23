# WasmGC バックエンド向け Node.js の機能調査(スパイク)

Node v26.9.0 の V8 が WasmGC バックエンドに必要な機能をどこまで受け付けるかを、小さなモジュールを実際に動かして確かめた。
測定日は 2026-10-24 である。

## 環境と版

- Node v26.9.0、V8 14.6.202.34-node.32(`node -p process.versions.v8`)
- rustc / cargo 1.98.1
- crate は `Cargo.lock` で固定した。wasm-encoder 0.262.0、wasmparser 0.262.0、wat 1.262.0、wast 262.0.0(いずれも 2026-10-24 の `cargo info` の最新版)

## ファイル

- `wat/*.wat` は試験用のモジュールである。`gc`、`tail`、`eh`、`print`、`strings`、`utf8`、`jspi`、`fx`、`fx2`、`fx3` がある。
- `src/main.rs` は `wat/*.wat` を `wat` crate で組み立て、`wasmparser` で検証し、`out/*.wasm` に書く。
  あわせて wasm-encoder の API だけで 2 つのモジュールを組む(`out/encoder.wasm` は GC・try_table・throw・return_call_ref、`out/fx_encoder.wasm` は `fx.wat` と同じ生成器)。
- `run.mjs` はフラグなしの検査、`fx.mjs`・`fx2.mjs`・`fx3.mjs` はスタック切り替えの検査である。
- `depth.mjs` は末尾でない相互再帰の深さの上限を二分探索で測る。`tier.mjs` は V8 の層ごとに末尾呼び出しの時間を測る。`utf8.mjs` は UTF-8 の文字列組み込みを試す。
- `out/` と `target/` は生成物なので `.gitignore` で除いた。

## 手順

```sh
cd doc/log/261024-1-spike/wasm
cargo run -q                                   # out/*.wasm を作り wasmparser で検証する
node run.mjs                                   # フラグなしの検査
node --experimental-wasm-wasmfx fx.mjs         # cont.new / suspend / resume
node --experimental-wasm-wasmfx fx2.mjs        # cont.bind / resume_throw / (on $t switch)
node --experimental-wasm-wasmfx fx3.mjs        # switch 命令
node depth.mjs
node tier.mjs; node --liftoff-only tier.mjs; node --no-liftoff tier.mjs
node utf8.mjs; node --experimental-wasm-imported-strings-utf8 utf8.mjs
```

## 結果

### `cargo run -q`

```
eh: wasmparser validate OK (465 bytes)
fx: wasmparser validate ERROR: cannot define continuation types when stack switching is disabled (at offset 0xe)
fx (+stack_switching): wasmparser validate OK (229 bytes)
fx2 (+stack_switching): wasmparser validate OK (305 bytes)
fx3 (+stack_switching): wasmparser validate OK (131 bytes)
gc: wasmparser validate OK (734 bytes)
jspi / print / strings / tail / utf8: wasmparser validate OK
encoder: wasmparser validate OK (127 bytes)
fx_encoder (+stack_switching): wasmparser validate OK (139 bytes)
```

wasmparser の既定の機能集合では継続の型を拒む。`WasmFeatures::STACK_SWITCHING` を足すと通る。

### `node run.mjs`(フラグなし)

```
node v26.9.0 v8 14.6.202.34-node.32
PASS struct.new/get/set (expect 42): 42
PASS array i8 new/new_fixed/copy/get_u/get_s/len (expect 10000+255+104=10359): 10359
PASS i31 get_s(-5) / get_s(2^30) / get_u(-1): -5,-1073741824,2147483647
PASS ref.test classify (expect 1230): 1230
PASS ref.cast ok (expect 42): 42
PASS ref.cast failure traps: RuntimeError: illegal cast
PASS br_on_cast (expect 77,5): 77,5
PASS alloc 50M structs (GC runs): 49999999 in 391 ms
PASS return_call even(10_000_000) mutual tail recursion: 1 in 3.1 ms
PASS return_call even(10_000_001): 0
PASS return_call_ref even(10_000_000) mutual tail recursion: 1 in 6.5 ms
PASS return_call_ref even(10_000_001): 0
PASS call_ref via struct closure after even_ref set globals (odd(7)=1): 1
PASS return_call even(100000000): 1 in 21.5 ms
PASS return_call_ref even(100000000): 1 in 56.2 ms
PASS return_call even(1000000000): 1 in 213.2 ms
PASS return_call_ref even(1000000000): 1 in 560.0 ms
PASS non-tail call even_nt(10_000_000) (expect RangeError): RangeError: Maximum call stack size exceeded
PASS WebAssembly.JSTag exists: object
PASS try_table catch: no throw (5) / throw (1000+42): 5,1042
PASS catch_ref + throw_ref (3000+50): 3050
PASS catch_all_ref of JS exception (1): 1
PASS catch JSTag gives the JS object: true
PASS throw_ref of JS exception reaches JS unchanged: true
PASS wasm exception seen in JS as WebAssembly.Exception: true is(e)=true arg=7
PASS multi-value tag payload (5): 5
Hello, WasmGC bytes あ
PASS print strategy A (GC array -> JS via accessors): printed above
Hello, WasmGC bytes あ
PASS print strategy B (copy into linear memory): printed above
PASS JS sees GC array as opaque object: typeof=object keys=[] [0]=undefined length=undefined
PASS js-string builtins (fromCharCodeArray i16 array): len=3 str="Kel"
PASS JSPI API present: Suspending=function promising=function
PASS JSPI run (expect (21*2)+(100*2)=242): returns Promise -> 242
PASS wasm-encoder module enc_main(3)=8, enc_main(11)=1011: 8,1011
FAIL fx.wasm compile without --experimental-wasm-wasmfx: CompileError: WebAssembly.compile(): core stack switching not enabled (enable with --experimental-wasm-wasmfx) @+14
```

最後の FAIL は想定どおりである(フラグなしでは継続の型を拒む)。

印字は 2 通り試した。
A は GC 配列をそのまま JS に渡し、JS が wasm から書き出した `bytes_len`・`bytes_get` で 1 バイトずつ読んで `fs.writeSync(1, ...)` する。
B は wasm が線形メモリに写し、`(ptr, len)` を渡す。
JS から GC 配列の中身は見えない(`keys=[]`、`[0]` も `length` も `undefined`)ので、どちらかの手段が要る。

### スタック切り替え(`--experimental-wasm-wasmfx`)

```
$ node --experimental-wasm-wasmfx fx.mjs
PASS fx: sum(10)=55 (expect 55), sum(1e6)=1784293664 (expect 1784293664) in 63.8 ms
PASS fx_encoder: sum(10)=55 (expect 55), sum(1e6)=1784293664 (expect 1784293664) in 64.0 ms

$ node --experimental-wasm-wasmfx fx2.mjs        # 終了コード 133
PASS bind_test: 42
PASS resume_throw_test: 507
# Fatal error in , line 0
# unimplemented code

$ node --experimental-wasm-wasmfx fx3.mjs        # 終了コード 133
validate: true
compiled
# Fatal error in , line 0
# unimplemented code

$ node --experimental-wasm-wasmfx --no-wasm-lazy-compilation fx3.mjs   # 終了コード 133
validate: true
# Fatal error in , line 0

$ node --experimental-wasm-wasmfx --liftoff-only fx.mjs
# --liftoff-only: treating bailout as fatal error. Cause: unimplemented Liftoff instruction: cont.new
```

- `cont.new`、`suspend`、`resume`(`on $tag $label`)、`cont.bind`、`resume_throw` は動く。`--no-liftoff` でも同じ結果である。
- `fx2.mjs` の `switch_test` は `switch` 命令を含まず、`resume` の処理表に `(on $sw switch)` を書いただけで落ちる。
- `switch` 命令(`fx3`)も落ちる。遅延コンパイルを止めると `compiled` の前に落ちるので、TurboFan のコード生成の段階で未実装である。
- Liftoff は `cont.new` を実装していない。既定では Liftoff が退避して TurboFan で翻訳する。
- 落ち方は V8_Fatal によるプロセスの異常終了であり、JS の例外として捕まえられない。

V8 が受け付けた符号は wasm-encoder 0.262.0 が出すものと同じである。`xxd -g1 out/fx_encoder.wasm` での位置を示す。

| 構文 | 符号 | `fx_encoder.wasm` の位置 |
| --- | --- | --- |
| 継続の型 `(cont $ft)` | `0x5d` 型番号 | 0x0e: `5d 00` |
| `suspend $tag` | `0xe2` タグ番号 | 0x53: `e2 00` |
| `cont.new $ct` | `0xe0` 型番号 | 0x69: `d2 00 e0 01`(直前は `ref.func 0`) |
| `resume $ct (on $tag $l)` | `0xe3` 型番号 個数 (`0x00` タグ ラベル)* | 0x75: `e3 01 01 00 00 00` |

wasm-encoder の残りの符号は `cont.bind` が `0xe1`、`resume_throw` が `0xe4`、`resume_throw_ref` が `0xe5`、`switch` が `0xe6` である(`wasm-encoder-0.262.0/src/core/instructions.rs:4538-4608`)。
このうち `cont.bind` と `resume_throw` は V8 で動いた。

### 末尾でない再帰の深さ(`depth.mjs`)

```
$ node depth.mjs
max non-tail depth ~ 15689
$ node --stack-size=7800 depth.mjs
max non-tail depth ~ 126952
$ node --stack-size=65500 depth.mjs      # 終了コード 139(SIGSEGV)
```

`--stack-size` を OS のスタック(8 MiB)より大きくすると、V8 の検査より先に OS のスタックが尽きて落ちる。

### 層ごとの末尾呼び出し(`tier.mjs`、`even(1e7)`)

| フラグ | return_call | return_call_ref |
| --- | --- | --- |
| なし(2 回) | 3.3 / 3.2 ms | 6.5 / 6.5 ms |
| `--liftoff-only` | 39.0 ms | 44.2 ms |
| `--no-liftoff`(3 回) | 2.6〜2.8 ms | 31.8〜32.1 ms |

どの層でも 1000 万段の相互末尾再帰はスタックを溢れさせない。

### UTF-8 の文字列組み込み(`utf8.mjs`)

```
$ node utf8.mjs
FAIL TypeError: WebAssembly.instantiate(): Import #0 "wasm:text-decoder": module is not an object or function
$ node --experimental-wasm-imported-strings-utf8 utf8.mjs
PASS builtin text-decoder: "Kあ" (builtin used: true)
```

`wasm:js-string`(i16 配列から JS 文字列)はフラグなしで使える。
i8 配列を UTF-8 として解く `wasm:text-decoder` はフラグが要る。

### V8 のフラグの既定値

`node --v8-options` によると、`--experimental-wasm-exnref` と `--experimental-wasm-legacy-eh` は既定で有効、`--experimental-wasm-wasmfx` と `--experimental-wasm-imported-strings-utf8` は既定で無効である。
JSPI のフラグは一覧に無い(出荷済みのため)。

## 書いている途中で詰まった点

いずれも仕様どおりの振る舞いであり、V8 や crate の誤りではない。

- 関数型が後ろの継続の型を参照するときは、2 つを `rec` で囲む必要がある。囲まないと wasmparser は `type index out of bounds` を出す。
- `rec` の関数型を `cont.new` に渡す関数は `(type $fs)` で型を明示する。引数と結果を書くだけでは別の型になり、`cont.new[0] expected type (ref null 4), found ref.func of type (ref 7)` で拒まれる。
- `(on $y $l)` のラベルが受け取る継続の型は、タグ `$y` の結果を引数に取る。`(tag $y (result i32))` なら `(cont (func (param i32) ...))` であり、違えると V8 は `Type mismatch between cont parameters and tag returns` を出す。
- wasm-encoder 0.262.0 の `SubType` の欄は `supertype_idxs: Vec<u32>`、`CompositeType` は `shared`・`descriptor`・`describes` を持つ。API は版ごとに変わるので版を固定する。
