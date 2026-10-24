// Measures the bytes per frame of non-tail recursion in Wasm with 0, 64, 256 and 1024
// live locals (i64 and eqref), the bytes per frame as a function of the parameter count
// with the KIR signature (closure, N eqref arguments, frame), the usable upper limit of `node --stack-size` (with the
// soft RLIMIT_STACK raised by `ulimit -s` in a child shell), and the usable upper limit
// of Worker resourceLimits.stackSizeMb. Then it decides B_wasm and checks
// S = (L+R) x B_wasm + 64 MiB (plan 261024-1, section 6.10).
//
// The Wasm modules are generated as bytes by wasmgen.mjs; nothing has to be built first.
// Usage: node bigframe.mjs            (full run, about 1-2 minutes, touches up to 16 GiB)
//        node bigframe.mjs --quick    (frame tables only)
// Internal modes: --child <variant>, --worker <stackSizeMb> <variant>.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { Worker, isMainThread, workerData, parentPort } from "node:worker_threads";
import { I, T, module, funcType, arrayType, refNull } from "./wasmgen.mjs";

const SELF = fileURLToPath(import.meta.url);
const KS = [0, 64, 256, 1024];
// (N, K) for the signature variants: N user arguments (P = N + 2 parameters) and K locals.
const SIG = [[0, 0], [1, 0], [2, 0], [4, 0], [8, 0], [16, 0], [32, 0], [64, 0], [0, 64], [8, 64], [8, 166], [10, 160], [16, 150], [0, 182]];
// Frame estimate: 8 bytes per local, CP bytes per parameter and C bytes of constant.
const C = 64, CP = 24;
const estimate = (p, k) => 8 * k + CP * p + C;

// One module per variant. The function `f` recurses without a base case; each level
// increments the mutable global $count, loads K values (i64.load from linear memory, or
// array.get from a global GC array of eqref) into K locals before the call and uses all
// of them after the call, so all K values are live across the call in every tier.
// The loads cannot move past the call, which may write memory or the array.
function build(kind, k) {
  const arr = 0, fty = 1, initTy = 2, getTy = 3;
  const types = [arrayType(T.eqref, true), funcType([T.i64], [kind === "i64" ? T.i64 : T.i32]), funcType([], []), funcType([], [T.i64])];
  const gCount = 0, gArr = 1;
  const body = [...I.globalGet(gCount), ...I.i64Const(1), ...I.i64Add, ...I.globalSet(gCount)];
  if (kind === "i64") {
    for (let i = 0; i < k; i++) body.push(...I.i32Const(0), ...I.i64Load(8 * i), ...I.localSet(1 + i));
    body.push(...I.localGet(0), ...I.call(0));
    for (let i = 0; i < k; i++) body.push(...I.localGet(1 + i), ...I.i64Add);
  } else {
    for (let i = 0; i < k; i++) body.push(...I.globalGet(gArr), ...I.i32Const(i), ...I.arrayGet(arr), ...I.localSet(1 + i));
    body.push(...I.localGet(0), ...I.call(0));
    for (let i = 0; i < k; i++) body.push(...I.localGet(1 + i), ...I.refCastI31, ...I.i31GetS, ...I.i32Add);
  }
  const init = [...I.i32Const(1), ...I.refI31, ...I.i32Const(1024), ...I.arrayNew(arr), ...I.globalSet(gArr)];
  return module({
    types,
    memoryPages: 1,
    globals: [
      { type: [T.i64], mut: true, init: I.i64Const(0) },
      { type: refNull(arr), mut: true, init: [0xd0, 0x00] },
    ],
    funcs: [
      { type: fty, locals: k > 0 ? [[k, kind === "i64" ? T.i64 : T.eqref]] : [], body, export: "f" },
      { type: initTy, locals: [], body: init, export: "init" },
      { type: getTy, locals: [], body: I.globalGet(gCount), export: "count" },
    ],
  });
}

