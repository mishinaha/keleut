# 計画 261104-3 の初稿(04db99e)の検証の指摘と反証

観点は facts(事実の正しさ)、feasible(実装の見込み)、scope(範囲と漏れ)、tests(試験の網羅)、spec(仕様の文の整合)の 5 つで、観点ごとに検証者を 1 人立てた。
各指摘に反証者を 2 人立てた。
指摘は 52 件である。
指摘と反証の本文は検証者と反証者が英語で書いたもので、そのまま載せる。
本文の行番号は初稿(04db99e)の `doc/log/261104-3-plan.md` の行番号で、改訂の後の計画では動いている。
scratchpad のパスは検証のときのもので、残していない。

## facts-0

- 観点: facts(事実の正しさ)
- 重さ: high
- 節: 5.1 D575 (V238), M112
- 反証: 2 人のうち 0 人

### 問題

D575 can't be built the way it is written. Inside Node, a closed fd 1 looks exactly like `>/dev/null`, so the host cannot detect a closed stdout and exit 74 when the program is started with `node DIR/main.mjs`.

### 根拠

Record 3804-3805 says Node reopens a closed fd 1 as /dev/null at startup. Probe: `node -e '...readlinkSync("/proc/self/fd/1")...fstatSync(1).rdev'` gave `/dev/null 259` both with `>&-` and with `>/dev/null`. `node hiout/main.mjs >&-` exits 0 (reproduced). `keleut run --backend wasm >&-` exits 74 only because the Rust launcher checks fd 1 before it spawns Node. Record 3846 lists the missing item as a 起動の台本 (a launcher script), not a host check.

### 直し方

Change D575 so the check happens before Node starts. For example, `keleut build` writes a small POSIX sh launcher (or a shebang wrapper) that tests fd 1 (`{ : >&1; } 2>/dev/null || { echo ...; exit 74; }`) and then execs node. The other option is to close V238 as a documented limitation of running `node DIR/main.mjs` directly. Change the `tests/wasm_cmd.rs` test to match.

### 反証者 1: 反証できなかった

I confirmed the finding. One detail is slightly overstated, and that does not change the conclusion.

Plan line 554 (D575) asks the host to detect a closed stdout on the `node DIR/main.mjs` path too, and to exit 74. The plan never says how.

The record at doc/log/261024-3-record.md, item 12 (lines 3804-3807), says why that fails. Node reopens a closed fd 1 as /dev/null at startup, so main.mjs cannot tell it apart from a correct `>/dev/null` run. Only `keleut run` checks before spawning Node and passes `KELEUT_STDOUT_CLOSED` (keleut-host.mjs:377). The record's open item (line 3846) is a launcher script (起動の台本), not a host check.

I reproduced this. With `>&-` and with `>/dev/null`, readlink of /proc/self/fd/1 gives `/dev/null`, and fstat gives the same rdev 259 and ino 4.

The one nuance is the open mode. /proc/self/fdinfo/1 shows flags 02100002 (O_RDWR) when Node reopened a closed fd, but 02100001 (O_WRONLY) for `>/dev/null`. So a Linux-only guess is possible. It still misclassifies `1<>/dev/null`, which shows the same 02100002, and the plan does not propose it anyway.

So D575 as written depends on a check the record already found impossible. It needs one of the finding's fixes: check fd 1 in a launcher before Node starts, or close V238 as a documented limitation. The `tests/wasm_cmd.rs` test should change to match.

### 反証者 2: 反証できなかった

