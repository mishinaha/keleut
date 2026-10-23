// Finds the largest n for which the non-tail mutual recursion even_nt(n) still succeeds.
import { readFileSync } from "node:fs";
const { instance } = await WebAssembly.instantiate(readFileSync(new URL("./out/tail.wasm", import.meta.url)));
let lo = 1, hi = 10_000_000;
while (lo < hi) { const m = Math.ceil((lo + hi) / 2); try { instance.exports.even_nt(BigInt(m)); lo = m; } catch { hi = m - 1; } }
console.log(`max non-tail depth ~ ${lo}`);
