# 計画 261104-3 の改訂(075805d)の再検証の指摘と反証

観点は changes(改訂で変えた箇所)、design(設計と試験)、decisions(裁定、文案、台帳)の 3 つで、観点ごとに検証者を 1 人立てた。
各指摘に反証者を 2 人立てた。
指摘は 23 件である。
指摘と反証の本文は検証者と反証者が英語で書いたもので、そのまま載せる。
本文の行番号は改訂(075805d)の `doc/log/261104-3-plan.md` の行番号で、再検証の反映の後の計画では動いている。
scratchpad のパスは検証のときのもので、残していない。

## changes-0

- 観点: changes(改訂で変えた箇所)
- 重さ: medium
- 節: 5.1 D575 (launcher script DIR/run); 8 D575 rationale; fix for facts-0/feasible-0
- 反証: 2 人のうち 0 人

### 問題

The new launcher exits 74 whenever fd 1 is closed, before the program runs. `keleut run --backend wasm >&-` exits 74 only when the program actually writes. A program that writes nothing exits 0, and one that hits a runtime error before any output exits 3. So the script does not match `keleut run --backend wasm`, although D575's rationale in section 8 says it does (台本なら keleut run --backend wasm と同じ振る舞いにそろう). The host already has the hook that `keleut run` uses, and the plan does not use it.

### 根拠

Probe with release keleut at HEAD. A silent program (`let x = 1`): `keleut run --backend interp quiet.kel >&-` exits 0, `--backend wasm` exits 0, and `diktor quiet.kel >&-` exits 0. A runtime error before any output (`let x = 1 / 0`) with `>&-` exits 3 on both backends. probe/plan/launcher.sh exits 74 in every case. Record 261024-3 line 3807 says `keleut run --backend wasm` checks before starting Node and passes `KELEUT_STDOUT_CLOSED`. host/keleut-host.mjs:377 does `new Out(process.env.KELEUT_STDOUT_CLOSED === '1')`, and crates/keleut-wasm/src/launch.rs:346 sets it. The plan never mentions KELEUT_STDOUT_CLOSED (grep finds no match).

### 直し方

When `{ true 3>&1; } 2>/dev/null` fails, have the script do `KELEUT_STDOUT_CLOSED=1 exec node .../main.mjs "$@"` instead of printing the message and exiting 74. The host then fails on the first write with the same message, exactly as `keleut run` does. In tests/wasm_cmd.rs, add a silent program and a runtime-error-before-output program under `DIR/run >&-`, and check that the exit status and stderr equal those of `keleut run --backend wasm >&-`.

### 反証者 1: 反証できなかった

I confirmed the finding. The plan's D575 (doc/log/261104-3-plan.md lines 720-734) has the DIR/run script check fd 1 before starting Node and, when fd 1 is closed, print the message and exit 74 without running the program. I ran the release keleut built from HEAD on a silent program (`let x = 1`) with `>&-`. Both `--backend interp` and `--backend wasm` exit 0. With a runtime error before any output (`let x = 1 / 0`), both exit 3 with `実行時エラー: ゼロ除算です`. So an unconditional 74 does not match `keleut run --backend wasm`, and section 8's rationale for D575 (台本なら keleut run --backend wasm と同じ振る舞いにそろう) is false as the plan stands. The tests the plan proposes (`DIR/run >&-` gives 74 and the message) would lock that mismatch in. The hook the finding names exists. keleut-wasm/src/launch.rs:345-349 sets KELEUT_STDOUT_CLOSED=1 when stdout is closed. host/keleut-host.mjs:377 reads it into `new Out(...)`. main.mjs is the same host file with one launch line appended (launch.rs:41-44), so a script that passes `KELEUT_STDOUT_CLOSED=1 exec node main.mjs` would behave like `keleut run`. The plan never mentions KELEUT_STDOUT_CLOSED. The suggested fix and the extra tests (a silent program and an error-before-output program, compared with `keleut run --backend wasm >&-`) are correct.

### 反証者 2: 反証できなかった

The finding holds, so it is not refuted. Section 5.1 of the plan (lines 720-734) has DIR/run print the 74 message and exit 74 whenever fd 1 is closed, before Node starts. The plan never uses KELEUT_STDOUT_CLOSED (grep finds nothing). Measured with the release keleut built from HEAD: `keleut run --backend interp|wasm quiet.kel >&-` (`let x = 1`) exits 0 on both backends, and `err.kel` (`let x = 1 / 0`) exits 3 on both. So keleut run fails with 74 only when the program writes, and the planned script does not match it. That contradicts the D575 rationale at line 989 (台本なら keleut run --backend wasm と同じ振る舞いにそろう). The suggested fix works. launch.rs:346 sets KELEUT_STDOUT_CLOSED=1, and the host at keleut-host.mjs:377 reads it. The main.mjs written by keleut build contains the same line 377. Running `KELEUT_STDOUT_CLOSED=1 node out/main.mjs >&-` on the built quiet program exited 0, and on the built error program it exited 3, matching keleut run. So `KELEUT_STDOUT_CLOSED=1 exec node ...` in the script reproduces keleut run's behaviour. I did not test a program that writes output, because my probe programs failed to compile. Record 261024-3 line 3807 says the same thing.

## changes-1

- 観点: changes(改訂で変えた箇所)
- 重さ: medium
- 節: 4.1 D566 (row 3 of the §15.5 table); fix for spec-1
- 反証: 2 人のうち 0 人

### 問題

The plan widens the condition column of row 3 to 初期化を始めるときか、初期化の途中で. It leaves the 初期化 column at 途中まで行う. For the new case of failing at the very start (the execution thread or the start-up shape-table registration cannot be created), no initialization runs. The row would then contradict itself, and the revision introduces this.

### 根拠

doc/LangSpec.md:1784 `| 実行時エラー | 初期化の途中で実行時エラーが起きた | 途中まで行う | 3 |`. Plan lines 370-374 change only the condition text. Lines 383-384 and 395 put thread-creation failure and start-up registration failure into row 3. Section 6's table (line 875) lists only 表の 1 の行と 3 の行、表の下の 2 文.

### 直し方

Also change the 初期化 column of row 3, for example to 行わないか途中まで行う. Say this in D566, in section 6's table and in P259.

### 反証者 1: 反証できなかった

I confirmed the finding. In doc/LangSpec.md:1784, row 3 of the §15.5 table reads `| 実行時エラー | 初期化の途中で実行時エラーが起きた | 途中まで行う | 3 |`. D566 in the plan (lines 370-374) changes only the condition, to 初期化を始めるときか、初期化の途中で. Line 395 puts thread-creation failure and start-up shape-table registration failure into row 3. The plan's own rationale (line 393) says that in these cases 初期化を1つも行っていない. Its 初期化 column would still say 途中まで行う, while rows 1 and 2 use 行わない for the no-initialization case, so the revised row would contradict itself. A grep shows the plan never mentions the 初期化 column (no 途中まで, no 行わないか). Section 6's table (line 875), P259 (line 1057) and the spec-1 resolution (line 1117) mention only the condition text. The suggested fix is correct: also change the 初期化 column of row 3, for example to 行わないか途中まで行う, and record this in D566, in the section 6 table and in P259.

### 反証者 2: 反証できなかった

The finding holds. doc/LangSpec.md:1784 has row 3 as `| 実行時エラー | 初期化の途中で実行時エラーが起きた | 途中まで行う | 3 |`. In D566 (plan lines 370-374) only the 条件 text changes, to 「初期化を始めるときか、初期化の途中で実行時エラーが起きた」. Lines 384, 395 and 536 put thread-creation failure and start-up shape-table registration failure into row 3. Line 403 itself says these cases do no initialization at all (「初期化を 1 つも行っていない」). Grep finds no change to the 初期化 column anywhere in the plan. Section 6's table (line 875), the 裁定 row (line 980) and P259 (line 1057) also mention only the 条件 widening. So the widened row would say 「途中まで行う」 for a case the plan says runs no initialization, and the revision causes this. Changing the 初期化 column to something like 「行わないか途中まで行う」 is a sound fix. It should go into D566, the section 6 table and P259.

## changes-2

- 観点: changes(改訂で変えた箇所)
- 重さ: medium
- 節: 4.5 M111 test (injection after cont_resume failure); fix for tests-3
- 反証: 2 人のうち 0 人

### 問題

The plan expects the injection test to fail on code that has only the chunk reclamation. That holds only when the continuation needs more than one 1 MiB chunk. When cont_resume's chunk allocation fails, take_mem arms the preallocated spare chunk. The pre-fix injection then gets that armed chunk from take_mem without any allocation, so `exec:chunk:hard:N` does not stop it, and `inner cancel` still runs. The plan sets no size for the continuation, so the required pre-fix failure may not show, or may depend on the depth chosen.

### 根拠

crates/keleut-rt/src/stack.rs:127-143: take_mem returns `self.armed` before calling new_mem, and arms `spare` on failure. Plan line 639 itself relies on this armed spare being usable after the cont_resume failure. Record 261024-3 lines 2746-2747 limit the problem to large continuations: 予備(1 MiB)に入らない大きな継続では起こりうる. Plan lines 652-655 give no continuation size.

### 直し方

Require that the captured continuation span well over one chunk (for example a capture depth giving more than 131,072 words, such as continj.kel at DEPTH 3,000,000). Alternatively, make the test consume the armed spare first. State the condition in 4.5 and in the M111 acceptance.

### 反証者 1: 反証できなかった

I confirmed the finding. In crates/keleut-rt/src/stack.rs:127-143, take_mem hands out `armed` before it calls new_mem, and when new_mem fails it arms `spare` (CHUNK_WORDS = 1<<17 words, stack.rs:42). cont_resume (crates/keleut-interp/src/interp.rs:1588-1623) places the frames with stack.place, which reaches take_mem. So once the `hard` chunk failure hits cont_resume, the 1 MiB spare is armed. The pre-fix injection that follows then gets that spare from take_mem without calling new_mem, so the `chunk` site never fails for it. Plan line 521 and lines 638-639 rely on exactly this: the value-stack spare chunk stays usable under `hard`. As a result, the pre-fix injection fails only when the continuation needs more than the 1 MiB spare. Record 261024-3 lines 2746-2747 and line 5672 say the same: 予備の 1 MiB に入らない大きな継続. Plan lines 652-655 give no continuation size or depth, and continj.kel appears nowhere else in the plan or the repo. A small continj therefore would not show the required failure on code with only the chunk reclamation. The plan's own step (直す前のコードで落ちることを確かめる) would reveal this during implementation, but the plan does not state the condition, so the suggested fix stands. The fix is to require a capture well over 131,072 words, or to consume the armed spare first, and to state this in 4.5 and in the M111 acceptance.

### 反証者 2: 反証できなかった