I confirmed the finding. D575 (plan line 554) asks the host to see a closed fd 1 under `node DIR/main.mjs` and exit 74, and it gives no mechanism for doing that. Record 261024-3 item 12 (lines 3804-3807) says Node reopens a closed fd 1 as /dev/null at startup, so main.mjs cannot tell that case from `>/dev/null`. The same record (line 3846) lists the missing piece as a launcher script (起動の台本), not a host check. My probe: with `>&-` and with `>/dev/null`, readlink gave /dev/null and rdev 259, inode 4 in both cases. I also looked for a way around it. /proc/self/fdinfo/1 does differ: flags 02100002 (O_RDWR, Node's reopen) under `>&-` and 02100001 (O_WRONLY) under a shell `>/dev/null`. That test is not usable, though. Python's subprocess.DEVNULL also gives 02100002, so a program run that way with discarded output would wrongly exit 74. `1<>/dev/null` gives the same flags, and the test only works on Linux. keleut-host.mjs (line 377) already relies only on KELEUT_STDOUT_CLOSED, which the Rust launcher sets. So D575 as written, together with its planned test that `node DIR/main.mjs >&-` exits 74, cannot be built reliably. The suggestion is sound: check fd 1 before Node starts, with a launcher script written by `keleut build`, or close V238 as a documented limitation.

## facts-1

- 観点: facts(事実の正しさ)
- 重さ: medium
- 節: 1.4 (REPL の表示の書式), 5.2 D578, M107
- 反証: 2 人のうち 0 人

### 問題

The plan says LangSpec does not define a display format anywhere. That is false: LangSpec section 8 already fixes part of how values are displayed. The proposed §6.6 sentence (受理した入力の結果を表示するかどうかと、その書式は、処理系が定める) would then contradict that existing rule unless it is qualified.

### 根拠

doc/LangSpec.md:825-828: 処理系が型や診断を表示するときは、`_item` だけを持つ閉じた行をタプルの記法で表示する。…空の行は、型なら `{}`、値なら `()` と表示する。 Also :1860 (nan/inf spelling, under 表示). The plan's 1.4 relies on decide.md 4.1, which only looked at §6.6, §18 and §18.5.

### 直し方

Correct 1.4. Reword the D578 sentence so it keeps §8, for example 「…その書式は、本書が定める綴り(§8 の行の表示など)を除いて処理系が定める」. Add §8 to the consistency lenses in section 12.

### 反証者 1: 反証できなかった

I confirmed the finding. Section 1.4 of the plan (line 177) says "LangSpec は表示の書式をどこにも定めず". That is false. doc/LangSpec.md:825-828 (§8) requires that a closed row with only `_item` be shown in tuple notation whenever the implementation displays types or diagnostics, that a one-element tuple keep its comma, that an empty row be shown as `{}` for a type and `()` for a value, and that an open row keep `_item`. §16.2 (around lines 1850-1862) also fixes the Show spellings, including the Float64 rules and nan/inf/-inf. The REPL output follows these rules. Piping `let x = (1,)` and `let y = ()` into `keleut repl` printed `x : (Int32,) = (1,)` and `y : {} = ()`, so the §8 rules govern this exact output. Plan lines 594-598 (D578) would add, without qualification, "受理した入力の結果を表示するかどうかと、その書式は、処理系が定める" to §6.6. That would conflict with §8 unless it is qualified. Searching the plan for §8 found no mention of it, so the plan does not already handle this. The suggested correction (fix 1.4, and qualify the D578 sentence so the spellings LangSpec already fixes, §8 and the Show rules, still apply) is sound.

### 反証者 2: 反証できなかった

I could not refute the finding. I checked each part in the repository.

1. The plan's claim is false as written. Plan 1.4 (line 177) says "LangSpec は表示の書式をどこにも定めず". But doc/LangSpec.md:824-827 (§8) sets display rules. When an implementation displays types or diagnostics, a closed row that has only `_item` is shown in tuple notation, and the trailing comma stays even for one element. An empty row is shown as `{}` for a type and `()` for a value, and an open row is not shown as a tuple. LangSpec.md:1850-1860 also sets display spellings: the Show table for `List` and `Option`, the Float64 notation, and `nan`/`inf`/`-inf` ("と表示する").

2. The plan does not handle this anywhere. It never cites §8. D578 (lines 594-605) mentions only §16.2, and only when it argues for V241.

3. The consistency lenses in section 12 (line 826) list §15.5, §6.6, IRSpec 1/11.1/14, §13.6, §18 and §18.5. §8 is not among them.

4. The proposed §6.6 sentence would conflict with §8. The sentence is 「受理した入力の結果を表示するかどうかと、その書式は、処理系が定める。」 REPL output shows types such as tuple types, and §8 fixes how those are shown. Read without a qualifier, the sentence leaves to the implementation what §8 already fixes.

So the suggested change holds: correct 1.4, qualify the D578 sentence so it keeps §8 (and the §16.2 spellings), and add §8 to the lenses in section 12.

## facts-2

- 観点: facts(事実の正しさ)
- 重さ: medium
- 節: 5.2 D576 (親の作業), M113
- 反証: 2 人のうち 0 人

### 問題

The plan says to remove 「Diktor は止まる」 from the `bytes.t` comment at lines 31-34. If that deletes a line, every later command in bytes.t moves up. known-diffs.toml has 8 V217 entries keyed on bytes.t lines 57, 63, 70 and 73. They would no longer match, so the M113 acceptance would show new diffs plus unused known diffs. The plan does not mention this.

### 根拠

known-diffs.toml:280-345 has bytes.t entries at `line = 57/63/70/73` (ledger V217). bytes.t:31-33 is the comment block, and line 33 is 「Diktor は Sys_blocked_io の例外で止まる(この試験は Diktor と一致しない):」.

### 直し方

In D576/M113, either rewrite the sentence in place so the line count stays the same, or update the `line =` values of the remaining bytes.t entries in the same commit. Then check with `kconform --pairs dk,dk-ir,kf,x-ir,ki conformance/bytes/bytes.t` that the unused-known-diff count does not change.

### 反証者 1: 反証できなかった

I confirmed the problem; the finding stands. In bytes.t, line 33 is the whole sentence 「Diktor は Sys_blocked_io の例外で止まる(この試験は Diktor と一致しない):」. The plan writes one sentence per line, so removing that sentence would most naturally delete the whole line. Every later command would then move up one line. known-diffs.toml:280-345 holds 8 V217 entries for conformance/bytes/bytes.t, keyed on lines 57, 63, 70 and 73. kconform only accepts an exact line match (crates/kconform/src/known_diffs.rs:128, `self.line == line`), so after the deletion these entries would stop applying. The 4 commands would show up as new diffs, and all 8 entries as unused known diffs. Plan section 5.2 (D576, line 571) only says to delete the V218 entries at known-diffs.toml:416-434 and to remove 「Diktor は止まる」 from the bytes.t:31-34 comment. Neither 5.2 nor the M113 row in 7.2 (line 703) mentions keeping the line count the same or updating the `line =` values of the remaining V217 bytes.t entries. M113's acceptance check (bytes.t with zero diffs, and no increase in unused known diffs) would catch the problem, but the plan gives no step that prevents it. The fix the finding suggests is right: rewrite the line in place, or update lines 57/63/70/73 in the same commit.

### 反証者 2: 反証できなかった

I confirmed the problem. Line 33 of rustkeleut/conformance/bytes/bytes.t is the whole sentence 「Diktor は Sys_blocked_io の例外で止まる(この試験は Diktor と一致しない):」, which is the text D576 says to remove (plan line 571). Once diktor is fixed the whole sentence becomes untrue, so the natural edit deletes the line. That moves every later command up by one line. known-diffs.toml has 8 V217 entries for bytes.t at line = 57, 63, 70 and 73 (lines 282-339). kconform matches an entry only when its line is exactly equal (known_diffs.rs:128, `self.line == line`). After the shift, those 8 entries would stop matching: the acceptance would report new diffs and the entries would show as unused. Section 5.2 / D576 and the M113 row (plan line 703) do not mention keeping the line count or updating the `line =` values. The M113 acceptance (no new unused known diffs, and bytes.t with no diffs) would catch the breakage but does not prevent it. The suggested fix is correct: either rewrite the sentence in place so the line count stays the same, or update the 57/63/70/73 entries in the same commit.

## facts-3

- 観点: facts(事実の正しさ)
- 重さ: medium
- 節: 7.1 共通の条件 3
- 反証: 2 人のうち 0 人

### 問題

The plan tells every milestone commit to omit `Co-Authored-By`. Its reason is a property of the session that wrote the plan, and it is stated as fact. The rule contradicts .claude/rules/commit.md, plan 261104-1, and the commits just before this plan.

### 根拠

.claude/rules/commit.md requires `Co-Authored-By: Claude <noreply@anthropic.com>`. doc/log/261104-1-plan.md:542 says to add it. 69febfd, a648352 and ecde210 all carry `Co-Authored-By: Claude Opus 5.5`. Only the plan commit 04db99e lacks it. The owner's practice (re-adding trailers to 200 parent and 32 diktor commits on 2026-11-04) shows the trailer is wanted.

### 直し方

Make the rule conditional: follow commit.md and add `Co-Authored-By` with the model actually used. Leave it out only when the implementing session's own system instructions forbid attribution, and in that case record and report the conflict.

### 反証者 1: 反証できなかった

I confirmed the finding; it is not refuted. doc/log/261104-3-plan.md:679-681 tells every milestone commit to leave out `Co-Authored-By`. The only reason given is that the plan-writing session's system instructions forbid attribution, which describes the session that wrote the plan. The plan does say to record the conflict and report it, but the rule itself applies to all later sessions whether or not those sessions are under the same restriction. This contradicts .claude/rules/commit.md, which requires `Co-Authored-By`, and plan 261104-1 section 7.1 item 3 (line 542), which says to add it. The trailer is present on 69febfd, a648352 and ecde210 but missing from the plan commit 04db99e. The owner's memory note (commit-trailer-conflict) also makes the rule conditional: leave the trailer out only when the session's own system instructions forbid attribution, and report it. The suggested fix, to follow commit.md with the model actually used and leave the trailer out only when the implementing session is itself restricted, matches that rule and is correct.

### 反証者 2: 反証できなかった

I confirmed the finding. Lines 679-681 of doc/log/261104-3-plan.md (7.1 item 3) tell every milestone commit to leave out `Co-Authored-By`. The only reason given is that the plan-writing session's system instructions forbid the trailer, and the plan states this as fact. That reason belongs to one session. It does not carry over to the sessions that will implement the plan.

The repo and history all point the other way:
- .claude/rules/commit.md requires the trailer.
- doc/log/261104-1-plan.md:542 says to add it.
- 69febfd, a648352, ecde210 and 5cbfe33 all carry `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Only 04db99e, the plan commit, lacks it.
- The owner's memory note commit-trailer-conflict.md gives the rule as conditional: add the trailer when the session's system instructions do not forbid it, and leave it out and report only when they do. On 2026-11-04 the owner also had the trailer re-added to 200 parent and 32 diktor commits.

The suggested conditional wording matches that established rule, so the fix is correct.

## facts-4

- 観点: facts(事実の正しさ)
- 重さ: medium
- 節: 4.1 D566 item 3, 4.4 D570 item 3
- 反証: 2 人のうち 0 人

### 問題

D566 and D570 say exit 1 (the type-check rejection row of §15.5) when the KIR-processing thread can't be created. They don't separate `.kel` input from `.kir` input. For `keleut run FILE.kir` no Keleut type check happens, and IRSpec gives reader-side limits status 65. AGENTS.md also says a deep `.kir` exits 65 when the KIR nesting limit is lowered to fit the stack.

### 根拠

doc/IRSpec.md:1655 and :1670: readers reject with 終了状態 65, including nodes deeper than `N_nest`. AGENTS.md: `the KIR nesting limit drops to what fits, so a deep .kir exits 65`. D570 item 3 keeps that lowering but gives 1 for spawn failure.

### 直し方

Give the status for each input kind: 1 for `.kel` (front end, then KIR, per D566), and for `.kir` input either 65 (a reader limit, as with the lowered nesting limit) or a status IRSpec defines. Add a test for `.kir` input under `ulimit -v` to the M110 tests.

### 反証者 1: 反証できなかった

I checked this and the finding holds. D570 item 3 (plan line 459) says that when the KIR-processing thread cannot be created even at the 16 MiB floor, keleut exits 1 "(D566)". D566 (lines 315-331) widens the LangSpec §15.5 row for status 1, which covers type-check rejection, to cover "実行の前の変換", defined as what happens "型検査の後、初期化を始める前". That thread is not used only after the front end. In rustkeleut/crates/kir/src/lib.rs:105-148, `on_stack` (the spawn at line 117, the one D570 cites) is also what `read_kir_with` and `read_kirt_with` use. So the rule also covers `keleut run FILE.kir`, where no Keleut type check happens. That case belongs to IRSpec, and IRSpec gives reader-side rejections status 65 (doc/IRSpec.md, end of 13.8: "13.7 節の検査と本節の検証のどちらで拒否したときも、終了状態 65"). It also gives 65 for rejections at the nesting limit (13.9). AGENTS.md says the lowered KIR nesting limit makes a deep `.kir` exit 65. Nothing in the plan separates the two input kinds. D567's revised IRSpec §1 exclusions (lines 341-347) list only V8/Node, LLVM and Rust allocation failure after the reserve (134), and SIGKILL (137). They do not cover failing to create the KIR thread. The M110 tests (lines 466-467) only expect 0 for `.kir` under `ulimit -v 1000000`. None of them tests the floor-failure status for `.kir` input. So for `.kir` input the plan picks status 1 with no basis in LangSpec or IRSpec, and that gap is real.

### 反証者 2: 反証できなかった

I confirmed the problem in the repo. The KIR-processing thread that D570 item 3 resizes is created by `kir::on_stack` (rustkeleut/crates/kir/src/lib.rs:105-128). `read_kir_with` and `read_kirt_with` use that same thread, so it is also the reader thread for `keleut run FILE.kir`, where no Keleut type check runs.

The plan does not separate the two input kinds. D566 item 3 (plan line 333) and D570 item 3 (plan line 459) both give status 1 when the thread cannot be created at the 16 MiB minimum. No text anywhere in the plan gives a status for `.kir` input. Grepping the plan for FILE.kir, the input kind and `65 で` finds nothing.

The M110 tests run `.kir` input only in the success case (`ulimit -v 1000000`, expected 0). No test covers `.kir` input when the thread cannot be created.

IRSpec gives reader-side outcomes status 65. Line 1655 says rejection by the 13.7 checks or by verification ends with 65. Line 1670 says nodes deeper than `N_nest` are rejected with 65. AGENTS.md says the lowered nesting limit makes a deep `.kir` exit 65. D570 item 3 keeps that lowering, so the plan would give the same `.kir` input 65 when it is too deep for the stack and 1 when the stack cannot be allocated at all. That is inconsistent.

The new §15.5 sentence does list 読み込みと検証 among the 実行の前の変換. But that table is the Keleut source program's table (type check, import), and IRSpec 11.1 and 13.8/13.9 define the statuses for a KIR program. So the plan needs either a status per input kind or an explicit argument that 1 also applies to `.kir`. The suggestion is reasonable: 1 for `.kel`, and 65 or an IRSpec-defined status for `.kir`, plus a `.kir` test under `ulimit -v` in M110.

## facts-5

- 観点: facts(事実の正しさ)
- 重さ: medium
- 節: 3.1 D564
- 反証: 2 人のうち 1 人

### 問題

`B_prog = B_wasm + k × (B_wasm − E_rres)` assumes each extra cancel layer adds only a `cancel` clause and one `$rt_handle`, with no extra `rt_resume`. That assumption is stated without the 【推論】 mark. gaps.md says the combination of a resumed continuation with stacked cancel clauses was never tried. The planned acceptance (measured bytes per level ≤ `B_prog`) can't detect a formula that is too small, because the largest measured value (≈1,683) is far below `B_prog` (2,896).

### 根拠

gaps.md 1.6: 「G の節の継続の再開(`rt_resume` が同じ段に加わる形)と `cancel` 節の重なりを組み合わせた形は試していない」. func.rs:445-463: the base budget for a cancel clause already subtracts `E_RRES`, i.e. it assumes one `rt_resume` per level. Re-ran: c92m (N=299,000, `--max-depth 600000`) kw 70 / interp 0, c92n (N=280,000) kw 0, c92 (`--max-depth 1048576`) kw 70. These confirm the bug, not the bound.

### 直し方

Mark the per-layer composition 【推論】. In M106, add a probe and a unit-test shape where the handled body inside the nested cancel handler resumes a G-captured continuation, and measure bytes per level. Alternatively use the safer `B_prog = (k+1) × B_wasm` (176 bytes more per layer, still the Worker path).

### 反証者 1: 反証できなかった

The finding holds. In the plan (doc/log/261104-3-plan.md), line 266 gives the formula `B_prog = B_wasm + k × (B_wasm − E_rres)`. Line 268 says each added layer adds only a 1,040 `cancel` clause plus a 320 `$rt_handle` (= 1,360), and it carries no 【推論】 mark. Section 3.1 and the risk table (lines 766-767) never mention `rt_resume` stacking inside a nested layer. The only `rt_resume` text (line 95) covers the base layer.

The evidence supports the finding:
- gaps.md 1.6 (doc/log/261104-3-spike/survey/gaps.md:140-141) says the combination of a resumed continuation with `cancel` stacking was never tried. Its claim that the combination is not harsher is itself marked 【推論】, and it covers only the single-layer case.
- `budget_of` (func.rs:445-463) and `E_RRES` (func.rs:435, a separate helper frame) show that one `rt_resume` is budgeted per logical level.

lir.md 16.3 points to a concrete path where `rt_resume` can stack at the nested depth `h`. When a G clause returns a value without resuming, `$rt_handle` does `tail_call_direct $rt_handle (hr kk p)` in resume mode to inject DISCARD. The `rt_resume` helper frame then sits at depth `h` while the continuation's `cancel` clauses run. Those clauses can contain further handler expressions. So inside a `cancel` clause's handler expression, one layer can plausibly hold `cancel` (1,040) + `$rt_handle` (320) + `rt_resume` (176) = `B_wasm`, which is more than the plan's 1,360.

The planned acceptance check would not catch this. It only requires the measured bytes per level (at most about 1,683) to stay below `B_prog` (2,896), and that holds whether or not the formula is too small.

The suggested remedy is sound. Either mark the composition 【推論】 and add a probe or unit-test shape that resumes or injects inside the nested `cancel` handler, or use `(k+1) × B_wasm`, which costs 176 bytes more per layer and stays on the `Worker` path.

### 反証者 2: 反証した

The finding says each extra cancel layer could also carry its own rt_resume, so B_prog = B_wasm + k×(B_wasm − E_rres) might be too small. The code rules this out. rt_resume (cont_resume) is a helper that the $rt_handle frame calls in step 2 of doc/lir.md 16.3. In keleut-wasm it is called from inside the lowered $rt_handle body (func.rs:2009, rt.rs gen_rt_resume). The cancel clause runs in step 4.5 of that same $rt_handle, and only after cont_resume has returned the sentinel. So when a cancel clause frame sits on a $rt_handle at depth h, that handler's rt_resume is already gone from the stack. A cancel clause also cannot hold a non-tail resume of an outer continuation (LangSpec 1415-1416). So within one logical depth, only the topmost $rt_handle can have an rt_resume above it, and every lower $rt_handle has a cancel frame above it instead. The finding's proposed shape is the worst case. Inside the nested cancel handler, a G clause's tail resume makes the inner $rt_handle tail-call itself and then call rt_resume at depth h. The stack at depth h is then fn 1040 + rth 320 + cancel 1040 + rth 320 + rres 176 = 2896, which equals B_prog for k=1. Each further layer adds only cancel + rth = 1360 = B_wasm − E_rres. The single rres in the base B_wasm already covers the one rt_resume a depth can have, so the formula is not too small. Switching to the 'safer' (k+1)×B_wasm would only add slack that is not needed. A missing 【推論】 tag on a statement that follows from the lir.md 16.3 step order is a wording nit at most, not a medium defect. The finding's re-runs (c92m/c92n/c92) only reproduce the known bug and say nothing against the bound.

## facts-6

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 1.3, 1.4 (record citations)
- 反証: 2 人のうち 0 人

### 問題

Two record line citations are off by a few lines. The plan cites 4562-4564 for M98 item 6 (`keleut check` not freeing memory) and 4564-4566 for M98 item 5 (the parse-limit counting).

### 根拠

doc/log/261024-3-record.md: item 5 is at 4563-4564 and item 6 at 4565-4567. Plan sections 1.3, 1.4 and 4.4 item 1 use the wrong ranges. critic.md 3.8 repeats 4564-4566 for item 5.

### 直し方

Change to 4563〜4564 (item 5) and 4565〜4567 (item 6).

### 反証者 1: 反証できなかった

The finding is correct. In doc/log/261024-3-record.md, M98 item 5 (the parse-limit counting) is on lines 4563 to 4564, and item 6 (`keleut check` not freeing the type-check context and trees) is on lines 4565 to 4567. The plan cites 4562〜4564 for item 6 at lines 149 and 452 (sections 1.3 and 4.4) and 4564〜4566 for item 5 at line 168. Both ranges are off by one or two lines. The suggested fix is right: 4563〜4564 for item 5 and 4565〜4567 for item 6.

### 反証者 2: 反証できなかった

I confirmed the finding. In doc/log/261024-3-record.md, M98 item 5 (counting the parse limit by tree depth) is at lines 4563-4564. Item 6 (`keleut check` does not free the type-check context and trees) is at lines 4565-4567. Line 4562 is item 4's last line. The plan gets these ranges wrong in three places. Line 149 cites 4562〜4564 for item 6, line 168 cites 4564〜4566 for item 5, and line 452 cites 4562〜4564 for the forget decision. The fix should be 4563〜4564 for item 5 and 4565〜4567 for item 6.

## facts-7

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 5.2 D577 (V237)
- 反証: 2 人のうち 0 人

### 問題

The plan says `conformance/frontend-depth/typecheck.t` already fixes the V237 behaviour, so no tests need changing. The file fixes only the 50,000-level `else if` chain (0) and the 50,000/100,000-level `id(...)` cases. The 100,000-level `else if` chain, where keleut exits 2 and Diktor 0 and which V237's own description names, is not fixed.

### 根拠

typecheck.t:76-78 (50,000-level else-if) and :101-110 (ids5/ids10). The file has no 100,000-level else-if command.

### 直し方

Either add a 100,000-level `else if` command to typecheck.t (via `kconform expect`, with the same cost as ids10), or reword D577 so the claim covers only `id(...)`.

### 反証者 1: 反証できなかった

The finding is confirmed. In plan line 587 (D577), the plan says `conformance/frontend-depth/typecheck.t` "既に固定している", so no code or tests change. Line 798 describes V237 as covering the 100,000-level `id(...)` chain and the 100,000-level `else if` chain. typecheck.t has three relevant commands: a 50,000-level `else if` chain that exits 0 (lines 76-81), and ids5 (exit 0) and ids10 (exit 2) at lines 105-113. Nothing under rustkeleut/conformance outside corpus/ writes a 100,000-level `else if` chain. The only `else if` chain generator is typecheck.t:78, at 50,000 levels. So the 100,000-level `else if` half of V237 is not fixed by any test, and the claim is wrong as written. Either remedy is reasonable: add the command, or narrow the wording to `id(...)`.

### 反証者 2: 反証できなかった

I checked the finding and it holds. The plan's ledger row for V237 (plan line 798) says that both the 100,000-level `id(...)` case and the 100,000-level `else if` chain end with exit 0 in Diktor and exit 2 in keleut. D577 (line 587) then says `conformance/frontend-depth/typecheck.t` already fixes this behaviour, so no code or tests change. That file has the 50,000-level else-if chain (lines 76-81, exit 0) and ids5/ids10 (lines 105-113), but no 100,000-level else-if command, and no other frontend-depth .t file has one. I ran the case myself (scratch work/ref2-f7/elif10.kel). `keleut check` printed '処理系の限度: 構文解析で再帰が深すぎます(入れ子の限度 200000 を超えました)' and exited 2, while `diktor --type-check` printed 'f : (Int32) => Int32' and exited 0. So the else-if half of V237 is real but no test fixes it. Both suggested fixes are sound: add a 100,000-level else-if command to typecheck.t (through `kconform expect --promote`), or limit D577's claim to `id(...)`. Severity is low: ids10 already exercises the same tree-depth limit.

## facts-8

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 1.3, 5.1 D574, 8 (D574 row)
- 反証: 2 人のうち 0 人

### 問題

The plan gives M101's reason for having no `float/` cram as only 'no milestone assigned it' (「理由は割り当てが無かったことだけ」). The record and the README also give a second reason: Float64 printing is already covered by prim.t and fltcmp.rs. D574's justification leaves that out.

### 根拠

Record 5461-5464: 「どのマイルストーンの行もこの試験を割り当てておらず、Float64 の印字は別の 2 か所で確かめているからである」. rustkeleut/conformance/README.md:23-24 names prim/prim.t and tests/fltcmp.rs.

### 直し方

State both reasons and say what float.t adds beyond prim.t and fltcmp.rs: literal reading through the kr-* and x-ir front ends, and wasm32 printing without wasm_check.mjs.

### 反証者 1: 反証できなかった

The finding holds. The plan says three times that M101's only reason was that no milestone assigned the test: line 158-159 (section 1.3), line 554 (D574's 理由) and the D574 row at line 753 (「M101 の理由は割り当てが無かったことだけ」). The record at 5461-5464 gives a second reason: Float64 printing is already checked by prim/prim.t (the §16.2 spelling, libm, conversions) and by keleut-rtcore tests/fltcmp.rs (comparison with glibc). README.md:23-24 names the same two places. So the plan misquotes its source. Only part of the suggestion is already in the plan. Line 555 says what float.t adds: literal reading through kr-* and x-ir, and wasm32 printing through kw and kr-kw without wasm_check.mjs. Lines 545 and 548 split the work with prim/. What is missing is the plan stating the second reason and answering it: why prim.t and fltcmp.rs are not enough for literal-boundary values. The fix is to drop 「だけ」 and state both reasons, pointing to the existing line 555 as the answer to the second.

### 反証者 2: 反証できなかった

I confirmed the finding. The plan gives "no milestone assigned it" as M101's only reason in three places: line 159 (§1.3, 「理由は、どのマイルストーンもこの試験を割り当てていなかったことである」), line 549 (D574, 「M101 が置かないとした理由は、どのマイルストーンも割り当てていなかったことである」) and line 753 (the 裁定 row, 「M101 の理由は割り当てが無かったことだけ」). The record gives a second reason at doc/log/261024-3-record.md:5461-5464: 「…割り当てておらず、Float64 の印字は別の 2 か所で確かめているからである」. That passage names conformance/prim/prim.t and keleut-rtcore tests/fltcmp.rs. rustkeleut/conformance/README.md:23-24 says the same. So the plan misstates M101's reasoning, and the word 「だけ」 is wrong. Part of the suggestion is already done: D574 (lines 550-551) already says what float.t adds, namely literal reading through the kr-* and x-ir pairs and wasm32 printing through kw and kr-kw without wasm_check.mjs. The fix that remains is to state both reasons in §1.3, D574 and the 裁定 row, and to say why prim.t and fltcmp.rs do not cover what float.t would test.

## facts-9

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 1.2 (V229, V230 現状)
- 反証: 2 人のうち 0 人

### 問題

Two parts of 1.2 are imprecise. (a) It says only the GC heap is outside the Rust global allocator. LLVM's C++ allocations (malloc/new in the compile thread) are also outside it, which D567 relies on. (b) For kw, it says running out of memory gives 「形の表を広げられません」. In fact host `shapes_grow` → `newTable` → `alloc` exits 70 with 「線形メモリを広げられません」. 「形の表を広げられません」 is reached only when `krt_shapes_copy` fails.

### 根拠

host/keleut-host.mjs:395-405 (alloc → `exit(70)` with 線形メモリを広げられません) and :691-697 (`shapes_grow` returns 1 only on a copy failure). crates/keleut-wasm/src/rt.rs:255.

### 直し方

Reword 1.2 to name both kw messages. Say the GC heap is the only allocation of keleut-rt/keleut-interp outside the global allocator, and that LLVM's allocations are also outside it.

### 反証者 1: 反証できなかった

I could not refute it; both parts hold. Part (b), the clear one: plan line 126 says that in kw, failing to grow the shape table gives 「形の表を広げられません」 (exit 70), and it cites host/keleut-host.mjs:404. That line actually prints 「線形メモリを広げられません」. It is inside `alloc()` (lines 396-410), which calls exit(70) when `mem.grow` fails. `shapes_grow` (lines 691-698) calls `newTable`, and `newTable` calls `alloc`. So when memory runs out, the run exits 70 with the linear-memory message before `shapes_grow` can return. `shapes_grow` returns 1 only when `krt_shapes_copy` fails, and only that path reaches the 「形の表を広げられません」 template at rt.rs:255. The plan's cited line contradicts its own text. Part (a): line 131 says the GC heap's `mmap` is the only allocation outside the global allocator. That is true within keleut-rt, but LLVM's C++ allocations do not go through Rust's global allocator either. The plan itself mentions LLVM's `bad_alloc` two bullets earlier. This is a minor imprecision. The suggested rewording is correct and low-risk.

### 反証者 2: 反証できなかった

I checked both parts of the finding against the repo, and both hold.

(b) The plan at line 126 says that in kw, failing to grow the shape table gives the internal error 形の表を広げられません (70), and it cites host/keleut-host.mjs:404. The code does not do that. shapes_grow (keleut-host.mjs:691-697) calls newTable, and newTable (:416-418) calls alloc. When mem.grow throws, alloc prints 「keleut: 内部エラー: 線形メモリを広げられません」 and exits 70 (:395-405). Line 404 is that message, not the shape-table one. shapes_grow returns 1 only when krt_shapes_copy fails, and only that path reaches the 形の表を広げられません template in rt.rs:255. So running out of memory gives 線形メモリを広げられません. The plan's own line 421 names the alloc path separately, which shows the two are different.

(a) Line 132 says the GC heap's mmap is the only allocation outside the global allocator. The plan itself contradicts this in several places: line 130 (LLVM ERROR / std::bad_alloc), lines 345 and 353, and D567 (the LLVM C++ allocations cannot be caught from Rust). Read in context the sentence may mean keleut-rt only, but as written it is inaccurate.

Both fixes are only wording changes. They are correct and do not affect the design.

## facts-10

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 4.2 D568 item 1-2
- 反証: 2 人のうち 0 人

### 問題

D568 defines four roles (実行, フロントエンド, 翻訳, そのほか) but gives a place to check the flag only for the first three. A failure in the KIR read/verify/lower thread or the main thread uses up the shared 8 MiB reserve and sets a flag nobody reads. That thread then continues without a reserve, and its next failure gives 134 again.

### 根拠

Plan 4.2 item 2 lists check sites for 実行, フロントエンド and 翻訳 only. LIR lowering (`Points`, live sets) allocates heavily before the run starts (record 1963-1969).

### 直し方

Say where the そのほか flag is read. For example, check it after KIR reading, verification and lowering, and end with the status D566/IRSpec gives for that stage. Alternatively state that those threads deliberately keep the 134 window and list it in D567.

### 反証者 1: 反証できなかった

I confirmed the finding against /home/sumito3478/repo/keleut/doc/log/261104-3-plan.md. In section 4.2, D568 item 1 (line 365) names four roles: 実行, フロントエンド, 翻訳 and そのほか. Item 2 (lines 372-380) says where the flag is read for 実行 (the Rt heap entry and the value-stack chunk path), フロントエンド (after type checking and KIR writing, see 4.4) and 翻訳 (stop JIT translation). The only other entry is the keleut repl line about re-taking the reserve. A search for そのほか turns up only line 365. Nothing says where the そのほか flag is read, and nothing says the KIR read, verify and lower thread or the main thread deliberately goes unchecked. D570 item 3 (4.4) changes only the stack size of the KIR thread. It does not check the flag. So a failure on that thread uses up the shared reserve, the retry succeeds and the flag is never read. Any later failure on any thread, including the run thread, then has no reserve and gives 134 instead of 3. D567 item 2 covers 134 after the reserve is used up only in general terms. It does not name this stage or say the gap is intended, so the plan leaves an unexplained loose end. The suggested change (name a check site with the D566/IRSpec status for that stage, or document the window under D567) is reasonable and would not be wrong.

### 反証者 2: 反証できなかった

I confirmed the finding against the plan text. Section 4.2, D568 item 1 (lines 365-366) gives every thread one of four roles: 実行, フロントエンド, 翻訳, そのほか. Item 2 (lines 374-383) only says where the flag is read for 実行 (the Rt heap entry and value-stack chunk paths), フロントエンド (after type checking and KIR writing, see 4.4 item 2) and 翻訳 (discard the translation and stay on the interpreter). It never says where the そのほか flag is read, and no other part of the plan does either: 4.4 and the risk row at line 769 are silent on it.

Two threads get the そのほか role:
- the main thread;
- the KIR thread, which kir::on_stack in crates/kir/src/lib.rs:105-127 spawns under the name "kir". run_cmd.rs runs verify_and_lower there.

Because the plan keeps a single reserve, set aside at startup (line 367), a failure on one of these threads uses up the 8 MiB reserve and sets a flag that nothing reads. That thread keeps going without a reserve, so its next failure ends in 134. The execution thread then also starts with no reserve, which cancels much of the benefit of M108 for heavy .kir programs.

D567 item 2 does cover the outcome in general ("予備を使い切った後に Rust の確保が失敗する場合、134"). Even so, the plan neither defines a check site for そのほか nor says this window is deliberate. The suggested fix is sound: either check the flag after reading, verifying and lowering, and exit with the status D566 gives for the pre-execution stage, or state the gap explicitly in D567. The finding holds.

## facts-11

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 4.1 D567 item 1, 4.6 D572
- 反証: 2 人のうち 1 人

### 問題

D567 says Node failing to get its stack or threads under a virtual-memory cap ends with 70. That is true only when the program runs through `keleut run --backend wasm`. Running the `keleut build` output directly with `node DIR/main.mjs` dies with a signal status.

### 根拠

Probe under `ulimit -v 524288`: `keleut run --backend wasm hi.kel` → 70. `node hiout/main.mjs` → 133 (`Fatal process out of memory: Failed to reserve virtual memory for CodeRange`, SIGTRAP). This matches record 3702-3703.

### 直し方

In the IRSpec exclusion and the AGENTS.md text, say that 70 is `keleut run`'s status and that the standalone `node DIR/main.mjs` can end with 133/134. Also say that the 2 GiB / 4 GiB thresholds were measured at the default L with `B_wasm` and move once D564 raises the stack for k≥1.

### 反証者 1: 反証できなかった

The finding holds. D567 item 1 (plan line 344) says that when Node cannot get its stack or threads under a virtual-memory cap, the program ends with 70. I reproduced it with a corpus program that runs successfully. Under `ulimit -v 524288`, `keleut run --backend wasm` exited 70, but running the `keleut build --target wasm` output directly with `node out/main.mjs` exited 133. That is V8's `CodeRange` reservation failing, which raises SIGTRAP, and record lines 3702-3703 say the same. At 2 GiB and 4 GiB both paths exited 70. So the signal status shows up only at the 512 MiB cap, not at every cap as the finding's wording suggests. The plan does treat `node DIR/main.mjs` as a launch path whose exit status matters: D575 (line 554) gives it the same 74 as `keleut run`. So D567 item 1, and D572's AGENTS.md text, should say that 70 is the status of `keleut run --backend wasm`, and that the standalone `node DIR/main.mjs` can end with 133 (or 134) when the cap is 512 MiB. The finding's second suggestion is guesswork, not a measured fact, and does not change this verdict. It says the 2 GiB / 4 GiB thresholds will move once D564 raises the stack.

### 反証者 2: 反証した

The measurement is right, but the plan is not wrong where the finding says it is. I probed it myself under ulimit -v. At 524288 KiB, `keleut run --backend wasm` exits 70 ("node がシグナル 5 で終わりました"), while `node DIR/main.mjs` on the `keleut build` output exits 133 (CodeRange reservation). At 2 GiB and 4 GiB both exit 70. This is what the record says at lines 3702-3703.

The finding does not apply to the text the plan changes. D567 edits the exclusion list in IRSpec section 1. That section (doc/IRSpec.md lines 26-27) defines a reader as "KIR を読み込み、検証し、実行する処理系" and names only `diktor --run-ir` and `keleut run`. The standalone `node DIR/main.mjs` is not an IRSpec reader, so the IRSpec guarantee and its exclusions do not cover it, and "(70)" is the right status for the reader `keleut run`.

D572's AGENTS.md text also goes into the `--backend wasm` item of `keleut run` and describes the `kw` pair. That pair always runs `keleut run --backend wasm`, which exits 70 at 512 MiB, 2 GiB and 4 GiB.

The suggestion also asks to say that the 2 GiB and 4 GiB thresholds move once D564 raises the stack. That is speculation, not a confirmed defect. Adding standalone-node exit codes to the IRSpec exclusion would describe a non-reader there, which would be wrong.

(A separate observation, outside this finding: at 2 GiB the 70 came from keleut's own "実行のスレッドを作れません(1656 MiB のスタック)", not from a Worker. At 4 GiB it was ERR_WORKER_OUT_OF_MEMORY.)

## facts-12

- 観点: facts(事実の正しさ)
- 重さ: low
- 節: 5.2 D577
- 反証: 2 人のうち 0 人

### 問題

V224 is closed by adding a sentence only to IRSpec section 1. But LangSpec §6.6 lists exactly which inputs the REPL does not accept (lexical/syntax errors, failed type check, runtime error), and an input rejected for a KIR implementation limit is not one of them. keleut repl's behaviour would still not match the language spec.

### 根拠

doc/LangSpec.md:737-738 (§6.6 の受理しない入力). The V224 probe: keleut repl prints 「<stdin>: 処理系の限度: KIR の書き出しで入れ子が深すぎます」 and does not accept the input.

### 直し方

In M107, also add to the §6.6 bullet 「型検査か実行の前の変換に要する領域が処理系の限度を超えた入力」, matching the widened §15.5 row (D566), or explicitly name that §15.5 row as covering it.

### 反証者 1: 反証できなかった

I confirmed the problem. LangSpec §6.6 (doc/LangSpec.md:737) says the REPL does not accept three kinds of input: inputs with lexical or syntax errors, inputs that fail type checking, and inputs that stop with a runtime error. It says nothing about inputs rejected because they exceed an implementation limit. §15.5 (1783-1784) does not fill that gap. Its row 1 lists "型検査に通らない" and "型検査に要する領域が処理系の限度を超えた" as separate conditions, so failing a type check does not cover exceeding a limit. D577 (plan 5.2, lines 579-589) adds a sentence only to IRSpec section 1 and to AGENTS.md. D578 edits §6.6 too, but only to add the sentence about the display format. D566 widens only the §15.5 table row. As a result, the language spec would still not say that the REPL may reject an input at the KIR-writing nesting limit, and keleut repl's behaviour would still sit outside the §6.6 list. The same gap already exists for the parse and type-check limits (rows 2 and 1), which both REPLs enforce and which V237 is being closed under. That makes the suggested fix more useful, not less. M107 already edits §6.6, so the suggestion is cheap and fits the plan: add to the §6.6 bullet inputs whose reading, type checking or pre-run conversion exceeds an implementation limit, or point that bullet to the §15.5 rows.

### 反証者 2: 反証できなかった

I confirmed the finding. LangSpec §6.6 (doc/LangSpec.md:737-738) names three kinds of input the REPL does not accept: inputs with lexical or syntax errors, inputs that fail the type check, and inputs that stop with a runtime error. Implementation limits are not on that list. LangSpec has no general implementation-limit clause either. The only limit wording is in §15.5 (lines 1772, 1783-1784, 1792), and its table is about how a file run ends and which exit status it gives, not about accepting REPL inputs.

The plan's D577 (plan lines 579-591) adds a sentence only to IRSpec section 1 and a sentence to AGENTS.md. Its §6.6 change, D578 (lines 593-599 and 654), only adds the sentence about the display format. D566 widens §15.5 row 1 to "実行の前の変換", but neither D566 nor D577 links that row to §6.6. The M107 row (line 697) and the commit split (line 723) also show no §6.6 change for V224.

So the language spec still has no text that allows keleut repl to reject a deep input for the KIR limit. Someone could argue the case is covered indirectly: §6.6 treats each input as appended to the earlier ones, and the widened §15.5 row would reject that program. But the plan never says this, and IRSpec cannot set the acceptance rules of LangSpec §6.6. Diktor's parse-depth limit in the REPL is also outside the §6.6 list, so the gap is older than V224. That makes it more worth closing in M107, which already edits §6.6. The suggested fix is cheap and consistent with D566: either add the clause to the §6.6 bullet or point that bullet at the §15.5 row. Severity is low.

## feasible-0

- 観点: feasible(実装の見込み)
- 重さ: high
- 節: 5.1 D575 (V238), M112
- 反証: 2 人のうち 0 人

### 問題

D575 cannot be implemented in the host. Node reopens a closed fd 1 as /dev/null before any JS runs, so `node DIR/main.mjs >&-` looks exactly like `node DIR/main.mjs >/dev/null`. If the host returned 74 on that check, it would also break legitimate runs redirected to /dev/null.

### 根拠

Probe (Node 26.9.0): `node -e "...fstatSync(1)...readlinkSync('/proc/self/fd/1')" >&-` prints {rdev:259, ino:4, link:'/dev/null'}, and the same command with `>/dev/null` prints the same values. Both exit 0. probe/records/wasm_closed_stdout.out records that `node main.mjs >&-` exits 0, while `keleut run --backend wasm >&-` exits 74 only because the Rust launcher checks the fd before it spawns Node.

### 直し方

Drop the host-side check. Close V238 as a limitation of running `node DIR/main.mjs` directly, documented in AGENTS.md next to the `keleut build` item. The other option is to have main.mjs be launched through a small shell or Rust wrapper that checks fd 1 before exec'ing node. Remove the `node DIR/main.mjs >&-` → 74 test from M112.

### 反証者 1: 反証できなかった

I confirmed the finding, with one detail to correct.

Probe (Node v26.9.0): a script given `>&-` and the same script given `>/dev/null` both see fd 1 as /dev/null, with the same {rdev:259, ino:4} and readlink '/dev/null'. Both exit 0. So by the time host JS runs, a closed fd 1 has already been reopened, and an fstat-style "is fd 1 closed" check cannot work.

Plan 5.1 (line 554) only says "標準出力の記述子が閉じていれば ... 74 で終える". It names no detection method and does not mention that Node reopens the fd. M112 budgets "host 10 行" and a `node DIR/main.mjs >&-` → 74 test (line 702 and the V238 commit at line 729). The plan does not handle the problem.

The correction: the two cases are not exactly identical. /proc/self/fdinfo/1 shows flags 02100002 (O_RDWR, the fd Node reopened) after `>&-` and 02100001 (O_WRONLY) after `>/dev/null`. A heuristic could tell them apart that way. It would be unreliable:
- It only works on Linux.
- `1<>/dev/null` would be read as a closed stdout.
- Daemonized parents commonly attach stdio to /dev/null opened O_RDWR, so legitimate runs would exit 74.

The finding's conclusion therefore stands. The host cannot reliably detect a closed stdout, and D575 and its M112 test need to change: either document the limitation, or launch main.mjs through a wrapper that checks fd 1 before running node.

### 反証者 2: 反証できなかった

I confirmed the finding. With Node 26.9.0, a probe run under `>&-` and the same probe run under `>/dev/null` print identical fd 1 metadata: {rdev:259, ino:4, mode:8630, link:/dev/null}, and both exit 0. Node reopens a closed fd 1 as /dev/null before any JS runs, so main.mjs and keleut-host.mjs cannot tell the two cases apart. A host check that exits 74 would also break legitimate runs redirected to /dev/null.

The repo already records this limit. doc/log/261024-3-record.md:3804-3807 (item 12) says Node reopens fd 1 as /dev/null, that main.mjs cannot distinguish this from a correct `>/dev/null` run, and that `keleut run --backend wasm` gets 74 only because the Rust launcher checks fd 1 before spawning Node and passes KELEUT_STDOUT_CLOSED. Line 3846 also notes that no launcher script was made for this case.

Plan 5.1 (doc/log/261104-3-plan.md:554-555) still says D575 makes the host exit 74 on both launch paths, including `node DIR/main.mjs`, and adds a `node DIR/main.mjs >&-` → 74 test to tests/wasm_cmd.rs. That test cannot pass with a host-side check. The plan does not mention the reopen problem or a wrapper, so it does not handle this. The suggested fix is sound: drop the host check and record V238 as a limitation of running main.mjs directly, or add a sh/Rust wrapper that checks fd 1 before exec'ing node.

## feasible-1

- 観点: feasible(実装の見込み)
- 重さ: high
- 節: 4.2 D568 test-only failure, 4.3 M109 tests, 4.5 M111 tests
- 反証: 2 人のうち 0 人

### 問題

KELEUT_TEST_ALLOC_FAIL fails one allocation once and then takes the reserve path, where the retry succeeds. So the injected failure never reaches the fallible code these tests are meant to cover: the `Result`/try_reserve path of the shape table (D569), the value stack's own spare chunk (`armed`/`spare` in stack.rs:120-140), and the `cont_make` failure path (V239). Separately, the M111 test 'fail the chunk allocation at injection time' targets an allocation that D571 removes: after the change, injection reuses the continuation copy and allocates nothing.

### 根拠

Plan 4.2: 'SKIP + 1 回目を 1 回だけ失敗させる。失敗は OS の失敗と同じく予備の道を通る'. Plan 4.2 order of reserve use: global reserve first, then the value-stack spare. Plan 4.5: '注入では…新しい確保は要らない', yet the M111 test says '注入のときの塊の確保を KELEUT_TEST_ALLOC_FAIL で失敗させて'. stack.rs take_mem/new_mem is the fallible path that would be bypassed.

### 直し方

Add a 'hard' mode (for example ROLE:MIN:SKIP:COUNT, or a flag that also fails the retry, or that bypasses the reserve) so the fallible callers really see null. Use it in the shape-table, value-stack spare and V239 tests. In M111, inject the failure into the `cont_resume` placement chunk, which is what triggers injection, and then assert the order inner/own/outer cancel.

### 反証者 1: 反証できなかった

I confirmed the finding against the plan text and the code. Plan 4.2 (lines 391-394) defines KELEUT_TEST_ALLOC_FAIL=ROLE:MIN:SKIP. It fails the SKIP+1-th allocation once, and that failure "goes through the reserve path like an OS failure". By 4.2 item 1, that path gives back the 8 MiB reserve, raises the role flag and retries, and the retry succeeds. So the caller never gets null, and the fallible code the tests are meant to cover never runs:
(a) the D569 try_reserve_exact/Result path of the shape table (4.3);
(b) the value stack's own spare chunk: take_mem/new_mem with spare/armed at crates/keleut-rt/src/stack.rs:120-140, which by the plan's stated order is used only after the global retry also fails;
(c) cont_make's failure path, try_reserve_exact followed by fail_into_segment at crates/keleut-interp/src/interp.rs:1525-1527, which the V239 test (plan line 492) claims to reach by failing the continuation-copy allocation.
The M109 test "the extension inside the cancel clause fails again and writes the abort record" (line 438) needs a second injected failure, which a once-only switch cannot produce.
Separately, D571 (lines 478-484) says injection reuses the continuation copy as a chunk and "needs no new allocation". Yet the M111 test at line 500 fails "the chunk allocation at injection time" with KELEUT_TEST_ALLOC_FAIL. That is a direct contradiction in the plan.
No section of the plan adds a mode that also fails the retry or bypasses the reserve. The suggested hard mode, and moving the M111 injection target, are sound changes.

### 反証者 2: 反証できなかった

I confirmed the finding by checking the plan and the code.

1. In plan 4.2 (lines 365-368 and 391-393), a failed allocation releases the global reserve, sets the role flag and is retried. `KELEUT_TEST_ALLOC_FAIL=ROLE:MIN:SKIP` fails only the (SKIP+1)-th allocation, and only once. That injected failure also takes the reserve path. So the retry always succeeds, and the caller never sees null or Err. Only the role flag gets tested.
2. In the code, `cont_make` (`crates/keleut-interp/src/interp.rs` ~1520) reaches `fail_into_segment` only when `frames.try_reserve_exact` / `sizes.try_reserve_exact` return Err. Plan 4.2 says the role flag is checked only at the Rt heap-allocation entry and on the value-stack chunk path, not in `cont_make`. So the V239 test (plan line 492) cannot reach the JIT path it targets.
3. The plan's own reserve order (line 387) is: global reserve first, then the value-stack spare. So `take_mem`'s Err branch, which arms `spare` (`stack.rs:126-139`), is never reached by a one-shot injection.
4. D569's `try_reserve_exact`/`Result` path in the shape table is bypassed the same way. The M109 test 'the extension inside the cancel clause fails again and the abort record is written' (line 438) needs a second failure after the reserve is used up, and a one-shot injection cannot produce one. A real second failure would abort with `handle_alloc_error` (line 368).
5. D571 (lines 484-485) says injection needs no new allocation. Yet the M111 test (line 500) fails 'the chunk allocation at injection time', which no longer exists after the change.

No other part of the plan adds a mode that bypasses the reserve or fails more than once. The suggested hard or count mode, and moving the M111 injection target, are reasonable fixes.

## feasible-2

- 観点: feasible(実装の見込み)
- 重さ: medium
- 節: 4.2 D568 item 1 (reserve release)
- 反証: 2 人のうち 0 人

### 問題

The wrapper cannot tell a fallible request (try_reserve, or `alloc_zeroed` checked for null) from an infallible one. It will release the 8 MiB reserve and set the role flag for requests the reserve cannot satisfy or that already degrade gracefully. Examples: the 150 MB shape-table doubling, where old and new tables coexist during the copy; the GC sweep's `keep.try_reserve` fallback (heap.rs:392-399); the mark-stack overflow fallback in gc.rs. The reserve is then gone, and the flag forces an OOM even where the collector would have recovered.

### 根拠

keleut-rtcore/src/lib.rs:193-224: grow() allocates the doubled table before freeing the old one, and results.md 2 shows the failing size of 150,994,968 bytes. heap.rs:392 and gc.rs mark push already use try_reserve with in-place fallbacks.

### 直し方

Release the reserve only when layout.size() is at most a fraction of the reserve (for example ≤ 1 MiB). Alternatively, give fallible call sites a thread-local 'fallible scope' bit so the wrapper returns null straight away. State in D568 that the wrapper returns null and never calls handle_alloc_error itself.

### 反証者 1: 反証できなかった

I confirmed the problem. D568 item 1 (doc/log/261104-3-plan.md:363-368) releases the 8 MiB reserve and sets the role flag on any failed request. A GlobalAlloc wrapper sees `try_reserve` failures as ordinary null returns, so it cannot tell them apart from other failures, and the plan does not handle this case.

1. **Fallible paths that already recover.** keleut-rt has several `try_reserve` fallbacks:
   - the sweep's `keep` in heap.rs:392-399, which falls back to sweeping pages in place;
   - the mark stack at gc.rs:314;
   - the pool at heap.rs:317 and 410;
   - gc.rs:654.

   Under D568, a failure at any of these would spend the reserve and set the execution-role flag. Item 2 then makes the next heap allocation return `Stop::OutOfMemory` with exit 3. Today such a run continues.

2. **Large requests the reserve cannot satisfy.** D569 (plan 4.3) turns `ShapeTable::alloc` into `Vec::try_reserve_exact`. `grow` (keleut-rtcore/src/lib.rs:208-225) allocates the doubled table while the old one is still live, and the failing size in that path is about 150 MB. Releasing 8 MiB cannot satisfy that request. The reserve is used up anyway and is not taken back during `keleut run` (only the REPL takes it back, item 2). So the `cancel` clauses that run afterwards have no global reserve for their small allocations.

3. **Contradictory wording.** Item 1 says that when the retry fails, it 'stops with `handle_alloc_error` as now'. Read as the wrapper's own behaviour, this would abort every `try_reserve` caller, including the new D569 path and the existing heap.rs and gc.rs fallbacks. The plan should say that the wrapper returns null and that only infallible callers end in `handle_alloc_error`.

One detail in the finding's evidence is imprecise: the current `grow` uses an infallible `vec!`, so this case only arises once D569 is applied. The concern holds for the plan as written, because D569 does exactly that. The suggested fix is sound: release the reserve only for requests below a size threshold, or use a thread-local 'fallible scope' bit.

### 反証者 2: 反証できなかった

The core problem holds. `GlobalAlloc::alloc` cannot tell a `try_reserve` request from an infallible one, and D568 item 1 (plan lines 367-368) does not deal with fallible requests. Two things follow.

(a) Plan line 368 says that if the retry fails or there is no reserve, the process stops with `handle_alloc_error` "as now". That reads as the wrapper aborting by itself. If it does, the fallible paths the plan relies on stop working. These are the D569 change of `ShapeTable::alloc` to `try_reserve_exact` (line 415), the `rec_fields` `try_reserve` (line 420), and the GC work areas taken by `try_reserve` before collection in D568 item 3 (line 383). The plan never says the wrapper returns null and leaves `handle_alloc_error` to infallible callers.

(b) Existing graceful fallbacks also pass through the wrapper and would now raise the execution-role flag, which forces `Stop::OutOfMemory` at the next heap entry. These are `heap.rs:393` (`keep.try_reserve`, falling back to an in-place sweep), `gc.rs:314` (mark-stack `try_reserve`, falling back to the overflow flag), `gc.rs:654` and `heap.rs:317/410` (the pool `try_reserve`). The plan does not decide whether this change in behaviour is intended.

Parts of the finding are overstated or wrong:
- "The reserve is then gone" is weaker than claimed. Releasing the 8 MiB returns address space that the cancel clauses can still use, so a failed 150 MB shape-table doubling (`keleut-rtcore` `lib.rs:208-224`, old and new tables live together) loses little in practice.
- The suggested threshold of at most 1 MiB conflicts with the plan's own design. Plan lines 388 and 394 deliberately send value-stack chunks of 1 MiB or more and the shape-table growth through the reserve path.

The alternatives are sound: a thread-local "fallible scope" bit, plus an explicit statement that the wrapper returns null. The finding is therefore confirmed as a real gap in D568's specification, but the threshold fix it proposes should not be adopted.

## feasible-3

- 観点: feasible(実装の見込み)
- 重さ: medium
- 節: 4.2 D568 item 2 (flag checks)
- 反証: 2 人のうち 0 人

### 問題

The plan never says when the execution-role flag is cleared. If it stays set after `Stop::OutOfMemory` is raised, the first heap allocation in each cancel clause sees it again. The clauses would then be cut off as cancel escapes, not run as §15.5 requires. Only the REPL's re-arming of the reserve is specified.

### 根拠

Plan 4.2: '立っていれば Stop::OutOfMemory を返し、いまのメモリの不足と同じく cancel 節を走らせて 3 で終わる'. The existing heap path in gc.rs Rt::alloc → heap.exhausted has its own 1 MiB page reserve that is released exactly once.

### 直し方

Make the check consume the flag with an atomic swap(false) when it raises the OOM, the same one-shot behaviour as the heap's page reserve. Add an oom_cmd.rs case whose cancel clause allocates (for example builds a list) after the injected failure.

### 反証者 1: 反証できなかった

I confirmed the gap. Plan 4.2 D568 (lines 363-382) says the execution-role flag is raised when the global reserve is released. It is checked at the Rt heap allocation entry and on the value-stack chunk path, and on a hit Stop::OutOfMemory is returned "いまのメモリの不足と同じく cancel 節を走らせて 3 で終わる". No line in the plan says when the flag is cleared or consumed; a grep of every '印' occurrence (lines 64, 363-376, 393, 455, 621, 661, 698, 705, 725, 747, 769, 827, 846) shows none. The REPL item only re-takes the reserve, and the risk row at 769 only covers a second failure before the flag is checked. By contrast, the existing heap path (heap.rs:313-319, Heap::exhausted) moves its page reserve into the pool once and sets in_reserve. That is what lets cancel clauses allocate after an OutOfMemory. If an implementer leaves the new flag sticky, every allocation inside a cancel clause would raise OutOfMemory again, against §15.5 and against the plan's own wording. Making the check consume the flag once (swap(false)) and adding an oom_cmd.rs case whose cancel clause allocates is a correct, low-cost addition. The checklist at line 827 lists 'cancel 節の中の失敗' as a review aspect, but the design does not settle it.

### 反証者 2: 反証できなかった

I confirmed the gap. Section 4.2 of the plan says three things about the execution-role flag. It is set when the reserve is released (item 1). It is checked at the Rt heap-allocation entry, including the fast path, and on the value-stack chunk path (item 2). When it is set, the check returns Stop::OutOfMemory and the cancel clauses run. I grepped the whole plan for 印 and for any wording that lowers, clears or consumes it, and found nothing. The only rearming step described is the keleut repl re-taking the reserve before the next input. Nothing says the flag is consumed when it raises OOM. If an implementer reads 4.2 literally, the flag stays set. The first heap allocation in a cancel clause would then raise OutOfMemory again, and the clause would be cut short instead of running as §15.5 requires. The risk table (line 769) and the D568 summary (line 747) do not cover this either. The existing heap path works differently: Heap::exhausted in crates/keleut-rt/src/heap.rs:313 moves its page reserve into the pool once, and the next exhaustion returns OutOfMemory directly. The new flag needs a similar one-shot rule, for example a swap(false), and the plan does not state it. The planned oom_cmd.rs cases (line 403: in-collection, rec_fields, call_clause, chunk; line 500: continj) do not explicitly include a cancel clause that allocates on the heap after the injected failure. So the suggested test would add coverage the plan lacks. The suggestion is sound.

## feasible-4

- 観点: feasible(実装の見込み)
- 重さ: medium
- 節: 4.4 D570 item 2/4, goal 3, V236
- 反証: 2 人のうち 0 人

### 問題

An 8 MiB reserve does not cover the front end: a large check uses hundreds of MB (top 3,000 uses 443 MB in emit-kir). After the reserve is returned, the next front-end failure still aborts with 134. The deterministic KELEUT_TEST_ALLOC_FAIL=front test will pass, but real `ulimit -v` front-end exhaustion, the reported V236 case, will mostly stay 134. Closing V236 in M110 overstates the fix.

### 根拠

results.md 2: the func 3,000 `.kel` at 524,288 KiB aborts in Tc::extend (22,120 bytes), and results.md 1 shows the front end needs ~443 MB for top 3,000. Plan 4.4 item 4 itself says that after the reserve is used up the result stays 134.

### 直し方

For roles that run before execution (front end, KIR read/verify/lowering), have the wrapper write the '処理系の限度' line and `_exit(1)` directly when the retry fails. These roles have no cancel clauses and no buffered program output to preserve. Then add a `ulimit -v` test of the func 3,000 `.kel` that expects 1.

### 反証者 1: 反証できなかった

I confirmed the problem. In plan section 4.2 (D568), the 8 MiB reserve is returned and the allocation is retried. If the retry fails, or there is no reserve left, the process aborts with 134. The front-end flag is checked only after type checking and KIR writing (4.2 item 2, 4.4 item 2), and 4.4 item 4 says a failure after the reserve is used up stays 134. Nothing in LangSpec covers this. D567 item 2 is an IRSpec exclusion, and it only covers ki/kj. The ledger still closes V236 in M110 (plan line 797). Its only front-end test is the deterministic KELEUT_TEST_ALLOC_FAIL=front one; the ulimit -v tests expect 0 and never exhaust the front end.

I measured this on the HEAD build with a func 3,000 .kel (gen_chain.py func 3000) and `keleut emit-kir -o /dev/null`, under ulimit -v from 300,000 to 1,500,000 KiB. Every limit exited 134 inside the front end. The failed allocation sizes ranged from 1 byte to 201,326,592 bytes (50 MB at 450,000; 100 MB at 500,000; 22,024 bytes at 524,288; 201 MB at 700,000). So the front end's failure point moves across a range of hundreds of MB. With only 8 MiB of headroom after the reserve is returned, the next allocation fails again. Some failures are Vec doublings of 50 to 200 MB, which an 8 MiB reserve cannot satisfy even on the first retry. Real ulimit -v front-end exhaustion would therefore still mostly end in 134, and closing V236 in M110 overstates the fix.

The suggestion is sound. Roles that run before execution (front end, KIR read, verify and lowering) have no cancel clauses and no program output to preserve. They could write the implementation-limit line and _exit(1) when the retry fails. A ulimit -v test of the func 3,000 .kel expecting 1 would then catch the real case. My probe files are in /tmp/claude-1000/-home-sumito3478-repo-keleut/f07707f3-2b31-49a0-9315-41cda0f0bf0b/scratchpad/work/ref1-feas4/.

### 反証者 2: 反証できなかった

I confirmed the finding against the plan and the spike material. In plan 4.2 (D568), the global allocator returns the 8 MiB reserve on the first failure, sets the role flag and retries once. Any later failure, or one with no reserve left, still goes to handle_alloc_error (134). Plan 4.2 item 2 and 4.4 item 2 check the front-end flag only after type checking or the KIR write. That leaves no point inside the type checker where the run stops early, so the front end keeps allocating after the reserve is gone.

The measurements show the front end needs far more than 8 MiB:
- probe/alloc/results.md section 1: emit-kir on top 3,000 uses 443 MB, and keleut check on top 3,000 uses 498 MB.
- results.md section 2: the func 3,000 .kel under `ulimit -v 524288` aborts with 134 on a 22,120-byte Vec growth in Tc::extend, called from inside map_generics_with. That is mid-check, with a large amount of work still to do.

So in the real `ulimit -v` case that V236 reports, the retry gets past one failure and then almost certainly fails again with 134. The plan says this itself (4.4 item 4: after the reserve is used up, a failure in the front end stays 134; risk table line 769). Even so, 4.4 and the ledger (line 797) say V236 is closed in M110.

The M110 tests do not catch this:
- KELEUT_TEST_ALLOC_FAIL=front fails only one allocation (4.2: "1 回だけ"), so the retry always succeeds.
- The two ulimit tests expect exit 0.
- No test runs the front end out of memory under a real `ulimit -v`.

D567 item 2 lists only the remaining windows in ki/kj, not the front end, so the plan does not document the gap either.

The suggestion fits this problem. Before execution, the front end and KIR read/verify/lowering have no cancel clauses to run and no program output in a buffer. Writing the 処理系の限度 line with a raw write(2) and calling `_exit(1)` from the allocator when the retry fails would fix this in practice. The suggested func 3,000 .kel test under `ulimit -v`, expecting exit 1, would check it.

One caveat: in keleut repl the front-end role should not end the session with `_exit`. It needs separate handling or an explicit exclusion. This changes the details, not the conclusion.

## feasible-5

- 観点: feasible(実装の見込み)
- 重さ: medium
- 節: 4.4 D570 item 3 (threads under RLIMIT_AS)
- 反証: 2 人のうち 0 人

### 問題

D570 shrinks and retries only the KIR thread and the execution thread. Two more threads are created per run and fail with 70 under a finite RLIMIT_AS. (a) The structural-Eq stack-hop threads are spawned at the full `native_stack` size, nest, and return Stop::Internal (70) when the spawn fails. They also need the execution role in D568. (b) The LLVM compile thread fails with 70 if it cannot be spawned. Goal 3 ('type-checked programs never end with 70 under a VM limit') stays false for deep structural Eq and for JIT runs.

### 根拠

keleut-interp/src/interp.rs:345-372: the hop spawns `stack_size(size)` and on Err returns `Stop::Internal("新しいスタックのスレッドを作れません…")`. keleut-interp/src/lib.rs:85 sets the hop size to opts.native_stack. keleut-llvm/src/run.rs:272-283: the JIT thread spawn failure prints an internal error and returns 70.

### 直し方

Add both threads to D570 and M110. Size the hop thread like the shrunken execution thread and turn its spawn failure into OOM (3). If the JIT thread cannot be spawned, fall back to the interpreter, as D568 item 4 already does for translation. Add a test of deep structural Eq (about 200k levels) under `ulimit -v` with the shrunken stack.

### 反証者 1: 反証できなかった

I confirmed the finding against the code and the plan. Plan section 4.4, D570 item 3 shrinks and retries only two threads: the KIR thread (kir/src/lib.rs:117) and the execution thread (keleut-interp/src/lib.rs:193, repl_cmd.rs:487). The plan never mentions the structural-Eq stack-hop thread or the keleut-jit thread.

Hop thread (the stronger part): in keleut-interp/src/lib.rs:85-90 the hop size is opts.native_stack, never less than HOP_STACK_MIN (8 MiB). The default native_stack is native_stack_for(DEFAULT_MAX_DEPTH) = (L+R)*1024 + 64 MiB, which is on the order of a GiB. interp.rs:345-372 spawns the hop at that size, and on a spawn error it returns Stop::Internal (70). Record 261024-3 lines 3169-3180 says the hop is taken for deep structural Eq (200k levels) and has a stack as large as the native stack. After D570 the execution thread will have fitted under ulimit -v by shrinking. A deep Eq would then still try to reserve a full-size hop stack, and it would end with 70. That breaks D570's stated aim that type-checked programs end with a §15.5 status.

JIT thread: keleut-llvm/src/run.rs:272-283 returns 70 when the 16 MiB (JIT_STACK) keleut-jit thread cannot be spawned. That is less likely to happen, but the plan does not handle it. The plan's own M110 tests include --tier jit-only under ulimit -v.

The suggested additions are reasonable and do not conflict with the plan: size the hop like the shrunken execution thread and map its failure to 3, fall back to the interpreter when the JIT thread cannot be spawned, and add a deep-Eq test under ulimit -v. I did not run a probe; the finding rests on reading the code.

### 反証者 2: 反証できなかった

I confirmed the finding. Part (a) is the main problem and part (b) is a minor one.

The plan never mentions the hop threads. D570 item 3 in plan section 4.4 names only the KIR thread (kir/src/lib.rs:117) and the execution threads (keleut-interp/src/lib.rs:193, repl_cmd.rs:487), and a grep of the plan for hop, 移って and stack/eq finds nothing.

Code checks:
- keleut-interp/src/lib.rs:85-89 sets the hop size to max(opts.native_stack, HOP_STACK_MIN).
- interp.rs:345-372 spawns each hop thread with stack_size(size). If the spawn fails, it returns Stop::Internal("新しいスタックのスレッドを作れません…"), which ends in 70.
- The default native_stack is native_stack_for(1<<20), which works out to (1M+16k)*1024+64 MiB, about 1.1 GB.

Under D570, `ulimit -v 1000000` shrinks the execution thread to a quarter of the limit, about 244 MB. A 200k-level structural Eq needs about 300 MB at 1.5 KiB per level, as recorded in 261024-3 and tested by conformance/stack/eq.t. So the comparison must hop, and the hop asks for the full 1.1 GB stack, which cannot fit.

【実測】I reproduced the failure today with the eq.t program at 200k levels, emitted to .kir. Under `ulimit -v 700000` with `--native-stack 200m --backend interp`, the run prints `keleut: 内部エラー: 新しいスタックのスレッドを作れません(209715200 バイトのスタック): Resource temporarily unavailable (os error 11)`. With 2 GB of virtual memory the same run succeeds. Hops also nest, and every waiting parent thread keeps its stack, so the shrink-and-retry rule must cover this spawn as well.

Part (b): the JIT thread's spawn failure returns 70 (run.rs:280-283). It is real but much less likely, because JIT_STACK is only 16 MiB. It is still a 70 that D570 leaves open, and the M110 test runs `--tier jit-only` under `ulimit -v`.

The suggestion is sound: size the hop thread from the shrunken stack, turn a failed hop spawn into OOM (3), fall back to the interpreter when the JIT thread cannot be created, and add a deep-Eq test under `ulimit -v`.

## feasible-6

- 観点: feasible(実装の見込み)
- 重さ: medium
- 節: 4.4 D570 item 3 / 4.1 D566
- 反証: 2 人のうち 0 人

### 問題

Under RLIMIT_AS the KIR thread lowers its nesting limit to what fits in the reduced stack. The front-end thread is also capped (at a quarter), but it computes its own limit. If the KIR limit ends up below the front end's, a `.kel` program that type-checked is rejected while reading its own KIR. Today that path exits 65 ('KIR の読み込みで入れ子が深すぎます'), and 65 is not in the §15.5 table for source programs. This is exactly the class of failure D566 assigns to exit 1.

### 根拠

results.md 2: `--native-stack 16m` gives 65 'KIR の読み込みで入れ子が深すぎます' for a `.kel`-derived `.kir`. AGENTS.md: the KIR nesting limit 'drops to what fits, so a deep .kir exits 65'. Plan 4.4 item 3: 'KIR の処理のスレッドは、いまと同じく収まる深さまで入れ子の限度を下げる'.

### 直し方

State in D570 that when the input came from the in-process front end, a KIR-side nesting or stack limit reports the '処理系の限度' line and exits 1, not 65. Alternatively, size the KIR thread so its limit is never below the front end's. Add a `ulimit -v` case near the nesting limit to M110.

### 反証者 1: 反証できなかった

I confirmed the finding, and the gap is wider than it says. I measured it with the HEAD release build of keleut, using `run --backend interp` on `.kel` files. The flow is front end, then in-process KIR, then verify_and_lower with kir_limits.

- **512k stack:** the parse limit (128) is the first to stop deep nesting, with exit 2.
- **4m stack:** `1 + (...)` nested 1534 levels type-checks and is written. KIR verification then rejects it: 'keleut: KIR の検証に失敗しました: 処理系の限度: KIR の検証で入れ子が深すぎます', exit 65. A 1536-statement block (desugared let chain) does the same.
- **16m stack:** the same happens at 6142 and 6144 levels. The front end's parse limit at 16m is 7168. The KIR limit from `Limits::with_stack` is (16M - 4M) / 2048, about 6144, which is lower. So the KIR limit sits below the front end's limit.

Exit 65 is not in the source-program table of LangSpec §15.5, which lists 0/1/2/3. This failure fits the plan's widened row 1 (D566), whose 'その読み込みと検証' wording covers it. But the plan only gives it threads that cannot be created, KIR-writing limits and front-end allocation. D570 item 3 says the KIR thread keeps lowering its nesting limit to what fits. Under the plan the KIR thread gets the same quarter-of-RLIMIT_AS stack as the front end, so `ulimit -v` alone would now cause the same 65. Example: at 128 MiB the KIR limit is about 57344, but the front end accepts up to about 63488 (the AGENTS.md figure for ulimit -v 524288).

None of the plan text I read (§1.4, 4.1, 4.4) handles the 65 for `.kel` input. The suggestion is right: report the '処理系の限度' line and exit 1, or size the KIR limit at or above the front end's, and add a test.

### 反証者 2: 反証できなかった

I confirmed the finding by measurement. One detail is wrong but does not change the conclusion. For a .kel input, `keleut run --frontend rust` never re-reads KIR. In run_cmd.rs `load()`, `program_for_run` returns the Program and calls `verify_and_lower(o, &prog, &kir_limits(o))`. So the failure comes from KIR verification, not from reading. `kir_limits` with `--native-stack S` is `Limits::with_stack(N_NEST, front_stack_for_run(S))`, which is the same stack as the front end. Even so, the KIR verifier's depth limit with that stack is lower than the front end's parse limit. Probe (scratchpad work/ref2-f6): `let x = 1 + (1 + (... 7000 levels ...))` and a 7000-statement block both run under `--native-stack 16m --backend interp`. Each passes the front end, whose parse limit there is 7168, then exits 65 with 'keleut: KIR の検証に失敗しました: 処理系の限度: KIR の検証で入れ子が深すぎます'. So a program that type-checked already ends with 65, which is not in the §15.5 table, whenever `--native-stack` is given. Today, under `ulimit -v` without `--native-stack`, the KIR thread uses the default stack of about 211 MiB (V235, exit 70). D570 item 3 caps that thread at a quarter of the limit, like the front end, and keeps 'lowering the nesting limit to what fits'. That brings the same 65 path to `ulimit -v`. D566 lists only three cases for exit 1: the write limit, front-end allocation failure and failure to create the KIR thread. Neither D566 nor D570 nor the M110 tests mention the KIR verification or reading nesting limit for front-end inputs (grep for 検証で入れ子 / 読み込みで入れ子 finds nothing). Under the D566 wording this is plainly 'space needed for translation before running exceeds the implementation limit', and it should be 1. Corrections: the message is the verify message, not the read message. The case already occurs with `--native-stack`, independent of RLIMIT_AS.

## feasible-7

- 観点: feasible(実装の見込み)
- 重さ: medium
- 節: 5.3 D579 item 3 (M115 JIT allocation fast path)
- 反証: 2 人のうち 0 人

### 問題

The fast path described only takes a cell from the free list or the bump position. Collection is triggered by `since_gc >= trigger`, counted on every allocation. A JIT fast path that skips this accounting never triggers collection while free-list cells remain. Heap growth and GC timing would then diverge from `ki`, and kj/ki differences in OOM-boundary tests would follow.

### 根拠

keleut-rt/src/gc.rs:613-631 Rt::alloc checks `self.heap.should_collect()` before try_alloc. heap.rs:255 `should_collect` is `self.since_gc >= self.trigger`.

### 直し方

Specify in D579/M115 that the inline path increments since_gc (or the same counter held in NativeCtx) and branches to the slow path when the trigger is reached, in addition to checking the OOM flag. Add a `KELEUT_GC_STATS` comparison of collection counts between ki and kj to the M115 acceptance.

### 反証者 1: 反証できなかった

I confirmed the finding. In plan 5.3, D579 item 3 (line 618 onward) describes the M115 fast path as taking one cell from the free list or the bump position, and calling the runtime only when neither gives a cell. It also says the fast path skips `--gc-stress`, checks the D568 role flag, and handles at_safepoint. Nothing in the plan covers the normal collection trigger. Rt::alloc (gc.rs ~613) runs `heap.should_collect()` (heap.rs:255, `since_gc >= trigger`) before try_alloc, and try_alloc adds to since_gc (heap.rs:339-380). An inline path that skips this check would not collect while free cells or bump space remain. kj would then grow the heap and time collections differently from ki, which matters for the conformance/gc `ulimit -v` tests and for OOM-boundary results. A grep of the plan finds no mention of since_gc, trigger or 回収の時機. The only related entry is the risk row at line 776, and it covers only the `--gc-stress` count. The M115 acceptance (line 705) has no comparison of collection counts. The suggestion fits: count since_gc, or a counter mirrored in NativeCtx, and take the slow path when the trigger is reached.

### 反証者 2: 反証できなかった

I confirmed the problem. Plan 5.3 D579 item 3 (line 618) says only that the fast path takes one cell from the free list or the bump position, and calls the runtime only when that fails. Its guards are --gc-stress (which bypasses the fast path) and the D568 run-role mark. Line 776 of the risk table covers only --gc-stress counting. Nothing in the plan (I searched for since_gc, 回収の時機, should_collect and GC_STATS) makes the inline path update the collection counter or check the trigger. In the runtime, Rt::alloc (keleut-rt/src/gc.rs, around line 616) checks heap.should_collect() before try_alloc. should_collect is since_gc >= trigger (heap.rs:256). try_alloc increments since_gc on every path (heap.rs:339/346/358/380). An inline fast path that skips both would never start a collection while free cells or bump space remain, and kj's heap growth and GC timing would diverge from ki's. The suggested change is right: count since_gc (or a mirror in NativeCtx) and branch to the slow path at the trigger. A KELEUT_GC_STATS comparison of collection counts belongs in the M115 acceptance.

## feasible-8

- 観点: feasible(実装の見込み)
- 重さ: low
- 節: 3.1 D564
- 反証: 2 人のうち 0 人

### 問題

B_prog grows linearly in k, and k is a lexical nesting depth with no bound. Every program with k ≥ 1 is moved onto the Worker path (k=1 is about 3.0 GB, k=3 about 5.9 GB of stack). D572 rejects V215 option B partly because 'always Worker' changes signal forwarding, RLIMIT_STACK handling and the bytes/sinkfail behaviour. Those same concerns now apply to every k ≥ 1 program, and the plan does not test them on the Worker path.

### 根拠

Plan 3.1: 'k = 1 のとき…約 3,044,608 KiB…Worker の道を選ぶ'. Plan 4.6 reasons against option B: 'シグナルの転送、RLIMIT_STACK を上げる道、起動の時間が変わる…bytes/ と sinkfail の試験…が要る'.

### 直し方

Add an upper bound for k: reject at build time with a limit message, or cap and document it. Have M106 run conformance/bytes/ and the sinkfail cases on a k=1 program through `kw`, or note explicitly that the Worker path is already covered.

### 反証者 1: 反証できなかった

I confirmed the finding. It is low severity.

1. The plan puts no bound on k. Section 3.1 defines k as the maximum lexical nesting with no upper limit. The risks table in section 9 covers only the case where k is computed too small. A k that is too large is not covered.

2. I measured the limit with Node 26.9.0, using a probe in scratchpad/work/ref1/w.mjs. A Worker with resourceLimits.stackSizeMb of 3000, 6000, 9000, 16000 and 32000 MB is created and runs a 1e6-deep recursion. At 64000 MB, new Worker throws EAGAIN. The host (keleut-host.mjs:776-779) turns that throw into exit 70 before the program starts.

3. Each extra level of k adds about 1360 × (L + R) bytes of stack, roughly 1.45 GB at the default L. So with D564, a program with a large enough k (around 40, or fewer with a larger --max-depth) would exit 70 at startup. Today the same program runs fine at shallow depth. The plan neither caps k nor documents this regression.

4. The Worker path's output behaviour is untested. conformance/bytes/bytes.t and sinkfail.t run `keleut run --backend wasm` with the default max-depth, which takes the direct --stack-size path. The only test that reaches the Worker path in wasm_cmd.rs is the `keleut build` + `node main.mjs` test, on small fixtures. Nothing tests bytes or sinkfail behaviour there.

5. The plan's M106 acceptance checks only c92.kel, the per-level byte count, and that `kconform ki,kw` shows no differences on the existing suites. All existing suite programs have k=0, so none of them reaches the Worker path that D564 adds. The plan raises the concerns about signal forwarding and bytes/sinkfail against option B in section 4.6. It does not apply them to the k≥1 programs that D564 moves onto the Worker path.

The suggested fixes are reasonable: a bound on k or a documented cap, and a bytes/sinkfail check on a k=1 program, or an explicit note that the Worker path is already covered.

### 反証者 2: 反証できなかった

I'm upholding this finding, though one part of it is overstated.

**What holds:**
- In plan 3.1, `B_prog = B_wasm + k × (B_wasm − E_rres)`. Nothing in 3.1, 7.2 (M106), 9 (risks) or 8 (D564) puts an upper bound on `k` or says what happens when `k` is large.
- With the default L, each extra level of `k` adds about 1.4 GB of stack.
- I measured how large a Worker stack Node can create on this machine (overcommit_memory = 0, probe in scratchpad `work/ref2-f8/w.mjs`). `stackSizeMb` 3000, 6000, 16000 and 32000 all succeed. 64000 fails with EAGAIN.
- So a program nesting handler expressions inside `cancel` clauses about 40 deep now runs at shallow depth and exits 0. Under D564 it would exit 70 at startup ("実行のスレッドを作れません").
- That is a new failure the plan creates and does not name. The plan also does not say whether `k` gets a cap with a documented message.
- The plan never says how the Worker path is tested for `conformance/bytes/` and `sinkfail.t`. No test runs them on the Worker path: `wasm_cmd.rs` only runs the fixtures through `node main.mjs`, and the signal tests use the default path.

**What is overstated:**
- The Worker path is not new. It already exists (D545). Builds run through `node main.mjs` use it, and so do large `--max-depth` values and a finite `ulimit -v`.
- On that path the Worker runs the same `runProgram`/`writeSync` code as the main-thread path. Ignored signals and signal forwarding are still handled on Node's main thread and in keleut's Rust code (host/keleut-host.mjs:747-789, launch.rs).
- So the D572 concerns (signal forwarding, raising `RLIMIT_STACK`) apply to D564 much more weakly than the finding says. Those concerns are about making every program use the Worker. Under D564, only programs with `k ≥ 1` move there.

**Fix the plan should take:** state a bound or cap for `k`, or the startup behaviour when it is exceeded, with its message. Also add an explicit line, or a `k = 1` run of `bytes/` and `sinkfail` through `kw`, saying the Worker path is covered.

## feasible-9

- 観点: feasible(実装の見込み)
- 重さ: low
- 節: 4.2 test-only failure (SKIP targeting)
- 反証: 2 人のうち 0 人

### 問題

Targeting a site by the count of allocations at or above MIN bytes (rec_fields, call_clause argument lists, inside the collector) depends on how many allocations happen before it. M114 (reusing Vecs in krt_fx) and M115 (JIT inline allocation) change those counts on purpose, so the M108 oom_cmd.rs cases would silently start testing different sites, or none. The counts also differ between `--backend interp` and `--tier jit-only`.

### 根拠

Plan 4.2: 'MIN を小さくして SKIP を選べば回収の中や rec_fields の確保を狙える'. Plan 5.3 M114 removes the call_clause/perform argument Vecs.

### 直し方

Target sites by a label instead: a thread-local 'site' tag set around rec_fields, collect, chunk allocation and cont_make, which the injector matches. Alternatively, have each test assert the site it hit through a debug line on stderr.

### 反証者 1: 反証できなかった

I confirmed the problem in the plan. In section 4.2 (lines 391-394), `KELEUT_TEST_ALLOC_FAIL=ROLE:MIN:SKIP` fails the (SKIP+1)-th allocation of at least MIN bytes, and the plan targets the collector and `rec_fields` by picking MIN and SKIP. That makes the choice of site depend on how many allocations come first. The M108 tests (line 403) aim at four sites: inside the collector, `rec_fields`, the `call_clause` argument lists, and value-stack chunks. Each test checks only `before`, `cancel`, `実行時エラー: メモリ不足です` and exit 3, and that output is the same whichever allocation fails. The plan gives no way to check which site a test actually hit, such as a site tag or a stderr line. So the tests would not notice when they hit a different site. Two later steps change the counts. Section 5.3 item 2 (M114) turns the `call_clause`/`perform` argument Vecs into reused storage, which removes the target of one test. Item 3 (M115) adds a JIT inline fast path for allocation. M115's acceptance condition (line 705) only asks that `oom_cmd.rs` still passes under `--tier jit-only`, and a test that has quietly moved to another site still passes. The same SKIP is used for `--backend interp` and `--tier jit-only` even though their counts can differ. The plan does not deal with this. The suggested fix is sound and cheap: a thread-local site label that the injector matches, or a check on which site was hit. Severity is low, as the finding says.

### 反証者 2: 反証できなかった

I could not refute this finding. Plan 4.2 (lines 391-394) defines `KELEUT_TEST_ALLOC_FAIL=ROLE:MIN:SKIP`, which fails the (SKIP+1)-th allocation of at least MIN bytes. It says a small MIN with a chosen SKIP can target allocations inside the collector or `rec_fields`. Line 403 then has `oom_cmd.rs` fail four sites (the collector, `rec_fields`, the `call_clause` argument list and a value-stack chunk). Every case expects the same output (before, cancel, メモリ不足です, 3) under both `--backend interp` and `--tier jit-only`. Because the output is the same at every site, a shifted count would still pass while hitting a different site.

M114 (5.3, line 614) deliberately turns the `call_clause`/`perform` argument Vecs into a reused area, and M115 adds a JIT inline allocation fast path. Both change how many allocations come before a site. M115's acceptance (line 705) only requires `oom_cmd.rs` to pass under jit-only, which it would do even after a silent retarget. The plan never says how a test confirms which site it hit: there is no site label, no stderr line, and no per-backend count. The risk table (lines 769-776) does not cover this either. Nothing in the plan handles the issue, and the suggested fix (a site tag, or asserting the site that was hit) is reasonable and not wrong.

## feasible-10

- 観点: feasible(実装の見込み)
- 重さ: low
- 節: 4.5 D571 (injection as chunk)
- 反証: 2 人のうち 1 人

### 問題

'新しい確保は要らない' holds only for placing the copy. The continuation copy becomes an exactly sized chunk, so the first cancel-clause frame pushed during the unwind fits only if it is no larger than the frames already popped above the handler. Otherwise `place` needs a new chunk at the moment memory is exhausted, and the cancel clauses then depend on the armed spare chunk.

### 根拠

stack.rs:1-30: frames never cross chunks, and a frame that does not fit goes to the next chunk. Plan 4.5: '長さちょうどに取った継続の写しの Vec<u64> を、1 つの塊として値スタックにつなぐ。新しい確保は要らない'.

### 直し方

In M111, add a test whose innermost cancel clause has a frame larger than the popped frames (many live slots). Run it with chunk allocation hard-failed and check that it uses the armed spare. Alternatively, have cont_make reserve some headroom words in the copy.

### 反証者 1: 反証できなかった

I confirmed the problem in the code, and the plan does not handle it. In rustkeleut/crates/keleut-rt/src/stack.rs, `place` (lines 199-251) puts a frame that does not fit the current chunk into the next chunk. When the next chunk is missing or too small, `place` calls `take_mem`. On a failed allocation, `take_mem` (lines 127-143) arms the spare and returns OutOfMemory. A retry then takes the armed chunk. The plan's D571 (4.5) turns the continuation copy, taken at exactly its length, into one chunk. So after injection the top of the stack sits at the very end of that chunk. A frame pushed by a cancel clause during the unwind fits only into space freed by frames already popped inside the copy. A larger frame needs a new chunk at the moment memory is exhausted, so it depends on the armed spare. The plan's line "新しい確保は要らない" covers placing the copy, not the unwind that follows. D571's rationale in the 裁定 table (line 750) says the same: "注入に新しい確保が要らず". The planned test variant of continj.kel has its inner cancel clause under DEPTH deep frames. Popping those frees plenty of room, so the test would not exercise this case. The plan's note to check how `place` handles chunks smaller than CHUNK_WORDS does not cover this headroom issue either. The suggested extra test, or headroom words in the copy, is reasonable and consistent with the design. Severity is low, as the finding says.

### 反証者 2: 反証した

The finding's literal point is correct. With the copy as an exact-length chunk, a cancel-clause callee frame that is larger than the frames popped above the handler goes to the next chunk through `place` (stack.rs:199-240). But this is not a gap the plan opens or leaves unhandled.

(1) The runtime is designed for exactly this. stack.rs:27-29 says that when a chunk cannot be taken, the 1 MiB spare is armed, and cancel clauses that run while an OutOfMemory unwinds push their frames into it. `take_mem` (stack.rs:127-143) uses the armed spare for any frame of up to 1 MiB.

(2) In the V243 scenario, the injection is triggered by `place` failing inside `cont_resume` (interp.rs:1611). That failure goes through `take_mem`, which arms the spare before the injection starts. So a cancel frame that overflows the copy chunk lands in the armed spare. This is the same path every OOM unwind already uses, so D571 adds no new dependency. If the OOM came from a heap or Vec failure instead, the spare is not armed. The next chunk allocation either succeeds or arms the spare, which is the same as an ordinary OOM unwind today and not specific to the copy-as-chunk design.

(3) The plan already covers this area. Section 4.5 asks M111 to check how `place` handles a chunk smaller than `CHUNK_WORDS`. Its continj test makes chunk allocation fail at injection through `KELEUT_TEST_ALLOC_FAIL` and checks that the cancel clauses run in order.

The alternative fix, reserving headroom in `cont_make`'s copy, would undo the exact-length copy and add memory for every continuation, all for a case the spare already handles. "新しい確保は要らない" is correct for its subject, connecting the copy. At most the wording could be made more precise, which is not a defect.

## scope-0

- 観点: scope(範囲と漏れ)
- 重さ: medium
- 節: 冒頭「決定」, 2.2, 10 台帳
- 反証: 2 人のうち 0 人

### 問題

Plan 261104-1 section 2.2 put three items out of scope, but the new plan handles only two of them: specifying the REPL display format, and the display-order registration that comes from 2.3. The third has no treatment anywhere: in keleut, the <値> in REPL runtime-error lines and cancel-clause truncation records is spelled differently depending on earlier inputs, including rejected ones. The header (line 4) says the plan covers the items that 261104-1's 2.2 put out of scope, so this is a silent omission.

### 根拠

doc/log/261104-1-plan.md:227 ('REPL の実行時エラーの行と打ち切りの記録の <値> を、前の入力に依らない順で書くこと' listed as out of scope) and :139-141 (measured: the spelling changes with label numbers assigned by earlier inputs, including rejected ones). Grepping 261104-3-plan.md for 前の入力, 履歴 and <値> finds no treatment. Only the appendix-B order relaxation (D555) touches it, and history dependence is not discussed there. The record's 残した項目 also listed V221 and V228 as kept differences. The new plan never says that 261104-2 closed them (261104-2-record.md:405-406).

### 直し方

Add the item to section 1.4 and 2.1 or 2.2 with a ruling. For example, close it as conforming under D555/appendix B (the reader decides field order; the line is a §15.5 diagnostic, not a §18.5 display), or give it a V number and a return condition. Add one line in 1.4 noting that V221 and V228 from the record's 残した項目 were already closed by 261104-2 (M102), so every item in that list is accounted for.

### 反証者 1: 反証できなかった

I confirmed the finding. Plan 261104-1 section 2.2 (doc/log/261104-1-plan.md:220-229) put three items out of scope:
1. The rest of 261024-1's leftovers. The new plan covers these.
2. Specifying the REPL display format. The new plan rules on this in D578.
3. Writing the <値> of REPL runtime-error lines and cancel-clause truncation records in an order that does not depend on earlier inputs (lines 227-228). Lines 139-141 of 261104-1 record the measurement behind it: the spelling changes with label numbers assigned by earlier inputs, including rejected ones.

Header line 4 of 261104-3-plan.md says the plan covers the items that 261104-1's 2.2 put out of scope. Even so, the third item appears nowhere in the plan:
- Grepping for 前の入力, 受理しなかった, Rt::show, ラベルの番号, §15.5 の診断 and <値> finds no treatment of it.
- It is absent from 1.4 (lines 165-181), the 2.1 table, 2.2 (lines 232-243), 2.3 and D578 (lines 591-606).
- D578 covers only the display format, V241 and the display-order registration rule (V221/D558).

Closing it as conforming under D555/appendix B is defensible, but the plan has to say so. It is a silent omission, not something the plan already handles.

The second point also holds. V221 and V228 appear in the plan only as the registration ledger for display-order diffs (lines 180 and 606). The plan nowhere notes that the 261024-3 record's 残した項目 V221 and V228 were already closed by 261104-2 (doc/log/261104-2-record.md:405-406, in M102). The suggested fix is sound: add the item to 1.4 and 2.1/2.2 with a ruling, and add the V221/V228 accounting line.

### 反証者 2: 反証できなかった

I confirmed the finding. 261104-1-plan.md:227-228 puts three items out of scope in its 2.2. The third is writing the `<値>` in REPL runtime-error lines and cancel-clause truncation records in an order that does not depend on earlier inputs. Lines 139-141 measured that keleut's spelling changes with label numbers assigned by earlier inputs, including rejected ones. The header of 261104-3-plan.md (line 4) says the plan covers the items 261104-1's 2.2 put out of scope. The new plan handles only the other two: the REPL display format (D578, which adds a §6.6 sentence about how accepted results are displayed) and the display-order registration rule (lines 180 and 606). Grepping the plan for 実行時エラーの行, 打ち切りの記録の, 前の入力, 履歴, Rt::show and 通し番号 finds no treatment of the third item. It is absent from 1.4, 1.6, 2.1, 2.2, 2.3 and D578. D578's new sentence covers the display of accepted results, not §15.5 diagnostic lines. The plan never mentions V221 or V228 except as the registration ledger number at lines 180 and 606, so the side point is also accurate. The finding's suggestion is sound: add the item to 1.4 and 2.1/2.2 with a ruling (for example close it as conforming under D555/appendix B, since it is a §15.5 diagnostic), and add one line noting that M102 closed V221 and V228.

## scope-1

- 観点: scope(範囲と漏れ)
- 重さ: medium
- 節: 冒頭「対象」 / 11 所有者への問い (P257) / M113
- 反証: 2 人のうち 0 人

### 問題

The diktor change is presented both as conditional on owner approval and as the default. The header says changing diktor 'を前提にする' owner approval (P257). Section 11 says that without an answer the plan proceeds as decided, which means diktor gets modified without approval. Both earlier plans explicitly kept diktor out of scope, so the plan cannot treat silence as approval for this item.

### 根拠

261104-3-plan.md:12 ('diktor を変えるのは本計画 5.2 節の 2 件だけで、所有者の承認(P257)を前提にする') vs :810 ('答えが無ければ本計画の決定のとおりに進める'). Goal 6 (:25) and the M113 acceptance assume the fix happens. 261104-1-plan.md:10 and the record (5658: 'diktor の変更は計画の範囲の外') excluded diktor changes.

### 直し方

Make P257 (and P258) an explicit gate: M113's diktor part starts only after a yes. Without a yes, M113 does only the README rule (D578), V218 and V223 stay as Diktor bugs, and goal 6 is marked conditional. State this exception next to the sentence at line 810.

### 反証者 1: 反証できなかった

I confirmed the contradiction in the plan text. Line 12 of /home/sumito3478/repo/keleut/doc/log/261104-3-plan.md says the diktor change 「所有者の承認(P257)を前提にする」, which makes the owner's approval a precondition. But the 判断事項 header (around line 35) and section 11 (line 810, 「答えが無ければ本計画の決定のとおりに進める」) make the default to go ahead without an answer. The P257 row's 本計画の決定 is 「入れる」. M113 (line 703) has no gate: its acceptance criteria require diktor's dune runtest to pass and the V218 entries to be removed, and goal 6 (line 25) assumes the fix happens. D576 (line 577) itself gives the reason for asking: 「計画 261024-1 と 261104-1 は diktor の変更を範囲の外としたので」. That matches 261104-1-plan.md line 10 (「サブモジュール diktor は変えない」). So as written, M113 changes diktor if the owner is silent, while line 12 says it needs approval. The plan does not resolve this. Making P257 an explicit gate is consistent with line 12 and with D576's own reason. It would mean M113 does only the README rule (D578) without a yes, and goal 6 would be marked conditional. The other answers can default as written. The fix is either that gate or rewording line 12. Either way the inconsistency needs a fix.

### 反証者 2: 反証できなかった

I confirmed the contradiction in the plan text, so the finding is not refuted.

- **Header, line 12:** it says the diktor change is made only with the owner's approval: 「diktor を変えるのは本計画 5.2 節の 2 件だけで、所有者の承認(P257)を前提にする」.
- **D576, line 577:** it asks the owner for approval explicitly, because both earlier plans kept diktor changes out of scope.
- **Section 11, line 810:** it says that without an answer the plan goes ahead as decided.
- **Goal 6, M113 and the commit plan:** they all assume the fix happens. These are goal 6 at line 25, the M113 row at line 703, and the commit split at line 730, which lists three diktor commits.

The plan's general rule, 「判断事項は推奨案で確定」, comes from the owner and appears at 261104-1-plan.md:33 and 261024-1-plan.md:26/59. That rule lets most questions default to the recommendation. Line 12, however, sets an explicit approval precondition for this one item, and line 810 cancels it. Diktor is the test oracle and was out of scope in both earlier plans, so the plan has to say which rule wins.

The inconsistency is real. There are two valid fixes:
- Gate M113's diktor part on a yes to P257, as the finding suggests.
- Drop 「承認を前提にする」 and treat P257 like the other questions, which default to the recommendation.

The finding's suggestion is a sound way to resolve it.

## scope-2

- 観点: scope(範囲と漏れ)
- 重さ: medium
- 節: 7.1 共通の条件 3
- 反証: 2 人のうち 0 人

### 問題

The plan fixes for every later implementation session that commits carry no Co-Authored-By trailer, because the system instruction of the session that wrote the plan forbade it. That condition is per session. It contradicts .claude/rules/commit.md, plan 261104-1's 7.1, and the trailers the owner had re-added to the history on 2026-11-04.

### 根拠

261104-3-plan.md:680-681 ('作業のセッションのシステムの指示が帰属のトレイラを禁じているので、Co-Authored-By のトレイラは付けない'). .claude/rules/commit.md requires 'Co-Authored-By: Claude <noreply@anthropic.com>'. 261104-1-plan.md:542 says to add it. git log shows 69febfd, a648352, ecde210 and 5cbfe33 all carry 'Co-Authored-By: Claude Opus 5.5'; only the plan-draft commit 04db99e lacks it.

### 直し方

Reword: follow commit.md and add Co-Authored-By with the model actually in use. If the implementing session's own system instruction forbids attribution trailers, omit them in that session and report this in the record.

### 反証者 1: 反証できなかった

I confirmed the finding, so it is not refuted. Lines 680-681 of doc/log/261104-3-plan.md (7.1 item 3) tell every implementation session not to add Co-Authored-By. The stated reason is that "作業のセッションのシステムの指示が帰属のトレイラを禁じている", but that held only for the session that wrote the plan. This conflicts with four sources. First, .claude/rules/commit.md requires the trailer. Second, 7.1 item 3 of plan 261104-1 says to add it. Third, git log shows that 69febfd, a648352, ecde210, 5cbfe33 and 76155bb all carry 'Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>', and only the plan-draft commit 04db99e lacks it. Fourth, the owner's recorded rule (memory commit-trailer-conflict) says to add the trailer when a session has no such system instruction, and to omit it and report only when one does. The owner also had the trailer re-added to 232 unpushed commits on 2026-11-04. The plan already reports the conflict in the record, but it turns a condition that holds per session into a standing rule. The suggested rewording is right: follow commit.md, add the trailer with the model actually in use, and omit it only in a session whose own system instruction forbids it, saying so in the record.

### 反証者 2: 反証できなかった

I confirmed the problem, so the finding stands. Lines 680-681 of 261104-3-plan.md (7.1, condition 3) say that every implementing session must leave out Co-Authored-By. The plan's only reason is that the session writing the plan had a system instruction forbidding attribution trailers. That reason is tied to one session, but the rule is fixed for all later ones. It contradicts three sources. First, .claude/rules/commit.md requires the trailer, naming the model actually in use. Second, condition 3 in 7.1 of 261104-1-plan.md says to add the trailer. Third, the owner's memory note commit-trailer-conflict.md says what to do: add the trailer in any session that has no such system instruction, and omit it and report the omission only in a session that does. The history matches this. On 2026-11-04 the owner had the trailers added back to all 200 unpushed parent commits and all 32 unpushed diktor commits. Commits 69febfd, a648352, ecde210, 5cbfe33 and 76155bb all carry 'Co-Authored-By: Claude Opus 5.5'. Only the plan-draft commit 04db99e lacks it. The fix the finding suggests is correct and matches the memory note word for word: follow commit.md and add Co-Authored-By with the model in use, and omit it only in a session whose own system instruction forbids attribution trailers, recording that in the record.

## scope-3

- 観点: scope(範囲と漏れ)
- 重さ: medium
- 節: 4.4 D570 項目 3 / 4.1 D566
- 反証: 2 人のうち 0 人

### 問題

D570 says that when the KIR-processing thread cannot be created even at the minimum stack, keleut exits 1, justified by LangSpec §15.5 row 1 as broadened by D566. The same thread (kir::on_stack) also reads and verifies .kir inputs (keleut run FILE.kir, keleut kir verify, dis, asm). For those inputs there is no type check, and §15.5 row 1 does not apply; IRSpec gives KIR reading limits exit 65. The plan does not define the exit status on the .kir path, so either .kir inputs get 1, which no spec assigns, or the behavior is left undefined.

### 根拠

rustkeleut/crates/kir/src/lib.rs:105-127: on_stack is the shared thread runner, and read_kir_with (lib.rs:136-138) calls on_stack(lim.stack_bytes, ...). IRSpec.md:1655 and :1670 make reading and verification rejections 65. 261104-3-plan.md:459 ('KIR の処理のスレッドは 1(D566)') has no distinction by input kind. Section 1.2 (:144) itself measured .kir runs failing with 70 under ulimit -v.

### 直し方

In D570, specify separately the exit status for a .kel input (1 under D566) and for a .kir/.kirt input, or for keleut kir *: for example 65 as a reader limit, or 3 as lack of space for evaluation. Add the matching sentence to the IRSpec change in M107. Add a .kir case to the M110 tests.

### 反証者 1: 反証できなかった

I confirmed the problem. In crates/kir/src/lib.rs:105-138, on_stack is the one thread runner for every KIR operation. read_kir_with and read_kirt_with call it, so `keleut run FILE.kir`, `kir verify`, `kir dis` and `kir asm` all go through the same "KIR の処理のスレッド" as the .kel path.

The plan's D570 item 3 (plan line 459) says flatly that when that thread cannot be created even at the 16 MiB minimum, keleut exits 1 under D566. It makes no distinction by input kind. D566 changes only LangSpec §15.5. That table covers source programs, which have a type-check step. Its added sentence describes 実行の前の変換 as the processing done after type checking, so it does not cover a .kir input, which has no type check.

IRSpec handles .kir inputs differently. Reading and verification rejections exit 65 (IRSpec.md:1655, and 13.9 at :1670). Readers must not crash before reaching the limit (:1672). IRSpec says nothing about failing to create the reader thread.

D567's new IRSpec exclusions do not fill the gap. They name only Node or V8 failures (70), LLVM or Rust allocations after the reserve runs out (134), and SIGKILL (137). None of them is about failing to create the KIR thread on a .kir input.

The M110 tests (plan lines 463-473) check a .kir input under ulimit -v only for exit 0. They never check the minimum-stack failure for .kir. The plan itself measured .kir runs exiting 70 under ulimit -v (line 144).

So for .kir and .kirt inputs, either they get exit 1, which no spec assigns, or the behavior is left undefined. The finding is correct, and its suggestion to state a separate exit status for .kir inputs, add it to the IRSpec text and add a .kir test is reasonable.

### 反証者 2: 反証できなかった

I checked the finding against the repo and it holds. In D570 item 3 (plan line 459), every case where the KIR-processing thread cannot be created even at its 16 MiB minimum exits 1, citing D566. The plan does not separate this by input kind. kir::on_stack (rustkeleut/crates/kir/src/lib.rs:105-127) is the one thread runner behind read_kir_with, read_kirt, verify, verify_extension, encode, print and print_to (lib.rs:138-237). So the rule also covers `keleut run FILE.kir` and `keleut kir verify|dis|asm`, which have no type check. D566's new sentence for the §15.5 table (plan 4.1) defines 実行の前の変換 as work done after type checking, and §15.5 covers Keleut programs. Nothing in it assigns 1 to a .kir input. IRSpec 13.8 and 13.9 (around IRSpec.md:1655 and :1670) give every reading and verification rejection, including reader limits, exit 65. The plan mentions .kir only in measurements (lines 123, 144) and in one success test (line 467, a .kir under ulimit -v should exit 0). Neither the D567 IRSpec text nor the M107 or M110 entries say what a .kir input exits with when the thread cannot be created. The gap is real. The suggested fix (state the .kir/.kirt and `keleut kir` status separately, add the sentence to the M107 IRSpec change, add a .kir case to the M110 tests) would be correct and is not covered anywhere in the plan.

## scope-4

- 観点: scope(範囲と漏れ)
- 重さ: low
- 節: 2.2 範囲に入れないもの (3 つ目の項目)
- 反証: 2 人のうち 0 人

### 問題

The plan drops the missing test for stdout failures with OS errors not in the table, on the grounds that Linux has no ordinary way to produce EDESTADDRREQ and similar errors. That is false: an unconnected socket used as stdout produces them. The item could be closed cheaply in M112 instead of being dropped on an incorrect reason.

### 根拠

Probe (scratchpad work/verify-scope/sock.py, echoln("hi")): with stdout as an unconnected AF_INET SOCK_DGRAM socket, keleut run --backend interp prints 'keleut: 標準出力に書き出せません: Destination address required' and exits 74. With an unconnected AF_UNIX SOCK_STREAM socket it prints '... Transport endpoint is not connected' and exits 74. --backend wasm and diktor give the same results (diktor: prefix). rustkeleut/conformance/bytes/bytes.t already uses python3, so a cram using this method fits existing practice.

### 直し方

Add one or two commands to conformance/bytes/bytes.t in M112 that run the program with stdout set to an unconnected UDP socket and an unconnected Unix socket (the 74 exit and the message), and close the record 3845 item with them. At minimum, correct the stated reason.

### 反証者 1: 反証できなかった

I confirmed the finding. Plan line 241, in the third bullet of section 2.2, gives this reason for leaving the test out: `EDESTADDRREQ` and similar errors cannot be produced on an ordinary Linux system. That reason is false. I reproduced the probe with a scratch Python script, passing an unconnected socket as the child's stdout and running `echoln("hi")`. With an AF_INET SOCK_DGRAM socket, `keleut run --backend interp` exits 74 and prints 'keleut: 標準出力に書き出せません: Destination address required'. With an AF_UNIX SOCK_STREAM socket it exits 74 and prints '... Transport endpoint is not connected'. Diktor gives the same two results, with the prefix 'diktor:'. The item is still open in record 261024-3 at line 3845 ('表に無かった誤り(EDESTADDRREQ、ENOTCONN など)で標準出力が失敗する試験は足していない'), and the plan does not handle it anywhere. `rustkeleut/conformance/bytes/bytes.t` already calls python3 at line 50 (`python3 nb.py`), so a cram test built this way fits existing practice. Adding it is cheap. At minimum, the plan should correct the stated reason.

### 反証者 2: 反証できなかった

I confirmed the problem, so the finding stands. Line 241 of doc/log/261104-3-plan.md drops the test for OS errors outside the table on the grounds that ordinary Linux has no way to cause EDESTADDRREQ and similar errors. That reason is false. I re-ran the probe in scratch (now deleted): echoln("hi") with stdout set to an unconnected socket. With an AF_INET SOCK_DGRAM socket, keleut run --backend interp and --backend wasm both print "keleut: 標準出力に書き出せません: Destination address required" and exit 74. With an AF_UNIX SOCK_STREAM socket they print "...: Transport endpoint is not connected" and exit 74. diktor gives the same message and exit code with the "diktor:" prefix in both cases. rustkeleut/conformance/bytes/bytes.t already drives keleut through python3 (line 50, "python3 nb.py keleut run big.kel"), so a cram test written this way follows existing practice. It would also check the strings the wasm host maps these errors to (rustkeleut/host/keleut-host.mjs lines 140 and 159 now list EDESTADDRREQ and ENOTCONN). This is the item the record leaves open in doc/log/261024-3-record.md around line 3845. The test is cheap to add, and the plan should either add it or at least correct the stated reason.

## scope-5

- 観点: scope(範囲と漏れ)
- 重さ: low
- 節: 2.2 範囲に入れないもの / 5.3 D580 (V211)
- 反証: 2 人のうち 0 人

### 問題

Section 2.2 says the slowness of kw on deep recursion and yields will be decided 'by the same measurement as V211's return condition'. D580's V211 return condition is not a measurement: it is Node enabling --experimental-wasm-wasmfx by default so that modules with switch compile. The out-of-scope item therefore has no real criterion.

### 根拠

261104-3-plan.md:240 ('kw の遅さは V211(WasmFX)の戻す条件と同じ測り方で決める') vs :643 (V211: 'Node の出荷の版で --experimental-wasm-wasmfx が既定で有効になり … 戻す'). Record lines 3844 and 4000 are the slowness items.

### 直し方

Either give the kw slowness its own return condition (a bench/programs case and a threshold ratio against ki or kt1000), or state plainly that it is deferred until V211 is reopened, without claiming a shared measurement.

### 反証者 1: 反証できなかった

The finding holds. At 261104-3-plan.md:240, the plan says the slowness of kw is decided "V211(WasmFX)の戻す条件と同じ測り方で" (by the same way of measuring as V211's return condition). The only return condition the plan gives for V211 is at :643 (D580): reopen it once a shipped Node enables --experimental-wasm-wasmfx by default and modules containing switch compile without crashing. That condition is an external event, not a measurement. No other line in the plan mentions V211 (193, 759 and 786 give only its status and the probe material), so "同じ測り方" points to nothing. The kw slowness items (record lines 3844 and 4000, listed at :203) therefore have no criterion of their own. The suggested fix is sound and does not conflict with anything in the plan. Either state that kw slowness is deferred until V211 reopens, or give it a bench/programs case with a ratio threshold.

### 反証者 2: 反証できなかった

The finding holds. Line 240 of the plan says the kw slowness is decided "V211(WasmFX)の戻す条件と同じ測り方で" (by the same measurement as V211's return condition). D580's V211 row (line 643) is the only stated V211 return condition anywhere in the plan; V211 also appears at 193, 759 and 786, and none of those adds another condition. That row is not a measurement: V211 comes back when shipped Node enables --experimental-wasm-wasmfx by default and modules containing switch compile without crashing. No bench program, ratio or threshold is attached to the kw slowness anywhere. Line 203 only lists the item, citing record lines 3844 and 4000. So the out-of-scope item claims a shared measurement that does not exist. The suggested fix is sound and cheap: either say plainly that the slowness waits until V211 is reopened, or give it its own bench/programs case and a ratio against ki or kt1000. Severity is low because the item is out of scope either way.

## scope-6

- 観点: scope(範囲と漏れ)
- 重さ: low
- 節: 5.3 D579 / 7.2 M114, M115
- 反証: 2 人のうち 0 人

### 問題

The prototypes in M114 (V225 stage 1) and M115 (V207 alternative 1) have no criterion for keeping or reverting the change. The acceptance only requires measuring the benchmarks and, for M114, no regression of 5 % or more elsewhere. A JIT allocation fast path of 200-300 lines could therefore be committed even if it gives no gain. Also, the V225 stage-2 return condition (gen kt1000 > 2× kw) will almost certainly trigger: the current ratio is 627/146 ≈ 4.3, and stage 1 targets at most libc ~20 % plus Vec ~4 %. The plan should say so rather than leave the item open.

### 根拠

261104-3-plan.md:704-705 (acceptance lists measurement only) and :627-630 (return conditions). perf.md section 3.2: gen kt1000 627 ms vs kw 146 ms; the gen breakdown is libc 20 %, Interp::* about 36 %, krt_fx 7 %.

### 直し方

Add a keep criterion to M114 and M115 (for example, at least X % improvement on gen/tailresume or tail64 at load average ≤ 1; otherwise revert and record the values). Note in D579 that the stage-2 condition is expected to hold after stage 1, so V225 stage 2 remains a deferred design item with that figure recorded.

### 反証者 1: 反証できなかった

I confirmed the finding against the plan. In 261104-3-plan.md, the M114 acceptance (line 704) requires only the gates, measuring gen and tailresume, and that no other benchmark gets 5 % or more slower. The M115 acceptance (line 705) requires only measuring the five programs. Neither D579 (lines 610-631) nor section 12 (進め方の原則) says when a prototype is kept or reverted, so a change with no gain could still be committed. The stage-2 return condition in D579 (line 628, gen kt1000 > 2x kw) is written as an open condition. The plan's own figures (line 190: kt1000 627 ms, kw 146 ms, a ratio of about 4.3) and survey/perf.md:128 (gen breakdown: libc 20 %, the Interp::* functions about 36 %, krt_fx 7 %) show it cannot fail after stage 1. Even removing all of the libc time leaves about 500 ms, well above 2 x 146 = 292 ms. So the plan should say outright that the condition is expected to hold. Both gaps are real, and the suggested change (a keep-or-revert criterion, plus a statement in D579 that stage 2 stays deferred) is reasonable.

### 反証者 2: 反証できなかった

I confirmed the finding against the plan text. The M114 row (line 704) only asks for gen and tailresume to be measured and for no other benchmark to slow down by 5 % or more. The M115 row (line 705) only asks for five benchmarks to be measured. Neither row says what gain the prototype must show to be kept, or that it is reverted when there is no gain. D579 (lines 610-630), the risk section (section 9) and the principles (section 12) have no keep-or-revert rule either. So a 200-300 line JIT fast path could be committed even if it gives nothing. On the stage-2 condition: line 190 of the plan gives gen at kt1000 627 ms and kw 146 ms. That is about 4.3 times, against the threshold of 2 times (292 ms). Line 190 also gives the tailresume breakdown as libc 32 % and Vec 4 %. Even if stage 1 removed all of the libc and Vec share, kt1000 would stay around 400 ms, well above 292 ms. So the stage-2 condition will almost surely still hold after stage 1, but D579 leaves it as an open condition instead of saying so. The finding cites the 20 % libc figure for gen, which may be different, but the conclusion holds either way. The suggested change is sound and low risk: add a keep criterion to M114 and M115, and note in D579 that stage 2 is expected to stay deferred.

## scope-7

- 観点: scope(範囲と漏れ)
- 重さ: low
- 節: 5.1 D573 / 11
- 反証: 2 人のうち 2 人

### 問題

D573 replaces design decision D533 of plan 261024-1 (built-ins written in KIR in rt.kirt) and the 6.7 wide-form requirement with a different design. A ruling made in an earlier owner-reviewed plan is overturned without an owner question, while smaller reversals (V208-V212 closure) do get one (P265).

### 根拠

261104-3-plan.md:522-524 ('計画 261024-1 の D533 … を … に置き換える'). Section 11 has no question for D573 (P257-P265).

### 直し方

Either add an owner question (for example, 'replace D533 with LIR wrapper functions and drop wide forms?', recommended answer yes) or add D573 to P265's scope.

### 反証者 1: 反証した

The finding's premise is that D533 was an owner ruling. It was not. In 261024-1-plan.md, D533 (line 1379, section 4.9) is an ordinary planner decision. No owner question (P) was attached to it.

The implementation had already departed from D533 by the time this plan was written:
- The 261024-3 record, lines 1768-1771, says Array.each and pinned are built as LIR functions in keleut-lir, and rt.kirt may replace them "if M87 writes it".
- Lines 2085-2089 say rt.kirt was not written and leave the choice to M89 (line 1810: "M87 と M89 で決める").
- Lines 2634, 5587 and 5676 repeat this and list it under 残した項目.

So the record itself delegated this choice to the implementer, not to the owner. D573 closes that delegated choice with measured reasons. It is listed in the section 8 裁定 table (line 752) with its alternative, and it is visible in the 0 要約 (line 74) and in the section 2 scope table (line 221). The owner reviews the whole plan, and many other D decisions also have no P (D570, D574, D575).

P265 is not a fair comparison. V208-V212 are ledger entries that previous plans kept as deferred, and closing them changes what the project commits to doing later. D573 changes no behaviour (the record says behaviour is the same either way, and plan 5.1 cites measured agreement with Diktor). The wide-form requirement was also met another way (32-bit operands; a 20,002-slot function measured as working).

Adding a P is not needed under the plan's own convention (section header 判断事項: P questions are for owner-level choices). The finding's premise is false.

### 反証者 2: 反証した

I'm treating this finding as refuted. Its basis, that D573 overturns an owner-made ruling, doesn't hold up.

1. **D533 was never an owner ruling.** In plan 261024-1 it is the planner's own design choice. It appears at line 1379 of that plan's decision table, and none of that plan's P questions concern rt.kirt or the built-ins.

2. **The change already happened in the record.** Record 261024-3 reports that rt.kirt was not written and the wide forms were not made (lines 2085-2090 and 2634). Plan 261024-1 itself allowed the LIR-function bodies (Array.each, pinned) to stand in for rt.kirt (lines 1768-1771). The record lists both as left items (line 5676), which the owner has seen. D573 records that existing state; it does not reverse anything new.

3. **The owner asked for these items to be closed.** The owner's request covers the 残した項目, and these are among them. In the new plan, line 74 and the 番号の無い項目 paragraph (line 806) say D573 closes them.

4. **Section 11 does not hold every decision.** Other new rulings that change earlier design have no question either: D564 (B_prog), D569 (shape table), D570, D571 (value-stack chunks), D579. So D573 lacking a P question is consistent with the plan.

5. **The comparison with P265 doesn't fit.** P265 covers closing deferred ledger items V208-V212, which is a scope choice. D573 is an implementation detail with measured evidence: Diktor agreement on eachfx.kel and the 20,002-slot run.

Adding a question would do no harm, but nothing in the plan is missing or wrong.

## scope-8

- 観点: scope(範囲と漏れ)
- 重さ: low
- 節: 5.2 D577 (V237) / 11 P263
- 反証: 2 人のうち 2 人

### 問題

V237 is closed as an implementation limit: keleut-front counts parse depth by tree depth, so 100,000-level id(...) and else-if chains exit 2 in keleut but 0 in Diktor, the reference. The difference shows up in keleut check and keleut run, not only in the REPL. P263 asks only about V224 (the REPL KIR limit), so the owner is never asked to accept a difference between the main implementation and the oracle on ordinary inputs.

### 根拠

261104-3-plan.md:586-587 (V237 closed, code and tests unchanged) and :820 (P263 mentions V224 only). critic.md 3.8 recommends treating it separately from V224 because it is a keleut check/run difference.

### 直し方

Extend P263 (or add a P) to cover V237 explicitly, with the alternative of counting nesting the way Diktor does so the 100,000-level chains are accepted.

### 反証者 1: 反証した

The finding is literally true: P263 (261104-3-plan.md:820) asks only about V224. It is still refuted, because the plan already handles V237 and the change it suggests would not achieve what it claims.

(1) V237 is not hidden. D577 (:586-588) states it, 1.4 (:167-169) gives the measured facts, the 裁定 table (:756) lists "V224 と V237 を処理系の限度として閉じ" with the rejected alternative, and the ledger row (:798) records it. The owner reviews all of these.

(2) The design behind V237 was already approved. Plan 261024-1 §6.11 (D546, :1106-1111) decided that every recursive front-end pass has a nesting counter and exits 2 under §15.5 when it goes over. Record 261024-3 M96 item 4 (:4153-4156) explains why the counter must count tree depth: later passes recurse over the tree, and left-recursive forms build deep trees without deep parser stacks. The plan only gives this existing, approved behaviour a ledger number.

(3) I measured the run case (scratch work/ref-v237/p.kel, `let id = fn(x) => x` plus 100,000 nested `id(...)`). Diktor's tree evaluator exits 0. `diktor --via-ir` exits 1 with "KIR を書き出せません: 処理系の限度: KIR の書き出しで入れ子が深すぎます". keleut check and keleut run exit 2 with the parse-limit message (入れ子の限度 200000). So Diktor's own KIR path also rejects this input. `keleut run` always goes through KIR, so counting nesting the way Diktor does would not make these chains run. It would only move the failure from the parser to the KIR writer and change exit 2 into a KIR-limit failure. D577 already gives this as its reason: the limit sits in the same place on every KIR path, so lifting one limit leaves the others.

The only real effect of the alternative would be on `keleut check`. Changing it would undo an approved design (D546) and bring back the stack-depth risk the tree-depth count exists to prevent. No new owner question is needed.

### 反証者 2: 反証した

The finding is partly accurate but its suggested change would be largely ineffective, so I refute it. I re-measured the facts in the scratchpad (work/ref2-v237). For a 100,000-level id(...) chain with `let id(x: Int32): Int32 = x` defined, `keleut check` and `keleut run` exit 2 at the parse limit ("入れ子の限度 200000"). `diktor --type-check` and Diktor's tree evaluator accept it. So the difference exists, and P263's question text names only V224. The finding's argument does not hold for three reasons. (1) Diktor's own KIR path also rejects this input: `diktor --emit-ir o.kir id2.kel` exits 1 with "KIR を書き出せません: 処理系の限度: KIR の書き出しで入れ子が深すぎます". Plan line 165 and D577 already record this KIR limit at 99,999/100,000 levels for keleut run, keleut repl and diktor --emit-ir. Counting parse depth the way Diktor does would therefore not let `keleut run` accept these chains. It would change only `keleut check`. The run-path difference is the same limit Diktor's own KIR path has, and P263/D577 already put that limit to the owner. (2) The owner already approved this design. Plan 261024-1 section 6.11 (D546) chose per-pass nesting counters in keleut-front that exit 2 with the §15.5 limit diagnostic. Its default only had to pass the 20,000-level run_exit.t block, and `conformance/frontend-depth/typecheck.t` tests that. V237 records this approved limit and does not add a new behavior choice. (3) D577 is the decision P263 points to in its 本計画の決定 column, and D577 itself (plan line 586) states that V237 is closed. Line 798 of the ledger shows the same, so an owner who reads P263 sees V237. At most, P263's question text could also name V237. The alternative it proposes, Diktor-style counting, would not remove the keleut run difference.

## scope-9

- 観点: scope(範囲と漏れ)
- 重さ: low
- 節: 3.2 D565
- 反証: 2 人のうち 0 人

### 問題

The source of item (1b) also includes M89 item 6: there is no automated or nightly run of the extra-flag acceptance. D565 only adds a manual step at plan close and does not say whether automation is closed or deferred, so that part of the item has no treatment.

### 根拠

records.md section 1.1 table, row (1b): '自動で回す夜間の試験が無いことは :2692-2695(M89 の 6)'. records.md section 4 point 5. 261104-3-plan.md:302-309 mention only the manual procedure.

### 直し方

Add one sentence to D565 that closes or defers the nightly/automated run, giving the reason (for example, no CI in the repository) and a return condition.

### 反証者 1: 反証できなかった

I confirmed the gap. Record 261024-3 lines 2692-2695 (M89 item 6) say the --no-yield-prune comparison does not run automatically yet. They leave that as a remaining item for "the milestone that sets up the nightly test". In plan 261104-3, D565 (section 3.2, around lines 302-309) only adds a manual three-way acceptance at plan close, run by M116, and records it in AGENTS.md. A grep of the plan for 夜間, 自動, CI and the record lines 2692/2695 finds nothing. The plan's only M89 mentions (lines 134 and 804) are about the remaining restriction 1, not item 6. So the automated or nightly run is neither closed nor deferred, and the plan does not say that the manual procedure replaces it. Adding one sentence to D565 that closes or defers it, with a reason and a condition for coming back to it, is a correct and low-cost fix.

### 反証者 2: 反証できなかった

I confirmed the finding. Record 261024-3-record.md:2692-2695 (M89 item 6) says the --no-yield-prune comparison, and the nightly test in plan 261024-1 section 8 (261024-1-plan.md:1255 says the nightly test also runs --gc-stress and --no-yield-prune), does not yet run automatically. It is handled as a manual step for now and is left as "夜間の試験を置くマイルストーンの残りの項目". The record's closing list (around line 5679) mentions only that the extra-flag acceptance was not run at the M101 close. That is the item D565 closes. D565 (261104-3-plan.md:300-309) adds only a manual three-way acceptance at each plan close. Nowhere in the plan (a grep for 夜間/自動/定期/CI finds nothing; sections 2.2 and 2.3 are silent too) is the nightly or automated run closed, deferred or replaced. D565 could be read as replacing the nightly run without saying so, but the plan never states that, so this part of the item is untreated. Adding one sentence to D565 that closes or defers automation, with a reason and a return condition, is a correct, low-severity fix.

## tests-0

- 観点: tests(試験の網羅)
- 重さ: high
- 節: 4.4 (M110 test 1)
- 反証: 2 人のうち 1 人

### 問題

The proposed ulimit threshold for the forget→drop regression test lies where HEAD (forget) already passes, so the test would not fail before the fix and would not catch a regression of the drop.

### 根拠

Plan line 466 sets N to 1.3 × the lowest limit at which the .kir runs. small.md gives that limit as 1,250,000, so N would be about 1,625,000. small.md's table shows forget already exits 0 at 1,500,000; it fails only between 1,250,000 and 1,450,000. My probe with the release keleut at HEAD on gen_decls.py 160000 list, `--native-stack 512k --backend interp d.kel`: ulimit -v 1450000 gave rc=3, 1500000 gave rc=0, and 1625000 gave rc=0. A test at 1.3× passes with or without the fix.

### 直し方

Pick N inside the window where forget fails and drop passes (about 1,300,000–1,400,000 in release). Measure it with the binary cargo test actually runs (CARGO_BIN_EXE_keleut, the debug build). State in the M110 acceptance that the test must fail on the pre-fix commit, as M106 does. If the window proves too narrow to be robust, say so and rely on a KELEUT_TEST_ALLOC_FAIL-based test instead.

### 反証者 1: 反証できなかった

I confirmed the finding. Plan line 466 sets N for the ulimit test in M110 at about 1.3 times the lowest ulimit at which the `.kir` run passes, citing small.md section 1. The table in doc/log/261104-3-spike/survey/small.md (lines 94-101) shows:
- The `.kir` run passes at 1,250,000, so 1.3 times that is about 1,625,000.
- forget already exits 0 at 1,500,000.
- forget fails only between 1,250,000 and 1,450,000 (3 or 70); drop passes there.
- At 1,200,000 both fail with 134.

So a test placed at 1.3 times passes on HEAD and would not catch a regression of forget to drop. The phrase 'M110 で測って決める' does not fix this, because the rule it gives points away from the window. Plan line 152 itself puts the fail range at 1,250,000-1,450,000.

Nothing else in the plan covers it:
- Neither 4.4 nor the M110 row of 7.2 (line 700) requires the test to fail before the fix, although M106 (line 696) and M111 (line 498) do.
- The other M110 test runs the `.kir` under ulimit -v 1000000 with the default native stack. It exercises the thread-stack shrinking (V235), not the drop.

The suggestion is sound: choose N inside the window where forget fails and drop passes, measure it with the binary cargo test runs, and require the test to fail on the pre-fix commit. The window is narrow (about 200 MB, with 134 just below it), so naming KELEUT_TEST_ALLOC_FAIL or another measure as the fallback is reasonable.

### 反証者 2: 反証した

The finding rests on a false premise. It treats 1,250,000 KiB as the lowest limit at which the .kir runs, but small.md line 101 only says the .kir exits 0 at 1,250,000. It does not say that is the lowest passing limit.

I generated gen_decls.py 160000 list, emitted d.kir and ran release keleut at HEAD with `--native-stack 512k --backend interp`:
- **.kir:** it exits 0 at every limit from 1,250,000 down to 725,000 and exits 134 at 700,000. So the real lowest passing limit is about 710–725k.
- **Rule from plan line 466:** 1.3 times that gives N of about 925–945k.
- **.kel at HEAD (forget):** it exits 134 at 1,000,000, 1,050,000 and 1,100,000. It exits 3 at 1,450,000 and 0 at 1,500,000 and 1,625,000, which matches the finding.

So with the plan's rule, N falls about 0.5 GB below the point where forget starts passing. The test would therefore fail before the fix and would catch the drop being reverted. Line 466 also already says N is to be measured in M110 ("目安に M110 で測って決める").

Whether drop passes at about 940k is still unmeasured, but the plan defers that measurement to M110. That is a different question from the finding's claim that the test cannot fail without the fix.

The scratch probe directory has been deleted.

## tests-1

- 観点: tests(試験の網羅)
- 重さ: medium
- 節: 4.4 (M110 test 4) / 7.2 M110
- 反証: 2 人のうち 0 人

### 問題

The small-stack drop tests at 512k and 1m never run at those sizes in `cargo test`. The tests use the debug keleut, and it raises any --native-stack below about 2.3 MiB up to that minimum. The max-depth figures (124/380/1,788) are release-only numbers.

### 根拠

AGENTS.md says a value below the front end's minimum (about 384 KiB in a release build, 2.3 MiB in a debug build) is raised to it. run_cmd.rs and wasm_cmd.rs call env!("CARGO_BIN_EXE_keleut"), which is the debug build under `cargo test`. small.md section 1 measured the 512k/1m/4m depths with target/release/keleut.

### 直し方

Use stack sizes above the debug minimum (for example 4m, 8m, 16m) in the cargo test. Have the test find the maximum accepted depth empirically, for example by bisecting on the limit diagnostic, instead of hard-coding release depths. Or state that these checks are run by hand with the release binary and record the results in the implementation record.

### 反証者 1: 反証できなかった

I confirmed the core problem. Plan 4.4 test 4 asks for drop tests at --native-stack 512k, 1m and 4m. M110 places the tests in crates/keleut-cli/tests/run_cmd.rs, and that file runs env!("CARGO_BIN_EXE_keleut"), which is the debug binary under `cargo test`. That binary raises any smaller --native-stack to the front end's minimum: front_stack_for_run clamps to min_front_stack(), whose rustdoc in emit_kir_cmd.rs:397-416 says about 384 KiB in a release build and about 2.3 MiB in an unoptimized one. So in `cargo test`, 512k and 1m both run at about 2.3 MiB, and the two cases are the same. The plan never mentions this split between debug and release builds. It also does not say how "the largest accepted depth" is to be found, or that the check is run by hand with the release binary. One detail of the finding is wrong: the plan does not hard-code 124, 380 or 1,788. That part of the claim is weaker, but the gap in the test design is real.

### 反証者 2: 反証できなかった

I confirmed the main claim. In plan section 4.4, the M110 tests are under "試験(M110。`crates/keleut-cli/tests/run_cmd.rs`)". One of them reads "小さい `--native-stack`(512k、1m、4m)で、受け付ける最大の深さの木を `drop` で捨てても落ちないこと". run_cmd.rs starts the binary with `env!("CARGO_BIN_EXE_keleut")`, which is the debug build under the plain `cargo test` that 7.1 requires. rustkeleut/Cargo.toml has no custom profile, so that build is unoptimized. emit_kir_cmd.rs:397-416 (`min_front_stack`, `front_stack_for_run`) clamps the front-end stack to at least about 2.3 MiB in an unoptimized build ("最適化なしの構築で約 2.3 MiB"). So in `cargo test`, both 512k and 1m run at about 2.3 MiB, and neither checks the small-stack drop path it names. The plan does not say these checks are run by hand with the release binary; its only "手で走らせる検査" line, at line 407, is about the alloc sweep. One detail of the finding is wrong: the depths 124/380/1,788 do not appear in the plan text. The plan says "受け付ける最大の深さ" without numbers, so that detail does not hold, but the main problem does.

## tests-2

- 観点: tests(試験の網羅)
- 重さ: medium
- 節: 3.1 (M106 tests)
- 反証: 2 人のうち 0 人

### 問題

Nothing automated covers the host change that makes the Worker path size its stack from META's B_prog. The wasm_cmd test at --max-depth 600000 uses the direct path, and the default-L c92 check that goes through the Worker path is manual. If the host keeps the constant B_WASM, `node DIR/main.mjs` would still exit 70 and every test would still pass.

### 根拠

host/keleut-host.mjs:32 has `const B_WASM = 1536`. Header lines 10-12: without KELEUT_WASM_DIRECT, the host runs in a Worker with resourceLimits.stackSizeMb, which is the `node main.mjs` form. At --max-depth 600000 with k=1, plan 3.1 gives a stack below STACK_SIZE_MAX_KIB, so stack_plan (launch.rs:224) picks the direct path. The plan's only Worker-path checks are the manual c92 run in the M106 acceptance and the risk-table item to measure with the Worker.

### 直し方

Add a wasm_cmd test that runs `keleut build --target wasm --max-depth 600000 -o DIR` on the same shape-c program, then `node DIR/main.mjs`. Expect 2/bottom/7 and exit 0, and confirm the test fails before the fix. This covers both META.B_prog and the host's Worker sizing.

### 反証者 1: 反証できなかった

I confirmed the finding against the plan and the code. Plan 3.1 (line 279) adds only one automated test, in wasm_cmd.rs: `--max-depth 600000`, N = 299,000, run with `--backend wasm`. With k = 1, B_prog = 1536 + 1360 = 2896 and R = max(1024, 600000/64) = 9375. The stack is then (609375 × 2896 + 32 MiB)/1024 ≈ 1,756,000 KiB, which is below STACK_SIZE_MAX_KIB (2,097,151). So stack_plan (launch.rs:221-233) returns Some, the direct KELEUT_WASM_DIRECT path runs, and the host's Worker branch is never exercised. That branch (host/keleut-host.mjs:763-789) computes stackKiB from the constant B_WASM (line 32), and only META.maxDepth comes from META. If M106 updated stack_size_kib but forgot the host, the new test and the k=0 unit test would still pass. The only check that reaches the Worker path is the c92.kel run at `--max-depth 1048576` (about 3.04M KiB). That check is in the M106 acceptance column at line 696 and in the risk-table row at line 767, and both are manual steps, not tests. The existing wasm_cmd test `build_の出力は_node_だけで走る` (line 107) runs a k = 0 program, so it cannot catch the miss either. The suggested `keleut build` plus `node DIR/main.mjs` test is a sensible fix. One caveat: under the old constant the Worker gets about 989 MB of stack, and that may be just enough at N = 299,000. The rule that the test must fail before the fix matters, and N may need to be raised, for example to 300,000 or higher at a larger L.

### 反証者 2: 反証できなかった

I confirmed the finding. host/keleut-host.mjs:32 defines `const B_WASM = 1536`. The Worker path at lines 763-789 sizes stackSizeMb from META.maxDepth multiplied by the B_WASM constant. That is the only place the host sizes a stack. With KELEUT_WASM_DIRECT=1, the host just runs on the stack that `keleut run` set up.

Plan 3.1 has one automated test, in wasm_cmd: `--backend wasm` at `--max-depth 600000` with k=1. That run takes the direct path, not the Worker path. With B_prog = 1536 + 1360 = 2896 and R = 16384 (derived from stack_size_kib(1<<20) = 1630208), the stack is (616384 × 2896)/1024 + 32768 ≈ 1,776,000 KiB. That is below STACK_SIZE_MAX_KIB (2,097,151), so stack_plan (launch.rs:221) returns Some, and keleut launches node directly with --stack-size.

The other checks the plan gives for the host's Worker sizing are all manual or measurement steps:
- The M106 acceptance runs c92 at `--max-depth 1048576`.
- The risk table says to measure with the Worker.
- The bytes.sh measurements.

So if M106 changed launch.rs and META but left the host on B_WASM, the planned test would still pass. Default-L k=1 programs and `node DIR/main.mjs` would still exit 70.

The suggested test is feasible. build_cmd.rs:21 shows that `keleut build` accepts `--max-depth`. wasm_cmd.rs:137-142 already runs `node DIR/main.mjs` with KELEUT_WASM_DIRECT removed, which is the Worker path. At 600000 with B_WASM, the Worker stack (~957k KiB) is too small for N = 299,000 frames of about 2.9 KB each. So the new test would fail before the fix and pass after it.

## tests-3

- 観点: tests(試験の網羅)
- 重さ: medium
- 節: 4.5 (M111 test 3)
- 反証: 2 人のうち 0 人

### 問題

The injection test is self-contradictory. It makes 'the chunk allocation at injection' fail through KELEUT_TEST_ALLOC_FAIL, but D571 removes that allocation (the continuation's copy is linked as a chunk, 'no new allocation needed'). After the fix the injected failure either has nothing to hit or lands on some other ≥MIN allocation, and the test cannot tell which happened.

### 根拠

Plan line 486-487 says the copy becomes one chunk and 新しい確保は要らない. Plan line 500 says 注入のときの塊の確保を KELEUT_TEST_ALLOC_FAIL で失敗させて, expecting inner/own/outer cancel and 3. alloc.md 5.3 says injection currently reuses the old chunk, which is why the limitation never reproduced.

### 直し方

Reword the test: after capturing the continuation and forcing a collection (so chunks are reclaimed), fail the next exec-role allocation of at least 1 MiB. Before the fix this hits injection's chunk allocation and loses inner cancel; after the fix the expected order appears. Check that it fails on the pre-D571 code, or on code with only the reclamation half applied. Optionally have KELEUT_TEST_ALLOC_FAIL report which allocation it failed (stderr or a counter) so the test can assert that the intended site was hit.

### 反証者 1: 反証できなかった

I confirmed the finding against the plan text. Plan lines 486-487 (D571) say that at injection the exact-length copy of the continuation, held in a `Vec<u64>`, is linked into the value stack as one chunk, and that no new allocation is needed (新しい確保は要らない). The M111 test at line 500 makes "the chunk allocation at injection" (注入のときの塊の確保) fail through `KELEUT_TEST_ALLOC_FAIL`. After the fix that allocation does not exist.

`KELEUT_TEST_ALLOC_FAIL` (lines 391-394) is defined only by `ROLE:MIN:SKIP` (role, minimum size, and how many qualifying allocations to skip), not by call site. So a failure that targets ≥1 MiB lands on whatever qualifying allocation comes next. The copied chunk has exactly the frames' length, so that is probably the chunk taken when the cancel clauses push frames above it, which is a different scenario with a different expected output. Neither the test nor the plan says which allocation is hit.

Under D568, a single injected failure of a chunk allocation also first retries after releasing the global reserve and then falls back to the reserve chunk (line 388). The test therefore may not exercise the failure it describes at all.

alloc.md 5.3 confirms the background. Injection currently reuses the old chunk, which is why the limitation never reproduced. alloc.md 5.5 also separates the two tests: forced failure before the fix, a collection after capture after the fix. The plan merged them into one test that contradicts D571.

Nothing else in the plan handles this. The suggested rewording is sound: force a collection after capturing the continuation, then fail the next ≥1 MiB exec-role allocation, and check that the test fails on code before D571 or with only the reclamation half applied. So is the optional report of which allocation was failed.

### 反証者 2: 反証できなかった

I could not refute the finding. The plan contradicts itself. Lines 486-487 (D571) say that injection links the continuation's exact-length Vec<u64> copy into the value stack as one chunk, and that no new allocation is needed (新しい確保は要らない). Line 500 (M111 test 3) then fails 注入のときの塊の確保 through KELEUT_TEST_ALLOC_FAIL. After D571 that allocation no longer exists. KELEUT_TEST_ALLOC_FAIL=ROLE:MIN:SKIP (plan line 391) fails the SKIP+1-th allocation of at least MIN bytes. So after the fix the injected failure either never fires or lands on some other allocation of 1 MiB or more, and test 3 cannot tell which. Because continj.kel exhausts the heap by itself, the expected order (inner cancel, own cancel, outer cancel, then exit 3) can appear even when the injected failure never fired, so a passing run proves nothing. alloc.md 5.3 confirms that today injection reuses an old chunk without allocating (stack.rs:233-234), which is why the limitation never reproduced. alloc.md 5.5 has the same unresolved wording. The plan does not say how test 3 should still find its target after the fix, or how to show that it fails on the pre-D571 code. The suggested rewording is sound. Force a collection after the continuation is captured, then fail the next allocation of at least 1 MiB. Before the fix that hits injection's chunk allocation, and the test should be shown to fail on code with only the reclamation half applied. Having KELEUT_TEST_ALLOC_FAIL report whether it fired would let the test check that the intended site was hit. The finding stands.

## tests-4

- 観点: tests(試験の網羅)
- 重さ: medium
- 節: 4.2 (M108 tests)
- 反証: 2 人のうち 0 人

### 問題

The four site-targeted failure tests (inside collection, rec_fields, call_clause args, value-stack chunk) are aimed only by ROLE:MIN:SKIP counts. Nothing confirms the injected failure hit the intended site, and the counts differ between debug and release, between tiers, and with later code changes. The plan also never says when the exec-role flag is cleared. If cancel clauses do not allocate, the expected 'cancel' line appears even when the flag stays set forever and every later allocation fails.

### 根拠

Plan line 391-394 describes the env var as count-based. Plan line 403-404 expects before/cancel/メモリ不足/3 for each site with --backend interp and --tier jit-only. Plan line 372-373 says the exec flag is checked at the heap entry, but no line says when it is reset. The probe cancel clauses (oom.t, continj.kel) only call echoln with a literal.

### 直し方

Have the test hook name the site it failed, for example a stderr line when an env debug flag is set, and assert on it. Alternatively add a site-keyed form (ROLE:SITE). Specify when the flag clears (for example after the OutOfMemory stop is raised, so cancel clauses can allocate again). Make at least one test's cancel clause allocate, such as echoln(show(n)) or building a short list, so a flag that never clears is caught.

### 反証者 1: 反証できなかった

The finding holds. In the plan's section 4.2 (lines 360-406), D568 says the exec-role flag is raised when the reserve is returned (lines 365-367). It also says the flag is checked at the heap entry and at the chunk path, which returns Stop::OutOfMemory (lines 372-374). Nowhere in the plan is the flag lowered or reset. A grep for 下ろ, 戻す, 消す and 外す in that context, and for every line mentioning 印, found nothing. The one later mention is line 423, which covers re-failure inside a cancel clause, and only for the shape table, which has no reserve. KELEUT_TEST_ALLOC_FAIL=ROLE:MIN:SKIP is a pure count (lines 391-394). Under D568 the failed allocation is retried after the reserve is released, so the targeted allocation itself succeeds. Only the flag records it, and it is observed later. Nothing in the M108 tests (lines 403-404) confirms which site took the failure. Those tests expect only before / cancel / メモリ不足 / 3, and that output would look the same whichever site, or how many sites, consumed the injected failure. Counts that change between debug and release, tier and code version make the four "site" tests liable to pass without exercising their target. The fix is cheap: a site report under a debug variable or a site-keyed form, a stated rule for when the flag clears, and one cancel clause that allocates. None of it is wrong to add.

### 反証者 2: 反証できなかった

I checked this against the plan text and the finding holds. Plan lines 363-394 describe the flag being set ("予備を返して役割ごとの印を立て") and checked at the heap entry and value-stack chunk path ("立っていれば Stop::OutOfMemory を返し"). No line says when the exec-role flag is cleared. A grep for 取り直/下ろ/消/戻 finds only the REPL re-taking its reserve before the next input (line 379), which is not the same as clearing the flag within a run. So once the flag is set, every later heap allocation, including one inside a cancel clause, would also return OutOfMemory. Lines 391-394 aim the failure only with ROLE:MIN:SKIP counts ("MIN を小さくして SKIP を選べば回収の中や rec_fields の確保を狙える"). Lines 403-404 test only the visible output (before, cancel, メモリ不足, 3) on interp and jit-only. Nothing confirms which site actually failed, and the allocation counts can differ between tiers and builds. Line 438 adds a case where an extension inside a cancel clause fails again, but that is a shape-table case and does not cover the flag's lifetime. The suggestion fits the plan: specify when the flag clears, have the test hook report the site it failed, and make at least one cancel clause allocate.

## tests-5

- 観点: tests(試験の網羅)
- 重さ: low
- 節: 7.2 M108 / M114 / M115
- 反証: 2 人のうち 0 人

### 問題

Performance risks get no acceptance threshold where they first appear, and the optimization milestones have no rule for whether to keep the change.

### 根拠

Risk table line 768 says M108 measures `ki` with bench/run.py, but the M108 acceptance (line 698) has no bench item. M114's acceptance has a 5% no-regression threshold for other benchmarks. M114 and M115 only 'measure' gen/tailresume and the five programs, with no improvement criterion or revert rule for a prototype that gains nothing.

### 直し方

Add to the M108 acceptance: bench/run.py --configs ki,kt1000 --interleave at load average ≤1, with no benchmark more than 5% slower than the M106/M114 baseline. For M114 and M115, state the keep/revert rule, for example: commit only if the target benchmarks improve by more than the noise band; otherwise record the numbers and revert.

### 反証者 1: 反証できなかった

I checked the finding against /home/sumito3478/repo/keleut/doc/log/261104-3-plan.md and it holds.

1. Line 768 in the risk table says M108 measures `ki` with `bench/run.py`. But the M108 acceptance at line 698 has no bench item, and the common conditions in 7.1 have none either. So the mitigation has no threshold.
2. M114 at line 704 sets a 5% no-regression limit only for the other benchmarks. For gen and tailresume it just says to measure them.
3. M115 at line 705 just says to measure the five programs.
4. D579 (lines 610-630) gives conditions only for revisiting the deferred later stages: V225 stage 2 if gen's kt1000 time stays above twice kw's, and V207's unboxed-argument entry and alternative 2 if tail64 stays above twice tail32b. No rule says to drop or revert stage 1 or alternative 1 if they gain nothing (or gain less than the noise). Searching for wording like 効か, 改善 or コミットしない found no such rule anywhere in the plan.

The suggestion would not be wrong. Adding a bench item to M108 and a keep/revert rule to M114 and M115 matches the plan's own load-average ≤ 1 measurement rule at lines 831-832.

### 反証者 2: 反証できなかった

I checked the finding against the plan and it holds. In the risk table (line 768), the mitigation for the global-allocator wrapper slowing things down is "M108 で bench/run.py の ki を測る". The M108 acceptance cell (line 698) has no bench item and no threshold, so that mitigation is not enforced anywhere. M114's acceptance (line 704) has only a no-regression rule ("ほかのベンチマークが基準より 5 % 以上遅くならないこと") and says nothing about gen/tailresume improving. M115's acceptance (line 705) only says "bench/run.py で本計画 5.3 節の 5 本を測る", with no threshold and no rule for regressions. D579 and section 5.3 (lines 610-633) give return conditions for the deferred parts (段 2, the entry, 代案 2). They give no rule for keeping or reverting the prototypes themselves (段 1, 代案 1) if those gain nothing or slow things down. The commit-split section (lines 733-735) assumes those commits always land. The suggested change fits the plan's existing style (the 5% rule, measuring at load average ≤1) and would not be wrong. It is low severity, but the gap is real.

## tests-6

- 観点: tests(試験の網羅)
- 重さ: low
- 節: 7.2 M110 / M108 (feature gating)
- 反証: 2 人のうち 0 人

### 問題

Tests that pass --tier jit-only (M108 oom_cmd, M110's depth-1M recursion, M111) are not marked as needing the llvm feature, but common condition 7.1 requires `cargo test --no-default-features` to pass. There jit-only exits 4.

### 根拠

AGENTS.md says --backend jit and jit-only exit 4 without the llvm feature. The plan says 'feature llvm があれば' only for the M106 test (line 280). Lines 404, 468 and 500 use --tier jit-only without that guard.

### 直し方

State that every jit-only or tier variant in the new tests is guarded by cfg(feature = "llvm") or by the `backends:` line of `keleut --version`, as the existing jit tests do.

### 反証者 1: 反証できなかった

The finding holds. Plan section 7.1 item 1 (around line 675) requires `cargo test --no-default-features` to pass. The only `--tier jit-only` use that the plan guards with "feature `llvm` があれば" is the M106 wasm_cmd test at line 280. The new tests at lines 404 and 438 (M108, `tests/oom_cmd.rs`, which does not exist yet), line 468 (M110, depth-1M recursion) and line 500 (M111, continj) all list `--tier jit-only` with no guard. Nothing elsewhere in the plan, including 7.1, states a general rule for this.

In the code, `crates/keleut-cli/src/run_cmd.rs` lines 28-32 say that `--tier` other than `interp-only` without `--backend` selects `jit`, and that `jit` is unimplemented (exit 4) in a `--no-default-features` build. So these tests would fail that required check unless they are guarded.

The existing practice matches the suggestion: `crates/keleut-cli/tests/jit_cmd.rs` is gated with `#![cfg(feature = "llvm")]`.

The severity is low, because an implementer would likely notice when running the check. Still, the plan is inconsistent as written, and the suggested fix (state that every jit-only or tier variant is guarded by `cfg(feature = "llvm")`) is correct.

### 反証者 2: 反証できなかった

I checked this and the finding holds. The plan says "feature `llvm` があれば" at only one place, line 280, for the M106 comparison. The new oom_cmd.rs tests in M108 and M111 (lines 404, 438 and 500), the depth-1,000,000 recursion in M110 (line 468) and M115's "oom_cmd.rs が --tier jit-only で通ること" (line 705) all run `--tier jit-only`, and none of them says how that run is guarded. Condition 1 of section 7.1 (line 675) requires `cargo test --no-default-features` to pass, and in that build jit-only exits 4. The existing JIT tests do guard: jit_cmd.rs has `#![cfg(feature = "llvm")]` at line 8. oom_cmd.rs is new and mixes interp and jit-only cases, so a whole-file cfg like jit_cmd.rs's would skip its interp cases too. The jit-only cases need to be gated one by one, and the plan never says so. Severity is low because condition 1 would force whoever implements it to add the guards anyway. The suggested wording fix is right.

## tests-7

- 観点: tests(試験の網羅)
- 重さ: low
- 節: 7.2 M113
- 反証: 2 人のうち 0 人

### 問題

The acceptance item 'kconform --pairs dk,dk-ir,kf,x-ir,ki conformance/bytes/bytes.t で V218 の項目が使われずに差 0' is ambiguous about order, because the same milestone deletes the V218 entries. Once they are deleted, 'unused' cannot be observed.

### 根拠

Plan line 571 deletes known-diffs.toml:416-434. Plan line 703 asks for those entries to be reported as unused.

### 直し方

Split it into two steps. With the rebuilt diktor and the entries still present, kconform reports the two V218 entries as unused. After deleting them, the same command shows 0 differences.

### 反証者 1: 反証できなかった

I confirmed the problem. Plan line 571 (D576) removes the two V218 entries at known-diffs.toml:416-434 within M113. The commit order at lines 730-731 puts the parent's deletion commit inside the same milestone. The M113 acceptance on line 703 still asks for "V218 の項目が使われずに差 0", but the plan never says whether that check runs before or after the deletion. The phrase reads as an observation that the entries were reported as unused. After the deletion nothing can be reported, so the only remaining reading is "diff 0 without the entries", which the text does not state. The pre-deletion check is observable. kconform (rustkeleut/crates/kconform/src/main.rs:667-685) reports unused known diffs even for single-file runs: with --verbose it lists each one as "使われなかった既知の差: file:line(ledger)", and the summary line on line 703 always gives the count. So the suggested two-step check is feasible and not wrong. Step 1: with the rebuilt diktor and the entries still present, the two V218 entries are reported as unused (use -v to name them). Step 2: after the deletion, the same command shows diff 0. The plan does not handle this elsewhere. Severity is low, because the final check (entries deleted and diff 0) already shows that the fix works.

### 反証者 2: 反証できなかった

I confirmed the problem. Plan line 571 (D576) deletes the two V218 entries at known-diffs.toml:416-434 within M113. Line 731 (7.3) puts that deletion in M113's own parent commit. The M113 acceptance cell (line 703) is checked at the end of the milestone, so by then the entries are gone. "V218 の項目が使われずに差 0" then either cannot be observed or is just redundant with "差 0". Nothing in the plan says to check the unused report before the deletion.

The suggested two-step check works with the current tool. In kconform main.rs:666-678, an entry counts as unused when it was not used, one of its pairs ran, and its file is among the cases run. The summary line at 703-705 prints that count, and --verbose lists each entry with its ledger number.

So the proposed order is observable and stronger evidence than the current wording:
1. With the rebuilt diktor and the entries still present, `kconform --pairs dk,dk-ir,kf,x-ir,ki conformance/bytes/bytes.t` reports the V218 entry for these pairs as unused. Only the dk/dk-ir/kf/x-ir entry can be reported. The ki-group entry is reported only if ki runs with the --compare-pairs mode that applies against = "pair" entries, which needs checking.
2. After the deletion, the same command shows 0 differences.

The finding is low severity but valid.

## spec-0

- 観点: spec(仕様の文の整合)
- 重さ: high
- 節: 7.1 共通の条件 item 3 (plan line 680)
- 反証: 2 人のうち 0 人

### 問題

The plan tells implementers to leave out the Co-Authored-By trailer because "the work session's system instructions forbid attribution trailers". That is not true for the current sessions. It contradicts .claude/rules/commit.md, which AGENTS.md points to, and it breaks with the precedent of the previous plan.

### 根拠

commit.md requires `Co-Authored-By: Claude <noreply@anthropic.com>`. Plan 261104-1 line 542 says to add Co-Authored-By. `git log` shows 69febfd and a648352 both carry `Co-Authored-By: Claude Opus 5.5`. In this session the system git instruction only says to append any attribution lines a system-reminder gives; it forbids nothing. The auto-memory rule commit-trailer-conflict.md says to add the trailer unless a system instruction forbids it.

### 直し方

Rewrite item 3 to follow commit.md: add the Co-Authored-By trailer with the session's model name, and no Claude-Session trailer. Leave the trailer out only when that session's system instructions explicitly forbid attribution, and report it in the record when that happens.

### 反証者 1: 反証できなかった

I checked this and the finding holds, so it is not refuted. Plan line 682 tells implementers to leave out Co-Authored-By because, at the time the plan was written, the work session's system instructions forbid attribution trailers. Nothing supports that premise:
(1) .claude/rules/commit.md requires `Co-Authored-By: Claude <noreply@anthropic.com>` with the session's model name, and no Claude-Session trailer.
(2) Item 3 of 7.1 in plan 261104-1 (line 542) says to add Co-Authored-By.
(3) The latest implementation commits, 69febfd and a648352, carry `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Only the plan-draft commit 04db99e has none.
(4) In the current session the system git instruction only says to append the attribution lines a system-reminder gives, when there is one. It forbids nothing.
(5) The memory file commit-trailer-conflict.md says to add the trailer whenever no system instruction forbids it. It also records that on 2026-11-04 the owner had every unpushed commit rewritten to add the trailer, which shows the owner wants it.
Item 3 therefore contradicts commit.md and that precedent. The suggested rewrite is correct: follow commit.md, add Co-Authored-By with the session's model name and no Claude-Session, and leave the trailer out only when that session's system instructions explicitly forbid attribution, reporting it in the record if that happens.

### 反証者 2: 反証できなかった

I confirmed the finding. Plan line 680 says that, when the plan was written, the work session's system instructions forbid attribution trailers, and it concludes that `Co-Authored-By` is left out. That conclusion conflicts with `.claude/rules/commit.md`, which requires `Co-Authored-By: Claude <noreply@anthropic.com>` with the session's model name, and with plan 261104-1 line 542, which says to add the trailer. It also conflicts with recent history: 69febfd and a648352 both carry `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. The memory rule commit-trailer-conflict.md decides this per session: add the trailer by default, and leave it out only when that session's system instructions say to (then report it). This session's git instruction forbids nothing. It only asks for attribution lines from a system-reminder when one is present. The plan's own draft commit, 04db99e, has no trailer, so the session that wrote the plan may have had such a restriction. Even so, freezing that one session's condition as a blanket rule for every later implementation session is wrong. Follow the suggested rewrite instead: add the trailer by default, leave it out only when the implementing session's system instructions forbid attribution, and record it when that happens.

## spec-1

- 観点: spec(仕様の文の整合)
- 重さ: high
- 節: 4.1 D566 / 4.3 D569 / 4.4 D570 item 3 (vs LangSpec §15.5 table, doc/LangSpec.md:1781-1785)
- 反証: 2 人のうち 0 人

### 問題

D566's new definition of 実行の前の変換 covers whatever the implementation does "after type checking and before initialization starts". Its stated rationale is that every limit hit before initialization goes to row 1. Yet the plan sends two other before-initialization failures to exit 3: the execution thread cannot be created (D570 item 3, 4.1 line 333) and the shape-table registration at startup fails (D569, 'ctx.rs 119-123 ... 初期化の前のメモリの不足として 3'). Row 3 of the §15.5 table reads '初期化の途中で実行時エラーが起きた / 途中まで行う', and §15.5 line 1772 talks about 評価に要する領域. Neither literally covers a failure before any evaluation has started. So the revised table and the plan disagree on where the boundary lies.

### 根拠

Plan line 324 gives the new sentence (型検査の後、初期化を始める前に処理系が行う処理...その範囲は処理系が定める). Line 335 gives the reason: 3 つとも、初期化を始める前に処理系の限度に当たる場合. Line 333 sends execution-thread failure to 3. Line 418 sends startup shape registration to 3. The row 3 text is at doc/LangSpec.md:1785.

### 直し方

Make the boundary explicit in D566. Either (a) say that 実行の前の変換 ends when the runtime is set up, and amend row 3's condition (or the sentence at line 1772) so that it covers a 評価に要する領域 shortage before the first initialization step ('初期化を始める前か途中で' / '初期化: 途中まで、または行わない'), or (b) move execution-thread and startup-registration failures to row 1. Record the choice in D566 and P259.

### 反証者 1: 反証できなかった

I confirmed the problem in the files. In doc/log/261104-3-plan.md, line 324 defines 実行の前の変換 as the processing between type checking and the start of initialization. Line 335 gives the reason for D566: each case hits an implementation limit before initialization starts, so it goes to row 1. Two other lines contradict that. Line 418 (D569) says a failed shape-table registration at startup ends with exit 3 as 初期化の前のメモリの不足, using the plan's own "before initialization" wording. Lines 333 and 459 (D570) send a failure to create the execution thread to 3 as 評価に要する領域の不足. LangSpec §15.5 row 3 (doc/LangSpec.md:1785) reads '初期化の途中で実行時エラーが起きた / 途中まで行う'. Line 1772 covers a shortage of 評価に要する領域 during evaluation. Neither literally covers a failure before any initialization step. The plan never states where 実行の前の変換 ends and evaluation begins. "その範囲は処理系が定める" could cover this, but the plan does not use it here. Its own sentence at line 418 places these failures before initialization and still sends them to 3. The boundary needs to be stated explicitly in D566 and P259. The finding's option (a) or (b) would fix it.

### 反証者 2: 反証できなかった

I confirmed the problem by reading the plan and LangSpec. It is a wording gap, not a design error, so "high" overstates it.

What the texts say:
- LangSpec.md:1785, row 3 of the §15.5 table, reads "初期化の途中で実行時エラーが起きた / 途中まで行う / 3".
- Plan line 324 (D566) defines 実行の前の変換 as everything the implementation does "型検査の後、初期化を始める前に". It adds "その範囲は処理系が定める".
- Line 335 justifies row 1 by "初期化を始める前に処理系の限度に当たる".
- Line 418 (D569) sends a failure of the startup shape registration (Rt::new_with, ctx.rs:118-123, which runs before any initialization code) to 3, explicitly "初期化の前のメモリの不足として 3".
- Line 333 and D570 item 3 (line 457) send a failure to create the execution thread to 3. The stated reason is 評価に要する領域の不足.

The plan therefore names failures that happen before initialization begins and sends them to row 3. But row 3, even after D566, covers only errors "初期化の途中で". D566 says nothing about where 実行の前の変換 ends, and nothing says that runtime setup counts as the start of initialization.

"その範囲は処理系が定める" does not close the gap. A failure the implementation places outside the conversion but before initialization still has no row in the revised table. Line 418's own wording ("初期化の前") contradicts row 3's condition.

Reading line 1772 (評価に要する領域…も実行時エラー) together with "途中まで行う" (zero steps) could rescue this, but only loosely. Nothing in D566 or P259 settles it.

Making the boundary explicit (option a or b) is a sound, small change and does not conflict with any other decision in the plan.

## spec-2

- 観点: spec(仕様の文の整合)
- 重さ: medium
- 節: 5.2 D577 / M107 (vs LangSpec §6.6, doc/LangSpec.md:728-745 and §18.5)
- 反証: 2 人のうち 0 人

### 問題

D577 closes V224 by adding a sentence only to IRSpec 1 節. But LangSpec §6.6 lists exhaustively which REPL inputs are not accepted: 字句と構文の誤り, 型検査に通らなかった入力, 実行時エラーで止まった入力. An input that passes type checking but hits the KIR-writing limit fits none of these. So under LangSpec, keleut repl's rejection is still unspecified, and §18.5's guarantee relies on that list. D566 widens §15.5 row 1 for whole-program runs but leaves §6.6 alone.

### 根拠

probe/decide/v224.out at n=100000: keleut repl prints '<stdin>: 処理系の限度: KIR の書き出しで入れ子が深すぎます' and then rejects later references to x. doc/LangSpec.md §6.6 bullet 3 lists only the three categories.

### 直し方

In M107, extend the §6.6 bullet as well, for example to '字句と構文の誤りのある入力、型検査か実行の前の変換に通らなかった入力（処理系の限度を超えたものを含む）、実行時エラーで止まった入力は受理しない'. Add LangSpec §6.6 (D577) to the D581 table.

### 反証者 1: 反証できなかった

I confirmed the finding. LangSpec §6.6 bullet 3 (doc/LangSpec.md:735) names three kinds of input that are not accepted: 字句と構文の誤りのある入力, 型検査に通らなかった入力 and 実行時エラーで止まった入力. LangSpec has no general clause that lets an implementation reject an input because of an implementation limit. Running grep for 処理系の限度 finds it only in §15.5 (lines 1772, 1783, 1784 and 1792), and those lines cover runtime errors and the exit-status table for whole programs, not REPL acceptance. The plan's own D566 reasoning (plan 4.1) admits that because §15.5 row 1 says 「型検査」, the KIR-writing limit 「厳密には表に当てはまらない」. That is why D566 widens §15.5 row 1 to 「型検査か実行の前の変換」. The same gap exists in §6.6, but the plan does not close it there. D577 (plan lines 579-588) adds a sentence only to IRSpec 1 節, and IRSpec governs KIR readers and writers, not LangSpec's definition of what an interactive run accepts. D578 touches §6.6 only to add the display-format sentence after 「入力の区切り方は処理系が定める。」. A grep for 受理し and §6.6 in the plan finds no change to bullet 3. The D581 documents table (line 654) lists §6.6 only for D578. So under LangSpec, keleut repl's rejection of an input that passed type checking but hit the KIR nesting limit is still not covered. The non-accepted-input guarantee in §18.5 depends on the §6.6 list. The suggestion is consistent with D566: widen bullet 3 to cover 実行の前の変換 or 処理系の限度, and list it under D577 in the table.

### 反証者 2: 反証できなかった

I confirmed the finding. LangSpec §6.6 (doc/LangSpec.md:738) lists the REPL inputs that are not accepted: 字句と構文の誤りのある入力、型検査に通らなかった入力、実行時エラーで止まった入力. §18.5 (line 2052ff) builds on §6.6 and says rejected inputs are not visible to later inputs. In the plan, D577 (doc/log/261104-3-plan.md:579-588) adds the REPL-limit sentence only to IRSpec 1 節 and AGENTS.md. D578 touches §6.6, but only for the display-format sentence. The D581 table (line 654) lists just the D578 change for §6.6. D566 (lines 315-340) widens §15.5 row 1 to 型検査か実行の前の変換, and its own reasoning says the old wording 「型検査」 does not strictly cover the KIR-writing limit (line 336). That same reasoning applies to §6.6's 「型検査に通らなかった入力」, and the plan never extends §6.6 to match. So an input that passes type checking and then hits N_nest during KIR writing, which keleut repl rejects (plan line 165 marks this 【実測】), fits none of the three categories in the LangSpec. IRSpec is the KIR spec and does not define REPL acceptance in LangSpec. The suggested fix is consistent with D566 and is small: extend the §6.6 bullet in M107 and add it to the D581 table and the M107/D577 entries.

## spec-3

- 観点: spec(仕様の文の整合)
- 重さ: medium
- 節: 4.4 D570 item 3 / 4.1 D567 (vs IRSpec 13.9 and 1 節)
- 反証: 2 人のうち 0 人

### 問題

kir::on_stack (crates/kir/src/lib.rs:103-128) runs both KIR writing and KIR reading/verification. D570 has this thread shrink its stack under a finite RLIMIT_AS and then 'lower the nesting limit to what fits'. If it still cannot be created, it exits 1. For a reader (`keleut run FILE.kir`), IRSpec 13.9 requires that '読み手は、深さが N_nest 以下のプログラムを受け付け' and that rejection be 65. IRSpec 1 guarantees that every reader gives the same exit status on the same KIR. Lowering the limit automatically under ulimit -v, and exiting 1 on a reader path, both break these rules. D567's three enumerated exclusions do not cover either case.

### 根拠

crates/kir/src/lib.rs:117 is the shared helper. Plan lines 457-459: 'KIR の処理のスレッドは、いまと同じく収まる深さまで入れ子の限度を下げる ... KIR の処理のスレッドは 1(D566)'. doc/IRSpec.md 13.9 (line ~1669) gives reader acceptance up to N_nest with exit 65.

### 直し方

Split D570 item 3 into the writer path (exit 1 per D566) and the reader path. For the reader path, either keep N_nest and report memory shortage, or add an IRSpec 1 / 13.9 exclusion: a reader whose stack is capped by RLIMIT_AS or --native-stack may reject KIR within N_nest. State the reader's exit status (65 or 3).

### 反証者 1: 反証できなかった

I could not refute the finding. Every point checks out against the repo.
(1) kir::on_stack (rustkeleut/crates/kir/src/lib.rs:103-128) is the one helper for the KIR thread, used for both reading and writing.
(2) Plan D570 item 3 (doc/log/261104-3-plan.md:457-459) makes that thread shrink its stack under a finite RLIMIT_AS. It also keeps lowering the nesting limit to what fits ("いまと同じく収まる深さまで入れ子の限度を下げる"), and the thread exits 1 per D566 if even 16 MiB cannot be created. It does not separate the reader path (`keleut run FILE.kir`) from the writer path.
(3) doc/IRSpec.md 13.9 (line 1670) says readers must accept depth up to N_nest and reject only beyond it, with exit 65. IRSpec section 1 (lines 34-50) promises the same exit status from every reader, apart from listed exclusions. None of those exclusions covers a reader that lowers N_nest or exits 1 because its stack is capped.
(4) D567's three new exclusions (lines 342-348) cover only Node/V8, LLVM and Rust allocation after the reserve is used up, and the OOM killer. D566 (lines 315-337) changes the LangSpec §15.5 row, which applies to source programs, not to IRSpec readers of a .kir file. A grep of the plan for 読み手 and 13.9 finds no other place that handles this.
So under ulimit -v the reader path would reject KIR within N_nest with 65, or exit 1, and both conflict with IRSpec 13.9 and section 1.
One caveat: the same gap already exists today, because `--native-stack` already lowers the limit and IRSpec has no exclusion for that. This does not refute the finding; it means the spec exclusion the finding suggests should cover `--native-stack` too. Splitting D570 item 3 into a writer path and a reader path, or adding the IRSpec exclusion, would be a sound change.

### 反証者 2: 反証できなかった

I confirmed the problem. kir::on_stack (rustkeleut/crates/kir/src/lib.rs:103-128) is the one shared thread helper for KIR writing and for reading and verification. In the plan, D570 item 3 (doc/log/261104-3-plan.md:457-459) makes this thread cap its stack under a finite RLIMIT_AS, keep 'lowering the nesting limit to what fits', and exit 1 (D566) when even 16 MiB cannot be created. It does not separate the reader path (`keleut run FILE.kir`) from the writer path. D566 only justifies exit 1 through LangSpec §15.5 row 1, which covers source programs; a .kir input falls under IRSpec. IRSpec 13.9 (doc/IRSpec.md:1670) requires a reader to accept every program of depth up to N_nest and to reject with 65. IRSpec 1 (lines 34-49) promises the same exit status on the same KIR, and diktor --run-ir would accept such a program. D567 (plan lines 342-348) narrows the memory exclusion to three cases: V8 or Node, LLVM or Rust allocation after the reserve is used up (134), and the OOM killer (137). None of them covers a reader that lowers N_nest because of RLIMIT_AS, or a reader that exits 1 because the KIR thread cannot be created. The words 'いまと同じく' also rest on the existing --native-stack lowering, which IRSpec does not list as an exclusion either. The finding is valid: D570 item 3 needs a reader-path split or an IRSpec 1 / 13.9 exclusion that states the reader's exit status.

## spec-4

- 観点: spec(仕様の文の整合)
- 重さ: medium
- 節: 4.1 D567 (rewrite of IRSpec 1 節 bullet at doc/IRSpec.md:42-43)
- 反証: 2 人のうち 0 人

### 問題

D567 rewrites the bullet 'N_alloc_max の検査によらないメモリの不足' so that it lists three specific cases and keeps only 'Diktor での確保'. The current heading excludes every non-N_alloc_max memory shortage. The ordinary case, a RustKeleut reader running out of memory (exit 3) where another reader succeeds, is in none of the three cases. The plan itself creates such a case in shape.t (V240: ki gives 3, diktor --run-ir gives 0 on the same .kir). With an enumerated list, the IRSpec 1 guarantee would then claim that these must agree.

### 根拠

Plan lines 342-349 (the three cases plus Diktor) and lines 434-436 (shape.t: Diktor 0 under the same limit, registered as a known diff).

### 直し方

Keep the general clause: readers may run out of memory at different points, and a reader that does so ends with 3 through the normal OutOfMemory path. Then list the three cases as the ones where the exit status is not 3. Avoid kconform pair names (`ki`, `kj`) in IRSpec; it never uses them. Write 'インタプリタと LLVM の後段' instead.

### 反証者 1: 反証できなかった

I confirmed the problem. The current IRSpec bullet at doc/IRSpec.md:42-43 has a general heading, "`N_alloc_max` の検査によらないメモリの不足", and gives WasmGC/V8 and Diktor only as examples ("これにあたる"). D567 at plan lines 342-349 rewrites that bullet "次の 3 つの場合を挙げる形に改める": (1) Node/V8 with exit 70, (2) LLVM or Rust allocation after the reserve with exit 134, (3) the OOM killer with exit 137. It keeps only "Diktor での確保". Nothing in the plan (lines 342-356, 746, 817) keeps the general clause.

The plain case is a RustKeleut reader hitting the heap limit and exiting 3 through the ordinary OutOfMemory path while another reader succeeds. None of the three enumerated cases covers it. The kept Diktor item only partly covers it: it speaks of Diktor's own allocation, not RustKeleut running out first. RustKeleut back ends running out at different points from each other (for example the interpreter against WasmGC) is not covered at all. The plan's own shape.t test (lines 425-436, V240) is exactly this kind of case: the interpreter exits 3 while Diktor exits 0 on the same .kir under the same limit, and the plan registers it as a known diff. The enumerated wording would therefore make the IRSpec 1 guarantee claim a match that the plan itself breaks.

The suggested change is sound. Keep the general clause: readers may run out of memory at different points and end with 3 through the OutOfMemory path. Then list the three cases as the ones whose exit status is not 3. The point about pair names is also valid: grep finds no `ki`, `kj` or `kw` in doc/IRSpec.md, so the IRSpec wording should say "インタプリタと LLVM の後段" instead.

### 反証者 2: 反証できなかった

I confirmed the finding. Today the bullet at doc/IRSpec.md:42-43 opens with a general heading, "`N_alloc_max` の検査によらないメモリの不足", and then gives the kw/V8 case and Diktor's allocation as examples. Plan D567 (lines 342-349) recasts it as "次の 3 つの場合を挙げる形" (exit 70, 134 and 137) and keeps only the Diktor item. All three cases are ones where the exit status is not 3. None of them covers a reader that runs out of heap through the normal OutOfMemory path (exit 3) at a different point than another reader. The plan's own shape.t (lines 425-436, V240) is such a case: ki/RustKeleut gives 3 and Diktor gives 0 on the same .kir. Recording it in known-diffs.toml does not change what IRSpec 1 guarantees. RustKeleut back ends also use different amounts of memory, so they too can run out at different points. The "Diktor での確保" item alone does not clearly cover RustKeleut failing first. The plan nowhere says the general heading is kept, and its 3 cases are written as the full list. A grep also shows that IRSpec never uses pair names such as `ki`, `kj` or `kw`, so the wording in D567's case 2 would be new to IRSpec. The suggested fix is sound: keep the general clause (readers may run out of memory at different points and end with 3), then list the three cases where the exit status is not 3.

## spec-5

- 観点: spec(仕様の文の整合)
- 重さ: medium
- 節: 6 D581 table (AGENTS.md row)
- 反証: 2 人のうち 0 人

### 問題

Several AGENTS.md sentences become false after the milestones, but the D581 table does not assign their updates. (a) AGENTS.md line 38 says 'only allocations over `N_alloc_max` become `ERROR(OutOfMemory)` with exit 3', and after M109 the kw shape-table failure also exits 3 (D569). (b) The `keleut run` text describes RLIMIT_AS capping only for the front-end thread ('under a finite `RLIMIT_AS` ... cap that stack at a quarter'), and M110 extends it to the KIR and execution threads and adds front-end OOM exiting 1 (D570). Section 4.4 says to write the message in AGENTS.md, but the table lists neither D569 nor D570 nor M109/M110 for AGENTS.md. (c) V238 changes `node DIR/main.mjs` behaviour (D575), which is described in the `keleut build` item.

### 根拠

`grep -o` on AGENTS.md shows both sentences. The D581 AGENTS row lists D564, D565, D572, D577, D568, D573, D574, D576 and M106〜M113, but not D569, D570 or D575.

### 直し方

Add D569 (M109: revise the N_alloc_max-only sentence), D570 (M110: RLIMIT_AS handling for the KIR and execution threads, front-end OOM exit 1 and its message) and D575 (M112: closed stdout in `node DIR/main.mjs`) to the AGENTS.md row. Add each to the matching milestone's deliverables in 7.2.

### 反証者 1: 反証できなかった

I checked the finding against the plan and AGENTS.md and confirmed it, so it is not refuted.

(a) AGENTS.md line 38 says "only allocations over `N_alloc_max` become `ERROR(OutOfMemory)`" with exit 3. D569 (plan 4.3, M109) makes `kw` turn shape-table growth failure into OutOfMemory with exit 3, so after M109 that sentence is false. Neither the D581 AGENTS.md row (D564, D565, D572, D577, D568, D573, D574, D576) nor the M109 row in 7.2 / 7.3 schedules an AGENTS.md edit. D572 covers only the finite `ulimit -v` / V215 wording, not this sentence.

(b) AGENTS.md describes the RLIMIT_AS stack capping only for the front-end thread ("under a finite `RLIMIT_AS` ... cap that stack at a quarter of the limit ..."). D570 item 3 extends it to the KIR and execution threads, and item 2 adds the front-end limit line with exit 1. Plan 4.4 (line 456) says the limit-line wording goes into AGENTS.md, but the D581 table omits D570. The M110 deliverables and its commit list (7.3) also have no AGENTS.md edit. So this is only partly handled: the section text has it, but no table row or milestone schedules it.

(c) D575 (V238, closed stdout in `node DIR/main.mjs`, exit 74) is weaker. The current `keleut build` item does not describe stdout handling. Still, D575 adds a behaviour, the M112 row already touches AGENTS.md, and the D581 row lists only D573/D574 for M112.

Adding D569/M109, D570/M110 and D575/M112 to the D581 AGENTS.md row and to the 7.2 deliverables is correct and does not conflict with the plan.

### 反証者 2: 反証できなかった

The finding holds. Parts (a) and (b) are clearly right, and (c) is weaker. AGENTS.md says "only allocations over `N_alloc_max` become `ERROR(OutOfMemory)` with exit 3". Once D569/M109 is done, a `kw` shape-table failure also exits 3 (plan 4.3), so that sentence becomes false. AGENTS.md also describes the `RLIMIT_AS` stack capping only for the front-end thread ("under a finite `RLIMIT_AS` ... `keleut run` and `keleut emit-kir` cap that stack at a quarter..."). D570/M110 extends the capping to the KIR-processing and execution threads and adds a front-end out-of-memory exit 1. Plan 4.4 item 2 even says to write the message "in `AGENTS.md` and the record". Even so, the D581 table's AGENTS.md row (line 658) names only D564, D565, D572, D577, D568, D573, D574 and D576. The README row lists D569, but that file is not AGENTS.md. In 7.2, the M109 and M110 rows (lines 699-700) have no AGENTS.md deliverable, and neither do the M109/M110 commit splits in 7.3. Part (c) is weaker. The `keleut build` item ("`node DIR/main.mjs` runs the program on its own") says nothing about stdout handling, so D575 may not strictly need an AGENTS.md change. The M112 row already mentions "README と AGENTS.md", but that refers to the directory listing. The gap is real for D569 and D570. The fix is to add D569/M109 and D570/M110, and optionally D575, to the AGENTS.md row and to the matching 7.2 deliverables.

## spec-6

- 観点: spec(仕様の文の整合)
- 重さ: medium
- 節: Header 前提/判断事項 vs 11 所有者への問い (P257)
- 反証: 2 人のうち 0 人

### 問題

The plan contradicts itself on whether the diktor changes wait for the owner. The header says the diktor changes are made 'on the premise of owner approval (P257)'. Section 11 says 'if there is no answer, proceed as the plan decides'. A reader cannot tell whether M113 may modify diktor/ without an explicit yes. AGENTS.md treats diktor as the oracle, and the earlier plans kept diktor out of scope.

### 根拠

Plan line 12: 'diktor を変えるのは本計画 5.2 節の 2 件だけで、所有者の承認(P257)を前提にする'. Line 810: '答えが無ければ本計画の決定のとおりに進める'. Line 577: '所有者の承認を問う(P257)'.

### 直し方

State one rule: either M113 waits for an explicit answer to P257 (and P258), or say in section 11 that P257 is the exception to the default-to-plan rule.

### 反証者 1: 反証できなかった

I confirmed the finding in the plan text. Line 12 says the diktor changes are made only with the owner's approval as a precondition: 「diktor を変えるのは本計画 5.2 節の 2 件だけで、所有者の承認(P257)を前提にする」. Line 577 says 「所有者の承認を問う(P257)」. But line 810 sets a general default, 「答えが無ければ本計画の決定のとおりに進める」. The P257 row's 本計画の決定 is 「入れる」, so with no answer M113 would go ahead and change diktor. The header's 判断事項 line ("the plan is written with the recommended answers and revised if the answer differs") supports reading silence as consent. 「承認を前提にする」 most naturally means approval is required first, though. No section makes P257 an exception to the default, so whether M113 may change diktor/ without an explicit yes stays ambiguous. The M113 row at line 703 lists the diktor commits with no gating condition. Earlier plans (261024-1, 261104-1) kept diktor out of scope, and AGENTS.md treats Diktor as the oracle, so this decision should not be left ambiguous. The suggested fix is a one-line clarification and would not be wrong: either M113 waits for an explicit answer to P257/P258, or section 11 says P257 is the exception.

### 反証者 2: 反証できなかった

I confirmed the problem by reading the plan. Line 12 says diktor is changed only for the two items in 5.2, and "所有者の承認(P257)を前提にする". Line 577 (D576) says that because the earlier plans kept diktor out of scope, "所有者の承認を問う(P257)". Line 810 sets the general rule for section 11: "答えが無ければ本計画の決定のとおりに進める". The P257 row (line 814) gives the plan's decision as "入れる". The header's 判断事項 (lines 36-37) says only that the plan is written with the recommended options and is revised if an answer differs. Neither section 11 nor 進め方の原則 (around line 682, which only says push waits for the owner) makes P257 an exception, or says M113 (lines 703 and 730, which commit inside diktor) waits for an explicit answer. "承認を前提にする" can be read either as "conditional on approval" or as "assuming approval". Read together with line 810, it leaves open whether M113 may modify diktor/ without an explicit yes, and this matters because diktor is the oracle that the earlier plans excluded. The suggested fix is sound: state one rule, either that M113 waits for P257 (and P258) or that P257 is an explicit exception to the default-to-plan rule.

## spec-7

- 観点: spec(仕様の文の整合)
- 重さ: medium
- 節: 4.4 D570 item 1 / 7.2 依存 (lines 454, 712-713)
- 反証: 2 人のうち 0 人

### 問題

The text says `drop` must come before the tests of group (2) because it moves the ulimit -v boundaries. But `drop` sits in M110, after M108 and M109, which run the (2) ulimit -v checks (sweep_as.sh, shape.t). The stated ordering does not match the milestone order. In fact both of those checks run .kir inputs, which `drop` does not affect, so the stated reason does not hold either.

### 根拠

Line 454: '(2) の試験より先に行う'. Line 713: 'ulimit -v の下の試験の境を動かすので、(2) の試験の前に行う'. M108 and M109 come before M110 in the 7.2 table. sweep_as.sh uses oomlist.kir (line 407), and shape.t runs chain.kir (line 428).

### 直し方

Either move the drop commit to the start of M108, or reword: drop is done in M110 before M110's own ulimit -v tests of `keleut run FILE.kel`, and M108/M109 are unaffected because they run .kir.

### 反証者 1: 反証できなかった

I checked the plan text, and the problem is real as worded. Line 454 says the drop change runs "(2) の試験より先に", and line 713 says "(2) の試験の前に行う". But the 7.2 table puts drop in M110 (line 700), after M108 and M109 (lines 698-699), which already run group (2)'s ulimit -v checks. Both of those checks run .kir inputs, which the type-check context drop does not touch. sweep_as.sh runs oomlist.kir under ulimit (line 407). shape.t runs emit-kir with no limit and then runs chain.kir under ulimit -v 524288, and line 428 says plainly not to run the .kel under ulimit. So the stated order does not match the milestones, and the stated reason does not apply to M108 or M109. The placement can still be justified. Section 0 item 6 (line 71) says only "ここで行う", meaning drop goes into (2)'s M110 rather than (3)'s M112. The tests that drop does affect run `keleut run` on .kel under ulimit -v: M110's own run_cmd.rs tests (line 465) and M111's chunks.kel test (line 497). So the fix is the finding's second suggestion: reword lines 454 and 713 to say drop comes before the .kel-input ulimit -v tests of M110 and M111, and that M108 and M109 are unaffected because they run .kir. Moving drop to M108 is not needed.

### 反証者 2: 反証できなかった

I checked the plan, and the problem is real. Lines 454 and 712-713 say the `drop` commit comes before the tests of group (2): 「(2) の試験より先に行う」 and 「(2) の試験の前に行う」. But `drop` is in M110, and the 7.2 table puts M108 (`sweep_as.sh`) and M109 (`shape.t`) before it. M110 depends only on M108, while M108 and M109 are both (2) items that run ulimit -v checks. Both of those checks run .kir inputs: `oomlist.kir` (line 407) and `chain.kir` (line 428, which says the .kel is not run under the ulimit). `drop` only affects `keleut run FILE.kel` and `emit-kir` (line 451), so it does not move the boundaries M108 and M109 measure. The reason the plan gives does hold for the tests that come later. M110's own run_cmd.rs tests run a .kel under ulimit -v (line 465), and M111 runs `chunks.kel` under ulimit -v (line 497). So the suggested rewording is correct, with one addition: `drop` comes before the ulimit -v tests of .kel programs in M110 and M111, and M108/M109 are unaffected because they run .kir. The same wording should also be fixed at line 71 (「ここで行う」).

## spec-8

- 観点: spec(仕様の文の整合)
- 重さ: low
- 節: 5.2 D577 (placement in IRSpec 1 節)
- 反証: 2 人のうち 2 人

### 問題

The new sentence '対話的な実行の入力も同じで、…受理しない' is to go 'after lines 52-54'. Line 54 is the sentence saying the Diktor tree evaluator 'そのまま実行しうる'. Placed right after it, '同じで' reads as if it referred to direct execution, which inverts the meaning.

### 根拠

doc/IRSpec.md:52-54: line 52 is 'ノードの深さが `N_nest` を超える…KIR を持たない', line 53 is '書き手は…処理系の限度で終える', and line 54 is '同じプログラムをソースから直接に実行する処理系(Diktor の木の評価器)は、そのまま実行しうる'.

### 直し方

Insert the sentence after line 53, before the Diktor sentence. Or name the referent explicitly: '書き手が KIR を書かない場合と同じで'.

### 反証者 1: 反証した

Plan lines 579-584 say the D577 sentence goes after IRSpec lines 52-54. I checked those lines in doc/IRSpec.md and they match what the finding quotes. The finding says this placement inverts the meaning; it does not. The new sentence names its subject and verdict itself: "KIR を通して実行する処理系は…処理系の限度として受理しない". So "同じで" cannot be read as "the input runs as it is". Placed after line 54, "同じで" most naturally takes in the whole paragraph, and that reading is correct. A program nested past N_nest has no KIR (line 52). A KIR writer stops at its limit (line 53). The Diktor direct evaluator may still run it (line 54). V224 is exactly this split for interactive input: keleut repl rejects, while diktor --repl accepts. Moving the sentence before line 54, as the finding suggests, would make "同じ" cover only the KIR-writer half. The sentence would then sit in front of the Diktor sentence, which is about non-interactive direct execution, and that order is no clearer. At most this is a stylistic choice of wording, not a defect.

### 反証者 2: 反証した

I checked doc/IRSpec.md lines 52-54 and plan section 5.2 (D577). The finding says the meaning is inverted, and that is false. Lines 52-54 form one paragraph with three rules: a program deeper than N_nest has no KIR, a writer does not write KIR for it and stops at an implementation limit, and a direct source executor (Diktor's tree evaluator) may run it. The new sentence comes after that paragraph and says "対話的な実行の入力も同じで" ("interactive input is the same"). Read as covering the whole paragraph, it is correct for interactive input too. diktor --repl uses the tree evaluator and accepts these inputs (that is the V224 observation), so the Diktor sentence holds for the REPL as well. The second half of the new sentence also names its own subject outright: "KIR を通して実行する処理系は…受理しない" (an implementation that executes through KIR does not accept the input). So it cannot be read as saying interactive input is executed directly. The suggested fix is also worse than the current placement. Putting the sentence after line 53 would split it from the Diktor sentence and leave unclear whether Diktor's REPL may accept the input, which it does. The placement after line 54 is the better one. At most a reader might find "同じで" slightly vague, which is a style point and not the inversion the finding claims.

## spec-9

- 観点: spec(仕様の文の整合)
- 重さ: low
- 節: 5.1 D573
- 反証: 2 人のうち 0 人

### 問題

D573 says to replace D533 as a whole with 'the builtins Array.each and pinned are LIR functions made by keleut-lir'. D533 (plan 261024-1 4.9) also covers removing `run`, the extern "prim"/"C" globals and their error text, and the `with_runtime` wiring in rt.kirt. Read literally, the replacement drops those decisions, and with_runtime is not mentioned at all.

### 根拠

doc/log/261024-1-plan.md:424-441 (section 4.9 'リージョン、extern、組み込み(D533)') and :1379.

### 直し方

Write 'D533 のうち rt.kirt に関する部分(Array.each、pinned、with_runtime の結線)を…に置き換える'. Also say where with_runtime is now implemented.

### 反証者 1: 反証できなかった

Partly refuted, but the core gap is real. The finding's first claim, that D573 drops the `run` removal and the extern "prim"/"C" decisions, is false. The plan's D573 (doc/log/261104-3-plan.md:523) limits what it replaces with the parenthetical "(組み込みを `rt.kirt` に KIR で書く)". The plan 261024-1 ledger entry for D533 (261024-1-plan.md:1379) says only "利用者の関数を呼ぶ組み込みは `rt.kirt` に KIR で書く", so the `run` and extern bullets of section 4.9 are not part of what D573 replaces. The `with_runtime` omission is confirmed, though. Section 4.9 (261024-1-plan.md, the bullet that begins "RustKeleut では…") puts `Array.each`, `pinned` and the `with_runtime` wiring together in `rt.kirt`. D573 drops `rt.kirt` and names a replacement only for `Array.each` and `pinned`, which leaves `with_runtime` with no stated home. `grep with_runtime doc/log/261104-3-plan.md` finds nothing. The record already settles where it lives: the record at 261024-3-record.md:2634-2636 (M89) says the wiring is done by lowering the `runtime`-marked `Console.write` to `console_write` (M86), and 2085-2089 had left that choice to M89. D573 should cite that, for example: "D533 のうち `rt.kirt` に関する部分(`Array.each`、`pinned`、`with_runtime` の結線)を置き換える。`with_runtime` の結線は M86 の `console_write` への下ろしで足りている(記録 2634〜2636 行)". Severity stays low.

### 反証者 2: 反証できなかった

The finding is partly right, so I am not refuting it, but its scope should be narrower. The wrong part: D573 does not replace "D533 as a whole" in a way that drops `run` or the extern globals. D573 quotes D533 as "組み込みを rt.kirt に KIR で書く", which is exactly D533's ledger entry at doc/log/261024-1-plan.md:1379 ("利用者の関数を呼ぶ組み込みは rt.kirt に KIR で書く"). That ledger statement says nothing about removing `run` or about extern "prim"/"C", which are separate bullets that only share section 4.9's "(D533)" heading. The right part: the 4.9 bullet the replacement targets (line 436) puts both "Array.each、pinned" and "with_runtime の結線" into rt.kirt. D573's replacement text (261104-3-plan.md:523) names only Array.each and pinned and never mentions with_runtime (grep finds no with_runtime anywhere in the new plan). The record settles it at 261024-3-record.md:2636: the wiring is handled by lowering `Console.write` marked `runtime` to console_write (M86). The plan leaves this unsaid, so a reader cannot tell where that part of D533 went. Suggested fix: keep D573's scoping, add one clause saying the with_runtime wiring is handled by the M86 lowering of `Console.write` (record line 2636), and optionally say that the `run` and extern parts of 4.9 are unaffected.
