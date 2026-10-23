# 下調べ: LLVM ORCv2 を Rust から(spike-llvm)

試作の所在: doc/log/261024-1-spike/llvm(Cargo.toml、src/main.rs 571 行、README.md、inkwell-check/、gc/、sig/)。以下は同じ README の内容に、計画への示唆を足したものである。


RustKeleut の LLVM 後段の前提を確かめるための小さな試作である。
llvm-sys 221.1.1 を通して、この機械のシステムの LLVM 22.1.8 の C API(ORC LLJIT)を呼ぶ。
計画 261024-1 の判断材料であり、RustKeleut の実装そのものではない。

## 環境(2026-10-24 に測った)

| 項目 | 値 |
| --- | --- |
| OS | Arch Linux on WSL2(カーネル 6.18.54.1-microsoft-standard-WSL2)、32 コア |
| Rust | rustc 1.98.1 (48a229cea 2026-09-01) (Arch Linux rust 1:1.98.1-1) |
| LLVM | `llvm-config --version --shared-mode --libdir` → `22.1.8` / `shared` / `/usr/lib` |
| 共有ライブラリ | `/usr/lib/libLLVM.so.22.1`(ほかに `libLLVM-15.so` もあるが使われない) |

## 構成

- `Cargo.toml`、`src/main.rs` — 試作の本体。依存は `llvm-sys = { version = "=221.1.1", features = ["prefer-dynamic"] }` だけである。
- `inkwell-check/` — inkwell 0.11.0 が LLVM 22 で組めるかを見る別の小さな crate。
- `gc/`、`sig/` — `llc` と `opt` で GC 戦略と musttail の型の規則を見るための IR。
- 実行すると `target/ir/*.ll` に、JIT に渡した IR の文面が書き出される。`llc -O2` で機械語を見るのに使う。

## 手順

```sh
cd doc/log/261024-1-spike/llvm
cargo build --release          # 2.3 秒(llvm-sys の build.rs は llvm-config を PATH から探す)
ldd target/release/orc-spike | grep -i llvm
./target/release/orc-spike     # 全実験。引数で basic callback tail tohost tier gc timing を選べる
llc -O2 -o - target/ir/tail_tailcc_xA.ll
llc -O2 -o - target/ir/tohost.ll
for s in shadow-stack statepoint-example coreclr ocaml erlang nonexistent-gc; do llc -O2 -o /dev/null gc/$s.ll; echo "$s exit=$?"; done
opt -passes=rewrite-statepoints-for-gc gc/statepoint-rewrite.ll -o - | llc -O2 -o -
for f in sig/*.ll; do opt -passes=verify -disable-output $f; llc -O2 -o - $f | grep TAILCALL; done
(cd inkwell-check && cargo build --release && ./target/release/inkwell-check)
```

## 結果

### crate の版

- `cargo info llvm-sys@221` → `version: 221.1.1 (latest 231.0.1)`。機能に `prefer-dynamic`、`force-dynamic` などがある。LLVM 22 に合うのは 221.x である(231.x は LLVM 23 用)。
- `prefer-dynamic` で組むと `ldd` は `libLLVM.so.22.1 => /usr/lib/libLLVM.so.22.1` を示す。実行時の `LLVMGetVersion` は `22.1.8` を返した。環境変数(`LLVM_SYS_221_PREFIX`)は要らなかった。
- inkwell 0.11.0 には `llvm22-1`(と `llvm22-1-prefer-dynamic` など)の機能があり、中身は `llvm-sys 221.1.1` である。`inkwell-check` は組めて `inkwell MCJIT add1(41) = 42` を出した。
- ただし inkwell 0.11.0 の JIT は MCJIT の `ExecutionEngine` だけで、LLJIT(ORCv2)の包みはない。`grep -rn 'lljit\|LLJIT' src` は 0 件で、`execution_engine.rs` の Orc は `#[cfg(feature = "experimental")]` の旧 ORCv1 の API(`llvm_sys::orc`、現在の llvm-sys にはない)である。
- LLVM 21 以降の C API には `LLVMOrcThreadSafeContextGetContext` がない(`/usr/include/llvm-c/Orc.h` にも llvm-sys 221 にもない)。代わりに `LLVMContextCreate` で作った文脈を `LLVMOrcCreateNewThreadSafeContextFromLLVMContext` で包む。試作は 1 つの模塊に 1 つの文脈を使った。

