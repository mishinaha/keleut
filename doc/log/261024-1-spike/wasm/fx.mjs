// Stack switching (WasmFX) checks; run with: node --experimental-wasm-wasmfx fx.mjs
import { readFileSync } from "node:fs";
const load = (n) => readFileSync(new URL(`./out/${n}.wasm`, import.meta.url));
for (const n of ["fx", "fx_encoder"]) {
  try {
    const { instance } = await WebAssembly.instantiate(load(n));
    const t = performance.now();
    const small = instance.exports.sum(10);
    const big = instance.exports.sum(1_000_000);
    console.log(`PASS ${n}: sum(10)=${small} (expect 55), sum(1e6)=${big} (expect ${(1_000_000 * 1_000_001 / 2) | 0}) in ${(performance.now() - t).toFixed(1)} ms`);
  } catch (e) {
    console.log(`FAIL ${n}: ${e.constructor.name}: ${e.message}`);
  }
}