I confirmed the problem. In stack.rs:127-143, take_mem hands back `self.armed` before it calls new_mem, and it arms `spare` (one CHUNK_WORDS = 2^17-word, 1 MiB chunk) only after an allocation fails. The `hard` mode of KELEUT_TEST_ALLOC_FAIL makes allocations fail. Handing back the armed spare is not an allocation, so `exec:chunk:hard:N` cannot stop it. The plan relies on this behaviour itself at line 639. On the pre-fix code (chunk reclamation only), the cont_resume failure arms the spare, and the injection then starts from the same position with one more chunk of room. Whether the injection also fails therefore depends on how large the continuation is. Record 261024-3 line 2747 says the same thing: 予備(1 MiB)に入らない大きな継続では起こりうる. Plan 4.5, lines 652-655, gives no size for the continuation. The probe doc/log/261104-3-spike/probe/alloc/continj.kel leaves DEPTH as a parameter. The M111 acceptance, line 930, only says that the injection test must fail on reclamation-only code. So the required pre-fix failure (inner cancel lost) may not show up. The suggested fix is correct: require a capture depth whose continuation spans well over one chunk plus the kept next chunk, or make the test use up the armed spare first, and state this condition in 4.5 and in the M111 acceptance.

## changes-3

- 観点: changes(改訂で変えた箇所)
- 重さ: low
- 節: 4.2 試験用の失敗 (hard in the conversion role); 4.4 test `conv:check:hard:0`; fix in 075805d
- 反証: 2 人のうち 0 人

### 問題

The 075805d fix says that `hard` in the conversion role exercises the reserve-exhausted path (limit line, then 1 or 65). The hard-mode definition contradicts this. Hard fails every retry and 予備には触れない, so the reserve is never released and is never empty. 呼び手は失敗を受け取る. The conversion-role exit at line 456 fires only when やり直しも失敗するか予備が無ければ. Read literally, a conv-role hard failure returns null to a Vec, and handle_alloc_error stops the process with 134, not with the expected fixed line and 1.

### 根拠

Plan lines 456-457 (conv exit condition), 499-500 (hard: no reserve, caller receives failure), 502 (conv hard passes the exhausted path), and 609 (expects 包みの書く固定の行と 1).

### 直し方

Define it explicitly: in the conversion role, a hard-mode failure outside a self-handled region goes straight to the conversion-role exit (fixed line, then _exit with 1 or 65), as if the reserve were exhausted.

### 反証者 1: 反証できなかった

I confirmed the problem by reading the plan. Line 499 says `hard` fails every retry and does not touch the reserve (予備には触れない), and line 500 says the caller receives the failure. The conversion-role exit at lines 456-457 lives inside the wrapper's reserve path. It fires only when the retry also fails or no reserve is left (やり直しも失敗するか予備が無ければ). As defined, a hard failure never touches the reserve, so it never reaches that exit. The wrapper just returns null, and line 449 says a caller like `Vec` that cannot handle a failure then stops through `handle_alloc_error` with 134. That contradicts line 502 (conversion-role `hard` goes through the reserve-exhausted path: the limit line, then 1 or 65) and the test at line 609 (`conv:check:hard:0` expects the wrapper's fixed line and exit 1). Nothing else in the plan says how `hard` in the conversion role reaches the exit. The suggested fix, to state that a conversion-role hard failure outside a self-handled region goes straight to the conversion-role exit, would resolve the contradiction. Severity is low: the intent is clear, but the text is internally inconsistent.

### 反証者 2: 反証できなかった

I confirmed the problem in the plan text. Lines 456-457 say the conversion-role exit (limit line, then _exit with 1 or 65) fires only when "やり直しも失敗するか予備が無ければ". Line 499 defines hard mode as failing every allocation from SKIP+1 on, retries included, without touching the reserve ("予備には触れない"). Line 500 then says the caller receives the failure ("呼び手は失敗を受け取る"). Line 502 says conv-role hard takes the reserve-exhausted path, and line 609 (M110 test conv:check:hard:0) expects the wrapper's fixed line and exit 1. The text can be read two ways. If hard bypasses the reserve logic and hands null back, the `check` site's Vec callers hit handle_alloc_error and the process exits 134, not 1. If the wrapper's "retry also failed" check still applies under hard, the conv exit fires, but then line 500 ("caller receives failure") is false for the conv role. Nothing in the plan says which reading is meant. The finding is right that the plan should say explicitly that in the conv role, a hard failure outside a self-handled region goes straight to the conversion-role exit, as if the reserve were exhausted. It is right that the severity is low: the plan's intent is clear, but the mechanism as written contradicts it.

## changes-4

- 観点: changes(改訂で変えた箇所)
- 重さ: low
- 節: 5.1 D575 and the M112 acceptance (dash)
- 反証: 2 人のうち 0 人

### 問題

M112's acceptance says to try the launcher's check under dash as well and to record the result. Section 5.1 defers the dash check to M112. dash is not installed on this machine, and /bin/sh is bash. As written, the acceptance cannot be carried out here.

### 根拠

`which dash` gives `dash not found`. `readlink -f /bin/sh` gives /usr/bin/bash. busybox, ksh, mksh and posh are also absent. Under `sh`, the launcher gives 74 with `>&-` and 0 with `>/dev/null` (that is bash only).

### 直し方

State how the dash check is to be done (install dash, or use a container), or replace it with a POSIX-mode shell that is available, such as `zsh --emulate sh` or `bash --posix`. Mark the result as untested on dash if none is available.

### 反証者 1: 反証できなかった

I confirmed the problem. Section 5.1 (line 732) says "dash では試していないので、M112 で確かめる". M112's acceptance (line 931) says "台本の確かめ方を dash でも試し、結果を記録に書く". Neither place says how to get dash, offers a substitute shell, or says what to do if dash is missing. On this machine `which dash` finds nothing. busybox, ksh, mksh and posh are also missing, and /bin/sh resolves to /usr/bin/bash, so a check run as `sh` only tests bash again. Installing dash (apt or a container) would need a package install that the plan does not mention. So the acceptance cannot be met here as written, and the plan should say how the check is to be done, or allow recording it as untested on dash. The finding stands, at its low severity.

### 反証者 2: 反証できなかった

I confirmed the problem. Plan line 732 says "dash では試していないので、M112 で確かめる", and the M112 acceptance (line 931) says "台本の確かめ方を dash でも試し、結果を記録に書く". dash, busybox, ksh, mksh and posh are all missing on this machine. /bin/sh resolves to /usr/bin/bash. The only other shell available is zsh. There is no route to install dash either: apt-get, docker, podman and nix are all absent. Nowhere does the plan say how dash is to be obtained, what to use in its place, or what to record if it cannot be had. As written, this acceptance step cannot be carried out here. The suggested fix is reasonable: name a stand-in such as bash --posix or zsh in sh emulation, or say that dash is to be recorded as untested. The finding is low severity.

## changes-5

- 観点: changes(改訂で変えた箇所)
- 重さ: low
- 節: 4.2 D568 item 1 (conversion-role _exit rationale)
- 反証: 2 人のうち 2 人

### 問題

The rationale says stopping with _exit in the conversion role loses nothing, because there is no program output yet. This is not true for every command whose front-end thread has the conversion role. `keleut check` writes the inferred types to stdout as it goes, and `keleut emit-kir` without -o writes KIR to stdout. A raw _exit drops anything still buffered. The normal type-error path prints those lines and then exits 1.

### 根拠

Probe: `keleut check bad.kel` (line 1 is `let x = 1`, line 2 has a type error) prints `x : Int32` to stdout and the error to stderr, then exits 1. Plan line 458: 変換の役割にはまだ cancel 節もプログラムの出力も無いので、ここで止めても失うものは無い.

### 直し方

Limit the claim to `keleut run`. For `check`, `emit-kir` and `dump`, either say that buffered stdout may be lost on this path (§15.5 leaves the output of non-running modes to the implementation), or flush stdout from a non-allocating path before _exit.

### 反証者 1: 反証した

The mechanism the finding describes does not exist in the code. `keleut check` writes nothing to stdout while it runs. In crates/keleut-cli/src/check_cmd.rs:161-199, run_check gathers every inferred-type line and the error text into a local `Vec<u8>`. It hands that buffer to stdout with one write_all and a flush only after type checking has finished. So there is no stdio buffer holding output that a raw `_exit` could drop. A hard allocation failure during type checking just means the type lines are never produced. That is the same result as any failure that happens before the final write.

The probe shows a different path: a type error, where the run finishes and the Vec is written. It does not show output being lost under `_exit`.

For `emit-kir` without `-o`, an `_exit` during conversion can only cut off a KIR stream on a run that exits 1 or 65 anyway. A partial KIR stream is useless, and the plan already says the run fails.

The plan's sentence at line 458 says "プログラムの出力", which means the output of the Keleut program. None of that exists in the conversion role, so the sentence is accurate. The plan also states what the hard path shows: only the wrapper's fixed line and exit 1 (line 609, `conv:check:hard:0`). Partial checker output is not part of the contract.

The suggested fix, flushing stdout before `_exit`, would do nothing for `check`, because no stdio buffer holds data at that point. At most the wording could be tightened slightly. There is no substantive error.

### 反証者 2: 反証した

I checked the code and the finding does not hold. The plan says the conversion role has no *program output*. That means output from the Keleut program, and it is true for every command: no Keleut code has run while the front-end thread or the KIR-processing thread is working. The finding's evidence also misreads how check and emit-kir write their output. Neither command writes to stdout from the front-end thread. check_cmd.rs:161-194 (run_check) collects the type lines (`x : Int32`) and the error line in a Vec. It writes them in one write_all only after check_program has returned. In emit_kir_cmd.rs, build() runs on the front-end stack via build_on_front_stack. It returns a kir::Program, and the bytes are written later on the main thread (write_all at :262). So while the conversion role is active, the stdout buffer holds nothing that a raw _exit could drop. The suggested fix (flush stdout before _exit) would do nothing. The only thing lost is the list of type lines from a type check that the OOM cut off. The tool already throws such lines away when type checking ends abnormally. For example, the out.panic path in run_check (check_cmd.rs:157-160) prints the panic and returns 3 without writing the collected lines. A memory-limit stop in the middle of conversion ending the same way matches this. The probe in the finding only shows the normal type-error path, which writes everything after checking ends. That path does not involve the role in question. At most the plan's wording "失うものは無い" could name the discarded partial type lines, but the plan's claim is correct and the proposed change would not be.

## changes-6

- 観点: changes(改訂で変えた箇所)
- 重さ: low
- 節: 4.2 M108 test (call_args site); fix for feasible-1/tests-4
- 反証: 2 人のうち 0 人

### 問題

The plan says the `call_args` example 'disappears in M114', so it will be deleted or retargeted then. M114's change is conditional: D579 item 4 commits the stage-1 prototype only if it is 5 % faster. If it is not committed, the site stays and the instruction to remove or replace the example is wrong.