// The signature variant: f has the shape of a KIR function on the WasmGC back end,
// (closure, N eqref arguments, frame) with P = N + 2 eqref parameters. Each level loads
// K values from the global GC array into K locals, passes all P parameters on to the
// non-tail self call, and uses all parameters and locals after the call.
function buildSig(n, k) {
  const p = n + 2, arr = 0;
  const types = [arrayType(T.eqref, true), funcType(Array(p).fill(T.eqref), [T.i32]), funcType([], []), funcType([], [T.i64]), funcType([], [T.i32])];
  const body = [...I.globalGet(0), ...I.i64Const(1), ...I.i64Add, ...I.globalSet(0)];
  for (let i = 0; i < k; i++) body.push(...I.globalGet(1), ...I.i32Const(i), ...I.arrayGet(arr), ...I.localSet(p + i));
  for (let i = 0; i < p; i++) body.push(...I.localGet(i));
  body.push(...I.call(0));
  for (let i = 0; i < p + k; i++) body.push(...I.localGet(i), ...I.refCastI31, ...I.i31GetS, ...I.i32Add);
  const init = [...I.i32Const(1), ...I.refI31, ...I.i32Const(1024), ...I.arrayNew(arr), ...I.globalSet(1)];
  const go = [];
  for (let i = 0; i < p; i++) go.push(...I.i32Const(1), ...I.refI31);
  go.push(...I.call(0));
  return module({
    types,
    globals: [
      { type: [T.i64], mut: true, init: I.i64Const(0) },
      { type: refNull(arr), mut: true, init: [0xd0, 0x00] },
    ],
    funcs: [
      { type: 1, locals: k > 0 ? [[k, T.eqref]] : [], body },
      { type: 2, locals: [], body: init, export: "init" },
      { type: 3, locals: [], body: I.globalGet(0), export: "count" },
      { type: 4, locals: [], body: go, export: "f" },
    ],
  });
}

// Runs f until the stack overflows and returns the number of frames reached.
// Variants are "i64:K", "eqref:K" and "sig:N:K".
function probe(variant) {
  const [kind, x, y] = variant.split(":").map((s, i) => (i === 0 ? s : Number(s)));
  const sig = kind === "sig";
  const { exports } = new WebAssembly.Instance(new WebAssembly.Module(sig ? buildSig(x, y) : build(kind, x)));
  exports.init();
  try {
    if (sig) exports.f();
    else exports.f(0n);
    throw new Error("unreachable: f returned");
  } catch (e) {
    if (!(e instanceof RangeError)) throw e;
  }
  return Number(exports.count());
}

// Runs `node <flags> bigframe.mjs <args>` under `ulimit -s <ulimitKb>` (if given).
function run(ulimitKb, flags, args) {
  const cmd = ulimitKb ? ["/bin/sh", ["-c", 'ulimit -s "$1" || exit 99; shift; exec "$@"', "sh", String(ulimitKb), process.execPath, ...flags, SELF, ...args]] : [process.execPath, [...flags, SELF, ...args]];
  const r = spawnSync(cmd[0], cmd[1], { encoding: "utf8", maxBuffer: 1 << 24 });
  const last = r.stdout.trim().split("\n").pop() || "";
  let depth = null, error = null;
  try {
    const j = JSON.parse(last);
    depth = j.depth ?? null;
    error = j.error ?? null;
  } catch {}
  return { status: r.status, signal: r.signal, depth, error, stderr: r.stderr.trim().split("\n").slice(-2).join(" | ") };
}

const outcome = (r) => (r.depth != null ? `depth ${r.depth}` : r.signal ? `killed by ${r.signal}` : `exit ${r.status}${r.error ? " " + r.error : ""}${r.stderr ? " (" + r.stderr + ")" : ""}`);

