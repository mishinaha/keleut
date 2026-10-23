// Runs out/*.wasm in Node without flags and prints one line per check.
import { readFileSync } from "node:fs";
import { writeSync } from "node:fs";

const dir = new URL("./out/", import.meta.url);
const load = (n) => readFileSync(new URL(`${n}.wasm`, dir));
const inst = async (n, imports = {}, opts) =>
  (await WebAssembly.instantiate(load(n), imports, opts)).instance.exports;

async function check(label, f) {
  try {
    const r = await f();
    console.log(`PASS ${label}: ${r}`);
  } catch (e) {
    console.log(`FAIL ${label}: ${e?.constructor?.name}: ${e?.message ?? e}`);
  }
}

console.log(`node ${process.version} v8 ${process.versions.v8}`);

// ---- GC ----
await check("gc compile", async () => (await inst("gc"), "instantiated"));
const gc = await inst("gc").catch(() => null);
if (gc) {
  await check("struct.new/get/set (expect 42)", () => gc.struct_test());
  await check("array i8 new/new_fixed/copy/get_u/get_s/len (expect 10000+255+104=10359)", () => gc.bytes_test());
  await check("i31 get_s(-5) / get_s(2^30) / get_u(-1)", () =>
    [gc.i31_test(-5), gc.i31_test(1 << 30), gc.i31_u(-1)].join(","));
  await check("ref.test classify (expect 1230)", () => gc.cast_test());
  await check("ref.cast ok (expect 42)", () => gc.cast_ok());
  await check("ref.cast failure traps", async () => {
    try { gc.cast_fail(); return "NO TRAP"; } catch (e) { return `${e.constructor.name}: ${e.message}`; }
  });
  await check("br_on_cast (expect 77,5)", () => [gc.br_on_cast_test(1), gc.br_on_cast_test(0)].join(","));
  await check("alloc 50M structs (GC runs)", () => {
    const t = performance.now();
    const r = gc.alloc_loop(50_000_000);
    return `${r} in ${(performance.now() - t).toFixed(0)} ms`;
  });
}

// ---- typed function references / tail calls ----
const tail = await inst("tail").catch((e) => (console.log(`FAIL tail compile: ${e.message}`), null));
if (tail) {
  for (const [name, f] of [["return_call even", tail.even], ["return_call_ref even", tail.even_ref]]) {
    await check(`${name}(10_000_000) mutual tail recursion`, () => {
      const t = performance.now();
      const r = f(10_000_000n);
      return `${r} in ${(performance.now() - t).toFixed(1)} ms`;
    });
    await check(`${name}(10_000_001)`, () => f(10_000_001n));
  }
  await check("call_ref via struct closure after even_ref set globals (odd(7)=1)", () => tail.call_ref_test(7n));
  for (const n of [100_000_000n, 1_000_000_000n]) for (const [name, f] of [["return_call", tail.even], ["return_call_ref", tail.even_ref]])
    await check(`${name} even(${n})`, () => { const t = performance.now(); const r = f(n); return `${r} in ${(performance.now() - t).toFixed(1)} ms`; });
  await check("non-tail call even_nt(10_000_000) (expect RangeError)", async () => {
    try { return `returned ${tail.even_nt(10_000_000n)}`; } catch (e) { return `${e.constructor.name}: ${e.message}`; }
  });
}

// ---- exception handling (exnref) ----
let lastJsError;
const ehImports = {
  env: {
    js_throw: () => { lastJsError = new TypeError("from JS"); throw lastJsError; },
    js_tag: WebAssembly.JSTag,
  },
};
await check("WebAssembly.JSTag exists", () => typeof WebAssembly.JSTag);
const eh = await inst("eh", ehImports).catch((e) => (console.log(`FAIL eh compile: ${e.message}`), null));
if (eh) {
  await check("try_table catch: no throw (5) / throw (1000+42)", () => [eh.catch_test(5), eh.catch_test(42)].join(","));
  await check("catch_ref + throw_ref (3000+50)", () => eh.throw_ref_test(50));
  await check("catch_all_ref of JS exception (1)", () => eh.catch_js());
  await check("catch JSTag gives the JS object", () => eh.catch_js_tag() === lastJsError);
  await check("throw_ref of JS exception reaches JS unchanged", () => {
    try { eh.rethrow_js(); return "no throw"; } catch (e) { return e === lastJsError; }
  });
  await check("wasm exception seen in JS as WebAssembly.Exception", () => {
    const ex = new WebAssembly.Exception(eh.e, [7]);
    return `${ex instanceof WebAssembly.Exception} is(e)=${ex.is(eh.e)} arg=${ex.getArg(eh.e, 0)}`;
  });
  await check("multi-value tag payload (5)", () => eh.throw_multi());
}

// ---- printing a byte string ----
let pr;
const prImports = {
  env: {
    write_bytes: (s) => {
      const n = pr.bytes_len(s);
      const buf = new Uint8Array(n);
      for (let i = 0; i < n; i++) buf[i] = pr.bytes_get(s, i);
      writeSync(1, buf);
    },
    write_mem: (p, n) => writeSync(1, new Uint8Array(pr.mem.buffer, p, n)),
  },
};
pr = await inst("print", prImports).catch((e) => (console.log(`FAIL print compile: ${e.message}`), null));
if (pr) {
  await check("print strategy A (GC array -> JS via accessors)", () => (pr.print_a(), "printed above"));
  await check("print strategy B (copy into linear memory)", () => (pr.print_b(), "printed above"));
  await check("JS sees GC array as opaque object", () => {
    let seen;
    const p2 = new WebAssembly.Instance(new WebAssembly.Module(load("print")), {
      env: { write_bytes: (s) => { seen = s; }, write_mem: () => {} },
    }).exports;
    p2.print_a();
    return `typeof=${typeof seen} keys=${JSON.stringify(Object.keys(seen))} [0]=${seen[0]} length=${seen.length}`;
  });
}

// ---- JS string builtins ----
await check("js-string builtins (fromCharCodeArray i16 array)", async () => {
  let logged;
  const ex = await inst("strings", { env: { log: (s) => { logged = s; } } }, { builtins: ["js-string"] });
  return `len=${ex.go()} str=${JSON.stringify(logged)}`;
});

// ---- JSPI ----
await check("JSPI API present", () =>
  `Suspending=${typeof WebAssembly.Suspending} promising=${typeof WebAssembly.promising}`);
await check("JSPI run (expect (21*2)+(100*2)=242)", async () => {
  const sleep_then = new WebAssembly.Suspending(async (x) => {
    await new Promise((r) => setTimeout(r, 10));
    return x * 2;
  });
  const ex = await inst("jspi", { env: { sleep_then } });
  const run = WebAssembly.promising(ex.run);
  const p = run(21);
  return `returns ${p instanceof Promise ? "Promise" : typeof p} -> ${await p}`;
});

// ---- wasm-encoder-built module ----
const enc = await inst("encoder").catch((e) => (console.log(`FAIL encoder compile: ${e.message}`), null));
if (enc) await check("wasm-encoder module enc_main(3)=8, enc_main(11)=1011", () => [enc.enc_main(3), enc.enc_main(11)].join(","));

// ---- stack switching without flag (expected to fail) ----
await check("fx.wasm compile without --experimental-wasm-wasmfx", async () => {
  await WebAssembly.compile(load("fx"));
  return "compiled";
});
