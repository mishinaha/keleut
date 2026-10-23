import { readFileSync } from "node:fs";
const { instance: { exports: x } } = await WebAssembly.instantiate(readFileSync(new URL("./out/fx2.wasm", import.meta.url)));
for (const k of ["bind_test", "resume_throw_test", "switch_test"]) {
  try { console.log(`PASS ${k}: ${x[k]()}`); } catch (e) { console.log(`FAIL ${k}: ${e.constructor.name}: ${e.message}`); }
}