### 根拠

Plan line 518 `call_args の例は M114 で場所が無くなるので、M114 で消すか別の場所に替える`. Plan lines 840-842: commit only on a 5 % or larger gain, otherwise restore the working tree.

### 直し方

Make it conditional: only if M114's prototype is committed and removes the call_args allocation, delete or retarget that example.

### 反証者 1: 反証できなかった

I confirmed the problem, so the finding stands. Plan line 518 says without condition that the call_args example "disappears in M114, so in M114 delete it or move it to another site". Line 1118 also says without condition "call_args の例は M114 で替える". But M114's change is conditional. D579 item 4 (lines 840-842) and the M114 row of table 7.2 (line 933) commit the stage-1 prototype only when gen or tailresume gets at least 5 % faster and nothing else gets 5 % slower. Otherwise the working tree is restored. In that case the call_args allocation site stays, and deleting or retargeting the example would be wrong. Even if M114 is committed, a reusable buffer (使い回す領域) may still allocate when it grows, so the site does not certainly disappear. Lines 518 and 1118 should both say the example is replaced only when M114 is committed and actually removes the call_args allocation.

### 反証者 2: 反証できなかった

I confirmed the finding. Plan line 518 says without condition that the `call_args` site has no place after M114 ("M114 で場所が無くなる") and must be deleted or moved then. Line 1118 repeats this as "`call_args` の例は M114 で替える".

M114's change is conditional, though. D579 item 4 (lines 840-842) commits the stage-1 prototype only when `gen` or `tailresume` gets at least 5 % faster under `kt1000` and no other benchmark gets 5 % slower. Otherwise the working tree is restored. The M114 row in table 7.2 (line 933) says the same: "満たさなければコミットせず値を記録に書く".

If the prototype is not committed, the `call_args` `Vec` allocation in `krt_fx` stays, so the example stays valid, and the instruction to remove or replace it is wrong.

Nothing else in the plan makes this conditional. The suggested fix is correct: only when the M114 prototype is committed and removes the `call_args` allocation should the example be deleted or moved. One possible extra nuance: even a reused buffer can still allocate when it grows, so the site might not vanish entirely. That only adds to the case for making the wording conditional.

## changes-7

- 観点: changes(改訂で変えた箇所)
- 重さ: low
- 節: 4.1 D567 item 2 versus the 13.9 exclusion
- 反証: 2 人のうち 0 人

### 問題

D567 item 2 puts every Rust allocation failure after the reserve is exhausted, in the interpreter and LLVM back ends, under SIGABRT 134. For a `.kir` input, the same plan sends allocation failures while reading, verifying or lowering to 65 (the conversion-role wrapper exit and the new 13.9 exclusion). The two IRSpec texts give different statuses for the same reader stage.

### 根拠

Plan line 413 (item 2, 134) versus lines 397-398, 423 and 457 (`.kir` read, verify and lower allocation exhaustion → 65).

### 直し方

Limit item 2 to allocations during execution (after the reader has finished reading and verifying), and refer to the 13.9 exclusion for the reading stage.

### 反証者 1: 反証できなかった

The overlap is real, and nothing else in the plan resolves it. D567 item 2 (plan line 413) says that in the interpreter and LLVM back ends, a Rust allocation that fails after the global allocator's reserve is used up ends in SIGABRT 134. The item is defined by back end, not by stage. But D568 (lines 456-458, 474-475) has the conversion role, which includes KIR reading, verification and lowering, end the process with the limit line and 65 for `.kir` input. The new 13.9 exclusion (line 423) and line 398 say the same: allocation exhaustion in read, verify or lower is 65. Under `keleut run FILE.kir --backend interp`, the same failure would then be covered by both item 2 (134) and 13.9 (65) in IRSpec. The plan never says that 13.9 takes precedence. The suggested change needs one adjustment. Line 478 deliberately puts main-thread failures after the reserve is used up into item 2 as the 134 window. That includes argument parsing and file reading, which happen before the reader's work. Line 461 does the same for the `keleut repl` conversion role. So item 2 should not literally say "only during execution". It should exclude the conversion-role cases that 13.9 covers (the KIR processing thread for `.kir` input, which ends with 65), or point to them, and keep the main-thread, REPL and LLVM-internal windows. With that change, the finding stands as a low-severity consistency fix.

### 反証者 2: 反証できなかった

The finding holds; this is low severity. Plan line 413 (D567 item 2) gives the IRSpec §1 exclusion the wording "RustKeleut のインタプリタと LLVM の後段で … 大域の確保器の予備を使い切った後に Rust の確保が失敗する場合(SIGABRT、134)". That text has no limit to the execution stage. Lines 398 and 457, item 2 at line 579 and the new 13.9 exclusion (line 423) say that for `.kir` input, running out of allocation while reading, verifying or lowering ends with 65. The `.kel` case ends with 1, and only the REPL's conversion role keeps 134 (lines 461, 595). Under D568, the conversion role of a `.kir` run never reaches 134. The planned IRSpec wording does not show this, because IRSpec does not talk about roles. Lowering to LIR (keleut-lir) is part of the interpreter and LLVM back ends' pipeline, so a reader of IRSpec can take both texts to cover the same failure and get 134 from one and 65 from the other. The plan does not say anywhere which text wins. Nothing in it limits item 2 to execution after reading and verification finish. The suggested change fits D568 and is correct: limit item 2 to allocations made during execution, and point to the 13.9 exclusion for the reading, verification and lowering stage.

## design-0

- 観点: design(設計と試験)
- 重さ: high
- 節: 3.1 tests (Worker-path test) / 7.2 M106 acceptance
- 反証: 2 人のうち 0 人

### 問題

The M106 Worker-path test (keleut build --max-depth 600000, form c NV=92 GADD=1, N=299,000, run with node DIR/main.mjs) already passes before the fix. So the acceptance condition that 'both overlap tests fail on the unfixed keleut' cannot be met. The fallback 'if it does not fail, raise N' is also impossible, because N is capped by L (depth is about 2N, so N can be at most about 300,000 at L=600,000). The host's Worker path adds a 64 MiB margin (host/keleut-host.mjs:767, stackSizeMb = ceil(stackKiB/1024) + 64) on top of (L+R)*B_WASM + 32 MiB. That margin covers form c's excess of about 147 B per level at both L=600,000 and the default L. The plan's statement in 1.1 that the Worker path 'passed the same input' (at --max-depth 1,900,000) therefore says nothing about whether the frame sizes are equal: the Worker path is simply more lenient.

### 根拠

Probe at HEAD 69febfd with release keleut. Built c92m (N=299,000) with `keleut build --target wasm --max-depth 600000` and ran `node outm/main.mjs`: printed 2, bottom, 7 and exited 0. N=299,990 (the depth-limit edge): node main.mjs exited 0, while `keleut run --backend wasm --max-depth 600000` on the same file gave RangeError and exit 70. Default-L c92 (N=510,000) built with `keleut build` and run with node main.mjs: exited 0. The plan's own record has `keleut run --backend wasm` of the same file failing with 70. Arithmetic: Worker stack at L=600,000 is 989 MiB ≈ 1.037 GB, against about 598,000 × 1,683 ≈ 1.006 GB needed.

### 直し方

For the Worker-path test, use a program whose per-level use exceeds what the Worker stack gives. At L=600,000 that is about 1.73 KB per level, for example form f (predicted 2,896 B), measured first. Alternatively, drop the requirement that this test fail before the fix and replace it with a deterministic unit or host test: main.mjs must take its stack size from META's B_prog (for example, run with KELEUT_WASM_STATS or a META inspection) instead of the constant B_WASM. Also correct 1.1 and 9 (risk row 3) so they say the Worker path has a +64 MiB margin and is not evidence that frames are equal.

### 反証者 1: 反証できなかった

I confirmed the finding against the current plan at HEAD 075805d.

Plan section 3.1 (lines 320-322) adds a Worker-path test. It uses form c with NV=92 and GADD=1, built with `keleut build --target wasm --max-depth 600000` and run with `node DIR/main.mjs`. The plan says to confirm it fails before the fix ("直す前に落ちることを確かめ、落ちなければ N を上げる"). The M106 row in 7.2 (line 925) also requires both overlap tests to fail on the unfixed keleut.

Probes on the current release keleut:
- c92m.kel (N=299,000), built at --max-depth 600000 and run with `node main.mjs` without KELEUT_WASM_DIRECT, printed 2 / bottom / 7 and exited 0.
- `keleut run --backend wasm --max-depth 600000` on the same file hit RangeError and exited 70.
- N=299,990 also passed on the Worker path (exit 0).
- At N=300,100 the interpreter already reaches the logical depth limit of 600000, so N cannot be raised. The plan's fallback is impossible.

The cause is in host/keleut-host.mjs:766-767. The stack is (L+R)*B_WASM + 32 MiB, and then +64 MiB on top: `stackSizeMb = ceil(stackKiB/1024) + 64`. At L=600,000 that is about 989 MiB, about 1.037 GB, while form c needs about 598,000 × 1,683 ≈ 1.006 GB.

So the Worker-path test cannot fail before the fix as written, and the acceptance condition cannot be met. Plan 1.1 (line 112) and risk row 3 (line 1003) treat the Worker run as evidence that frames are equal. The extra 64 MiB margin explains that run instead, so those passages also need the correction the finding suggests. Possible fixes are a heavier form such as form f, or a deterministic check that the host takes its stack size from META's B_prog.

### 反証者 2: 反証できなかった

I confirmed the problem, so the finding is not refuted. I reproduced it at the current HEAD 075805d, with the plan's 3.1 text unchanged from the finding's description. I built the plan's own probe (scratchpad work/plan/c92m.kel, form c, N=299000) with `keleut build --target wasm --max-depth 600000` and ran `node out/main.mjs` without KELEUT_WASM_DIRECT. Before any fix it printed 2, bottom, 7 and exited 0. So the Worker-path test in plan 3.1 (lines 319-322) and the M106 acceptance in 7.2 (line 925) cannot meet the condition that both overlap tests fail on the unfixed keleut. The fallback "直す前に落ちることを確かめ、落ちなければ N を上げる" (raise N if it does not fail) also cannot work as written, for two reasons. The test fixes --max-depth at 600000. The logical depth is about 2N, so N cannot go much past about 300,000. The cause is in rustkeleut/host/keleut-host.mjs:766-767: the Worker stack is (L+R)*B_WASM + 32 MiB, plus a further 64 MiB in stackSizeMb. At L=600000 that comes to 989 MiB, about 1.037 GB. The program needs about 598,000 x 1,683 B, about 1.006 GB, so the margin absorbs form c's excess. Plan 1.1 line 112 says the Worker path's frames passed the same input. That run used --max-depth 1900000 with N=510,000, so the stack had plenty of room, and it does not show that the frames are the same size. Risk row 3 (line 1003) leans on the same reading. The suggested fixes are sound. One is a heavier input such as form f, or a larger L near the depth edge where the 64 MiB margin is used up. The other is a deterministic check that main.mjs takes its stack size from B_prog in META. Lines 1.1 and 9 should also say that the Worker path has the extra 64 MiB margin.