async function main() {
  const quick = process.argv.includes("--quick");
  const hard = spawnSync("/bin/sh", ["-c", "ulimit -Hs"], { encoding: "utf8" }).stdout.trim();
  const soft = spawnSync("/bin/sh", ["-c", "ulimit -s"], { encoding: "utf8" }).stdout.trim();
  console.log(`node ${process.version}  v8 ${process.versions.v8}`);
  console.log(`RLIMIT_STACK: soft ${soft} KiB, hard ${hard}`);

  // 1. Bytes per frame: depth at two --stack-size values; the slope cancels the fixed
  //    usage below the first Wasm frame.
  const s1 = 65536, s2 = 262144; // KiB
  const tiers = [["default", []], ["liftoff", ["--liftoff-only"]], ["turbofan", ["--no-liftoff"]], ["tf-noinl", ["--no-liftoff", "--no-wasm-inlining"]]];
  const table = {};
  console.log(`\n# bytes per frame of non-tail self recursion (--stack-size ${s1} vs ${s2} KiB)`);
  console.log("tier      kind  locals  depth@64MiB  depth@256MiB  bytes/frame");
  for (const [tier, flags] of tiers) {
    for (const kind of ["i64", "eqref"]) {
      for (const k of KS) {
        const v = `${kind}:${k}`;
        const a = run(s2 + 65536, [`--stack-size=${s1}`, ...flags], ["--child", v]);
        const b = run(s2 + 65536, [`--stack-size=${s2}`, ...flags], ["--child", v]);
        if (a.depth == null || b.depth == null) {
          console.log(`${tier.padEnd(9)} ${kind.padEnd(5)} ${String(k).padStart(6)}  failed: ${outcome(a)} / ${outcome(b)}`);
          continue;
        }
        const bpf = ((s2 - s1) * 1024) / (b.depth - a.depth);
        table[`${tier}:${v}`] = bpf;
        console.log(`${tier.padEnd(9)} ${kind.padEnd(5)} ${String(k).padStart(6)}  ${String(a.depth).padStart(11)}  ${String(b.depth).padStart(12)}  ${bpf.toFixed(1).padStart(11)}`);
      }
    }
  }

  // Linear fit bytes = a*K + c per tier and kind (from K = 0 and K = 1024).
  console.log("\n# linear fit bytes/frame = a*K + c (from K = 0 and K = 1024)");
  let worstA = 0, worstC = 0;
  for (const [tier] of tiers) {
    for (const kind of ["i64", "eqref"]) {
      const b0 = table[`${tier}:${kind}:0`], b1 = table[`${tier}:${kind}:1024`];
      if (b0 == null || b1 == null) continue;
      const a = (b1 - b0) / 1024;
      console.log(`${tier.padEnd(9)} ${kind.padEnd(5)} a = ${a.toFixed(3)} bytes/local, c = ${b0.toFixed(1)} bytes`);
      worstA = Math.max(worstA, a);
      worstC = Math.max(worstC, b0);
    }
  }

  // 1b. Bytes per frame with the KIR signature, as a function of the parameter count P
  //     and the local count K, against the estimate 8 K + CP P + C.
  console.log(`\n# bytes per frame with the signature (closure, N eqref, frame), P = N + 2; estimate = 8 K + ${CP} P + ${C}`);
  console.log("tier         N    P     K  bytes/frame  estimate  <= estimate");
  let sigOk = true;
  for (const [tier, flags] of tiers) {
    if (tier === "turbofan") continue;
    for (const [n, k] of SIG) {
      const v = `sig:${n}:${k}`;
      const a = run(s2 + 65536, [`--stack-size=${s1}`, ...flags], ["--child", v]);
      const b = run(s2 + 65536, [`--stack-size=${s2}`, ...flags], ["--child", v]);
      if (a.depth == null || b.depth == null) {
        console.log(`${tier.padEnd(9)} ${String(n).padStart(4)}  failed: ${outcome(a)} / ${outcome(b)}`);
        sigOk = false;
        continue;
      }
      const bpf = ((s2 - s1) * 1024) / (b.depth - a.depth);
      const est = estimate(n + 2, k);
      if (bpf > est) sigOk = false;
      console.log(`${tier.padEnd(9)} ${String(n).padStart(4)} ${String(n + 2).padStart(4)} ${String(k).padStart(5)}  ${bpf.toFixed(1).padStart(11)}  ${String(est).padStart(8)}  ${bpf <= est ? "yes" : "NO"}`);
    }
  }
  console.log(`estimate covers every measured signature frame: ${sigOk}`);
  if (quick) return;

  // 2. Upper limit of --stack-size: the run must reach depth close to S / bytes-per-frame.
  const bpf0 = table["default:i64:0"];
  const sizes = [1048576, 1500000, 2000000, 2097151, 2097152, 2200000, 3145728, 4194303, 4194304, 4194305, 6291456, 8388608];
  console.log("\n# upper limit of --stack-size (ulimit -s = size + 65536 KiB, variant i64:0, default tier)");
  console.log("--stack-size KiB  outcome                           depth x bytes/frame / size");
  let maxOk = 0;
  for (const kb of sizes) {
    const r = run(kb + 65536, [`--stack-size=${kb}`], ["--child", "i64:0"]);
    const ratio = r.depth != null ? (r.depth * bpf0) / (kb * 1024) : 0;
    const ok = r.depth != null && ratio > 0.9 && ratio < 1.1;
    if (ok) maxOk = Math.max(maxOk, kb);
    console.log(`${String(kb).padStart(16)}  ${outcome(r).padEnd(32)}  ${ratio.toFixed(3)} ${ok ? "ok" : "NOT OK"}`);
  }

  // 3. Upper limit of Worker resourceLimits.stackSizeMb (not bounded by RLIMIT_STACK).
  console.log("\n# Worker resourceLimits.stackSizeMb (variant i64:0, default tier, no ulimit change)");
  console.log("stackSizeMb  outcome                           depth x bytes/frame / size");
  let maxWorkerOk = 0;
  for (const mb of [1024, 2047, 2048, 3072, 4095, 4096, 8192, 16384]) {
    const r = run(null, [], ["--worker", String(mb), "i64:0"]);
    const ratio = r.depth != null ? (r.depth * bpf0) / (mb * 1048576) : 0;
    const ok = r.depth != null && ratio > 0.9 && ratio < 1.1;
    if (ok) maxWorkerOk = Math.max(maxWorkerOk, mb);
    console.log(`${String(mb).padStart(11)}  ${outcome(r).padEnd(32)}  ${ratio.toFixed(3)} ${ok ? "ok" : "NOT OK"}`);
  }

  // 4. B_wasm and S (plan 6.10). The worst tier is Liftoff (also what the default tier
  //    runs here, since a recursion that never returns never tiers up): a = 8 bytes per
  //    local and c = 64 bytes with one i64 parameter. The TurboFan rows with inlining are
  //    not per-frame costs (several levels share one frame, and load elimination merges
  //    the eqref loads of the inlined levels), so they are not used for the budget.
  //    A parameter that is passed on to the call costs 8 bytes while it fits in a
  //    register and 24 bytes beyond that (table 1b), so the estimate is
  //    8 K + 24 P + 64 bytes, which covers every row of table 1b. The big-frame form
  //    moves locals, not parameters, so 24 P + 64 + 8 (the struct reference) must stay
  //    within B_wasm on its own; that bounds the parameter count.
  //    The plan's check passes --stack-size = 0.9 S with S = (L+R) x B + 64 MiB. That
  //    leaves less than (L+R) x B for V8 whenever (L+R) x B > 576 MiB, so this script
  //    also computes the absolute margin: --stack-size = (L+R) x B + 32 MiB (JS frames
  //    below the first Wasm frame and estimate error) and S = --stack-size + 64 MiB
  //    (native frames beyond V8's limit).
  const L = 2 ** 20, R = Math.max(1024, L / 64);
  console.log(`\n# B_wasm and S (L = ${L}, R = ${R}; worst tier a = ${worstA.toFixed(3)} bytes/local, c = ${worstC.toFixed(1)} bytes)`);
  console.log(`usable --stack-size: <= ${maxOk} KiB; usable Worker stackSizeMb: >= ${maxWorkerOk} (largest probed)`);
  console.log("        | plan: S = (L+R)B + 64 MiB, --stack-size = 0.9 S      | absolute margin: --stack-size = (L+R)B + 32 MiB, S = +64 MiB");
  console.log("B_wasm  | K max (P = 2)  S KiB    0.9 S KiB  <= limit  >= (L+R)B | --stack-size KiB  S KiB    <= limit  Worker stackSizeMb");
  const plan = (B) => {
    const S = ((L + R) * B) / 1024 + 65536;
    return { S, ss: Math.ceil(0.9 * S) };
  };
  const fixed = (B) => {
    const ss = ((L + R) * B) / 1024 + 32768;
    return { ss, S: ss + 65536, mb: Math.ceil((ss + 65536) / 1024) };
  };
  let chosen = null;
  for (const B of [4096, 2048, 1536, 1024]) {
    const p = plan(B), f = fixed(B);
    const kmax = Math.floor((B - C - 2 * CP) / 8);
    const fits = f.ss <= maxOk;
    if (fits && B <= 2048 && chosen == null) chosen = B;
    console.log(`${String(B).padStart(6)}  | ${String(kmax).padStart(13)}  ${String(p.S).padStart(7)}  ${String(p.ss).padStart(9)}  ${String(p.ss <= maxOk).padStart(8)}  ${String(p.ss * 1024 >= (L + R) * B).padStart(9)} | ${String(f.ss).padStart(16)}  ${String(f.S).padStart(7)}  ${String(fits).padStart(8)}  ${String(f.mb).padStart(6)} ${f.mb <= maxWorkerOk ? "ok" : "not probed"}`);
  }
  if (chosen == null) {
    console.log("no B_wasm <= 2048 fits the --stack-size limit; use the Worker path or lower L (plan section 11)");
    return;
  }
  const f = fixed(chosen);
  const kc = Math.floor((chosen - C - 2 * CP) / 8);
  const pmax = Math.floor((chosen - C - 8) / CP);
  console.log(`\nB_wasm = ${chosen} bytes (small frame when 8 K + ${CP} P + ${C} <= ${chosen}: K <= ${kc} at P = 2; P <= ${pmax} even in the big-frame form); --stack-size = ${f.ss} KiB (${((100 * f.ss) / maxOk).toFixed(1)}% of the limit), S = RLIMIT_STACK = ${f.S} KiB`);

  // 5. Verification: frames of exactly B_wasm bytes (one parameter and (B_wasm - 64)/8
  //    locals) must reach L + R levels under the chosen settings in every tier, and so
  //    must the signature variants at the edge of the estimate (estimate = B_wasm). The
  //    plan's literal setting for B = 2048 is shown as the counterexample.
  console.log(`\n# verification: frames of ${chosen} bytes under ulimit -s ${f.S}, --stack-size=${f.ss} (need depth >= L+R = ${L + R})`);
  const kv = (chosen - 64) / 8;
  for (const [tier, flags] of tiers) {
    if (tier === "turbofan") continue;
    for (const kind of ["i64", "eqref"]) {
      const r = run(f.S, [`--stack-size=${f.ss}`, ...flags], ["--child", `${kind}:${kv}`]);
      console.log(`${tier.padEnd(9)} ${kind}:${kv}  ${outcome(r).padEnd(20)} ${r.depth != null && r.depth >= L + R ? "ok" : "NOT OK"}`);
    }
    for (const n of [0, 10, pmax - 2]) {
      const k = Math.floor((chosen - C - CP * (n + 2)) / 8);
      const v = `sig:${n}:${k}`;
      const r = run(f.S, [`--stack-size=${f.ss}`, ...flags], ["--child", v]);
      console.log(`${tier.padEnd(9)} ${v} (estimate ${estimate(n + 2, k)})  ${outcome(r).padEnd(20)} ${r.depth != null && r.depth >= L + R ? "ok" : "NOT OK"}`);
    }
  }
  const p2 = plan(2048), k2 = (2048 - 64) / 8;
  const r2 = run(p2.S, [`--stack-size=${p2.ss}`, "--liftoff-only"], ["--child", `i64:${k2}`]);
  console.log(`plan literal, B = 2048: ulimit -s ${p2.S}, --stack-size=${p2.ss}, liftoff i64:${k2}: ${outcome(r2)} (L = ${L}: ${r2.depth != null && r2.depth >= L ? "reached" : "NOT reached"})`);
}

if (!isMainThread) {
  parentPort.postMessage(probe(workerData.variant));
} else if (process.argv[2] === "--child") {
  console.log(JSON.stringify({ depth: probe(process.argv[3]) }));
} else if (process.argv[2] === "--worker") {
  const w = new Worker(SELF, { workerData: { variant: process.argv[4] }, resourceLimits: { stackSizeMb: Number(process.argv[3]) } });
  w.on("message", (d) => console.log(JSON.stringify({ depth: d })));
  w.on("error", (e) => console.log(JSON.stringify({ error: String(e) })));
} else {
  await main();
}