### 実験の出力(`./target/release/orc-spike`、抜粋)

```
LLVM runtime version 22.1.8
== basic
triple = x86_64-pc-linux-gnu
add1(41) = 42
create LLJIT 0.482 ms / parse 0.649 ms / add 0.004 ms / first lookup (compile+link) 2.690 ms / second lookup 0.003 ms
== callback
cb_entry(3) = 46 (expected 46: 4 JIT->Rust->JIT round trips, leaf 21*2=42, +1 per level)
IRBuilder build 0.019 ms / lookup+compile 2 fns 1.635 ms
== tail
tailcc musttail, same module: depth 10000000 result 1; ... growth 16 bytes (0.00 B/level) on a 256 KiB thread stack
ccc musttail, same module: depth 10000000 result 1; ... growth 16 bytes (0.00 B/level) on a 256 KiB thread stack
tailcc musttail, two modules: depth 10000000 result 1; ... growth 16 bytes (0.00 B/level) on a 256 KiB thread stack
ccc musttail, two modules: depth 10000000 result 1; ... growth 16 bytes (0.00 B/level) on a 256 KiB thread stack
baseline plain call (no tail): depth 1000 result 1; ... growth 16016 bytes (16.02 B/level) on a 256 KiB thread stack
tailcc -> ccc musttail: rejected: verify: cannot guarantee tail call due to mismatched calling conv
== tohost
host_tail sp: direct 0x7fff8af76b4f, via musttail 0x7fff8af76b4f (diff 0), via plain call 0x7fff8af76b3f (diff 16)
== tier
1->1(interp) 2->4(interp) 3->9(interp) [tier-up 0.705 ms] 4->16(jit) 5->25(jit) 6->36(jit)
== gc
define llvm_gc_root_chain as Rust absolute symbol: Ok(())
lookup llvm_gc_root_chain = Ok("0x5cba6f3ddee0") (Rust ROOT_HEAD at 0x5cba6f3ddee0)
gc_outer(0x10000000) = 0x10000008 (expected 0x10000008 if the walker updated the outer root)
  walker saw frame0 root0 = 0x1000
  walker saw frame1 root0 = 0x10000000
ROOT_HEAD after return = 0x0
== timing
no IR passes: parse+add+compile+link per module: min 0.854 ms median 0.900 ms p90 0.963 ms max 1.163 ms; fib(32) run 6.428 ms
default<O2>: parse+add+compile+link per module: min 1.872 ms median 1.957 ms p90 2.044 ms max 3.084 ms; fib(32) run 4.175 ms
```

各実験の中身と読み方は次のとおりである。