## design-1

- 観点: design(設計と試験)
- 重さ: high
- 節: 4.5 (D571) / M111 injection test
- 反証: 2 人のうち 0 人

### 問題

Under the current spare-chunk semantics, the post-fix expectation of the injection test is contradictory. ValueStack::take_mem (crates/keleut-rt/src/stack.rs:127-143) arms the spare only when a chunk allocation fails, and it still returns Err for that failing request. The spare serves only the next request. The test is built on continj.kel, where the G clause exhausts the heap before `resume`, so the only cont_resume is the injection itself. After D571 the injection allocates no chunk (it uses the copy as a chunk). The first chunk allocation under `exec:chunk:hard:N` is therefore the cancel frame that overflows the exact-size copy chunk. That request fails (spare armed, Err returned), and the cancel clause is aborted, so the expected `inner cancel, own cancel, outer cancel, 3` will not appear for the 'cancel clause with many live values' variant. The claim in 4.5 that 'the spare is already armed by the cont_resume chunk failure that triggered the injection' holds only in the V243 scenario, where an ordinary cont_resume failed on a chunk first. That is not the continj trigger. In addition, N is chosen as 'the value that hits cont_resume's allocation' in the pre-fix allocation sequence. After the fix that allocation no longer exists, so the same N points at a different chunk allocation, which defeats the plan's goal of targeting allocations by site name.

### 根拠

stack.rs:127-143: `match new_mem(...) { Ok(m) => Ok(m), Err(e) => { if let Some(sp) = self.spare.take() { self.armed = Some(sp); } Err(e) } }`. probe/alloc/continj.kel: `case op() => 1 + resume(len(build(2000000000, Nil), 0))` exhausts the heap while evaluating the argument, before any resume. interp.rs:1588-1660: injection and ordinary resume share cont_resume, and both place frames with place().

### 直し方

Choose one approach. (a) Have D571 arm the spare chunk when injection or unwinding starts, or let take_mem fall back to the spare within the same request while the runtime is unwinding an OutOfMemory. Say this in 4.5. (b) Make the test trigger a real cont_resume chunk failure first (the V243 shape: the clause calls resume, which fails to place a large continuation), and state that the spare is armed by that failure. In both cases, define the hard trigger by the site plus the test's stderr line, not by a pre-fix SKIP count. State the expected output separately for the pre-fix code (inner cancel lost) and for the post-fix code.

### 反証者 1: 反証できなかった

I confirmed the problem against the code, so I could not refute the finding.

1. **The spare is armed in only one place.** `ValueStack::take_mem` (rustkeleut/crates/keleut-rt/src/stack.rs:127-143) arms it only when `new_mem` fails, and it still returns `Err` for that same request. A grep of keleut-rt and keleut-interp found no other code that sets `armed`. GC heap exhaustion does not arm it.

2. **In `continj.kel` the only `cont_resume` is the injection.** The probe is doc/log/261104-3-spike/probe/alloc/continj.kel. Its G clause exhausts the heap inside `len(build(2000000000, Nil), 0)`, before `resume` is ever called. Injection runs through that same `cont_resume` (interp.rs:1588-1660, with a Pending `v`), and the record at lines 2672-2679 says so as well.

3. **What happens after D571.**
   - The injection links the copy as a chunk and allocates no chunk.
   - The first chunk allocation under `exec:chunk:hard:N` is therefore the oversized cancel frame's own `place` → `take_mem`.
   - That call fails. It arms the spare and returns `Err`, so the "many live values" inner cancel clause is aborted.
   - The expected order `inner cancel, own cancel, outer cancel, 3` cannot appear for that variant.

4. **The plan's stated reason does not apply to this test.** Section 4.5 (around line 639) says the spare "is already armed by the `cont_resume` chunk failure that triggered the injection". That is true only in the V243 shape, where an ordinary `resume` first failed to place frames. In this test the trigger is heap exhaustion.

5. **The SKIP count points elsewhere after the fix.** Line 653 picks `N` as "the value that hits `cont_resume`'s allocation" in the pre-fix allocation sequence. D571 removes that allocation, so the same `N` lands on a different chunk allocation. This defeats the plan's own aim (section 4.2) of targeting allocations by site name.

6. **Not already handled by the plan.** The feasible-10 row (line 1145) only promises to describe this path. It relies on the same wrong premise.

The suggested remedies are sound. One option is to arm the spare when injection or OutOfMemory unwinding starts, or to let `take_mem` fall back to the spare within the same request. The other is to make the test use the V243 trigger shape. Either way, the expected output should be stated separately for the code before and after the fix.

### 反証者 2: 反証できなかった

I checked this against the code and the plan, and the finding holds. First, rustkeleut/crates/keleut-rt/src/stack.rs:127-143 (take_mem) moves `spare` into `armed` only when new_mem fails, and it still returns Err for that request. Only a later request can use the armed chunk. Neither place() (stack.rs:199-) nor native.rs:758-790 (place) retries a failed request; both report OutOfMemory. Second, in doc/log/261104-3-spike/probe/alloc/continj.kel the G clause `case op() => 1 + resume(len(build(2000000000, Nil), 0))` runs out of heap while it builds the argument, before any resume. The `use()` clauses never run. So the injection was not set off by a failed cont_resume chunk allocation. Third, section 4.5 of the plan says the opposite: 「注入の引き金になった cont_resume の塊の確保の失敗で、値スタックの予備の塊はもう使える状態になっている」. That holds only for the V243 shape. The M111 test says only that continj should run a collection that drops chunks. It never says that an ordinary resume fails on a chunk first. Under the fix, the injection allocates no chunk, so with `exec:chunk:hard:N` the first chunk request that fails is the cancel frame with many live values that overflows the exact-size copy chunk. That request gets Err, the spare is armed too late for it, and the expected inner/own/outer cancel, 3 sequence does not appear. Finally, N is chosen as 「cont_resume の確保に当たる値」 in the pre-fix allocation order. That allocation disappears after the fix, so the same N points at a different chunk allocation. This undercuts the plan's own reason for naming sites instead of counting allocations (section 4.2). The suggested remedies are sound: arm the spare when injection or unwinding starts, or build the V243 shape on purpose, and state the expected output separately for the code before and after the fix.

## design-2

- 観点: design(設計と試験)
- 重さ: medium
- 節: 4.2 (D568) item 3 and M108 tests
- 反証: 2 人のうち 0 人

### 問題

The `collect` test uses `soft` mode, so it cannot detect whether item 3 (removing allocations inside collection) was done. With soft mode the wrapper releases the 8 MiB reserve, the retry succeeds and the flag gives OutOfMemory → 3. That holds whether or not the collector still allocates. The original V230 crash (Points::live inside Rt::collect) would also turn into 3 through the reserve alone. Item 3 is also under-specified. Allocations during the scan include: lazy FuncRoots::new on first sight of a function (gc.rs:430 get_or_insert_with); the per-row and per-phase caches built with Vec and into_boxed_slice in FuncRoots::row and phase (gc.rs:361-401); and the HashMap inside Points::live (live.rs:76). None of these can be reserved 'before collection starts', because which functions and rows get visited is known only during the mark. The plan does not say what happens when one of them fails in the middle of marking.

### 根拠

gc.rs:430 `self.funcs[f.index()].get_or_insert_with(|| FuncRoots::new(self.prog, f))`. gc.rs:363 and 390 `let mut v = Vec::new(); ... self.rows[row] = Some(v.into_boxed_slice())`. live.rs:68-76 `base.to_vec()` and `let mut over: HashMap<Reg, bool> = HashMap::new()`. Plan 4.2 item 3 names only Points::live's output Vec, FuncRoots::new and the any_slots work area. Plan 4.2 says `hard` must not be used in the exec role at sites that do not handle failure themselves.

### 直し方

After item 3, make `collect` a site that handles failure itself, and add an `exec:collect:hard:0` test that must give before/cancel/OOM/3. Without item 3 that test would give 134. Extend item 3 to cover the row and phase caches and the HashMap in live(). Possible ways: precompute FuncRoots at load or extension time, compute rows into a pre-reserved scratch area without caching, or define a safe abort of a half-finished mark that clears the mark bits and returns OutOfMemory.

### 反証者 1: 反証できなかった

I confirmed the finding. Plan section 4.2 item 3 (lines 481-483) names only three things: the output Vec of Points::live, FuncRoots::new and the any_slots work area. It does not cover these allocations made during marking:
- the lazily built row and phase caches in FuncRoots::row and FuncRoots::phase. At gc.rs:361-401 each one does Vec::new and then into_boxed_slice into self.rows[row] or self.phase[row].
- the lazy get_or_insert_with of FuncRoots in Frames::scan (gc.rs:430).
- the HashMap inside Points::live (live.rs:76). Passing an output Vec does not remove it.

Which functions and rows get visited is known only during the mark. So "try_reserve before collection starts" cannot cover these allocations as written, and the plan does not say what happens when one of them fails in the middle of marking.

The M108 test (line 514) also runs `collect` only in `soft` mode. Under the plan's own rules, a soft failure outside the self-handled range releases the 8 MiB reserve, the retry succeeds and the flag gives OutOfMemory → 3. That happens whether or not the collector still allocates. After item 3, a failure inside the pre-reserve range returns OutOfMemory directly, which is also 3. So the test cannot tell whether item 3 was done.

The plan also says `hard` must not be used in the exec role at sites that do not handle failure themselves (line 504). The suggested change is consistent with that: make `collect` a site that handles failure itself, and add a hard test that would give 134 without item 3. I found nothing in sections 13.3 or 13.4 that already deals with this.

### 反証者 2: 反証できなかった

I checked the finding against the plan and the code, and it holds. Plan 4.2 item 3 (lines 481-483) names only three things. They are a `Points::live` form that writes into a given Vec, reuse of the `gc::Frames` work area, and taking `FuncRoots::new` and the `any_slots` work area with `try_reserve` before collection starts. The code allocates in more places during the mark. gc.rs:430 builds FuncRoots lazily with `get_or_insert_with` during `Frames::scan`, so the set of functions is known only while marking. `FuncRoots::row` and `phase` (gc.rs:361-401) build a fresh Vec for each row and cache it with `into_boxed_slice`. live.rs:76 creates a `HashMap<Reg, bool>` inside `Points::live`, apart from the `to_vec` and `collect` output. Item 3 says nothing about the row and phase caches or the HashMap. Nor does it say what happens when an allocation fails partway through a mark. The test check is also right. Plan line 497 says `soft` fails once and retries through the reserve path. The M108 tests (line 514) use `soft` at `collect`, so they end in OutOfMemory and exit 3 whether or not item 3 was done. The original V230 crash in `Points::live` inside `Rt::collect` would also become 3 through the reserve alone. Lines 499-501 forbid `hard` at exec-role sites that do not handle their own failures, so no test checks that item 3 actually removed the allocations. The M108 acceptance (line 927) adds only the manual `sweep_as.sh` run, which is machine-dependent and not a test. The suggested fix is sound: extend item 3 to cover the caches and the HashMap, precompute or reserve them, or define a safe abort of a half-finished mark. Then treat `collect` as a site that handles its own failures and add an `exec:collect:hard:0` test.

