// Finds the largest n (up to 10,000,000) for which the non-tail mutual recursion
// even_nt(n) of ../wasm/out/tail.wasm still succeeds, by binary search.
// Build tail.wasm first: (cd doc/log/261024-1-spike/wasm && cargo run -q)
// Example (E12): (ulimit -s 2000000; node --stack-size=1500000 depth.mjs)
import { readFileSync } from "node:fs";
const url = new URL("../wasm/out/tail.wasm", import.meta.url);
let bytes;
try {
  bytes = readFileSync(url);
} catch (e) {
  console.error(`cannot read ${url.pathname}: ${e.code ?? e.message}`);
  console.error("build it with: (cd doc/log/261024-1-spike/wasm && cargo run -q)");
  process.exit(2);
}
const { instance } = await WebAssembly.instantiate(bytes);
let lo = 1, hi = 10_000_000;
while (lo < hi) {
  const m = Math.ceil((lo + hi) / 2);
  try {
    instance.exports.even_nt(BigInt(m));
    lo = m;
  } catch (e) {
    if (!(e instanceof RangeError)) throw e;
    hi = m - 1;
  }
}
console.log(`max non-tail depth ~ ${lo}`);