- **basic**: `LLVMOrcCreateLLJIT`、`LLVMParseIRInContext2`(メモリ上の文面)、`LLVMOrcLLJITAddLLVMIRModule`、`LLVMOrcLLJITLookup` が通った。翻訳は最初の lookup で起こる。最初の lookup の 2.7 ms には初回だけの初期化が含まれる(timing の 2 つ目以降の模塊では 1 個 0.9 ms)。
- **callback**: IR の文面ではなく IRBuilder の C API で模塊を作った。Rust の `extern "C" fn host_cb` は `LLVMOrcAbsoluteSymbols` と `LLVMOrcJITDylibDefine` で絶対記号として定義した(旗は Exported | Callable、名前は `LLVMOrcLLJITMangleAndIntern`)。Rust → JIT → Rust → JIT … と 4 往復して正しい値 46 を返した。
- **tail**: 偶奇の相互再帰を深さ 1000 万で走らせた。スタックが 256 KiB しかない専用のスレッドで走らせ、底と入口でスタックの番地を記録した。4 通り(tailcc と ccc、同じ模塊と 2 つの模塊(別々の LLVMContext))すべてで深さに関わらず 16 バイト(入口の 1 枠)しか伸びなかった。比較のための普通の call は 1 段 16 バイト伸びるので、1000 万段なら約 160 MB 要る。`llc -O2` の出力では `jmp tx_odd@PLT  # TAILCALL` である。各組の翻訳は 1.4〜1.9 ms、実行は 2.0〜2.1 ms(スレッドの起動を含む)だった。
- **tohost**: JIT の ccc 関数から Rust の `extern "C" fn host_tail` を `musttail` で呼べた。`host_tail` が見たスタックの番地は、Rust から直に呼んだときと同じだった(差 0)。普通の call では 16 バイト深い。機械語は `jmp host_tail@PLT  # TAILCALL` である。一方、tailcc の関数から ccc の関数への musttail は検証器が `cannot guarantee tail call due to mismatched calling conv` で拒んだ。
- **tier**: Rust が持つ表 `dispatch`(`[AtomicUsize; 4]`)を、Callable でない絶対記号として JIT に見せた。JIT の `call_slot0` は表の 0 番を読んで `musttail` で間接に呼ぶ。最初は 0 番が Rust の `interp_square`(解釈器の代役)を指し、3 回呼ばれたら `jit_square` を JIT で作って 0 番を書き換えた。4 回目からは JIT のコードが走った。書き換えまでの JIT の時間は 0.7 ms である。
- **gc**: `gc "shadow-stack"` の関数を 2 つの模塊に分けて JIT した。`llvm_gc_root_chain` を Rust の静的変数の番地として先に絶対記号で定義しておくと、ShadowStackGCLowering が各模塊に作る linkonce の定義は捨てられ、両方の模塊が Rust の鎖の頭を共有した(lookup の番地が Rust の番地と一致する)。Rust の模擬の回収器は 2 つの枠の根を見つけ、根を +8 だけ書き換え、JIT のコードは書き換えた値を読み戻した。戻ったあとの鎖の頭は 0 である。

### 小さな関数の JIT の時間

`fib` 1 個だけの模塊を 200 個、同じ LLJIT に順に足し、文面の解析から lookup(翻訳と結合)までの時間を測った。3 回走らせた値はほぼ同じだった。

| IR の最適化 | 最小 | 中央値 | p90 | 最大 | fib(32) の実行 |
| --- | --- | --- | --- | --- | --- |
| なし(コード生成は LLJIT の既定の水準) | 0.83〜0.85 ms | 0.87〜0.90 ms | 0.92〜0.98 ms | 1.2〜2.0 ms | 6.3〜6.4 ms |
| `LLVMRunPasses(m, "default<O2>")` | 1.78〜1.88 ms | 1.92〜1.96 ms | 1.99〜2.09 ms | 3.0〜3.1 ms | 4.1〜4.2 ms |

LLJIT の作成は 0.47〜0.50 ms だった。

### GC 戦略(llc)

```
shadow-stack exit=0
statepoint-example exit=0
coreclr exit=0
ocaml exit=0
erlang exit=0
nonexistent-gc exit=134   (LLVM ERROR: unsupported GC: nonexistent-gc)
```

`opt -passes=rewrite-statepoints-for-gc` は `llvm.experimental.gc.statepoint` と `gc.relocate` を作り、`llc` は `.llvm_stackmaps` 節と `__LLVM_StackMaps` を出した。statepoint の JIT での利用(stackmap 節を JIT から受け取ること)は試していない。

### musttail の型の規則(opt -passes=verify)

- ccc で引数の数が違う musttail は `cannot guarantee tail call due to mismatched parameter counts` で拒まれる。
- tailcc なら、1 引数の関数から 8 引数(レジスタに収まらずスタックを使う)の関数への musttail が通り、`jmp g@PLT  # TAILCALL` になる。

### Rust 側からの末尾呼び出し