## design-3

- 観点: design(設計と試験)
- 重さ: medium
- 節: 4.2 (D568) tests / M108 unit test
- 反証: 2 人のうち 0 人

### 問題

The plan places #[global_allocator] only in keleut-cli's main.rs. It also asks for a keleut-rt unit test showing that a failure inside the self-handling scope (the `keep` of collection) leaves the reserve intact, sets no flag and lets collection continue. A keleut-rt lib unit test runs with the default System allocator: neither the wrapper, nor the reserve, nor KELEUT_TEST_ALLOC_FAIL exists in that test binary. As specified, the test cannot observe any of the three properties.

### 根拠

Plan 4.2 item 1: the wrapper and flags live in a small keleut-rt module, and the `#[global_allocator]` declaration goes in keleut-cli's main.rs. The M108 test list includes the keleut-rt unit test for `keep`.

### 直し方

Declare the wrapper as #[global_allocator] in a dedicated keleut-rt integration test target (for example crates/keleut-rt/tests/alloc_wrap.rs; one global allocator per test binary is allowed). Or run this check through the keleut binary in tests/oom_cmd.rs with a `collect` hard trigger and stderr evidence that the reserve was not used, for example a KELEUT_GC_STATS or test line.

### 反証者 1: 反証できなかった

I could not refute this finding; the gap is real. In plan 4.2 item 1 (line 463), the wrapper and its flags go in a keleut-rt module, but the `#[global_allocator]` declaration goes only in keleut-cli's main.rs. The reserve is taken at startup, and the `KELEUT_TEST_ALLOC_FAIL` injection (lines 491-512) happens inside the wrapper. Line 522 asks for a keleut-rt unit test showing that a failure of `keep` (heap.rs:393) leaves the reserve intact, sets no flag and lets collection continue. In keleut-rt's own test binary the `keep` Vec goes through the default System allocator. There is no wrapper, no reserve and no way to inject the failure. Calling the wrapper's `GlobalAlloc` methods directly on a local instance could check the null return, but not that collection continues. No crate declares a global allocator today (grep finds none). The plan says nothing about a test-only global allocator: no `cfg(test)` declaration and no separate test target. So the test cannot observe the three properties as written. The suggested fix is sound, since each test binary may have its own `#[global_allocator]`. Either declare the wrapper in a dedicated keleut-rt integration test or a `cfg(test)` declaration and set up the reserve there, or check this case through the keleut binary in oom_cmd.rs.

### 反証者 2: 反証できなかった

I checked this against the plan and it holds. Line 463 of the plan puts the wrapper and its flags in a keleut-rt module and declares `#[global_allocator]` only in keleut-cli's main.rs. Nothing else in the plan installs the wrapper anywhere else; grep finds no `#[global_allocator]` for a test binary and no `#[cfg(test)]` allocator. Line 522 asks for a keleut-rt unit test showing that when the `keep` allocation in collection fails, the reserve stays, no flag is set and collection continues. A keleut-rt lib test binary runs on the System allocator, so the `try_reserve` for `keep` never passes through the wrapper. The test also has no way to inject the failure: `KELEUT_TEST_ALLOC_FAIL` is read at startup through the wrapper, and the wrapper is only installed in the keleut binary. Calling the wrapper's `alloc` by hand could show "no flag, reserve intact" for a single call. It cannot show the third property, that collection continues after `keep` fails. That needs the wrapper as the global allocator of the test binary, or the test run through the keleut binary. The suggested fixes are both valid: a dedicated keleut-rt integration test target or a `#[cfg(test)]` `#[global_allocator]`, or an oom_cmd.rs case that targets `collect`. The plan does not address this, so the finding stands.

## design-4

- 観点: design(設計と試験)
- 重さ: medium
- 節: 7.2 M108 acceptance (sweep_as.sh)
- 反証: 2 人のうち 0 人

### 問題

Before the fix, the interp 134 shows up only at isolated limits between 3-exits. A 25,000 KiB grid samples just one of them (350,000), so 'interp 134 = 0 in the 300,000–700,000 sweep at 25,000 steps' is weak evidence for the fix. The acceptance never establishes a pre-fix baseline on the same grid, so a shift of the failing points could be read as success.

### 根拠

Probe at HEAD (oomlist.kir, --backend interp, --native-stack 64m): as=325k/335k/340k/345k → 3 with 2 lines; 350k → 134 'memory allocation of 12 bytes failed'; 355k → 3; 360k → 134; 365k/375k → 3. alloc results.md section 3 recorded only 350k at the 25k step.

### 直し方

Run the sweep at 5,000 KiB (or 1,000 KiB) steps from 300,000 to 450,000 for the interp column, both before and after the fix, record the pre-fix 134 count (it must be non-zero), and require 0 afterwards. Keep 25k steps only for the JIT columns.

### 反証者 1: 反証できなかった

I confirmed the finding. In the plan, section 4.2 (line 525) and the M108 acceptance in section 7.2 (line 927) require that `sweep_as.sh` over `ulimit -v` from 300,000 to 700,000 KiB in 25,000 KiB steps shows 0 interp 134 exits. No pre-fix baseline on the same grid is required. Line 32 of the plan itself says that moving the limit by 25,000 KiB flips the result between 3 and 134.

I re-probed at HEAD with oomlist.kir, `--backend interp`, `--native-stack 64m`. At 325k, 340k, 345k, 355k, 360k, 365k and 375k it exited 3 with 2 lines of output. At 350k alone it exited 134 with "memory allocation of 12 bytes failed".

So the 134 failures sit at isolated limits, and at 360k they don't even repeat (the finding's probe got 134 there, mine got 3). A 25k grid hits only one such point, so an empty result after the fix is weak evidence. A failing limit that merely moved would read as success.

The plan's main tests are the deterministic `KELEUT_TEST_ALLOC_FAIL` cases in `tests/oom_cmd.rs`. That makes the sweep secondary, but it is still listed as an acceptance condition. The suggested change is cheap and correct: run the interp sweep at fine steps both before and after the fix, record a non-zero pre-fix count, and require 0 afterwards. Repeating runs at each limit would also help, given the nondeterminism.

### 反証者 2: 反証できなかった

I confirmed the finding at HEAD, though it matters less than it first looks. I ran target/release/keleut run --backend interp --native-stack 64m on scratchpad/work/verify-facts/oomlist.kir under several ulimit -v values. 325k, 340k, 355k, 365k, 375k and 400k exit 3, while 350k and 360k abort with 134. So the 134 shows up only at isolated limits between exits of 3, and a 25,000 KiB grid lands on just one of them (350k). The plan's only sweep check is in section 4.2 (line 525) and the M108 acceptance (line 927): "sweep 300,000-700,000 at 25,000 KiB steps; interp 134 must be 0". I searched for 刻み and 直す前 and found nothing that asks for a pre-fix sweep baseline on the same grid. Other tests in the plan do ask for a pre-fix failure (lines 318, 603, 650, 655, and M106 and M110), but the sweep does not. That makes 0 after the fix weak evidence, and a shift of the failing points could pass as success. The plan's main M108 evidence is the deterministic KELEUT_TEST_ALLOC_FAIL tests in oom_cmd.rs, so the sweep is only a supporting manual check, which is why the severity is lower. Still, the change the finding suggests is correct and cheap: sweep interp at 1,000-5,000 KiB steps over about 300,000-450,000, record a non-zero 134 count before the fix, and require 0 after it.

## design-5

- 観点: design(設計と試験)
- 重さ: low
- 節: 4.2 (D568) translation role / 4.4 item 6
- 反証: 2 人のうち 0 人

### 問題

The design lets the translation role use the single global 8 MiB reserve, and it must: Rust Vecs in keleut-llvm abort on null. After that the role only stops translating. Nothing re-acquires the reserve, so a later OutOfMemory in the exec role, typically at the same moment under one RLIMIT_AS, finds no reserve and aborts with 134. Under the default tier the compile thread and the exec thread run at the same time, so this race is the usual case under ulimit -v. In addition, the M108 'translation role' test names no site, and none of the listed sites (collect, rec_fields, call_args, chunk, cont, shape, check, kir) belongs to the jit role.

### 根拠

Plan 4.2: the reserve is a single global reserve. When the translation role's flag is set, the current function's translation is discarded and translation stops. Only `keleut repl` re-acquires the reserve. alloc results.md section 3: threshold=1 at 400,000 → 134 St9bad_alloc while exec runs.

### 直し方

After the translation role has consumed the reserve and translation has stopped, try to re-acquire the reserve, as the REPL does. Alternatively, give the translation role its own small reserve. Name a jit-role site (for example `jit` around the Rust-side IR building) for the M108 test.

### 反証者 1: 反証できなかった

The finding holds. Plan 4.2 (D568) keeps a single global reserve. Item 1 says any role's failed allocation returns the reserve and sets that role's flag, so the translation thread can use it up. Item 2 says that when the translation flag is set, the plan discards the current translation and stops translating. Only `keleut repl` re-acquires the reserve, at lines 479-480. Nothing re-acquires it after the translation role has used it. A later out-of-memory in the exec role then finds no reserve. Rust allocations outside the self-handled scope, such as a value-stack chunk or a Vec in a cancel clause, would then abort with 134 instead of exiting 3. The GC heap's mmap path keeps its own 1 MiB reserve, so this is partial. The plan does not mention this case: the risk table entry at line 1005 covers a different window. The M108 sweep only counts the remaining JIT-stage 134s. Under the default threshold tier the translation thread and the exec thread run at the same time, so the case is plausible. On the test: the M108 site list at line 495 is collect, rec_fields, call_args, chunk, cont, shape, check and kir. None of these belongs to the jit role. The test at line 523 ('fail in the translation role and the program continues on the interpreter with 0') names no site, although ROLE `jit` is defined. The plan says the names are decided in M108, which softens this point, but the gap is real. The suggested fixes are sound and low-cost: re-acquire the reserve after translation stops, or give the translation role its own small reserve, and name a jit site.

### 反証者 2: 反証できなかった

