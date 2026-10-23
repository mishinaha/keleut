import { readFileSync } from "node:fs";
const { instance: { exports: t } } = await WebAssembly.instantiate(readFileSync(new URL("./out/tail.wasm", import.meta.url)));
for (const [n, f] of [["return_call", t.even], ["return_call_ref", t.even_ref]]) { const s = performance.now(); const r = f(10_000_000n); console.log(`${n} even(1e7)=${r} ${(performance.now() - s).toFixed(1)} ms`); }