`become` は安定版にない。rustc 1.98.1 で `become g(n - 1)` を書くと `error[E0658]: \`become\` expression is experimental`(issue #112788)になる。

## 分かったことと分からないこと

測った事実:

1. llvm-sys 221.1.1 + `prefer-dynamic` で、`/usr/lib/libLLVM.so.22.1` に動的に結合して動く。
2. LLJIT の C API(作成、IR 模塊の追加、lookup、絶対記号、ThreadSafeContext)は動く。JIT と Rust の間の呼び出しは両向きに、入れ子にもできる。
3. tailcc でも ccc でも、musttail の相互再帰は同じ模塊でも別の模塊でも一定のスタックで走る。
4. JIT から Rust の `extern "C"` への末尾呼び出しは、呼ぶ側が ccc で型が完全に一致すれば `jmp` になる。tailcc からはできない。Rust から JIT への末尾呼び出しは安定版の Rust ではできない。
5. Rust 側の表を介した間接 musttail と表の書き換えで、解釈器から JIT のコードへの段の切り替えができる。
6. shadow-stack は JIT でも動き、鎖の頭を Rust に置ける。statepoint-example を含む組み込みの戦略は llc で使える。

推測(この試作では確かめていない):

- 解釈器と JIT のコードが末尾位置で互いに飛び合うには、両方の向きで一定のスタックを保つ規約が要る。JIT → 解釈器は「ccc で全関数が同じ型」にすれば musttail で済む。解釈器 → JIT は、解釈器の主ループが JIT の関数を普通に呼び、JIT のコードは解釈器へ末尾で戻る(または「次に呼ぶもの」を返すトランポリン)という形が要る。
- LLJIT がこの機械で JITLink と RuntimeDyld のどちらを使っているかは確かめていない。`@PLT` 経由の呼び出しが模塊をまたいでも一定のスタックだったことは測ったが、どちらの結合器かには依存しないと考えている。
- 例外の巻き戻し(JIT の枠を越えた unwind)、デバッグ情報、遅延翻訳(`LLVMOrcLazyReexports`)、並列翻訳は試していない。
- inkwell で IR を組み、模塊を llvm-sys の ORC に渡すことは、inkwell の `Module`/`Context` の Drop と所有権の受け渡しが衝突するので、包みを自作する必要があると考える。試作は llvm-sys を直に使った。

## 計画への示唆(推測)

- 依存は `llvm-sys = { version = "221.1", features = ["prefer-dynamic"] }` を薄く包む自作の層とする。inkwell は LLJIT を包まないので使わない。llvm-sys の版は LLVM の主版に結び付くので、LLVM 23 に上げるときは 231.x に替える。LLVM 後段は cargo の feature で切り離し、LLVM のない環境でも WasmGC とバイトコード解釈器だけで組めるようにする。
- 呼び出し規約: Keleut の関数を JIT するときは、関数どうしの末尾呼び出しを tailcc + musttail で保証する。解釈器(Rust)との境目には ccc の「全関数が同じ型」の入口(例: `extern "C" fn(ctx: *mut Ctx, args: *mut u64) -> u64`)を置く。JIT → 解釈器はこの型で musttail でき(tohost の実験)、解釈器 → JIT はトランポリンにする(`become` が安定版にないため)。
- 段の切り替えは、関数ごとの入口の表(Rust が持つ)を介した間接呼び出しと、表の書き換えで行う(tier の実験)。ORC の遅延再輸出(LLVMOrcLazyReexports)を使わずに済む。
- GC はまず shadow-stack で根を Rust の回収器に渡す(gc の実験)。statepoint は要らないので後回しにでき、使うなら stackmap 節を JIT から受け取る仕組み(ObjectLinkingLayer のプラグインなど)を別に調べる。
- 翻訳の費用は小さな関数 1 個で約 0.9 ms(IR 最適化なし)、約 1.9 ms(default<O2>)である。段を上げる閾値は、この費用と解釈器の速さの比で決める。