I confirmed the finding against /home/sumito3478/repo/keleut/doc/log/261104-3-plan.md, section 4.2 (D568).

1. The plan has one global reserve, and the translation role draws on it like every other role. Line 447 applies to any failed allocation: "確保が失敗し予備があれば、予備を返して役割ごとの印を立て、確保をやり直す". It has no exception for the translation role. The translation role has to use the reserve, because Rust Vecs in keleut-llvm abort when they get null.

2. Once the translation flag is set, the plan only discards the translation and stops translating (lines 474-476). The only place the reserve is taken back is `keleut repl` (lines 479-480). Nothing re-acquires it after a translation-role failure. A later Rust allocation failure in the exec role then finds no reserve, gets null and aborts with 134.

3. The plan treats "Rust allocations after the reserve is used up" as a documented window (D567, risk table at line 1005). But that window assumes the program itself used the reserve. Losing the reserve to the compile thread is a separate gap, and re-acquiring the reserve or giving the translation role its own reserve would close it cheaply. The 64 MiB check before translation (item 4) narrows the window but does not close it.

4. The M108 translation-role test (line 523, and the M108 row at line 927) names no SITE. All the listed site names (lines 493-494: collect, rec_fields, call_args, chunk, cont, shape, check, kir) belong to the exec or conv roles. So `KELEUT_TEST_ALLOC_FAIL=jit:...` has no site to target, and the requirement to confirm the hit by the test line cannot be met for this test.

One part I could not confirm: the claim that this race is "the usual case" under ulimit -v. That is an inference I did not measure. The gap itself is real, and low severity is fair.

## decisions-0

- 観点: decisions(裁定、文案、台帳)
- 重さ: medium
- 節: 5.2 D578 (new LangSpec §6.6 sentence), 6 table, P264
- 反証: 2 人のうち 0 人

### 問題

The proposed §6.6 sentence says that whether and how a result is displayed is up to the implementation, "except for the spellings this document defines (§8 row display, §16.2 show spelling, etc.)". Read literally, that requires the REPL to display a String value with the §16.2 show spelling, which is the raw content without quotes. Both implementations deliberately display top-level, tuple and record strings with quotes and escapes (decision D202). So the new sentence would make the measured, byte-identical behaviour of both REPLs non-conforming. That contradicts the plan's claim that the sentence only writes down the existing premise.

### 根拠

LangSpec.md:1848 (`String`: 内容をそのまま返す。引用符は付けない). probe/decide/display.out: `s : String = "a\"b\n"`, `t : (Int32, String) = (1, "a")`, `rs : {name: String} = {name = "n"}` (same for diktor and keleut). doc/log/261002-2-plan.md:3245-3250 (D202: the REPL uses Value.show with quotes precisely because Show[String] would print `s : String = abc`).

### 直し方

Limit the exception to the places where LangSpec defines how the implementation displays something: §8 tuple/row notation, the arrow-row display at LangSpec:130/1249, and Float64 non-finite spellings. Alternatively, say explicitly that the display may differ from `show` (for example quoted strings) and only that the §8 row notation applies. Cite D202 in D578.

### 反証者 1: 反証できなかった

I confirmed the problem, so the finding is not refuted. The D578 text at doc/log/261104-3-plan.md:803 says display formats are up to the implementation "except for the spellings this document defines (§8 row display, §16.2 show spelling, etc.)". That exception covers all of §16.2, and §16.2 (doc/LangSpec.md:1848) says `String` is shown as its content as-is, without quotes. The plan's own section 1.4 (line 195) records the measured fact that both REPLs print top-level, tuple and record strings with quotes. That is the deliberate choice D202 made (doc/log/261002-2-plan.md:3248-3249: using Show[String] would print `s : String = abc`). So, read literally, the new sentence makes the existing, identical behaviour of both REPLs non-conforming. This contradicts the plan's stated aim of writing down the existing premise and closing the issue without changes. The plan only fixed the conflict with §8 (facts-1, line 1130). It never addresses the String/quote conflict with §16.2, and D578 does not cite D202. The plan's intent seems to be only the Float64 spellings (line 192: Float64 forms, nan, inf). The sentence should therefore name the specific spellings (§8 row notation, Float64 non-finite spellings) or say explicitly that the display may differ from `show`, for example by quoting strings.

### 反証者 2: 反証できなかった

I confirmed the finding. In the plan, D578 (around line 800) would add this sentence to §6.6: 受理した入力の結果を表示するかどうかと、その書式は、本書が定める綴り(§8 の行の表示、§16.2 の `show` の綴りなど)を除いて、処理系が定める. Read literally, that makes the §16.2 `show` spellings binding on REPL display. One of those spellings is the String row at doc/LangSpec.md:1848: 内容をそのまま返す。引用符は付けない.

The plan's own probe output (doc/log/261104-3-spike/probe/decide/display.out) shows both REPLs printing strings with quotes and escapes at top level and inside tuples and records: `s : String = "a\"b\n"`, `t : (Int32, String) = (1, "a")` and `rs : {name: String} = {name = "n"}`. The output is byte-identical between diktor and keleut. Plan section 1.4 records the same fact. D202 (doc/log/261002-2-plan.md:3244-3250) chose Value.show over Show precisely so that strings would not print as `s : String = abc`.

So the sentence, as worded, would make the existing behaviour of both REPLs non-conforming. That contradicts D578's stated aim of only writing down the existing premise. Section 1.4 suggests the authors meant only the Float64 spellings (nan, inf, -inf), but the sentence names §16.2 `show` as a whole. The earlier facts-1 fix in section 13.3 added the §16.2 exception and does not cover this. The plan never cites D202 or limits the exception. The suggested change (narrow the exception to §8 row notation and the Float64 spellings, or say that the display may differ from `show`, and cite D202) is reasonable.

## decisions-1

- 観点: decisions(裁定、文案、台帳)
- 重さ: medium
- 節: 5.2 D578 (V241), 10 台帳, 13.5
- 反証: 2 人のうち 0 人

### 問題

V241 (strings inside List/Option displayed without quotes, close unchanged) duplicates an existing ledger entry. V74 already records exactly this (`Some("x")` displayed as `Some(x)`) and closed it as "直さない(表示の設計)". The M39 record ties it to D202. Opening a new number for the same item splits the history, and the plan's rationale never cites the earlier decision.

### 根拠

doc/log/261002-2-plan.md:5578: `| V74 | REPL の結果の表示で…`Some("x")` が `Some(x)` と引用符なしで表示される | M47/M48 の検証 | 直さない(表示の設計) |`. doc/log/261002-3-m39.md:394-395 (D202 が避けようとした、文字列が引用符なしで出る形が…残っている).

### 直し方

Drop V241 and refer to V74/D202 in D578 and in the 1.4 bullet, for example "V74 で直さないと決めた不揃い". Then either renumber V242–V244 down or state in 13.5 that V241 is unused, and update the next free number accordingly.

### 反証者 1: 反証できなかった

I confirmed the finding. Line 5578 of doc/log/261002-2-plan.md already has a ledger entry for this. V74 records that `Some("x")` is shown as `Some(x)`, without quotes, and closes it as 直さない(表示の設計). Lines 394-395 of doc/log/261002-3-m39.md link this behaviour to D202, the decision it went against. The plan 261104-3 opens V241 for the same item: strings inside `List` and `Option` shown without quotes, closed unchanged. That entry appears at lines 248, 811, 992 (D578) and 1039. The plan never mentions V74 or D202, so the new number duplicates the existing entry and splits its history. The suggested fix is sound. Drop V241 or mark it unused, cite V74/D202 in D578, in 1.4 (line 195) and in the 台帳 section, and update the next free V number.

### 反証者 2: 反証できなかった

I confirmed the finding, so it is not refuted. doc/log/261002-2-plan.md line 5578 already records V74. Part of that entry reads "`Some(\"x\")` が `Some(x)` と引用符なしで表示される", and it is closed as "直さない(表示の設計)". doc/log/261002-3-m39.md lines 394-395 give the same fault for Option and List and tie it to D202. The plan doc/log/261104-3-plan.md gives this same fault a new number, V241, in four places: lines 248, 811, 992 (D578) and 1039. It never mentions V74 or D202; a grep of the plan finds neither. So V241 duplicates an entry that is already closed, and the plan does not cite the earlier decision. The suggestion is sound: refer to V74/D202 instead of V241, and either renumber the later entries or record that V241 is unused.

## decisions-2

- 観点: decisions(裁定、文案、台帳)
- 重さ: medium
- 節: 5.2 D578 (V244)
- 反証: 2 人のうち 0 人

### 問題

V244 says that if a diktor/test --repl cram hits the keleut-dependent field order in runtime-error `<値>` lines or cancel-record lines, it will be registered as a known diff "in the V221 form". This cannot happen, and the ledger named is the wrong one. kconform already cuts runtime-error lines after the kind word and sorts the fields of cancel-suppression record lines before comparing (D560, V232). So such a difference never surfaces against Diktor, and the RustKeleut backend pairs all send --repl to the same `keleut repl`. Also, V221/D558 is the display-order ledger. The `<値>` order belongs to V228/D555 (IRSpec appendix B).

### 根拠

rustkeleut/crates/kconform/src/normalize.rs:6-13 (runtime-error lines are cut after the kind word; cancel-record lines are sorted first, D560). fieldsort.rs:224 LEADS covers both prefixes. 261104-2-record.md:405-408 (V228 closed via D555, V232 fixed by normalization).

### 直し方

Replace the registration sentence with one saying that kconform's normalization (D560) already absorbs this order for diagnostics, so no known-diff entry is needed. If a registration rule is kept for some other path, name V244 (or V228/D555), not V221.

### 反証者 1: 反証できなかった

I confirmed the finding. Plan line 818, in the V244 bullet of section 5.2, says that if a diktor/test --repl cram hits this difference, it is registered as a known diff "V221 の形で".

kconform already normalizes the lines in question. normalize.rs:11-13 runs fieldsort::sort_line on every line before comparing, and that holds for both expected and actual output (main.rs:529 and :567, known_diffs.rs:147). sort_line uses the prefixes in fieldsort.rs:224 LEADS (実行時エラー: and cancel 節で例外が抑制されました:) together with the appendix-B message prefixes. It sorts the fields with different labels in `<値>`, and runtime-error lines are then also cut after the kind word. So against Diktor this order difference can never show up.

The backend pairs also cannot differ from each other: AGENTS.md says the shim sends every RustKeleut pair's --repl to the same `keleut repl` on the interpreter.

The ledger reference is also wrong. V221/D558 covers REPL display order. The `<値>` order belongs to V228/D555 (261104-2-record.md:405-406, and plan line 817 itself cites D555), and the normalization is V232/D560.

So the registration sentence describes a case that cannot arise, and it names the wrong ledger. It should say that D560's normalization absorbs the difference, so no known-diff entry is needed.

