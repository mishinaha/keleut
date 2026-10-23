import { readFileSync } from "node:fs";
const bytes = readFileSync(new URL("./out/fx3.wasm", import.meta.url));
try { WebAssembly.validate(bytes) ? console.log("validate: true") : console.log("validate: false"); const m = new WebAssembly.Module(bytes); console.log("compiled"); console.log(`run=${new WebAssembly.Instance(m).exports.run()}`); }
catch (e) { console.log(`FAIL ${e.constructor.name}: ${e.message}`); }