### 反証者 2: 反証できなかった

I confirmed the finding. Line 818 of the plan (5.2, under V244) says that if a diktor/test --repl cram hits this order difference, it is registered as a known diff "in the V221 form" (`V221 の形で既知の差に登録する`). Line 814 also points the registration form at V221/D558.

kconform's normalization already absorbs this difference before any comparison. In rustkeleut/crates/kconform/src/normalize.rs, `line()` first calls `fieldsort::sort_line`. LEADS at fieldsort.rs:224 covers both `実行時エラー: ` and `cancel 節で例外が抑制されました: ` (D560). `line()` then cuts every line after the kind word (`実行時エラー`), and it finds that word anywhere in the line, so a REPL prompt prefix does not stop it. Its own tests show both cases: a runtime-error line with a record becomes just `実行時エラー`, and a cancel-record line's fields are sorted. Both comparison paths use `normalize::lines` (main.rs:529 and 567, known_diffs.rs:147). The RustKeleut backend pairs all route --repl to the same deterministic `keleut repl`, so they cannot differ from each other either. A difference in this `<値>` order therefore never shows up as a diff, and the registration rule can never apply.

The plan does not mention this normalization anywhere: there are no hits for D560 or 正規化. Its 1.4 section cites only D555. The ledger is also misattributed: V221/D558 is the display-order ledger, while the `<値>` order belongs to V228/D555 (IRSpec appendix B). The suggested replacement is correct: say that kconform's normalization (D560) absorbs it, so no known-diff entry is needed.

## decisions-3

- 観点: decisions(裁定、文案、台帳)
- 重さ: medium
- 節: 4.1 D567 / 6 table (IRSpec 13.9 and 14 history row)
- 反証: 2 人のうち 0 人

### 問題

D567 adds an exception to IRSpec 13.9 that lets readers reject KIR of depth ≤ N_nest. It also widens section 1's exclusions while M108–M110 change reader exit statuses (70→65, 134→3). The plan then adds an unversioned '1.0(本文の改訂)' row and cites only D556. But IRSpec 14 allows an unversioned revision only when it changes neither KIR acceptance nor writer/reader behaviour. Relaxing the normative 'readers accept every program of depth ≤ N_nest' is on its face a change to acceptance. The plan never argues why this still qualifies. Separately, IRSpec already has a row labelled exactly '1.0(本文の改訂)' dated 2026-11-04, so a second identical label is ambiguous.

### 根拠

doc/IRSpec.md:1670 (読み手は、深さが `N_nest` 以下のプログラムを受け付け…). doc/IRSpec.md:1698 (KIR の受理と意味…を変えず、書き手と読み手の振る舞いも変えない本文の改訂…では、版を変えない). doc/IRSpec.md:1724 (existing row `1.0(本文の改訂) | 2026-11-04`). Plan 4.1 lines 421-425.

### 直し方

Add a sentence to D567 or D581 explaining why the 13.9 exception fits rule 14. For example: it records a resource-limit exclusion of the same kind as section 1, and the reader's KIR-level acceptance is unchanged. If it does not fit, amend rule 14 explicitly in M107 and ask the owner (extend P260). Give the new history row a distinguishing date or note so it cannot be confused with the 2026-11-04 row.

### 反証者 1: 反証できなかった

I confirmed the finding. Under D567 (plan lines 421-424), IRSpec 13.9 gains an exception: a reader with a reduced stack, or one that cannot create its thread or runs out of allocation, may reject KIR of depth ≤ N_nest with exit 65. Today 13.9 is normative and says readers accept every program of depth ≤ N_nest (doc/IRSpec.md:1670). Rule 14 at IRSpec:1698 allows a revision without a version change only if it keeps "KIR の受理と意味" and reader behaviour unchanged. Its listed examples are widening the allowed diagnostic text and fixing typos. Line 425 only asserts in parentheses that acceptance, meaning and encoding are unchanged, and cites D556. It gives no argument for why weakening a normative acceptance guarantee counts as "受理を変えない". There is a counter-argument the plan could make: the 2026-11-04 revision already added items to section 1's exclusions without a version change, and the --native-stack limit-lowering behaviour already exists (AGENTS.md). But the plan does not make that argument. The claim is still contestable, so the requested justification sentence, or an explicit amendment to rule 14 or an owner question, is warranted. The label point is also valid. IRSpec:1724 already has a row "1.0(本文の改訂) | 2026-11-04", and D581/table row 880 adds another row with the identical label. Today is 2026-11-04, so the date may not tell the two rows apart either. The suggested changes are small and correct.

### 反証者 2: 反証できなかった

I checked the finding against the repo and it holds. IRSpec 13.9 (doc/IRSpec.md:1670) requires readers to accept every program of depth up to N_nest. Rule 14 (doc/IRSpec.md:1698) allows an unversioned revision only when it changes neither KIR acceptance nor writer or reader behaviour. Its examples are wider latitude for diagnostic wording and erratum fixes. D567 (plan lines 421-425) adds an exception that lets a reader whose stack was shrunk reject KIR inside N_nest with exit 65. That is a relaxation of what readers must accept. The plan supports the unversioned row only with a bare parenthetical: "計画 261104-1 の D556 の規則。KIR の受理、意味、符号を変えない". D581 (line 995) gives the same unargued reason. Neither explains why a resource-limit exception does not count as a change to acceptance. An argument is available: the current --native-stack reader already lowers the limit, so the text would catch up with existing behaviour, like the section 1 resource exclusions. The earlier unversioned row is a weaker precedent, because its section 1 addition was about the order of record fields in diagnostics, not acceptance. The plan never makes either argument and does not ask the owner; P260 covers the content of the exclusion, not the versioning question. The second point also stands. The history table already has a row labelled exactly "1.0(本文の改訂)" and dated 2026-11-04 (doc/IRSpec.md:1724). The plan adds a row with the same label and does not say how to tell the two apart. If M107 lands on the same day, the dates will match too. The suggested fix, a justifying sentence or an explicit rule-14 amendment or owner question plus a distinguishing note on the row, is appropriate.

## decisions-4

- 観点: decisions(裁定、文案、台帳)
- 重さ: low
- 節: 4.1 D566 (LangSpec §15.5 table)
- 反証: 2 人のうち 0 人

### 問題

D566 widens only the 条件 column. Row 1 keeps the 終わり方 label '型検査での拒否', although it will now also cover failures after the program passed type checking (KIR writer limit, conversion allocation, KIR thread). Row 3 keeps 初期化 = '途中まで行う', although the new condition 'initialization start' means no initialization ran at all. A reader of the table gets a column that contradicts the condition.

### 根拠

doc/LangSpec.md:1783-1785 (columns 終わり方 / 条件 / 初期化 / 終了状態; row 3 初期化 = 途中まで行う). Plan 4.1 lines 364-375 change only the condition text.

### 直し方

Also change row 1's 終わり方 (for example to '型検査と変換での拒否') and row 3's 初期化 cell (for example to '行わないか、途中まで行う'). Include both cells in the D566 text and in the M107 acceptance check.

### 反証者 1: 反証できなかった

I confirmed the finding. In doc/LangSpec.md:1781-1785 the table's columns are 終わり方 / 条件 / 初期化 / 終了状態. Row 1 is labelled 型検査での拒否 and row 3 has 初期化 = 途中まで行う. The plan changes only the 条件 text, in 4.1 at lines 364-375. The same holds in its summary at line 64, the decision table at line 980, P259 at line 1057, and the M107 acceptance at line 926. Searching the plan for 終わり方, 途中まで and 型検査での拒否 finds no change to either cell. As a result, row 1 would also cover failures after type checking passes (the KIR writer limit, front-end allocation, the KIR thread, and KIR verification limits for .kel input) while still being called 型検査での拒否. Row 3 would cover a failure at initialization start, when nothing has been initialized, yet still say 途中まで行う. Two mitigating points do not refute this. First, the row 1 label is already loose, since it also covers import errors. Second, 途中まで could be read as including zero steps. The mismatch is real and low in severity, as stated. One addition to the suggestion: if row 1's label is renamed, the sentence at LangSpec.md:1800 that names 型検査での拒否 must be updated to match.

### 反証者 2: 反証できなかった

I confirmed the finding, so it is not refuted. doc/LangSpec.md lines 1781-1785 give the §15.5 table the columns 終わり方 / 条件 / 初期化 / 終了状態. Row 1 is labelled 型検査での拒否, and row 3 has 初期化 = 途中まで行う. Plan section 4.1 (doc/log/261104-3-plan.md lines 364-375) rewrites only the 条件 cells. Neither section 4.1, the summary at line 64, the D566 decision row at line 980, P259 at line 1057 nor the M107 row at line 926 touches the 終わり方 or 初期化 cells. A grep of the plan for 終わり方, 途中まで行う and 型検査での拒否 finds nothing that deals with them. After the change, row 3's condition says the error can occur 初期化を始めるとき, with none of the initialization run. The 初期化 cell would still say 途中まで行う, which contradicts it. Row 1 would now cover failures after the program has passed type checking, such as the KIR writer limit, front-end allocation and the KIR thread, as plan lines 385-389 list. Its label 型検査での拒否 would no longer describe all of them. The defect is small and cosmetic, but it is real, and the suggestion to revise both cells as part of D566 and check them in M107 is sound.

## decisions-5

- 観点: decisions(裁定、文案、台帳)
- 重さ: low
- 節: 5.2 D577 (new §6.6 list sentence)
- 反証: 2 人のうち 0 人

### 問題

The proposed sentence nests a comma-separated sub-list inside a comma-separated list of input kinds. In '…型検査に通らなかった入力、読み込み、型検査か実行の前の変換に要する領域が…入力(§15.5)、…', the word '読み込み、' reads as if it began a new outer item. The connective changes between '、' and 'か', so the scope of '要する領域' is ambiguous.

### 根拠

Plan line 787: `字句と構文の誤りのある入力、型検査に通らなかった入力、読み込み、型検査か実行の前の変換に要する領域が処理系の限度を超えた入力(§15.5)、実行時エラーで止まった入力は受理しない。`

### 直し方

Write the inner list with one connective. For example: '読み込み、型検査、実行の前の変換のどれかに要する領域が処理系の限度を超えた入力', or '読み込みか型検査か実行の前の変換に要する領域が…'.

### 反証者 1: 反証できなかった

I confirmed the problem. Plan line 787 (D577, the proposed new first sentence of the third §6.6 item) reads: '字句と構文の誤りのある入力、型検査に通らなかった入力、読み込み、型検査か実行の前の変換に要する領域が処理系の限度を超えた入力(§15.5)、実行時エラーで止まった入力は受理しない。' The outer list of input kinds uses '、'. The inner alternatives (読み込み / 型検査 / 実行の前の変換) also use '、' and then switch to 'か'. So '読み込み、' reads like the start of a new outer item, and the scope of '要する領域' is unclear. The 'か' adds a second ambiguity: '型検査か実行の前の変換' can also be read as '(型検査か実行)の前の変換'. The plan does not deal with this anywhere. Its revision table (lines 1130-1131) added the limit to the enumeration but did not rephrase the sentence. Rewriting the inner list with one connective, as the finding suggests, would fix it. The sentence is meant for LangSpec, so this is worth fixing. Severity is low: it is wording only, and the meaning can be recovered from §15.5.

### 反証者 2: 反証できなかった

I confirmed the problem. Line 787 of doc/log/261104-3-plan.md (inside D577, the proposed new first sentence of LangSpec §6.6 item 3) reads 「…型検査に通らなかった入力、読み込み、型検査か実行の前の変換に要する領域が処理系の限度を超えた入力(§15.5)、実行時エラーで止まった入力は受理しない。」. The outer list of input kinds is separated by 、, and the inner list uses 、 and then か. So 「読み込み、」 can be read as the start of a new outer item. It can also be read as 「読み込み」 plus 「(型検査か実行)の前の変換」, which makes the scope of 「要する領域」 unclear. The plan does not resolve this anywhere else. The text is meant to go into the normative LangSpec, so the ambiguity matters there. The current LangSpec line 737 lists only three kinds and has no nested list. The suggested fix is correct: use a single connective for the inner list, for example 「読み込み、型検査、実行の前の変換のどれかに要する領域が…」. Severity is low (wording only).

## decisions-6

- 観点: decisions(裁定、文案、台帳)
- 重さ: low
- 節: 4.2 D568 / 6 table / 7.3 M108
- 反証: 2 人のうち 0 人

### 問題

D568 says KELEUT_TEST_ALLOC_FAIL is documented only in AGENTS.md's test description, but section 6's AGENTS.md rows do not list it. The 6 table also assigns the AGENTS.md 'JIT の 134 の文(D568 の後の窓)' change to D568, but the M108 commit split in 7.3 has no commit that touches AGENTS.md. Both documentation changes therefore have no milestone home. The same applies to the M109 test-only host memory-limit variable, which is named in M109 but not documented anywhere.

### 根拠

Plan line 506 (変数は…AGENTS.md の試験の説明にだけ書く). Plan line 881 (AGENTS row lists JIT の 134 の文 under D568). Plan line 957 (M108 commits: 4 rustkeleut/test commits, none for AGENTS.md). AGENTS.md currently says the JIT pairs 'can abort with SIGABRT (exit 134)… recorded in M92'.

### 直し方

Add a 6-table row for KELEUT_TEST_ALLOC_FAIL (and the M109 variable) under D568/D569. Name the commit in 7.3 M108 (and M109) that carries the AGENTS.md text, for example by folding it into the 'test:' commit or adding a 'doc:' commit.

### 反証者 1: 反証できなかった

I confirmed the finding against doc/log/261104-3-plan.md.

1. Line 506 (D568) says `KELEUT_TEST_ALLOC_FAIL` is written only in AGENTS.md's test description. None of the section 6 AGENTS.md rows (lines 881-884) mention it, and grep finds the variable only at lines 491, 514, 558, 608, 612, 642, 643, 653 and 1008. None of those is a documentation row.
2. Line 881 assigns AGENTS.md's 'JIT の 134 の文(D568 の後の窓)' to D568, and that row's milestone range is M106〜M113. But the M108 row in table 7.2 (line 927) lists no AGENTS.md text. The M108 commit split in 7.3 (line 957) has four commits: three `rustkeleut:` commits and one `test:` commit. None of them carries AGENTS.md text. M107, M109 and M110 each say explicitly which commit carries their AGENTS.md sentence. M108 has no such line.
3. M109 (test section at line 559) adds a test-only environment variable for the host's linear-memory limit and says its name is decided in M109. No documentation row or commit covers it.

So both D568 documentation items have no milestone or commit that carries them, and the M109 variable is undocumented. The suggestion is reasonable: add a section 6 row for the variables under D568/D569, and name the AGENTS.md commit (or fold the text into an existing commit) in the 7.3 entries for M108 and M109.

### 反証者 2: 反証できなかった

I checked this against doc/log/261104-3-plan.md and the problem is real. Line 506 says KELEUT_TEST_ALLOC_FAIL "is documented only in AGENTS.md's test description" (`AGENTS.md の試験の説明にだけ書く`). But none of the AGENTS.md rows in the D581 table in section 6 (lines 881-884) mention that variable or a test description. The first AGENTS.md row (line 881) assigns "JIT の 134 の文(D568 の後の窓)" to D568, with the milestone column given only as the range M106〜M113. The M108 row in the 7.2 list (line 927) does not mention AGENTS.md. The four M108 commits in 7.3 are three rustkeleut commits and one test commit, and none of them touches AGENTS.md. This differs from M107, M109 and M110: for each of those, 7.3 says which commit carries the AGENTS.md text (lines 956, 959, 961). The M109 example in tests/wasm_cmd.rs (line 562) passes the memory limit through a test environment variable whose name is decided in M109. Neither D581 nor the M109 commit list says where that variable is documented, though it is test-only, so leaving it undocumented is a smaller gap than KELEUT_TEST_ALLOC_FAIL. The suggested fix fits the plan's own pattern: add the AGENTS.md text to the 6 table and name the commit that carries it in 7.3 M108 (and M109). I did not change any files.

## decisions-7

- 観点: decisions(裁定、文案、台帳)
- 重さ: low
- 節: 10 台帳 / 5.1 D575 / 5.2 D576
- 反証: 2 人のうち 0 人

### 問題

The plan adds new V217 known-diff entries (2 entries for each of the 2 bytes.t socket commands, plus V217 entries for both repl_stdin.t commands if P257 is accepted). The ledger table in section 10, which M116 uses to write the final ledger state, does not list V217, and the previous record states the V217 count ('31 項目'). The plan also gives the chunk reclamation item a new number V242. The record's milestone table lists chunk non-reclamation under V229, so the split is not stated.

### 根拠

261024-3-record.md:5648 (`V217 | 診断の接頭辞 | 既知の差のまま(31 項目)`). 261024-3-record.md:5590 (`M90 | … | 形の表と値スタックの塊を回収しない(V229)`). Plan lines 742, 763, 1017-1042.

### 直し方

Add a V217 row to section 10 ('既知の差のまま。M112 と M113 で項目を足し、件数を M116 に書く'). In the V242 row, note that it is split from V229 (record 5590), as V240 is.

### 反証者 1: 反証できなかった

I confirmed the finding by reading both documents. Section 10 of the plan (lines 1017-1042) has no V217 row. Yet section 5.2 adds V217 known-diff entries: lines 742 and 763 add them for the bytes.t socket commands and for the two repl_stdin.t commands. The previous record's ledger gives V217 as '既知の差のまま(31 項目)' at 261024-3-record.md:5648. M116 (line 935) requires the record to write the final treatment of each item listed in section 10. As written, the changed V217 count would not be carried into the ledger. The paragraph after the table, '番号の無い項目の扱い', does not cover V217 either. On V242: the row cites only '記録の M90 の 4'. The record's milestone table (5590) puts chunk non-reclamation under V229, but the record's ledger row for V229 (5665) is about the shape table only. So noting the split is a small clarity fix, the same way the plan already notes the V240 split. Adding a V217 row, plus that note in the V242 row, is a correct low-severity improvement.

### 反証者 2: 反証できなかった

I confirmed the finding. Section 10's ledger table (plan lines 1017-1042) has no V217 row. Yet the plan adds V217 known-diff entries in two places. M112 adds them unconditionally for the two bytes.t socket commands (5.1, line 742). M113 adds them for the repl_stdin.t commands when P257 is "入れる" (line 763). M116 must check that the record "書く本計画 10 節の各項目の最後の扱い" (line 935), so V217 is outside what the closing record has to cover. The previous record's ledger gives a count for V217 ("既知の差のまま(31 項目)", record 5648), so that count would go stale. M113 does ask for "既知の差の件数の変化を実施記録に書く", but only on the P257-accepted path. M112's row has no such requirement, and M112's V217 additions happen either way. The V242 part also holds, though it is minor. The record's milestone table files chunk non-reclamation under V229 (record 5590). The plan's V242 row cites only "記録の M90 の 4". The V240 row and 4.3 say outright that V240 is split from V229, but nothing does the same for V242. The suggested change is cheap and correct: add a V217 row, and note in the V242 row that it is split from V229. Severity is low.

## decisions-8

- 観点: decisions(裁定、文案、台帳)
- 重さ: low
- 節: 5.3 D580 (V212)
- 反証: 2 人のうち 0 人

### 問題

The closing reason for V212 is marked 【推論】 ('host must emit strings that are not valid UTF-8'), but LangSpec states this directly. The unverified tag weakens a closure that the owner is asked to accept in P265.

### 根拠

doc/LangSpec.md:322-323 (切り出した結果が UTF-8 として正しくないバイト列になることもあり、そのバイト列も `String` の値である).

### 直し方

Replace 【推論】 with a citation of LangSpec §(string slicing) lines 322-323. With that citation, a UTF-8 text decoder cannot be used for output without changing the language semantics.

### 反証者 1: 反証できなかった

I confirmed the finding. Plan line 866 (D580, the V212 row) gives the closing reason as "host は文字列をバイト列のまま書き、UTF-8 として正しくない文字列もそのまま出す必要がある", tags it 【推論】, and cites no source. The rest of the plan never cites LangSpec for this point either. LangSpec states the premise directly. Lines 322-323 say a sliced result may be a byte sequence that is not valid UTF-8, and that it is still a `String` value. Line 324 says `echo`/`echoln` write the string's bytes to stdout without checking or converting them. So the premise is a specified fact, not an inference, and the 【推論】 tag understates it. Citing doc/LangSpec.md §4 (lines 322-324, including the echo sentence at 324) would strengthen the closure the owner is asked to accept in P265. The severity is low and the suggested change is correct.

### 反証者 2: 反証できなかった

The finding holds. In plan 5.3, the D580 table row for V212 gives its reason, "host must write strings as bytes, including strings that are not valid UTF-8", and tags it 【推論】 with no citation (doc/log/261104-3-plan.md:866). The language spec states this directly. doc/LangSpec.md:322-323 says a slice may be invalid UTF-8 and is still a `String` value. Line 324 says `echo`/`echoln` write the string's bytes to stdout without checking or converting them. So the reason follows from the spec, and the 【推論】 tag understates its basis. The plan does not cite LangSpec anywhere for this point (its other LangSpec citations are about §15.5, §8 and §6.6). Swapping the tag for a citation of LangSpec lines 322-324 is correct and makes the closure the owner is asked to accept in P265 better supported. Severity is low, as the finding says.
